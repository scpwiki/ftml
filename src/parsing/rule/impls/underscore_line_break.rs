/*
 * parsing/rule/impls/underscore_line_break.rs
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
use std::num::NonZeroU32;

pub const RULE_UNDERSCORE_LINE_BREAK: Rule = Rule {
    name: "underscore-line-break",
    position: LineRequirement::Any,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Trying to parse underscore line break");

    if parser.settings().layout.legacy() && wikidot_heading_owns_underscore(parser) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    match parser.next_two_tokens() {
        (Token::Whitespace, Some(Token::Underscore)) => {
            parser.step()?;
        }
        (Token::Underscore, Some(_))
            if parser.start_of_line() && !parser.settings().layout.legacy() => {}
        _ => return Err(parser.make_err(ParseErrorKind::RuleFailed)),
    }

    let (current, next) = parser.next_two_tokens();
    let terminal = parser.settings().layout.legacy()
        && current == Token::Underscore
        && next == Some(Token::InputEnd);
    if terminal {
        parser.step()?;
        return ok!(Element::LineBreaks(NonZeroU32::new(2).unwrap()));
    }
    let paragraph_break =
        current == Token::Underscore && next == Some(Token::ParagraphBreak);
    let has_line_break = current == Token::Underscore
        && matches!(next, Some(Token::LineBreak | Token::ParagraphBreak));
    if !has_line_break {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    parser.step_n(2)?;

    if parser.settings().layout.legacy() && paragraph_break {
        ok!(Element::LineBreaks(NonZeroU32::new(2).unwrap()))
    } else {
        ok!(Element::LineBreak)
    }
}

fn wikidot_heading_owns_underscore(parser: &Parser<'_, '_>) -> bool {
    let source = parser.full_text().inner();
    let underscore = if parser.current().token == Token::Whitespace {
        parser
            .look_ahead(0)
            .filter(|token| token.token == Token::Underscore)
            .map(|token| token.span.start)
    } else if parser.current().token == Token::Underscore {
        Some(parser.current().span.start)
    } else {
        None
    };
    let Some(underscore) = underscore else {
        return false;
    };
    let line_start = source[..underscore]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let (_, prefix) =
        crate::preproc::quote_depth_and_body(&source[line_start..underscore]);
    let pluses = prefix.bytes().take_while(|byte| *byte == b'+').count();
    if pluses == 0 {
        return false;
    }
    let suffix = &prefix[pluses..];
    let suffix = suffix.strip_prefix('*').unwrap_or(suffix);
    suffix.starts_with([' ', '\t'])
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    fn render(source: &str) -> String {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{source:?}: {errors:#?}");
        HtmlRender.render(&tree, &page_info, &settings).body
    }

    #[test]
    fn wikidot_heading_keeps_a_trailing_underscore_literal() {
        assert_eq!(
            render("+ A _\nB"),
            "<h1 id=\"toc0\"><span>A _</span></h1><p>B</p>",
        );
    }

    #[test]
    fn wikidot_paragraph_break_after_underscore_emits_two_breaks() {
        assert_eq!(render("A _\n\nB"), "<p>A<br>\n<br>\nB</p>");
        assert_eq!(render("A _\nB"), "<p>A<br>\nB</p>");
    }
}
