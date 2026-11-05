/*
 * parsing/rule/impls/block/blocks/tabs.rs
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

use super::super::mapping::get_block_rule_with_name_for_layout;
use super::prelude::*;
use crate::parsing::ParserWrap;
use crate::parsing::collect::{
    consume_iftags_segment_fused, consume_valid_comment, iftags_segment_fused,
};
use crate::parsing::consume::consume;
use crate::parsing::paragraph::{ParagraphStack, gather_paragraphs};
use crate::parsing::parser::{QuoteBodyLineStatus, QuoteEscapedUnit};
use crate::parsing::rule::impls::block::parser::WikidotHeadSpaceRegion;
use crate::tree::{
    AcceptsPartial, AttributeMap, Container, LinkLabel, LinkLocation, PartialElement,
    Tab, TabViewItem,
};

pub const BLOCK_TABVIEW: BlockRule = BlockRule {
    name: "block-tabview",
    accepts_names: &["tabview", "tabs"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn: parse_tabview,
};

pub const BLOCK_TAB: BlockRule = BlockRule {
    name: "block-tab",
    accepts_names: &["tab"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn: parse_tab,
};

fn wikidot_quote_opener_at_row_start(source: &str, opener_start: usize) -> bool {
    let line_start = source[..opener_start]
        .rfind('\n')
        .map_or(0, |offset| offset + 1);
    let mut prefix = &source[line_start..opener_start];
    let mut saw_quote = false;
    while let Some(rest) = prefix.strip_prefix('>') {
        saw_quote = true;
        prefix = rest.strip_prefix(' ').unwrap_or(rest);
    }
    saw_quote && prefix.is_empty()
}

fn close_content_run<'t>(
    run: &mut Vec<Element<'t>>,
    items: &mut Vec<TabViewItem<'t>>,
    runs: &mut usize,
    pop_trailing: bool,
) {
    if pop_trailing {
        while matches!(run.last(), Some(element) if element.is_whitespace()) {
            run.pop();
        }
    } else if !run.is_empty()
        && !matches!(run.last(), Some(element) if element.is_whitespace())
    {
        run.push(Element::LineBreak);
    }
    if !run.is_empty() {
        *runs += 1;
        items.push(TabViewItem::Content(std::mem::take(run)));
    }
    run.clear();
}

fn wikidot_failed_tab_gap_valid(lower: &[u8], start: usize, end: usize) -> bool {
    lower[start..end]
        .iter()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
}

fn wikidot_find_failed_opener(lower: &[u8], from: usize) -> Option<usize> {
    let mut best = None;
    for pattern in [b"[[tab ]]" as &[u8], b"[[tab\n]]"] {
        let mut offset = from;
        while offset + pattern.len() <= lower.len() {
            if &lower[offset..offset + pattern.len()] == pattern {
                if best.is_none_or(|found| offset < found) {
                    best = Some(offset);
                }
                break;
            }
            offset += 1;
        }
    }
    best
}

fn wikidot_find_leading_opener(lower: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut offset = from;
    while offset + 7 <= lower.len() {
        if lower[offset] == b'['
            && lower[offset + 1] == b'['
            && matches!(lower[offset + 2], b' ' | b'\t')
        {
            let mut cursor = offset + 2;
            while cursor < lower.len() && matches!(lower[cursor], b' ' | b'\t') {
                cursor += 1;
            }
            if lower[cursor..].starts_with(b"tab") {
                let mut end = cursor + 3;
                while end < lower.len()
                    && !matches!(lower[end], b'\n' | b'\r' | b'[' | b']')
                {
                    end += 1;
                }
                if lower[end..].starts_with(b"]]") {
                    return Some((offset, end + 2));
                }
            }
        }
        offset += 1;
    }
    None
}

fn wikidot_find_tab_closer(lower: &[u8], from: usize) -> Option<usize> {
    let pattern = b"[[/tab]]" as &[u8];
    let mut offset = from;
    while offset + pattern.len() <= lower.len() {
        if &lower[offset..offset + pattern.len()] == pattern {
            return Some(offset);
        }
        offset += 1;
    }
    None
}

fn wikidot_failed_tab_push_link_target(joined: &mut String, link: &LinkLocation) {
    match link {
        LinkLocation::Url(url) => joined.push_str(url),
        LinkLocation::Page(page) => {
            joined.push_str(&page.page);
            if let Some(extra) = &page.extra {
                joined.push_str(extra);
            }
        }
    }
}

fn wikidot_tabview_failed_content_gate(items: &[TabViewItem]) -> bool {
    let tab_count = items
        .iter()
        .filter(|item| matches!(item, TabViewItem::Tab(_)))
        .count();
    let last_tab = items
        .iter()
        .rposition(|item| matches!(item, TabViewItem::Tab(_)));
    let content_after_last_tab = match last_tab {
        Some(index) => items[index + 1..]
            .iter()
            .any(|item| matches!(item, TabViewItem::Content(_))),
        None => false,
    };
    let mut singles = 0usize;
    let mut inside: Option<bool> = None;
    for item in items {
        let TabViewItem::Content(content) = item else {
            continue;
        };
        let mut joined = String::new();
        for element in content {
            match element {
                Element::Text(text) => joined.push_str(text),
                Element::LineBreak => joined.push('\n'),
                Element::Link { link, label, .. } => {
                    joined.push('[');
                    wikidot_failed_tab_push_link_target(&mut joined, link);
                    joined.push(' ');
                    match label {
                        LinkLabel::Text(text)
                        | LinkLabel::Slug(text)
                        | LinkLabel::Title(text) => {
                            joined.push_str(text);
                        }
                        LinkLabel::Url | LinkLabel::Page => {
                            wikidot_failed_tab_push_link_target(&mut joined, link);
                        }
                    }
                    joined.push(']');
                }
                _ => joined.push('\0'),
            }
        }
        let lower = joined.to_ascii_lowercase();
        let bytes = lower.as_bytes();
        let mut pos = 0usize;
        loop {
            if inside.is_some() {
                match wikidot_find_tab_closer(bytes, pos) {
                    Some(close) => {
                        pos = close + 8;
                        inside = None;
                    }
                    None => break,
                }
            } else {
                let single = wikidot_find_failed_opener(bytes, pos);
                let leading = wikidot_find_leading_opener(bytes, pos);
                let next_opener = match (single, leading) {
                    (Some(open), Some((start, end))) if start < open => {
                        Some((start, end, true))
                    }
                    (Some(open), _) => Some((open, open + 8, false)),
                    (None, Some((start, end))) => Some((start, end, true)),
                    (None, None) => None,
                };
                let next_closer = wikidot_find_tab_closer(bytes, pos);
                match (next_opener, next_closer) {
                    (None, None) => {
                        if !wikidot_failed_tab_gap_valid(bytes, pos, bytes.len()) {
                            return false;
                        }
                        break;
                    }
                    (None, Some(close)) => {
                        if !wikidot_failed_tab_gap_valid(bytes, pos, close) {
                            return false;
                        }
                        pos = close + 8;
                    }
                    (Some((open, end, leading)), None) => {
                        if !wikidot_failed_tab_gap_valid(bytes, pos, open) {
                            return false;
                        }
                        pos = end;
                        inside = Some(leading);
                        if !leading {
                            singles += 1;
                        }
                    }
                    (Some((open, end, leading)), Some(close)) => {
                        if close < open {
                            if !wikidot_failed_tab_gap_valid(bytes, pos, close) {
                                return false;
                            }
                            pos = close + 8;
                        } else {
                            if !wikidot_failed_tab_gap_valid(bytes, pos, open) {
                                return false;
                            }
                            pos = end;
                            inside = Some(leading);
                            if !leading {
                                singles += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    match inside {
        Some(true) => false,
        Some(false) => tab_count > 0 && !content_after_last_tab,
        None => tab_count > 0 || singles > 0,
    }
}

fn parse_tabview<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    if parser.settings().layout.legacy()
        && parser.native_blockquote_depth().is_some()
        && !wikidot_quote_opener_at_row_start(parser.full_text().inner(), opener_start)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if parser.wikidot_footnote_zone_blocks(opener_start) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if parser.settings().layout.legacy() && parser.in_wikidot_bibliography_body() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if parser.settings().layout.legacy()
        && parser
            .full_text()
            .inner()
            .as_bytes()
            .get(opener_start + 2)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let quote_ejection =
        parser.settings().layout.legacy() && parser.native_blockquote_depth().is_some();

    let parser = &mut ParserWrap::new(parser, AcceptsPartial::Tab);

    debug!("Parsing tabview block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Tabview doesn't allow star flag");
    assert!(!flag_score, "Tabview doesn't allow score flag");
    assert_block_name(&BLOCK_TABVIEW, name);

    let (tabview_name, body_start) = parser.get_head_value_resolved_with_body_start(
        &BLOCK_TABVIEW,
        in_head,
        |_, value| Ok(value),
    )?;

    let records_at_entry = parser.degraded_title_record_count();

    let previous_ejection = parser.wikidot_tab_quote_ejection();
    if quote_ejection {
        parser.set_wikidot_tab_quote_ejection(true);
        parser.set_wikidot_body_soft_ended(false);
    }

    let (elements, mut errors, _) =
        match parser.get_body_elements_with_context(&BLOCK_TABVIEW, false, body_start) {
            Ok(success) => success.into(),
            Err(error) => {
                if quote_ejection {
                    parser.set_wikidot_tab_quote_ejection(previous_ejection);
                }
                parser.truncate_degraded_title_records(records_at_entry);
                return Err(error);
            }
        };

    if quote_ejection {
        let soft_ended = parser.take_wikidot_body_soft_ended();
        let degraded = if soft_ended {
            consume_degraded_tabview_close(parser, &mut errors)
        } else {
            Ok(())
        };
        parser.set_wikidot_tab_quote_ejection(previous_ejection);
        if let Err(error) = degraded {
            parser.truncate_degraded_title_records(records_at_entry);
            return Err(error);
        }
    }

    let mut items = Vec::new();
    let mut run = Vec::new();
    let mut runs = 0usize;

    let legacy = parser.settings().layout.legacy();
    for element in elements {
        match element {
            Element::Partial(PartialElement::Tab(tab)) => {
                close_content_run(&mut run, &mut items, &mut runs, !legacy);
                items.push(TabViewItem::Tab(tab));
            }

            element if element.is_whitespace() && run.is_empty() => (),

            element => run.push(element),
        }
    }
    close_content_run(&mut run, &mut items, &mut runs, true);

    let records_added = parser.degraded_title_record_count() - records_at_entry;
    let native = !parser.settings().layout.legacy();
    parser.truncate_degraded_title_records(records_at_entry);

    if runs > 0 && native && runs != records_added {
        return Err(parser.make_err(ParseErrorKind::TabViewContainsNonTab));
    }

    // A failed tab head (one whose title is an elided footnoteblock) leaves its
    // source literal inside the view; live keeps the view and renders that
    // content instead of failing the whole tabview.
    if runs > 0
        && !native
        && runs > records_added
        && !wikidot_tabview_failed_content_gate(&items)
    {
        return Err(parser.make_err(ParseErrorKind::TabViewContainsNonTab));
    }

    if items.is_empty() {
        return Err(parser.make_err(ParseErrorKind::TabViewEmpty));
    }

    let tabview = Element::TabView(items);
    if parser.settings().layout.legacy()
        && let Some(name) = tabview_name
    {
        return ok!(
            false;
            vec![
                Element::Text(std::borrow::Cow::Owned(format!("{name} {name}\n"))),
                tabview,
            ],
            errors
        );
    }
    success_elements_with_paragraph_safety(false, tabview, errors)
}

fn classify_quote_line<'r, 't>(parser: &Parser<'r, 't>) -> Option<(usize, Parser<'r, 't>)>
where
    'r: 't,
{
    let mut probe = parser.clone();
    let mut depth = 0usize;
    while probe.current().token == Token::Quote {
        depth += probe.current().slice.len();
        probe.step().ok()?;
        probe.get_optional_space().ok()?;
    }
    if depth == 0 {
        return None;
    }
    Some((depth, probe))
}

fn end_block_tag_at<'r, 't>(parser: &Parser<'r, 't>) -> Option<&'static str>
where
    'r: 't,
{
    let mut probe = parser.clone();
    let name = probe.get_end_block().ok()?;
    let tag = if name.eq_ignore_ascii_case("tab") {
        "tab"
    } else if name.eq_ignore_ascii_case("tabview") || name.eq_ignore_ascii_case("tabs") {
        "tabview"
    } else {
        return None;
    };
    let _ = probe.get_optional_space();
    matches!(
        probe.current().token,
        Token::LineBreak | Token::ParagraphBreak | Token::InputEnd | Token::LeftBlockEnd
    )
    .then_some(tag)
}

fn consume_degraded_tabview_close<'r, 't>(
    parser: &mut Parser<'r, 't>,
    errors: &mut Vec<ParseError>,
) -> Result<(), ParseError>
where
    'r: 't,
{
    let Some((depth, after_prefix)) = classify_quote_line(parser) else {
        return Err(parser.make_err(ParseErrorKind::EndOfInput));
    };
    if end_block_tag_at(&after_prefix) != Some("tabview") {
        return Err(parser.make_err(ParseErrorKind::EndOfInput));
    }
    let saved_cursor = parser.quote_body_cursor();
    parser.install_quote_body_cursor(depth);
    let result = parser.get_body_elements(&BLOCK_TABVIEW, false);
    parser.set_quote_body_cursor(saved_cursor);
    let (_elements, close_errors, _) = result?.into();
    errors.extend(close_errors);
    if parser.current().token == Token::LineBreak {
        parser.step()?;
    }
    Ok(())
}

fn collect_quote_scan_line<'r, 't>(
    parser: &mut Parser<'r, 't>,
    errors: &mut Vec<ParseError>,
) -> Result<(Vec<Element<'t>>, bool), ParseError>
where
    'r: 't,
{
    let line_end = std::iter::once(parser.current())
        .chain(parser.remaining().iter())
        .find(|token| {
            matches!(
                token.token,
                Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
            )
        })
        .expect("tokenization always ends with input-end")
        .span
        .end;
    let mut elements = Vec::new();
    let mut paragraph_safe = true;
    loop {
        if matches!(
            parser.current().token,
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
        ) {
            break;
        }
        let before = parser.current().span.start;
        let success = consume(parser)?;
        let crate::parsing::ParseSuccess {
            item,
            errors: chunk_errors,
            paragraph_safe: chunk_safe,
            ..
        } = success;
        errors.extend(chunk_errors);
        paragraph_safe &= chunk_safe;
        match item {
            Elements::Multiple(chunk) => elements.extend(chunk),
            Elements::Single(element) => elements.push(element),
            Elements::None => {}
        }
        if parser.current().span.start >= line_end
            || parser.current().span.start == before
        {
            break;
        }
    }
    Ok((elements, paragraph_safe))
}

fn gather_quote_ejecta_run<'r, 't>(
    parser: &mut Parser<'r, 't>,
    errors: &mut Vec<ParseError>,
) -> Result<(Vec<Element<'t>>, bool), ParseError>
where
    'r: 't,
{
    let saved_cursor = parser.quote_body_cursor();
    let saved_depth = parser.native_blockquote_depth();
    parser.set_quote_body_cursor(None);
    parser.set_native_blockquote_depth(None);
    let rule = parser.rule();
    let result = gather_paragraphs(
        parser,
        rule,
        Some(|p: &mut Parser<'r, 't>| {
            Ok(p.start_of_line() && p.current().token == Token::Quote)
        }),
    );
    parser.set_quote_body_cursor(saved_cursor);
    parser.set_native_blockquote_depth(saved_depth);
    let (elements, run_errors, paragraph_safe) = result?.into();
    errors.extend(run_errors);
    Ok((elements, paragraph_safe))
}

fn form_quote_scan_segment<'t>(
    row_elements: Vec<Element<'t>>,
    wrap_depth: usize,
) -> Vec<Element<'t>> {
    let mut stack = ParagraphStack::new_wikidot();
    for element in row_elements {
        stack.push_element(element, true);
    }
    stack.pop_line_break();
    let mut formed = stack.into_elements();
    for _ in 0..wrap_depth {
        formed = vec![Element::Container(Container::new(
            ContainerType::Blockquote,
            formed,
            AttributeMap::new(),
        ))];
    }
    formed
}

fn flush_quote_scan_segment<'t>(
    units: &mut Vec<QuoteEscapedUnit<'t>>,
    pending: &mut Option<(usize, Vec<Element<'t>>)>,
) {
    if let Some((depth, elements)) = pending.take() {
        units.push(QuoteEscapedUnit::Elements(
            form_quote_scan_segment(elements, depth),
            false,
        ));
    }
}

fn empty_blockquote_nest<'t>(nesting: usize) -> Vec<Element<'t>> {
    let mut inner: Vec<Element<'t>> = Vec::new();
    for _ in 0..nesting {
        let mut attributes = AttributeMap::new();
        attributes.insert(crate::tree::WIKIDOT_EMPTY_QUOTE_MARKER, cow!(""));
        inner = vec![Element::Container(Container::new(
            ContainerType::Blockquote,
            inner,
            attributes,
        ))];
    }
    inner
}

fn append_blockquote_tail<'t>(elements: &mut Vec<Element<'t>>, nesting: usize) {
    if matches!(
        elements.last(),
        Some(Element::Container(container))
            if container.ctype() == ContainerType::Blockquote
    ) {
        if let Some(Element::Container(container)) = elements.last_mut() {
            append_blockquote_tail(container.elements_mut(), nesting);
        }
        return;
    }
    elements.extend(empty_blockquote_nest(nesting));
}

fn quote_scan_segment_open_depth(segment: &[Element]) -> usize {
    let mut depth = 0;
    let mut current = segment;
    loop {
        match current.last() {
            Some(Element::Container(container))
                if container.ctype() == ContainerType::Blockquote =>
            {
                depth += 1;
                current = container.elements();
            }
            _ => break,
        }
    }
    depth
}

fn quote_scan_open_depth<'t>(units: &[QuoteEscapedUnit<'t>]) -> usize {
    match units.last() {
        Some(QuoteEscapedUnit::Elements(segment, _)) => {
            quote_scan_segment_open_depth(segment)
        }
        Some(QuoteEscapedUnit::Row(depth, _, _)) => *depth,
        None => 0,
    }
}

/// After a tab ejection the rows holding the remaining tab/tabview closers
/// still open their blockquote run in live, even though their content is
/// consumed: the run survives as an empty blockquote (nested when the closer
/// rows are deeper than the row that was open).
fn append_quote_scan_closer_nest<'t>(
    units: &mut Vec<QuoteEscapedUnit<'t>>,
    closer_depth: usize,
) {
    let open_depth = quote_scan_open_depth(units);
    if closer_depth <= open_depth {
        return;
    }
    let nesting = closer_depth - open_depth;
    match units.last_mut() {
        Some(QuoteEscapedUnit::Elements(segment, _)) => {
            append_blockquote_tail(segment, nesting);
        }
        Some(QuoteEscapedUnit::Row(row_depth, _, _)) => {
            let row_depth = *row_depth;
            units.push(QuoteEscapedUnit::Row(
                row_depth,
                empty_blockquote_nest(nesting),
                false,
            ));
        }
        None => {
            units.push(QuoteEscapedUnit::Elements(
                empty_blockquote_nest(nesting),
                false,
            ));
        }
    }
}

fn scan_tab_close_after_soft_end<'r, 't>(
    parser: &mut Parser<'r, 't>,
    required_depth: usize,
    errors: &mut Vec<ParseError>,
) -> Result<(), ParseError>
where
    'r: 't,
{
    let mut units: Vec<QuoteEscapedUnit<'t>> = Vec::new();
    let mut pending_segment: Option<(usize, Vec<Element<'t>>)> = None;
    let mut saw_content = false;
    loop {
        match parser.current().token {
            Token::InputEnd => {
                return Err(parser.make_err(ParseErrorKind::EndOfInput));
            }
            Token::ParagraphBreak | Token::LineBreak => {
                parser.step()?;
            }
            Token::Quote => {
                let Some((depth, after_prefix)) = classify_quote_line(parser) else {
                    return Err(parser.make_err(ParseErrorKind::EndOfInput));
                };
                if after_prefix.current().token == Token::LineBreak {
                    while parser.current().token == Token::Quote {
                        parser.step()?;
                        parser.get_optional_space()?;
                    }
                    parser.step()?;
                    continue;
                }
                match end_block_tag_at(&after_prefix) {
                    Some("tabview") if depth <= required_depth => {
                        return Err(parser.make_err(ParseErrorKind::EndOfInput));
                    }
                    Some("tab") if depth <= required_depth => {
                        if depth < required_depth {
                            parser.install_quote_body_cursor(depth);
                        }
                        let (_elements, close_errors, _) =
                            parser.get_body_elements(&BLOCK_TAB, true)?.into();
                        errors.extend(close_errors);
                        flush_quote_scan_segment(&mut units, &mut pending_segment);
                        if saw_content {
                            append_quote_scan_closer_nest(&mut units, depth);
                        }
                        break;
                    }
                    _ => {
                        saw_content = true;
                        let saved_cursor = parser.quote_body_cursor();
                        let saved_depth = parser.native_blockquote_depth();
                        while parser.current().token == Token::Quote {
                            parser.step()?;
                            parser.get_optional_space()?;
                        }
                        if parser.current().token != Token::Quote {
                            parser.mark_virtual_start_of_line();
                        }
                        parser.install_quote_body_cursor_at_current_line(depth);
                        parser.set_native_blockquote_depth(Some(depth));
                        let collected = collect_quote_scan_line(parser, errors);
                        parser.set_quote_body_cursor(saved_cursor);
                        parser.set_native_blockquote_depth(saved_depth);
                        let (row_elements, row_safe) = collected?;
                        if depth < required_depth {
                            flush_quote_scan_segment(&mut units, &mut pending_segment);
                            let run_ended = units.iter().any(|unit| {
                                matches!(unit, QuoteEscapedUnit::Elements(..))
                            });
                            if run_ended {
                                units.push(QuoteEscapedUnit::Elements(
                                    form_quote_scan_segment(row_elements, depth),
                                    false,
                                ));
                            } else {
                                units.push(QuoteEscapedUnit::Row(
                                    depth,
                                    row_elements,
                                    row_safe,
                                ));
                            }
                        } else {
                            match &mut pending_segment {
                                Some((pending_depth, elements))
                                    if *pending_depth == depth =>
                                {
                                    elements.push(Element::LineBreak);
                                    elements.extend(row_elements);
                                }
                                _ => {
                                    flush_quote_scan_segment(
                                        &mut units,
                                        &mut pending_segment,
                                    );
                                    pending_segment = Some((depth, row_elements));
                                }
                            }
                        }
                    }
                }
            }
            _ => {
                saw_content = true;
                flush_quote_scan_segment(&mut units, &mut pending_segment);
                let (elements, safe) = gather_quote_ejecta_run(parser, errors)?;
                units.push(QuoteEscapedUnit::Elements(elements, safe));
            }
        }
    }
    for unit in units {
        parser.push_quote_escaped_unit(unit);
    }
    Ok(())
}

fn tab_label_finish<'t>(
    source: &'t str,
    start: usize,
    output: Option<String>,
    copied_until: usize,
    end: usize,
    saw_unit: bool,
) -> std::borrow::Cow<'t, str> {
    if saw_unit {
        return std::borrow::Cow::Borrowed("");
    }
    match output {
        Some(mut output) => {
            if end >= copied_until {
                output.push_str(&source[copied_until..end]);
            }
            std::borrow::Cow::Owned(output)
        }
        None => std::borrow::Cow::Borrowed(&source[start..end]),
    }
}

fn tab_title_unit_end<'r, 't>(
    parser: &Parser<'r, 't>,
    probe: &Parser<'r, 't>,
    closer_name: Option<&str>,
) -> bool {
    if probe.current().token == Token::InputEnd {
        return false;
    }
    let start = parser.current().span.start;
    let end = probe.current().span.start;
    let layout = parser.settings().layout;
    let mut terminator = false;
    let tokens: Vec<&crate::parsing::ExtractedToken<'t>> =
        std::iter::once(parser.current())
            .chain(parser.remaining())
            .take_while(|token| token.span.start < end)
            .collect();
    let _ = start;
    for (index, token) in tokens.iter().enumerate() {
        match token.token {
            Token::LineBreak | Token::ParagraphBreak => return false,
            Token::RightBlock | Token::RightLink => {
                if closer_name.is_none() {
                    terminator = true;
                }
            }
            Token::LeftBlockEnd => {
                let name = tokens.get(index + 1).map_or("", |next| next.slice);
                if name.eq_ignore_ascii_case("tab")
                    || name.eq_ignore_ascii_case("tabview")
                    || name.eq_ignore_ascii_case("tabs")
                {
                    return false;
                }
                if let Some(closer) = closer_name
                    && name.eq_ignore_ascii_case(closer)
                {
                    terminator = true;
                }
            }
            _ => {}
        }
    }
    let _ = layout;
    terminator
}

fn tab_title_unit_fused_end<'r, 't>(
    parser: &Parser<'r, 't>,
    probe: &Parser<'r, 't>,
) -> bool {
    let current = probe.current();
    if current.token != Token::RightBracket {
        return false;
    }
    let end = current.span.start;
    let last = std::iter::once(parser.current())
        .chain(parser.remaining())
        .take_while(|token| token.span.start < end)
        .last();
    matches!(last, Some(token) if token.token == Token::RightLink && token.span.end == end)
}

fn tab_title_block_unit<'r, 't>(parser: &Parser<'r, 't>) -> Option<(Parser<'r, 't>, bool)>
where
    'r: 't,
{
    let mut name_probe = parser.clone();
    let (name, _, _) = name_probe.get_block_name(false).ok()?;
    let rule = get_block_rule_with_name_for_layout(&name, parser.settings().layout)?;
    let (closer_name, head_only) = match rule.name {
        "block-span" => (Some("span"), false),
        "block-size" => (Some("size"), false),
        "block-collapsible" => (Some("collapsible"), false),
        "block-anchor" => (Some("a"), false),
        "block-user" => (None, true),
        "block-date" => (None, true),
        "block-equation-ref" => (None, true),
        _ => return None,
    };
    let mut probe = parser.clone();
    let success = match consume(&mut probe) {
        Ok(success) => success,
        Err(_) => {
            if head_only {
                return tab_title_head_only_fused_unit(parser);
            }
            let closer = closer_name?;
            return tab_title_scoped_block_unit(parser, rule.name, closer);
        }
    };
    if matches!(success.item, Elements::None) {
        return None;
    }
    let fused_end = tab_title_unit_fused_end(parser, &probe);
    if head_only {
        if tab_title_unit_end(parser, &probe, None) {
            return Some((probe, fused_end));
        }
        return tab_title_head_only_fused_unit(parser);
    }
    if tab_title_unit_end(parser, &probe, closer_name) {
        return Some((probe, fused_end));
    }
    let closer = closer_name?;
    tab_title_scoped_block_unit(parser, rule.name, closer)
}

fn tab_title_token_block_name(
    tokens: &[&crate::parsing::ExtractedToken],
    index: usize,
    layout: crate::layout::Layout,
) -> Option<&'static str> {
    let mut cursor = index + 1;
    if tokens.get(cursor).map(|token| token.token) == Some(Token::Whitespace) {
        cursor += 1;
    }
    let raw = tokens.get(cursor)?.slice;
    let name = raw.strip_suffix('_').unwrap_or(raw);
    get_block_rule_with_name_for_layout(name, layout).map(|rule| rule.name)
}

fn tab_title_scoped_block_unit<'r, 't>(
    parser: &Parser<'r, 't>,
    rule_name: &'static str,
    _closer_name: &str,
) -> Option<(Parser<'r, 't>, bool)>
where
    'r: 't,
{
    let layout = parser.settings().layout;
    let tokens: Vec<&crate::parsing::ExtractedToken<'t>> =
        std::iter::once(parser.current())
            .chain(parser.remaining())
            .collect();
    if tab_title_token_block_name(&tokens, 0, layout) != Some(rule_name) {
        return None;
    }
    let mut depth = 0_usize;
    let mut end_close = None;
    let mut index = 0_usize;
    while index < tokens.len() {
        let token = tokens[index];
        match token.token {
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                return None;
            }
            Token::LeftBlock | Token::LeftBlockStar => {
                if tab_title_token_block_name(&tokens, index, layout) == Some(rule_name) {
                    depth += 1;
                }
            }
            Token::LeftBlockEnd => {
                let name = tab_title_token_block_name(&tokens, index, layout);
                if matches!(name, Some("block-tab") | Some("block-tabview")) {
                    return None;
                }
                if name == Some(rule_name) {
                    if depth <= 1 {
                        let mut close_index = index + 1;
                        let close = loop {
                            let close_token = tokens.get(close_index)?;
                            match close_token.token {
                                Token::RightBlock | Token::RightLink => {
                                    break *close_token;
                                }
                                Token::LineBreak
                                | Token::ParagraphBreak
                                | Token::InputEnd => {
                                    return None;
                                }
                                _ => {
                                    close_index += 1;
                                }
                            }
                        };
                        end_close = Some(close);
                        break;
                    }
                    depth -= 1;
                }
            }
            _ => {}
        }
        index += 1;
    }
    let close = end_close?;
    let start = parser.current().span.start;
    let slice = &parser.full_text().inner()[start..close.span.start + 2];
    let tokenization = crate::tokenize(slice);
    let outcome =
        crate::parsing::parse(&tokenization, parser.page_info(), parser.settings());
    let (tree, _) = outcome.into();
    if !tab_title_single_construct(&tree.elements) {
        return None;
    }
    let mut probe = parser.clone();
    while probe.current().span.start < close.span.end {
        probe.step().ok()?;
    }
    let fused_end = tab_title_unit_fused_end(parser, &probe);
    Some((probe, fused_end))
}

fn tab_title_single_construct(elements: &[Element]) -> bool {
    match elements {
        [Element::Container(container)]
            if container.ctype() == crate::tree::ContainerType::Paragraph =>
        {
            tab_title_single_construct(container.elements())
        }
        [element] => !matches!(element, Element::Text(_)),
        _ => false,
    }
}

fn tab_title_head_only_fused_unit<'r, 't>(
    parser: &Parser<'r, 't>,
) -> Option<(Parser<'r, 't>, bool)>
where
    'r: 't,
{
    let start = parser.current().span.start;
    let tokens: Vec<&crate::parsing::ExtractedToken<'t>> =
        std::iter::once(parser.current())
            .chain(parser.remaining())
            .collect();
    let mut close = None;
    for token in &tokens {
        match token.token {
            Token::RightBlock | Token::RightLink => {
                close = Some(*token);
                break;
            }
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                return None;
            }
            _ => {}
        }
    }
    let close = close?;
    let slice = &parser.full_text().inner()[start..close.span.start + 2];
    let tokenization = crate::tokenize(slice);
    let outcome =
        crate::parsing::parse(&tokenization, parser.page_info(), parser.settings());
    let (tree, _) = outcome.into();
    if !tab_title_single_construct(&tree.elements) {
        return None;
    }
    let mut probe = parser.clone();
    while probe.current().span.start < close.span.end {
        probe.step().ok()?;
    }
    let fused_end = tab_title_unit_fused_end(parser, &probe);
    Some((probe, fused_end))
}

fn tab_title_link_unit<'r, 't>(parser: &Parser<'r, 't>) -> Option<(Parser<'r, 't>, bool)>
where
    'r: 't,
{
    let mut probe = parser.clone();
    let success = consume(&mut probe).ok()?;
    if matches!(success.item, Elements::None) {
        return None;
    }
    let end = probe.current().span.start;
    let mut terminator = false;
    for token in std::iter::once(parser.current())
        .chain(parser.remaining())
        .take_while(|token| token.span.start < end)
    {
        match token.token {
            Token::LineBreak | Token::ParagraphBreak => return None,
            Token::RightLink => terminator = true,
            Token::LeftBlockEnd => {
                return None;
            }
            _ => {}
        }
    }
    let fused_end = tab_title_unit_fused_end(parser, &probe);
    terminator.then_some((probe, fused_end))
}

fn advance_parser_to<'r, 't>(
    parser: &mut Parser<'r, 't>,
    target: &Parser<'r, 't>,
) -> Result<(), ParseError> {
    let end = target.current().span.start;
    while parser.current().span.start < end {
        parser.step()?;
    }
    Ok(())
}

enum TabTitleFootnote<'r, 't> {
    NotFootnote,
    Break,
    Unit(Parser<'r, 't>, bool),
    Elided(Parser<'r, 't>, bool),
}

fn tab_title_footnote_construct<'r, 't>(
    parser: &Parser<'r, 't>,
    title_has_content: bool,
) -> TabTitleFootnote<'r, 't>
where
    'r: 't,
{
    let layout = parser.settings().layout;
    let tokens: Vec<&crate::parsing::ExtractedToken<'t>> =
        std::iter::once(parser.current())
            .chain(parser.remaining())
            .collect();
    match tab_title_token_block_name(&tokens, 0, layout) {
        Some("block-footnote") => {
            if !tab_title_footnote_closes(&tokens, layout) {
                return TabTitleFootnote::NotFootnote;
            }
            let footnote_count = parser.footnote_count();
            let mut probe = parser.clone();
            let outer_fused_end = probe.title_footnote_fused_end();
            probe.set_title_footnote_fused_end(true);
            let probe_result = consume(&mut probe);
            probe.set_title_footnote_fused_end(outer_fused_end);
            let consumed = match probe_result {
                Ok(success) => {
                    success
                        .item
                        .as_ref()
                        .iter()
                        .any(|element| matches!(element, Element::Footnote(_)))
                        && probe.footnote_count() == footnote_count + 1
                }
                Err(_) => false,
            };
            if !consumed {
                probe.truncate_footnotes(footnote_count);
                return TabTitleFootnote::NotFootnote;
            }
            let end = probe.current().span.start;
            let crossed_break = std::iter::once(parser.current())
                .chain(parser.remaining())
                .take_while(|token| token.span.start < end)
                .any(|token| {
                    matches!(token.token, Token::LineBreak | Token::ParagraphBreak)
                });
            if crossed_break {
                probe.truncate_footnotes(footnote_count);
                return TabTitleFootnote::NotFootnote;
            }
            if !title_has_content {
                probe.truncate_footnotes(footnote_count);
                return TabTitleFootnote::Break;
            }
            let fused_end = tab_title_unit_fused_end(parser, &probe);
            TabTitleFootnote::Unit(probe, fused_end)
        }
        Some("block-footnote-block") => {
            match tab_title_footnoteblock_probe(parser, &tokens) {
                Some(probe) => {
                    let fused_end = tab_title_unit_fused_end(parser, &probe);
                    TabTitleFootnote::Elided(probe, fused_end)
                }
                None => TabTitleFootnote::NotFootnote,
            }
        }
        _ => TabTitleFootnote::NotFootnote,
    }
}

fn tab_title_footnoteblock_probe<'r, 't>(
    parser: &Parser<'r, 't>,
    tokens: &[&crate::parsing::ExtractedToken<'t>],
) -> Option<Parser<'r, 't>>
where
    'r: 't,
{
    let mut close = None;
    for token in tokens.iter().skip(1) {
        match token.token {
            Token::RightBlock | Token::RightLink => {
                close = Some(*token);
                break;
            }
            Token::LeftBlock
            | Token::LeftBlockEnd
            | Token::LineBreak
            | Token::ParagraphBreak
            | Token::InputEnd => {
                return None;
            }
            _ => {}
        }
    }
    let close = close?;
    let start = parser.current().span.start;
    let slice = &parser.full_text().inner()[start..close.span.start + 2];
    let tokenization = crate::tokenize(slice);
    let outcome =
        crate::parsing::parse(&tokenization, parser.page_info(), parser.settings());
    let (tree, _) = outcome.into();
    if !tab_title_single_footnoteblock(&tree.elements) {
        return None;
    }
    let mut probe = parser.clone();
    while probe.current().span.start < close.span.end {
        probe.step().ok()?;
    }
    Some(probe)
}

fn tab_title_single_footnoteblock(elements: &[Element]) -> bool {
    match elements {
        [Element::Container(container)]
            if container.ctype() == crate::tree::ContainerType::Paragraph =>
        {
            tab_title_single_footnoteblock(container.elements())
        }
        [Element::FootnoteBlock { .. }] => true,
        _ => false,
    }
}

fn tab_title_remainder_empty<'r, 't>(parser: &Parser<'r, 't>) -> bool
where
    'r: 't,
{
    let mut probe = parser.clone();
    loop {
        match probe.current().token {
            Token::RightBlock | Token::RightLink => return true,
            Token::LeftComment => {
                if consume_valid_comment(&mut probe).is_err() {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

fn tab_title_footnote_closes(
    tokens: &[&crate::parsing::ExtractedToken],
    layout: crate::layout::Layout,
) -> bool {
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.token {
            Token::LeftBlock => match tab_title_token_block_name(tokens, index, layout) {
                Some("block-footnote") => depth += 1,
                Some("block-tab") | Some("block-tabview") => return false,
                _ => {}
            },
            Token::LeftBlockEnd => {
                match tab_title_token_block_name(tokens, index, layout) {
                    Some("block-footnote") => {
                        if depth <= 1 {
                            return true;
                        }
                        depth -= 1;
                    }
                    Some("block-tab") | Some("block-tabview") => return false,
                    _ => {}
                }
            }
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                return false;
            }
            _ => {}
        }
    }
    false
}

fn collect_tab_label<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<(std::borrow::Cow<'t, str>, Option<&'t str>, bool), ParseError>
where
    'r: 't,
{
    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let mut copied_until = start;
    let mut output = None::<String>;
    let mut saw_unit = false;
    let mut link_unit_end = None::<usize>;
    let mut elided_footnoteblock = false;

    loop {
        let current = parser.current();
        let pending_link_end = link_unit_end.take();
        match current.token {
            Token::GeneratedPageLink | Token::GeneratedTagLinks => {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            Token::InputEnd => {
                return Err(parser.make_err(ParseErrorKind::EndOfInput));
            }
            Token::LineBreak | Token::ParagraphBreak => {
                return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
            }
            Token::RightBlock => {
                let text = tab_label_finish(
                    source,
                    start,
                    output,
                    copied_until,
                    current.span.start,
                    saw_unit,
                );
                if elided_footnoteblock && !saw_unit && text.is_empty() {
                    parser.record_degraded_title(current.span.start);
                    return Err(parser.make_err(ParseErrorKind::RuleFailed));
                }
                parser.step()?;
                return Ok((text, None, saw_unit));
            }
            Token::RightLink => {
                let text = tab_label_finish(
                    source,
                    start,
                    output,
                    copied_until,
                    current.span.start,
                    saw_unit,
                );
                if elided_footnoteblock && !saw_unit && text.is_empty() {
                    parser.record_degraded_title(current.span.start);
                    return Err(parser.make_err(ParseErrorKind::RuleFailed));
                }
                let leftover = &source[current.span.start + 2..current.span.end];
                parser.step()?;
                return Ok((text, Some(leftover), saw_unit));
            }
            Token::LeftComment => {
                let range = consume_valid_comment(parser)?;
                let output = output.get_or_insert_with(String::new);
                output.push_str(&source[copied_until..range.start]);
                copied_until = range.end;
            }
            Token::LeftBlock => {
                let title_has_content = saw_unit
                    || output
                        .as_deref()
                        .is_some_and(|text| text.chars().any(|c| !c.is_whitespace()))
                    || source[copied_until..current.span.start]
                        .chars()
                        .any(|c| !c.is_whitespace());
                match tab_title_footnote_construct(parser, title_has_content) {
                    TabTitleFootnote::Break => {
                        if !parser.settings().layout.legacy() {
                            parser.record_degraded_title(current.span.start);
                        }
                        return Err(parser.make_err(ParseErrorKind::RuleFailed));
                    }
                    TabTitleFootnote::Unit(unit, fused_end) => {
                        saw_unit = true;
                        output = Some(String::new());
                        advance_parser_to(parser, &unit)?;
                        copied_until = parser.current().span.start;
                        if fused_end {
                            parser.step()?;
                            return Ok((std::borrow::Cow::Borrowed(""), None, saw_unit));
                        }
                        continue;
                    }
                    TabTitleFootnote::Elided(elided, fused_end) => {
                        let title_so_far_empty = !saw_unit
                            && output.as_deref().is_none_or(|text| text.is_empty())
                            && copied_until == start
                            && current.span.start == start;
                        if title_so_far_empty
                            && (fused_end || tab_title_remainder_empty(&elided))
                        {
                            parser.record_degraded_title(current.span.start);
                            return Err(parser.make_err(ParseErrorKind::RuleFailed));
                        }
                        let construct_start = current.span.start;
                        advance_parser_to(parser, &elided)?;
                        let out_text = output.get_or_insert_with(String::new);
                        out_text.push_str(&source[copied_until..construct_start]);
                        copied_until = parser.current().span.start;
                        if title_so_far_empty {
                            elided_footnoteblock = true;
                        }
                        if fused_end {
                            let text = tab_label_finish(
                                source,
                                start,
                                output,
                                copied_until,
                                parser.current().span.start,
                                saw_unit,
                            );
                            if elided_footnoteblock && !saw_unit && text.is_empty() {
                                parser.record_degraded_title(current.span.start);
                                return Err(parser.make_err(ParseErrorKind::RuleFailed));
                            }
                            parser.step()?;
                            return Ok((text, None, saw_unit));
                        }
                        continue;
                    }
                    TabTitleFootnote::NotFootnote => {}
                }
                if iftags_segment_fused(parser).is_some() {
                    let segment = consume_iftags_segment_fused(parser)?
                        .expect("segment presence was just checked");
                    {
                        let output = output.get_or_insert_with(String::new);
                        output.push_str(&source[copied_until..segment.range.start]);
                        if segment.include_body {
                            output.push_str(&source[segment.body.clone()]);
                        }
                    }
                    copied_until = segment.range.end;
                    if segment.fused_closer {
                        let next = parser.current();
                        if next.token == Token::RightBracket
                            && next.span.start == segment.range.end + 1
                        {
                            let text = tab_label_finish(
                                source,
                                start,
                                output,
                                copied_until,
                                segment.range.end,
                                saw_unit,
                            );
                            parser.step()?;
                            return Ok((text, None, saw_unit));
                        }
                    }
                    continue;
                }
                if let Some((unit, fused_end)) = tab_title_block_unit(parser) {
                    saw_unit = true;
                    output = Some(String::new());
                    parser.update(&unit);
                    copied_until = parser.current().span.start;
                    if fused_end {
                        parser.step()?;
                        return Ok((std::borrow::Cow::Borrowed(""), None, saw_unit));
                    }
                    continue;
                }
                return finish_tab_label_at_construct(
                    parser,
                    source,
                    start,
                    output,
                    copied_until,
                    saw_unit,
                );
            }
            Token::RightBracket => {
                if pending_link_end == Some(current.span.start)
                    && let Some(next) = parser.remaining().first()
                    && next.token == Token::RightBracket
                    && next.span.start == current.span.end
                {
                    parser.step()?;
                    parser.step()?;
                    return Ok((std::borrow::Cow::Borrowed(""), None, saw_unit));
                }
                parser.step()?;
            }
            Token::LeftLink => {
                if let Some((unit, _fused_end)) = tab_title_link_unit(parser) {
                    saw_unit = true;
                    output = Some(String::new());
                    parser.update(&unit);
                    copied_until = parser.current().span.start;
                    link_unit_end = Some(copied_until);
                    continue;
                }
                return finish_tab_label_at_construct(
                    parser,
                    source,
                    start,
                    output,
                    copied_until,
                    saw_unit,
                );
            }
            _ => {
                parser.step()?;
            }
        }
    }
}

fn finish_tab_label_at_construct<'r, 't>(
    parser: &mut Parser<'r, 't>,
    source: &'t str,
    start: usize,
    output: Option<String>,
    copied_until: usize,
    saw_unit: bool,
) -> Result<(std::borrow::Cow<'t, str>, Option<&'t str>, bool), ParseError> {
    let tokens: Vec<&crate::parsing::ExtractedToken<'t>> =
        std::iter::once(parser.current())
            .chain(parser.remaining())
            .collect();
    for token in &tokens {
        match token.token {
            Token::RightBlock => {
                let text = tab_label_finish(
                    source,
                    start,
                    output,
                    copied_until,
                    token.span.start,
                    saw_unit,
                );
                while parser.current().span.start < token.span.end {
                    parser.step()?;
                }
                return Ok((text, None, saw_unit));
            }
            Token::RightLink => {
                let text = tab_label_finish(
                    source,
                    start,
                    output,
                    copied_until,
                    token.span.start,
                    saw_unit,
                );
                let leftover = &source[token.span.start + 2..token.span.end];
                while parser.current().span.start < token.span.end {
                    parser.step()?;
                }
                return Ok((text, Some(leftover), saw_unit));
            }
            Token::LineBreak | Token::ParagraphBreak => {
                return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
            }
            Token::InputEnd => {
                return Err(parser.make_err(ParseErrorKind::EndOfInput));
            }
            _ => {}
        }
    }
    Err(parser.make_err(ParseErrorKind::EndOfInput))
}

struct WikidotTabHeadToken<'t> {
    key: Option<(&'t str, &'t str)>,
    raw: &'t str,
}

fn wikidot_tab_head_argument_at(text: &str, start: usize) -> Option<(&str, &str, usize)> {
    let bytes = text.as_bytes();
    let mut index = start;
    while index < bytes.len()
        && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'_' | b'-'))
    {
        index += 1;
    }
    if index == start
        || bytes.get(index) != Some(&b'=')
        || bytes.get(index + 1) != Some(&b'"')
    {
        return None;
    }
    let value_start = index + 2;
    let value_end = value_start + text[value_start..].find('"')?;
    if value_end == value_start {
        return None;
    }
    let end = value_end + 1;
    if bytes
        .get(end)
        .is_some_and(|byte| !byte.is_ascii_whitespace())
    {
        return None;
    }
    Some((&text[start..index], &text[value_start..value_end], end))
}

fn wikidot_tab_head_tokens(text: &str) -> Vec<WikidotTabHeadToken<'_>> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if let Some((key, value, end)) = wikidot_tab_head_argument_at(text, index) {
            tokens.push(WikidotTabHeadToken {
                key: Some((key, value)),
                raw: &text[index..end],
            });
            index = end;
            continue;
        }
        let mut end = index;
        while end < bytes.len() && !bytes[end].is_ascii_whitespace() {
            end += 1;
        }
        tokens.push(WikidotTabHeadToken {
            key: None,
            raw: &text[index..end],
        });
        index = end;
    }
    tokens
}

fn wikidot_tab_head_normalize(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for word in text.split_ascii_whitespace() {
        if !output.is_empty() {
            output.push(' ');
        }
        output.push_str(word);
    }
    output
}

fn wikidot_tab_head_label(text: &str) -> String {
    let tokens = wikidot_tab_head_tokens(text);
    if tokens.is_empty() {
        return "untitled".to_owned();
    }

    if tokens.len() == 1
        && let Some((key, value)) = tokens[0].key
    {
        return if key == "title" {
            wikidot_tab_head_normalize(value)
        } else {
            "untitled".to_owned()
        };
    }

    let trailing_whitespace = text
        .as_bytes()
        .last()
        .is_some_and(|byte| byte.is_ascii_whitespace());
    let mut suffix_start = tokens.len();
    if !trailing_whitespace {
        while suffix_start > 0 && tokens[suffix_start - 1].key.is_some() {
            suffix_start -= 1;
        }
    }
    let suffix = &tokens[suffix_start..];
    let rest = &tokens[..suffix_start];

    let title = if rest.is_empty()
        && let Some(("title", value)) = suffix.first().and_then(|token| token.key)
    {
        Some(value)
    } else {
        suffix.iter().rev().find_map(|token| match token.key {
            Some(("title", value)) => Some(value),
            _ => None,
        })
    };
    if let Some(title) = title {
        return wikidot_tab_head_normalize(title);
    }
    if rest.is_empty() {
        return "untitled".to_owned();
    }

    let mut joined = String::new();
    for token in rest {
        if !joined.is_empty() {
            joined.push(' ');
        }
        joined.push_str(token.raw);
    }
    let joined = wikidot_tab_head_normalize(&joined);
    if wikidot_tab_head_has_raw_construct(&joined) {
        return String::new();
    }
    joined
}

fn wikidot_tab_head_has_raw_construct(text: &str) -> bool {
    let mut search = 0;
    while let Some(offset) = text[search..].find("@@") {
        let close = offset + 2;
        if text[close..].contains("@@") {
            return true;
        }
        search = close;
    }
    let mut search = 0;
    while let Some(offset) = text[search..].find("@<") {
        let close = offset + 2;
        if text[close..].contains(">@") {
            return true;
        }
        search = close;
    }
    false
}

/// The line break that ends the last quoted row of a tab body is the row
/// terminator, not content: live drops it before the panel paragraph closes.
fn trim_wikidot_quote_tab_body_break<'t>(elements: &mut Vec<Element<'t>>) {
    let Some(Element::Container(paragraph)) = elements.last_mut() else {
        return;
    };
    if paragraph.ctype() != ContainerType::Paragraph {
        return;
    }
    if !matches!(paragraph.elements().last(), Some(Element::LineBreak)) {
        return;
    }
    let inner = paragraph.elements_mut();
    inner.pop();
    if inner.is_empty() {
        elements.pop();
    }
}

/// A tab head whose title is an elided `[[footnoteblock]]` fails, but live
/// keeps the head as literal tab content with the footnoteblock construct
/// removed: `[[tab [[footnoteblock]]]]` renders `[[tab ]]`.
fn wikidot_degraded_tab_head_literal<'t>(
    source: &'t str,
    opener_start: usize,
) -> Option<(std::borrow::Cow<'t, str>, usize)> {
    let head_end = source[opener_start..]
        .find('\n')
        .map_or(source.len(), |offset| opener_start + offset);
    let head = &source[opener_start..head_end];
    let lower = head.to_ascii_lowercase();
    let mut search = 0;
    let construct_start = loop {
        let found = lower[search..].find("[[footnoteblock")? + search;
        let after_name = found + 2 + "footnoteblock".len();
        match lower.as_bytes().get(after_name) {
            Some(b']') | Some(b' ') | Some(b'\t') => break found,
            Some(_) => search = after_name,
            None => return None,
        }
    };
    let construct_end = head[construct_start..].find("]]")? + construct_start + 2;
    let closer = head[construct_end..].find("]]")? + construct_end;
    let mut literal = String::new();
    literal.push_str(&head[..construct_start]);
    literal.push_str(&head[closer..closer + 2]);
    Some((std::borrow::Cow::Owned(literal), opener_start + closer + 2))
}

fn parse_tab<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing tab block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Tab doesn't allow star flag");
    assert!(!flag_score, "Tab doesn't allow score flag");
    assert_block_name(&BLOCK_TAB, name);

    let legacy = parser.settings().layout.legacy();
    let leading_space = legacy
        && parser
            .full_text()
            .inner()
            .as_bytes()
            .get(opener_start + 2)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'));
    if legacy && leading_space {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if legacy
        && in_head
        && !leading_space
        && matches!(
            parser.wikidot_head_space_region(opener_start, false),
            WikidotHeadSpaceRegion::SingleSeparator
        )
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let head_has_elided_footnoteblock = legacy && in_head && {
        let head_start = parser.current().span.start;
        let mut end = head_start;
        for token in std::iter::once(parser.current()).chain(parser.remaining()) {
            if matches!(
                token.token,
                Token::RightBlock
                    | Token::RightLink
                    | Token::LineBreak
                    | Token::ParagraphBreak
                    | Token::InputEnd
            ) {
                end = token.span.start;
                break;
            }
        }
        parser.full_text().inner()[head_start..end]
            .to_ascii_lowercase()
            .contains("[[footnoteblock")
    };

    let (label, leftover, saw_title_unit) = if in_head {
        let checkpoint = parser.clone();
        let records_before = parser.degraded_title_record_count();
        match collect_tab_label(parser) {
            Ok((label, leftover, saw_unit)) => {
                if leftover.is_none() && parser.current().token == Token::LineBreak {
                    parser.step()?;
                }
                (Some(label), leftover, saw_unit)
            }
            Err(error) => {
                if legacy
                    && parser.degraded_title_record_count() > records_before
                    && let Some((literal, head_end)) = wikidot_degraded_tab_head_literal(
                        parser.full_text().inner(),
                        opener_start,
                    )
                {
                    parser.update(&checkpoint);
                    while parser.current().span.start < head_end {
                        parser.step()?;
                    }
                    return ok!(true; Elements::Single(Element::Text(literal)), Vec::new());
                }
                return Err(error);
            }
        }
    } else {
        (None, None, false)
    };

    let label = if legacy {
        match label {
            None => std::borrow::Cow::Borrowed("untitled"),
            Some(value) => {
                if saw_title_unit || head_has_elided_footnoteblock {
                    value
                } else {
                    std::borrow::Cow::Owned(wikidot_tab_head_label(&value))
                }
            }
        }
    } else {
        match label {
            Some(value) => value,
            None => {
                return Err(parser.make_err(ParseErrorKind::BlockMissingArguments));
            }
        }
    };

    let (mut elements, mut errors, _) =
        parser.get_body_elements(&BLOCK_TAB, true)?.into();
    if parser.settings().layout.legacy()
        && parser.wikidot_tab_quote_ejection()
        && parser.quote_body_cursor().is_some()
        && let Some(required_depth) = parser.native_blockquote_depth()
    {
        let mut probe = parser.clone();
        if matches!(
            probe.prepare_quote_body_line(),
            Ok(QuoteBodyLineStatus::Boundary)
        ) {
            scan_tab_close_after_soft_end(parser, required_depth, &mut errors)?;
        }
    }
    if let Some(leftover) = leftover {
        // Live keeps the residual closer of a malformed tab head in the tab
        // body's inline flow, so it joins the leading body paragraph.
        let mut joined = false;
        if legacy
            && let Some(Element::Container(paragraph)) = elements.first_mut()
            && paragraph.ctype() == ContainerType::Paragraph
        {
            paragraph
                .elements_mut()
                .insert(0, Element::Text(std::borrow::Cow::Borrowed(leftover)));
            joined = true;
        }
        if !joined {
            elements.insert(0, Element::Text(std::borrow::Cow::Borrowed(leftover)));
        }
    }
    if legacy && parser.wikidot_tab_quote_ejection() {
        trim_wikidot_quote_tab_body_break(&mut elements);
    }

    let element = Element::Partial(PartialElement::Tab(Tab { label, elements }));

    success_elements_with_paragraph_safety(false, element, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    fn render_wikidot(source: &str) -> (String, Vec<ParseError>) {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        (HtmlRender.render(&tree, &page_info, &settings).body, errors)
    }

    #[test]
    fn wikidot_named_tabview_and_untitled_tab_match_live_behavior() {
        let (html, errors) = render_wikidot(
            "NAME:\n[[tabview Foo]]\n[[tab Bar]].[[/tab]]\n[[/tabview]]\nNO LABEL:\n[[tabview]]\n[[tab]].[[/tab]]\n[[/tabview]]",
        );

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(
            html.starts_with("NAME:<br>\nFoo Foo\n<div id=\"wiki-tabview-"),
            "{html}",
        );
        assert!(html.contains("<em>Bar</em>"), "{html}");
        assert!(html.contains("NO LABEL:<br>\n"), "{html}");
        assert!(html.contains("<em>untitled</em>"), "{html}");
        assert!(!html.contains("<p>NAME:"), "{html}");
        assert!(!html.contains("<script"), "{html}");
    }

    #[test]
    fn underclosed_wikidot_tabviews_reuse_semantic_failure_cache() {
        use std::time::{Duration, Instant};

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = format!(
            "{}X{}",
            "[[tabview]]".repeat(128),
            "[[/tabview]]".repeat(64),
        );
        let tokenization = crate::tokenize(&source);
        let started = Instant::now();
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(
            started.elapsed() < Duration::from_secs(1),
            "underclosed Wikidot tabviews took {:?}",
            started.elapsed(),
        );
        assert_eq!(errors.len(), 512);
        assert!(html.contains("[[tabview]]"), "{html}");
    }

    #[test]
    fn tabviews_reuse_unclosed_collapsible_failures() {
        use std::time::{Duration, Instant};

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = format!(
            "{}{}{}",
            "[[tabview]]".repeat(64),
            "[[collapsible]]".repeat(64),
            "[[/tabview]]".repeat(128),
        );
        let tokenization = crate::tokenize(&source);
        let started = Instant::now();
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(
            started.elapsed() < Duration::from_secs(1),
            "tabview/collapsible recovery took {:?}",
            started.elapsed(),
        );
        assert!(!errors.is_empty());
        assert!(html.contains("[[tabview]]"), "{html}");
    }
}
