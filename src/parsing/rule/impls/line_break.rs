/*
 * parsing/rule/impls/line_break.rs
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
use super::{
    RULE_BLOCKQUOTE, RULE_CENTER, RULE_CLEAR_FLOAT, RULE_HEADER, RULE_HORIZONTAL_RULE,
    RULE_LIST, RULE_SECTION_MARKER, RULE_TABLE,
};

pub const RULE_LINE_BREAK: Rule = Rule {
    name: "line-break",
    position: LineRequirement::Any,
    try_consume_fn: line_break,
};

pub const RULE_LINE_BREAK_PARAGRAPH: Rule = Rule {
    name: "line-break-paragraph",
    position: LineRequirement::Any,
    try_consume_fn: line_break_in_no_paragraph_context,
};

const START_OF_LINE_RULE_TOKENS: &[Token] = &[
    Token::Quote,
    Token::BulletItem,
    Token::NumberedItem,
    Token::Heading,
    Token::Equals,
    Token::TripleDash,
    Token::ClearFloatLeft,
    Token::ClearFloatRight,
    Token::ClearFloatBoth,
    Token::TableColumn,
    Token::TableColumnRight,
    Token::TableColumnCenter,
    Token::TableColumnTitle,
];

pub(crate) fn starts_own_line_rule(token: Token) -> bool {
    START_OF_LINE_RULE_TOKENS.contains(&token)
}

fn own_line_rules(token: Token) -> &'static [Rule] {
    match token {
        Token::Quote => &[RULE_BLOCKQUOTE],
        Token::BulletItem | Token::NumberedItem => &[RULE_LIST],
        Token::Heading => &[RULE_HEADER],
        Token::Equals => &[RULE_SECTION_MARKER, RULE_CENTER],
        Token::TripleDash => &[RULE_HORIZONTAL_RULE],
        Token::ClearFloatLeft | Token::ClearFloatRight | Token::ClearFloatBoth => {
            &[RULE_CLEAR_FLOAT]
        }
        Token::TableColumn
        | Token::TableColumnRight
        | Token::TableColumnCenter
        | Token::TableColumnTitle => &[RULE_TABLE],
        _ => &[],
    }
}

fn line_break<'r, 't>(parser: &mut Parser<'r, 't>) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Consuming newline token as line break");

    if line_break_skipped_for_own_line_block(parser) {
        if wikidot_injected_footnote_block_break(parser) {
            return ok!(Element::LineBreak);
        }
        debug!("Skipping line break element because of upcoming token");
        return ok!(Elements::None);
    }

    ok!(Element::LineBreak)
}

/// The preprocessor splits a footnoteblock authored on a definition-list or
/// table line onto its own line. Live's injection leaves one newline between
/// the footer and the fragment after the marker, and that newline stays a
/// break in front of the following line, unlike an ordinary block-starting
/// line.
fn wikidot_injected_footnote_block_break(parser: &Parser<'_, '_>) -> bool {
    if !parser.settings().layout.legacy() {
        return false;
    }
    let current = parser.current();
    if current.token != Token::LineBreak {
        return false;
    }
    let source = parser.full_text().inner();
    let newline_start = current.span.start;
    let previous_start = source[..newline_start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let previous_line =
        source[previous_start..newline_start].trim_end_matches([' ', '\t', '\r']);
    let lowercase = previous_line.to_ascii_lowercase();
    lowercase.starts_with("[[footnoteblock") && lowercase.ends_with("]]")
}

pub(crate) fn line_break_skipped_for_own_line_block<'r, 't>(
    parser: &Parser<'r, 't>,
) -> bool
where
    'r: 't,
{
    let quote_aware = parser.quote_body_cursor().is_some();
    parser.evaluate_fn(|parser| {
        parser.step()?;
        let padded = parser.current().token == Token::Whitespace;
        parser.get_optional_space()?;
        if quote_aware {
            parser.prepare_quote_body_line()?;
        }

        if parser.settings().layout.legacy()
            && padded
            && parser.current().token == Token::Equals
        {
            return Ok(false);
        }

        if quote_aware && !parser.start_of_line() {
            return Ok(false);
        }

        if parser.current().token == Token::Heading
            && parser.look_ahead(0).map(|token| token.token) != Some(Token::Whitespace)
        {
            return Ok(false);
        }

        // L85 scope-5: live keeps the break before a table in item bodies
        // (li-table-contained); the top-level path keeps it too.
        if parser.settings().layout.legacy()
            && parser.wikidot_li_inside_item_body()
            && matches!(
                parser.current().token,
                Token::TableColumn
                    | Token::TableColumnRight
                    | Token::TableColumnCenter
                    | Token::TableColumnTitle
            )
        {
            return Ok(false);
        }
        let rules = own_line_rules(parser.current().token);
        Ok(rules.iter().any(|rule| rule.try_consume(parser).is_ok()))
    })
}

#[derive(Clone, Copy)]
struct NoParagraphContext;

#[inline]
fn line_break_in_no_paragraph_context<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    line_break_with_context(parser, NoParagraphContext)
}

#[inline]
fn line_break_with_context<'r, 't>(
    parser: &mut Parser<'r, 't>,
    _: NoParagraphContext,
) -> ParseResult<'r, 't, Elements<'t>> {
    line_break(parser)
}
