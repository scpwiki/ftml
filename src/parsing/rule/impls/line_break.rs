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
    try_consume_fn: line_break,
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


fn fuse_own_line_kind(token: Token) -> bool {
    matches!(token, Token::BulletItem | Token::NumberedItem)
}

pub(crate) fn try_fuse_own_line_consume<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Option<ParseResult<'r, 't, Elements<'t>>>
where
    'r: 't,
{
    if parser.quote_body_cursor().is_some()
        || parser.in_wikidot_bibliography_body()
        || parser.discarding_hidden_body()
    {
        return None;
    }
    if parser.current().token == Token::LineBreak
        && wikidot_injected_footnote_block_break(parser)
    {
        return None;
    }
    let snapshot = parser.clone();
    let registries = parser.snapshot_shared_registries();
    let result = try_fuse_own_line_attempt(parser);
    if result.is_none() || parser.shared_registries_differ(&registries) {
        parser.restore_shared_registries(registries);
        *parser = snapshot;
        return None;
    }
    result
}

fn try_fuse_own_line_attempt<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Option<ParseResult<'r, 't, Elements<'t>>>
where
    'r: 't,
{
    if parser.step().is_err() {
        return None;
    }
    let token = parser.current().token;
    if token == Token::Whitespace || !fuse_own_line_kind(token) {
        return None;
    }
    if let Some(barrier) = parser.wikidot_heading_barrier()
        && parser.current().span.start >= barrier
    {
        return None;
    }
    if parser.wikidot_iftags_body_end().is_some() {
        return None;
    }
    if parser.at_body_end_boundary() || parser.at_hard_body_end_boundary() {
        return None;
    }
    let rules = own_line_rules(token);
    if rules.is_empty() {
        return None;
    }
    for rule in rules {
        let footnote_count = parser.footnote_count();
        let attempt_start = parser.remaining();
        match rule.try_consume(parser) {
            Ok(success) => {
                if parser.same_pointer(attempt_start) && parser.step().is_err() {
                    return None;
                }
                return Some(Ok(success));
            }
            Err(_) => {
                parser.truncate_footnotes(footnote_count);
            }
        }
    }
    None
}

fn line_break<'r, 't>(parser: &mut Parser<'r, 't>) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Consuming newline token as line break");

    if let Some(result) = try_fuse_own_line_consume(parser) {
        return result;
    }

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
