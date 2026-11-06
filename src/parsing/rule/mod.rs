/*
 * parsing/rule/mod.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2026 Wikijump Team
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program. If not, see <http://www.gnu.org/licenses/>.
 */

use super::prelude::*;
use crate::parsing::Parser;
use std::fmt::{self, Debug};

mod mapping;

pub(crate) mod bibliography_order;

pub mod impls;

pub use self::mapping::get_rules_for_token;

#[derive(Copy, Clone)]
pub struct Rule {
    name: &'static str,

    position: LineRequirement,

    try_consume_fn: TryConsumeFn,
}

struct FailedAttemptMemoizationPolicy;

impl FailedAttemptMemoizationPolicy {
    const MIN_TOKENS_CONSUMED: usize = 16;

    fn memoizes(consumed_tokens: usize) -> bool {
        consumed_tokens >= Self::MIN_TOKENS_CONSUMED
    }
}

impl Rule {
    pub fn name(self) -> &'static str {
        self.name
    }

    #[inline]
    pub fn try_consume<'r, 't>(
        self,
        parser: &mut Parser<'r, 't>,
    ) -> ParseResult<'r, 't, Elements<'t>> {
        if let LineRequirement::StartOfLine = self.position
            && !parser.start_of_line()
        {
            return Err(parser.make_err(ParseErrorKind::NotStartOfLine));
        }

        let rule_start = parser.current().span.start;
        let parent_rule = parser.rule().name();
        if let Some(error) = parser.partial_rejection(self.name, parent_rule, rule_start)
        {
            return Err(error);
        }

        if let Some(error) = parser.rule_attempt_failure(self.name) {
            return Err(error);
        }

        let parser_state = parser.get_mutable_state();
        let mut sub_parser = parser.clone_with_rule(self);
        let enclosing_peak = parser.start_rule_attempt();
        let result = (self.try_consume_fn)(&mut sub_parser);
        let height = parser.finish_rule_attempt(enclosing_peak);
        if let Err(error) = &result
            && FailedAttemptMemoizationPolicy::memoizes(
                parser.remaining().len() - sub_parser.remaining().len(),
            )
        {
            parser.cache_rule_attempt_failure(self.name, error, height);
        }

        match result {
            Ok(ref output) => {
                if let Err(error) = output.check_partials(parser) {
                    if matches!(
                        error.kind(),
                        ParseErrorKind::ListItemOutsideList
                            | ParseErrorKind::TableRowOutsideTable
                            | ParseErrorKind::TableCellOutsideTable
                            | ParseErrorKind::TabOutsideTabView
                            | ParseErrorKind::RubyTextOutsideRuby
                    ) {
                        parser.cache_partial_rejection(
                            self.name,
                            parent_rule,
                            rule_start,
                            &error,
                        );
                    }
                    parser.reset_mutable_state(parser_state);
                    return Err(error);
                }

                parser.update(&sub_parser);
            }

            Err(_) => {
                if parser.discarding_hidden_body() && sub_parser.at_hidden_body_boundary()
                {
                    parser.update(&sub_parser);
                }
                parser.reset_mutable_state(parser_state);
            }
        }

        result
    }
}

impl Debug for Rule {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Rule")
            .field("name", &self.name)
            .field("position", &self.position)
            .field("try_consume_fn", &(self.try_consume_fn as *const ()))
            .finish()
    }
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub enum LineRequirement {
    Any,

    StartOfLine,
}

pub type TryConsumeFn = for<'p, 'r, 't> fn(
    parser: &'p mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>>;

#[cfg(test)]
mod tests {
    use super::impls::{RULE_HEADER, RULE_TEXT};
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::{ParseErrorKind, Parser};
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::Elements;

    #[test]
    fn rule_metadata_and_line_requirements_are_exercised() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        assert_eq!(RULE_TEXT.name(), "text");
        let debug = format!("{RULE_TEXT:?}");
        assert!(debug.contains("Rule"));
        assert!(debug.contains("text"));

        let text_tokens = crate::tokenize("alpha");
        let mut text_parser = Parser::new(&text_tokens, &page_info, &settings);
        text_parser
            .step()
            .expect("identifier should follow input start");
        let text = RULE_TEXT
            .try_consume(&mut text_parser)
            .expect("text rule should accept any line position");
        assert_eq!(text.item, Elements::Single(text!("alpha")));

        let header_tokens = crate::tokenize("alpha + heading");
        let mut header_parser = Parser::new(&header_tokens, &page_info, &settings);
        header_parser
            .step()
            .expect("identifier should follow input start");
        header_parser
            .step()
            .expect("space should follow identifier");
        header_parser.step().expect("heading should follow space");
        let error = RULE_HEADER
            .try_consume(&mut header_parser)
            .expect_err("header rule should require start of line");
        assert_eq!(error.kind(), ParseErrorKind::NotStartOfLine);
    }
}
