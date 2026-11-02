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
use super::prelude::*;
use crate::parsing::parser::QuoteBodyLineStatus;
use crate::parsing::rule::impls::line_break_skipped_for_own_line_block;

/// Generic function to parse upcoming tokens until conditions are met.
///
/// Each handled token can then processed in some manner, in accordance
/// to the passed closure.
///
/// The conditions for how to consume tokens are passed as arguments,
/// which are explained below.
///
/// Mutable parser reference:
/// * `parser`
///
/// The rule we're parsing for:
/// * `rule`
///
/// The conditions we should end iteration on:
/// If one of these is true, we will return success.
/// * `close_conditions`
///
/// The conditions we should abort on:
/// If one of these is true, we will return failure.
/// * `invalid_conditions`
///
/// If one of the failures is activated, then this `ParseErrorKind`
/// will be returned. If `None` is provided, then `ParseErrorKind::RuleFailed` is used.
/// * `error_kind`
///
/// The closure we should execute each time a token extraction is reached:
/// If the return value is `Err(_)` then collection is aborted and that error
/// is bubbled up.
/// * `process`
///
/// This will proceed until a closing condition is found, an abort is found,
/// or the end of the input is reached.
///
/// It is up to the caller to save whatever result they need while running
/// in the closure.
///
/// The final token from the collection, one prior to the now-current token,
/// is returned.
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
        // A paragraph-break token can terminate the current quoted physical
        // line and simultaneously represent the unquoted blank line after
        // it. Let a child container that explicitly accepts that token close
        // before the quote-body adapter reports the outer run boundary.
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

        if parser.evaluate_any(close_conditions) {
            let last = parser.current();
            if consume_terminator && parser.current().token != Token::InputEnd {
                parser.step()?;
            }

            return ok!(paragraph_safe; last, errors);
        }

        // A simple-table cell delimiter outranks ordinary inline formatting
        // in Wikidot layout. Commit the live part of the inline owner, leave
        // the table token for the row parser, and remember only the matching
        // authored closer so it cannot leak into a later cell.
        if parser.in_wikidot_simple_table_cell()
            && is_table_column_token(parser.current().token)
            && let Some(closer) = simple_table_inline_closer(_rule)
            && simple_table_inline_closer_follows(parser, closer)
        {
            parser.mark_wikidot_simple_table_crossed_closer(closer);
            return ok!(paragraph_safe; parser.current(), errors);
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
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
        ) {
            return false;
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
