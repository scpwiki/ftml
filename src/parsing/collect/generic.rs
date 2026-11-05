/*
 * parsing/collect/generic.rs
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

use super::consume_valid_comment;
use super::iftags::{SegmentCtx, SplitDelimiterMatch, TokenWindow, split_delimiter_end};
use super::prelude::*;
use crate::parsing::parser::QuoteBodyLineStatus;
use crate::parsing::rule::impls::line_break_skipped_for_own_line_block;

pub fn collect<'p, 'r, 't, F>(
    parser: &'p mut Parser<'r, 't>,
    _rule: Rule,
    close_conditions: &[ParseCondition],
    invalid_conditions: &[ParseCondition],
    error_kind: Option<ParseErrorKind>,
    process: F,
) -> ParseResult<'r, 't, &'r ExtractedToken<'t>>
where
    F: FnMut(&mut Parser<'r, 't>) -> ParseResult<'r, 't, ()>,
{
    collect_with_terminator(
        parser,
        _rule,
        close_conditions,
        invalid_conditions,
        error_kind,
        true,
        process,
    )
}

pub(super) fn collect_before<'p, 'r, 't, F>(
    parser: &'p mut Parser<'r, 't>,
    rule: Rule,
    close_conditions: &[ParseCondition],
    invalid_conditions: &[ParseCondition],
    error_kind: Option<ParseErrorKind>,
    process: F,
) -> ParseResult<'r, 't, &'r ExtractedToken<'t>>
where
    F: FnMut(&mut Parser<'r, 't>) -> ParseResult<'r, 't, ()>,
{
    collect_with_terminator(
        parser,
        rule,
        close_conditions,
        invalid_conditions,
        error_kind,
        false,
        process,
    )
}

fn collect_with_terminator<'p, 'r, 't, F>(
    parser: &'p mut Parser<'r, 't>,
    _rule: Rule,
    close_conditions: &[ParseCondition],
    invalid_conditions: &[ParseCondition],
    error_kind: Option<ParseErrorKind>,
    consume_terminator: bool,
    mut process: F,
) -> ParseResult<'r, 't, &'r ExtractedToken<'t>>
where
    F: FnMut(&mut Parser<'r, 't>) -> ParseResult<'r, 't, ()>,
{
    let mut errors = Vec::new();
    let mut paragraph_safe = true;

    loop {
        if parser.current().token == Token::ParagraphBreak
            && parser.evaluate_any(close_conditions)
        {
            let last = parser.current();
            if consume_terminator {
                parser.step()?;
            }
            return ok!(paragraph_safe; last, errors);
        }

        if parser.prepare_quote_body_line()? == QuoteBodyLineStatus::Boundary {
            return Err(parser.make_err(ParseErrorKind::EndOfInput));
        }

        if let Some(barrier) = parser.wikidot_heading_barrier()
            && parser.current().span.start >= barrier
        {
            if _rule.name() == "header" {
                return ok!(paragraph_safe; parser.current(), errors);
            }
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }

        if parser.evaluate_any(close_conditions) {
            if consume_terminator
                && let Some(split_match) = split_close_match(parser, close_conditions)
            {
                let leftover = split_match
                    .leftover
                    .map(|range| parser.full_text().inner()[range].to_owned());
                let mut last = parser.current();
                while parser.current().span.start < split_match.consume_end
                    && parser.current().token != Token::InputEnd
                {
                    last = parser.current();
                    parser.step()?;
                }
                let mut success = ParseSuccess::new(last, errors, paragraph_safe);
                success.split_close_leftover = leftover;
                return Ok(success);
            }
            let last = parser.current();
            if consume_terminator
                && !matches!(
                    parser.current().token,
                    Token::InputEnd | Token::BulletItem | Token::NumberedItem,
                )
            {
                parser.step()?;
            }

            return ok!(paragraph_safe; last, errors);
        }

        if !parser.discarding_hidden_body() {
            let at_boundary = match _rule.name() {
                "header" => parser.at_hard_body_end_boundary(),
                "list" | "definition-list" => parser.at_body_end_boundary(),
                _ => false,
            };
            if at_boundary {
                return ok!(paragraph_safe; parser.current(), errors);
            }
        }

        if parser.in_wikidot_simple_table_cell()
            && is_table_column_token(parser.current().token)
            && let Some(closer) = simple_table_inline_closer(_rule)
            && simple_table_inline_closer_follows(parser, closer)
        {
            parser.mark_wikidot_simple_table_crossed_closer(closer);
            return ok!(paragraph_safe; parser.current(), errors);
        }

        if parser.in_wikidot_simple_table_cell()
            && is_table_column_token(parser.current().token)
            && let Some(delimiter) = simple_table_split_delimiter(_rule)
            && simple_table_split_closer_follows(parser, delimiter)
        {
            parser.mark_wikidot_simple_table_crossed_split_closer(delimiter);
            return ok!(paragraph_safe; parser.current(), errors);
        }

        if parser.in_wikidot_simple_table_cell()
            && parser.current().token == Token::LineBreak
        {
            return Err(parser.make_err(error_kind.unwrap_or(ParseErrorKind::RuleFailed)));
        }

        if parser.in_wikidot_definition_list_line()
            && parser.current().token == Token::LineBreak
            && simple_table_inline_closer(_rule).is_some()
        {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }

        if parser.in_wikidot_single_line_content()
            && parser.current().token == Token::LineBreak
            && simple_table_inline_closer(_rule).is_some()
        {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }

        if parser.settings().layout.legacy()
            && simple_table_inline_closer(_rule).is_some()
            && parser.current().token == Token::LineBreak
            && line_break_skipped_for_own_line_block(parser)
        {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }

        if parser.evaluate_any(invalid_conditions) {
            return Err(parser.make_err(error_kind.unwrap_or(ParseErrorKind::RuleFailed)));
        }

        if parser.current().token == Token::InputEnd {
            return Err(parser.make_err(ParseErrorKind::EndOfInput));
        }

        let old_remaining = parser.remaining();
        process(parser)?.chain(&mut errors, &mut paragraph_safe);

        if parser.same_pointer(old_remaining) {
            parser.step()?;
        }
    }
}

fn is_table_column_token(token: Token) -> bool {
    matches!(
        token,
        Token::TableColumn
            | Token::TableColumnTitle
            | Token::TableColumnCenter
            | Token::TableColumnRight
    )
}

fn simple_table_inline_closer(rule: Rule) -> Option<Token> {
    match rule.name() {
        "bold" => Some(Token::Bold),
        "italics" => Some(Token::Italics),
        "strikethrough-dash" => Some(Token::DoubleDash),
        "underline" => Some(Token::Underline),
        "superscript" => Some(Token::Superscript),
        "subscript" => Some(Token::Subscript),
        "monospace" => Some(Token::RightMonospace),
        _ => None,
    }
}

fn simple_table_inline_closer_follows<'r, 't>(
    parser: &Parser<'r, 't>,
    closer: Token,
) -> bool
where
    'r: 't,
{
    let mut scan = parser.clone();
    if scan.step().is_err() {
        return false;
    }

    let mut raw = false;
    let mut alternate_raw = false;
    let mut triple_link_depth = 0usize;
    loop {
        if matches!(
            scan.current().token,
            Token::ParagraphBreak | Token::InputEnd
        ) {
            return false;
        }

        if scan.current().token == Token::LineBreak {
            let mut next = scan.clone();
            if next.step().is_err() || !is_table_column_token(next.current().token) {
                return false;
            }
        }

        if scan.current().token == Token::LeftComment {
            let mut comment = scan.clone();
            if let Ok(range) = consume_valid_comment(&mut comment) {
                let source = &scan.full_text().inner()[range];
                if source.contains('\n') || source.contains('\r') {
                    return false;
                }
                scan.update(&comment);
                continue;
            }
        }

        match scan.current().token {
            Token::Raw => raw = !raw,
            Token::LeftRaw if !raw => alternate_raw = true,
            Token::RightRaw if alternate_raw => alternate_raw = false,
            Token::LeftLink | Token::LeftLinkStar if !raw && !alternate_raw => {
                triple_link_depth += 1;
            }
            Token::RightLink if triple_link_depth > 0 => {
                triple_link_depth -= 1;
            }
            token
                if !raw
                    && !alternate_raw
                    && triple_link_depth == 0
                    && token == closer =>
            {
                return true;
            }
            _ => {}
        }

        if scan.step().is_err() {
            return false;
        }
    }
}

fn split_close_match<'r, 't>(
    parser: &Parser<'r, 't>,
    close_conditions: &[ParseCondition],
) -> Option<SplitDelimiterMatch> {
    close_conditions
        .iter()
        .find_map(|condition| match condition {
            ParseCondition::SplitClose(delimiter) => split_delimiter_end(
                TokenWindow::at(parser),
                SegmentCtx::from_parser(parser),
                *delimiter,
            ),
            _ => None,
        })
}

fn simple_table_split_delimiter(rule: Rule) -> Option<SplitDelimiter> {
    match rule.name() {
        "bold" => Some(SplitDelimiter::Bold),
        "italics" => Some(SplitDelimiter::Italics),
        _ => None,
    }
}

fn simple_table_split_closer_follows<'r, 't>(
    parser: &Parser<'r, 't>,
    delimiter: SplitDelimiter,
) -> bool
where
    'r: 't,
{
    let ctx = SegmentCtx::from_parser(parser);
    let mut scan = parser.clone();
    if scan.step().is_err() {
        return false;
    }

    let mut raw = false;
    let mut alternate_raw = false;
    let mut triple_link_depth = 0usize;
    loop {
        if matches!(
            scan.current().token,
            Token::ParagraphBreak | Token::InputEnd
        ) {
            return false;
        }

        if scan.current().token == Token::LineBreak {
            let mut next = scan.clone();
            if next.step().is_err() || !is_table_column_token(next.current().token) {
                return false;
            }
        }

        if scan.current().token == Token::LeftComment {
            let mut comment = scan.clone();
            if let Ok(range) = consume_valid_comment(&mut comment) {
                let source = &scan.full_text().inner()[range];
                if source.contains('\n') || source.contains('\r') {
                    return false;
                }
                scan.update(&comment);
                continue;
            }
        }

        if !raw
            && !alternate_raw
            && triple_link_depth == 0
            && delimiter.is_half(scan.current())
            && scan.look_ahead(0).is_some_and(|token| {
                matches!(token.token, Token::LeftBlock | Token::LeftComment)
            })
            && split_delimiter_end(TokenWindow::at(&scan), ctx, delimiter).is_some()
        {
            return true;
        }

        match scan.current().token {
            Token::Raw => raw = !raw,
            Token::LeftRaw if !raw => alternate_raw = true,
            Token::RightRaw if alternate_raw => alternate_raw = false,
            Token::LeftLink | Token::LeftLinkStar if !raw && !alternate_raw => {
                triple_link_depth += 1;
            }
            Token::RightLink if triple_link_depth > 0 => {
                triple_link_depth -= 1;
            }
            _ => {}
        }

        if scan.step().is_err() {
            return false;
        }
    }
}
