/*
 * parsing/rule/impls/block/mod.rs
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

use crate::parsing::Parser;
use crate::parsing::result::ParseResult;
use crate::parsing::rule::{LineRequirement, Rule};
use crate::tree::Elements;
use std::fmt::{self, Debug};

mod argument_value;
mod arguments;
mod mapping;
mod parser;
mod rule;

pub mod blocks;

pub use self::arguments::Arguments;
pub(crate) use self::parser::BlockBodyStart;
pub use self::rule::{RULE_BLOCK, RULE_BLOCK_SKIP_NEWLINE, RULE_BLOCK_STAR};

#[derive(Clone)]
pub struct BlockRule {
    name: &'static str,

    accepts_names: &'static [&'static str],

    accepts_star: bool,

    accepts_score: bool,

    accepts_newlines_outside_body: bool,

    parse_fn: BlockParseFn,
}

impl BlockRule {
    #[cold]
    pub fn rule(&self) -> Rule {
        fn try_consume_fn<'r, 't>(
            _: &mut Parser<'r, 't>,
        ) -> ParseResult<'r, 't, Elements<'t>> {
            panic!("Pseudo rule for this block should not be executed directly!");
        }

        Rule {
            name: self.name,
            position: LineRequirement::Any,
            try_consume_fn,
        }
    }
}

impl Debug for BlockRule {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BlockRule")
            .field("name", &self.name)
            .field("accepts_names", &self.accepts_names)
            .field("accepts_star", &self.accepts_star)
            .field("accepts_score", &self.accepts_score)
            .field(
                "accepts_newlines_outside_body",
                &self.accepts_newlines_outside_body,
            )
            .field("parse_fn", &(self.parse_fn as *const ()))
            .finish()
    }
}

pub type BlockParseFn = for<'r, 't, 'n> fn(
    &mut Parser<'r, 't>,
    &'n str,
    usize,
    bool,
    bool,
    bool,
) -> ParseResult<'r, 't, Elements<'t>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn block_rule_metadata_helpers_cover_debug_and_rule() {
        let rule = blocks::BLOCK_DIV.rule();
        assert_eq!(rule.name(), "block-div");

        let debug = format!("{:?}", blocks::BLOCK_DIV);
        assert!(debug.contains("BlockRule"));
        assert!(debug.contains("block-div"));
        assert!(debug.contains("accepts_names"));
        assert!(debug.contains("accepts_star"));
        assert!(debug.contains("accepts_score"));
        assert!(debug.contains("accepts_newlines_outside_body"));
        assert!(debug.contains("parse_fn"));
    }

    #[test]
    #[should_panic(
        expected = "Pseudo rule for this block should not be executed directly"
    )]
    fn block_rule_pseudo_rule_panics_when_executed() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[div]]");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);

        let _ = blocks::BLOCK_DIV.rule().try_consume(&mut parser);
    }
}
