/*
 * parsing/rule/impls/blockquote.rs
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

use super::footnote_first::{
    remove_first_footnote, starts_footnote, unowned_footnote_first,
};
use super::line_owner::{LineOwner, LostOwnerBlock, try_consume_lost_owner_block};
use super::prelude::*;
use crate::parsing::ParseSuccess;
use crate::parsing::paragraph::{
    ParagraphStack, collapsible_has_direct_literal_nested_opener,
};
use crate::parsing::parser::{QuoteBodyLineStatus, QuoteEscapedUnit};
use crate::parsing::{
    DepthItem, DepthList, ParagraphSplitEntry, ParagraphSplitKind, process_depths,
};
use crate::tree::{
    AttributeMap, Container, ContainerType, ListItem, ListType, PartialElement,
};

const MAX_BLOCKQUOTE_DEPTH: usize = 128;

#[derive(Debug)]
struct NativeQuoteRow<'t> {
    elements: Vec<Element<'t>>,
    paragraph_safe: bool,
    paragraph_split: Option<Vec<ParagraphSplitEntry>>,
    empty_spaced: bool,
    indent: usize,
    source_line: Option<&'t str>,
    cut_boundary: bool,
    keep_trailing_break: bool,
    marker_final: bool,
    residual_block: bool,
}

struct QuoteFlatSplit<'t> {
    close: Element<'t>,
    rest: Vec<Element<'t>>,
    rows: Vec<(usize, NativeQuoteRow<'t>)>,
    base_depth: usize,
    /// The closed row's own trailing break. Live keeps it when the flat close
    /// ends the quoted run (no quoted content follows the closer), giving
    /// `</div><br>` before the following unquoted line.
    trailing_break: bool,
}

fn trim_wikidot_flat_close_rest(rest: &mut Vec<Element>) {
    while let Some(Element::Text(text)) = rest.first_mut() {
        if text.chars().all(|c| c == ' ' || c == '\t') {
            rest.remove(0);
            continue;
        }
        let trimmed = text.trim_start_matches([' ', '\t']);
        if trimmed.len() == text.len() {
            break;
        }
        if trimmed.is_empty() {
            rest.remove(0);
        } else {
            *text = trimmed.to_owned().into();
        }
        break;
    }
}

fn find_external_flat_close(
    parser: &Parser<'_, '_>,
    elements: &[Element],
) -> Option<usize> {
    elements.iter().position(|element| {
        let Element::Partial(PartialElement::WikidotBlockClose(kind)) = element else {
            return false;
        };
        parser.wikidot_flat_close_splits_quote_run(kind.head_byte())
    })
}

fn build_flat_split_segments<'t>(
    split: QuoteFlatSplit<'t>,
    wikidot: bool,
) -> Vec<(Vec<Element<'t>>, bool)> {
    let mut segments: Vec<(Vec<Element<'t>>, bool)> = Vec::new();
    let mut bare: Vec<Element<'t>> = vec![split.close];
    bare.extend(split.rest);
    let mut bare_extended_by_rows = false;
    // L8.5: a bare batch still carrying the closer (same-row rest)
    // stays bare, but a fresh batch after a flush boundary (spaced
    // blank, quote run) is a closed paragraph so following text
    // cannot join its chunk.
    let flush_bare = |bare: &mut Vec<Element<'t>>,
                      segments: &mut Vec<(Vec<Element<'t>>, bool)>| {
        if bare.is_empty() {
            return;
        }
        let fresh = !matches!(
            bare.first(),
            Some(Element::Partial(PartialElement::WikidotBlockClose(_)))
        );
        let batch = std::mem::take(bare);
        if fresh && batch.iter().all(Element::paragraph_safe) {
            segments.push((
                vec![Element::Container(Container::new(
                    ContainerType::Paragraph,
                    batch,
                    AttributeMap::new(),
                ))],
                false,
            ));
        } else {
            segments.push((batch, false));
        }
    };
    let mut quote_run: Vec<(usize, (), NativeQuoteRow<'t>)> = Vec::new();
    let flush_quote_run =
        |quote_run: &mut Vec<(usize, (), NativeQuoteRow<'t>)>,
         segments: &mut Vec<(Vec<Element<'t>>, bool)>| {
            if quote_run.is_empty() {
                return;
            }
            let run = std::mem::take(quote_run);
            let depth_lists = process_depths((), run);
            let mut elements: Vec<Element> = depth_lists
                .into_iter()
                .filter_map(|(_, depth_list)| {
                    build_blockquote_element(depth_list, wikidot)
                })
                .collect();
            if !elements.is_empty() {
                segments.push((elements, false));
            }
        };
    for (depth, row) in split.rows {
        let rerouted = depth.saturating_sub(split.base_depth);
        if rerouted > 0 {
            flush_bare(&mut bare, &mut segments);
            quote_run.push((rerouted - 1, (), row));
            continue;
        }
        flush_quote_run(&mut quote_run, &mut segments);
        if row.empty_spaced {
            flush_bare(&mut bare, &mut segments);
            continue;
        }
        if row.elements.is_empty() {
            continue;
        }
        if !bare.is_empty() {
            bare.push(Element::LineBreak);
        }
        bare.extend(row.elements);
        bare_extended_by_rows = true;
    }
    flush_quote_run(&mut quote_run, &mut segments);
    if split.trailing_break && !bare_extended_by_rows {
        bare.push(Element::LineBreak);
    }
    flush_bare(&mut bare, &mut segments);
    segments
}

fn consume_deeper_collapsible_close<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<bool, ParseError>
where
    'r: 't,
{
    if !parser.settings().layout.legacy()
        || !parser.in_wikidot_collapsible()
        || parser.native_blockquote_depth().is_none()
        || parser.quote_body_cursor().is_none()
        || parser.current().token != Token::Quote
    {
        return Ok(false);
    }

    let mut close = parser.clone();
    while close.current().token == Token::Quote {
        close.step()?;
        close.get_optional_space()?;
    }
    if !close
        .get_end_block()
        .is_ok_and(|name| name.eq_ignore_ascii_case("collapsible"))
    {
        return Ok(false);
    }
    close.get_optional_space()?;
    if !matches!(
        close.current().token,
        Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
    ) {
        return Ok(false);
    }

    parser.update(&close);
    parser.set_wikidot_collapsible_closed_at_deeper_quote(true);
    Ok(true)
}

fn consumed_deeper_collapsible_close(
    parser: &Parser<'_, '_>,
    start: usize,
    end: usize,
    required_depth: usize,
) -> bool {
    parser.full_text().inner()[start..end].lines().any(|line| {
        let quoted = line.bytes().take_while(|byte| *byte == b'>').count();
        quoted > required_depth
            && line[quoted..]
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("[[/collapsible]]")
    })
}

fn consumed_unquoted_collapsible_close(
    parser: &Parser<'_, '_>,
    start: usize,
    end: usize,
) -> bool {
    parser.full_text().inner()[start..end].lines().any(|line| {
        line.trim_start()
            .to_ascii_lowercase()
            .starts_with("[[/collapsible]]")
            && !line.trim_start().starts_with('>')
    })
}

pub const RULE_BLOCKQUOTE: Rule = Rule {
    name: "blockquote",
    position: LineRequirement::StartOfLine,
    try_consume_fn,
};

struct VirtualGateQuote {
    tight: bool,
    strict_line_start: bool,
}

fn virtual_gate_quote_leader(parser: &Parser<'_, '_>) -> Option<VirtualGateQuote> {
    if parser.current().token != Token::Quote {
        return None;
    }
    let source = parser.full_text().inner();
    let scan = scan_virtual_gate_line(source, parser.current().span.start);
    if scan.gates == 0 {
        return None;
    }
    let mut probe = parser.clone();
    while probe.current().token == Token::Quote {
        if probe.step().is_err() {
            break;
        }
    }
    let tight = !matches!(
        probe.current().token,
        Token::Whitespace | Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
    );
    Some(VirtualGateQuote {
        tight,
        strict_line_start: scan.strict_line_start,
    })
}

struct VirtualGateLineScan {
    gates: usize,
    strict_line_start: bool,
}

fn scan_virtual_gate_line(source: &str, quote_start: usize) -> VirtualGateLineScan {
    let bytes = source.as_bytes();
    let mut pos = quote_start.min(bytes.len());
    let mut gates = 0usize;
    let mut strict = true;
    let before_quote = pos;
    while pos > 0
        && (bytes[pos - 1] == b' ' || bytes[pos - 1] == b'\t' || bytes[pos - 1] == b'\0')
    {
        pos -= 1;
    }
    if pos != before_quote {
        strict = false;
    }
    while let Some(open_start) = match_iftags_gate_backward(source, pos) {
        gates += 1;
        pos = open_start;
        let before_ws = pos;
        while pos > 0
            && (bytes[pos - 1] == b' '
                || bytes[pos - 1] == b'\t'
                || bytes[pos - 1] == b'\0')
        {
            pos -= 1;
        }
        if match_iftags_gate_backward(source, pos).is_none() {
            break;
        }
        if pos != before_ws {
            strict = false;
        }
    }
    let at_line_start = pos == 0
        || (pos <= bytes.len() && (bytes[pos - 1] == b'\n' || bytes[pos - 1] == b'\r'));
    VirtualGateLineScan {
        gates,
        strict_line_start: strict && at_line_start,
    }
}

fn match_iftags_gate_backward(source: &str, pos: usize) -> Option<usize> {
    let close_start = match_iftags_close_backward(source, pos)?;
    let mut depth = 1usize;
    let mut search = close_start;
    while depth > 0 {
        let open_at = source[..search].rfind("[[")?;
        if is_iftags_close_at(source, open_at) {
            depth += 1;
            search = open_at;
        } else if is_iftags_open_at(source, open_at) {
            depth -= 1;
            search = open_at;
        } else {
            return None;
        }
    }
    Some(search)
}

fn match_iftags_close_backward(source: &str, pos: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if pos < "]]".len() || bytes[pos - "]]".len()..pos] != *b"]]" {
        return None;
    }
    let mut inner_end = pos - "]]".len();
    while inner_end > 0 && (bytes[inner_end - 1] == b' ' || bytes[inner_end - 1] == b'\t')
    {
        inner_end -= 1;
    }
    if inner_end < "iftags".len()
        || !bytes[inner_end - "iftags".len()..inner_end].eq_ignore_ascii_case(b"iftags")
    {
        return None;
    }
    let mut inner_start = inner_end - "iftags".len();
    while inner_start > 0
        && (bytes[inner_start - 1] == b' ' || bytes[inner_start - 1] == b'\t')
    {
        inner_start -= 1;
    }
    if inner_start < "[[/".len()
        || bytes[inner_start - "[[/".len()..inner_start] != *b"[[/"
    {
        return None;
    }
    Some(inner_start - "[[/".len())
}

fn is_iftags_close_at(source: &str, at: usize) -> bool {
    let Some(rest) = source.get(at + "[[".len()..) else {
        return false;
    };
    let rest = rest.trim_start_matches([' ', '\t']);
    let Some(after_slash) = rest.strip_prefix('/') else {
        return false;
    };
    let after_slash = after_slash.trim_start_matches([' ', '\t']);
    let bytes = after_slash.as_bytes();
    bytes.len() > "iftags".len()
        && bytes[..6].eq_ignore_ascii_case(b"iftags")
        && !bytes[6].is_ascii_alphanumeric()
        && bytes[6] != b'_'
}

fn is_iftags_open_at(source: &str, at: usize) -> bool {
    let Some(rest) = source.get(at + "[[".len()..) else {
        return false;
    };
    if rest.starts_with('/') {
        return false;
    }
    let rest = rest.trim_start_matches([' ', '\t']);
    if rest.starts_with('/') {
        return false;
    }
    let bytes = rest.as_bytes();
    bytes.len() > "iftags".len()
        && bytes[..6].eq_ignore_ascii_case(b"iftags")
        && !bytes[6].is_ascii_alphanumeric()
        && bytes[6] != b'_'
}

struct MidlineHoist<'r, 't> {
    prefix: Vec<Element<'t>>,
    prefix_errors: Vec<ParseError>,
    committed: Parser<'r, 't>,
    lost: LostOwnerBlock<'t>,
}

/// An explicit `[[ul]]` or `[[ol]]` that appears after other content on a
/// quote line, with an unowned body, ends the quote and hoists out, the same
/// as when it starts the line. The prefix is either inline content (the quote
/// keeps it as a bare row) or a bare native list marker (live drops both
/// shells). Returns the collected prefix and the parser state after the
/// hoisted list, or None when the line is not this shape.
fn try_midline_lost_owner_list<'r, 't>(
    parser: &Parser<'r, 't>,
    absolute_depth: usize,
    physical_depth: usize,
) -> Option<MidlineHoist<'r, 't>>
where
    'r: 't,
{
    let source = parser.full_text().inner();
    let mut prefix_kinds: Vec<Token> = Vec::new();
    let mut head_index = None;
    let mut head_start = 0;
    for (index, token) in std::iter::once(parser.current())
        .chain(parser.remaining().iter())
        .enumerate()
    {
        match token.token {
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => break,
            Token::LeftBlock => {
                head_index = Some(index);
                head_start = token.span.start;
                break;
            }
            kind => prefix_kinds.push(kind),
        }
    }
    let Some(head_index) = head_index else { return None; };
    if head_index == 0 {
        return None;
    }
    let marker_only = prefix_kinds.len() <= 2
        && matches!(
            prefix_kinds.first(),
            Some(Token::BulletItem | Token::NumberedItem)
        )
        && prefix_kinds[1..]
            .iter()
            .all(|kind| *kind == Token::Whitespace);
    if !marker_only
        && (prefix_kinds.iter().all(|kind| *kind == Token::Whitespace)
            || prefix_kinds.iter().any(|kind| {
                matches!(
                    kind,
                    Token::Quote
                        | Token::BulletItem
                        | Token::NumberedItem
                        | Token::Heading
                        | Token::Colon
                )
            }))
    {
        return None;
    }

    let mut head_parser = parser.clone();
    for _ in 0..head_index {
        head_parser.step().ok()?;
    }
    let (name, _, _) = head_parser.clone().get_block_name(false).ok()?;
    let view = name.strip_suffix('_').unwrap_or(&name);
    if !view.eq_ignore_ascii_case("ul") && !view.eq_ignore_ascii_case("ol") {
        return None;
    }
    let after_head_line = &source[head_start.min(source.len())..];
    let line_end = after_head_line.find('\n')?;
    let next_line = after_head_line[line_end + 1..].lines().next()?;
    if !next_line.trim_start().starts_with("[[") {
        return None;
    }

    let mut prefix = Vec::new();
    let mut prefix_errors = Vec::new();
    if !marker_only {
        let mut collect_parser = parser.clone();
        collect_parser.set_native_blockquote_depth(Some(absolute_depth));
        let close = [
            ParseCondition::current(Token::LineBreak),
            ParseCondition::current(Token::ParagraphBreak),
            ParseCondition::current(Token::InputEnd),
            ParseCondition::current(Token::LeftBlock),
        ];
        let Ok(result) = collect_native_blockquote_line(&mut collect_parser, &close) else { return None; };
        let line = result.item;
        let collect_errors = result.errors;
        if !line.escaped_units.is_empty() {
            return None;
        }
        prefix = line.elements;
        prefix_errors = collect_errors;
        while matches!(prefix.last(), Some(Element::Text(text)) if text.chars().all(|c| c.is_whitespace())) {
            prefix.pop();
        }
        if let Some(Element::Text(text)) = prefix.last_mut() {
            let trimmed = text.trim_end_matches([' ', '\t']);
            if trimmed.len() < text.len() {
                if trimmed.is_empty() {
                    prefix.pop();
                } else {
                    *text = trimmed.to_owned().into();
                }
            }
        }
        if prefix.is_empty()
            || prefix
                .iter()
                .any(|element| !element.paragraph_safe() || matches!(element, Element::LineBreak))
        {
            return None;
        }
    }

    head_parser.mark_virtual_start_of_line();
    let lost = try_consume_lost_owner_block(
        &mut head_parser,
        LineOwner::Quote {
            depth: physical_depth,
        },
    )
    .ok()??;
    if !lost.hoisted_list {
        return None;
    }
    Some(MidlineHoist {
        prefix,
        prefix_errors,
        committed: head_parser,
        lost,
    })
}

/// Whether the token stream after a hoisted list begins a block construct on
/// the next line. Live keeps the list's line break only before a block; inline
/// text on the next line joins the list without one.
fn next_line_starts_block(parser: &Parser<'_, '_>) -> bool {
    if parser.current().token != Token::LineBreak {
        return false;
    }
    parser.look_ahead(0).is_some_and(|token| {
        matches!(
            token.token,
            Token::LeftBlock
                | Token::Quote
                | Token::Heading
                | Token::BulletItem
                | Token::NumberedItem
                | Token::Colon
        )
    })
}

fn following_quote_line_is_prose<'r, 't>(parser: &Parser<'r, 't>) -> bool
where
    'r: 't,
{
    let mut probe = parser.clone();
    while probe.current().token == Token::Quote {
        if probe.step().is_err() {
            return false;
        }
        if probe.get_optional_space().is_err() {
            return false;
        }
    }
    let start = probe.current().span.start;
    let end = std::iter::once(probe.current())
        .chain(probe.remaining())
        .find(|token| {
            matches!(
                token.token,
                Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
            )
        })
        .map(|token| token.span.start)
        .unwrap_or(start);
    if end <= start {
        return false;
    }
    let text = &probe.full_text().inner()[start..end];
    !text.contains('[') && !text.contains(']')
}

fn append_prose_to_tab_segment<'t>(
    segments: &mut [(Vec<Element<'t>>, bool)],
    line: Vec<Element<'t>>,
) {
    let mut inline = Vec::new();
    for element in line {
        match element {
            Element::Container(container)
                if container.ctype() == ContainerType::Paragraph =>
            {
                inline.extend(Vec::<Element<'t>>::from(container));
            }
            other => inline.push(other),
        }
    }
    while matches!(
        inline.last(),
        Some(Element::LineBreak | Element::LineBreaks(_))
    ) {
        inline.pop();
    }
    if inline.is_empty() {
        return;
    }
    let Some(index) =
        segments
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, (elements, _))| {
                segment_ends_in_blockquote(elements).then_some(index)
            })
    else {
        return;
    };
    let Some(target) = innermost_blockquote_elements(&mut segments[index].0) else {
        return;
    };
    target.push(Element::LineBreak);
    target.extend(inline);
}

fn segment_ends_in_blockquote(elements: &[Element<'_>]) -> bool {
    matches!(
        elements.last(),
        Some(Element::Container(container))
            if container.ctype() == ContainerType::Blockquote
    )
}

fn innermost_blockquote_elements<'a, 't>(
    elements: &'a mut Vec<Element<'t>>,
) -> Option<&'a mut Vec<Element<'t>>> {
    if !segment_ends_in_blockquote(elements) {
        return None;
    }
    let Element::Container(container) = elements.last_mut()? else {
        return None;
    };
    let nested = container.elements_mut();
    if segment_ends_in_blockquote(nested) {
        return innermost_blockquote_elements(nested);
    }
    Some(nested)
}

/// Whether the migrated list-end break of a quote whose explicit list closed
/// at a shallower quote depth should be dropped before a following line.
/// Live keeps the break at the end of input, before a blank line and before
/// a following quote line, but drops it when the next line carries content:
/// the content then starts a fresh paragraph (or forms its own block)
/// instead of continuing bare after the break.
fn migrated_break_dropped_before_following_line(parser: &Parser<'_, '_>) -> bool {
    !matches!(
        parser.current().token,
        Token::LineBreak | Token::ParagraphBreak | Token::InputEnd | Token::Quote
    )
}

/// What live does with the migrated break when an explicit list inside a
/// blockquote boundary-closes and a blank line precedes the follower.
///
/// Determined from saved-page probes (sandbox-for-codex,
/// muse-probe-qcl-s3* / muse-probe-qcl-v7*):
/// - `Paragraph`: the following paragraph forms and live renders the break
///   as its leading element (`<p><br>x</p>`).
/// - `Suppress`: a block forms instead and live drops the break entirely
///   (`<hr>`, `<h1>`, the code block, the head-alone div block).
/// - `Keep`: live keeps the break (or the ordered degraded shell) outside
///   the follower (EOF, a following quote line, a new list, an image).
enum BlankFollowerLead {
    Paragraph,
    Suppress,
    Keep,
}

/// See the `BlankFollowerLead` documentation for the evidence.
fn migrated_break_follower_after_blank_line(
    parser: &Parser<'_, '_>,
) -> BlankFollowerLead {
    if !matches!(
        parser.current().token,
        Token::LineBreak | Token::ParagraphBreak
    ) {
        return BlankFollowerLead::Keep;
    }
    let remaining = parser.remaining();
    let mut index = 0;
    let first = loop {
        let Some(token) = remaining.get(index) else {
            return BlankFollowerLead::Keep;
        };
        match token.token {
            Token::Whitespace | Token::LineBreak | Token::ParagraphBreak => index += 1,
            _ => break index,
        }
    };
    // `1. q` opens a new ordered list rather than a paragraph.
    if remaining[first].token == Token::Identifier
        && remaining[first].slice.chars().all(|c| c.is_ascii_digit())
        && let Some(dot) = remaining.get(first + 1)
        && dot.token == Token::Other
        && dot.slice == "."
    {
        return BlankFollowerLead::Keep;
    }
    match remaining[first].token {
        Token::InputEnd
        | Token::Quote
        | Token::LeftBlockStar
        | Token::LeftBlockEnd
        | Token::BulletItem
        | Token::NumberedItem
        | Token::TableColumn
        | Token::TableColumnRight
        | Token::TableColumnCenter
        | Token::TableColumnTitle
        | Token::LeftComment
        | Token::DoubleDash
        | Token::Equals
        | Token::Colon
        | Token::Pipe => BlankFollowerLead::Keep,
        Token::TripleDash | Token::Heading => BlankFollowerLead::Suppress,
        Token::LeftBlock => {
            let Some(identifier) = remaining.get(first + 1) else {
                return BlankFollowerLead::Keep;
            };
            if identifier.token != Token::Identifier {
                return BlankFollowerLead::Keep;
            }
            let name = identifier.slice;
            if name.eq_ignore_ascii_case("span") || name.eq_ignore_ascii_case("tabview") {
                return BlankFollowerLead::Paragraph;
            }
            if name.eq_ignore_ascii_case("code") {
                return BlankFollowerLead::Suppress;
            }
            if name.eq_ignore_ascii_case("image") {
                return BlankFollowerLead::Keep;
            }
            if name.eq_ignore_ascii_case("div") || name.eq_ignore_ascii_case("note") {
                // Same-line content forms literal inline text inside the
                // paragraph (muse-probe-qcl-s3blankdiv / s3blanknote); a
                // head-alone div forms a block and live drops the break
                // (muse-probe-qcl-s3blankdivalone). A head-alone note is
                // unprobed and stays Keep.
                let mut cursor = first + 1;
                let mut head_end = None;
                while let Some(token) = remaining.get(cursor) {
                    if token.token == Token::RightBlock {
                        head_end = Some(cursor + 1);
                        break;
                    }
                    cursor += 1;
                }
                let Some(mut cursor) = head_end else {
                    return BlankFollowerLead::Keep;
                };
                while let Some(token) = remaining.get(cursor) {
                    match token.token {
                        Token::Whitespace => cursor += 1,
                        Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                            return if name.eq_ignore_ascii_case("div") {
                                BlankFollowerLead::Suppress
                            } else {
                                BlankFollowerLead::Keep
                            };
                        }
                        _ => return BlankFollowerLead::Paragraph,
                    }
                }
                BlankFollowerLead::Keep
            } else {
                BlankFollowerLead::Keep
            }
        }
        _ => BlankFollowerLead::Paragraph,
    }
}

/// The degraded trailing shell an ordered list closed at a shallower quote
/// depth leaves behind live: a bullet list holding only one
/// list-style-none item with a break (the a5e3e034c degraded form). At the
/// end of input it has no closing tags; before a following quoted line live
/// closes it between the two blockquotes. An unordered list leaves a plain
/// break instead.
fn ordered_boundary_close_shell(closed: bool) -> Element<'static> {
    let mut item_attributes = AttributeMap::new();
    item_attributes.insert("style", cow!("list-style: none"));
    Element::List {
        ltype: ListType::Bullet,
        items: vec![ListItem::Elements {
            elements: vec![Element::LineBreak],
            attributes: item_attributes,
        }],
        attributes: AttributeMap::new(),
        wikidot_explicit_block: false,
        wikidot_unclosed: !closed,
    }
}

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    let _ = parser.take_quote_escaped_units();
    if consume_deeper_collapsible_close(parser)? {
        return ok!(false; Elements::None);
    }

    if parser.wikidot_footnote_zone_blocks(parser.current().span.start) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let mut depths = Vec::new();
    let mut gate_escaped_segments: Vec<(Vec<Element<'t>>, bool)> = Vec::new();
    let mut pending_rows: Vec<(usize, Vec<Element<'t>>, bool)> = Vec::new();
    let mut escaped_rows = Vec::new();
    let mut escaped_elements = Vec::new();
    let mut errors = Vec::new();
    let mut consumed_pruned_row = false;
    let mut quote_run_active = true;
    let mut fold_following_into_tab_segment = false;
    let mut flatten_following_rows = false;
    let mut append_unquoted_close_break = false;
    let mut append_from_list_boundary = false;
    let mut append_from_ordered_list_boundary = false;
    let mut flat_split: Option<QuoteFlatSplit<'t>> = None;
    let mut first_row_depth: Option<usize> = None;

    if parser.settings().layout.legacy()
        && let Some(gate_quote) = virtual_gate_quote_leader(parser)
    {
        if gate_quote.tight {
            if gate_quote.strict_line_start {
                while !matches!(
                    parser.current().token,
                    Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
                ) {
                    parser.step()?;
                }
                return ok!(false; Elements::None, errors);
            }
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        } else if parser.native_blockquote_depth().is_some()
            || parser.quote_body_cursor().is_some()
        {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
    }

    while quote_run_active
        && parser.prepare_quote_body_line()? != QuoteBodyLineStatus::Boundary
        && parser.current().token == Token::Quote
    {
        if consume_deeper_collapsible_close(parser)? {
            break;
        }
        if fold_following_into_tab_segment && !following_quote_line_is_prose(parser) {
            break;
        }
        let current = parser.current();

        let physical_depth = current.slice.len();
        let (depth, absolute_depth) = parser.native_blockquote_depths(physical_depth);
        debug_assert!(depth > 0, "residual quote depth must be positive");
        parser.step()?;
        let single_space_after_marker = parser.current().slice == " ";
        let spaced_after_marker = parser.current().token == Token::Whitespace;
        let pre_space = parser.clone();
        let leave_indented_whitespace = parser.settings().layout.legacy()
            && spaced_after_marker
            && !single_space_after_marker
            && parser.look_ahead(0).is_some_and(|token| {
                !matches!(
                    token.token,
                    Token::Quote
                        | Token::LineBreak
                        | Token::ParagraphBreak
                        | Token::InputEnd
                        | Token::BulletItem
                        | Token::NumberedItem
                )
            });
        parser.get_optional_space()?;
        if parser.current().token != Token::Quote {
            parser.mark_virtual_start_of_line();
        }
        let content_start = parser.current().span.start;
        let row_indent = quote_row_indent(parser.full_text().inner(), content_start);

        if absolute_depth > MAX_BLOCKQUOTE_DEPTH {
            return Err(parser.make_err(ParseErrorKind::BlockquoteDepthExceeded));
        }

        if parser.settings().layout.legacy()
            && let Some(lost) = try_consume_lost_owner_block(
                parser,
                LineOwner::Quote {
                    depth: physical_depth,
                },
            )?
        {
            errors.extend(lost.errors);
            if let Some(control) = lost.control {
                depths.push((
                    depth - 1,
                    (),
                    NativeQuoteRow {
                        elements: vec![control],
                        paragraph_safe: false,
                        paragraph_split: None,
                        empty_spaced: false,
                        indent: row_indent,
                        source_line: None,
                        cut_boundary: false,
                        keep_trailing_break: false,
                        marker_final: false,
                        residual_block: false,
                    },
                ));
            } else {
                consumed_pruned_row = true;
            }
            if lost.hoisted_list
                && let Some((_, _, row)) = depths.last_mut()
            {
                // A list hoisted out of the quote interrupts the row paragraph:
                // live leaves the preceding row bare with its line break.
                row.paragraph_safe = false;
                row.keep_trailing_break = true;
                if !matches!(row.elements.last(), Some(Element::LineBreak)) {
                    row.elements.push(Element::LineBreak);
                }
            }
            escaped_elements.extend(lost.body);
            if lost.append_unquoted_close_break
                || lost.hoisted_list && next_line_starts_block(parser)
            {
                // A following quote line keeps this break in live even when
                // the element after it would absorb a plain post-list break
                // (a second hoisted list); the counted form is not absorbed.
                if lost.hoisted_list
                    && parser.current().token == Token::LineBreak
                    && parser
                        .look_ahead(0)
                        .is_some_and(|token| token.token == Token::Quote)
                {
                    escaped_elements
                        .push(Element::LineBreaks(std::num::NonZeroU32::new(1).unwrap()));
                } else {
                    escaped_elements.push(Element::LineBreak);
                }
            }
            break;
        }

        if leave_indented_whitespace {
            parser.update(&pre_space);
        }

        let close_conditions = [
            ParseCondition::current(Token::LineBreak),
            ParseCondition::current(Token::ParagraphBreak),
            ParseCondition::current(Token::InputEnd),
        ];
        let close = &close_conditions;
        let mut paragraph_safe = true;
        let original_depth = parser.native_blockquote_depth();
        let (physical_line_end, line_text_end, ends_quote_run) =
            std::iter::once(parser.current())
                .chain(parser.remaining().iter())
                .find(|token| {
                    matches!(
                        token.token,
                        Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
                    )
                })
                .map(|token| {
                    (
                        token.span.end,
                        token.span.start,
                        token.token == Token::ParagraphBreak,
                    )
                })
                .expect("tokenization always ends with input-end");
        if parser.settings().layout.legacy()
            && let Some(midline) =
                try_midline_lost_owner_list(parser, absolute_depth, physical_depth)
        {
            let MidlineHoist {
                prefix,
                prefix_errors,
                committed,
                lost,
            } = midline;
            errors.extend(prefix_errors);
            errors.extend(lost.errors);
            if lost.hoisted_list
                && let Some((_, _, row)) = depths.last_mut()
            {
                row.paragraph_safe = false;
                row.keep_trailing_break = true;
                if !matches!(row.elements.last(), Some(Element::LineBreak)) {
                    row.elements.push(Element::LineBreak);
                }
            }
            if prefix.is_empty() {
                consumed_pruned_row = true;
            } else {
                depths.push((
                    depth - 1,
                    (),
                    NativeQuoteRow {
                        elements: prefix,
                        paragraph_safe: false,
                        paragraph_split: None,
                        empty_spaced: false,
                        indent: row_indent,
                        source_line: None,
                        cut_boundary: false,
                        keep_trailing_break: false,
                        marker_final: false,
                        residual_block: false,
                    },
                ));
            }
            parser.update(&committed);
            escaped_elements.extend(lost.body);
            if lost.append_unquoted_close_break
                || lost.hoisted_list && next_line_starts_block(parser)
            {
                if lost.hoisted_list
                    && parser.current().token == Token::LineBreak
                    && parser
                        .look_ahead(0)
                        .is_some_and(|token| token.token == Token::Quote)
                {
                    escaped_elements
                        .push(Element::LineBreaks(std::num::NonZeroU32::new(1).unwrap()));
                } else {
                    escaped_elements.push(Element::LineBreak);
                }
            }
            break;
        }

        parser.clear_wikidot_quote_boundary_list_close();
        let _ = parser.take_wikidot_quote_boundary_li_auto_close();
        parser.set_native_blockquote_depth(Some(absolute_depth));
        let result = collect_native_blockquote_line(parser, close);
        parser.set_native_blockquote_depth(original_depth);
        let errors_before = errors.len();
        let ParseSuccess {
            item: line,
            errors: line_errors,
            paragraph_safe: line_safe,
            ..
        } = result?;
        errors.extend(line_errors);
        paragraph_safe &= line_safe;
        // BRIEF-QR45 part (b): the fresh run's paired `[[ /ul]]`/`[[ /ol]]`
        // row renders as an empty-list-style break, not literal text. The
        // pending flag survives across non-close rows (v2m `> [[li]]b` first)
        // and is consumed by the first matching close row.
        let row_source = parser
            .full_text()
            .inner()
            .get(content_start..line_text_end)
            .unwrap_or("");
        let is_paired_close_row = parser.settings().layout.legacy()
            && parser.wikidot_pending_paired_list_close()
            && {
                let trimmed = row_source.trim();
                trimmed.eq_ignore_ascii_case("[[/ul]]")
                    || trimmed.eq_ignore_ascii_case("[[/ol]]")
            };
        if is_paired_close_row {
            errors.truncate(errors_before);
            let _ = parser.take_wikidot_pending_paired_list_close();
        }
        for unit in line
            .escaped_units
            .into_iter()
            .chain(parser.take_quote_escaped_units())
        {
            match unit {
                QuoteEscapedUnit::Elements(elements, safe) => {
                    gate_escaped_segments.push((elements, safe));
                }
                QuoteEscapedUnit::Row(depth, elements, safe) => {
                    pending_rows.push((depth, elements, safe));
                }
            }
        }
        let mut elements = line.elements;
        if is_paired_close_row {
            // v2m: when the previous row of this fresh run already ends with
            // a break (`> [[li]]b` + break), drop that break and keep only the
            // paired close's empty-list break (single break, bare).
            if let Some((_, _, prev_row)) = depths.last_mut()
                && matches!(
                    prev_row.elements.last(),
                    Some(Element::LineBreak | Element::LineBreaks(_))
                )
            {
                prev_row.elements.pop();
            }
            elements = vec![Element::LineBreak];
            paragraph_safe = false;
        }
        let marker_row = parser.settings().layout.legacy()
            && quote_row_line_ends_with_marker(
                parser.full_text().inner(),
                content_start,
                line_text_end,
            );
        let marker_next_is_quote = parser.current().token == Token::Quote;
        let marker_final = marker_row && !marker_next_is_quote;
        let mut paragraph_split = line.paragraph_split;
        if is_paired_close_row {
            paragraph_split = None;
        }
        if leave_indented_whitespace
            && elements.len() > 1
            && matches!(elements.first(), Some(Element::Text(text)) if text.chars().all(|c| c.is_whitespace()))
            && !matches!(elements.get(1), Some(Element::Text(_)))
        {
            elements.remove(0);
            if let Some(counts) = &mut paragraph_split {
                counts[0].0 -= 1;
            }
        }
        if parser.settings().layout.legacy()
            && unowned_footnote_first(parser, content_start, physical_line_end, &elements)
        {
            let len_before_footnote_removal = elements.len();
            let removed = remove_first_footnote(&mut elements);
            if elements.len() != len_before_footnote_removal {
                paragraph_split = None;
            }
            debug_assert!(removed, "footnote-first classification found no footnote");
        }

        let stopped_at_footnote_close = wikidot_footnote_close_ahead(parser);
        if stopped_at_footnote_close && elements.is_empty() {
            consumed_pruned_row = true;
            continue;
        }

        let stopped_at_div_close = wikidot_div_close_ahead(parser);
        if stopped_at_div_close && elements.is_empty() {
            consumed_pruned_row = true;
            continue;
        }

        let opened_tab_tail = parser.settings().layout.legacy()
            && !gate_escaped_segments.is_empty()
            && elements
                .iter()
                .any(|element| matches!(element, Element::TabView(_)));
        if opened_tab_tail {
            fold_following_into_tab_segment = true;
        }
        quote_run_active = !ends_quote_run
            && !stopped_at_footnote_close
            && !stopped_at_div_close
            && (gate_escaped_segments.is_empty() || fold_following_into_tab_segment);
        if fold_following_into_tab_segment && !opened_tab_tail {
            append_prose_to_tab_segment(&mut gate_escaped_segments, elements);
            continue;
        }

        // A quote line whose only content is @@@@ is a complete empty raw
        // line. Live keeps its break in the paragraph. The element list is
        // empty, and that must not make the line the empty spaced row that
        // ends the paragraph.
        let complete_empty_raw_line = parser.settings().layout.legacy()
            && parser
                .full_text()
                .inner()
                .get(content_start..line_text_end)
                .is_some_and(|line| line.trim_end_matches([' ', '\t']) == "@@@@");
        let row_is_empty = elements.is_empty()
            && errors.len() == errors_before
            && !complete_empty_raw_line;
        let mut absorbed_unquoted_footnote = false;
        if parser.settings().layout.legacy() && starts_footnote(parser) {
            let mut candidate = parser.clone_with_rule(RULE_BLOCKQUOTE);
            if let Ok(footnote) = super::RULE_BLOCK.try_consume(&mut candidate)
                && let Elements::Single(Element::Footnote(index)) = footnote.item
            {
                errors.extend(footnote.errors);
                parser.update(&candidate);
                if row_is_empty {
                    consumed_pruned_row = true;
                    continue;
                }
                elements.push(Element::Footnote(index));
                if let Some(counts) = &mut paragraph_split
                    && let Some(last) = counts.last_mut()
                {
                    last.0 += 1;
                }
                absorbed_unquoted_footnote = true;
            }
        }
        let consumed_past_line = parser.current().span.start > physical_line_end;
        let escaped_after_deeper_close = parser.settings().layout.legacy()
            && consumed_past_line
            && matches!(elements.first(), Some(Element::Collapsible { .. }))
            && consumed_deeper_collapsible_close(
                parser,
                physical_line_end,
                parser.current().span.start,
                absolute_depth,
            );
        append_unquoted_close_break |= parser.settings().layout.legacy()
            && consumed_past_line
            && matches!(elements.first(), Some(Element::Collapsible { .. }))
            && consumed_unquoted_collapsible_close(
                parser,
                physical_line_end,
                parser.current().span.start,
            );
        if parser.settings().layout.legacy()
            && consumed_past_line
            && parser.current().token == Token::LineBreak
        {
            parser.step()?;
        } else if parser.settings().layout.legacy()
            && consumed_past_line
            && parser.current().token == Token::ParagraphBreak
        {
            quote_run_active = false;
        }
        let empty_spaced_row =
            row_is_empty && spaced_after_marker && single_space_after_marker;
        if row_is_empty && (consumed_past_line || !spaced_after_marker) {
            consumed_pruned_row = true;
            continue;
        }

        let alignment_block = elements.iter().any(|element| {
            matches!(
                element,
                Element::Container(container)
                    if matches!(container.ctype(), ContainerType::Align(_))
            )
        });
        let note_row = row_has_note(&elements);
        let collapsible_row =
            matches!(elements.first(), Some(Element::Collapsible { .. }));

        // A line carrying a flat block closer keeps its break (the closer
        // consumed no break itself); flat heads already ate theirs.
        let wikidot_block_close_line = parser.settings().layout.legacy()
            && (elements.iter().any(|element| {
                matches!(
                    element,
                    Element::Partial(PartialElement::WikidotBlockClose(_))
                )
            }) || row_ends_with_formed_div(&elements));
        // A tabview ending the row keeps the row break: live renders the
        // line after the tabview in the same inline flow ("</div><br>\noutro").
        let wikidot_tabview_line = parser.settings().layout.legacy()
            && elements
                .iter()
                .rev()
                .find(|element| **element != Element::LineBreak)
                .is_some_and(|element| matches!(element, Element::TabView(_)));
        // A div that closes while a span is still open leaves that newline
        // inside the span. A table or rule on the same kind of row does not.
        let span_owns_div_break = parser.settings().layout.legacy()
            && parser.in_wikidot_span_body()
            && elements.iter().any(|element| {
                matches!(
                    element,
                    Element::Container(container)
                        if container.ctype() == ContainerType::Div
                )
            });
        let mut keep_line_break = if parser.settings().layout.legacy() {
            paragraph_safe
                || alignment_block
                || note_row
                || collapsible_row
                || wikidot_block_close_line
                || wikidot_tabview_line
                || span_owns_div_break
        } else {
            !consumed_past_line || paragraph_safe
        };

        if marker_row
            && (marker_next_is_quote
                || matches!(elements.last(), Some(Element::LineBreaks(_))))
        {
            keep_line_break = false;
        }
        let quote_end_list_row = parser.settings().layout.legacy()
            && parser.current().token != Token::Quote
            && matches!(
                elements.iter().rev().find(|element| {
                    !matches!(element, Element::LineBreak | Element::LineBreaks(_))
                }),
                Some(Element::List { wikidot_explicit_block: true, .. })
            );
        let quote_end_ordered_list_row = quote_end_list_row
            && matches!(
                elements.iter().rev().find(|element| {
                    !matches!(element, Element::LineBreak | Element::LineBreaks(_))
                }),
                Some(Element::List {
                    ltype: ListType::Numbered,
                    ..
                })
            );
        let boundary_list_close = parser.take_wikidot_quote_boundary_list_close();
        let li_auto_close = parser.take_wikidot_quote_boundary_li_auto_close();
        let quote_end_list_break =
            quote_end_list_row && !boundary_list_close && !li_auto_close;
        if quote_end_list_row && boundary_list_close {
            append_unquoted_close_break = true;
            append_from_list_boundary = true;
            append_from_ordered_list_boundary = quote_end_ordered_list_row;
        }
        // A boundary list close followed by a quoted line ends the quote run
        // there: live starts a new blockquote for the follower and the
        // migrated break (or the ordered degraded shell) lands between the
        // two blockquotes.
        let boundary_list_row = parser.settings().layout.legacy()
            && !quote_end_list_row
            && boundary_list_close
            && parser.current().token == Token::Quote
            && matches!(
                elements.iter().rev().find(|element| {
                    !matches!(element, Element::LineBreak | Element::LineBreaks(_))
                }),
                Some(Element::List {
                    wikidot_explicit_block: true,
                    ..
                })
            );
        if boundary_list_row {
            append_unquoted_close_break = true;
            append_from_list_boundary = true;
            append_from_ordered_list_boundary = matches!(
                elements.iter().rev().find(|element| {
                    !matches!(element, Element::LineBreak | Element::LineBreaks(_))
                }),
                Some(Element::List {
                    ltype: ListType::Numbered,
                    ..
                })
            );
            quote_run_active = false;
        }
        // BRIEF-QR45: a UL/OL that soft-ended at a shallower `[[/li]]`
        // boundary also ends the run here, but with no break between the
        // blockquotes — the paired quoted close renders inside the fresh
        // run instead (break or nothing, per part (b)).
        let li_auto_list_row = parser.settings().layout.legacy()
            && li_auto_close
            && parser.current().token == Token::Quote
            && matches!(
                elements.iter().rev().find(|element| {
                    !matches!(element, Element::LineBreak | Element::LineBreaks(_))
                }),
                Some(Element::List {
                    wikidot_explicit_block: true,
                    ..
                })
            );
        if li_auto_list_row {
            quote_run_active = false;
        }
        if quote_end_list_break
            && !matches!(elements.last(), Some(Element::LineBreak))
        {
            elements.push(Element::LineBreak);
        }
        if !empty_spaced_row
            && keep_line_break
            && !absorbed_unquoted_footnote
            && !parser.pending_wikidot_collapsible_closer()
            && !parser.pending_wikidot_div_closer()
        {
            elements.push(Element::LineBreak);
            if let Some(counts) = &mut paragraph_split {
                let index = counts
                    .iter()
                    .rposition(|(count, _, _)| *count > 0)
                    .unwrap_or(counts.len() - 1);
                counts[index].0 += 1;
            }
        }

        if flat_split.is_none()
            && !flatten_following_rows
            && parser.settings().layout.legacy()
            && let Some(close_index) = find_external_flat_close(parser, &elements)
        {
            let trailing_break = matches!(elements.last(), Some(Element::LineBreak));
            if trailing_break {
                elements.pop();
            }
            let mut rest = elements.split_off(close_index);
            let close = rest.remove(0);
            trim_wikidot_flat_close_rest(&mut rest);
            // Only a closer that ends its row (nothing follows it on that
            // line) carries the row's break over to the unquoted line.
            let trailing_break = trailing_break && rest.is_empty();
            if first_row_depth.is_none() {
                first_row_depth = Some(depth);
            }
            let base_depth = first_row_depth.unwrap_or(depth);
            if !elements.is_empty() {
                let source_line = if row_indent > 0 {
                    Some(&parser.full_text().inner()[content_start..line_text_end])
                } else {
                    None
                };
                let cut_boundary = parser.take_wikidot_table_cut_boundary();
                let before_safe = elements.iter().all(Element::paragraph_safe);
                elements.push(Element::LineBreak);
                depths.push((
                    depth - 1,
                    (),
                    NativeQuoteRow {
                        elements,
                        paragraph_safe: before_safe,
                        paragraph_split: None,
                        empty_spaced: false,
                        indent: row_indent,
                        source_line,
                        cut_boundary,
                        keep_trailing_break: false,
                        marker_final: false,
                        residual_block: false,
                    },
                ));
            }
            flat_split = Some(QuoteFlatSplit {
                close,
                rest,
                rows: Vec::new(),
                base_depth,
                trailing_break,
            });
            push_quote_row_units(
                &mut depths,
                &mut escaped_rows,
                flatten_following_rows,
                std::mem::take(&mut pending_rows),
            );
            flatten_following_rows |= escaped_after_deeper_close;
            continue;
        }
        if let Some(split) = flat_split.as_mut() {
            if matches!(elements.last(), Some(Element::LineBreak)) {
                elements.pop();
            }
            let source_line = if row_indent > 0 {
                Some(&parser.full_text().inner()[content_start..line_text_end])
            } else {
                None
            };
            let _ = parser.take_wikidot_table_cut_boundary();
            split.rows.push((
                depth,
                NativeQuoteRow {
                    elements,
                    paragraph_safe,
                    paragraph_split: None,
                    empty_spaced: empty_spaced_row,
                    indent: row_indent,
                    source_line,
                    cut_boundary: false,
                    keep_trailing_break: false,
                        marker_final: false,
                        residual_block: false,
                },
            ));
            push_quote_row_units(
                &mut depths,
                &mut escaped_rows,
                flatten_following_rows,
                std::mem::take(&mut pending_rows),
            );
            flatten_following_rows |= escaped_after_deeper_close;
            continue;
        }

        let source_line = if row_indent > 0 {
            Some(&parser.full_text().inner()[content_start..line_text_end])
        } else {
            None
        };
        let paragraph_split = paragraph_split.filter(|counts| {
            counts.len() >= 2
                && counts.iter().map(|entry| entry.0).sum::<usize>() == elements.len()
        });
        let cut_boundary = parser.take_wikidot_table_cut_boundary();
        let keep_trailing_break = quote_end_list_break
            || (marker_final
                && !matches!(elements.last(), Some(Element::LineBreaks(_))));
        let residual_block = parser.settings().layout.legacy()
            && match elements.iter().position(|element| {
                matches!(
                    element,
                    Element::Container(container)
                        if container.ctype() == ContainerType::Div
                )
            }) {
                Some(position) if position > 0 => elements[..position]
                    .iter()
                    .all(|element| quote_residue_text(element, true)),
                _ => false,
            };
        let row = NativeQuoteRow {
            elements,
            paragraph_safe,
            paragraph_split,
            empty_spaced: empty_spaced_row,
            indent: row_indent,
            source_line,
            cut_boundary,
            keep_trailing_break,
            marker_final,
            residual_block,
        };
        if flatten_following_rows {
            escaped_rows.push(row);
        } else {
            if first_row_depth.is_none() {
                first_row_depth = Some(depth);
            }
            depths.push((depth - 1, (), row));
        }
        push_quote_row_units(
            &mut depths,
            &mut escaped_rows,
            flatten_following_rows,
            std::mem::take(&mut pending_rows),
        );
        flatten_following_rows |= escaped_after_deeper_close;
    }

    if depths.is_empty()
        && escaped_elements.is_empty()
        && gate_escaped_segments.is_empty()
        && pending_rows.is_empty()
        && !parser.quote_escaped_units_pending()
        && flat_split.is_none()
    {
        if consumed_pruned_row {
            return ok!(false; Elements::None, errors);
        }
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let depth_lists = process_depths((), depths);
    let wikidot = parser.settings().layout.legacy();
    let mut elements: Vec<Element> = depth_lists
        .into_iter()
        .filter_map(|(_, depth_list)| build_blockquote_element(depth_list, wikidot))
        .collect();
    if !escaped_rows.is_empty() {
        elements.extend(build_flattened_quote_rows(escaped_rows, wikidot));
    }
    elements.extend(escaped_elements);
    let mut escaped_segments = Vec::new();
    if let Some(split) = flat_split {
        escaped_segments = build_flat_split_segments(split, wikidot);
    }
    escaped_segments.extend(gate_escaped_segments);
    escaped_segments.extend(pending_rows.into_iter().map(|(depth, row_elements, _)| {
        (form_quote_row_unit(row_elements, depth), false)
    }));
    for unit in parser.take_quote_escaped_units() {
        match unit {
            QuoteEscapedUnit::Elements(segment, segment_safe) => {
                escaped_segments.push((segment, segment_safe));
            }
            QuoteEscapedUnit::Row(depth, row_elements, _) => {
                escaped_segments.push((form_quote_row_unit(row_elements, depth), false));
            }
        }
    }
    if escaped_segments.is_empty() {
        if append_unquoted_close_break
            && !(append_from_list_boundary
                && migrated_break_dropped_before_following_line(parser))
        {
            match append_from_list_boundary
                .then(|| migrated_break_follower_after_blank_line(parser))
            {
                Some(BlankFollowerLead::Paragraph) => {
                    parser.mark_wikidot_quote_boundary_break_leads_paragraph();
                }
                Some(BlankFollowerLead::Suppress) => {}
                _ if append_from_ordered_list_boundary => {
                    elements.push(ordered_boundary_close_shell(
                        parser.current().token == Token::Quote,
                    ));
                }
                _ => {
                    elements.push(Element::LineBreak);
                }
            }
        }
        return ok!(false; elements, errors);
    }
    let mut split_counts: Vec<ParagraphSplitEntry> =
        vec![(elements.len(), false, ParagraphSplitKind::Paragraph)];
    for (segment, segment_safe) in escaped_segments {
        split_counts.push((segment.len(), segment_safe, ParagraphSplitKind::Paragraph));
        elements.extend(segment);
    }

    if append_unquoted_close_break
        && !(append_from_list_boundary
            && migrated_break_dropped_before_following_line(parser))
    {
        match append_from_list_boundary
            .then(|| migrated_break_follower_after_blank_line(parser))
        {
            Some(BlankFollowerLead::Paragraph) => {
                parser.mark_wikidot_quote_boundary_break_leads_paragraph();
            }
            Some(BlankFollowerLead::Suppress) => {}
            _ => {
                if append_from_ordered_list_boundary {
                    elements.push(ordered_boundary_close_shell(
                        parser.current().token == Token::Quote,
                    ));
                } else {
                    elements.push(Element::LineBreak);
                }
                if let Some(last) = split_counts.last_mut() {
                    last.0 += 1;
                }
            }
        }
    }
    let mut result = ok!(false; elements, errors);
    if let Ok(success) = &mut result {
        success.paragraph_split = Some(split_counts);
    }
    result
}

fn wikidot_footnote_close_ahead<'r, 't>(parser: &Parser<'r, 't>) -> bool
where
    'r: 't,
{
    parser.in_footnote() && wikidot_named_close_ahead(parser, "footnote")
}

fn wikidot_div_close_ahead<'r, 't>(parser: &Parser<'r, 't>) -> bool
where
    'r: 't,
{
    parser.in_wikidot_div_body() && wikidot_named_close_ahead(parser, "div")
}

fn wikidot_named_close_ahead<'r, 't>(parser: &Parser<'r, 't>, name: &str) -> bool
where
    'r: 't,
{
    if !parser.settings().layout.legacy() {
        return false;
    }
    if parser.current().token != Token::LeftBlockEnd {
        return false;
    }

    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let Some(close_offset) = source[start..].find("]]") else {
        return false;
    };
    if source[start..start + close_offset]
        .chars()
        .last()
        .is_some_and(char::is_whitespace)
    {
        return false;
    }

    let mut probe = (*parser).clone();
    matches!(probe.get_end_block(), Ok(found) if found.eq_ignore_ascii_case(name))
}

struct NativeQuoteLine<'t> {
    elements: Vec<Element<'t>>,
    paragraph_split: Option<Vec<ParagraphSplitEntry>>,
    escaped_units: Vec<QuoteEscapedUnit<'t>>,
}

fn native_quote_line<'t>(
    elements: Vec<Element<'t>>,
    layout: Vec<ParagraphSplitEntry>,
    layout_live: bool,
    escaped_units: Vec<QuoteEscapedUnit<'t>>,
) -> NativeQuoteLine<'t> {
    let paragraph_split = if layout_live
        && layout.len() >= 2
        && layout.iter().map(|entry| entry.0).sum::<usize>() == elements.len()
    {
        Some(layout)
    } else {
        None
    };
    NativeQuoteLine {
        elements,
        paragraph_split,
        escaped_units,
    }
}

fn merge_quote_line_chunk<'t>(
    layout: &mut Vec<ParagraphSplitEntry>,
    layout_live: &mut bool,
    chunk: &[Element<'t>],
    chunk_safe: bool,
    split: Option<Vec<ParagraphSplitEntry>>,
) {
    let split = split.filter(|counts| {
        counts.len() >= 2
            && counts.iter().map(|entry| entry.0).sum::<usize>() == chunk.len()
    });
    match split {
        Some(counts) => {
            *layout_live = true;
            if layout.is_empty() {
                *layout = counts;
            } else {
                let last = layout.last_mut().expect("layout is non-empty");
                last.0 += counts[0].0;
                last.1 &= counts[0].1;
                layout.extend_from_slice(&counts[1..]);
            }
        }
        None => {
            if layout.is_empty() {
                layout.push((chunk.len(), chunk_safe, ParagraphSplitKind::Paragraph));
            } else {
                let last = layout.last_mut().expect("layout is non-empty");
                last.0 += chunk.len();
                last.1 &= chunk_safe;
            }
        }
    }
}

fn push_quote_row_units<'t>(
    depths: &mut Vec<(usize, (), NativeQuoteRow<'t>)>,
    escaped_rows: &mut Vec<NativeQuoteRow<'t>>,
    flatten: bool,
    units: Vec<(usize, Vec<Element<'t>>, bool)>,
) {
    for (depth, elements, paragraph_safe) in units {
        let row = NativeQuoteRow {
            elements,
            paragraph_safe,
            paragraph_split: None,
            empty_spaced: false,
            indent: 0,
            source_line: None,
            cut_boundary: false,
            keep_trailing_break: false,
                        marker_final: false,
                        residual_block: false,
        };
        if flatten {
            escaped_rows.push(row);
        } else if depth >= 1 {
            depths.push((depth - 1, (), row));
        }
    }
}

fn form_quote_row_unit<'t>(
    row_elements: Vec<Element<'t>>,
    depth: usize,
) -> Vec<Element<'t>> {
    let mut stack = ParagraphStack::new_wikidot();
    for element in row_elements {
        stack.push_element(element, true);
    }
    stack.pop_line_break();
    let mut formed = stack.into_elements();
    for _ in 0..depth {
        formed = vec![Element::Container(Container::new(
            ContainerType::Blockquote,
            formed,
            AttributeMap::new(),
        ))];
    }
    formed
}

fn collect_native_blockquote_line<'r, 't>(
    parser: &mut Parser<'r, 't>,
    close: &[ParseCondition],
) -> ParseResult<'r, 't, NativeQuoteLine<'t>> {
    let line_end_token = std::iter::once(parser.current())
        .chain(parser.remaining().iter())
        .find(|token| {
            token.token == Token::LineBreak
                || token.token == Token::ParagraphBreak
                || token.token == Token::InputEnd
        })
        .expect("tokenization always ends with input-end");
    let line_end = line_end_token.span.end;
    let mut elements = Vec::new();
    let mut errors = Vec::new();
    let mut paragraph_safe = true;
    let mut layout: Vec<ParagraphSplitEntry> = Vec::new();
    let mut layout_live = false;
    let mut escaped_units: Vec<QuoteEscapedUnit<'t>> = Vec::new();

    loop {
        if parser.evaluate_any(close) {
            if parser.current().token != Token::InputEnd {
                parser.step()?;
            }
            return ok!(paragraph_safe; native_quote_line(elements, layout, layout_live, escaped_units), errors);
        }

        if wikidot_footnote_close_ahead(parser) {
            return ok!(paragraph_safe; native_quote_line(elements, layout, layout_live, escaped_units), errors);
        }

        if wikidot_div_close_ahead(parser) {
            return ok!(paragraph_safe; native_quote_line(elements, layout, layout_live, escaped_units), errors);
        }

        if !parser.discarding_hidden_body() && parser.at_body_end_boundary() {
            return ok!(paragraph_safe; native_quote_line(elements, layout, layout_live, escaped_units), errors);
        }

        let success = consume(parser)?;
        let ParseSuccess {
            item,
            errors: chunk_errors,
            paragraph_safe: chunk_safe,
            paragraph_split: chunk_split,
            ..
        } = success;
        errors.extend(chunk_errors);
        paragraph_safe &= chunk_safe;
        let chunk: Vec<Element> = match item {
            Elements::Multiple(elements) => elements,
            Elements::Single(element) => vec![element],
            Elements::None => Vec::new(),
        };
        merge_quote_line_chunk(
            &mut layout,
            &mut layout_live,
            &chunk,
            chunk_safe,
            chunk_split,
        );
        elements.extend(chunk);
        escaped_units.extend(parser.take_quote_escaped_units());

        if parser.current().span.start >= line_end {
            return ok!(paragraph_safe; native_quote_line(elements, layout, layout_live, escaped_units), errors);
        }
    }
}

fn quote_row_line_ends_with_marker(source: &str, start: usize, end: usize) -> bool {
    let line = &source[start.min(source.len())..end.min(source.len())];
    let trimmed = line.trim_end_matches([' ', '\t', '\r']);
    trimmed
        .strip_suffix('_')
        .is_some_and(|before| before.ends_with([' ', '\t']))
}

fn build_blockquote_element(
    list: DepthList<(), NativeQuoteRow>,
    wikidot: bool,
) -> Option<Element> {
    let list = if wikidot {
        merge_adjacent_native_list_rows(demote_ungrounded_indented_list_rows(list))
    } else {
        list
    };
    let mut stack = if wikidot {
        ParagraphStack::new_wikidot()
    } else {
        ParagraphStack::new()
    };

    let mut keep_trailing_break = false;
    let mut marker_final = false;
    let mut residual_chunk = false;
    for item in list {
        match item {
            DepthItem::Item(row) => {
                keep_trailing_break = row.keep_trailing_break;
                marker_final = row.marker_final;
                residual_chunk |= row.residual_block || row_has_residual_div(&row) || row_has_residual_list(&row);
                push_native_quote_row(&mut stack, row, wikidot);
            }
            DepthItem::List(_, list) => {
                keep_trailing_break = false;
                if let Some(blockquote) = build_blockquote_element(list, wikidot) {
                    stack.pop_line_break();
                    stack.push_element(blockquote, false);
                }
            }
        }
    }

    // The hoist recorded this break. The usual pop would drop the br live keeps.
    // A trailing break an all-empty explicit list dissolved to is body
    // content on live (a quoted empty list renders
    // <blockquote><br></blockquote>), not pop fodder.
    if !keep_trailing_break && !stack.wikidot_trailing_empty_list_break() {
        stack.pop_line_break();
    }
    let mut elements = stack.into_elements();
    if wikidot {
        elements = unwrap_scored_alias_quote_paragraphs(elements);
        if marker_final {
            elements = unwrap_all_quote_paragraphs(elements);
        }
        if residual_chunk {
            elements = unwrap_residual_run_paragraphs(elements);
        }
    }
    if elements.is_empty() {
        return None;
    }

    Some(Element::Container(Container::new(
        ContainerType::Blockquote,
        elements,
        AttributeMap::new(),
    )))
}

/// Live Wikidot leaves a quote line unwrapped when a paired scored span or
/// anchor opens it or closes it: `> [[span_]]x[[/span]] tail` renders bare
/// inside the blockquote, as does `> y [[span_]]z[[/span]]`.
fn unwrap_scored_alias_quote_paragraphs(elements: Vec<Element<'_>>) -> Vec<Element<'_>> {
    let mut output = Vec::with_capacity(elements.len());
    for element in elements {
        match element {
            Element::Container(mut container)
                if container.ctype() == ContainerType::Paragraph
                    && (container.elements().first().is_some_and(
                        ParagraphStack::wikidot_element_is_scored_alias_open,
                    ) || ParagraphStack::wikidot_run_ends_with_scored_alias(
                        container.elements(),
                    )) =>
            {
                output.append(container.elements_mut());
            }
            other => output.push(other),
        }
    }
    output
}

fn unwrap_all_quote_paragraphs(elements: Vec<Element<'_>>) -> Vec<Element<'_>> {
    let mut output = Vec::with_capacity(elements.len());
    for element in elements {
        match element {
            Element::Container(mut container)
                if container.ctype() == ContainerType::Paragraph =>
            {
                output.append(container.elements_mut());
            }
            other => output.push(other),
        }
    }
    output
}

fn row_has_residual_div(row: &NativeQuoteRow<'_>) -> bool {
    match row.elements.iter().position(|element| {
        matches!(
            element,
            Element::Container(container)
                if container.ctype() == ContainerType::Div
        )
    }) {
        Some(position) if position > 0 => {
            row.elements[..position]
                .iter()
                .all(|element| matches!(element, Element::Text(_)))
                && row.elements[..position]
                    .iter()
                    .any(|element| quote_residue_text(element, false))
        }
        _ => false,
    }
}

fn row_has_residual_list(row: &NativeQuoteRow<'_>) -> bool {
    match row.elements.iter().position(|element| {
        matches!(element, Element::List { .. })
    }) {
        // A list that closed at this row's own depth (its end break kept on
        // the row) leaves quote residue behind at the outer depth; a list
        // closed deeper (all rows at the deeper depth) keeps the residue
        // wrapped in a paragraph.
        Some(position) if position > 0 => {
            row.elements[..position]
                .iter()
                .all(|element| matches!(element, Element::Text(_)))
                && row.elements[..position]
                    .iter()
                    .any(|element| quote_residue_text(element, false))
                && matches!(
                    row.elements.get(position + 1),
                    Some(
                        Element::LineBreak
                            | Element::LineBreaks(_),
                    )
                )
        }
        _ => false,
    }
}

fn unwrap_residual_run_paragraphs(elements: Vec<Element<'_>>) -> Vec<Element<'_>> {
    fn is_break(element: &Element) -> bool {
        matches!(element, Element::LineBreak | Element::LineBreaks(_))
    }
    fn is_div(element: &Element) -> bool {
        matches!(
            element,
            Element::Container(container)
                if container.ctype() == ContainerType::Div
        )
    }
    let mut output: Vec<Element> = Vec::with_capacity(elements.len());
    let mut seam_pending = false;
    let mut after_residue_prefix = false;
    let mut iter = elements.into_iter().peekable();
    while let Some(element) = iter.next() {
        match element {
            Element::Container(mut container)
                if container.ctype() == ContainerType::Paragraph =>
            {
                let residue_only = !container.elements().is_empty()
                    && container
                        .elements()
                        .iter()
                        .all(|element| quote_residue_text(element, true));
                if residue_only {
                    if seam_pending && output.last().is_some_and(|last| !is_break(last)) {
                        output.push(Element::LineBreak);
                        seam_pending = false;
                    }
                    output.append(container.elements_mut());
                    after_residue_prefix = true;
                } else {
                    if output.last().is_some_and(|last| !is_break(last)) {
                        output.push(Element::LineBreak);
                    }
                    let elems = container.elements();
                    let tail_start = elems
                        .iter()
                        .rposition(|element| is_break(element))
                        .map(|index| index + 1)
                        .unwrap_or(0);
                    let tail_residue = tail_start < elems.len()
                        && elems[tail_start..]
                            .iter()
                            .all(|element| quote_residue_text(element, true));
                    seam_pending = !elems.is_empty() && !tail_residue;
                    after_residue_prefix = false;
                    output.append(container.elements_mut());
                }
            }
            other => {
                if is_break(&other) {
                    if after_residue_prefix && iter.peek().is_some_and(is_div) {
                        continue;
                    }
                    seam_pending = false;
                    after_residue_prefix = false;
                } else if seam_pending
                    && output.last().is_some_and(|last| !is_break(last))
                {
                    output.push(Element::LineBreak);
                    seam_pending = false;
                    after_residue_prefix = false;
                } else {
                    after_residue_prefix = false;
                }
                output.push(other);
            }
        }
    }
    output
}

fn unwrap_residue_quote_paragraphs(elements: Vec<Element<'_>>) -> Vec<Element<'_>> {
    let mut output = Vec::with_capacity(elements.len());
    for element in elements {
        match element {
            Element::Container(mut container)
                if container.ctype() == ContainerType::Paragraph
                    && container
                        .elements()
                        .iter()
                        .any(|element| quote_residue_text(element, false)) =>
            {
                output.append(container.elements_mut());
            }
            other => output.push(other),
        }
    }
    output
}

fn demote_ungrounded_indented_list_rows(
    list: DepthList<(), NativeQuoteRow>,
) -> DepthList<(), NativeQuoteRow> {
    let mut demoted = Vec::with_capacity(list.len());
    let mut previous_grounded = false;
    for item in list {
        match item {
            DepthItem::Item(mut row) => {
                if row.empty_spaced {
                    previous_grounded = false;
                } else if native_list_element(&mut row).is_some() {
                    if row.indent == 0 {
                        previous_grounded = true;
                    } else if !previous_grounded {
                        demote_indented_list_row(&mut row);
                    }
                } else {
                    previous_grounded = false;
                }
                demoted.push(DepthItem::Item(row));
            }
            DepthItem::List((), nested) => {
                previous_grounded = false;
                demoted.push(DepthItem::List(
                    (),
                    demote_ungrounded_indented_list_rows(nested),
                ));
            }
        }
    }
    demoted
}

fn demote_indented_list_row(row: &mut NativeQuoteRow<'_>) {
    let Some(source_line) = row.source_line else {
        return;
    };
    row.elements.clear();
    row.elements
        .push(Element::Text(std::borrow::Cow::Borrowed(source_line)));
    row.elements.push(Element::LineBreak);
    row.paragraph_safe = true;
}

fn merge_adjacent_native_list_rows(
    list: DepthList<(), NativeQuoteRow>,
) -> DepthList<(), NativeQuoteRow> {
    let mut merged = Vec::with_capacity(list.len());
    for item in list {
        match item {
            DepthItem::Item(mut row) => {
                if let Some(DepthItem::Item(previous)) = merged.last_mut()
                    && merge_native_list_rows(previous, &mut row)
                {
                    continue;
                }
                merged.push(DepthItem::Item(row));
            }
            DepthItem::List((), nested) => {
                merged.push(DepthItem::List((), merge_adjacent_native_list_rows(nested)));
            }
        }
    }
    merged
}

fn quote_row_indent(source: &str, content_start: usize) -> usize {
    let line_start = source[..content_start]
        .rfind('\n')
        .map_or(0, |offset| offset + 1);
    let prefix = &source[line_start..content_start];
    let Some(marker) = prefix.rfind('>') else {
        return 0;
    };
    let tail = &prefix[marker + 1..];
    if tail.chars().all(|c| matches!(c, ' ' | '\t')) {
        tail.len().saturating_sub(1)
    } else {
        0
    }
}

fn insert_native_list_item<'t>(
    list: &mut Element<'t>,
    depth: usize,
    ltype: ListType,
    elements: Vec<Element<'t>>,
) -> bool {
    let Element::List { items, .. } = list else {
        return false;
    };
    if depth == 0 {
        items.push(ListItem::Elements {
            attributes: AttributeMap::new(),
            elements,
        });
        return true;
    }
    if !matches!(items.last(), Some(ListItem::Elements { .. })) {
        items.push(ListItem::Elements {
            attributes: AttributeMap::new(),
            elements: Vec::new(),
        });
    }
    let Some(ListItem::Elements {
        elements: last_elements,
        ..
    }) = items.last_mut()
    else {
        return false;
    };
    if !matches!(last_elements.last(), Some(Element::List { .. })) {
        last_elements.push(Element::List {
            ltype,
            items: Vec::new(),
            attributes: AttributeMap::new(),
            wikidot_explicit_block: false,
            wikidot_unclosed: false,
        });
    }
    let Some(sublist) = last_elements.last_mut() else {
        return false;
    };
    insert_native_list_item(sublist, depth - 1, ltype, elements)
}

fn merge_native_list_rows<'t>(
    previous: &mut NativeQuoteRow<'t>,
    current: &mut NativeQuoteRow<'t>,
) -> bool {
    if previous.empty_spaced || current.empty_spaced {
        return false;
    }
    let depth = current.indent.saturating_sub(previous.indent);
    let Some(previous_list) = native_list_element(previous) else {
        return false;
    };
    let Element::List {
        ltype: previous_type,
        ..
    } = &*previous_list
    else {
        return false;
    };
    let previous_type = *previous_type;
    let previous_ends_with_empty_block = matches!(
        previous_list,
        Element::List { items, .. } if items.last().is_some_and(|item| match item {
            ListItem::Elements { elements, .. } => elements.iter().any(|element| {
                matches!(element, Element::Collapsible { .. })
                    || matches!(
                        element,
                        Element::Container(container)
                            if container.ctype() == ContainerType::Div
                    )
                    || matches!(
                        element,
                        Element::Partial(PartialElement::WikidotBlockOpen(_))
                    )
            }),
            _ => false,
        })
    );
    if previous_ends_with_empty_block {
        return false;
    }
    let Some(current_list) = native_list_element(current) else {
        return false;
    };
    let Element::List {
        ltype: current_type,
        items: current_items,
        ..
    } = current_list
    else {
        return false;
    };
    let current_type = *current_type;
    if depth == 0 && previous_type != current_type {
        return false;
    }
    if current_items.len() != 1 {
        return false;
    }
    let Some(ListItem::Elements { elements, .. }) = current_items.pop() else {
        return false;
    };
    insert_native_list_item(previous_list, depth, current_type, elements)
}

fn native_list_element<'a, 't>(
    row: &'a mut NativeQuoteRow<'t>,
) -> Option<&'a mut Element<'t>> {
    match row.elements.as_mut_slice() {
        [Element::List { .. }] | [Element::List { .. }, Element::LineBreak] => {
            row.elements.first_mut()
        }
        _ => None,
    }
}

fn build_flattened_quote_rows(
    rows: Vec<NativeQuoteRow<'_>>,
    wikidot: bool,
) -> Vec<Element<'_>> {
    let mut stack = if wikidot {
        ParagraphStack::new_wikidot()
    } else {
        ParagraphStack::new()
    };
    for row in rows {
        push_native_quote_row(&mut stack, row, wikidot);
    }
    stack.pop_line_break();
    stack.into_elements()
}

fn row_has_note(elements: &[Element<'_>]) -> bool {
    elements.iter().any(|element| {
        matches!(
            element,
            Element::Container(container) if container.ctype() == ContainerType::Note
        )
    })
}

fn row_ends_with_formed_div(elements: &[Element<'_>]) -> bool {
    elements
        .iter()
        .rev()
        .find(|element| **element != Element::LineBreak)
        .is_some_and(|element| {
            matches!(
                element,
                Element::Container(container)
                    if container.ctype() == ContainerType::Div
            )
        })
}

fn quote_row_explicit_list_leaves_residue_bare(
    wikidot: bool,
    elements: &[Element<'_>],
) -> bool {
    wikidot
        && elements.iter().any(|element| {
            matches!(
                element,
                Element::List {
                    wikidot_explicit_block: true,
                    ..
                }
            )
        })
}

fn row_has_quote_residue(elements: &[Element<'_>]) -> bool {
    elements.iter().any(|element| match element {
        Element::Text(text) => {
            let trimmed = text.trim();
            !trimmed.is_empty() && trimmed.bytes().all(|byte| byte == b'>')
        }
        _ => false,
    })
}

fn quote_residue_text(element: &Element<'_>, keep_space: bool) -> bool {
    let Element::Text(text) = element else {
        return false;
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return keep_space;
    }
    trimmed.bytes().all(|byte| byte == b'>')
}

fn push_native_quote_row<'t>(
    stack: &mut ParagraphStack<'t>,
    row: NativeQuoteRow<'t>,
    wikidot: bool,
) {
    if row.empty_spaced {
        stack.pop_line_break();
        stack.end_paragraph();
        stack.mark_wikidot_simple_table_boundary();
        stack.record_next_element_line_start(true);
        return;
    }
    // A row carrying only a line break lifted from an all-empty explicit
    // list arms the stack's empty-list state, as the top-level gather
    // does, so the post-list break handling (bare neighbours, break
    // collapse) applies in quote bodies too.
    if wikidot
        && !row.paragraph_safe
        && matches!(row.elements.as_slice(), [Element::LineBreak])
    {
        stack.mark_wikidot_empty_list_break();
    }
    let alignment_block = row.elements.iter().any(|element| {
        matches!(
            element,
            Element::Container(container)
                if matches!(container.ctype(), ContainerType::Align(_))
        )
    });
    let note_block = row_has_note(&row.elements);
    if wikidot && alignment_block {
        stack.ensure_wikidot_trailing_line_break();
    }
    let empty_list_break_row = wikidot
        && !row.paragraph_safe
        && matches!(row.elements.as_slice(), [Element::LineBreak]);
    if wikidot
        && !row.paragraph_safe
        && !alignment_block
        && !row.residual_block
        && !empty_list_break_row
    {
        stack.pop_line_break();
    }
    let split_counts = row
        .paragraph_split
        .as_ref()
        .filter(|counts| {
            counts.len() >= 2
                && counts.iter().map(|entry| entry.0).sum::<usize>() == row.elements.len()
        })
        .cloned();
    // A row carrying a flat block closer leaves following content unwrapped
    // and keeps its own trailing break (mirrors top-level closer lines).
    let wikidot_block_close_row = wikidot
        && (row.elements.iter().any(|element| {
            matches!(
                element,
                Element::Partial(PartialElement::WikidotBlockClose(_))
            )
        }) || row_ends_with_formed_div(&row.elements));
    if wikidot_block_close_row {
        stack.record_next_element_line_start(false);
    }
    // A tabview ending a row leaves the following line in its inline flow
    // (live: "</div><br>\noutro"), like a flat block closer row.
    let wikidot_tabview_row_end = wikidot
        && row
            .elements
            .iter()
            .rev()
            .find(|element| **element != Element::LineBreak)
            .is_some_and(|element| matches!(element, Element::TabView(_)));
    let wikidot_block_open_row = wikidot
        && matches!(
            row.elements.first(),
            Some(Element::Partial(PartialElement::WikidotBlockOpen(_)))
        );
    if wikidot_block_open_row {
        stack.record_next_element_line_start(true);
    }
    if let Some(counts) = split_counts {
        let leaves_following_content_unwrapped = row
            .elements
            .iter()
            .any(collapsible_has_direct_literal_nested_opener)
            || (wikidot && alignment_block)
            || (wikidot && note_block)
            || wikidot_block_close_row
            || wikidot_tabview_row_end
            || quote_row_explicit_list_leaves_residue_bare(wikidot, &row.elements);
        let has_simple_table = row.elements.iter().any(|element| {
            matches!(
                element,
                Element::Table(table)
                    if table.table_type == crate::tree::TableType::Simple
            )
        });
        let seam_bounded =
            crate::parsing::paragraph::batch_is_seam_bounded(&row.elements);
        let last_index = counts.len() - 1;
        let keep_quote_paragraph =
            quote_row_explicit_list_leaves_residue_bare(wikidot, &row.elements)
                && row_has_quote_residue(&row.elements);
        let mut iter = row.elements.into_iter();
        for (index, (count, safe, _)) in counts.iter().enumerate() {
            if index > 0 {
                stack.end_paragraph_at_break();
            }
            let mut slice: Vec<Element> = iter.by_ref().take(*count).collect();
            if index < last_index && matches!(slice.last(), Some(Element::LineBreak)) {
                slice.pop();
            }
            let per_element = (seam_bounded
                && !*safe
                && slice.iter().any(|element| {
                    !element.paragraph_safe()
                        && !matches!(
                            element,
                            Element::List { .. } | Element::DefinitionList(_)
                        )
                }))
                || wikidot_block_close_row;
            for element in slice {
                let element_safe = if note_block && element == Element::LineBreak {
                    true
                } else if per_element {
                    element.paragraph_safe()
                } else if keep_quote_paragraph && quote_residue_text(&element, true) {
                    true
                } else {
                    *safe
                };
                stack.push_element(element, element_safe);
            }
        }
        if row.cut_boundary {
            stack.mark_wikidot_simple_table_boundary();
        }
        if has_simple_table {
            stack.merge_wikidot_adjacent_simple_tables();
        }
        if leaves_following_content_unwrapped {
            stack.mark_next_unwrapped();
        }
        return;
    }
    let collapsible_row =
        matches!(row.elements.first(), Some(Element::Collapsible { .. }));
    if wikidot && collapsible_row {
        let mut elements = row.elements.into_iter();
        let collapsible = elements.next().unwrap();
        stack.push_element(collapsible, false);
        stack.mark_next_unwrapped();
        for (index, element) in elements.enumerate() {
            if index == 0 && element != Element::LineBreak {
                stack.push_element(text!("\n"), true);
            }
            let paragraph_safe = element.paragraph_safe();
            stack.push_element(element, paragraph_safe);
        }
        return;
    }
    let leaves_following_content_unwrapped = row
        .elements
        .iter()
        .any(collapsible_has_direct_literal_nested_opener)
        || (wikidot && alignment_block)
        || (wikidot && note_block)
        || wikidot_block_close_row
        || wikidot_tabview_row_end
        || quote_row_explicit_list_leaves_residue_bare(wikidot, &row.elements);
    let has_simple_table = row.elements.iter().any(|element| {
        matches!(
            element,
            Element::Table(table) if table.table_type == crate::tree::TableType::Simple
        )
    });
    let per_element = (!row.paragraph_safe
        && crate::parsing::paragraph::batch_is_seam_bounded(&row.elements)
        && row.elements.iter().any(|element| !element.paragraph_safe()))
        || wikidot_block_close_row;
    let keep_quote_paragraph =
        quote_row_explicit_list_leaves_residue_bare(wikidot, &row.elements)
            && row_has_quote_residue(&row.elements);
    for element in row.elements {
        let paragraph_safe = if note_block && element == Element::LineBreak {
            true
        } else if per_element {
            element.paragraph_safe()
        } else if keep_quote_paragraph && quote_residue_text(&element, true) {
            true
        } else {
            row.paragraph_safe
        };
        stack.push_element(element, paragraph_safe);
    }
    if row.cut_boundary {
        stack.mark_wikidot_simple_table_boundary();
    }
    if has_simple_table {
        stack.merge_wikidot_adjacent_simple_tables();
    }
    if leaves_following_content_unwrapped {
        stack.mark_next_unwrapped();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender, text::TextRender};
    use crate::settings::{WikitextMode, WikitextSettings};
    use std::sync::Once;

    #[derive(Debug)]
    struct TestLogger;

    impl log::Log for TestLogger {
        fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
            true
        }

        fn log(&self, record: &log::Record<'_>) {
            let _ = record.args().to_string();
        }

        fn flush(&self) {}
    }

    static TEST_LOGGER: TestLogger = TestLogger;
    static INIT_LOGGER: Once = Once::new();

    fn enable_test_logging() {
        INIT_LOGGER.call_once(|| {
            let _ = log::set_logger(&TEST_LOGGER);
            log::set_max_level(log::LevelFilter::Trace);
        });
    }

    #[test]
    fn native_blockquote_rejects_excessive_depth() {
        enable_test_logging();

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = format!("{} too deep", ">".repeat(MAX_BLOCKQUOTE_DEPTH + 1));
        let tokenization = crate::tokenize(&input);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("quote token should follow input start");
        parser.set_rule(RULE_BLOCKQUOTE);

        let error = RULE_BLOCKQUOTE
            .try_consume(&mut parser)
            .expect_err("excessive blockquote depth should fail");
        assert_eq!(error.kind(), ParseErrorKind::BlockquoteDepthExceeded);
    }

    #[test]
    fn native_blockquote_accepts_wikidot_observed_depth_31() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = format!("{} too deep", ">".repeat(31));
        let tokenization = crate::tokenize(&input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("<blockquote>").count(), 31, "{html}");
        assert!(html.contains("<p>too deep</p>"), "{html}");
    }

    #[test]
    fn native_blockquote_rejects_non_quote_start() {
        enable_test_logging();

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("plain");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("identifier token should follow input start");
        parser.set_rule(RULE_BLOCKQUOTE);

        let error = RULE_BLOCKQUOTE
            .try_consume(&mut parser)
            .expect_err("non-quote input should not produce a blockquote");
        assert_eq!(error.kind(), ParseErrorKind::RuleFailed);
    }

    #[test]
    fn native_blockquote_prunes_empty_rows_at_every_depth() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut input = "> \n>\n>> \n".to_owned();
        crate::preprocess(&mut input);
        let tokenization = crate::tokenize(&input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.trim().is_empty(), "{html}");
    }

    #[test]
    fn native_quote_simple_tables_join_across_unspaced_empty_row() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = concat!("> ||~ H || V ||\n", ">\n", "> || A || B ||",);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html.matches("<table class=\"wiki-content-table\">").count(),
            1,
            "{html}"
        );
        assert_eq!(html.matches("<tr>").count(), 2, "{html}");
    }

    #[test]
    fn native_quote_simple_tables_stay_separate_after_spaced_empty_row() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = concat!("> ||~ H || V ||\n", "> \n", "> || A || B ||",);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html.matches("<table class=\"wiki-content-table\">").count(),
            2,
            "{html}"
        );
        assert_eq!(html.matches("<tr>").count(), 2, "{html}");
    }

    #[test]
    fn native_blockquote_prunes_rows_consumed_by_an_invisible_child() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut input = concat!(
            "> [[collapsible show=\"show\" hide=\"hide\"]]\n",
            "> [[iftags +missing]]\n",
            "> [[div]]\n",
            "> OMEGA_HIDDEN\n",
            "> [[/iftags]]\n",
            "> OMEGA_VISIBLE_INSIDE\n",
            "> [[/collapsible]]\n",
            "OMEGA_AFTER",
        )
        .to_owned();
        crate::preprocess(&mut input);
        let tokenization = crate::tokenize(&input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(!html.contains("OMEGA_HIDDEN"), "{html}");
        assert!(html.contains("OMEGA_VISIBLE_INSIDE"), "{html}");
        assert!(html.contains("OMEGA_AFTER"), "{html}");
    }

    #[test]
    fn native_blockquote_content_respects_virtual_line_start_for_headings() {
        enable_test_logging();

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = concat!(
            "> + {{WARNING}}\n",
            ">\n",
            "> {{Body}}\n",
            ">\n",
            "> ++ {{LEVEL 5 AUTHORIZATION REQUIRED}}\n",
        );
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");

        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(html.contains("<blockquote>"));
        assert!(html.contains("<h1"));
        assert!(html.contains("WARNING"));
        assert!(html.contains("<h2"));
        assert!(html.contains("LEVEL 5 AUTHORIZATION REQUIRED"));
        assert!(!html.contains("+ <tt>WARNING</tt>"));
        assert!(!html.contains("++ <tt>LEVEL 5 AUTHORIZATION REQUIRED</tt>"));
        assert_eq!(html.matches("<p>").count(), 1, "{html}");
        assert_eq!(html.matches("<br>").count(), 0, "{html}");

        let text = TextRender.render(&tree, &page_info, &settings);
        assert!(text.contains("WARNING"));
        assert!(text.contains("LEVEL 5 AUTHORIZATION REQUIRED"));
        assert!(!text.contains("+ WARNING"));
        assert!(!text.contains("++ LEVEL 5 AUTHORIZATION REQUIRED"));
    }

    #[test]
    fn native_blockquote_depth_counts_only_contiguous_markers() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = concat!(
            ">> ALPHA_CONTIGUOUS_DEPTH_TWO\n",
            "> > ALPHA_SPACED_LITERAL\n",
            "> >ALPHA_TIGHT_SPACED_LITERAL\n",
            "> ALPHA_BEFORE\n",
            "> >ALPHA_ACTIVE_LITERAL\n",
            "> ALPHA_AFTER\n",
        );
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("<blockquote>").count(), 2, "{html}");
        assert!(html.contains("ALPHA_CONTIGUOUS_DEPTH_TWO"), "{html}");
        assert!(html.contains("&gt; ALPHA_SPACED_LITERAL"), "{html}");
        assert!(html.contains("&gt;ALPHA_TIGHT_SPACED_LITERAL"), "{html}");
        assert!(html.contains("&gt;ALPHA_ACTIVE_LITERAL"), "{html}");
        assert!(html.contains("ALPHA_AFTER"), "{html}");
    }

    #[test]
    fn native_blockquote_horizontal_rule_does_not_consume_outer_block_close() {
        enable_test_logging();

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut input = concat!(
            "[[collapsible]]\n",
            "> Derivative of:\n",
            "> ------\n",
            "> Author\n",
            "[[/collapsible]]\n",
            "After\n",
        )
        .to_owned();
        crate::preprocess(&mut input);
        let tokenization = crate::tokenize(&input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");

        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(html.contains(r#"<div class="collapsible-block""#), "{html}");
        assert!(html.contains("<blockquote>"), "{html}");
        assert!(html.contains("<hr>"), "{html}");
        assert!(html.contains("Author"), "{html}");
        assert!(html.contains("After"), "{html}");
        assert!(!html.contains("[[collapsible"), "{html}");
        assert!(!html.contains("[[/collapsible]]"), "{html}");
        assert_eq!(html.matches("<p>").count(), 3, "{html}");
        assert_eq!(html.matches("<br>").count(), 0, "{html}");
        assert!(html.contains("<p>After</p>"), "{html}");
    }

    #[test]
    fn native_blockquote_line_rules_do_not_trigger_combinatorial_div_retries() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let cases = [
            ("= Centered corpus quote", "text-align: center;"),
            ("+ Quoted heading", "<h1"),
            ("* Quoted list item", "<ul>"),
            ("[[toc]]", "id=\"toc\""),
            ("----", "<hr>"),
        ];

        for (quoted_line, expected) in cases {
            let mut input = String::new();
            for _ in 0..25 {
                input.push_str("[[div class=\"table1\"]]\n> ");
                input.push_str(quoted_line);
                input.push_str("\n[[/div]]\n");
            }
            input.push_str("Following sentinel\n");
            crate::preprocess(&mut input);

            let tokenization = crate::tokenize(&input);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;

            assert!(errors.is_empty(), "{quoted_line}: {errors:#?}");
            assert_eq!(
                html.matches("class=\"table1\"").count(),
                25,
                "{quoted_line}: {html}",
            );
            assert_eq!(
                html.matches("<blockquote>").count(),
                25,
                "{quoted_line}: {html}",
            );
            assert_eq!(html.matches("<br>").count(), 1, "{quoted_line}: {html}",);
            let expected_paragraphs = usize::from(quoted_line.starts_with('='));
            assert_eq!(
                html.matches("<p").count(),
                expected_paragraphs * 25,
                "{quoted_line}: {html}",
            );
            assert!(html.contains(expected), "{quoted_line}: {html}");
            assert!(html.contains("Following sentinel"), "{quoted_line}: {html}");
        }
    }

    #[test]
    fn structural_quote_line_break_policy_is_wikidot_only() {
        let page_info = PageInfo::dummy();
        let input = "> + Quoted heading\n";

        let render = |layout| {
            let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
            let tokenization = crate::tokenize(input);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            assert!(errors.is_empty(), "{layout:?}: {errors:#?}");
            HtmlRender.render(&tree, &page_info, &settings).body
        };

        let wikidot = render(Layout::Wikidot);
        let wikijump = render(Layout::Wikijump);
        assert_eq!(wikidot.matches("<br>").count(), 0, "{wikidot}");
        assert_eq!(wikijump.matches("<br>").count(), 1, "{wikijump}");
    }

    #[test]
    fn multiline_inline_pairs_stay_in_one_native_blockquote() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for (open, close) in [
            ("^^", "^^"),
            ("**", "**"),
            ("//", "//"),
            (",,", ",,"),
            ("__", "__"),
        ] {
            let mut input = String::new();
            for _ in 0..64 {
                input.push_str("> ");
                input.push_str(open);
                input.push_str("\n> quoted text");
                input.push_str(close);
                input.push('\n');
            }

            let tokenization = crate::tokenize(&input);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;

            assert!(errors.is_empty(), "{open}: {errors:#?}");
            assert_eq!(html.matches("<blockquote>").count(), 1, "{open}: {html}");
            assert_eq!(html.matches("<p>").count(), 1, "{open}: {html}");
            assert_eq!(html.matches("<br>").count(), 127, "{open}: {html}");
            assert_eq!(html.matches("quoted text").count(), 64, "{open}: {html}");
        }
    }

    #[test]
    fn native_blockquote_clear_float_row_is_consumed() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut input = String::from(
            "> ----\n\n> ||~ Head A ||~ Head B ||\n> || 1 || 2 ||\n\n> [[div class=\"note\"]]\n> +++ Inside div\n> [[/div]]\n\n> [[=]]\n> Centered\n> [[/=]]\n\n> ~~~~\n",
        );
        crate::preprocess(&mut input);
        let tokenization = crate::tokenize(&input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:#?}");
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(
            html.contains(
                "<div style=\"clear:both; height: 0px; font-size: 1px\"></div>"
            ),
            "{html}",
        );
        assert!(!html.contains("~~~~"), "{html}");
        assert!(
            html.contains("<h3 id=\"toc0\"><span>Inside div</span></h3>"),
            "{html}",
        );
    }

    #[test]
    fn native_blockquote_indented_heading_marker_stays_literal() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut input = String::from(
            "> + Level one\n> ++ Level two\n> +++ Level three\n> ++++ Level four\n> +++++ Level five\n> ++++++ Level six\n\n> +++* Hidden from the table of contents\n> +++ **Bold heading**\n>  +++ Indented is not a heading\n>> +++ Nested depth\n",
        );
        crate::preprocess(&mut input);
        let tokenization = crate::tokenize(&input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:#?}");
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(
            html.contains("<p>+++ Indented is not a heading</p>"),
            "{html}",
        );
        assert!(
            html.contains("<h3 id=\"toc7\"><span>Nested depth</span></h3>"),
            "{html}",
        );
        assert!(!html.contains("toc8"), "{html}");
    }

    #[test]
    fn native_blockquote_indented_list_items_nest() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut input = String::from(
            "> * Apple\n>  * Banana\n>   * Cherry\n> * Durian\n\n> # One\n> # Two\n\n> : Term : Definition\n",
        );
        crate::preprocess(&mut input);
        let tokenization = crate::tokenize(&input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:#?}");
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(html.contains("<li>Apple\n<ul>"), "{html}");
        assert!(html.contains("<li>Banana\n<ul>"), "{html}");
        assert!(html.contains("<li>Cherry</li>"), "{html}");
        assert!(html.contains("<li>Durian</li>"), "{html}");
        assert_eq!(html.matches("<ul>").count(), 3, "{html}");
        assert_eq!(html.matches("</ul>").count(), 3, "{html}");
        assert!(html.contains("<li>One</li>"), "{html}");
        assert!(html.contains("<dt>Term</dt>"), "{html}");
    }
}
