use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use crate::data::PageInfo;
use crate::layout::Layout;
use crate::parsing::rule::impls::{
    get_block_rule_with_name_for_layout, head_seam_forms_block, head_word_never_resolves,
};
use crate::parsing::{ExtractedToken, Token};
use crate::preproc::LiteralRegionIndex;
use crate::wikidot_code::WikidotCodeCandidate;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum WikidotBlockKind {
    Div,
    Collapsible,
    Note,
    AlignCenter,
    AlignRight,
    AlignLeft,
    AlignJustify,
    TabView,
    Tab,
    Table,
    Row,
    Cell,
}

impl WikidotBlockKind {
    fn is_nesting(self) -> bool {
        matches!(
            self,
            WikidotBlockKind::Div
                | WikidotBlockKind::AlignCenter
                | WikidotBlockKind::AlignRight
                | WikidotBlockKind::AlignLeft
                | WikidotBlockKind::AlignJustify
        )
    }

    fn needs_own_line(self) -> bool {
        matches!(
            self,
            WikidotBlockKind::Div
                | WikidotBlockKind::Note
                | WikidotBlockKind::AlignCenter
                | WikidotBlockKind::AlignRight
                | WikidotBlockKind::AlignLeft
                | WikidotBlockKind::AlignJustify
        )
    }

    /// Blocks the parser refuses inside a footnote body's first content line
    /// (the legacy footnote zone), so the scan must not pair them either.
    fn footnote_zone_gated(self) -> bool {
        matches!(
            self,
            WikidotBlockKind::AlignCenter
                | WikidotBlockKind::AlignRight
                | WikidotBlockKind::AlignLeft
                | WikidotBlockKind::AlignJustify
                | WikidotBlockKind::TabView
                | WikidotBlockKind::Tab
                | WikidotBlockKind::Table
        )
    }

    pub(crate) fn emits_flat(self) -> bool {
        matches!(
            self,
            WikidotBlockKind::Div
                | WikidotBlockKind::Collapsible
                | WikidotBlockKind::Note
                | WikidotBlockKind::AlignCenter
                | WikidotBlockKind::AlignRight
                | WikidotBlockKind::AlignLeft
                | WikidotBlockKind::AlignJustify
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WikidotBlockPair {
    pub kind: WikidotBlockKind,
    pub scored: bool,
    pub head: usize,
    pub head_end: usize,
    pub head_byte: usize,
    pub closer: usize,
    pub closer_end: usize,
    pub closer_byte: usize,
    pub flat: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct WikidotBlockPairs {
    pairs: Vec<WikidotBlockPair>,
    by_head_byte: BTreeMap<usize, usize>,
    by_closer_byte: BTreeMap<usize, usize>,
    head_event_bytes: BTreeSet<usize>,
    closer_event_bytes: BTreeSet<(WikidotBlockKind, usize)>,
    foster_heads: BTreeSet<usize>,
    greedy_ranges: Vec<(usize, usize)>,
    heading_ranges: Vec<(usize, usize)>,
    closer_end_bytes: BTreeMap<usize, usize>,
}

impl WikidotBlockPairs {
    /// The byte range of the heading line containing the byte, if that byte is
    /// on one. The range excludes the terminating break.
    pub(crate) fn heading_line_range(&self, byte: usize) -> Option<(usize, usize)> {
        let index = self
            .heading_ranges
            .partition_point(|(start, _)| *start <= byte);
        if index == 0 {
            return None;
        }
        let (start, end) = self.heading_ranges[index - 1];
        (byte < end).then_some((start, end))
    }

    pub(crate) fn closer_end_byte(&self, closer_byte: usize) -> Option<usize> {
        self.closer_end_bytes.get(&closer_byte).copied()
    }

    /// Pairs whose head sits in the given byte range.
    pub(crate) fn heads_between(
        &self,
        from: usize,
        to: usize,
    ) -> impl Iterator<Item = &WikidotBlockPair> {
        self.by_head_byte
            .range(from..to)
            .map(|(_, index)| &self.pairs[*index])
    }
    #[cfg(test)]
    pub(crate) fn pairs(&self) -> &[WikidotBlockPair] {
        &self.pairs
    }

    pub(crate) fn pair_at_head_byte(
        &self,
        head_byte: usize,
    ) -> Option<&WikidotBlockPair> {
        self.by_head_byte
            .get(&head_byte)
            .map(|index| &self.pairs[*index])
    }

    pub(crate) fn pair_at_closer_byte(
        &self,
        closer_byte: usize,
    ) -> Option<&WikidotBlockPair> {
        self.by_closer_byte
            .get(&closer_byte)
            .map(|index| &self.pairs[*index])
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    pub(crate) fn closers_between(
        &self,
        from_byte: usize,
        to_byte: usize,
    ) -> impl Iterator<Item = (usize, &WikidotBlockPair)> {
        self.by_closer_byte
            .range(from_byte..to_byte)
            .map(|(byte, index)| (*byte, &self.pairs[*index]))
    }

    pub(crate) fn is_foster_head(&self, head_byte: usize) -> bool {
        self.foster_heads.contains(&head_byte)
    }

    pub(crate) fn has_greedy_tail_within(&self, from: usize, to: usize) -> bool {
        self.greedy_ranges
            .iter()
            .any(|&(start, _)| from <= start && start < to)
    }

    /// The greedy iframe URL tail that covers the byte, if any.
    pub(crate) fn greedy_range_covering(&self, byte: usize) -> Option<(usize, usize)> {
        let index = self
            .greedy_ranges
            .partition_point(|&(start, _)| start <= byte);
        if index == 0 {
            return None;
        }
        let range = self.greedy_ranges[index - 1];
        (byte < range.1).then_some(range)
    }

    pub(crate) fn shows_stolen_head(
        &self,
        kind: WikidotBlockKind,
        head_byte: usize,
    ) -> bool {
        if !self.head_event_bytes.contains(&head_byte) {
            return false;
        }
        self.closer_event_bytes
            .range((kind, head_byte.saturating_add(1))..)
            .next()
            .is_some_and(|(found, _)| *found == kind)
    }

    pub(crate) fn has_closer_byte_in_range(&self, start: usize, end: usize) -> bool {
        self.by_closer_byte.range(start..end).next().is_some()
    }
}

/// L85: outcome of the explicit-list cross scan (one outer `[[ul]]`/`[[ol]]`
/// region). Records div/collapsible closers paired through list structure,
/// redundant list closers live drops, and `[[li]]` openers needing an
/// implicit nested list. Empty (all collections vacant) for div-free regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WikidotLiCrossKind {
    Div,
    Collapsible,
}

impl WikidotLiCrossKind {
    pub(crate) fn boundary_name(self) -> &'static str {
        match self {
            WikidotLiCrossKind::Div => "div",
            WikidotLiCrossKind::Collapsible => "collapsible",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct WikidotLiCrossScan {
    /// div/collapsible closer byte -> (paired kind, crossed through list
    /// structure). Breached-outer pairs are always crossed.
    pub(crate) divcoll_pairs: BTreeMap<usize, (WikidotLiCrossKind, bool)>,
    /// `li`/`ul`/`ol` closer bytes live drops as redundant.
    pub(crate) redundant: BTreeSet<usize>,
    /// `[[li]]` open bytes needing an implicit nested list.
    pub(crate) implicit_li: BTreeSet<usize>,
    /// Scanned region (after the outer head through its closer or input end).
    pub(crate) region_start: usize,
    pub(crate) region_end: usize,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct WikidotLineRuns {
    line_starts: Vec<usize>,
    quote_run: Vec<Option<u32>>,
    list_run: Vec<Option<u32>>,
    table_row: Vec<bool>,
}

pub(crate) fn compute_wikidot_line_runs(source: &str) -> WikidotLineRuns {
    let mut line_starts = Vec::new();
    let mut quote_run = Vec::new();
    let mut list_run = Vec::new();
    let mut table_row = Vec::new();
    let mut quote_id = 0u32;
    let mut quote_open = false;
    let mut list_id = 0u32;
    let mut list_open = false;
    let mut offset = 0usize;
    for fragment in source.split('\n') {
        let line = fragment.strip_suffix('\r').unwrap_or(fragment);
        line_starts.push(offset);
        offset += fragment.len() + 1;
        if line.starts_with('>') {
            if !quote_open {
                quote_open = true;
            }
            quote_run.push(Some(quote_id));
        } else {
            if quote_open {
                quote_open = false;
                quote_id = quote_id.saturating_add(1);
            }
            quote_run.push(None);
        }
        if is_native_list_item_line(line) {
            if !list_open {
                list_open = true;
            }
            list_run.push(Some(list_id));
        } else {
            if list_open {
                list_open = false;
                list_id = list_id.saturating_add(1);
            }
            list_run.push(None);
        }
        table_row.push(is_simple_table_row_line(line));
    }
    WikidotLineRuns {
        line_starts,
        quote_run,
        list_run,
        table_row,
    }
}

fn is_native_list_item_line(line: &str) -> bool {
    let stripped = line.trim_start_matches([' ', '\t']);
    let Some(marker) = stripped.as_bytes().first() else {
        return false;
    };
    if *marker != b'*' && *marker != b'#' {
        return false;
    }
    stripped[1..].starts_with([' ', '\t'])
}

fn is_simple_table_row_line(line: &str) -> bool {
    if line.starts_with("||") {
        return true;
    }
    let mut rest = line;
    let mut stripped_quote = false;
    while let Some(tail) = rest.strip_prefix('>') {
        rest = tail;
        stripped_quote = true;
    }
    if !stripped_quote {
        return false;
    }
    rest = rest.trim_start_matches([' ', '\t']);
    rest.starts_with("||")
}

impl WikidotLineRuns {
    pub(crate) fn line_of(&self, byte: usize) -> usize {
        self.line_starts
            .partition_point(|start| *start <= byte)
            .saturating_sub(1)
    }

    pub(crate) fn line_start(&self, line: usize) -> usize {
        self.line_starts.get(line).copied().unwrap_or(0)
    }

    pub(crate) fn on_list_item_line(&self, byte: usize) -> bool {
        let line = self.line_of(byte);
        self.list_run.get(line).copied().flatten().is_some()
    }

    pub(crate) fn quote_run_id(&self, byte: usize) -> Option<u32> {
        let line = self.line_of(byte);
        self.quote_run.get(line).copied().flatten()
    }

    pub(crate) fn on_table_row_line(&self, byte: usize) -> bool {
        let line = self.line_of(byte);
        self.table_row.get(line).copied().unwrap_or(false)
    }
}

#[cfg(test)]
thread_local! {
    static RECORDED_BLOCKS: std::cell::RefCell<Vec<(String, usize, usize)>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static RECORDER_ENABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn record_formed_block(name: &str, opener_start: usize, end: usize) {
    if RECORDER_ENABLED.with(|enabled| enabled.get()) {
        RECORDED_BLOCKS.with(|recorded| {
            recorded
                .borrow_mut()
                .push((name.to_string(), opener_start, end));
        });
    }
}

#[cfg(test)]
pub(crate) fn recorder_enable() {
    RECORDED_BLOCKS.with(|recorded| recorded.borrow_mut().clear());
    RECORDER_ENABLED.with(|enabled| enabled.set(true));
}

#[cfg(test)]
pub(crate) fn recorder_take() -> Vec<(String, usize, usize)> {
    RECORDER_ENABLED.with(|enabled| enabled.set(false));
    RECORDED_BLOCKS.with(|recorded| std::mem::take(&mut *recorded.borrow_mut()))
}

pub(crate) fn compute_wikidot_block_pairs<'t>(
    tokens: &[ExtractedToken<'t>],
    source: &'t str,
    page_info: &PageInfo<'t>,
) -> WikidotBlockPairs {
    let mut scanner = Scanner::new(tokens, source, page_info);
    scanner.bib_ranges = scanner.bibliography_ranges();
    scanner.scan_segment(0, tokens.len(), 0);
    scanner.finish()
}

struct Event {
    kind: WikidotBlockKind,
    scored: bool,
    head: usize,
    head_end: usize,
    closer: usize,
    closer_end: usize,
    is_close: bool,
}

fn line_prefix_before(source: &str, byte: usize) -> &str {
    let line_start = source[..byte.min(source.len())]
        .rfind('\n')
        .map_or(0, |offset| offset + 1);
    &source[line_start..byte.min(source.len())]
}

fn pipe_rowstart_head_prefix(source: &str, head_byte: usize) -> bool {
    let prefix = line_prefix_before(source, head_byte);
    prefix
        .strip_prefix("||")
        .is_some_and(|rest| rest.trim_matches([' ', '\t']).is_empty())
}

fn own_line_marker_prefix(source: &str, marker_byte: usize) -> bool {
    line_prefix_before(source, marker_byte)
        .trim_matches([' ', '\t'])
        .is_empty()
}

fn non_opaque_literal_start(text: &str) -> bool {
    if text.starts_with('<') {
        return true;
    }
    let Some(rest) = text.strip_prefix("[[") else {
        return false;
    };
    let name_end = rest
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(rest.len());
    let name = &rest[..name_end];
    name.eq_ignore_ascii_case("raw") || name.eq_ignore_ascii_case("html")
}

fn segment_head_name(
    tokens: &[ExtractedToken<'_>],
    start: usize,
    break_at_right_link: bool,
) -> Option<(Range<usize>, usize)> {
    let mut index = start + 1;
    while tokens.get(index)?.token == Token::Whitespace {
        index += 1;
    }
    let name_start = tokens.get(index)?.span.start;
    let mut name_end = name_start;
    loop {
        let token = tokens.get(index)?;
        match token.token {
            Token::Whitespace
            | Token::LineBreak
            | Token::ParagraphBreak
            | Token::RightBlock
            | Token::InputEnd => break,
            Token::RightLink if break_at_right_link => break,
            _ => {
                name_end = token.span.end;
                index += 1;
            }
        }
    }
    if name_end == name_start {
        return None;
    }
    Some((name_start..name_end, index))
}

struct SegmentHit {
    include_body: bool,
    head_end: usize,
    end: usize,
    body: Range<usize>,
    range_end: usize,
}

struct Scanner<'t> {
    tokens: &'t [ExtractedToken<'t>],
    source: &'t str,
    tags: Vec<Cow<'t, str>>,
    zones: Vec<(usize, usize)>,
    code_candidates: BTreeMap<usize, WikidotCodeCandidate>,
    line_of: Vec<usize>,
    line_first: Vec<usize>,
    heading_line: Vec<bool>,
    heading_line_end_bytes: Vec<usize>,
    heading_ranges: Vec<(usize, usize)>,
    head_dropped: Vec<bool>,
    module_closers: Vec<usize>,
    iftags_closers: Vec<usize>,
    module_skip_memo: std::cell::RefCell<std::collections::HashMap<usize, Option<usize>>>,
    table_measure_memo:
        std::cell::RefCell<std::collections::HashMap<(usize, usize), Option<Measure>>>,
    tabview_measure_memo:
        std::cell::RefCell<std::collections::HashMap<(usize, usize), Option<Measure>>>,
    measure_guard: std::cell::Cell<usize>,
    measure_guard_trips: std::cell::Cell<usize>,
    tabview_banned_lines: Vec<bool>,
    footnote_zones: Vec<(usize, usize)>,
    bib_ranges: Vec<(usize, usize)>,
    pairs: Vec<WikidotBlockPair>,
    head_event_bytes: BTreeSet<usize>,
    closer_event_bytes: BTreeSet<(WikidotBlockKind, usize)>,
    greedy_ranges: Vec<(usize, usize)>,
}

impl<'t> Scanner<'t> {
    fn new(
        tokens: &'t [ExtractedToken<'t>],
        source: &'t str,
        page_info: &PageInfo<'t>,
    ) -> Self {
        let literal = LiteralRegionIndex::new(source);
        let mut zones: Vec<(usize, usize)> = Vec::new();
        for range in literal.ranges() {
            if non_opaque_literal_start(&source[range.start..]) {
                continue;
            }
            let start = tokens.partition_point(|token| token.span.end <= range.start);
            let end = tokens.partition_point(|token| token.span.start < range.end);
            if start < end {
                if let Some(last) = zones.last_mut() {
                    if start <= last.1 {
                        last.1 = last.1.max(end);
                        continue;
                    }
                }
                zones.push((start, end));
            }
        }

        let mut line_of = vec![0_usize; tokens.len()];
        let mut line_first: Vec<usize> = Vec::new();
        let mut heading_line: Vec<bool> = Vec::new();
        let mut heading_line_end_bytes: Vec<usize> = Vec::new();
        let mut heading_ranges: Vec<(usize, usize)> = Vec::new();
        let mut line = 0_usize;
        let mut index = 0_usize;
        while index < tokens.len() {
            line_first.push(index);
            let mut first_content = None;
            let mut end_byte = source.len();
            let mut cursor = index;
            loop {
                if cursor >= tokens.len() {
                    break;
                }
                let token = &tokens[cursor];
                line_of[cursor] = line;
                if first_content.is_none()
                    && !matches!(
                        token.token,
                        Token::Quote | Token::Whitespace | Token::InputStart
                    )
                {
                    first_content = Some(token.token);
                }
                if matches!(token.token, Token::LineBreak | Token::ParagraphBreak) {
                    end_byte = token.span.start;
                    cursor += 1;
                    break;
                }
                cursor += 1;
            }
            let is_heading = first_content == Some(Token::Heading);
            heading_line.push(is_heading);
            if is_heading {
                heading_line_end_bytes.push(end_byte);
                heading_ranges.push((tokens[index].span.start, end_byte));
            }
            index = cursor;
            line += 1;
        }

        let mut head_dropped = vec![false; tokens.len()];
        for line_idx in 0..line_first.len() {
            let start = line_first[line_idx];
            let end = line_first
                .get(line_idx + 1)
                .copied()
                .unwrap_or(tokens.len());
            let is_column = |token: Token| {
                matches!(
                    token,
                    Token::TableColumn
                        | Token::TableColumnRight
                        | Token::TableColumnCenter
                        | Token::TableColumnTitle
                )
            };
            let mut first_content: Option<usize> = None;
            let mut last_content: Option<usize> = None;
            let mut last_column_end: Option<usize> = None;
            for t in start..end {
                let token = &tokens[t];
                if is_column(token.token) {
                    last_column_end = Some(token.span.end);
                }
                if !matches!(
                    token.token,
                    Token::Whitespace
                        | Token::LineBreak
                        | Token::ParagraphBreak
                        | Token::InputEnd
                ) {
                    last_content = Some(t);
                    if first_content.is_none()
                        && !matches!(token.token, Token::Quote | Token::InputStart)
                    {
                        first_content = Some(t);
                    }
                }
            }
            let (Some(fc), Some(lc), Some(frag_start)) =
                (first_content, last_content, last_column_end)
            else {
                continue;
            };
            if !is_column(tokens[fc].token) || is_column(tokens[lc].token) {
                continue;
            }
            for t in start..end {
                if tokens[t].span.start >= frag_start {
                    // L8.5: fragment-row div/collapsible heads stay visible
                    // so the pairing matrix can bind them (render-interior
                    // flat); every other head on the fragment keeps the
                    // legacy drop. Downstream head_valid still enforces
                    // div-EOL-only.
                    let keep = matches!(
                        tokens[t].token,
                        Token::LeftBlock | Token::LeftBlockStar
                    ) && segment_head_name(tokens, t, false).is_some_and(
                        |(name, _)| {
                            let name = &source[name];
                            let base = name.strip_suffix('_').unwrap_or(name);
                            base.eq_ignore_ascii_case("div")
                                || base.eq_ignore_ascii_case("collapsible")
                        },
                    );
                    if !keep {
                        head_dropped[t] = true;
                    }
                }
            }
        }

        let line_count = heading_line.len();

        let mut module_closers = Vec::new();
        let mut iftags_closers = Vec::new();
        for index in 0..tokens.len() {
            if tokens[index].token == Token::LeftBlockEnd
                && let Some((name, _)) = segment_head_name(tokens, index, true)
            {
                if source[name.clone()].eq_ignore_ascii_case("module") {
                    module_closers.push(index);
                } else if source[name].eq_ignore_ascii_case("iftags") {
                    iftags_closers.push(index);
                }
            }
        }

        let mut scanner = Scanner {
            tokens,
            source,
            tags: page_info.tags.clone(),
            zones,
            code_candidates: crate::wikidot_code::candidates(source),
            line_of,
            line_first,
            heading_line,
            heading_line_end_bytes,
            heading_ranges,
            head_dropped,
            module_closers,
            iftags_closers,
            module_skip_memo: std::cell::RefCell::new(std::collections::HashMap::new()),
            table_measure_memo: std::cell::RefCell::new(std::collections::HashMap::new()),
            tabview_measure_memo: std::cell::RefCell::new(
                std::collections::HashMap::new(),
            ),
            measure_guard: std::cell::Cell::new(0),
            measure_guard_trips: std::cell::Cell::new(0),
            tabview_banned_lines: vec![false; line_count],
            footnote_zones: Vec::new(),
            bib_ranges: Vec::new(),
            pairs: Vec::new(),
            head_event_bytes: BTreeSet::new(),
            closer_event_bytes: BTreeSet::new(),
            greedy_ranges: Vec::new(),
        };
        scanner.footnote_zones = scanner.compute_footnote_zones();
        scanner
    }

    /// Byte ranges of each footnote body's first content line, mirroring
    /// `wikidot_footnote_zone_end()`: line-start blocks cannot own a line
    /// there, matching live.
    fn compute_footnote_zones(&self) -> Vec<(usize, usize)> {
        let mut zones = Vec::new();
        let mut index = 0usize;
        while index < self.tokens.len() {
            if matches!(
                self.tokens[index].token,
                Token::LeftBlock | Token::LeftBlockStar
            ) && let Some((name, name_end)) =
                segment_head_name(self.tokens, index, false)
                && self.source[name].eq_ignore_ascii_case("footnote")
            {
                let mut cursor = name_end;
                while cursor < self.tokens.len()
                    && !matches!(
                        self.tokens[cursor].token,
                        Token::RightBlock | Token::LineBreak | Token::ParagraphBreak
                    )
                {
                    cursor += 1;
                }
                if cursor < self.tokens.len()
                    && self.tokens[cursor].token == Token::RightBlock
                {
                    let zone_end = self.footnote_zone_end_from(cursor + 1);
                    let closer_byte = self
                        .footnote_closer_byte(cursor + 1)
                        .unwrap_or(self.source.len());
                    zones
                        .push((self.tokens[index].span.start, zone_end.min(closer_byte)));
                    index = cursor + 1;
                    continue;
                }
            }
            index += 1;
        }
        zones.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(zones.len());
        for (start, end) in zones {
            if let Some(previous) = merged.last_mut()
                && start <= previous.1
            {
                previous.1 = previous.1.max(end);
                continue;
            }
            merged.push((start, end));
        }
        merged
    }

    fn footnote_closer_byte(&self, mut index: usize) -> Option<usize> {
        while let Some(token) = self.tokens.get(index) {
            match token.token {
                Token::InputEnd => return None,
                Token::LeftComment => {
                    index = self.skip_comment(index)?;
                    continue;
                }
                Token::LeftBlock | Token::LeftBlockStar => {
                    if let Some((name, name_end)) =
                        segment_head_name(self.tokens, index, false)
                        && self.source[name].eq_ignore_ascii_case("footnote")
                    {
                        // Nested footnote heads cannot close this body.
                        index = name_end + 1;
                        continue;
                    }
                }
                Token::LeftBlockEnd => {
                    if let Some((name, _)) = segment_head_name(self.tokens, index, true)
                        && self.source[name].eq_ignore_ascii_case("footnote")
                    {
                        return Some(token.span.start);
                    }
                }
                _ => {}
            }
            index += 1;
        }
        None
    }

    fn footnote_zone_end_from(&self, mut index: usize) -> usize {
        let mut line_has_content = false;
        while let Some(token) = self.tokens.get(index) {
            match token.token {
                Token::InputEnd => return usize::MAX,
                Token::LeftComment => match self.skip_comment(index) {
                    Some(next) => {
                        index = next;
                        continue;
                    }
                    None => return usize::MAX,
                },
                Token::LineBreak | Token::ParagraphBreak => {
                    if line_has_content {
                        return token.span.end;
                    }
                }
                Token::Whitespace | Token::DiscardedControl | Token::InputStart => {}
                Token::LeftBlockEnd => {
                    if let Some((name, _)) = segment_head_name(self.tokens, index, true)
                        && self.source[name].eq_ignore_ascii_case("footnote")
                    {
                        return usize::MAX;
                    }
                    line_has_content = true;
                }
                _ => {
                    let slice = token.slice;
                    if slice.contains('\n') {
                        let mut offset = 0usize;
                        let mut segments = slice.split('\n').peekable();
                        while let Some(line) = segments.next() {
                            let line_content =
                                line.chars().any(|character| !character.is_whitespace());
                            if segments.peek().is_some() {
                                if line_has_content || line_content {
                                    return token.span.start + offset + line.len() + 1;
                                }
                                offset += line.len() + 1;
                            } else if line_content {
                                line_has_content = true;
                            }
                        }
                    } else {
                        line_has_content = true;
                    }
                }
            }
            index += 1;
        }
        usize::MAX
    }

    fn in_footnote_first_line(&self, byte: usize) -> bool {
        let index = self
            .footnote_zones
            .partition_point(|(start, _)| *start < byte);
        index > 0 && byte < self.footnote_zones[index - 1].1
    }

    /// Whether the parser collects this line as single-line content (a native
    /// list item or heading line), where a legacy table head cannot form.
    fn on_single_line_content_line(&self, index: usize) -> bool {
        if self.on_heading_line(index) {
            return true;
        }
        let byte = self.tokens[index].span.start;
        let line_start = self.source[..byte]
            .rfind('\n')
            .map_or(0, |offset| offset + 1);
        let line = self.source[line_start..]
            .split('\n')
            .next()
            .unwrap_or_default();
        let mut rest = line;
        loop {
            let trimmed = rest.trim_start_matches([' ', '\t']);
            let Some(after) = trimmed.strip_prefix('>') else {
                rest = trimmed;
                break;
            };
            rest = after;
        }
        let mut bytes = rest.bytes();
        matches!(bytes.next(), Some(b'*' | b'#'))
            && matches!(bytes.next(), Some(b' ' | b'\t'))
    }

    fn zone_end(&self, index: usize) -> Option<usize> {
        let zone = self.zones.partition_point(|zone| zone.0 <= index);
        if zone == 0 {
            return None;
        }
        let (_, end) = self.zones[zone - 1];
        (index < end).then_some(end)
    }

    fn token_at_or_after(&self, byte: usize) -> usize {
        self.tokens.partition_point(|token| token.span.start < byte)
    }

    fn head_byte(&self, head: usize) -> usize {
        self.tokens[head].span.start
    }

    fn on_heading_line(&self, index: usize) -> bool {
        self.heading_line[self.line_of[index]]
    }

    fn starts_line(&self, index: usize) -> bool {
        let first = self.line_first[self.line_of[index]];
        self.tokens[first..index].iter().all(|token| {
            matches!(
                token.token,
                Token::Quote | Token::Whitespace | Token::InputStart
            )
        })
    }

    fn followed_by_break(&self, mut index: usize) -> bool {
        while let Some(token) = self.tokens.get(index) {
            match token.token {
                Token::Whitespace
                    if token.slice.bytes().all(|b| matches!(b, b' ' | b'\t')) =>
                {
                    index += 1;
                }
                Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                    return true;
                }
                _ => return false,
            }
        }
        true
    }

    fn skip_comment(&self, index: usize) -> Option<usize> {
        let tokens = self.tokens;
        let mut index = index + 1;
        let mut hyphens = 0_usize;
        while let Some(token) = tokens.get(index) {
            if token.token == Token::InputEnd {
                return None;
            }
            let trailing_hyphens = token
                .slice
                .bytes()
                .rev()
                .take_while(|byte| *byte == b'-')
                .count();
            if trailing_hyphens >= 2
                && trailing_hyphens < token.slice.len()
                && tokens.get(index + 1).is_some_and(|next| {
                    next.token == Token::RightBracket && next.slice == "]"
                })
            {
                return Some(index + 2);
            }
            if !token.slice.is_empty() && token.slice.bytes().all(|byte| byte == b'-') {
                hyphens += token.slice.len();
                index += 1;
                continue;
            }
            if hyphens >= 2 && token.token == Token::RightBracket && token.slice == "]" {
                return Some(index + 1);
            }
            hyphens = 0;
            index += 1;
        }
        None
    }

    fn segment_at(&self, start: usize) -> Option<SegmentHit> {
        let tokens = self.tokens;
        if tokens[start].token != Token::LeftBlock {
            return None;
        }
        let (name, after_name) = segment_head_name(tokens, start, false)?;
        if !self.source[name].eq_ignore_ascii_case("iftags") {
            return None;
        }
        let spec_start = tokens.get(after_name)?.span.start;
        let mut index = after_name;
        let (spec_end, head_end_byte) = loop {
            let token = tokens.get(index)?;
            match token.token {
                Token::RightBlock => {
                    break (token.span.start, token.span.end);
                }
                Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                    return None;
                }
                _ => index += 1,
            }
        };
        let spec = self.source[spec_start..spec_end].trim();
        let conditions = if spec == "-" || spec.is_empty() {
            Vec::new()
        } else {
            crate::parsing::ElementCondition::parse(spec)
        };
        let include_body = if conditions.is_empty() {
            !self.tags.is_empty()
        } else {
            crate::parsing::ElementCondition::check(&conditions, &self.tags)
        };
        let head_end_index = index + 1;
        let next_closer = self
            .iftags_closers
            .partition_point(|closer| *closer < head_end_index);
        if self.iftags_closers.get(next_closer).is_none() {
            return None;
        }
        let body_start = tokens
            .get(head_end_index)
            .map_or(head_end_byte, |token| token.span.start);
        let mut depth = 1usize;
        index = head_end_index;
        let (body_end, closer_end) = loop {
            let token = tokens.get(index)?;
            match token.token {
                Token::InputEnd => return None,
                Token::LeftBlock => {
                    if let Some((inner, _)) = segment_head_name(tokens, index, true) {
                        if self.source[inner].eq_ignore_ascii_case("iftags") {
                            depth += 1;
                        }
                    }
                }
                Token::LeftBlockEnd => {
                    if let Some((inner, _)) = segment_head_name(tokens, index, true) {
                        if self.source[inner].eq_ignore_ascii_case("iftags") {
                            depth -= 1;
                            if depth == 0 {
                                let closer_start = token.span.start;
                                let mut end_index = index;
                                let closer_end = loop {
                                    let end_token = tokens.get(end_index)?;
                                    match end_token.token {
                                        Token::RightBlock => {
                                            break end_token.span.end;
                                        }
                                        Token::RightLink => {
                                            break end_token.span.start + 2;
                                        }
                                        Token::LineBreak
                                        | Token::ParagraphBreak
                                        | Token::InputEnd => {
                                            return None;
                                        }
                                        _ => end_index += 1,
                                    }
                                };
                                break (closer_start, closer_end);
                            }
                        }
                    }
                }
                _ => {}
            }
            index += 1;
        };
        Some(SegmentHit {
            include_body,
            head_end: head_end_index,
            end: self.token_at_or_after(closer_end),
            body: body_start..body_end,
            range_end: closer_end,
        })
    }
}

struct HeadRead {
    name: String,
    flag_star: bool,
    flag_score: bool,
    borrowed: bool,
    segment: bool,
    head_end: usize,
    name_end: usize,
    head_segments: Vec<(usize, usize, bool)>,
    name_fused: bool,
    has_args: bool,
}

impl<'t> Scanner<'t> {
    fn read_head(&self, start: usize, end: bool) -> Option<HeadRead> {
        let first = self.tokens.get(start)?;
        let flag_star = match (end, first.token) {
            (false, Token::LeftBlock) => false,
            (false, Token::LeftBlockStar) => true,
            (true, Token::LeftBlockEnd) => false,
            _ => return None,
        };
        let mut j = start + 1;
        let mut text = String::new();
        let mut edited = false;
        let mut segment_consumed = false;
        let mut head_segments = Vec::new();
        let mut name_fused = false;
        let mut fused_completion = None;
        let (name_end, terminator) = loop {
            let token = self.tokens.get(j)?;
            match token.token {
                Token::Whitespace
                | Token::LineBreak
                | Token::ParagraphBreak
                | Token::RightBlock => break (j, token.token),
                Token::InputEnd => return None,
                Token::LeftComment => {
                    j = self.skip_comment(j)?;
                    edited = true;
                }
                Token::LeftBlock => {
                    if let Some(segment) = self.segment_at(j) {
                        segment_consumed = true;
                        edited = true;
                        if segment.include_body {
                            text.push_str(&self.source[segment.body.clone()]);
                        }
                        head_segments.push((
                            self.token_at_or_after(segment.body.start),
                            self.token_at_or_after(segment.body.end),
                            segment.include_body,
                        ));
                        let straddle = self.tokens.get(segment.end.wrapping_sub(1));
                        let completes =
                            straddle.is_some_and(|token| {
                                token.token == Token::RightLink
                                    && token.span.start < segment.range_end
                                    && segment.range_end < token.span.end
                            }) && self.tokens.get(segment.end).is_some_and(|token| {
                                token.token == Token::RightBracket
                                    && token.span.start == segment.range_end + 1
                            });
                        if completes {
                            fused_completion = Some(segment.end + 1);
                            break (segment.end - 1, Token::RightBlock);
                        }
                        j = segment.end;
                    } else {
                        text.push_str(token.slice);
                        j += 1;
                    }
                }
                Token::RightLink => {
                    if end || segment_consumed || edited {
                        name_fused = true;
                        break (j, token.token);
                    }
                    text.push_str(token.slice);
                    j += 1;
                }
                _ => {
                    text.push_str(token.slice);
                    j += 1;
                }
            }
            if j - start > 8192 {
                return None;
            }
        };
        let trimmed = text.trim_matches([' ', '\t', '\r', '\n']);
        if trimmed.is_empty() {
            return None;
        }
        if edited && (trimmed.contains(char::is_whitespace) || trimmed.contains("]]")) {
            return None;
        }
        let flag_score = trimmed.ends_with('_');
        let name = if flag_score {
            let stripped = trimmed.strip_suffix('_').unwrap_or(trimmed);
            if stripped.ends_with('_') {
                return None;
            }
            stripped
        } else {
            trimmed
        };
        let (head_end, has_args) = match fused_completion {
            Some(head_end) => (head_end, false),
            None if end => (self.closer_extent(name_end, terminator)?, false),
            None => self.head_extent(name_end, terminator)?,
        };
        Some(HeadRead {
            name: name.to_string(),
            flag_star,
            flag_score,
            borrowed: !edited,
            segment: segment_consumed,
            head_end,
            name_end,
            head_segments,
            name_fused,
            has_args,
        })
    }

    fn head_extent(&self, name_end: usize, terminator: Token) -> Option<(usize, bool)> {
        match terminator {
            Token::RightBlock | Token::RightLink => return Some((name_end + 1, false)),
            Token::InputEnd => return None,
            _ => {}
        }
        let mut has_args = false;
        let mut in_double = false;
        let mut previous = Token::Whitespace;
        let mut j = name_end + 1;
        loop {
            let token = self.tokens.get(j)?;
            if !matches!(
                token.token,
                Token::Whitespace | Token::LineBreak | Token::ParagraphBreak
            ) {
                has_args = true;
            }
            match token.token {
                Token::InputEnd => return None,
                Token::RightBlock | Token::RightLink if !in_double => {
                    return Some((j + 1, has_args));
                }
                Token::RightBlock | Token::RightLink => {
                    let mut k = j + 1;
                    let closes_on_line = loop {
                        match self.tokens.get(k) {
                            None => break false,
                            Some(ahead) => match ahead.token {
                                Token::DoubleQuote => break true,
                                Token::LineBreak
                                | Token::ParagraphBreak
                                | Token::InputEnd => break false,
                                _ => k += 1,
                            },
                        }
                    };
                    if !closes_on_line {
                        return Some((j + 1, has_args));
                    }
                    previous = token.token;
                    j += 1;
                }
                Token::DoubleQuote if in_double => {
                    in_double = false;
                    previous = token.token;
                    j += 1;
                }
                Token::DoubleQuote
                    if matches!(
                        previous,
                        Token::Whitespace
                            | Token::LineBreak
                            | Token::ParagraphBreak
                            | Token::Equals
                            | Token::Pipe
                    ) =>
                {
                    in_double = true;
                    previous = token.token;
                    j += 1;
                }
                Token::LeftComment if !in_double => {
                    previous = Token::LeftComment;
                    j = self.skip_comment(j)?;
                }
                Token::LeftBlock if !in_double => {
                    previous = Token::LeftBlock;
                    if let Some(segment) = self.segment_at(j) {
                        let straddle = self.tokens.get(segment.end.wrapping_sub(1));
                        let completes =
                            straddle.is_some_and(|token| {
                                token.token == Token::RightLink
                                    && token.span.start < segment.range_end
                                    && segment.range_end < token.span.end
                            }) && self.tokens.get(segment.end).is_some_and(|token| {
                                token.token == Token::RightBracket
                                    && token.span.start == segment.range_end + 1
                            });
                        if completes {
                            return Some((segment.end + 1, has_args));
                        }
                        j = segment.end;
                    } else {
                        j += 1;
                    }
                }
                _ => {
                    previous = token.token;
                    j += 1;
                }
            }
            if j - name_end > 16384 {
                return None;
            }
        }
    }

    fn closer_extent(&self, name_end: usize, terminator: Token) -> Option<usize> {
        match terminator {
            Token::RightBlock | Token::RightLink => Some(name_end + 1),
            Token::Whitespace | Token::LineBreak | Token::ParagraphBreak => {
                let mut j = name_end + 1;
                let mut saw_break = terminator != Token::Whitespace;
                loop {
                    let token = self.tokens.get(j)?;
                    match token.token {
                        Token::Whitespace => j += 1,
                        Token::LineBreak if !saw_break => {
                            saw_break = true;
                            j += 1;
                        }
                        Token::RightBlock | Token::RightLink => return Some(j + 1),
                        _ => return None,
                    }
                }
            }
            _ => None,
        }
    }

    fn classify(&self, head: &HeadRead) -> Option<&'static str> {
        let block = get_block_rule_with_name_for_layout(&head.name, Layout::Wikidot)?;
        let actionable = if head.borrowed {
            true
        } else if head.segment {
            head_seam_forms_block(block, Layout::Wikidot)
        } else {
            !head_word_never_resolves(block)
        };
        if !actionable {
            return None;
        }
        if head.flag_star && !block.accepts_star() {
            return None;
        }
        if head.flag_score && !block.accepts_score() {
            return None;
        }
        Some(block.name())
    }

    fn kind_of(rule_name: &str) -> Option<WikidotBlockKind> {
        Some(match rule_name {
            "block-div" => WikidotBlockKind::Div,
            "block-collapsible" => WikidotBlockKind::Collapsible,
            "block-note" => WikidotBlockKind::Note,
            "block-align-center" => WikidotBlockKind::AlignCenter,
            "block-align-right" => WikidotBlockKind::AlignRight,
            "block-align-left" => WikidotBlockKind::AlignLeft,
            "block-align-justify" => WikidotBlockKind::AlignJustify,
            "block-tabview" => WikidotBlockKind::TabView,
            "block-tab" => WikidotBlockKind::Tab,
            "block-table" => WikidotBlockKind::Table,
            "block-table-row" => WikidotBlockKind::Row,
            "block-table-cell-regular" | "block-table-cell-header" => {
                WikidotBlockKind::Cell
            }
            _ => return None,
        })
    }

    fn bare_head(&self, head: &HeadRead) -> bool {
        head.head_end == head.name_end + 1
            && matches!(
                self.tokens[head.name_end].token,
                Token::RightBlock | Token::RightLink
            )
    }

    fn iframe_greedy_tail_range(
        &self,
        index: usize,
        head: &HeadRead,
    ) -> Option<(usize, usize)> {
        let head_end_token = self.tokens.get(head.head_end.checked_sub(1)?)?;
        if !matches!(head_end_token.token, Token::RightBlock | Token::RightLink) {
            return None;
        }
        let close_start = head_end_token.span.start;
        let head_start = self.tokens[index].span.start;
        let head_text = self.source.get(head_start..close_start)?;
        let inner = head_text.strip_prefix("[[")?;
        let (name, rest) = inner.split_once([' ', '\t'])?;
        if !name.eq_ignore_ascii_case("iframe") {
            return None;
        }
        let url = rest.trim_start_matches([' ', '\t']);
        if url.is_empty() || url.contains([' ', '\t', '[', ']', '\n', '\r']) {
            return None;
        }
        let (_, tail_end) = crate::parsing::rule::impls::wikidot_greedy_tail(
            self.source,
            url,
            close_start,
        )?;
        Some((close_start, tail_end))
    }

    fn line_quote_depth(&self, index: usize) -> usize {
        let line = self.line_of[index];
        let first = self.line_first[line];
        let mut depth = 0_usize;
        let mut t = first;
        while t < self.tokens.len() {
            match self.tokens[t].token {
                Token::Quote => depth += self.tokens[t].slice.len(),
                Token::Whitespace | Token::InputStart => {}
                _ => break,
            }
            t += 1;
        }
        depth
    }

    fn quote_depth_outside(&self, index: usize, head_depth: usize) -> bool {
        let depth = self.line_quote_depth(index);
        depth > head_depth || (head_depth > 0 && depth == 0)
    }

    fn skip_tabview_structure_space(&self, pos: usize, limit: usize) -> Option<usize> {
        let next = self.skip_structure_space(pos, limit)?;
        if (pos..next).any(|t| self.tokens[t].token == Token::ParagraphBreak) {
            None
        } else {
            Some(next)
        }
    }

    fn head_valid(
        &self,
        kind: WikidotBlockKind,
        head: &HeadRead,
        head_index: usize,
    ) -> bool {
        if kind.needs_own_line() && self.in_bibliography_body(head_index) {
            return false;
        }
        match kind {
            WikidotBlockKind::Div | WikidotBlockKind::Note => {
                !self.on_heading_line(head_index) && self.followed_by_break(head.head_end)
            }
            WikidotBlockKind::Collapsible => true,
            WikidotBlockKind::AlignCenter
            | WikidotBlockKind::AlignRight
            | WikidotBlockKind::AlignLeft
            | WikidotBlockKind::AlignJustify => {
                self.starts_line(head_index)
                    && self.followed_by_break(head.head_end)
                    && self.bare_head(head)
            }
            _ => true,
        }
    }

    fn line_prefix_clean(&self, index: usize) -> bool {
        let line = self.line_of[index];
        let first = self.line_first[line];
        let mut previous_quote = false;
        for t in first..index {
            match self.tokens[t].token {
                Token::Quote => previous_quote = true,
                Token::InputStart => {}
                Token::Whitespace if previous_quote && self.tokens[t].slice == " " => {
                    previous_quote = false
                }
                _ => return false,
            }
        }
        true
    }

    fn line_head_indented(&self, index: usize) -> bool {
        let line = self.line_of[index];
        let first = self.line_first[line];
        let mut previous_quote = false;
        for t in first..index {
            match self.tokens[t].token {
                Token::Quote => previous_quote = true,
                Token::InputStart => {}
                Token::Whitespace if previous_quote && self.tokens[t].slice == " " => {
                    previous_quote = false
                }
                Token::Whitespace => return true,
                _ => return false,
            }
        }
        false
    }

    fn closer_valid(
        &self,
        kind: WikidotBlockKind,
        closer_index: usize,
        closer_end: usize,
    ) -> bool {
        match kind {
            WikidotBlockKind::AlignCenter
            | WikidotBlockKind::AlignRight
            | WikidotBlockKind::AlignLeft
            | WikidotBlockKind::AlignJustify => {
                !self.in_bibliography_body(closer_index)
                    && self.starts_line(closer_index)
                    && self.followed_by_break(closer_end)
            }
            _ => true,
        }
    }
}

#[derive(Clone)]
struct Measure {
    end: usize,
    pairs: Vec<WikidotBlockPair>,
    children: Vec<(usize, usize)>,
}

const MAX_SCAN_DEPTH: usize = 256;

impl<'t> Scanner<'t> {
    fn finish(mut self) -> WikidotBlockPairs {
        self.pairs
            .sort_by_key(|pair| (pair.head, pair.kind, pair.closer));
        self.pairs.dedup_by(|a, b| {
            a.kind == b.kind && a.head == b.head && a.closer == b.closer
        });
        // L8.5: live drops the interior only for collapsible rowstart-head
        // x own-line-close pairs (st-open shape). Keep those pairs out of
        // the scan so the legacy descend-and-clear path owns them bit for
        // bit; every other pipe-head pair renders its interior.
        let mut dropped_heads = Vec::new();
        self.pairs.retain(|pair| {
            // closer_byte is populated below; read the span directly.
            let closer_byte = self.tokens[pair.closer].span.start;
            let head_end_byte = self.tokens[pair.head_end - 1].span.end;
            let head_line_end = self.source[head_end_byte..]
                .find('\n')
                .map_or(self.source.len(), |offset| head_end_byte + offset);
            let head_suffix_blank = self.source[head_end_byte..head_line_end]
                .trim_matches([' ', '\t'])
                .is_empty();
            let drop = pair.kind == WikidotBlockKind::Collapsible
                && pipe_rowstart_head_prefix(self.source, pair.head_byte)
                && head_suffix_blank
                && own_line_marker_prefix(self.source, closer_byte);
            if drop {
                dropped_heads.push(pair.head_byte);
            }
            !drop
        });
        for head in dropped_heads {
            self.head_event_bytes.remove(&head);
        }
        for pair in &mut self.pairs {
            pair.closer_byte = self.tokens[pair.closer].span.start;
        }
        let mut closer_end_bytes: BTreeMap<usize, usize> = BTreeMap::new();
        for pair in &self.pairs {
            let end = self.tokens[pair.closer_end - 1].span.end;
            closer_end_bytes.insert(pair.closer_byte, end);
        }
        let ranges: Vec<(usize, usize)> = self
            .pairs
            .iter()
            .map(|pair| {
                (
                    self.tokens[pair.head].span.start,
                    self.tokens[pair.closer_end - 1].span.end,
                )
            })
            .collect();
        let mut flat_flags = vec![false; self.pairs.len()];
        {
            let mut order: Vec<usize> = (0..self.pairs.len()).collect();
            order.sort_by_key(|&i| (ranges[i].0, std::cmp::Reverse(ranges[i].1)));
            let n = order.len();
            let mut size = 1_usize;
            while size < n {
                size *= 2;
            }
            let mut seg = vec![0_usize; 2 * size];
            for (pos, &i) in order.iter().enumerate() {
                seg[size + pos] = ranges[i].1;
            }
            for k in (1..size).rev() {
                seg[k] = seg[2 * k].max(seg[2 * k + 1]);
            }
            let range_max = |from: usize, to: usize| -> usize {
                let mut best = 0_usize;
                let mut l = size + from;
                let mut r = size + to;
                while l < r {
                    if l % 2 == 1 {
                        best = best.max(seg[l]);
                        l += 1;
                    }
                    if r % 2 == 1 {
                        r -= 1;
                        best = best.max(seg[r]);
                    }
                    l /= 2;
                    r /= 2;
                }
                best
            };
            let mut open: std::collections::BTreeMap<usize, usize> =
                std::collections::BTreeMap::new();
            let mut group_start = 0_usize;
            while group_start < n {
                let start_byte = ranges[order[group_start]].0;
                while let Some((&end, _)) = open.iter().next() {
                    if end <= start_byte {
                        open.remove(&end);
                    } else {
                        break;
                    }
                }
                let mut group_end = group_start;
                while group_end < n && ranges[order[group_end]].0 == start_byte {
                    group_end += 1;
                }
                for &i in &order[group_start..group_end] {
                    let (a_start, a_end) = ranges[i];
                    if let Some((&min_end, _)) = open.iter().next()
                        && min_end < a_end
                    {
                        flat_flags[i] = true;
                        continue;
                    }
                    let lo = order.partition_point(|&j| ranges[j].0 <= a_start);
                    let hi = order.partition_point(|&j| ranges[j].0 < a_end);
                    if lo < hi && range_max(lo, hi) > a_end {
                        flat_flags[i] = true;
                    }
                }
                for &i in &order[group_start..group_end] {
                    *open.entry(ranges[i].1).or_default() += 1;
                }
                group_start = group_end;
            }
        }
        for a in 0..self.pairs.len() {
            let (a_start, a_end) = ranges[a];
            let mut flat = flat_flags[a];
            if !flat && self.on_heading_line(self.pairs[a].closer) {
                flat = true;
            }
            if !flat {
                let body_after_heading_line = self.pairs[a].kind
                    == WikidotBlockKind::Collapsible
                    && self.on_heading_line(self.pairs[a].head)
                    && !self.on_heading_line(self.pairs[a].closer);
                if !body_after_heading_line {
                    let pos = self
                        .heading_line_end_bytes
                        .partition_point(|end| *end <= a_start);
                    flat = self
                        .heading_line_end_bytes
                        .get(pos)
                        .is_some_and(|end| *end < a_end);
                }
            }
            if !flat {
                flat = self.bib_ranges.first().is_some_and(|(start, end)| {
                    let pair = &self.pairs[a];
                    pair.head < *start && *start < pair.closer && pair.closer < *end
                });
            }
            self.pairs[a].flat = flat;
        }
        let tables: Vec<(usize, usize)> = self
            .pairs
            .iter()
            .enumerate()
            .filter(|(_, pair)| pair.kind == WikidotBlockKind::Table)
            .map(|(index, _)| ranges[index])
            .collect();
        let mut sources: Vec<(usize, usize)> = Vec::new();
        let mut foster_heads = BTreeSet::new();
        for (index, pair) in self.pairs.iter().enumerate() {
            if !pair.flat || !pair.kind.emits_flat() {
                continue;
            }
            let closer_byte = self.tokens[pair.closer].span.start;
            let innermost = tables
                .iter()
                .filter(|(start, end)| *start <= closer_byte && closer_byte < *end)
                .max_by_key(|(start, _)| *start);
            if innermost.is_some_and(|(start, _)| pair.head_byte < *start) {
                sources.push(ranges[index]);
                foster_heads.insert(pair.head_byte);
            }
        }
        let mut by_size: Vec<usize> = (0..self.pairs.len()).collect();
        by_size.sort_by_key(|&index| ranges[index].1 - ranges[index].0);
        for index in by_size {
            if self.pairs[index].flat || !self.pairs[index].kind.emits_flat() {
                continue;
            }
            let (start, end) = ranges[index];
            let contains_source = sources.iter().any(|(inner_start, inner_end)| {
                start <= *inner_start
                    && *inner_end <= end
                    && (start, end) != (*inner_start, *inner_end)
            });
            if contains_source {
                self.pairs[index].flat = true;
                sources.push((start, end));
            }
        }
        let by_head_byte = self
            .pairs
            .iter()
            .enumerate()
            .map(|(index, pair)| (pair.head_byte, index))
            .collect();
        let by_closer_byte = self
            .pairs
            .iter()
            .enumerate()
            .map(|(index, pair)| (self.tokens[pair.closer].span.start, index))
            .collect();
        let mut greedy_ranges = self.greedy_ranges;
        greedy_ranges.sort_unstable();
        greedy_ranges.dedup();
        WikidotBlockPairs {
            pairs: self.pairs,
            by_head_byte,
            by_closer_byte,
            head_event_bytes: self.head_event_bytes,
            closer_event_bytes: self.closer_event_bytes,
            foster_heads,
            greedy_ranges,
            heading_ranges: self.heading_ranges,
            closer_end_bytes,
        }
    }

    fn scan_segment(&mut self, start: usize, end: usize, depth: usize) {
        if depth > MAX_SCAN_DEPTH {
            return;
        }
        let mut events = Vec::new();
        self.walk_range(start, end, depth, &mut events);
        self.pair_events(events);
    }

    fn walk_range(
        &mut self,
        start: usize,
        end: usize,
        depth: usize,
        events: &mut Vec<Event>,
    ) {
        let mut i = start;
        while i < end {
            if self.head_dropped[i]
                && matches!(
                    self.tokens[i].token,
                    Token::LeftBlock | Token::LeftBlockStar
                )
            {
                i += 1;
                continue;
            }
            if let Some(zone_end) = self.zone_end(i) {
                i = zone_end.min(end);
                continue;
            }
            match self.tokens[i].token {
                Token::LeftComment => match self.skip_comment(i) {
                    Some(next) => i = next,
                    None => i += 1,
                },
                Token::LeftBlock | Token::LeftBlockStar => {
                    i = self.scan_head(i, end, depth, events);
                }
                Token::LeftBlockEnd => {
                    i = self.scan_closer(i, events);
                }
                _ => i += 1,
            }
        }
    }

    fn reprocess_head_segments(
        &mut self,
        head: &HeadRead,
        end: usize,
        depth: usize,
        events: &mut Vec<Event>,
    ) {
        for (body_start, body_end, include) in &head.head_segments {
            if *include {
                self.walk_range(*body_start, (*body_end).min(end), depth, events);
            }
        }
    }

    fn scan_head(
        &mut self,
        i: usize,
        end: usize,
        depth: usize,
        events: &mut Vec<Event>,
    ) -> usize {
        let first = &self.tokens[i];
        if first.token == Token::LeftBlock {
            if let Some(candidate) = self.code_candidates.get(&first.span.start)
                && self.raw_name_is(i, "code")
            {
                return self.token_at_or_after(candidate.owner_end);
            }
            if let Some(segment) = self.segment_at(i) {
                if segment.include_body {
                    return segment.head_end;
                }
                return segment.end;
            }
        }
        let Some(head) = self.read_head(i, false) else {
            return i + 1;
        };
        let Some(rule_name) = self.classify(&head) else {
            return i + 1;
        };
        if head.name_fused
            && matches!(
                rule_name,
                "block-div"
                    | "block-table"
                    | "block-tabview"
                    | "block-note"
                    | "block-align-left"
                    | "block-align-right"
                    | "block-align-center"
                    | "block-align-justify"
            )
        {
            return i + 1;
        }
        if rule_name == "block-note" && head.has_args {
            self.reprocess_head_segments(&head, end, depth, events);
            return head.head_end;
        }
        if rule_name == "block-iframe"
            && let Some(range) = self.iframe_greedy_tail_range(i, &head)
        {
            self.greedy_ranges.push(range);
            return self.token_at_or_after(range.1);
        }
        if let Some(kind) = Self::kind_of(rule_name)
            && kind.footnote_zone_gated()
            && self.in_footnote_first_line(self.tokens[i].span.start)
        {
            return head.head_end;
        }
        match Self::kind_of(rule_name) {
            Some(WikidotBlockKind::Table) => {
                if self.on_single_line_content_line(i) {
                    return head.head_end;
                }
                if let Some(measure) = self.measure_table(i, &head, end, depth) {
                    let next = measure.end;
                    self.apply_measure(measure, depth, events, true);
                    next
                } else {
                    head.head_end
                }
            }
            Some(WikidotBlockKind::TabView) => {
                if self.tabview_banned(i) || !self.line_prefix_clean(i) {
                    return head.head_end;
                }
                if let Some(measure) = self.measure_tabview(i, &head, end, depth) {
                    let next = measure.end;
                    self.apply_measure(measure, depth, events, false);
                    next
                } else {
                    head.head_end
                }
            }
            Some(
                kind @ (WikidotBlockKind::Div
                | WikidotBlockKind::AlignCenter
                | WikidotBlockKind::AlignRight
                | WikidotBlockKind::AlignLeft
                | WikidotBlockKind::AlignJustify
                | WikidotBlockKind::Collapsible
                | WikidotBlockKind::Note),
            ) => {
                if self.head_valid(kind, &head, i) {
                    events.push(Event {
                        kind,
                        scored: head.flag_score,
                        head: i,
                        head_end: head.head_end,
                        closer: 0,
                        closer_end: 0,
                        is_close: false,
                    });
                } else {
                    // Read but rejected (heading-line, mid-line): the scan
                    // saw this head, so descent may still judge it stolen.
                    self.head_event_bytes.insert(self.tokens[i].span.start);
                    self.reprocess_head_segments(&head, end, depth, events);
                }
                head.head_end
            }
            _ => {
                if rule_name == "block-module"
                    && let Some(next) = self.module_skip(i, &head, end)
                {
                    return next;
                }
                if rule_name == "block-footnote"
                    && let Some((body_start, body_end, after)) =
                        self.measure_footnote(&head, end, depth)
                {
                    let mut first = body_start;
                    while first < body_end
                        && matches!(
                            self.tokens[first].token,
                            Token::LineBreak | Token::ParagraphBreak
                        )
                    {
                        first += 1;
                    }
                    let line = self.line_of[first];
                    let previous = self.tabview_banned_lines[line];
                    self.tabview_banned_lines[line] = true;
                    self.scan_segment(body_start, body_end, depth + 1);
                    self.tabview_banned_lines[line] = previous;
                    return after;
                }
                head.head_end
            }
        }
    }

    fn scan_closer(&mut self, i: usize, events: &mut Vec<Event>) -> usize {
        let Some(head) = self.read_head(i, true) else {
            return i + 1;
        };
        let Some(rule_name) = self.classify(&head) else {
            return i + 1;
        };
        if let Some(
            kind @ (WikidotBlockKind::Div
            | WikidotBlockKind::AlignCenter
            | WikidotBlockKind::AlignRight
            | WikidotBlockKind::AlignLeft
            | WikidotBlockKind::AlignJustify
            | WikidotBlockKind::Collapsible
            | WikidotBlockKind::Note),
        ) = Self::kind_of(rule_name)
            && self.closer_valid(kind, i, head.head_end)
            && (!matches!(
                kind,
                WikidotBlockKind::AlignCenter
                    | WikidotBlockKind::AlignRight
                    | WikidotBlockKind::AlignLeft
                    | WikidotBlockKind::AlignJustify
            ) || self.bare_head(&head))
        {
            events.push(Event {
                kind,
                scored: false,
                head: 0,
                head_end: 0,
                closer: i,
                closer_end: head.head_end,
                is_close: true,
            });
        }
        head.head_end
    }

    fn bibliography_ranges(&self) -> Vec<(usize, usize)> {
        let mut ranges = Vec::new();
        let mut i = 0_usize;
        while i < self.tokens.len() {
            if let Some(next) = self.skip_bibliography_opaque(i) {
                i = next;
                continue;
            }
            if self.tokens[i].token == Token::LeftBlock
                && self.starts_line(i)
                && self.raw_name_is(i, "bibliography")
                && let Some(head) = self.read_head(i, false)
            {
                match self.bibliography_close_after(head.head_end) {
                    Some(end) => {
                        ranges.push((i, end));
                        i = end;
                        continue;
                    }
                    None => break,
                }
            }
            i += 1;
        }
        ranges
    }

    fn bibliography_close_after(&self, mut j: usize) -> Option<usize> {
        while j < self.tokens.len() {
            if let Some(next) = self.skip_bibliography_opaque(j) {
                j = next;
                continue;
            }
            match self.tokens[j].token {
                Token::LeftBlockEnd => {
                    if self.source[self.tokens[j].span.start..]
                        .starts_with("[[/bibliography]]")
                        && let Some(close) = self.read_head(j, true)
                        && self.followed_by_break(close.head_end)
                    {
                        return Some(close.head_end);
                    }
                    j += 1;
                }
                Token::InputEnd => return None,
                _ => j += 1,
            }
        }
        None
    }

    fn skip_bibliography_opaque(&self, i: usize) -> Option<usize> {
        let token = &self.tokens[i];
        match token.token {
            Token::LeftComment => self.skip_comment(i),
            Token::LeftBlock => {
                if let Some(candidate) = self.code_candidates.get(&token.span.start)
                    && self.raw_name_is(i, "code")
                {
                    return Some(self.token_at_or_after(candidate.owner_end));
                }
                if let Some(segment) = self.segment_at(i) {
                    return (!segment.include_body).then_some(segment.end);
                }
                if !self.raw_name_is(i, "math") {
                    return None;
                }
                let head = self.read_head(i, false)?;
                (head.head_end..self.tokens.len()).find_map(|j| {
                    (self.tokens[j].token == Token::LeftBlockEnd)
                        .then(|| self.read_head(j, true))
                        .flatten()
                        .filter(|close| close.name.eq_ignore_ascii_case("math"))
                        .map(|close| close.head_end)
                })
            }
            _ => None,
        }
    }

    fn in_bibliography_body(&self, index: usize) -> bool {
        self.bib_ranges
            .first()
            .is_some_and(|(start, end)| *start < index && index < *end)
    }

    fn tabview_banned(&self, head_index: usize) -> bool {
        self.tabview_banned_lines[self.line_of[head_index]]
            || self
                .bib_ranges
                .iter()
                .any(|(start, end)| *start < head_index && head_index < *end)
    }

    fn raw_name_is(&self, start: usize, expected: &str) -> bool {
        let mut j = start + 1;
        if self
            .tokens
            .get(j)
            .is_some_and(|token| token.token == Token::Whitespace)
        {
            j += 1;
        }
        let name_start = j;
        while let Some(token) = self.tokens.get(j) {
            match token.token {
                Token::Whitespace
                | Token::LineBreak
                | Token::ParagraphBreak
                | Token::RightBlock
                | Token::InputEnd => break,
                _ => j += 1,
            }
        }
        let text: String = self.tokens[name_start..j]
            .iter()
            .map(|token| token.slice)
            .collect();
        text.eq_ignore_ascii_case(expected)
    }

    fn apply_measure(
        &mut self,
        measure: Measure,
        depth: usize,
        events: &mut Vec<Event>,
        transparent: bool,
    ) {
        self.pairs.extend(measure.pairs);
        for (child_start, child_end) in measure.children {
            if transparent {
                self.walk_range(child_start, child_end, depth + 1, events);
            } else {
                self.scan_segment(child_start, child_end, depth + 1);
            }
        }
    }

    fn pair_events(&mut self, events: Vec<Event>) {
        for event in &events {
            if event.is_close {
                let byte = self.tokens[event.closer].span.start;
                self.closer_event_bytes.insert((event.kind, byte));
            } else {
                let byte = self.tokens[event.head].span.start;
                self.head_event_bytes.insert(byte);
            }
        }
        const KINDS: [WikidotBlockKind; 7] = [
            WikidotBlockKind::Div,
            WikidotBlockKind::Collapsible,
            WikidotBlockKind::Note,
            WikidotBlockKind::AlignCenter,
            WikidotBlockKind::AlignRight,
            WikidotBlockKind::AlignLeft,
            WikidotBlockKind::AlignJustify,
        ];
        for kind in KINDS {
            let evs: Vec<&Event> =
                events.iter().filter(|event| event.kind == kind).collect();
            if evs.is_empty() {
                continue;
            }
            let found = if kind.is_nesting() {
                pair_nest(&evs)
            } else {
                pair_first(&evs)
            };
            for (h, c) in found {
                let head_byte = self.head_byte(evs[h].head);
                self.pairs.push(WikidotBlockPair {
                    kind,
                    scored: evs[h].scored,
                    head: evs[h].head,
                    head_end: evs[h].head_end,
                    head_byte,
                    closer: evs[c].closer,
                    closer_end: evs[c].closer_end,
                    flat: false,
                    closer_byte: 0,
                });
            }
        }
    }
}

fn pair_first(evs: &[&Event]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < evs.len() {
        if evs[i].is_close {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let partner = loop {
            if j >= evs.len() {
                break None;
            }
            if evs[j].is_close {
                break Some(j);
            }
            j += 1;
        };
        match partner {
            Some(p) => {
                out.push((i, p));
                i = p + 1;
            }
            None => break,
        }
    }
    out
}

struct NestTree {
    size: usize,
    sum: Vec<i64>,
    min_prefix: Vec<i64>,
    heads: Vec<usize>,
    closers: Vec<usize>,
}

impl NestTree {
    fn new(evs: &[&Event]) -> Self {
        let mut size = 1_usize;
        while size < evs.len() {
            size *= 2;
        }
        let mut tree = NestTree {
            size,
            sum: vec![0; 2 * size],
            min_prefix: vec![0; 2 * size],
            heads: vec![0; 2 * size],
            closers: vec![0; 2 * size],
        };
        for (i, ev) in evs.iter().enumerate() {
            let value = if ev.is_close { -1 } else { 1 };
            tree.sum[size + i] = value;
            tree.min_prefix[size + i] = value;
            tree.heads[size + i] = usize::from(!ev.is_close);
            tree.closers[size + i] = usize::from(ev.is_close);
        }
        for k in (1..size).rev() {
            tree.pull(k);
        }
        tree
    }

    fn pull(&mut self, k: usize) {
        self.sum[k] = self.sum[2 * k] + self.sum[2 * k + 1];
        self.min_prefix[k] =
            self.min_prefix[2 * k].min(self.sum[2 * k] + self.min_prefix[2 * k + 1]);
        self.heads[k] = self.heads[2 * k] + self.heads[2 * k + 1];
        self.closers[k] = self.closers[2 * k] + self.closers[2 * k + 1];
    }

    fn kill(&mut self, index: usize) {
        let mut k = self.size + index;
        self.sum[k] = 0;
        self.min_prefix[k] = 0;
        self.heads[k] = 0;
        self.closers[k] = 0;
        k /= 2;
        while k >= 1 {
            self.pull(k);
            if k == 1 {
                break;
            }
            k /= 2;
        }
    }

    fn is_alive_head(&self, index: usize) -> bool {
        self.heads[self.size + index] == 1
    }

    fn range_count(&self, values: &[usize], mut l: usize, mut r: usize) -> usize {
        let mut total = 0_usize;
        l += self.size;
        r += self.size;
        while l < r {
            if l % 2 == 1 {
                total += values[l];
                l += 1;
            }
            if r % 2 == 1 {
                r -= 1;
                total += values[r];
            }
            l /= 2;
            r /= 2;
        }
        total
    }

    fn range_heads(&self, l: usize, r: usize) -> usize {
        self.range_count(&self.heads, l, r)
    }

    fn range_closers(&self, l: usize, r: usize) -> usize {
        self.range_count(&self.closers, l, r)
    }

    fn first_head(&self, lo: usize, hi: usize) -> Option<usize> {
        self.find_first(1, 0, self.size, lo, hi)
    }

    fn find_first(
        &self,
        node: usize,
        l: usize,
        r: usize,
        lo: usize,
        hi: usize,
    ) -> Option<usize> {
        if r <= lo || l >= hi || self.heads[node] == 0 {
            return None;
        }
        if r - l == 1 {
            return Some(l);
        }
        let mid = (l + r) / 2;
        self.find_first(2 * node, l, mid, lo, hi)
            .or_else(|| self.find_first(2 * node + 1, mid, r, lo, hi))
    }

    fn balance(&self, lo: usize, hi: usize) -> Option<usize> {
        self.find_balance(1, 0, self.size, lo, hi, 0)
    }

    fn find_balance(
        &self,
        node: usize,
        l: usize,
        r: usize,
        lo: usize,
        hi: usize,
        acc: i64,
    ) -> Option<usize> {
        if r <= lo || l >= hi {
            return None;
        }
        if lo <= l && r <= hi && acc + self.min_prefix[node] > 0 {
            return None;
        }
        if r - l == 1 {
            return (acc + self.sum[node] == 0).then_some(l);
        }
        let mid = (l + r) / 2;
        if let Some(found) = self.find_balance(2 * node, l, mid, lo, hi, acc) {
            return Some(found);
        }
        let left_from = l.max(lo);
        let left_to = mid.min(hi);
        let left_sum = if left_from < left_to {
            self.range_sum(left_from, left_to)
        } else {
            0
        };
        self.find_balance(2 * node + 1, mid, r, lo, hi, acc + left_sum)
    }

    fn range_sum(&self, mut l: usize, mut r: usize) -> i64 {
        let mut total = 0_i64;
        l += self.size;
        r += self.size;
        while l < r {
            if l % 2 == 1 {
                total += self.sum[l];
                l += 1;
            }
            if r % 2 == 1 {
                r -= 1;
                total += self.sum[r];
            }
            l /= 2;
            r /= 2;
        }
        total
    }
}

fn pair_nest(evs: &[&Event]) -> Vec<(usize, usize)> {
    let mut tree = NestTree::new(evs);
    let mut out = Vec::new();
    let mut stack: Vec<(usize, usize)> = vec![(0, evs.len())];
    while let Some((mut lo, mut hi)) = stack.pop() {
        loop {
            let Some(h) = tree.first_head(lo, hi) else {
                break;
            };
            let heads = tree.range_heads(h, hi);
            let closers = tree.range_closers(h, hi);
            if heads > closers {
                let mut to_kill = heads - closers;
                for k in (h..hi).rev() {
                    if to_kill == 0 {
                        break;
                    }
                    if tree.is_alive_head(k) {
                        tree.kill(k);
                        to_kill -= 1;
                    }
                }
                if !tree.is_alive_head(h) {
                    continue;
                }
            }
            let Some(p) = tree.balance(h, hi) else {
                tree.kill(h);
                continue;
            };
            out.push((h, p));
            if p + 1 < hi {
                stack.push((p + 1, hi));
            }
            lo = h + 1;
            hi = p;
        }
    }
    out
}

impl<'t> Scanner<'t> {
    fn read_kind_head(&self, pos: usize, kind: WikidotBlockKind) -> Option<HeadRead> {
        let head = self.read_head(pos, false)?;
        let rule_name = self.classify(&head)?;
        (Self::kind_of(rule_name) == Some(kind)).then_some(head)
    }

    fn read_kind_closer(&self, pos: usize, kind: WikidotBlockKind) -> Option<HeadRead> {
        let head = self.read_head(pos, true)?;
        let rule_name = self.classify(&head)?;
        (Self::kind_of(rule_name) == Some(kind)).then_some(head)
    }

    fn skip_structure_space(&self, pos: usize, limit: usize) -> Option<usize> {
        self.skip_structure_space_depth(pos, limit, 0)
    }

    fn skip_structure_space_depth(
        &self,
        mut pos: usize,
        limit: usize,
        depth: usize,
    ) -> Option<usize> {
        loop {
            if pos >= limit || self.zone_end(pos).is_some() {
                return None;
            }
            match self.tokens[pos].token {
                Token::Whitespace
                | Token::LineBreak
                | Token::ParagraphBreak
                | Token::Quote => pos += 1,
                Token::LeftComment => pos = self.skip_comment(pos)?,
                Token::LeftBlock if depth < 32 => {
                    if let Some(segment) = self.segment_at(pos) {
                        if segment.include_body {
                            let body_first = self.token_at_or_after(segment.body.start);
                            let body_last = self.token_at_or_after(segment.body.end);
                            if let Some(inner) = self.skip_structure_space_depth(
                                body_first,
                                body_last.min(limit),
                                depth + 1,
                            ) {
                                return Some(inner);
                            }
                        }
                        pos = segment.end;
                    } else {
                        return Some(pos);
                    }
                }
                _ => return Some(pos),
            }
        }
    }

    fn module_skip(
        &self,
        head_index: usize,
        head: &HeadRead,
        limit: usize,
    ) -> Option<usize> {
        if limit != self.tokens.len() {
            return self.module_skip_search(head_index, head, limit);
        }
        if let Some(cached) = self.module_skip_memo.borrow().get(&head_index) {
            return *cached;
        }
        let result = self.module_skip_search(head_index, head, limit);
        self.module_skip_memo
            .borrow_mut()
            .insert(head_index, result);
        result
    }

    fn module_skip_search(
        &self,
        head_index: usize,
        head: &HeadRead,
        limit: usize,
    ) -> Option<usize> {
        let mut k = head_index + 1;
        while k < head.head_end
            && !matches!(
                self.tokens[k].token,
                Token::Whitespace
                    | Token::LineBreak
                    | Token::ParagraphBreak
                    | Token::RightBlock
                    | Token::RightLink
            )
        {
            k += 1;
        }
        while k < head.head_end && self.tokens[k].token == Token::Whitespace {
            k += 1;
        }
        let mut subname = String::new();
        while k < head.head_end
            && !matches!(
                self.tokens[k].token,
                Token::Whitespace
                    | Token::LineBreak
                    | Token::ParagraphBreak
                    | Token::RightBlock
                    | Token::RightLink
            )
        {
            subname.push_str(self.tokens[k].slice);
            k += 1;
        }
        if matches!(
            subname.to_ascii_lowercase().as_str(),
            "backlinks"
                | "categories"
                | "join"
                | "pagetree"
                | "rate"
                | "tagcloud"
                | "redirect"
        ) {
            return Some(head.head_end);
        }
        let next_closer = self
            .module_closers
            .partition_point(|closer| *closer < head.head_end);
        if self
            .module_closers
            .get(next_closer)
            .is_none_or(|closer| *closer >= limit)
        {
            return Some(head.head_end);
        }
        let mut j = head.head_end;
        while j < limit {
            if let Some(zone_end) = self.zone_end(j) {
                j = zone_end.min(limit);
                continue;
            }
            match self.tokens[j].token {
                Token::LeftComment => {
                    j = self.skip_comment(j)?;
                }
                Token::LeftBlockEnd => {
                    if let Some(close) = self.read_head(j, true) {
                        if close.name.eq_ignore_ascii_case("module") {
                            return Some(close.head_end);
                        }
                        j = close.head_end;
                    } else {
                        j += 1;
                    }
                }
                Token::LeftBlock | Token::LeftBlockStar => {
                    j = self.skip_construct_in_body(j, limit, 0)?;
                }
                _ => j += 1,
            }
        }
        Some(head.head_end)
    }

    fn skip_construct_in_body(
        &self,
        j: usize,
        limit: usize,
        depth: usize,
    ) -> Option<usize> {
        self.skip_construct_in_body_flagged(j, limit, depth, true)
    }

    fn skip_construct_in_body_flagged(
        &self,
        j: usize,
        limit: usize,
        depth: usize,
        skip_module_bodies: bool,
    ) -> Option<usize> {
        let first = &self.tokens[j];
        if first.token == Token::LeftBlock {
            if let Some(candidate) = self.code_candidates.get(&first.span.start)
                && self.raw_name_is(j, "code")
            {
                return Some(self.token_at_or_after(candidate.owner_end));
            }
            if let Some(segment) = self.segment_at(j) {
                return Some(if segment.include_body {
                    segment.head_end
                } else {
                    segment.end
                });
            }
        }
        let head = self.read_head(j, false)?;
        let Some(rule_name) = self.classify(&head) else {
            return Some(j + 1);
        };
        match Self::kind_of(rule_name) {
            Some(WikidotBlockKind::Table) => {
                match self.measure_table(j, &head, limit, depth + 1) {
                    Some(measure) => Some(measure.end),
                    None => Some(head.head_end),
                }
            }
            Some(WikidotBlockKind::TabView) => {
                match self.measure_tabview(j, &head, limit, depth + 1) {
                    Some(measure) => Some(measure.end),
                    None => Some(head.head_end),
                }
            }
            _ => {
                if rule_name == "block-footnote" {
                    match self.measure_footnote(&head, limit, depth) {
                        Some((_, _, after)) => Some(after),
                        None => Some(head.head_end),
                    }
                } else if rule_name == "block-module" && skip_module_bodies {
                    self.module_skip(j, &head, limit)
                } else {
                    Some(head.head_end)
                }
            }
        }
    }

    fn measure_footnote(
        &self,
        head: &HeadRead,
        limit: usize,
        depth: usize,
    ) -> Option<(usize, usize, usize)> {
        let mut count = 1_usize;
        let mut j = head.head_end;
        while j < limit {
            if let Some(zone_end) = self.zone_end(j) {
                j = zone_end.min(limit);
                continue;
            }
            match self.tokens[j].token {
                Token::LeftComment => {
                    j = self.skip_comment(j)?;
                }
                Token::LeftBlock | Token::LeftBlockStar => {
                    let inner = self.read_head(j, false);
                    if inner.as_ref().and_then(|inner| self.classify(inner))
                        == Some("block-footnote")
                    {
                        count += 1;
                        j = inner?.head_end;
                    } else {
                        j = self.skip_construct_in_body(j, limit, depth)?;
                    }
                }
                Token::LeftBlockEnd => {
                    if let Some(close) = self.read_head(j, true) {
                        if self.classify(&close) == Some("block-footnote") {
                            count -= 1;
                            if count == 0 {
                                return Some((head.head_end, j, close.head_end));
                            }
                        }
                        j = close.head_end;
                    } else {
                        j += 1;
                    }
                }
                _ => j += 1,
            }
        }
        None
    }

    fn find_cell_end(
        &self,
        start: usize,
        limit: usize,
        depth: usize,
    ) -> Option<(usize, usize, usize)> {
        let mut j = start;
        while j < limit {
            if let Some(zone_end) = self.zone_end(j) {
                j = zone_end.min(limit);
                continue;
            }
            match self.tokens[j].token {
                Token::LeftComment => {
                    j = self.skip_comment(j)?;
                }
                Token::LeftBlock | Token::LeftBlockStar => {
                    j = self.skip_construct_in_body(j, limit, depth)?;
                }
                Token::LeftBlockEnd => {
                    if let Some(close) = self.read_head(j, true) {
                        match self.classify(&close).and_then(Self::kind_of) {
                            Some(WikidotBlockKind::Cell) => {
                                return Some((j, j, close.head_end));
                            }
                            Some(WikidotBlockKind::Row)
                            | Some(WikidotBlockKind::Table) => {
                                return None;
                            }
                            _ => j = close.head_end,
                        }
                    } else {
                        j += 1;
                    }
                }
                _ => j += 1,
            }
        }
        None
    }

    fn find_tab_end(
        &self,
        start: usize,
        limit: usize,
        depth: usize,
    ) -> Option<(usize, usize, usize)> {
        let mut j = start;
        while j < limit {
            if let Some(zone_end) = self.zone_end(j) {
                j = zone_end.min(limit);
                continue;
            }
            match self.tokens[j].token {
                Token::LeftComment => {
                    j = self.skip_comment(j)?;
                }
                Token::LeftBlock | Token::LeftBlockStar => {
                    j = self.skip_construct_in_body(j, limit, depth)?;
                }
                Token::LeftBlockEnd => {
                    if let Some(close) = self.read_head(j, true) {
                        match self.classify(&close).and_then(Self::kind_of) {
                            Some(WikidotBlockKind::Tab) => {
                                return Some((j, j, close.head_end));
                            }
                            Some(WikidotBlockKind::TabView) => return None,
                            _ => j = close.head_end,
                        }
                    } else {
                        j += 1;
                    }
                }
                _ => j += 1,
            }
        }
        None
    }

    fn measure_table(
        &self,
        head_index: usize,
        head: &HeadRead,
        limit: usize,
        depth: usize,
    ) -> Option<Measure> {
        if depth > MAX_SCAN_DEPTH {
            return None;
        }
        if let Some(cached) = self.table_measure_memo.borrow().get(&(head_index, limit)) {
            return cached.clone();
        }
        if self.measure_guard.get() >= MAX_SCAN_DEPTH {
            self.measure_guard_trips
                .set(self.measure_guard_trips.get() + 1);
            return None;
        }
        self.measure_guard.set(self.measure_guard.get() + 1);
        let trips_before = self.measure_guard_trips.get();
        let result = self.measure_table_inner(head_index, head, limit, 0);
        self.measure_guard.set(self.measure_guard.get() - 1);
        if self.measure_guard_trips.get() == trips_before {
            self.table_measure_memo
                .borrow_mut()
                .insert((head_index, limit), result.clone());
        }
        result
    }

    fn measure_table_inner(
        &self,
        head_index: usize,
        head: &HeadRead,
        limit: usize,
        depth: usize,
    ) -> Option<Measure> {
        if depth > MAX_SCAN_DEPTH {
            return None;
        }
        let mut measure = Measure {
            end: 0,
            pairs: Vec::new(),
            children: Vec::new(),
        };
        let mut rows = 0_usize;
        let mut pos = head.head_end;
        loop {
            pos = self.skip_structure_space(pos, limit)?;
            if let Some(close) = self.read_kind_closer(pos, WikidotBlockKind::Table) {
                if rows == 0 {
                    return None;
                }
                let head_byte = self.head_byte(head_index);
                measure.pairs.push(WikidotBlockPair {
                    kind: WikidotBlockKind::Table,
                    scored: false,
                    head: head_index,
                    head_end: head.head_end,
                    head_byte,
                    closer: pos,
                    closer_end: close.head_end,
                    flat: false,
                    closer_byte: 0,
                });
                measure.end = close.head_end;
                return Some(measure);
            }
            let row_head = self.read_kind_head(pos, WikidotBlockKind::Row)?;
            let row_index = pos;
            rows += 1;
            let mut cells = 0_usize;
            let mut rpos = row_head.head_end;
            let mut surplus = false;
            let (row_closer, row_closer_end) = loop {
                rpos = self.skip_structure_space(rpos, limit)?;
                if let Some(close) = self.read_kind_closer(rpos, WikidotBlockKind::Row) {
                    if cells == 0 {
                        return None;
                    }
                    break (rpos, close.head_end);
                }
                if let Some(close) = self.read_kind_closer(rpos, WikidotBlockKind::Cell) {
                    if cells == 0 {
                        return None;
                    }
                    surplus = true;
                    rpos = close.head_end;
                    continue;
                }
                if surplus {
                    return None;
                }
                cells += 1;
                let cell_head = self.read_kind_head(rpos, WikidotBlockKind::Cell)?;
                let (body_end, cell_closer, cell_closer_end) =
                    self.find_cell_end(cell_head.head_end, limit, depth)?;
                measure.children.push((cell_head.head_end, body_end));
                let head_byte = self.head_byte(rpos);
                measure.pairs.push(WikidotBlockPair {
                    kind: WikidotBlockKind::Cell,
                    scored: false,
                    head: rpos,
                    head_end: cell_head.head_end,
                    head_byte,
                    closer: cell_closer,
                    closer_end: cell_closer_end,
                    flat: false,
                    closer_byte: 0,
                });
                rpos = cell_closer_end;
            };
            let head_byte = self.head_byte(row_index);
            measure.pairs.push(WikidotBlockPair {
                kind: WikidotBlockKind::Row,
                scored: false,
                head: row_index,
                head_end: row_head.head_end,
                head_byte,
                closer: row_closer,
                closer_end: row_closer_end,
                closer_byte: 0,
                flat: false,
            });
            pos = row_closer_end;
        }
    }

    fn measure_tabview(
        &self,
        head_index: usize,
        head: &HeadRead,
        limit: usize,
        depth: usize,
    ) -> Option<Measure> {
        if depth > MAX_SCAN_DEPTH {
            return None;
        }
        if let Some(cached) = self.tabview_measure_memo.borrow().get(&(head_index, limit))
        {
            return cached.clone();
        }
        if self.measure_guard.get() >= MAX_SCAN_DEPTH {
            self.measure_guard_trips
                .set(self.measure_guard_trips.get() + 1);
            return None;
        }
        self.measure_guard.set(self.measure_guard.get() + 1);
        let trips_before = self.measure_guard_trips.get();
        let result = self.measure_tabview_inner(head_index, head, limit, 0);
        self.measure_guard.set(self.measure_guard.get() - 1);
        if self.measure_guard_trips.get() == trips_before {
            self.tabview_measure_memo
                .borrow_mut()
                .insert((head_index, limit), result.clone());
        }
        result
    }

    /// T1: mirror of the consumer's single-separator tab-title failure
    /// (`WikidotHeadSpaceRegion::SingleSeparator` in `parse_tab`). A tab head
    /// whose argument region (comments stripped) is exactly one space or one
    /// newline fails; the tree keeps it as failed content, never a Tab, so
    /// the scan predicts no pair for it while still scanning its body.
    fn tab_head_failed_title(&self, head: &HeadRead) -> bool {
        let Some(name_token) = self.tokens.get(head.name_end) else {
            return false;
        };
        let Some(close_token) = self.tokens.get(head.head_end.wrapping_sub(1)) else {
            return false;
        };
        if !matches!(close_token.token, Token::RightBlock | Token::RightLink) {
            return false;
        }
        let (start, end) = (name_token.span.start, close_token.span.start);
        let region = if start <= end {
            self.source.get(start..end).unwrap_or("")
        } else {
            return false;
        };
        let mut stripped = String::new();
        let mut rest = region;
        loop {
            match rest.find("[!--") {
                None => {
                    stripped.push_str(rest);
                    break;
                }
                Some(open) => {
                    stripped.push_str(&rest[..open]);
                    match rest[open + 4..].find("--]") {
                        Some(close) => {
                            rest = &rest[open + 4 + close + 3..];
                        }
                        None => return false,
                    }
                }
            }
        }
        stripped == " " || stripped == "\n"
    }

    /// T1: leading-space tab heads (`[[ tab]]`, T4) fail on live. The tree
    /// keeps them as failed content; the scan skips the head and its body
    /// without a pair. Unlike single-separator failures they do not count
    /// toward view formation (a view of only leading-space heads fails).
    fn tab_leading_space_head(&self, pos: usize) -> bool {
        if !matches!(
            self.tokens.get(pos).map(|token| token.token),
            Some(Token::LeftBlock)
        ) {
            return false;
        }
        let mut j = pos + 1;
        if !matches!(
            self.tokens.get(j).map(|token| token.token),
            Some(Token::Whitespace)
        ) {
            return false;
        }
        while matches!(
            self.tokens.get(j).map(|token| token.token),
            Some(Token::Whitespace)
        ) {
            j += 1;
        }
        let mut end = j;
        while let Some(token) = self.tokens.get(end) {
            match token.token {
                Token::Whitespace
                | Token::LineBreak
                | Token::ParagraphBreak
                | Token::RightBlock
                | Token::InputEnd => break,
                _ => end += 1,
            }
        }
        let text: String = self.tokens[j..end]
            .iter()
            .map(|token| token.slice)
            .collect();
        text.eq_ignore_ascii_case("tab")
    }

    fn tab_head_first_inner_block(&self, pos: usize, head: &HeadRead) -> Option<String> {
        let mut name_end = pos + 1;
        while name_end < head.head_end
            && !matches!(
                self.tokens[name_end].token,
                Token::Whitespace
                    | Token::LineBreak
                    | Token::ParagraphBreak
                    | Token::RightBlock
                    | Token::RightLink
            )
        {
            name_end += 1;
        }
        let first_content = (name_end..head.head_end)
            .find(|&t| self.tokens[t].token != Token::Whitespace);
        let t = first_content?;
        if !matches!(
            self.tokens[t].token,
            Token::LeftBlock | Token::LeftBlockStar
        ) {
            return None;
        }
        Some(
            (t + 1..head.head_end)
                .take_while(|&k| {
                    !matches!(
                        self.tokens[k].token,
                        Token::Whitespace
                            | Token::RightBlock
                            | Token::RightLink
                            | Token::LineBreak
                            | Token::ParagraphBreak
                            | Token::InputEnd
                    )
                })
                .map(|k| self.tokens[k].slice)
                .collect(),
        )
    }

    fn tab_head_footnote_first(&self, pos: usize, head: &HeadRead) -> bool {
        self.tab_head_first_inner_block(pos, head)
            .is_some_and(|name| {
                name.eq_ignore_ascii_case("footnote")
                    || name.eq_ignore_ascii_case("footnoteblock")
            })
    }

    /// A tab head whose title starts with a footnoteblock block fails, but
    /// live keeps it as literal content of the enclosing view; the scanner
    /// mirrors that by skipping the head and accepting content lines after it.
    fn tab_head_footnoteblock_first(&self, pos: usize, head: &HeadRead) -> bool {
        self.tab_head_first_inner_block(pos, head)
            .is_some_and(|name| name.eq_ignore_ascii_case("footnoteblock"))
    }

    /// T1: scan a failed tab head's body for nested real tabs. A failed
    /// outer tab containing a good inner tab still forms the inner (T2),
    /// so unlike `find_tab_end` (which skips nested heads) this pairs good
    /// inner heads into the measure and recurses into failed ones. End
    /// detection mirrors `find_tab_end`: the first tab closer ends the
    /// body, a tabview closer or the limit bails. Returns
    /// (body_end, closer, closer_end) like `find_tab_end`.
    fn scan_failed_tab_body(
        &self,
        start: usize,
        limit: usize,
        depth: usize,
        measure: &mut Measure,
    ) -> Option<(usize, usize, usize)> {
        if depth > MAX_SCAN_DEPTH {
            return None;
        }
        let mut j = start;
        loop {
            if j >= limit {
                return None;
            }
            if let Some(zone_end) = self.zone_end(j) {
                j = zone_end.min(limit);
                continue;
            }
            match self.tokens[j].token {
                Token::LeftComment => {
                    j = self.skip_comment(j)?;
                }
                Token::LeftBlock | Token::LeftBlockStar => {
                    if let Some(inner) = self.read_kind_head(j, WikidotBlockKind::Tab) {
                        if self.tab_head_failed_title(&inner)
                            || self.tab_head_footnote_first(j, &inner)
                        {
                            let (_, _, inner_end) = self.scan_failed_tab_body(
                                inner.head_end,
                                limit,
                                depth + 1,
                                measure,
                            )?;
                            j = inner_end;
                        } else {
                            let (inner_body, inner_closer, inner_end) =
                                self.find_tab_end(inner.head_end, limit, depth + 1)?;
                            measure.children.push((inner.head_end, inner_body));
                            let head_byte = self.head_byte(j);
                            measure.pairs.push(WikidotBlockPair {
                                kind: WikidotBlockKind::Tab,
                                scored: false,
                                head: j,
                                head_end: inner.head_end,
                                head_byte,
                                closer: inner_closer,
                                closer_end: inner_end,
                                closer_byte: 0,
                                flat: false,
                            });
                            j = inner_end;
                        }
                    } else if self.tab_leading_space_head(j) {
                        let (_, _, inner_end) =
                            self.scan_failed_tab_body(j + 1, limit, depth + 1, measure)?;
                        j = inner_end;
                    } else {
                        j = self.skip_construct_in_body(j, limit, depth)?;
                    }
                }
                Token::LeftBlockEnd => {
                    if let Some(close) = self.read_head(j, true) {
                        match self.classify(&close).and_then(Self::kind_of) {
                            Some(WikidotBlockKind::Tab) => {
                                return Some((j, j, close.head_end));
                            }
                            Some(WikidotBlockKind::TabView) => return None,
                            _ => j = close.head_end,
                        }
                    } else {
                        j += 1;
                    }
                }
                _ => j += 1,
            }
        }
    }

    fn measure_tabview_inner(
        &self,
        head_index: usize,
        head: &HeadRead,
        limit: usize,
        depth: usize,
    ) -> Option<Measure> {
        if depth > MAX_SCAN_DEPTH {
            return None;
        }
        let mut measure = Measure {
            end: 0,
            pairs: Vec::new(),
            children: Vec::new(),
        };
        let head_depth = self.line_quote_depth(head_index);
        let mut tabs = 0_usize;
        let mut degraded_content = false;
        // Live starts a tabview body with a tab head: a body whose first
        // significant construct is prose or a stray closer fails the view even
        // when a later attached closer would pair. Probe-frozen in
        // co-parse/work/b19-predictions2.json (q2/q6).
        let mut first_construct = false;
        // Live pairs a view closer only when only structure space separates it
        // from the body's last tab closer; a closer after prose stays literal
        // and the view keeps scanning for a later attached closer
        // (tabview/tab-stray-fails).
        let mut last_tab_close = false;
        let mut pos = head.head_end;
        loop {
            pos = if head_depth > 0 {
                self.skip_tabview_structure_space(pos, limit)?
            } else {
                self.skip_structure_space(pos, limit)?
            };
            if let Some(close) = self.read_kind_closer(pos, WikidotBlockKind::TabView) {
                if self.quote_depth_outside(pos, head_depth) {
                    return None;
                }
                if !last_tab_close {
                    // Not attached: the closer stays literal body content and
                    // the scan continues toward a later attached closer.
                    pos = close.head_end;
                    continue;
                }
                if !first_construct || (tabs == 0 && !degraded_content) {
                    return None;
                }
                let head_byte = self.head_byte(head_index);
                measure.pairs.push(WikidotBlockPair {
                    kind: WikidotBlockKind::TabView,
                    scored: false,
                    head: head_index,
                    head_end: head.head_end,
                    head_byte,
                    closer: pos,
                    closer_end: close.head_end,
                    flat: false,
                    closer_byte: 0,
                });
                measure.end = close.head_end;
                return Some(measure);
            }
            if self.line_head_indented(pos) || self.quote_depth_outside(pos, head_depth) {
                return None;
            }
            // T1: stray tab closers are failed content, not heads. The
            // consumer skips them without failing the view; mirror that
            // instead of bailing the whole view prediction. A stray closer
            // anchors a following view closer, but cannot open the body.
            let tab_head = match self.read_kind_head(pos, WikidotBlockKind::Tab) {
                Some(tab_head) => tab_head,
                None => {
                    if self.tab_leading_space_head(pos) {
                        let (body_end, tab_closer, tab_closer_end) = self
                            .scan_failed_tab_body(pos + 1, limit, depth, &mut measure)?;
                        if self.quote_depth_outside(tab_closer, head_depth) {
                            return None;
                        }
                        measure.children.push((pos + 1, body_end));
                        first_construct = true;
                        pos = tab_closer_end;
                        last_tab_close = true;
                        continue;
                    }
                    if let Some(stray) = self.read_kind_closer(pos, WikidotBlockKind::Tab)
                    {
                        if !first_construct {
                            return None;
                        }
                        pos = stray.head_end;
                        last_tab_close = true;
                        continue;
                    }
                    if !first_construct {
                        return None;
                    }
                    pos = self.skip_tabview_content(pos, limit)?;
                    last_tab_close = false;
                    continue;
                }
            };
            first_construct = true;
            if self.tab_head_footnoteblock_first(pos, &tab_head) {
                degraded_content = true;
                pos = tab_head.head_end;
                last_tab_close = false;
                continue;
            }
            tabs += 1;
            let failed = self.tab_head_failed_title(&tab_head);
            if self.tab_head_footnote_first(pos, &tab_head) {
                return None;
            }
            let (body_end, tab_closer, tab_closer_end) = if failed {
                self.scan_failed_tab_body(tab_head.head_end, limit, depth, &mut measure)?
            } else {
                self.find_tab_end(tab_head.head_end, limit, depth)?
            };
            if self.quote_depth_outside(tab_closer, head_depth) {
                return None;
            }
            measure.children.push((tab_head.head_end, body_end));
            if !failed {
                let head_byte = self.head_byte(pos);
                measure.pairs.push(WikidotBlockPair {
                    kind: WikidotBlockKind::Tab,
                    scored: false,
                    head: pos,
                    head_end: tab_head.head_end,
                    head_byte,
                    closer: tab_closer,
                    closer_end: tab_closer_end,
                    closer_byte: 0,
                    flat: false,
                });
            }
            pos = tab_closer_end;
            last_tab_close = true;
        }
    }

    /// Skip one significant view-body content construct while searching for
    /// the attached closer of a recovering tabview. Regular block heads stay
    /// literal (the consumer's tab view body only forms tabs as structure), so
    /// only their head tokens are skipped; opaque constructs are skipped whole.
    fn skip_tabview_content(&self, pos: usize, limit: usize) -> Option<usize> {
        if let Some(zone_end) = self.zone_end(pos) {
            return Some(zone_end.min(limit));
        }
        match self.tokens[pos].token {
            Token::LeftComment => self.skip_comment(pos),
            Token::LeftBlock | Token::LeftBlockStar => {
                if let Some(candidate) =
                    self.code_candidates.get(&self.tokens[pos].span.start)
                    && self.raw_name_is(pos, "code")
                {
                    return Some(self.token_at_or_after(candidate.owner_end));
                }
                if let Some(segment) = self.segment_at(pos) {
                    return Some(segment.end);
                }
                let Some(head) = self.read_head(pos, false) else {
                    return Some(pos + 1);
                };
                Some(head.head_end)
            }
            _ => Some(pos + 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::Parser;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::SyntaxTree;
    use std::collections::BTreeSet;
    use std::path::PathBuf;
    use std::time::Instant;

    const FINDINGS_TEXT: &str = include_str!("block_pairs_findings.txt");

    fn wikidot_settings() -> WikitextSettings {
        WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot)
    }

    #[test]
    fn block_pairs_wired_into_parser() {
        let settings = wikidot_settings();
        let info = PageInfo::dummy();
        for source in [
            "[[div]]\nA\n[[/div]]",
            "[[collapsible]]\nA\n[[/div]]\nB\n[[/collapsible]]",
            "[[div]]\nA\n[[collapsible]]\nB\n[[/div]]\nC\n[[/collapsible]]\nD",
        ] {
            let tokenization = crate::tokenize(source);
            let parser = Parser::new(&tokenization, &info, &settings);
            let via_cache = parser.wikidot_block_pairs_for_test();
            let direct =
                compute_wikidot_block_pairs(tokenization.tokens(), source, &info);
            assert_eq!(format!("{via_cache:?}"), format!("{direct:?}"), "{source}");
        }
        let native = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tokenization = crate::tokenize("[[div]]\nA\n[[/div]]");
        let parser = Parser::new(&tokenization, &info, &native);
        assert!(parser.wikidot_block_pairs_for_test().is_empty());
    }

    fn gen_ends(events: &[bool], start: usize) -> Vec<usize> {
        fn body(events: &[bool], mut p: usize, out: &mut Vec<usize>) {
            loop {
                if p >= events.len() {
                    return;
                }
                if !events[p] {
                    out.push(p);
                }
                if events[p] {
                    for end in gen_ends(events, p + 1) {
                        body(events, end + 1, out);
                        if out.len() > 4096 {
                            return;
                        }
                    }
                }
                p += 1;
            }
        }
        let mut out = Vec::new();
        body(events, start, &mut out);
        out
    }

    fn naive_pair_nest(events: &[bool]) -> BTreeSet<(usize, usize)> {
        let mut state = vec![0_u8; events.len()];
        let mut pairs = BTreeSet::new();
        loop {
            let Some(head) = (0..events.len()).find(|&k| state[k] == 0 && events[k])
            else {
                break;
            };
            let available: Vec<usize> =
                (0..events.len()).filter(|&k| state[k] == 0).collect();
            let position = available.iter().position(|&k| k == head).unwrap();
            let sub: Vec<bool> = available.iter().map(|&k| events[k]).collect();
            let ends = gen_ends(&sub, position + 1);
            match ends.first() {
                Some(&first) => {
                    let closer = available[first];
                    pairs.insert((head, closer));
                    state[head] = 2;
                    state[closer] = 2;
                }
                None => state[head] = 1,
            }
        }
        pairs
    }

    fn production_pair_nest(events: &[bool]) -> BTreeSet<(usize, usize)> {
        let evs: Vec<Event> = events
            .iter()
            .enumerate()
            .map(|(index, is_head)| Event {
                kind: WikidotBlockKind::Div,
                scored: false,
                head: index,
                head_end: index,
                closer: index,
                closer_end: index,
                is_close: !is_head,
            })
            .collect();
        let refs: Vec<&Event> = evs.iter().collect();
        pair_nest(&refs)
            .into_iter()
            .map(|(h, c)| (evs[h].head, evs[c].closer))
            .collect()
    }

    #[test]
    fn pair_nest_matches_recursive_lazy_matcher() {
        for len in 0..=11_usize {
            for bits in 0..(1_u32 << len) {
                let events: Vec<bool> = (0..len).map(|k| bits & (1 << k) != 0).collect();
                assert_eq!(
                    production_pair_nest(&events),
                    naive_pair_nest(&events),
                    "{events:?}"
                );
            }
        }
        let mut seed = 0x2545_u64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as u32
        };
        for _ in 0..3000 {
            let len = (next() % 15) as usize;
            let events: Vec<bool> = (0..len).map(|_| next() % 2 == 0).collect();
            assert_eq!(
                production_pair_nest(&events),
                naive_pair_nest(&events),
                "{events:?}"
            );
        }
    }

    #[test]
    fn block_pairs_hostile_timing() {
        let info = PageInfo::dummy();
        let cases: Vec<(String, String)> = vec![
            ("unclosed-div-20000".into(), "[[div]]\n".repeat(20000)),
            (
                "alternating-div-10000".into(),
                "[[div]]\n[[/div]]\n".repeat(10000),
            ),
            (
                "nested-div-2000".into(),
                "[[div]]\n".repeat(2000) + &"[[/div]]\n".repeat(2000),
            ),
            (
                "unclosed-collapsible-20000".into(),
                "[[collapsible]]\n".repeat(20000),
            ),
            (
                "crossed-div-collapsible-5000".into(),
                "[[div]]\n[[collapsible]]\n[[/div]]\n[[/collapsible]]\n".repeat(5000),
            ),
        ];
        for (name, source) in &cases {
            let tokenization = crate::tokenize(source);
            let start = Instant::now();
            let pairs = compute_wikidot_block_pairs(tokenization.tokens(), source, &info);
            println!(
                "hostile {name}: {:?} pairs={}",
                start.elapsed(),
                pairs.pairs().len()
            );
        }
    }

    struct Case {
        id: String,
        source: String,
    }

    fn load_cases() -> Vec<Case> {
        let mut cases = Vec::new();
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = manifest.join("test");
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.file_name().is_some_and(|name| name == "input.ftml")
                    && let Ok(source) = std::fs::read_to_string(&path)
                {
                    let relative =
                        path.strip_prefix(&manifest).unwrap_or(&path).display();
                    cases.push(Case {
                        id: format!("corpus-{relative}"),
                        source,
                    });
                }
            }
        }
        for line in include_str!("block_pairs_cases.jsonl").lines() {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            cases.push(Case {
                id: value["case_id"].as_str().unwrap().to_string(),
                source: value["source"].as_str().unwrap().to_string(),
            });
        }
        let battery_files: &[(&str, &str)] = &[
            ("disc3-crossed/cases.jsonl", "crossed"),
            ("disc3-crossed/cases2.jsonl", "crossed2"),
            ("disc3-crossed/controls.jsonl", "crossed-controls"),
            ("disc3-whitespace/all-cases.jsonl", "whitespace"),
            ("disc3-corpus-replay/cases-input.jsonl", "replay"),
        ];
        for (file, label) in battery_files {
            let path = format!("/home/roku/.local/state/wikijump/muse-bhl/{file}");
            let Ok(text) = std::fs::read_to_string(&path) else {
                eprintln!("battery set {path} absent, skipped");
                continue;
            };
            for line in text.lines() {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                    continue;
                };
                if value["layout"]
                    .as_str()
                    .is_some_and(|layout| layout != "wikidot")
                {
                    continue;
                }
                if let (Some(id), Some(source)) =
                    (value["case_id"].as_str(), value["source"].as_str())
                {
                    cases.push(Case {
                        id: format!("{label}-{id}"),
                        source: source.to_string(),
                    });
                }
            }
        }
        cases
    }

    fn tree_counts(tree: &SyntaxTree<'_>) -> BTreeMap<WikidotBlockKind, usize> {
        fn bump(
            counts: &mut BTreeMap<WikidotBlockKind, usize>,
            kind: WikidotBlockKind,
            by: usize,
        ) {
            *counts.entry(kind).or_insert(0) += by;
        }
        fn walk(
            value: &serde_json::Value,
            counts: &mut BTreeMap<WikidotBlockKind, usize>,
        ) {
            match value {
                serde_json::Value::Object(map) => {
                    if let Some(element) = map.get("element").and_then(|e| e.as_str()) {
                        match element {
                            "container" => {
                                if let Some(kind) =
                                    map.get("data").and_then(|d| d.get("type"))
                                {
                                    if let Some(name) = kind.as_str() {
                                        match name {
                                            "div" => {
                                                bump(counts, WikidotBlockKind::Div, 1)
                                            }
                                            "note" => {
                                                bump(counts, WikidotBlockKind::Note, 1)
                                            }
                                            _ => {}
                                        }
                                    } else if let Some(align) =
                                        kind.get("align").and_then(|a| a.as_str())
                                    {
                                        match align {
                                            "center" => bump(
                                                counts,
                                                WikidotBlockKind::AlignCenter,
                                                1,
                                            ),
                                            "right" => bump(
                                                counts,
                                                WikidotBlockKind::AlignRight,
                                                1,
                                            ),
                                            "left" => bump(
                                                counts,
                                                WikidotBlockKind::AlignLeft,
                                                1,
                                            ),
                                            "justify" => bump(
                                                counts,
                                                WikidotBlockKind::AlignJustify,
                                                1,
                                            ),
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            "collapsible" => {
                                bump(counts, WikidotBlockKind::Collapsible, 1)
                            }
                            "tab-view" => {
                                bump(counts, WikidotBlockKind::TabView, 1);
                                if let Some(items) =
                                    map.get("data").and_then(|d| d.as_array())
                                {
                                    // T1: count real tabs only. Failed heads
                                    // form content runs, never Tab pairs.
                                    let tabs = items
                                        .iter()
                                        .filter(|item| item.get("label").is_some())
                                        .count();
                                    bump(counts, WikidotBlockKind::Tab, tabs);
                                }
                            }
                            "table" => {
                                if map
                                    .get("data")
                                    .and_then(|d| d.get("type"))
                                    .and_then(|t| t.as_str())
                                    == Some("advanced")
                                {
                                    bump(counts, WikidotBlockKind::Table, 1);
                                    if let Some(rows) = map
                                        .get("data")
                                        .and_then(|d| d.get("rows"))
                                        .and_then(|r| r.as_array())
                                    {
                                        bump(counts, WikidotBlockKind::Row, rows.len());
                                        for row in rows {
                                            if let Some(cells) = row
                                                .get("cells")
                                                .and_then(|c| c.as_array())
                                            {
                                                bump(
                                                    counts,
                                                    WikidotBlockKind::Cell,
                                                    cells.len(),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    for value in map.values() {
                        walk(value, counts);
                    }
                }
                serde_json::Value::Array(items) => {
                    for item in items {
                        walk(item, counts);
                    }
                }
                _ => {}
            }
        }
        let value = serde_json::to_value(tree).unwrap();
        let mut counts = BTreeMap::new();
        walk(&value, &mut counts);
        counts
    }

    #[test]
    fn block_pairs_differential() {
        std::thread::Builder::new()
            .name("block-pairs-differential".to_owned())
            .stack_size(crate::parsing::DEEP_PARSE_STACK_BYTES)
            .spawn(block_pairs_differential_body)
            .expect("unable to start differential worker")
            .join()
            .expect("differential worker panicked");
    }

    fn block_pairs_differential_body() {
        let findings_set: std::collections::HashSet<(String, String)> = FINDINGS_TEXT
            .lines()
            .filter(|line| !line.is_empty())
            .filter_map(|line| line.split_once('\t'))
            .map(|(case, detail)| (case.to_string(), detail.to_string()))
            .collect();
        let settings = wikidot_settings();
        let info = PageInfo::dummy();
        let cases = load_cases();
        let mut violations: Vec<String> = Vec::new();
        let mut findings: Vec<String> = Vec::new();
        let mut inexact: Vec<String> = Vec::new();
        let mut total_pairs = 0_usize;
        for case in &cases {
            let tokenization = crate::tokenize(&case.source);
            let repaired = crate::parsing::repaired_tokenization_for_test(
                &tokenization,
                &info,
                &settings,
            );
            let effective = repaired.as_ref().unwrap_or(&tokenization);
            let tokens = effective.tokens();
            let scan =
                compute_wikidot_block_pairs(tokens, effective.full_text().inner(), &info);
            recorder_enable();
            let outcome = crate::parsing::parse_for_test(effective, &info, &settings);
            let recorded = recorder_take();
            let (tree, _errors) = outcome.into();
            let counts = tree_counts(&tree);

            let mut descent: BTreeMap<WikidotBlockKind, BTreeSet<(usize, usize)>> =
                BTreeMap::new();
            let mut seen: BTreeSet<(String, usize, usize)> = BTreeSet::new();
            for (name, start, end) in recorded {
                if !seen.insert((name.clone(), start, end)) {
                    continue;
                }
                if let Some(kind) = Scanner::kind_of(&name) {
                    descent.entry(kind).or_default().insert((start, end));
                }
            }
            let scan_bytes: Vec<(WikidotBlockKind, usize, usize, bool)> = scan
                .pairs()
                .iter()
                .map(|pair| {
                    (
                        pair.kind,
                        tokens[pair.head].span.start,
                        tokens[pair.closer_end - 1].span.end,
                        pair.flat,
                    )
                })
                .collect();
            total_pairs += scan_bytes.len();

            const KINDS: [WikidotBlockKind; 12] = [
                WikidotBlockKind::Div,
                WikidotBlockKind::Collapsible,
                WikidotBlockKind::Note,
                WikidotBlockKind::AlignCenter,
                WikidotBlockKind::AlignRight,
                WikidotBlockKind::AlignLeft,
                WikidotBlockKind::AlignJustify,
                WikidotBlockKind::TabView,
                WikidotBlockKind::Tab,
                WikidotBlockKind::Table,
                WikidotBlockKind::Row,
                WikidotBlockKind::Cell,
            ];
            for kind in KINDS {
                let rec = descent.get(&kind).cloned().unwrap_or_default();
                let tree_count = counts.get(&kind).copied().unwrap_or(0);
                let scan_of_kind: Vec<(usize, usize, bool)> = scan_bytes
                    .iter()
                    .filter(|(k, _, _, _)| *k == kind)
                    .map(|(_, s, e, f)| (*s, *e, *f))
                    .collect();
                if matches!(
                    kind,
                    WikidotBlockKind::Row
                        | WikidotBlockKind::Cell
                        | WikidotBlockKind::Tab
                ) && rec.len() != tree_count
                {
                    let parent = match kind {
                        WikidotBlockKind::Tab => WikidotBlockKind::TabView,
                        _ => WikidotBlockKind::Table,
                    };
                    let parents: Vec<(usize, usize)> = scan_bytes
                        .iter()
                        .filter(|(k, _, _, _)| *k == parent)
                        .map(|(_, s, e, _)| (*s, *e))
                        .collect();
                    let contained = scan_of_kind.iter().all(|(s, e, _)| {
                        parents.iter().any(|(ps, pe)| ps <= s && e <= pe)
                    });
                    if scan_of_kind.len() != tree_count || !contained {
                        let detail = format!(
                            "structural {:?} scan={} tree={} contained={}",
                            kind,
                            scan_of_kind.len(),
                            tree_count,
                            contained
                        );
                        if findings_set.contains(&(case.id.clone(), detail.clone())) {
                            findings.push(format!("{}: {detail}", case.id));
                        } else {
                            violations.push(format!("{}: {detail}", case.id));
                        }
                    }
                    continue;
                }
                if rec.len() != tree_count {
                    inexact.push(format!(
                        "{}: {:?} recorder={} tree={}",
                        case.id,
                        kind,
                        rec.len(),
                        tree_count
                    ));
                    continue;
                }
                let ends_match = |a: usize, b: usize| -> bool {
                    if a == b {
                        return true;
                    }
                    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
                    if hi - lo > 2 || lo >= case.source.len().min(hi) {
                        return false;
                    }
                    let between = &case.source[lo..hi];
                    between.contains('\n')
                        && between
                            .bytes()
                            .all(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
                };
                let scan_set: BTreeSet<(usize, usize)> =
                    scan_of_kind.iter().map(|(s, e, _)| (*s, *e)).collect();
                for (s, e, flat) in &scan_of_kind {
                    if !flat
                        && !rec.iter().any(|&(rs, re)| rs == *s && ends_match(*e, re))
                    {
                        let detail = format!("{kind:?}@{s}..{e} scan-only");
                        if findings_set.contains(&(case.id.clone(), detail.clone())) {
                            findings.push(format!("{}: {detail}", case.id));
                        } else {
                            violations.push(format!("{}: {detail}", case.id));
                        }
                    }
                }
                for (s, e) in &rec {
                    if scan_set
                        .iter()
                        .any(|&(ss, se)| ss == *s && ends_match(se, *e))
                    {
                        continue;
                    }
                    let covered = scan_bytes
                        .iter()
                        .any(|(_, fs, fe, flat)| *flat && fs < s && e <= fe);
                    if !covered {
                        let detail = format!("{kind:?}@{s}..{e} descent-only");
                        if findings_set.contains(&(case.id.clone(), detail.clone())) {
                            findings.push(format!("{}: {detail}", case.id));
                        } else {
                            violations.push(format!("{}: {detail}", case.id));
                        }
                    }
                }
            }
        }
        println!(
            "differential: {} cases, {} scan pairs, {} inexact entries, {} findings",
            cases.len(),
            total_pairs,
            inexact.len(),
            findings.len()
        );
        for entry in &inexact {
            println!("inexact: {entry}");
        }
        for entry in &findings {
            println!("finding: {entry}");
        }
        for entry in &violations {
            println!("violation: {entry}");
        }
        assert!(
            violations.is_empty(),
            "{} differential violations (see output above)",
            violations.len()
        );
    }
}
