/*
 * parsing/consume.rs
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

use super::Parser;
use super::parser::QuoteBodyLineStatus;
use super::prelude::*;
use super::rule::{
    get_rules_for_token,
    impls::{
        RULE_FALLBACK, glued_scheme_url_start, starts_own_line_rule,
        starts_wikidot_only_scheme_url, url_elements,
    },
};
use crate::tree::{LinkLabel, LinkLocation, LinkType, PartialElement};
use std::{borrow::Cow, mem};

fn try_consume_wikidot_parser_function_anchor_fallback<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<Elements<'t>>, ParseError>
where
    'r: 't,
{
    if !parser.settings().layout.legacy()
        || parser.current().token != Token::LeftBlockAnchor
    {
        return Ok(None);
    }

    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let target_start = start + "[[#".len();
    let bytes = source.as_bytes();
    let mut target_end = target_start;
    while bytes.get(target_end).is_some_and(u8::is_ascii_alphanumeric) {
        target_end += 1;
    }
    if target_end == target_start
        || !bytes
            .get(target_end)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        return Ok(None);
    }

    let mut label_start = target_end;
    while bytes
        .get(label_start)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        label_start += 1;
    }
    let line_end = source[label_start..]
        .find(['\r', '\n'])
        .map_or(source.len(), |offset| label_start + offset);
    let Some(relative_close) = source[label_start..line_end].find(']') else {
        return Ok(None);
    };
    let close = label_start + relative_close;
    let raw_label = &source[label_start..close];
    let trimmed = raw_label.trim_matches([' ', '\t']);
    if trimmed.is_empty() {
        return Ok(None);
    }
    let trailing_start = raw_label.trim_end_matches([' ', '\t']).len();
    let trailing = &raw_label[trailing_start..];

    if std::iter::once(parser.current())
        .chain(parser.remaining())
        .take_while(|token| token.span.start <= close)
        .any(|token| {
            matches!(
                token.token,
                Token::RuntimeText | Token::GeneratedPageLink | Token::GeneratedTagLinks
            )
        })
    {
        return Ok(None);
    }

    while parser.current().span.end <= close {
        parser.step()?;
    }
    let closer = parser.current();
    if closer.span.start != close || !closer.slice.starts_with(']') {
        return Ok(None);
    }
    let residual_closer = &closer.slice[1..];
    parser.step()?;

    let target = &source[target_start..target_end];
    let mut elements = vec![
        text!("["),
        Element::Link {
            ltype: LinkType::Anchor,
            link: LinkLocation::Url(Cow::Owned(format!("#{target}"))),
            label: LinkLabel::Text(Cow::Borrowed(trimmed)),
            target: None,
        },
    ];
    if !trailing.is_empty() {
        elements.push(text!(trailing));
    }
    if !residual_closer.is_empty() {
        elements.push(text!(residual_closer));
    }
    Ok(Some(Elements::Multiple(elements)))
}

fn try_consume_wikidot_unmatched_inline_close_link<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<Elements<'t>>, ParseError>
where
    'r: 't,
{
    if !parser.settings().layout.legacy() || parser.current().token != Token::LeftBlockEnd
    {
        return Ok(None);
    }

    if parser.in_wikidot_span_body() {
        return Ok(None);
    }

    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let Some(relative_marker_end) = source[start..].find("]]") else {
        return Ok(None);
    };
    let marker_end = start + relative_marker_end + 2;
    let name = source[start + "[[/".len()..start + relative_marker_end]
        .trim_matches([' ', '\t']);
    if !matches!(
        name.to_ascii_lowercase().as_str(),
        "div" | "span" | "footnote"
    ) {
        return Ok(None);
    }
    if line_has_unmatched_wikidot_owner_opener(source, start, name) {
        return Ok(None);
    }
    if (name.eq_ignore_ascii_case("span") && parser.rule().name() == "block-span")
        || (name.eq_ignore_ascii_case("div") && parser.rule().name() == "block-div")
        || (name.eq_ignore_ascii_case("footnote")
            && parser.rule().name() == "block-footnote")
    {
        return Ok(None);
    }
    if !source
        .as_bytes()
        .get(marker_end)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        return Ok(None);
    }

    let mut label_start = marker_end;
    while source
        .as_bytes()
        .get(label_start)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        label_start += 1;
    }
    let line_end = source[label_start..]
        .find(['\r', '\n'])
        .map_or(source.len(), |offset| label_start + offset);
    let Some(relative_close) = source[label_start..line_end].find(']') else {
        return Ok(None);
    };
    let close = label_start + relative_close;
    let raw_label = &source[label_start..close];
    if wikidot_label_region_has_owner_opener(&source[label_start..close]) {
        return Ok(None);
    }
    let label = raw_label.trim_matches([' ', '\t']);
    if label.is_empty() {
        return Ok(None);
    }
    let trailing_start = raw_label.trim_end_matches([' ', '\t']).len();
    let trailing = &raw_label[trailing_start..];

    if std::iter::once(parser.current())
        .chain(parser.remaining())
        .take_while(|token| token.span.start <= close)
        .any(|token| {
            matches!(
                token.token,
                Token::RuntimeText | Token::GeneratedPageLink | Token::GeneratedTagLinks
            )
        })
    {
        return Ok(None);
    }

    while parser.current().span.end <= close {
        parser.step()?;
    }
    let closer = parser.current();
    if closer.span.start != close || !closer.slice.starts_with(']') {
        return Ok(None);
    }
    let residual_closer = &closer.slice[1..];
    parser.step()?;

    let target = &source[start + 2..marker_end];
    let mut elements = vec![
        text!("["),
        Element::Link {
            ltype: LinkType::Direct,
            link: LinkLocation::Url(Cow::Borrowed(target)),
            label: LinkLabel::Text(Cow::Borrowed(label)),
            target: None,
        },
    ];
    if !trailing.is_empty() {
        elements.push(text!(trailing));
    }
    if !residual_closer.is_empty() {
        elements.push(text!(residual_closer));
    }
    Ok(Some(Elements::Multiple(elements)))
}

fn wikidot_label_region_has_owner_opener(region: &str) -> bool {
    let bytes = region.as_bytes();
    let mut offset = 0;
    while let Some(found) = region[offset..].find("[[") {
        let name_start = offset + found + 2;
        let name_end = bytes[name_start..]
            .iter()
            .position(|byte| !(byte.is_ascii_alphanumeric() || *byte == b'_'))
            .map_or(bytes.len(), |pos| name_start + pos);
        let name = &region[name_start..name_end];
        let name = name.strip_suffix('_').unwrap_or(name);
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "footnote" | "span" | "size" | "a"
        ) {
            return true;
        }
        offset = name_start;
    }
    false
}

fn line_has_unmatched_wikidot_owner_opener(
    source: &str,
    close_start: usize,
    name: &str,
) -> bool {
    let line_start = source[..close_start]
        .rfind(['\r', '\n'])
        .map_or(0, |newline| newline + 1);
    let prefix = &source[line_start..close_start];
    let open = format!("[[{name}");
    let close = format!("[[/{name}");
    let last_open = prefix
        .as_bytes()
        .windows(open.len())
        .enumerate()
        .filter(|(index, window)| {
            window.eq_ignore_ascii_case(open.as_bytes())
                && prefix
                    .as_bytes()
                    .get(index + open.len())
                    .is_some_and(|byte| matches!(byte, b']' | b' ' | b'\t' | b'_'))
        })
        .map(|(index, _)| index)
        .next_back();
    let last_close = prefix
        .as_bytes()
        .windows(close.len())
        .enumerate()
        .filter(|(index, window)| {
            window.eq_ignore_ascii_case(close.as_bytes())
                && prefix
                    .as_bytes()
                    .get(index + close.len())
                    .is_some_and(|byte| matches!(byte, b']' | b' ' | b'\t' | b'_'))
        })
        .map(|(index, _)| index)
        .next_back();
    last_open.is_some_and(|open| last_close.is_none_or(|close| open > close))
}

fn try_consume_structural_close<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<Elements<'t>>, ParseError>
where
    'r: 't,
{
    if parser.settings().layout.legacy()
        && parser.pending_wikidot_collapsible_closer()
        && parser.current().token == Token::LeftBlockEnd
    {
        let unquoted_close = parser.native_blockquote_depth().is_none();
        let mut close = parser.clone();
        if close
            .get_end_block()
            .is_ok_and(|name| name.eq_ignore_ascii_case("collapsible"))
        {
            parser.update(&close);
            parser.set_pending_wikidot_collapsible_closer(false);
            return Ok(Some(if unquoted_close {
                Element::LineBreak.into()
            } else {
                Elements::None
            }));
        }
    }

    if parser.settings().layout.legacy()
        && !parser.discarding_hidden_body()
        && parser.has_wikidot_crossed_close()
        && parser.current().token == Token::LeftBlockEnd
    {
        let mut close = parser.clone();
        if let Ok(name) = close.get_end_block() {
            let name = name.strip_suffix('_').unwrap_or(name);
            if parser.take_wikidot_crossed_close(name) {
                parser.update(&close);
                return Ok(Some(Elements::None));
            }
        }
    }

    if !parser.settings().layout.legacy() || parser.current().token != Token::LeftBlockEnd
    {
        return Ok(None);
    }

    let mut close = parser.clone();
    let Ok((name, residual_close_bracket)) = close.get_wikidot_end_block_with_residual()
    else {
        return Ok(None);
    };
    let normalized = name.strip_suffix('_').unwrap_or(name);
    let scored_close = normalized.len() != name.len();
    let start = parser.current().span.start;
    let end = close.current().span.start;
    let close_source = &parser.full_text().inner()[start..end];
    if let Some(label) = wikidot_leading_space_close_label(close_source) {
        parser.update(&close);
        return Ok(Some(Elements::Multiple(vec![
            text!("["),
            Element::Link {
                ltype: LinkType::Direct,
                link: LinkLocation::Url(cow!("/")),
                label: LinkLabel::Text(cow!(label)),
                target: None,
            },
            text!("]"),
        ])));
    }
    let element = if scored_close && normalized.eq_ignore_ascii_case("span") {
        text!(close_source)
    } else if normalized.eq_ignore_ascii_case("size") {
        Element::Partial(PartialElement::InlineSizeClose(cow!(close_source)))
    } else if normalized.eq_ignore_ascii_case("span") {
        Element::Partial(PartialElement::InlineSpanClose(cow!(close_source)))
    } else if !parser.discarding_hidden_body()
        && parser.rule().name() == "block-table-row"
        && (normalized.eq_ignore_ascii_case("cell")
            || normalized.eq_ignore_ascii_case("hcell"))
    {
        text!(close_source)
    } else {
        return Ok(None);
    };

    let closes_scored_span = !scored_close
        && normalized.eq_ignore_ascii_case("span")
        && parser.wikidot_span_alias_close_is_scored(start);
    parser.update(&close);
    if !scored_close && normalized.eq_ignore_ascii_case("span") {
        parser.leave_wikidot_span_body(closes_scored_span);
    }
    Ok(Some(if residual_close_bracket {
        Elements::Multiple(vec![element, text!("]")])
    } else {
        element.into()
    }))
}

fn try_consume_wikidot_adjacent_unmatched_closes_as_link<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<Elements<'t>>, ParseError>
where
    'r: 't,
{
    if !parser.settings().layout.legacy() || parser.current().token != Token::LeftBlockEnd
    {
        return Ok(None);
    }

    if parser.in_wikidot_span_body() {
        return Ok(None);
    }

    let source = parser.full_text().inner();
    if parser
        .look_ahead(0)
        .is_some_and(|token| token.token == Token::Whitespace)
    {
        let Some(label_start) = parser.look_ahead(1).map(|token| token.span.start) else {
            return Ok(None);
        };
        let mut offset = 1usize;
        let right_block_start = loop {
            let Some(token) = parser.look_ahead(offset) else {
                return Ok(None);
            };
            match token.token {
                Token::RightBlock => break token.span.start,
                Token::LeftBlock | Token::LeftBlockEnd => return Ok(None),
                Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                    return Ok(None);
                }
                _ => offset += 1,
            }
        };
        let label = source[label_start..right_block_start].trim();
        if label.is_empty() {
            return Ok(None);
        }
        parser.step_n(offset + 2)?;
        return Ok(Some(Elements::Multiple(vec![
            text!("["),
            Element::Link {
                ltype: LinkType::Direct,
                link: LinkLocation::Url(cow!("/")),
                label: LinkLabel::Text(cow!(label)),
                target: None,
            },
            text!("]"),
        ])));
    }

    let first_start = parser.current().span.start;
    let mut scan = parser.clone();
    if scan.get_end_block().is_err() || scan.current().token != Token::Whitespace {
        return Ok(None);
    }
    let first_end = scan.current().span.start;
    scan.step()?;
    while !matches!(
        scan.current().token,
        Token::LeftBlock
            | Token::LeftBlockEnd
            | Token::LineBreak
            | Token::ParagraphBreak
            | Token::InputEnd
    ) {
        scan.step()?;
    }
    if !matches!(scan.current().token, Token::LeftBlock | Token::LeftBlockEnd) {
        return Ok(None);
    }
    let second_start = scan.current().span.start;
    let second_is_close = scan.current().token == Token::LeftBlockEnd;
    let mut second_opener_name = None;
    let second_valid = if second_is_close {
        scan.get_end_block().is_ok()
    } else {
        match scan.get_block_name(false) {
            Ok((name, _)) => {
                second_opener_name = Some(name);
                true
            }
            Err(_) => false,
        }
    };
    if let Some(name) = second_opener_name {
        let name = name.strip_suffix('_').unwrap_or(name);
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "footnote" | "span" | "size" | "a"
        ) {
            return Ok(None);
        }
    }
    if !second_valid
        || second_is_close
            && !matches!(
                scan.current().token,
                Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
            )
    {
        return Ok(None);
    }
    let second_end = scan.current().span.start;
    let first = &source[first_start..first_end];
    let second = &source[second_start..second_end];
    if !first.starts_with("[[/")
        || !first.ends_with("]]")
        || !(second.starts_with("[[/") || second.starts_with("[["))
        || !second.ends_with("]]")
    {
        return Ok(None);
    }

    let url = &first[2..];
    let label = source[first_end..second_end - 2].trim();
    parser.update(&scan);
    Ok(Some(Elements::Multiple(vec![
        text!("["),
        Element::Link {
            ltype: LinkType::Direct,
            link: LinkLocation::Url(cow!(url)),
            label: LinkLabel::Text(cow!(label)),
            target: None,
        },
        text!("]"),
    ])))
}

fn wikidot_leading_space_close_label(source: &str) -> Option<&str> {
    let inner = source.strip_prefix("[[/")?.strip_suffix("]]")?;
    let label = inner.strip_prefix([' ', '\t'])?;
    let label = label.trim_start_matches([' ', '\t']);
    if label.is_empty()
        || label.chars().any(char::is_whitespace)
        || !matches!(label.to_ascii_lowercase().as_str(), "div" | "span")
    {
        return None;
    }
    Some(label)
}

fn can_consume_as_text_token<'r, 't>(parser: &Parser<'r, 't>) -> bool {
    match parser.current().token {
        Token::Identifier => {
            !(parser.settings().layout.legacy()
                && (starts_wikidot_only_scheme_url(
                    parser.full_text().inner(),
                    parser.current().span.start,
                ) || glued_scheme_url_start(
                    parser.full_text().inner(),
                    parser.current().span.start,
                    parser.current().span.end,
                )
                .is_some()))
        }

        Token::RightBracket
        | Token::RightParentheses
        | Token::Pipe
        | Token::DoubleQuote
        | Token::EscapedDoubleQuote
        | Token::EscapedBackslash => true,

        Token::Other => {
            // A "/" directly before a block opener or a comment may be
            // half of an italics opener split by an iftags segment or a
            // comment; let the rule loop probe it instead of claiming
            // it as text here. The text rule remains the fallback when
            // the probe fails.
            if parser.current().slice == "/"
                && parser.look_ahead(0).is_some_and(|token| {
                    matches!(token.token, Token::LeftBlock | Token::LeftComment)
                })
            {
                return false;
            }
            parser.settings().layout.legacy() || parser.current().slice != "&"
        }

        Token::Whitespace => {
            !parser.start_of_line()
                && !matches!(
                    parser.next_two_tokens(),
                    (Token::Whitespace, Some(Token::Underscore))
                )
        }

        Token::Underscore => {
            parser.settings().layout.legacy()
                || !(parser.start_of_line()
                    && matches!(
                        parser.look_ahead(0).map(|token| token.token),
                        Some(Token::LineBreak | Token::ParagraphBreak)
                    ))
        }

        // Wikidot leaves padded formatting openers as literal text. A
        // delimiter followed by whitespace cannot begin any of these inline
        // containers, so do not send it through a rule that is guaranteed to
        // fail and add a warning.
        Token::Underline => {
            matches!(
                parser.look_ahead(0).map(|token| token.token),
                Some(Token::Whitespace | Token::LeftBlockEnd)
            )
        }

        Token::Bold | Token::Italics | Token::Superscript | Token::Subscript => matches!(
            parser.look_ahead(0).map(|token| token.token),
            Some(Token::Whitespace)
        ),

        // These markers are structural only at the start of a line. Real
        // Wikidot pages also use repeated tildes as ordinary punctuation.
        Token::ClearFloatBoth | Token::ClearFloatLeft | Token::ClearFloatRight => {
            !parser.start_of_line()
        }

        // Wikidot treats a nested monospace opener as literal text and closes
        // the outer span at the first following terminator.
        Token::LeftMonospace => parser.rule().name() == "monospace",

        // A closing raw marker is intercepted by the raw collector when it
        // has a matching opener. Outside a raw span Wikidot preserves it.
        Token::RightRaw => true,

        // A standalone closing bracket pair is literal Wikidot text. Valid
        // block collectors intercept their own closer before `consume()`.
        Token::RightBlock => parser.start_of_line(),

        Token::BulletItem | Token::NumberedItem | Token::Equals | Token::Colon => {
            !parser.start_of_line()
        }

        // Four-or-more hyphen runs can be horizontal rules only at an
        // immediate line boundary, but Wikidot still gives their inline
        // fallback structured dash/strikethrough semantics.
        Token::TripleDash => false,

        _ => false,
    }
}

fn try_consume_text_token<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<Elements<'t>>, ParseError> {
    if parser.settings().layout.legacy()
        && parser.current().slice == "\u{fffd}"
        && parser
            .look_ahead(0)
            .is_some_and(|token| token.token == Token::LineBreak && token.slice == "\\")
    {
        parser.step()?;
        return Ok(Some(Elements::None));
    }
    if parser.current().token == Token::DiscardedControl {
        parser.step()?;
        return Ok(Some(Elements::None));
    }
    // Wikidot discards adjacent underline delimiters as empty containers. Do
    // that one pair at a time so a long run never needs a forward scan at
    // every delimiter.
    if parser.settings().layout.legacy() && parser.current().token == Token::Underline {
        #[cfg(test)]
        parser.increment_underline_fast_path_visits();

        if parser
            .look_ahead(0)
            .is_some_and(|token| token.token == Token::Underline)
        {
            parser.step()?;
            parser.step()?;
            return Ok(Some(Elements::None));
        }
    }
    if !can_consume_as_text_token(parser) {
        return Ok(None);
    }

    let token = parser.current().token;
    let slice = parser.current().slice;
    let trailing_whitespace = token == Token::Whitespace
        && matches!(
            parser.look_ahead(0).map(|next| next.token),
            Some(Token::LineBreak | Token::ParagraphBreak | Token::InputEnd)
        );
    parser.step()?;
    if trailing_whitespace {
        Ok(Some(Elements::None))
    } else if token == Token::Whitespace {
        Ok(Some(text!(" ").into()))
    } else {
        Ok(Some(text!(slice).into()))
    }
}

fn try_consume_line_break<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<Elements<'t>>, ParseError> {
    if parser.current().token != Token::LineBreak {
        return Ok(None);
    }
    if parser.settings().layout.legacy()
        && parser.current().slice != "\\"
        && parser
            .look_ahead(0)
            .is_some_and(|token| token.token == Token::InputEnd)
    {
        parser.step()?;
        return Ok(Some(Elements::None));
    }

    if parser.quote_body_cursor().is_some() {
        let mut next = parser.clone();
        next.step()?;
        next.get_optional_space()?;
        if next.prepare_quote_body_line()? == QuoteBodyLineStatus::Prepared {
            return Ok(None);
        }
    }

    let next = parser.next_three_tokens();
    let starts_definition =
        next.1 == Some(Token::Colon) && next.2 == Some(Token::Whitespace);
    let starts_block = matches!(next.1, Some(Token::LeftBlock | Token::LeftBlockStar));
    if starts_definition
        || (starts_block && !upcoming_block_ends_with_single_bracket(parser))
    {
        return Ok(None);
    }

    let next_offset = if matches!(
        parser.look_ahead(0).map(|token| token.token),
        Some(Token::Whitespace)
    ) {
        1
    } else {
        0
    };
    let skip = parser.look_ahead(next_offset).is_some_and(|token| {
        let following = parser.look_ahead(next_offset + 1).map(|next| next.token);
        let valid_shape = match token.token {
            Token::Heading | Token::BulletItem | Token::NumberedItem => {
                following == Some(Token::Whitespace)
            }
            Token::Equals => {
                !(parser.settings().layout.legacy() && next_offset == 1)
                    && (following == Some(Token::Whitespace)
                        || parser
                            .remaining()
                            .iter()
                            .skip(next_offset + 1)
                            .take_while(|next| next.token == Token::Equals)
                            .count()
                            >= 3)
            }
            Token::TripleDash => matches!(
                following,
                Some(Token::LineBreak | Token::ParagraphBreak | Token::InputEnd)
            ),
            _ => true,
        };
        starts_own_line_rule(token.token)
            && valid_shape
            && !(next_offset == 1
                && matches!(
                    token.token,
                    Token::Quote | Token::BulletItem | Token::NumberedItem
                ))
    });

    parser.step()?;
    if skip {
        Ok(Some(Elements::None))
    } else {
        Ok(Some(Element::LineBreak.into()))
    }
}

fn upcoming_block_ends_with_single_bracket(parser: &Parser<'_, '_>) -> bool {
    let mut last = None;
    for token in parser.remaining() {
        if matches!(
            token.token,
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
        ) {
            break;
        }
        if token.token == Token::RightBlock {
            return false;
        }
        if token.token != Token::Whitespace {
            last = Some(token.token);
        }
    }
    last == Some(Token::RightBracket)
}

fn try_consume_leaf_token<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<Elements<'t>>, ParseError>
where
    'r: 't,
{
    if matches!(parser.current().token, Token::Url | Token::UrlStar) {
        let elements = url_elements(parser)?;
        return Ok(Some(elements));
    }

    let element = match parser.current().token {
        Token::Email => Element::Email(cow!(parser.current().slice)),

        Token::Variable => {
            let slice = parser.current().slice;
            let variable = slice
                .strip_prefix("{$")
                .and_then(|inner| inner.strip_suffix('}'))
                .expect("variable token must retain its lexical delimiters");
            Element::Variable(cow!(variable))
        }

        _ => return Ok(None),
    };

    parser.step()?;
    Ok(Some(element.into()))
}

/// Main function that consumes tokens to produce a single element, then returns.
///
/// It will use the fallback if all rules, fail, so the only failure case is if
/// the end of the input is reached.
pub fn consume<'r, 't>(parser: &mut Parser<'r, 't>) -> ParseResult<'r, 't, Elements<'t>> {
    if parser.prepare_quote_body_line()? == QuoteBodyLineStatus::Boundary {
        return Err(parser.make_err(ParseErrorKind::EndOfInput));
    }

    if parser.settings().layout.legacy()
        && parser.current().token == Token::ParagraphBreak
    {
        parser.clear_wikidot_literal_triple_links();
    }

    parser.depth_increment()?;

    let pending_unquoted_collapsible_close = parser.settings().layout.legacy()
        && parser.pending_wikidot_collapsible_closer()
        && parser.native_blockquote_depth().is_none();
    if let Some(elements) = try_consume_wikidot_unmatched_inline_close_link(parser)? {
        parser.depth_decrement();
        return ok!(elements);
    }
    if let Some(elements) = try_consume_structural_close(parser)? {
        parser.depth_decrement();
        if pending_unquoted_collapsible_close {
            return ok!(false; elements);
        }
        return ok!(elements);
    }

    if let Some(elements) = try_consume_wikidot_adjacent_unmatched_closes_as_link(parser)?
    {
        parser.depth_decrement();
        return ok!(elements);
    }

    if let Some(elements) = try_consume_wikidot_parser_function_anchor_fallback(parser)? {
        parser.depth_decrement();
        return ok!(elements);
    }

    if let Some(elements) = try_consume_line_break(parser)? {
        parser.depth_decrement();
        return ok!(elements);
    }

    if let Some(elements) = try_consume_text_token(parser)? {
        parser.depth_decrement();
        return ok!(elements);
    }

    if let Some(elements) = try_consume_leaf_token(parser)? {
        parser.depth_decrement();
        return ok!(elements);
    }

    let mut all_errors = Vec::new();
    let current = parser.current();

    for &rule in get_rules_for_token(current) {
        if rule.name() == "delayed-conditional"
            && parser.generated_until_right_block().is_empty()
        {
            continue;
        }
        let old_remaining = parser.remaining();
        let footnote_count = parser.footnote_count();
        match rule.try_consume(parser) {
            Ok(output) => {
                if parser.same_pointer(old_remaining) {
                    parser.step()?;
                }

                mem::drop(all_errors);

                parser.depth_decrement();

                return Ok(output);
            }
            Err(error) => {
                parser.truncate_footnotes(footnote_count);

                if parser.discarding_hidden_body() {
                    if parser.at_hidden_body_boundary() {
                        parser.depth_decrement();
                        return Err(error);
                    }

                    if hidden_failure_must_close_to_eof(error.kind()) {
                        parser.skip_to_hidden_body_boundary()?;
                        parser.depth_decrement();
                        return Err(error);
                    }
                }

                all_errors.push(error);
            }
        }
    }

    if parser.settings().layout.legacy() {
        match current.token {
            Token::LeftLink | Token::LeftLinkStar => {
                parser.enter_wikidot_literal_triple_link();
            }
            Token::RightLink => parser.leave_wikidot_literal_triple_link(),
            _ => {}
        }
    }

    let element = if parser.settings().layout.legacy()
        && current.token == Token::LeftBlock
        && current.span.start == 0
        && parser
            .full_text()
            .inner()
            .get(.."[[ bibliography]]".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("[[ bibliography]]"))
    {
        Element::Text(Cow::Owned(format!("\n\n{}", current.slice)))
    } else {
        text!(current.slice)
    };
    parser.step()?;

    if let Some(error) = all_errors.last()
        && error.kind() == ParseErrorKind::RecursionDepthExceeded
    {
        error!("Found recursion depth error, failing");
        return Err(error.clone());
    }

    let error = ParseError::new(ParseErrorKind::NoRulesMatch, RULE_FALLBACK, current);
    all_errors.push(error);

    parser.depth_decrement();

    ok!(element, all_errors)
}

fn hidden_failure_must_close_to_eof(kind: ParseErrorKind) -> bool {
    kind.must_close_hidden_body_through_end_of_input()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::{LinkLabel, LinkLocation, LinkType};

    fn parser_for<'t>(
        input: &'t str,
    ) -> (
        crate::tokenizer::Tokenization<'t>,
        PageInfo<'static>,
        WikitextSettings,
    ) {
        (
            crate::tokenize(input),
            PageInfo::dummy(),
            WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump),
        )
    }

    fn parser_at<'r, 't>(
        tokenization: &'r crate::tokenizer::Tokenization<'t>,
        page_info: &'r PageInfo<'static>,
        settings: &'r WikitextSettings,
        steps: usize,
    ) -> Parser<'r, 't> {
        let mut parser = Parser::new(tokenization, page_info, settings);
        for _ in 0..steps {
            parser.step().expect("test token step should succeed");
        }
        parser
    }

    #[test]
    fn direct_text_fast_path_preserves_structural_starts() {
        let (tokens, page_info, settings) = parser_for("word text");
        let parser = parser_at(&tokens, &page_info, &settings, 1);
        assert!(can_consume_as_text_token(&parser));

        let (tokens, page_info, settings) = parser_for("word text");
        let parser = parser_at(&tokens, &page_info, &settings, 2);
        assert!(can_consume_as_text_token(&parser));

        let (tokens, page_info, settings) = parser_for(" * item");
        let parser = parser_at(&tokens, &page_info, &settings, 1);
        assert!(!can_consume_as_text_token(&parser));

        let (tokens, page_info, settings) = parser_for("word _\nnext");
        let parser = parser_at(&tokens, &page_info, &settings, 2);
        assert!(!can_consume_as_text_token(&parser));

        let (tokens, page_info, settings) = parser_for(": term\n: value");
        let parser = parser_at(&tokens, &page_info, &settings, 1);
        assert!(!can_consume_as_text_token(&parser));

        let (tokens, page_info, settings) =
            parser_for("# // This is literal punctuation");
        let mut parser = parser_at(&tokens, &page_info, &settings, 3);
        let elements = try_consume_text_token(&mut parser)
            .expect("padded italics marker should not fail")
            .expect("padded italics marker should use the text fast path");
        assert_eq!(elements, text!("//").into());

        let (tokens, page_info, settings) = parser_for("text~~~~!!!");
        let mut parser = parser_at(&tokens, &page_info, &settings, 2);
        let elements = try_consume_text_token(&mut parser)
            .expect("mid-line clear-float marker should not fail")
            .expect("mid-line clear-float marker should use the text fast path");
        assert_eq!(elements, text!("~~~~").into());

        let (tokens, page_info, mut settings) = parser_for("______ ______");
        settings.layout = Layout::Wikidot;
        let mut parser = parser_at(&tokens, &page_info, &settings, 1);
        let elements = try_consume_text_token(&mut parser)
            .expect("repeated underline spacer should not fail")
            .expect("repeated underline spacer should use the text fast path");
        assert_eq!(elements, Elements::None);

        let (tokens, page_info, mut settings) = parser_for("______[[/span]]");
        settings.layout = Layout::Wikidot;
        let mut parser = parser_at(&tokens, &page_info, &settings, 1);
        let elements = try_consume_text_token(&mut parser)
            .expect("repeated underline before a block closer should not fail")
            .expect("repeated underline before a block closer should stay literal");
        assert_eq!(elements, Elements::None);

        let (tokens, page_info, settings) = parser_for("x>@");
        let mut parser = parser_at(&tokens, &page_info, &settings, 2);
        let elements = try_consume_text_token(&mut parser)
            .expect("unmatched raw closer should not fail")
            .expect("unmatched raw closer should use the text fast path");
        assert_eq!(elements, text!(">@").into());

        let (tokens, page_info, settings) = parser_for("alpha\n]]");
        let mut parser = parser_at(&tokens, &page_info, &settings, 3);
        let elements = try_consume_text_token(&mut parser)
            .expect("standalone closing brackets should not fail")
            .expect("standalone closing brackets should use the text fast path");
        assert_eq!(elements, text!("]]").into());
    }

    #[test]
    fn direct_line_break_fast_path_preserves_skips_and_block_fallbacks() {
        let (tokens, page_info, settings) = parser_for("alpha\nbeta");
        let mut parser = parser_at(&tokens, &page_info, &settings, 2);
        let elements = try_consume_line_break(&mut parser)
            .expect("line break fast path should not fail")
            .expect("plain line break should use fast path");
        assert_eq!(elements, Element::LineBreak.into());
        assert_eq!(parser.current().token, Token::Identifier);

        let (tokens, page_info, settings) = parser_for("alpha\n+ heading");
        let mut parser = parser_at(&tokens, &page_info, &settings, 2);
        let elements = try_consume_line_break(&mut parser)
            .expect("line break before heading should not fail")
            .expect("line break before heading should use fast path");
        assert_eq!(elements, Elements::None);
        assert_eq!(parser.current().token, Token::Heading);

        let (tokens, page_info, settings) = parser_for("alpha\n> > quoted");
        let mut parser = parser_at(&tokens, &page_info, &settings, 2);
        parser.install_quote_body_cursor_with_literal_residuals(1);
        assert!(
            try_consume_line_break(&mut parser)
                .expect("quote-aware line break deferral should not fail")
                .is_none(),
        );
        assert_eq!(parser.current().token, Token::LineBreak);

        let (tokens, page_info, settings) = parser_for("alpha\n[[code]]");
        let mut parser = parser_at(&tokens, &page_info, &settings, 2);
        assert!(
            try_consume_line_break(&mut parser)
                .expect("line break block fallback check should not fail")
                .is_none(),
        );
        assert_eq!(parser.current().token, Token::LineBreak);

        let (tokens, page_info, settings) = parser_for("alpha\n[[iftags +alphaX]\nomega");
        let mut parser = parser_at(&tokens, &page_info, &settings, 2);
        let elements = try_consume_line_break(&mut parser)
            .expect("line break before malformed block should not fail")
            .expect("single-bracket block fallback must keep its line break");
        assert_eq!(elements, Element::LineBreak.into());
        assert_eq!(parser.current().token, Token::LeftBlock);

        let (tokens, page_info, settings) = parser_for("alpha\n[[code]]]");
        let mut parser = parser_at(&tokens, &page_info, &settings, 2);
        assert!(
            try_consume_line_break(&mut parser)
                .expect("valid block followed by literal bracket should not fail")
                .is_none(),
        );
        assert_eq!(parser.current().token, Token::LineBreak);

        let (tokens, page_info, settings) = parser_for("alpha\n: term");
        let mut parser = parser_at(&tokens, &page_info, &settings, 2);
        assert!(
            try_consume_line_break(&mut parser)
                .expect("definition-list fallback check should not fail")
                .is_none(),
        );
        assert_eq!(parser.current().token, Token::LineBreak);
    }

    #[test]
    fn wikidot_underline_pair_consumption_has_linear_deterministic_work() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for marker_count in [1, 2, 3, 4, 255, 256, 511, 512] {
            let input = format!("{}x", "__".repeat(marker_count));
            let tokenization = crate::tokenize(&input);
            let mut parser = Parser::new(&tokenization, &page_info, &settings);
            parser
                .step()
                .expect("the first underline token should exist");

            while parser.current().token != Token::InputEnd {
                let _ = consume(&mut parser).expect("the underline run should parse");
            }

            assert_eq!(
                parser.underline_fast_path_visits(),
                marker_count.div_ceil(2),
                "{marker_count} underline tokens",
            );
        }
    }

    #[test]
    fn direct_leaf_fast_path_preserves_leaf_elements() {
        let (tokens, page_info, settings) = parser_for("https://example.com");
        let mut parser = parser_at(&tokens, &page_info, &settings, 1);
        let elements = try_consume_leaf_token(&mut parser)
            .expect("url fast path should not fail")
            .expect("url should use leaf fast path");
        assert_eq!(
            elements,
            Element::Link {
                ltype: LinkType::Direct,
                link: LinkLocation::Url(cow!("https://example.com")),
                label: LinkLabel::Url,
                target: None,
            }
            .into(),
        );
        assert_eq!(parser.current().token, Token::InputEnd);

        let (tokens, page_info, settings) = parser_for("abc@example.com");
        let mut parser = parser_at(&tokens, &page_info, &settings, 1);
        let elements = try_consume_leaf_token(&mut parser)
            .expect("email fast path should not fail")
            .expect("email should use leaf fast path");
        assert_eq!(elements, Element::Email(cow!("abc@example.com")).into());
        assert_eq!(parser.current().token, Token::InputEnd);

        let (tokens, page_info, settings) = parser_for("{$title}");
        let mut parser = parser_at(&tokens, &page_info, &settings, 1);
        let elements = try_consume_leaf_token(&mut parser)
            .expect("variable fast path should not fail")
            .expect("variable should use leaf fast path");
        assert_eq!(elements, Element::Variable(cow!("title")).into());
        assert_eq!(parser.current().token, Token::InputEnd);
    }

    #[test]
    fn wikidot_url_leaf_fast_path_splits_terminal_period() {
        let (tokens, page_info, mut settings) =
            parser_for("https://example.com/test. next");
        settings.layout = Layout::Wikidot;
        let mut parser = parser_at(&tokens, &page_info, &settings, 1);
        let elements = try_consume_leaf_token(&mut parser)
            .expect("URL fast path should not fail")
            .expect("URL should use leaf fast path");
        assert_eq!(
            elements,
            Elements::Multiple(vec![
                Element::Link {
                    ltype: LinkType::Direct,
                    link: LinkLocation::Url(cow!("https://example.com/test")),
                    label: LinkLabel::Url,
                    target: None,
                },
                text!("."),
            ]),
        );
        assert_eq!(parser.current().token, Token::Whitespace);
    }

    #[test]
    fn wikidot_adjacent_unmatched_block_closes_reenter_single_link_lexing() {
        let (tokens, page_info, mut settings) = parser_for("[[/cell]] [[/table]]");
        settings.layout = Layout::Wikidot;
        let mut parser = parser_at(&tokens, &page_info, &settings, 1);
        let elements = try_consume_wikidot_adjacent_unmatched_closes_as_link(&mut parser)
            .expect("adjacent close fallback should not fail")
            .expect("adjacent unmatched closes should reenter link lexing");

        assert_eq!(
            elements,
            Elements::Multiple(vec![
                text!("["),
                Element::Link {
                    ltype: LinkType::Direct,
                    link: LinkLocation::Url(cow!("/cell]]")),
                    label: LinkLabel::Text(cow!("[[/table")),
                    target: None,
                },
                text!("]"),
            ]),
        );
        assert_eq!(parser.current().token, Token::InputEnd);

        let (tokens, page_info, mut settings) = parser_for("[[/cell]]");
        settings.layout = Layout::Wikidot;
        let mut parser = parser_at(&tokens, &page_info, &settings, 1);
        assert!(
            try_consume_wikidot_adjacent_unmatched_closes_as_link(&mut parser)
                .expect("single unmatched close should not fail")
                .is_none(),
        );

        let (tokens, page_info, mut settings) = parser_for("[[/cell]][[/table]]");
        settings.layout = Layout::Wikidot;
        let mut parser = parser_at(&tokens, &page_info, &settings, 1);
        assert!(
            try_consume_wikidot_adjacent_unmatched_closes_as_link(&mut parser)
                .expect("unspaced unmatched closes should not fail")
                .is_none(),
        );

        let (tokens, page_info, settings) = parser_for("[[/cell]] [[/table]]");
        let mut parser = parser_at(&tokens, &page_info, &settings, 1);
        assert!(
            try_consume_wikidot_adjacent_unmatched_closes_as_link(&mut parser)
                .expect("Wikijump layout fallback should not fail")
                .is_none(),
        );
    }

    #[test]
    fn wikidot_spaced_unmatched_block_close_reenters_single_link_lexing() {
        let (tokens, page_info, mut settings) = parser_for("[[/ head]]");
        settings.layout = Layout::Wikidot;
        let mut parser = parser_at(&tokens, &page_info, &settings, 1);
        let elements = try_consume_wikidot_adjacent_unmatched_closes_as_link(&mut parser)
            .expect("spaced close fallback should not fail")
            .expect("spaced unmatched close should reenter link lexing");

        assert_eq!(
            elements,
            Elements::Multiple(vec![
                text!("["),
                Element::Link {
                    ltype: LinkType::Direct,
                    link: LinkLocation::Url(cow!("/")),
                    label: LinkLabel::Text(cow!("head")),
                    target: None,
                },
                text!("]"),
            ]),
        );
        assert_eq!(parser.current().token, Token::InputEnd);

        for source in ["[[/head]]", "[[/ ]]", "[[/ head\n]]"] {
            let (tokens, page_info, mut settings) = parser_for(source);
            settings.layout = Layout::Wikidot;
            let mut parser = parser_at(&tokens, &page_info, &settings, 1);
            assert!(
                try_consume_wikidot_adjacent_unmatched_closes_as_link(&mut parser)
                    .expect("negative spaced-close control should not fail")
                    .is_none(),
                "{source}",
            );
        }

        let (tokens, page_info, settings) = parser_for("[[/ head]]");
        let mut parser = parser_at(&tokens, &page_info, &settings, 1);
        assert!(
            try_consume_wikidot_adjacent_unmatched_closes_as_link(&mut parser)
                .expect("Wikijump layout fallback should not fail")
                .is_none(),
        );
    }
}
