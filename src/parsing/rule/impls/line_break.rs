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
        debug!("Skipping line break element because of upcoming token");
        return ok!(Elements::None);
    }

    ok!(Element::LineBreak)
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
            // Wikidot does not grant content-separator ownership through
            // leading SP/TAB. Keep the preceding physical line break instead
            // of treating the padded equals run like an own-line block.
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
