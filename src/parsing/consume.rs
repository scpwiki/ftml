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
use super::parser::block_pairs::WikidotBlockKind;
use super::parser::inline_pairs::InlineMarkerRole;
use super::parser::wikidot_own_line_prefix;
use super::prelude::*;
use super::rule::{
    get_rules_for_token,
    impls::{
        RULE_FALLBACK, glued_scheme_url_start, starts_own_line_rule,
        starts_wikidot_only_scheme_url, try_consume_wikidot_end_block_link, url_elements,
    },
};
use crate::tree::{
    LinkLabel, LinkLocation, LinkType, PartialElement, WikidotBlockCloseKind,
};
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

    if parser.in_wikidot_span_body() || parser.in_wikidot_size_body() {
        return Ok(None);
    }

    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let Some(marker_found) = parser.cached_close_marker(source, start) else {
        return Ok(None);
    };
    let relative_marker_end = marker_found - start;
    let marker_end = marker_found + 2;
    let name = source[start + "[[/".len()..start + relative_marker_end]
        .trim_matches([' ', '\t']);
    if !matches!(
        name.to_ascii_lowercase().as_str(),
        "div" | "span" | "footnote"
    ) {
        return Ok(None);
    }
    if parser
        .cached_owner_opener_before(source, start, name)
        .unwrap_or_else(|| line_has_unmatched_wikidot_owner_opener(source, start, name))
    {
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
    let (_, line_end) = parser.unmatched_line_bounds(source, label_start);
    let Some(close) = parser
        .cached_close_bracket(source, label_start)
        .filter(|found| *found < line_end)
    else {
        return Ok(None);
    };
    let raw_label = &source[label_start..close];
    if wikidot_label_region_has_owner_opener(&source[label_start..close]) {
        return Ok(None);
    }
    // Live: an enclosing body's closer inside the label region (tab, cell,
    // collapsible, ...) still ends that body; the stray closer stays literal
    // and no link forms across it.
    if wikidot_label_region_has_body_closer(parser, source, label_start, close) {
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

fn wikidot_label_region_has_body_closer<'r, 't>(
    parser: &Parser<'r, 't>,
    source: &str,
    region_start: usize,
    region_end: usize,
) -> bool
where
    'r: 't,
{
    let mut offset = region_start;
    while let Some(found) = source[offset..region_end].find("[[/") {
        let at = offset + found;
        let mut probe = parser.clone();
        while probe.current().span.start < at && probe.step().is_ok() {}
        if probe.current().span.start == at && probe.at_body_end_boundary() {
            return true;
        }
        offset = at + "[[/".len();
    }
    false
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

pub(crate) fn line_has_unmatched_wikidot_owner_opener(
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

pub(crate) fn wikidot_flat_close_kind_at_byte<'r, 't>(
    parser: &Parser<'r, 't>,
    closer_byte: usize,
) -> Option<WikidotBlockCloseKind<'t>>
where
    'r: 't,
{
    if !parser.settings().layout.legacy() || parser.discarding_hidden_body() {
        return None;
    }
    let pair = parser.wikidot_block_pair_at_closer_byte(closer_byte)?;
    if !parser.wikidot_pair_goes_flat(&pair) {
        return None;
    }
    // A scored head rejected by the alias check stays literal, so its closer
    // must stay literal too (memoized by the head-side check).
    if pair.kind == WikidotBlockKind::Div
        && pair.scored
        && !super::rule::impls::wikidot_scored_div_close_compatible(
            parser,
            pair.head_byte,
        )
    {
        return None;
    }
    Some(match pair.kind {
        WikidotBlockKind::Div => WikidotBlockCloseKind::Div {
            head: pair.head_byte,
            scored: pair.scored,
        },
        WikidotBlockKind::Collapsible => WikidotBlockCloseKind::Collapsible {
            head: pair.head_byte,
        },
        WikidotBlockKind::Note => WikidotBlockCloseKind::Note {
            head: pair.head_byte,
        },
        WikidotBlockKind::AlignCenter
        | WikidotBlockKind::AlignRight
        | WikidotBlockKind::AlignLeft
        | WikidotBlockKind::AlignJustify => WikidotBlockCloseKind::Align {
            head: pair.head_byte,
        },
        _ => return None,
    })
}

fn render_interior_line_prefix(source: &str, byte: usize) -> &str {
    let line_start = source[..byte.min(source.len())]
        .rfind('\n')
        .map_or(0, |offset| offset + 1);
    &source[line_start..byte.min(source.len())]
}

fn wikidot_render_interior_close_kind_at<'r, 't>(
    parser: &Parser<'r, 't>,
    closer_byte: usize,
) -> Option<WikidotBlockCloseKind<'t>>
where
    'r: 't,
{
    let pair = parser.wikidot_block_pair_at_closer_byte(closer_byte)?;
    if !pair.kind.emits_flat() {
        return None;
    }
    let kind = wikidot_flat_close_kind_at_byte(parser, closer_byte)?;
    if !parser.wikidot_head_on_table_row_line(pair.head_byte)
        && !parser.wikidot_byte_on_list_item_line(pair.head_byte)
    {
        return None;
    }
    if parser.wikidot_byte_on_list_item_line(closer_byte) {
        return None;
    }
    let source = parser.full_text().inner();
    if pair.head_byte >= pair.closer_byte
        || !source[pair.head_byte..pair.closer_byte].contains('\n')
    {
        return None;
    }
    Some(kind)
}

fn render_interior_close_line_is_quoted(source: &str, closer_byte: usize) -> bool {
    render_interior_line_prefix(source, closer_byte)
        .trim_start_matches([' ', '\t'])
        .starts_with('>')
}

fn render_interior_close_has_midrow_suffix(parser: &Parser<'_, '_>) -> bool {
    let source = parser.full_text().inner();
    let start = parser.current().span.start.min(source.len());
    let end = source[start..]
        .find('\n')
        .map_or(source.len(), |offset| start + offset);
    !source[start..end].trim_matches([' ', '\t']).is_empty()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderInteriorNext {
    Blank,
    Block,
    Breakable,
    Plain,
}

fn render_interior_next_token<'a>(
    parser: &'a Parser<'_, '_>,
    index: usize,
) -> Option<&'a crate::parsing::ExtractedToken<'a>> {
    // look_ahead(0) is the token after current, so index 0 is current.
    if index == 0 {
        Some(parser.current())
    } else {
        parser.look_ahead(index - 1)
    }
}

fn render_interior_next_line_shape(parser: &Parser<'_, '_>) -> RenderInteriorNext {
    let mut index = 0usize;
    loop {
        let Some(token) = render_interior_next_token(parser, index) else {
            return RenderInteriorNext::Blank;
        };
        index += 1;
        match token.token {
            Token::Whitespace => {}
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                return RenderInteriorNext::Blank;
            }
            Token::LeftBlock | Token::LeftBlockStar | Token::LeftBlockEnd => {
                return RenderInteriorNext::Block;
            }
            // L8.5: live drops the closer break before a fresh list
            // (P27, l-coll-item-cross) but keeps it before tables.
            Token::BulletItem | Token::NumberedItem => {
                return RenderInteriorNext::Block;
            }
            Token::TableColumn
            | Token::TableColumnRight
            | Token::TableColumnCenter
            | Token::TableColumnTitle
            | Token::Quote
            | Token::Heading
            | Token::TripleDash => return RenderInteriorNext::Breakable,
            _ => return RenderInteriorNext::Plain,
        }
    }
}

fn render_interior_plain_line_ahead(parser: &Parser<'_, '_>) -> bool {
    let mut index = 0usize;
    let mut first_meaningful = true;
    loop {
        let Some(token) = render_interior_next_token(parser, index) else {
            return false;
        };
        index += 1;
        match token.token {
            Token::Whitespace => {}
            Token::LineBreak | Token::InputEnd => return !first_meaningful,
            Token::ParagraphBreak => return false,
            Token::LeftBlock
            | Token::LeftBlockEnd
            | Token::LeftBlockStar
            | Token::TableColumn
            | Token::TableColumnRight
            | Token::TableColumnCenter
            | Token::TableColumnTitle
            | Token::Quote
            | Token::BulletItem
            | Token::NumberedItem
            | Token::Heading
            | Token::TripleDash => return false,
            Token::Colon if first_meaningful => return false,
            _ => {
                first_meaningful = false;
            }
        }
    }
}

fn consume_render_interior_chain<'r, 't>(
    parser: &mut Parser<'r, 't>,
    errors: &mut Vec<ParseError>,
) -> Result<Vec<Element<'t>>, ParseError>
where
    'r: 't,
{
    let mut elements = Vec::new();
    loop {
        if !render_interior_plain_line_ahead(parser) {
            break;
        }
        if !elements.is_empty() {
            elements.push(Element::LineBreak);
        }
        loop {
            match parser.current().token {
                Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => break,
                _ => {}
            }
            let before = parser.current().span.start;
            let checkpoint = parser.clone();
            match consume(parser) {
                Ok(success) => {
                    if parser.current().span.start == before {
                        parser.update(&checkpoint);
                        break;
                    }
                    let (item, mut item_errors, _) = success.into();
                    errors.append(&mut item_errors);
                    match item {
                        Elements::None => {}
                        Elements::Single(element) => elements.push(element),
                        Elements::Multiple(items) => elements.extend(items),
                    }
                }
                Err(error) => {
                    parser.update(&checkpoint);
                    errors.push(error);
                    break;
                }
            }
        }
        if parser.current().token != Token::LineBreak {
            break;
        }
        let mut probe = parser.clone();
        if probe.step().is_err() {
            break;
        }
        match render_interior_next_line_shape(&probe) {
            RenderInteriorNext::Plain => {
                parser.step()?;
            }
            RenderInteriorNext::Blank => break,
            _ => {
                parser.step()?;
                break;
            }
        }
    }
    Ok(elements)
}

fn try_consume_wikidot_fused_span_close_then_table_column<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<Elements<'t>>, ParseError>
where
    'r: 't,
{
    if !parser.settings().layout.legacy() || parser.current().token != Token::LeftBlockEnd
    {
        return Ok(None);
    }
    if !parser.in_wikidot_span_body() {
        return Ok(None);
    }
    let start = parser.current().span.start;
    let source = parser.full_text().inner();
    if !source.get(start..).is_some_and(|rest| {
        rest.len() >= "[[/span]]".len()
            && rest[.."[[/span]]".len()].eq_ignore_ascii_case("[[/span]]")
    }) {
        return Ok(None);
    }
    let mut probe = parser.clone();
    let Ok(name) = probe.get_end_block() else {
        return Ok(None);
    };
    let name_ref = name.as_ref();
    let base = name_ref.strip_suffix('_').unwrap_or(name_ref);
    if !base.eq_ignore_ascii_case("span") || name_ref.len() != base.len() {
        return Ok(None);
    }
    let column_start = probe.current().span.start;
    if column_start < 2 || source.get(column_start - 2..column_start) != Some("]]") {
        return Ok(None);
    }
    if !matches!(
        probe.current().token,
        Token::TableColumn
            | Token::TableColumnRight
            | Token::TableColumnCenter
            | Token::TableColumnTitle,
    ) {
        return Ok(None);
    }
    if !parser.in_wikidot_advanced_table_cell_body() {
        return Ok(None);
    }
    let closes_scored_span = parser.wikidot_span_alias_close_is_scored(start);
    parser.update(&probe);
    let literal = parser.current().slice;
    parser.step()?;
    parser.leave_wikidot_span_body(closes_scored_span);
    Ok(Some(text!(literal).into()))
}

fn try_consume_wikidot_render_interior_close<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<ParseResult<'r, 't, Elements<'t>>>, ParseError>
where
    'r: 't,
{
    if !parser.settings().layout.legacy()
        || parser.discarding_hidden_body()
        || parser.current().token != Token::LeftBlockEnd
    {
        return Ok(None);
    }
    let closer_byte = parser.current().span.start;
    let Some(kind) = wikidot_render_interior_close_kind_at(parser, closer_byte) else {
        return Ok(None);
    };
    let source = parser.full_text().inner();
    if render_interior_close_line_is_quoted(source, closer_byte) {
        return Ok(None);
    }
    let mut close = parser.clone();
    if close.get_end_block().is_err() {
        return Ok(None);
    }
    parser.update(&close);
    let close_element = Element::Partial(PartialElement::WikidotBlockClose(kind));
    if render_interior_close_has_midrow_suffix(parser) {
        return Ok(Some(ok!(false; close_element, Vec::new())));
    }
    while parser.current().token == Token::Whitespace {
        if parser.step().is_err() {
            break;
        }
    }
    let mut probe = parser.clone();
    let ate_break = if probe.current().token == Token::LineBreak {
        probe.step().is_ok()
    } else {
        false
    };
    if !ate_break {
        return Ok(Some(ok!(false; close_element, Vec::new())));
    }
    match render_interior_next_line_shape(&probe) {
        RenderInteriorNext::Blank | RenderInteriorNext::Block => {
            parser.step()?;
            Ok(Some(ok!(false; close_element, Vec::new())))
        }
        RenderInteriorNext::Breakable => {
            parser.step()?;
            Ok(Some(ok!(false;
                Elements::Multiple(vec![close_element, Element::LineBreak]),
                Vec::new()
            )))
        }
        RenderInteriorNext::Plain => {
            parser.step()?;
            let mut errors = Vec::new();
            let mut output = vec![close_element, Element::LineBreak];
            match consume_render_interior_chain(parser, &mut errors) {
                Ok(chain) => output.extend(chain),
                Err(error) => errors.push(error),
            }
            Ok(Some(ok!(false; Elements::Multiple(output), errors)))
        }
    }
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
        let close_pos = parser.current().span.start;
        if let Ok(name) = close.get_end_block() {
            let name = name.strip_suffix('_').unwrap_or(&name);
            let closer_own_line =
                wikidot_own_line_prefix(parser.full_text().inner(), close_pos);
            if parser.take_wikidot_crossed_close(name) {
                parser.update(&close);
                // L85: a silently taken crossed close that starts its own line
                // absorbs that line's break (closeout keeps a single break
                // around `[[/div]]`, q-close-in-quote drops it). A closer that
                // ends a content line keeps the break
                // (wikidot_div_closes_at_closer_on_quote_line).
                if closer_own_line && parser.current().token == Token::LineBreak {
                    let _ = parser.step();
                }
                return Ok(Some(Elements::None));
            }
        }
    }

    if !parser.settings().layout.legacy() || parser.current().token != Token::LeftBlockEnd
    {
        return Ok(None);
    }

    let mut close = parser.clone();
    let raw_close = close.get_wikidot_end_block_with_residual();
    let (name, residual_close_bracket, resolved_close) = match raw_close {
        Ok((name, residual_close_bracket)) => {
            (Cow::Borrowed(name), residual_close_bracket, false)
        }
        Err(_) => {
            let mut scan = parser.clone();
            let mut seamed = false;
            loop {
                match scan.current().token {
                    Token::LeftBlock => {
                        seamed = true;
                        break;
                    }
                    Token::RightBlock
                    | Token::RightLink
                    | Token::Whitespace
                    | Token::LineBreak
                    | Token::ParagraphBreak
                    | Token::InputEnd => break,
                    _ => {
                        if scan.step().is_err() {
                            break;
                        }
                    }
                }
            }
            if !seamed {
                return Ok(None);
            }
            let mut resolved = parser.clone();
            let Ok(name) = resolved.get_end_block() else {
                return Ok(None);
            };
            close = resolved;
            (name, false, true)
        }
    };
    let name: &str = &name;
    let normalized = name.strip_suffix('_').unwrap_or(name);
    let scored_close = normalized.len() != name.len();
    let start = parser.current().span.start;
    let end = close.current().span.start;
    let raw_close_source = &parser.full_text().inner()[start..end];
    let close_source_cow: Cow<'t, str> = if resolved_close {
        Cow::Owned(format!("[[/{normalized}]]"))
    } else {
        Cow::Borrowed(raw_close_source)
    };
    if !resolved_close
        && let Some(label) = wikidot_leading_space_close_label(raw_close_source)
    {
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
        Element::Text(close_source_cow.clone())
    } else if normalized.eq_ignore_ascii_case("size") {
        Element::Partial(PartialElement::InlineSizeClose(close_source_cow.clone()))
    } else if normalized.eq_ignore_ascii_case("span") {
        Element::Partial(PartialElement::InlineSpanClose(close_source_cow.clone()))
    } else if normalized.eq_ignore_ascii_case("a") {
        if !scored_close
            && !parser.in_wikidot_anchor_body()
            && !parser.in_wikidot_literal_alias_body()
        {
            return Ok(None);
        }
        if scored_close {
            Element::Text(close_source_cow.clone())
        } else {
            Element::Partial(PartialElement::InlineAnchorClose(close_source_cow.clone()))
        }
    } else if !parser.discarding_hidden_body()
        && parser.rule().name() == "block-table-row"
        && (normalized.eq_ignore_ascii_case("cell")
            || normalized.eq_ignore_ascii_case("hcell"))
    {
        Element::Text(close_source_cow.clone())
    } else if let Some(kind) = wikidot_flat_close_kind_at_byte(parser, start) {
        Element::Partial(PartialElement::WikidotBlockClose(kind))
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
    if !scored_close && normalized.eq_ignore_ascii_case("size") {
        parser.leave_wikidot_size_body();
    }
    if !scored_close && normalized.eq_ignore_ascii_case("a") {
        parser.leave_wikidot_anchor_body();
    }
    let fused_residual = resolved_close && parser.fused_name_residual() == Some(end) && {
        parser.clear_fused_name_residual();
        true
    };
    Ok(Some(if residual_close_bracket || fused_residual {
        Elements::Multiple(vec![element, text!("]")])
    } else {
        element.into()
    }))
}

fn wikidot_adjacent_link_div_head_after(source: &str, from: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut offset = from;
    while matches!(bytes.get(offset), Some(b' ' | b'\t')) {
        offset += 1;
    }
    if offset == from {
        return None;
    }
    let rest = bytes.get(offset..)?;
    if rest.len() < 5
        || !rest.starts_with(b"[[")
        || !rest[2..5].eq_ignore_ascii_case(b"div")
    {
        return None;
    }
    let mut scan = offset + 2;
    while scan < bytes.len() {
        if bytes[scan] == b'\n' {
            return None;
        }
        if bytes[scan..].starts_with(b"[[/") {
            return None;
        }
        if bytes[scan..].starts_with(b"]]") {
            return Some(offset);
        }
        scan += 1;
    }
    None
}

fn wikidot_absorb_adjacent_div_head(url_extra: &mut String, run: &str) {
    let bytes = run.as_bytes();
    if bytes.len() >= 5
        && bytes.starts_with(b"[[")
        && bytes[2..5].eq_ignore_ascii_case(b"div")
    {
        url_extra.push_str("[[div");
        url_extra.push_str(&run[5..]);
    } else {
        url_extra.push_str(run);
    }
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

    if parser.at_body_end_boundary() {
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
    let mut url_extra = String::new();
    let mut label_from = first_end;
    while let Some(head_start) = wikidot_adjacent_link_div_head_after(source, label_from)
    {
        let bytes = source.as_bytes();
        let mut run_end = head_start;
        while run_end < bytes.len()
            && !matches!(bytes[run_end], b' ' | b'\t' | b'\n' | b'\r')
        {
            run_end += 1;
        }
        wikidot_absorb_adjacent_div_head(&mut url_extra, &source[head_start..run_end]);
        label_from = run_end;
    }
    while scan.current().span.start < label_from {
        scan.step()?;
    }
    if scan.current().token == Token::InputEnd {
        return Ok(None);
    }
    if !url_extra.is_empty() {
        loop {
            match scan.current().token {
                Token::RightBracket | Token::RightBlock | Token::RightLink => break,
                Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                    return Ok(None);
                }
                _ => {
                    scan.step()?;
                }
            }
        }
        let label_end = scan.current().span.start;
        let label = source[label_from..label_end].trim();
        if label.is_empty() {
            return Ok(None);
        }
        scan.step()?;
        let url = format!("{}{}", &source[first_start + 2..first_end], url_extra);
        parser.update(&scan);
        return Ok(Some(Elements::Multiple(vec![
            text!("["),
            Element::Link {
                ltype: LinkType::Direct,
                link: LinkLocation::Url(Cow::Owned(url)),
                label: LinkLabel::Text(cow!(label)),
                target: None,
            },
            text!("]"),
        ])));
    }
    scan.step()?;
    while !matches!(
        scan.current().token,
        Token::LeftBlock
            | Token::LeftBlockEnd
            | Token::LeftLink
            | Token::LineBreak
            | Token::ParagraphBreak
            | Token::InputEnd
    ) {
        scan.step()?;
    }
    // Live r10: a `[[[` link between the closes forms on its own and the
    // div stays literal — the bridge must not span across it.
    if !matches!(scan.current().token, Token::LeftBlock | Token::LeftBlockEnd) {
        return Ok(None);
    }
    if parser.in_wikidot_size_body() && scan.current().token == Token::LeftBlockEnd {
        let mut probe = scan.clone();
        if let Ok(name) = probe.get_end_block()
            && name.eq_ignore_ascii_case("size")
        {
            return Ok(None);
        }
    }
    if scan.at_body_end_boundary() {
        return Ok(None);
    }
    let second_start = scan.current().span.start;
    let second_is_close = scan.current().token == Token::LeftBlockEnd;
    let mut second_opener_name = None;
    let second_valid = if second_is_close {
        scan.get_end_block().is_ok()
    } else {
        match scan.get_block_name(false) {
            Ok((name, _, segment)) => {
                second_opener_name = Some((name, segment));
                true
            }
            Err(_) => false,
        }
    };
    if let Some((name, segment)) = second_opener_name {
        let view = name.strip_suffix('_').unwrap_or(&name);
        if matches!(
            view.to_ascii_lowercase().as_str(),
            "footnote" | "span" | "size" | "a"
        ) && scan.head_word_view_is_actionable(
            &name,
            matches!(name, Cow::Borrowed(_)),
            segment,
            view,
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

    let url = if url_extra.is_empty() {
        cow!(&first[2..])
    } else {
        Cow::Owned(format!("{}{}", &first[2..], url_extra))
    };
    let label = source[label_from..second_end - 2].trim();
    parser.update(&scan);
    Ok(Some(Elements::Multiple(vec![
        text!("["),
        Element::Link {
            ltype: LinkType::Direct,
            link: LinkLocation::Url(url),
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
        || !matches!(
            label.to_ascii_lowercase().as_str(),
            "div" | "span" | "bibliography"
        )
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

        Token::ClearFloatBoth | Token::ClearFloatLeft | Token::ClearFloatRight => {
            !parser.start_of_line()
        }

        Token::LeftMonospace => parser.rule().name() == "monospace",

        Token::RightRaw => true,

        Token::RightBlock => parser.start_of_line(),

        Token::BulletItem => {
            if parser.look_ahead(0).is_some_and(|token| {
                matches!(token.token, Token::LeftBlock | Token::LeftComment)
            }) {
                return false;
            }
            !parser.start_of_line()
        }

        Token::NumberedItem | Token::Equals | Token::Colon => !parser.start_of_line(),

        Token::TripleDash => false,

        _ => false,
    }
}

fn try_consume_text_token<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<Elements<'t>>, ParseError>
where
    'r: 't,
{
    if let Some(elements) = try_consume_wikidot_inline_marker(parser)? {
        return Ok(Some(elements));
    }
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
    // Live Wikidot counts a trailing space after a scored span/anchor close
    // as content: the run keeps its paragraph wrapper. Keep the space for the
    // root lowering to see and then drop from the output. The same holds when
    // a scored opener follows the break: only the newline run is eaten, the
    // space before that newline stays.
    let keep_trailing_space = trailing_whitespace
        && (super::paragraph::wikidot_scored_alias_open_after_break(parser)
            || (!parser.in_wikidot_single_line_content()
                && wikidot_trailing_space_after_scored_alias_close(parser)));
    parser.step()?;
    if trailing_whitespace && !keep_trailing_space {
        Ok(Some(Elements::None))
    } else if token == Token::Whitespace {
        Ok(Some(text!(" ").into()))
    } else {
        Ok(Some(text!(slice).into()))
    }
}

fn wikidot_trailing_space_after_scored_alias_close(parser: &Parser<'_, '_>) -> bool {
    if !parser.settings().layout.legacy() {
        return false;
    }
    let source = parser.full_text().inner();
    let start = parser.current().span.start.min(source.len());
    let prefix = source[..start].trim_end_matches([' ', '\t']);
    let Some(inner) = prefix.strip_suffix("]]") else {
        return false;
    };
    let Some(open) = inner.rfind("[[/") else {
        return false;
    };
    let name = inner[open + 3..].trim_matches([' ', '\t']);
    name.eq_ignore_ascii_case("span") || name.eq_ignore_ascii_case("a")
}

fn try_consume_line_break<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<Elements<'t>>, ParseError>
where
    'r: 't,
{
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
        let keep_for_explicit_list = parser.settings().layout.legacy()
            && matches!(next.1, Some(Token::LeftBlock))
            && wikidot_upcoming_explicit_list(parser);
        if !keep_for_explicit_list {
            return Ok(None);
        }
    }
    // L8.5: a break before an own-line render-interior closer vanishes so
    // the interior tail keeps its paragraph shape (live shows no break).
    if parser.settings().layout.legacy() {
        let closer_ahead = match next.1 {
            Some(Token::LeftBlockEnd) => {
                parser.look_ahead(0).map(|token| token.span.start)
            }
            Some(Token::Whitespace) => parser
                .look_ahead(1)
                .filter(|token| token.token == Token::LeftBlockEnd)
                .map(|token| token.span.start),
            _ => None,
        };
        if let Some(closer_byte) = closer_ahead
            && wikidot_render_interior_close_kind_at(parser, closer_byte).is_some()
            && !render_interior_close_line_is_quoted(
                parser.full_text().inner(),
                closer_byte,
            )
        {
            parser.step()?;
            return Ok(Some(Elements::None));
        }
    }

    let next_offset = if matches!(
        parser.look_ahead(0).map(|token| token.token),
        Some(Token::Whitespace)
    ) {
        1
    } else {
        0
    };
    let looks_skippable = parser.look_ahead(next_offset).is_some_and(|token| {
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
            Token::ClearFloatLeft | Token::ClearFloatRight | Token::ClearFloatBoth => {
                !parser.settings().layout.legacy() || {
                    let source = parser.full_text().inner();
                    let line_end = source[token.span.end..]
                        .find(['\r', '\n'])
                        .map_or(source.len(), |offset| token.span.end + offset);
                    token.span.end == line_end
                }
            }
            _ => true,
        };
        starts_own_line_rule(token.token)
            && valid_shape
            && !(next_offset == 1
                && matches!(
                    token.token,
                    Token::Quote | Token::BulletItem | Token::NumberedItem
                ))
            // L85 scope-5: live keeps the break before a table in item
            // bodies (li-table-contained).
            && !(parser.settings().layout.legacy()
                && parser.wikidot_li_inside_item_body()
                && matches!(
                    token.token,
                    Token::TableColumn
                        | Token::TableColumnRight
                        | Token::TableColumnCenter
                        | Token::TableColumnTitle
                ))
    });
    // A marker line owns its break only when the list really forms: a list
    // live rejects stays literal, and live keeps the break in front of it.
    let skip = if looks_skippable
        && matches!(
            parser.look_ahead(next_offset).map(|token| token.token),
            Some(Token::BulletItem | Token::NumberedItem)
        ) {
        crate::parsing::rule::impls::line_break_skipped_for_own_line_block(parser)
    } else {
        looks_skippable
    };

    parser.step()?;
    if skip {
        Ok(Some(Elements::None))
    } else {
        Ok(Some(Element::LineBreak.into()))
    }
}

fn wikidot_upcoming_explicit_list<'r: 't, 't>(parser: &Parser<'r, 't>) -> bool {
    let mut probe = parser.clone();
    if probe.step().is_err() {
        return false;
    }
    if probe.current().token != Token::LeftBlock {
        return false;
    }
    probe.get_block_name(false).is_ok_and(|(name, _, segment)| {
        let view = name.strip_suffix('_').unwrap_or(&name);
        (view.eq_ignore_ascii_case("ol") || view.eq_ignore_ascii_case("ul"))
            && probe.head_word_view_is_actionable(
                &name,
                matches!(name, Cow::Borrowed(_)),
                segment,
                view,
            )
    })
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

fn step_over_wikidot_designated_end<'r, 't>(parser: &mut Parser<'r, 't>) -> Option<bool> {
    if parser.current().token != Token::LeftBlockEnd {
        return None;
    }
    parser.step().ok()?;
    loop {
        match parser.current().token {
            Token::RightBlock => {
                parser.step().ok()?;
                return Some(false);
            }
            Token::RightLink => {
                parser.step().ok()?;
                return Some(true);
            }
            Token::InputEnd | Token::ParagraphBreak => return None,
            _ => {
                parser.step().ok()?;
            }
        }
    }
}

fn try_consume_past_wikidot_designated_iftags_end<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Option<ParseResult<'r, 't, Elements<'t>>> {
    if parser.discarding_hidden_body() {
        return None;
    }
    let designated = parser.wikidot_iftags_body_end()?;
    if parser.current().token != Token::LeftBlockEnd
        || parser.current().span.start != designated
    {
        return None;
    }
    let mut probe = parser.clone();
    let fused = step_over_wikidot_designated_end(&mut probe)?;
    step_over_wikidot_designated_end(parser)?;
    Some(consume_past_iftags_end(parser, fused))
}

fn consume_past_iftags_end<'r, 't>(
    parser: &mut Parser<'r, 't>,
    fused: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    match consume(parser) {
        Ok(mut success) => {
            if fused {
                let mut elements = vec![Element::Text(Cow::Borrowed("]"))];
                elements.extend(success.item.as_ref().iter().cloned());
                success.item = Elements::Multiple(elements);
                if let Some(split) = &mut success.paragraph_split
                    && let Some(first) = split.first_mut()
                {
                    first.0 += 1;
                }
            }
            Ok(success)
        }
        Err(error) => {
            if error.kind() == ParseErrorKind::EndOfInput {
                let item = if fused {
                    Elements::Single(Element::Text(Cow::Borrowed("]")))
                } else {
                    Elements::None
                };
                Ok(ParseSuccess::new(item, Vec::new(), true))
            } else {
                Err(error)
            }
        }
    }
}

fn try_consume_pending_iftags_end_swallow<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Option<ParseResult<'r, 't, Elements<'t>>> {
    if parser.discarding_hidden_body() {
        return None;
    }
    if parser.current().token != Token::LeftBlockEnd {
        return None;
    }
    let position = parser.current().span.start;
    if !parser.has_pending_iftags_end_swallow(position) {
        return None;
    }
    let mut probe = parser.clone();
    let fused = step_over_wikidot_designated_end(&mut probe)?;
    if !parser.take_pending_iftags_end_swallow(position) {
        return None;
    }
    step_over_wikidot_designated_end(parser)?;
    Some(consume_past_iftags_end(parser, fused))
}

fn try_consume_discarded_row_closer<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Option<ParseResult<'r, 't, Elements<'t>>> {
    if !parser.settings().layout.legacy() || parser.discarding_hidden_body() {
        return None;
    }
    if parser.current().token != Token::LeftBlockEnd {
        return None;
    }
    let position = parser.current().span.start;
    if !parser.wikidot_discarded_row_closer(position) {
        return None;
    }
    let closer = parser.full_text().inner().get(position..).unwrap_or("");
    let eats_following_break =
        closer.len() >= 6 && closer.as_bytes()[..6].eq_ignore_ascii_case(b"[[/li]");
    step_over_wikidot_designated_end(parser)?;
    if parser.current().token == Token::LineBreak
        && parser.look_ahead(0).is_some_and(|token| {
            token.token == Token::LeftBlockEnd
                && parser.wikidot_discarded_row_closer(token.span.start)
        })
    {
        parser.step().ok()?;
    }
    if eats_following_break && parser.current().token == Token::LineBreak {
        parser.step().ok()?;
    }
    Some(Ok(crate::parsing::ParseSuccess::new(
        Elements::None,
        Vec::new(),
        true,
    )))
}

fn try_consume_wikidot_li_listbody_silent_close<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Option<ParseResult<'r, 't, Elements<'t>>> {
    if !parser.settings().layout.legacy() {
        return None;
    }
    if parser.discarding_hidden_body() {
        return None;
    }
    if parser.current().token != Token::LeftBlockEnd {
        return None;
    }
    if !matches!(parser.wikidot_li_collect_top(), Some(None)) {
        return None;
    }
    let position = parser.current().span.start;
    parser.exact_wikidot_li_close_end()?;
    if !parser.wikidot_li_listbody_close_silent(position) {
        return None;
    }
    step_over_wikidot_designated_end(parser)?;
    Some(Ok(crate::parsing::ParseSuccess::new(
        Elements::None,
        Vec::new(),
        true,
    )))
}

/// L85: parse an implicit nested list at a scan-marked `[[li]]` inside a
/// div/collapsible body (l-ul-li-cross). Falls through on any failure.
fn try_consume_wikidot_li_cross_implicit_list<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Option<ParseResult<'r, 't, Elements<'t>>> {
    if !parser.settings().layout.legacy() {
        return None;
    }
    if parser.discarding_hidden_body() {
        return None;
    }
    if parser.current().token != Token::LeftBlock {
        return None;
    }
    if !parser.wikidot_top_boundary_is_div_or_collapsible() {
        return None;
    }
    if !parser.wikidot_li_cross_implicit_li_at(parser.current().span.start) {
        return None;
    }
    let mut probe = parser.clone();
    let output = super::rule::impls::parse_wikidot_implicit_list_block(&mut probe);
    let output = output.ok()?;
    parser.update(&probe);
    Some(Ok(output))
}

/// L85: silently drop a scan-redundant list closer (`l-ul-li-closein` keeps
/// `EG` joined: the redundant `[[/li]]`/`[[/ul]]` vanish with one following
/// line break each).
fn try_consume_wikidot_li_cross_redundant_close<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Option<ParseResult<'r, 't, Elements<'t>>> {
    if !parser.settings().layout.legacy() {
        return None;
    }
    if parser.discarding_hidden_body() {
        return None;
    }
    if parser.current().token != Token::LeftBlockEnd {
        return None;
    }
    let position = parser.current().span.start;
    if !parser.wikidot_li_cross_redundant_at(position) {
        return None;
    }
    let mut close = parser.clone();
    if close.get_end_block().is_err() {
        return None;
    }
    parser.update(&close);
    if parser.current().token == Token::LineBreak {
        let _ = parser.step();
    }
    Some(Ok(crate::parsing::ParseSuccess::new(
        Elements::None,
        Vec::new(),
        true,
    )))
}

fn try_consume_wikidot_inline_marker<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<Elements<'t>>, ParseError> {
    let Some(role) = parser.wikidot_inline_marker_role() else {
        return Ok(None);
    };
    let element = match role {
        InlineMarkerRole::Open(kind) => {
            Element::Partial(PartialElement::InlineFormatOpen(kind))
        }
        InlineMarkerRole::Close(kind) => {
            Element::Partial(PartialElement::InlineFormatClose(kind))
        }
        InlineMarkerRole::ColorOpen { color, body_start } => {
            while parser.current().span.start < body_start {
                parser.step()?;
            }
            return Ok(Some(
                Element::Partial(PartialElement::InlineColorOpen(
                    Cow::Owned(color),
                    false,
                ))
                .into(),
            ));
        }
        InlineMarkerRole::ColorClose => {
            Element::Partial(PartialElement::InlineColorClose)
        }
        InlineMarkerRole::Literal => text!(parser.current().slice),
        InlineMarkerRole::Removed => {
            parser.step()?;
            return Ok(Some(Elements::None));
        }
    };
    parser.step()?;
    Ok(Some(element.into()))
}

pub fn consume<'r, 't>(parser: &mut Parser<'r, 't>) -> ParseResult<'r, 't, Elements<'t>> {
    if parser.prepare_quote_body_line()? == QuoteBodyLineStatus::Boundary {
        return Err(parser.make_err(ParseErrorKind::EndOfInput));
    }

    if let Some(result) = try_consume_past_wikidot_designated_iftags_end(parser) {
        return result;
    }

    if let Some(result) = try_consume_pending_iftags_end_swallow(parser) {
        return result;
    }

    if let Some(result) = try_consume_discarded_row_closer(parser) {
        return result;
    }

    if let Some(result) = try_consume_wikidot_li_listbody_silent_close(parser) {
        return result;
    }

    if let Some(result) = try_consume_wikidot_li_cross_redundant_close(parser) {
        return result;
    }

    if let Some(position) = parser.fused_name_residual()
        && parser.current().span.start >= position
    {
        parser.clear_fused_name_residual();
        return ok!(text!("]"));
    }

    if parser.settings().layout.legacy()
        && parser.current().token == Token::ParagraphBreak
    {
        parser.clear_wikidot_literal_triple_links();
    }

    parser.depth_increment()?;

    if let Some(result) = try_consume_wikidot_render_interior_close(parser)? {
        parser.depth_decrement();
        return result;
    }

    if let Some(elements) =
        try_consume_wikidot_fused_span_close_then_table_column(parser)?
    {
        parser.depth_decrement();
        return ok!(elements);
    }

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

    if let Some(elements) = try_consume_wikidot_end_block_link(parser)? {
        parser.depth_decrement();
        return ok!(elements);
    }

    if let Some(elements) = try_consume_line_break(parser)? {
        parser.depth_decrement();
        return ok!(elements);
    }

    if parser.settings().layout.legacy()
        && matches!(
            parser.current().token,
            Token::TableColumn
                | Token::TableColumnRight
                | Token::TableColumnCenter
                | Token::TableColumnTitle
        )
        && crate::parsing::rule::impls::wikidot_simple_table_markers_literal_on_line_after_explicit_li(
            parser,
            parser.current().span.start,
        )
    {
        let literal = parser.current().slice;
        parser.step()?;
        parser.depth_decrement();
        return ok!(text!(literal));
    }

    if let Some(elements) = try_consume_text_token(parser)? {
        parser.depth_decrement();
        return ok!(elements);
    }

    if let Some(elements) = try_consume_leaf_token(parser)? {
        parser.depth_decrement();
        return ok!(elements);
    }

    if let Some(result) = try_consume_wikidot_li_cross_implicit_list(parser) {
        parser.depth_decrement();
        return result;
    }

    let mut all_errors = Vec::new();
    let current = parser.current();

    for &rule in get_rules_for_token(current) {
        if rule.name() == "delayed-conditional"
            && parser.generated_until_right_block().is_empty()
        {
            continue;
        }
        if parser.in_wikidot_bibliography_body()
            && crate::parsing::rule::bibliography_order::line_rule_cannot_form_in_bibliography_body(
                rule.name(),
            )
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

                if error.kind() == ParseErrorKind::AnchorBlockWithInsufficientClosers {
                    continue;
                }

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
