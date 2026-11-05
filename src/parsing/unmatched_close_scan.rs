/*
 * parsing/unmatched_close_scan.rs
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

//! Linear-time memoization for the repeated per-line scans that unmatched
//! block closers trigger (C3L9-1).
//!
//! Every unmatched `[[/name]]` on a line re-ran several whole-line scans:
//! the backward owner-opener scan in `consume.rs`, the forward `]]` / `]`
//! / region scans, the head-claim token walk and the link target/label
//! token walks in `link_single.rs`. Each scan is exact; the cache only
//! skips rescanning bytes and tokens whose outcome is already known, so
//! rendered output is unchanged. Every entry is self-validating: a query
//! that does not match the cached key recomputes with the original logic.

use super::{ExtractedToken, Parser};
use std::collections::BTreeMap;

#[cfg(test)]
use std::cell::Cell;

#[derive(Debug, Default)]
pub(super) struct UnmatchedCloseScanCache {
    line_bounds: Option<(usize, usize)>,
    double_bracket: Option<(usize, Option<usize>)>,
    single_bracket: Option<(usize, Option<usize>)>,
    owner_scans: [Option<OwnerLineScan>; 3],
    link_region: Option<(usize, usize)>,
    link_no_head: Option<(usize, usize, usize)>,
    div_line_close: Option<(usize, Option<usize>)>,
    next_line_break: Option<(usize, Option<usize>)>,
    target_failures: BTreeMap<(usize, bool, bool), ()>,
    label_failures: BTreeMap<(usize, bool, bool, bool), ()>,
    #[cfg(test)]
    visits: Cell<usize>,
}

/// Entry flags for the link-target failure memo. The collect walk never
/// mutates these, so a recorded failure replays exactly for a later walk
/// that reaches the same token with the same flags.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LinkTargetScanMemo {
    pub line_break_separator: bool,
    pub in_iftags_span: bool,
}

/// Entry flags for the link-label failure memo, used only when no body-end
/// boundary is in force (the label walk then never consults boundaries).
#[derive(Debug, Clone, Copy)]
pub(crate) struct LinkLabelScanMemo {
    pub in_table_cell: bool,
    pub in_triple_link: bool,
    pub in_iftags_span: bool,
}

#[derive(Debug, Default, Clone)]
pub(super) struct OwnerLineScan {
    line_start: usize,
    line_end: usize,
    open_len: usize,
    close_len: usize,
    opens: Vec<usize>,
    closes: Vec<usize>,
}

fn owner_slot(name: &str) -> Option<usize> {
    if name.eq_ignore_ascii_case("div") {
        Some(0)
    } else if name.eq_ignore_ascii_case("span") {
        Some(1)
    } else if name.eq_ignore_ascii_case("footnote") {
        Some(2)
    } else {
        None
    }
}

fn is_owner_follow(byte: u8) -> bool {
    matches!(byte, b']' | b' ' | b'\t' | b'_')
}

fn precompute_owner_line(
    source: &str,
    line_start: usize,
    line_end: usize,
    name: &str,
) -> OwnerLineScan {
    let bytes = source.as_bytes();
    let open = format!("[[{name}");
    let close = format!("[[/{name}");
    let mut opens = Vec::new();
    let mut closes = Vec::new();
    let mut offset = line_start;
    while offset < line_end {
        let rest = &bytes[offset..];
        if rest.len() >= open.len()
            && rest[..open.len()].eq_ignore_ascii_case(open.as_bytes())
            && bytes
                .get(offset + open.len())
                .is_some_and(|byte| is_owner_follow(*byte))
        {
            opens.push(offset);
        }
        if rest.len() >= close.len()
            && rest[..close.len()].eq_ignore_ascii_case(close.as_bytes())
            && bytes
                .get(offset + close.len())
                .is_some_and(|byte| is_owner_follow(*byte))
        {
            closes.push(offset);
        }
        offset += 1;
    }
    OwnerLineScan {
        line_start,
        line_end,
        open_len: open.len(),
        close_len: close.len(),
        opens,
        closes,
    }
}

fn owner_opener_before_scan(scan: &OwnerLineScan, close_start: usize) -> bool {
    let open_bound = close_start.saturating_sub(scan.open_len);
    let close_bound = close_start.saturating_sub(scan.close_len);
    let open_count = scan.opens.partition_point(|&start| start < open_bound);
    let close_count = scan.closes.partition_point(|&start| start < close_bound);
    let last_open = scan.opens[..open_count].last().copied();
    let last_close = scan.closes[..close_count].last().copied();
    last_open.is_some_and(|open| last_close.is_none_or(|close| open > close))
}

fn find_forward(
    source: &str,
    slot: &mut Option<(usize, Option<usize>)>,
    needle: &str,
    start: usize,
) -> (Option<usize>, usize) {
    if let Some((scanned_from, found)) = *slot
        && scanned_from <= start
        && found.is_none_or(|found| found >= start)
    {
        return (found, 0);
    }
    let found = source[start..].find(needle).map(|offset| start + offset);
    let scanned = found.map_or(source.len(), |found| found + needle.len()) - start;
    *slot = Some((start, found));
    (found, scanned)
}

/// ASCII-case-insensitive variant of [`find_forward`]: returns the first
/// occurrence of `needle` at or after `start`, reusing the slot when the
/// previous search already covers `start`.
fn find_forward_ignore_ascii_case(
    source: &str,
    slot: &mut Option<(usize, Option<usize>)>,
    needle: &[u8],
    start: usize,
) -> (Option<usize>, usize) {
    if let Some((scanned_from, found)) = *slot
        && scanned_from <= start
        && found.is_none_or(|found| found >= start)
    {
        return (found, 0);
    }
    let bytes = source.as_bytes();
    let found = (start..bytes.len().saturating_sub(needle.len() - 1)).find(|&offset| {
        bytes[offset..offset + needle.len()].eq_ignore_ascii_case(needle)
    });
    let scanned = found.map_or(bytes.len(), |found| found + needle.len()) - start;
    *slot = Some((start, found));
    (found, scanned)
}

/// First `\r` or `\n` at or after `start`, with the same reuse rules as
/// [`find_forward`].
fn find_forward_line_break(
    source: &str,
    slot: &mut Option<(usize, Option<usize>)>,
    start: usize,
) -> (Option<usize>, usize) {
    if let Some((scanned_from, found)) = *slot
        && scanned_from <= start
        && found.is_none_or(|found| found >= start)
    {
        return (found, 0);
    }
    let found = source.as_bytes()[start..]
        .iter()
        .position(|byte| matches!(byte, b'\r' | b'\n'))
        .map(|offset| start + offset);
    let scanned = found.map_or(source.len(), |found| found + 1) - start;
    *slot = Some((start, found));
    (found, scanned)
}

/// The region an unmatched `[[/name]]` closer may claim as its single-link
/// target runs to the end of the authored line: a complete block later on
/// the same line owns the closer instead of the link, as live shows for
/// `x[[/math]] [[eref eq1]]` (literal closer, then the equation reference).
fn end_block_link_region_end(source: &str, start: usize) -> usize {
    let text = source;
    let bytes = text.as_bytes();
    let mut region_end = bytes.len();
    let mut i = start + 2;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"[!--") {
            match text[i..].find("--]") {
                Some(offset) => i += offset + 3,
                None => {
                    i = bytes.len();
                }
            }
        } else if matches!(bytes[i], b'\n' | b'\r') {
            region_end = i;
            break;
        } else {
            i += 1;
        }
        if i >= bytes.len() {
            region_end = bytes.len();
        }
    }
    region_end
}

impl<'r, 't> Parser<'r, 't> {
    pub(crate) fn unmatched_line_bounds(
        &self,
        source: &str,
        pos: usize,
    ) -> (usize, usize) {
        let mut cache = self.block_end_scan_cache.borrow_mut();
        let scan = &mut cache.unmatched_close_scan;
        if let Some((line_start, line_end)) = scan.line_bounds
            && line_start <= pos
            && pos <= line_end
        {
            return (line_start, line_end);
        }
        let line_start = source[..pos]
            .rfind(['\r', '\n'])
            .map_or(0, |newline| newline + 1);
        let line_end = source[pos..]
            .find(['\r', '\n'])
            .map_or(source.len(), |offset| pos + offset);
        scan.line_bounds = Some((line_start, line_end));
        #[cfg(test)]
        scan.add_visits(line_end - line_start);
        (line_start, line_end)
    }

    pub(crate) fn cached_close_marker(
        &self,
        source: &str,
        start: usize,
    ) -> Option<usize> {
        let mut cache = self.block_end_scan_cache.borrow_mut();
        let scan = &mut cache.unmatched_close_scan;
        let (found, _scanned) =
            find_forward(source, &mut scan.double_bracket, "]]", start);
        #[cfg(test)]
        scan.add_visits(_scanned);
        found
    }

    pub(crate) fn cached_close_bracket(
        &self,
        source: &str,
        start: usize,
    ) -> Option<usize> {
        let mut cache = self.block_end_scan_cache.borrow_mut();
        let scan = &mut cache.unmatched_close_scan;
        let (found, _scanned) =
            find_forward(source, &mut scan.single_bracket, "]", start);
        #[cfg(test)]
        scan.add_visits(_scanned);
        found
    }

    pub(crate) fn cached_owner_opener_before(
        &self,
        source: &str,
        close_start: usize,
        name: &str,
    ) -> Option<bool> {
        let slot = owner_slot(name)?;
        let (line_start, line_end) = self.unmatched_line_bounds(source, close_start);
        let mut cache = self.block_end_scan_cache.borrow_mut();
        let scan = &mut cache.unmatched_close_scan;
        let cached = scan.owner_scans[slot].as_ref().filter(|cached| {
            cached.line_start == line_start && cached.line_end == line_end
        });
        if let Some(cached) = cached {
            return Some(owner_opener_before_scan(cached, close_start));
        }
        let computed = precompute_owner_line(source, line_start, line_end, name);
        #[cfg(test)]
        scan.add_visits(line_end - line_start);
        let outcome = owner_opener_before_scan(&computed, close_start);
        scan.owner_scans[slot] = Some(computed);
        Some(outcome)
    }

    /// Whether a `[[/div]]` closer (ASCII-case-insensitive) starts on the
    /// physical line that begins at `start`.
    ///
    /// `paragraph::wikidot_div_closes_on_current_line` asks this once per
    /// `[[div]]` opener, and a document with no newlines makes every ask
    /// scan to the end of the input: input consisting of repeated `[[div]]`
    /// openers grew quadratically. Both searches are monotone while the
    /// parser advances, so the shared slots keep the total work linear; the
    /// answer is identical to scanning the line slice directly.
    pub(crate) fn wikidot_div_closes_on_physical_line(&self, start: usize) -> bool {
        let source = self.full_text().inner();
        let mut cache = self.block_end_scan_cache.borrow_mut();
        let scan = &mut cache.unmatched_close_scan;
        let (close, _close_scanned) = find_forward_ignore_ascii_case(
            source,
            &mut scan.div_line_close,
            b"[[/div]]",
            start,
        );
        let (line_break, _break_scanned) =
            find_forward_line_break(source, &mut scan.next_line_break, start);
        #[cfg(test)]
        scan.add_visits(_close_scanned + _break_scanned);
        close.is_some_and(|close| line_break.is_none_or(|line_break| close < line_break))
    }

    pub(crate) fn cached_end_block_link_region(
        &self,
        source: &str,
        start: usize,
    ) -> usize {
        let from = start + 2;
        let mut cache = self.block_end_scan_cache.borrow_mut();
        let scan = &mut cache.unmatched_close_scan;
        if let Some((scan_from, region_end)) = scan.link_region
            && scan_from <= from
            && from <= region_end
            && !source[scan_from..from].contains("[!--")
        {
            scan.link_region = Some((from, region_end));
            return region_end;
        }
        let region_end = end_block_link_region_end(source, start);
        scan.link_region = Some((from, region_end));
        #[cfg(test)]
        scan.add_visits(region_end.saturating_sub(start));
        region_end
    }

    pub(crate) fn end_block_link_head_outcome(
        &self,
        remaining: &[ExtractedToken<'t>],
        region_end: usize,
    ) -> Option<bool> {
        let cache = self.block_end_scan_cache.borrow();
        let scan = &cache.unmatched_close_scan;
        let (end, start, cached_region) = scan.link_no_head?;
        let range = remaining.as_ptr_range();
        if range.end as usize == end
            && range.start as usize >= start
            && cached_region == region_end
        {
            Some(false)
        } else {
            None
        }
    }

    pub(crate) fn cache_end_block_link_head_outcome(
        &self,
        remaining: &[ExtractedToken<'t>],
        region_end: usize,
        saw_left_block: bool,
    ) {
        if saw_left_block {
            return;
        }
        let mut cache = self.block_end_scan_cache.borrow_mut();
        let scan = &mut cache.unmatched_close_scan;
        let range = remaining.as_ptr_range();
        scan.link_no_head = Some((range.end as usize, range.start as usize, region_end));
    }

    pub(crate) fn link_target_scan_failed(
        &self,
        token_addr: usize,
        line_break_separator: bool,
        in_iftags_span: bool,
    ) -> bool {
        self.block_end_scan_cache
            .borrow()
            .unmatched_close_scan
            .target_failures
            .contains_key(&(token_addr, line_break_separator, in_iftags_span))
    }

    pub(crate) fn cache_link_target_scan_failed(
        &self,
        token_addrs: &[usize],
        line_break_separator: bool,
        in_iftags_span: bool,
    ) {
        let mut cache = self.block_end_scan_cache.borrow_mut();
        let failures = &mut cache.unmatched_close_scan.target_failures;
        for &addr in token_addrs {
            failures.insert((addr, line_break_separator, in_iftags_span), ());
        }
    }

    pub(crate) fn link_label_scan_failed(
        &self,
        token_addr: usize,
        in_table_cell: bool,
        in_triple_link: bool,
        in_iftags_span: bool,
    ) -> bool {
        self.block_end_scan_cache
            .borrow()
            .unmatched_close_scan
            .label_failures
            .contains_key(&(token_addr, in_table_cell, in_triple_link, in_iftags_span))
    }

    pub(crate) fn cache_link_label_scan_failed(
        &self,
        token_addrs: &[usize],
        in_table_cell: bool,
        in_triple_link: bool,
        in_iftags_span: bool,
    ) {
        let mut cache = self.block_end_scan_cache.borrow_mut();
        let failures = &mut cache.unmatched_close_scan.label_failures;
        for &addr in token_addrs {
            failures.insert((addr, in_table_cell, in_triple_link, in_iftags_span), ());
        }
    }

    #[cfg(test)]
    pub(crate) fn add_unmatched_close_scan_visits(&self, visits: usize) {
        let cache = self.block_end_scan_cache.borrow();
        cache.unmatched_close_scan.add_visits(visits);
    }

    #[cfg(test)]
    pub(crate) fn unmatched_close_scan_visits(&self) -> usize {
        self.block_end_scan_cache
            .borrow()
            .unmatched_close_scan
            .visits
            .get()
    }
}

impl UnmatchedCloseScanCache {
    #[cfg(test)]
    fn add_visits(&self, visits: usize) {
        self.visits.set(self.visits.get() + visits);
    }
}

#[cfg(test)]
mod tests {
    use super::super::paragraph::{gather_paragraphs, no_close_condition};
    use super::super::rule::impls::RULE_PAGE;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::Parser;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};

    fn settings() -> WikitextSettings {
        WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot)
    }

    fn scan_visits(input: &str) -> usize {
        let page_info = PageInfo::dummy();
        let settings = settings();
        let tokenization = crate::tokenize(input);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        let _ = gather_paragraphs(&mut parser, RULE_PAGE, no_close_condition());
        parser.unmatched_close_scan_visits()
    }

    fn render_body(input: &str) -> String {
        let page_info = PageInfo::dummy();
        let settings = settings();
        let tokenization = crate::tokenize(input);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        HtmlRender.render(&tree, &page_info, &settings).body
    }

    #[test]
    fn unmatched_closer_scans_scale_linearly() {
        let shapes = [
            "[[/span]]",
            "[[/div]]",
            "[[/table]]",
            "[[/c3l9zzz]]",
            "[[/span]] ",
            "[[/table]] ",
            "[[/span]] x ",
            "[[/span",
        ];
        for shape in shapes {
            for &count in &[250usize, 500, 1000, 2000] {
                let input = shape.repeat(count);
                let visits = scan_visits(&input);
                assert!(
                    visits <= input.len() * 16,
                    "{shape:?} x {count}: {visits} visits for {} bytes",
                    input.len(),
                );
            }
        }
    }

    #[test]
    fn div_line_close_matches_uncached_line_scan() {
        let page_info = PageInfo::dummy();
        let settings = settings();
        let lines = [
            "[[div]][[/div]]",
            "[[div]] [[/DIV]]",
            "[[div]] no closer",
            "[[div]][[/div]]\n[[div]]",
            "[[div]]\r\n[[/div]]",
            "[[di]][[div]]",
            "[[divx]][[/divx]]",
            "[[div]][[/div]] [[/div]]",
            "\u{1f600}[[div]][[/div]]",
        ];
        for source in lines {
            let tokenization = crate::tokenize(source);
            let parser = Parser::new(&tokenization, &page_info, &settings);
            let mut positions: Vec<usize> = (0..=source.len()).collect();
            for &start in &positions {
                if !source.is_char_boundary(start) {
                    continue;
                }
                let line = source[start..]
                    .split_once(['\n', '\r'])
                    .map_or(&source[start..], |(line, _)| line);
                let cached = parser.wikidot_div_closes_on_physical_line(start);
                let uncached = line
                    .as_bytes()
                    .windows("[[/div]]".len())
                    .any(|window| window.eq_ignore_ascii_case(b"[[/div]]"));
                assert_eq!(cached, uncached, "{source:?} @{start}");
            }
            positions.reverse();
            for &start in &positions {
                if !source.is_char_boundary(start) {
                    continue;
                }
                let line = source[start..]
                    .split_once(['\n', '\r'])
                    .map_or(&source[start..], |(line, _)| line);
                let cached = parser.wikidot_div_closes_on_physical_line(start);
                let uncached = line
                    .as_bytes()
                    .windows("[[/div]]".len())
                    .any(|window| window.eq_ignore_ascii_case(b"[[/div]]"));
                assert_eq!(cached, uncached, "{source:?} @{start} reversed");
            }
        }
    }

    #[test]
    fn div_line_close_scans_scale_linearly() {
        // Kept below the default recursion limit: this helper parses on the
        // test thread, which has no deep-parse stack.
        for source in [
            "[[div]]".repeat(96),
            "[[/div]]".repeat(96),
            "[[div]]\n".repeat(96),
            format!("{}[[/div]]", "[[div]]".repeat(96)),
            "[[div]]. ".repeat(96),
        ] {
            let visits = scan_visits(&source);
            assert!(
                visits <= source.len() * 16,
                "{visits} visits for {} bytes",
                source.len(),
            );
        }
    }

    #[test]
    fn owner_scan_matches_uncached_prefix_scan() {
        let page_info = PageInfo::dummy();
        let settings = settings();
        let cases = [
            ("x [[/span]]", 2, "span"),
            ("[[/span]]", 0, "span"),
            ("[[span]] [[/span]]", 9, "span"),
            ("[[span]] [[/span]] [[/span]]", 19, "span"),
            ("[[span]][[/span]][[/span]]", 16, "span"),
            ("[[SPAN]] [[/span]]", 9, "span"),
            ("[[span_]] [[/span]]", 10, "span"),
            ("[[spanx]] [[/span]]", 10, "span"),
            ("xx[[span [[/span]]", 8, "span"),
            ("xx[[span [[/span]]", 9, "span"),
            ("[[div]] [[/div]]", 8, "div"),
            ("[[div]] [[/div]] [[/div]]", 17, "div"),
            ("[[footnote]] x [[/footnote]]", 15, "footnote"),
            ("a\n[[span]] [[/span]]", 11, "span"),
        ];
        for (line, close_start, name) in cases {
            let tokenization = crate::tokenize(line);
            let parser = Parser::new(&tokenization, &page_info, &settings);
            let cached = parser
                .cached_owner_opener_before(line, close_start, name)
                .expect("slot names must hit the cache");
            let uncached = super::super::consume::line_has_unmatched_wikidot_owner_opener(
                line,
                close_start,
                name,
            );
            assert_eq!(cached, uncached, "{line:?} @{close_start} {name}");
        }
        for (line, close_start, name, expected) in [
            ("[[/table]]", 0, "table", None),
            ("[[/table]]", 0, "SPAN", Some(false)),
            ("[[span]] [[/span]]", 9, "span", Some(true)),
            ("[[span]] [[/span]] [[/span]]", 19, "span", Some(false)),
        ] {
            let tokenization = crate::tokenize(line);
            let parser = Parser::new(&tokenization, &page_info, &settings);
            assert_eq!(
                parser.cached_owner_opener_before(line, close_start, name),
                expected,
                "{line:?} @{close_start} {name}"
            );
        }
    }

    #[test]
    fn region_scan_matches_uncached_scan() {
        let page_info = PageInfo::dummy();
        let settings = settings();
        let lines = [
            "[[/table]] rest",
            "[[/table]]x[!-- a b --]y z",
            "[[/table]]xyz",
            "[[/table]]x[!-- y",
            "[[/a\nb [[/c]] d",
            "[[/a]][[/b]] [[/c]]",
        ];
        for line in lines {
            let tokenization = crate::tokenize(line);
            let parser = Parser::new(&tokenization, &page_info, &settings);
            let mut starts: Vec<usize> =
                line.match_indices("[[/").map(|(index, _)| index).collect();
            for &start in &starts {
                assert_eq!(
                    parser.cached_end_block_link_region(line, start),
                    super::end_block_link_region_end(line, start),
                    "{line:?} @{start}"
                );
            }
            starts.reverse();
            for &start in &starts {
                assert_eq!(
                    parser.cached_end_block_link_region(line, start),
                    super::end_block_link_region_end(line, start),
                    "{line:?} @{start} reversed"
                );
            }
        }
    }

    #[test]
    fn forward_find_matches_str_find() {
        for needle in ["]]", "]"] {
            let source = "ab]]cd]ef[[/span]]gh]ij";
            let mut slot = None;
            let mut positions: Vec<usize> = (0..source.len()).collect();
            for &start in &positions {
                if !source.is_char_boundary(start) {
                    continue;
                }
                let (found, _) = super::find_forward(source, &mut slot, needle, start);
                let expected = source[start..].find(needle).map(|offset| start + offset);
                assert_eq!(found, expected, "{needle:?} @{start}");
            }
            positions.reverse();
            for &start in &positions {
                if !source.is_char_boundary(start) {
                    continue;
                }
                let (found, _) = super::find_forward(source, &mut slot, needle, start);
                let expected = source[start..].find(needle).map(|offset| start + offset);
                assert_eq!(found, expected, "{needle:?} @{start} reversed");
            }
        }
    }

    #[test]
    fn line_bounds_match_uncached_bounds() {
        let page_info = PageInfo::dummy();
        let settings = settings();
        let source = "ab\ncdef\r\ngh[[/span]]ij";
        let tokenization = crate::tokenize(source);
        let parser = Parser::new(&tokenization, &page_info, &settings);
        let mut positions: Vec<usize> = (0..=source.len()).collect();
        for &pos in &positions {
            if !source.is_char_boundary(pos) {
                continue;
            }
            let expected_start = source[..pos]
                .rfind(['\r', '\n'])
                .map_or(0, |newline| newline + 1);
            let expected_end = source[pos..]
                .find(['\r', '\n'])
                .map_or(source.len(), |offset| pos + offset);
            assert_eq!(
                parser.unmatched_line_bounds(source, pos),
                (expected_start, expected_end),
                "@{pos}"
            );
        }
        positions.reverse();
        for &pos in &positions {
            if !source.is_char_boundary(pos) {
                continue;
            }
            let expected_start = source[..pos]
                .rfind(['\r', '\n'])
                .map_or(0, |newline| newline + 1);
            let expected_end = source[pos..]
                .find(['\r', '\n'])
                .map_or(source.len(), |offset| pos + offset);
            assert_eq!(
                parser.unmatched_line_bounds(source, pos),
                (expected_start, expected_end),
                "@{pos} reversed"
            );
        }
    }

    #[test]
    fn unmatched_close_pins() {
        assert_eq!(render_body("[[/table]]"), "<p>[[/table]]</p>");
        assert_eq!(render_body("[[/c3l9zzz]]"), "<p>[[/c3l9zzz]]</p>");
        assert_eq!(
            render_body("[[/div]] label ]"),
            "<p>[<a href=\"/div]]\">label</a></p>"
        );
        assert_eq!(
            render_body("[[/footnote]] label ]"),
            "<p>[<a href=\"/footnote]]\">label</a></p>"
        );
        assert_eq!(
            render_body("[[div]] x [[/div]] label ]"),
            "<p>[[div]] x [[/div]] label ]</p>"
        );
    }
}
