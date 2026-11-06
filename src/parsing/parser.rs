/*
 * parsing/parser.rs
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

use super::RULE_PAGE;
use super::body_end::BodyEndBoundary;
use super::collect::{SegmentCtx, TokenWindow, split_delimiter_end};
use super::condition::ParseCondition;
use super::hidden_body::HiddenBodyBoundary;
use super::prelude::*;
use super::rule::Rule;
use crate::data::PageInfo;
use crate::delayed::{
    GeneratedInput, elements_contain_delayed, resolve_static_suppressions,
};
use crate::preproc::LiteralRegionIndex;
use crate::render::text::TextRender;
use crate::tokenizer::{DelayedMarker, Tokenization};
use crate::tree::{
    AcceptsPartial, Bibliography, BibliographyList, CodeBlock, HeadingLevel,
};
use std::borrow::Cow;
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::rc::Rc;
use std::{mem, ptr};

pub(crate) mod block_pairs;
pub(crate) mod closer_scan;
pub(crate) mod inline_pairs;
mod wikidot_state;
use self::closer_scan::CloserScanCache;
use self::wikidot_state::WikidotState;

pub(crate) const DEFAULT_MAX_RECURSION_DEPTH: usize = 100;
pub(crate) const DEEP_MAX_RECURSION_DEPTH: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuoteBodyLineStatus {
    Inactive,
    Prepared,
    Boundary,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum QuoteBodyLineState {
    NeedPrefix,
    PreparedContent {
        content_start: usize,
    },
    PreparedResidual {
        token_start: usize,
        stripped_depth: usize,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct QuoteBodyCursor {
    required_depth: usize,
    line_state: QuoteBodyLineState,
    literal_residual_quotes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum QuoteScanOutcome {
    HasCandidateClose,
    Missing,
}

type QuoteScanKey = (&'static str, usize, bool, usize);
type BlockEndScanKey = (&'static str, usize, bool);
type ContextualBlockEndScanKey = (BlockEndScanKey, BlockEndScanContext);
type LostOwnerScanKey = (&'static str, u8, usize, usize, bool);
type PartialRejectionKey = (&'static str, &'static str, usize, u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct BlockEndScanContext {
    discarding_hidden_body: bool,
    in_wikidot_center_body: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RuleAttemptKey {
    rule: &'static str,
    remaining: usize,
    depth: Option<usize>,
    accepts_partial: AcceptsPartial,
    accepts_partial_offset: Option<isize>,
    in_footnote: bool,
    in_note: bool,
    in_color: bool,
    has_footnote_block: bool,
    start_of_line: bool,
    wikidot: WikidotState,
}

/// A body walk at one position: the rule whose closer it looks for, the parser
/// context, and what can end it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct BodyWalkKey {
    attempt: RuleAttemptKey,
    boundaries: u64,
}

#[cfg(test)]
thread_local! {
    static RULE_ATTEMPT_MEMO_DISABLED: Cell<bool> = const { Cell::new(false) };
}

#[derive(Debug, Default)]
pub(super) struct BlockScanCache<'t> {
    peak_depth: Cell<usize>,
    rule_attempt_failures: HashMap<RuleAttemptKey, (ParseError, usize)>,
    rule_attempt_failure_positions: Vec<bool>,
    body_walk_failures: HashMap<BodyWalkKey, usize>,
    body_walk_failure_positions: Vec<bool>,
    body_end_outcomes: BTreeMap<ContextualBlockEndScanKey, bool>,
    wikidot_composite_span_close: BTreeMap<(usize, BlockEndScanContext), bool>,
    #[cfg(test)]
    composite_span_close_scan_token_visits: Cell<usize>,
    underclosed_block_failures: BTreeMap<(&'static str, usize), ()>,
    deterministic_block_failures: BTreeMap<(&'static str, usize), ParseError>,
    partial_rejections: BTreeMap<PartialRejectionKey, ParseError>,
    wikidot_tabview_failures: BTreeMap<(usize, bool), ParseError>,
    wikidot_collapsible_failures: BTreeMap<usize, ParseError>,
    wikidot_div_bounded_span: BTreeMap<usize, bool>,
    wikidot_div_stream_marks: BTreeMap<usize, (u32, u32)>,
    wikidot_block_pairs: block_pairs::WikidotBlockPairs,
    wikidot_line_runs: block_pairs::WikidotLineRuns,
    pub(super) wikidot_li_cross_scan: Option<block_pairs::WikidotLiCrossScan>,
    wikidot_li_body_boundaries: Vec<usize>,
    wikidot_table_cut_boundary: Cell<bool>,
    wikidot_inline_markers: inline_pairs::WikidotInlineMarkers,
    wikidot_heading_barrier: Option<usize>,
    wikidot_heading_reopen: Option<(usize, crate::tree::HeadingReopen<'t>)>,
    lost_owner_outcomes: BTreeMap<LostOwnerScanKey, bool>,
    span_alias_close_scores: BTreeMap<usize, bool>,
    wikidot_scored_join_heads: std::collections::BTreeSet<usize>,
    literal_ranges: Option<Vec<Range<usize>>>,
    wikidot_iftags_body_end: Cell<Option<usize>>,
    pending_iftags_end_swallows: Vec<usize>,
    wikidot_li_paired_heads: std::collections::BTreeSet<usize>,
    wikidot_li_pairing_depth: Cell<usize>,
    wikidot_li_heads: Vec<WikidotLiHead>,
    wikidot_li_closes: Vec<WikidotLiClose>,
    wikidot_li_collect_stack: Vec<Option<usize>>,
    wikidot_explicit_list_ends: std::collections::BTreeSet<usize>,
    gate_items_allowed: Cell<bool>,
    title_footnote_fused_end: Cell<bool>,
    degraded_title_records: Vec<usize>,
    wikidot_footnote_zone_until: Cell<Option<usize>>,
    fused_name_residual: Cell<Option<usize>>,
    head_had_segment: Cell<bool>,
    pub(super) wikidot_end_head_scan: Option<(usize, Option<usize>)>,
    pub(super) wikidot_line_break_scan: Option<(usize, Option<usize>)>,
    pub(super) wikidot_lf_line_bounds: Option<(usize, usize)>,
    wikidot_tab_quote_ejection: Cell<bool>,
    wikidot_body_soft_ended: Cell<bool>,
    /// Set once a quoted tabview's tab body has soft-ended at an unquoted
    /// ejection: every later tab opener in that view lives in a later quote
    /// segment (Wikidot's cross-segment panel split).
    wikidot_tabview_ejected: Cell<bool>,
    /// Ordinal of the next tab parsed inside the current quoted tabview.
    wikidot_tabview_tab_ordinal: Cell<usize>,
    /// Ordinals of tabs whose panels render outside the navset (bare panel in
    /// a later quote segment).
    wikidot_tabview_remote_ordinals: Vec<usize>,
    colon_line_vanished: Cell<bool>,
    line_follows_block: Cell<bool>,
    bare_div_literal: Cell<bool>,
    inline_container_body_names: Vec<&'static str>,
    pub(super) body_end_boundaries: Vec<BodyEndBoundary>,
    quote_escaped_units: Vec<QuoteEscapedUnit<'t>>,
    pub(super) unmatched_close_scan: super::unmatched_close_scan::UnmatchedCloseScanCache,
    /// Monotone scan slot for the Wikidot anchor-label walk's termination
    /// bytes; see `Parser::anchor_label_reaches_input_end`.
    anchor_label_stop: Option<(usize, Option<usize>)>,
    /// Monotone scan slots for the link-walk end-of-input fast paths; see
    /// `Parser::link_target_reaches_input_end` and
    /// `Parser::raw_forbids_link_reaches_input_end`.
    link_target_stop: [Option<(usize, Option<usize>)>; 2],
    raw_forbids_stop: Option<(usize, Option<usize>)>,
    last_virtual_gate_end: Cell<Option<usize>>,
    /// Monotone "no closer ahead" memos for forward closer scans in parser
    /// rules; see `parser/closer_scan.rs`.
    closer_scan: CloserScanCache,
}

#[derive(Debug)]
pub(crate) enum QuoteEscapedUnit<'t> {
    Elements(Vec<Element<'t>>, bool),
    Row(usize, Vec<Element<'t>>, bool),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WikidotLiHead {
    pub(crate) pos: usize,
    pub(crate) depth: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WikidotLiClose {
    pub(crate) pos: usize,
    pub(crate) depth: usize,
    pub(crate) paired: Option<usize>,
    pub(crate) prev_paired: Option<usize>,
}

fn token_starts_line(token: Token) -> bool {
    token == Token::InputStart
        || token == Token::LineBreak
        || token == Token::ParagraphBreak
}

fn is_optional_space_token(token: Token) -> bool {
    token == Token::Whitespace
        || token == Token::LineBreak
        || token == Token::ParagraphBreak
        || token == Token::Equals
}

#[derive(Debug, Clone)]
pub struct Parser<'r, 't> {
    page_info: &'r PageInfo<'t>,
    settings: &'r WikitextSettings,

    current: &'r ExtractedToken<'t>,
    remaining: &'r [ExtractedToken<'t>],
    full_text: FullText<'t>,
    delayed_markers: &'r BTreeMap<usize, DelayedMarker>,

    rule: Rule,
    depth: usize,
    max_recursion_depth: usize,

    pub(super) discarding_hidden_body: bool,
    pub(super) hidden_body_boundaries: Vec<HiddenBodyBoundary>,

    table_of_contents: Rc<RefCell<Vec<(usize, String)>>>,

    html_blocks: Rc<RefCell<Vec<Cow<'t, str>>>>,

    code_blocks: Rc<RefCell<Vec<CodeBlock<'t>>>>,

    footnotes: Rc<RefCell<Vec<Vec<Element<'t>>>>>,

    bibliographies: Rc<RefCell<BibliographyList<'t>>>,

    quote_scan_cache: Rc<RefCell<BTreeMap<QuoteScanKey, QuoteScanOutcome>>>,
    pub(super) block_end_scan_cache: Rc<RefCell<BlockScanCache<'t>>>,
    two_block_end_scan_cache: Rc<RefCell<BTreeMap<ContextualBlockEndScanKey, u8>>>,
    single_link_label_owner_scan_cache: Rc<RefCell<BTreeMap<usize, bool>>>,
    wikidot_code_candidate_cache:
        Rc<RefCell<Option<BTreeMap<usize, crate::wikidot_code::WikidotCodeCandidate>>>>,
    #[cfg(test)]
    quote_scan_token_visits: Rc<Cell<usize>>,
    #[cfg(test)]
    underline_fast_path_visits: Rc<Cell<usize>>,
    #[cfg(test)]
    block_end_scan_token_visits: Rc<Cell<usize>>,

    accepts_partial: AcceptsPartial,
    accepts_partial_depth: u16,
    in_footnote: bool,
    in_note: bool,
    in_color: bool,
    has_footnote_block: bool,
    start_of_line: bool,
    native_blockquote_depth: Option<usize>,
    quote_body_cursor: Option<QuoteBodyCursor>,
    wikidot: WikidotState,
}

pub(crate) fn wikidot_own_line_prefix(source: &str, byte: usize) -> bool {
    let end = byte.min(source.len());
    let start = source[..end].rfind('\n').map_or(0, |offset| offset + 1);
    source[start..end].trim_matches([' ', '\t']).is_empty()
}

/// Own line inside a quote run: only quote markers and spacing precede the
/// byte (`> [[/div]]` counts, `> text [[/div]]` does not).
fn wikidot_quoted_own_line_prefix(source: &str, byte: usize) -> bool {
    let end = byte.min(source.len());
    let start = source[..end].rfind('\n').map_or(0, |offset| offset + 1);
    source[start..end]
        .chars()
        .all(|character| matches!(character, '>' | ' ' | '\t'))
}

fn wikidot_closer_ends_line(source: &str, byte: usize) -> bool {
    let start = byte.min(source.len());
    let end = source[start..]
        .find('\n')
        .map_or(source.len(), |offset| start + offset);
    source[start..end].trim_end().ends_with("]]")
}

impl<'r, 't> Parser<'r, 't> {
    pub(crate) fn new(
        tokenization: &'r Tokenization<'t>,
        page_info: &'r PageInfo<'t>,
        settings: &'r WikitextSettings,
    ) -> Self {
        Self::new_with_recursion_limit(
            tokenization,
            page_info,
            settings,
            DEFAULT_MAX_RECURSION_DEPTH,
        )
    }

    pub(crate) fn new_with_recursion_limit(
        tokenization: &'r Tokenization<'t>,
        page_info: &'r PageInfo<'t>,
        settings: &'r WikitextSettings,
        max_recursion_depth: usize,
    ) -> Self {
        let full_text = tokenization.full_text();
        let (current, remaining) = tokenization
            .tokens()
            .split_first()
            .expect("Parsed tokens list was empty (expected at least one element)");

        Parser {
            page_info,
            settings,
            current,
            remaining,
            full_text,
            delayed_markers: tokenization.delayed_markers(),
            rule: RULE_PAGE,
            depth: 0,
            max_recursion_depth,
            discarding_hidden_body: false,
            hidden_body_boundaries: Vec::new(),
            table_of_contents: make_shared_vec(),
            html_blocks: make_shared_vec(),
            code_blocks: make_shared_vec(),
            footnotes: make_shared_vec(),
            bibliographies: Rc::new(RefCell::new(BibliographyList::new())),
            quote_scan_cache: Rc::new(RefCell::new(BTreeMap::new())),
            block_end_scan_cache: Rc::new(RefCell::new({
                let wikidot_block_pairs = if settings.layout.legacy() {
                    block_pairs::compute_wikidot_block_pairs(
                        tokenization.tokens(),
                        tokenization.full_text().inner(),
                        page_info,
                    )
                } else {
                    block_pairs::WikidotBlockPairs::default()
                };
                let wikidot_inline_markers = if settings.layout.legacy() {
                    inline_pairs::compute_wikidot_inline_markers(
                        tokenization.tokens(),
                        &wikidot_block_pairs,
                    )
                } else {
                    inline_pairs::WikidotInlineMarkers::default()
                };
                BlockScanCache {
                    wikidot_div_stream_marks: if settings.layout.legacy() {
                        wikidot_state::compute_wikidot_div_stream_marks(
                            tokenization.tokens(),
                        )
                    } else {
                        BTreeMap::new()
                    },
                    wikidot_block_pairs,
                    wikidot_inline_markers,
                    closer_scan: CloserScanCache::new(tokenization.tokens().len()),
                    wikidot_line_runs: if settings.layout.legacy() {
                        block_pairs::compute_wikidot_line_runs(
                            tokenization.full_text().inner(),
                        )
                    } else {
                        block_pairs::WikidotLineRuns::default()
                    },
                    ..Default::default()
                }
            })),
            two_block_end_scan_cache: Rc::new(RefCell::new(BTreeMap::new())),
            single_link_label_owner_scan_cache: Rc::new(RefCell::new(BTreeMap::new())),
            wikidot_code_candidate_cache: Rc::new(RefCell::new(None)),
            #[cfg(test)]
            quote_scan_token_visits: Rc::new(Cell::new(0)),
            #[cfg(test)]
            underline_fast_path_visits: Rc::new(Cell::new(0)),
            #[cfg(test)]
            block_end_scan_token_visits: Rc::new(Cell::new(0)),
            accepts_partial: AcceptsPartial::None,
            accepts_partial_depth: 0,
            in_footnote: false,
            in_note: false,
            in_color: false,
            has_footnote_block: false,
            start_of_line: true,
            native_blockquote_depth: None,
            quote_body_cursor: None,
            wikidot: WikidotState::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn wikidot_block_pairs_for_test(&self) -> block_pairs::WikidotBlockPairs {
        self.block_end_scan_cache
            .borrow()
            .wikidot_block_pairs
            .clone()
    }

    pub(crate) fn wikidot_inline_marker_role(
        &self,
    ) -> Option<inline_pairs::InlineMarkerRole> {
        let current = self.current();
        if !inline_pairs::is_marker(current.token) {
            return None;
        }
        self.block_end_scan_cache
            .borrow()
            .wikidot_inline_markers
            .role_at_byte(current.span.start)
            .cloned()
    }

    pub(crate) fn wikidot_block_pair_at_head_byte(
        &self,
        head_byte: usize,
    ) -> Option<block_pairs::WikidotBlockPair> {
        self.block_end_scan_cache
            .borrow()
            .wikidot_block_pairs
            .pair_at_head_byte(head_byte)
            .copied()
    }

    pub(crate) fn wikidot_block_pair_at_closer_byte(
        &self,
        closer_byte: usize,
    ) -> Option<block_pairs::WikidotBlockPair> {
        self.block_end_scan_cache
            .borrow()
            .wikidot_block_pairs
            .pair_at_closer_byte(closer_byte)
            .copied()
    }

    pub(crate) fn wikidot_block_pairs_closed_between(
        &self,
        from_byte: usize,
        to_byte: usize,
    ) -> Vec<(usize, block_pairs::WikidotBlockPair)> {
        self.block_end_scan_cache
            .borrow()
            .wikidot_block_pairs
            .closers_between(from_byte, to_byte)
            .map(|(byte, pair)| (byte, *pair))
            .collect()
    }

    pub(crate) fn wikidot_greedy_tail_within(&self, from: usize, to: usize) -> bool {
        self.block_end_scan_cache
            .borrow()
            .wikidot_block_pairs
            .has_greedy_tail_within(from, to)
    }

    // INLINE-L86-HEADING-METHODS-START
    /// The reopen state a heading line leaves for a collapsible head that ends
    /// it: the head byte and the heading to reopen inside the shell frames.
    pub(crate) fn set_wikidot_heading_reopen(
        &self,
        value: Option<(usize, crate::tree::HeadingReopen<'t>)>,
    ) {
        self.block_end_scan_cache
            .borrow_mut()
            .wikidot_heading_reopen = value;
    }

    pub(crate) fn take_wikidot_heading_reopen(
        &self,
    ) -> Option<(usize, crate::tree::HeadingReopen<'t>)> {
        self.block_end_scan_cache
            .borrow_mut()
            .wikidot_heading_reopen
            .take()
    }

    pub(crate) fn table_of_contents_len(&self) -> usize {
        self.table_of_contents.borrow().len()
    }

    /// While a Wikidot heading line is being split at a flat block closer,
    /// this holds the closer byte so nested collections stop there too.
    pub(crate) fn set_wikidot_heading_barrier(&self, value: Option<usize>) {
        self.block_end_scan_cache
            .borrow_mut()
            .wikidot_heading_barrier = value;
    }

    pub(crate) fn wikidot_heading_barrier(&self) -> Option<usize> {
        self.block_end_scan_cache.borrow().wikidot_heading_barrier
    }

    /// The byte range of the heading line containing the byte, if any.
    pub(crate) fn wikidot_heading_line_range(
        &self,
        byte: usize,
    ) -> Option<(usize, usize)> {
        self.block_end_scan_cache
            .borrow()
            .wikidot_block_pairs
            .heading_line_range(byte)
    }

    /// Whether the byte is on a heading line (the scan's definition).
    pub(crate) fn wikidot_byte_on_heading_line(&self, byte: usize) -> bool {
        self.wikidot_heading_line_range(byte).is_some()
    }

    /// Flat closers at or after the byte on the same heading line, as
    /// (closer start, closer end, kind), in source order.
    pub(crate) fn wikidot_heading_flat_closers(
        &self,
        from_byte: usize,
    ) -> Vec<(usize, usize, block_pairs::WikidotBlockKind)> {
        let cache = self.block_end_scan_cache.borrow();
        let pairs = &cache.wikidot_block_pairs;
        let Some((line_start, line_end)) = pairs.heading_line_range(from_byte) else {
            return Vec::new();
        };
        let closers: Vec<(usize, usize, block_pairs::WikidotBlockKind)> = pairs
            .closers_between(from_byte, line_end)
            .filter(|(_, pair)| pair.head_byte < line_start)
            .filter(|(_, pair)| self.wikidot_pair_goes_flat(pair))
            .filter_map(|(closer_byte, pair)| {
                pairs
                    .closer_end_byte(closer_byte)
                    .map(|end| (closer_byte, end, pair.kind))
            })
            .collect();
        closers
    }

    /// The closer byte of the pair whose head is at the byte, when that pair's
    /// closer sits on the same heading line as its head (a collapsible opener
    /// and closer both inside a heading line).
    pub(crate) fn wikidot_heading_pair_same_line_closer(
        &self,
        head_byte: usize,
    ) -> Option<usize> {
        let cache = self.block_end_scan_cache.borrow();
        let pairs = &cache.wikidot_block_pairs;
        let pair = pairs.pair_at_head_byte(head_byte)?;
        let (line_start, line_end) = pairs.heading_line_range(head_byte)?;
        (line_start <= pair.closer_byte && pair.closer_byte < line_end)
            .then_some(pair.closer_byte)
    }

    /// The leftmost collapsible head on the heading line at or after the byte.
    pub(crate) fn wikidot_heading_collapsible_head(
        &self,
        from_byte: usize,
    ) -> Option<usize> {
        let cache = self.block_end_scan_cache.borrow();
        let pairs = &cache.wikidot_block_pairs;
        let (_, line_end) = pairs.heading_line_range(from_byte)?;
        pairs
            .heads_between(from_byte, line_end)
            .find(|pair| pair.kind == block_pairs::WikidotBlockKind::Collapsible)
            .map(|pair| pair.head_byte)
    }

    pub(crate) fn wikidot_scan_shows_stolen_head(
        &self,
        kind: block_pairs::WikidotBlockKind,
        head_byte: usize,
    ) -> bool {
        self.block_end_scan_cache
            .borrow()
            .wikidot_block_pairs
            .shows_stolen_head(kind, head_byte)
    }

    pub(crate) fn wikidot_close_is_foster(&self, head_byte: usize) -> bool {
        self.block_end_scan_cache
            .borrow()
            .wikidot_block_pairs
            .is_foster_head(head_byte)
    }

    #[inline]
    pub(crate) fn wikidot_iftags_body_end(&self) -> Option<usize> {
        self.block_end_scan_cache
            .borrow()
            .wikidot_iftags_body_end
            .get()
    }

    #[inline]
    pub(crate) fn set_wikidot_iftags_body_end(&self, value: Option<usize>) {
        self.block_end_scan_cache
            .borrow()
            .wikidot_iftags_body_end
            .set(value);
    }

    pub(crate) fn push_pending_iftags_end_swallow(&self, position: usize) {
        self.block_end_scan_cache
            .borrow_mut()
            .pending_iftags_end_swallows
            .push(position);
    }

    /// Whether an iftags end swallow is pending at `position`, without
    /// consuming it. The pending list is empty for pages without iftags, and
    /// the caller scans the rest of the line on a clone before it would learn
    /// that, which is quadratic for repeated unmatched closers on one line.
    pub(crate) fn has_pending_iftags_end_swallow(&self, position: usize) -> bool {
        self.block_end_scan_cache
            .borrow()
            .pending_iftags_end_swallows
            .contains(&position)
    }

    /// L85: whether a suspended iftags gate still has its designated end
    /// ahead of the current position (the post-list text belongs to its
    /// body even though the gate's boundary is not on the stack).
    pub(crate) fn has_pending_iftags_end_ahead(&self) -> bool {
        let here = self.current().span.start;
        self.block_end_scan_cache
            .borrow()
            .pending_iftags_end_swallows
            .iter()
            .any(|pending| *pending > here)
    }

    pub(crate) fn take_pending_iftags_end_swallow(&self, position: usize) -> bool {
        let mut cache = self.block_end_scan_cache.borrow_mut();
        match cache
            .pending_iftags_end_swallows
            .iter()
            .position(|pending| *pending == position)
        {
            Some(index) => {
                cache.pending_iftags_end_swallows.remove(index);
                true
            }
            None => false,
        }
    }

    pub(crate) fn set_wikidot_li_paired_heads(
        &self,
        heads: std::collections::BTreeSet<usize>,
    ) {
        self.block_end_scan_cache
            .borrow_mut()
            .wikidot_li_paired_heads = heads;
    }

    pub(crate) fn wikidot_li_head_paired(&self, position: usize) -> bool {
        self.block_end_scan_cache
            .borrow()
            .wikidot_li_paired_heads
            .contains(&position)
    }

    pub(crate) fn record_wikidot_explicit_list_end(&self, position: usize) {
        self.block_end_scan_cache
            .borrow_mut()
            .wikidot_explicit_list_ends
            .insert(position);
    }

    pub(crate) fn wikidot_line_follows_explicit_list(&self, position: usize) -> bool {
        let cache = self.block_end_scan_cache.borrow();
        let text = self.full_text().inner();
        cache.wikidot_explicit_list_ends.iter().any(|end| {
            *end < position && matches!(text.get(*end..position), Some("\n" | "\r\n"))
        })
    }

    pub(crate) fn wikidot_li_pairing_depth(&self) -> usize {
        self.block_end_scan_cache
            .borrow()
            .wikidot_li_pairing_depth
            .get()
    }

    pub(crate) fn set_wikidot_li_pairing_depth(&self, depth: usize) {
        self.block_end_scan_cache
            .borrow()
            .wikidot_li_pairing_depth
            .set(depth);
    }

    pub(crate) fn set_wikidot_li_stream(
        &self,
        heads: Vec<WikidotLiHead>,
        closes: Vec<WikidotLiClose>,
    ) {
        let mut cache = self.block_end_scan_cache.borrow_mut();
        let mut boundaries: Vec<usize> =
            cache.wikidot_li_paired_heads.iter().copied().collect();
        for close in &closes {
            let Some(pair) = close.paired else {
                continue;
            };
            let pair_depth = heads
                .iter()
                .find(|head| head.pos == pair)
                .map(|head| head.depth);
            if pair_depth.is_some_and(|depth| depth >= close.depth) {
                boundaries.push(close.pos);
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        cache.wikidot_li_body_boundaries = boundaries;
        cache.wikidot_li_heads = heads;
        cache.wikidot_li_closes = closes;
    }

    pub(crate) fn wikidot_head_on_table_row_line(&self, head_byte: usize) -> bool {
        if !self.settings().layout.legacy() {
            return false;
        }
        self.block_end_scan_cache
            .borrow()
            .wikidot_line_runs
            .on_table_row_line(head_byte)
    }

    pub(crate) fn wikidot_byte_on_list_item_line(&self, byte: usize) -> bool {
        if !self.settings().layout.legacy() {
            return false;
        }
        self.block_end_scan_cache
            .borrow()
            .wikidot_line_runs
            .on_list_item_line(byte)
    }

    pub(crate) fn wikidot_byte_in_quote_run(&self, byte: usize) -> bool {
        if !self.settings().layout.legacy() {
            return false;
        }
        self.block_end_scan_cache
            .borrow()
            .wikidot_line_runs
            .quote_run_id(byte)
            .is_some()
    }

    pub(crate) fn clear_wikidot_table_cut_boundary(&self) {
        self.block_end_scan_cache
            .borrow()
            .wikidot_table_cut_boundary
            .set(false);
    }

    pub(crate) fn set_wikidot_table_cut_boundary(&self) {
        self.block_end_scan_cache
            .borrow()
            .wikidot_table_cut_boundary
            .set(true);
    }

    pub(crate) fn take_wikidot_table_cut_boundary(&self) -> bool {
        self.block_end_scan_cache
            .borrow()
            .wikidot_table_cut_boundary
            .take()
    }

    pub(crate) fn wikidot_flat_close_splits_quote_run(&self, head_byte: usize) -> bool {
        if !self.settings().layout.legacy() {
            return false;
        }
        let cache = self.block_end_scan_cache.borrow();
        let runs = &cache.wikidot_line_runs;
        runs.quote_run_id(head_byte).is_none()
            && !runs.on_list_item_line(head_byte)
            && !runs.on_table_row_line(head_byte)
    }

    pub(crate) fn wikidot_li_items_at_depth(
        &self,
        depth: usize,
    ) -> (Vec<usize>, Vec<usize>) {
        let cache = self.block_end_scan_cache.borrow();
        let heads: Vec<usize> = cache
            .wikidot_li_heads
            .iter()
            .filter(|head| {
                head.depth == depth && cache.wikidot_li_paired_heads.contains(&head.pos)
            })
            .map(|head| head.pos)
            .collect();
        let closes: Vec<usize> = cache
            .wikidot_li_closes
            .iter()
            .filter(|close| close.depth == depth && close.paired.is_some())
            .map(|close| close.pos)
            .collect();
        (heads, closes)
    }

    pub(crate) fn wikidot_li_straddles_bodies(
        &self,
        first: usize,
        second: usize,
    ) -> bool {
        let (lo, hi) = if first < second {
            (first, second)
        } else if first > second {
            (second, first)
        } else {
            return false;
        };
        let cache = self.block_end_scan_cache.borrow();
        let boundaries = &cache.wikidot_li_body_boundaries;
        let index = boundaries.partition_point(|pos| *pos <= lo);
        boundaries.get(index).is_some_and(|pos| *pos < hi)
    }

    pub(crate) fn wikidot_li_cross_head_in_region(&self, byte: usize) -> bool {
        self.block_end_scan_cache
            .borrow()
            .wikidot_li_cross_scan
            .as_ref()
            .is_some_and(|scan| byte > scan.region_start && byte < scan.region_end)
    }

    pub(crate) fn wikidot_li_cross_pair_crossed(&self, closer_byte: usize) -> bool {
        self.block_end_scan_cache
            .borrow()
            .wikidot_li_cross_scan
            .as_ref()
            .is_some_and(|scan| {
                scan.divcoll_pairs
                    .get(&closer_byte)
                    .is_some_and(|(_, crossed)| *crossed)
            })
    }

    pub(crate) fn wikidot_pair_goes_flat(
        &self,
        pair: &block_pairs::WikidotBlockPair,
    ) -> bool {
        if !pair.kind.emits_flat() {
            return false;
        }
        // L85: pairs crossed through explicit-list structure nest under the
        // cross scan; they are never flat (l-ul-li-*).
        if self.wikidot_li_cross_pair_crossed(pair.closer_byte) {
            return false;
        }
        // L85: a head inside the scanned list region whose closer falls past
        // the region end terminates at `[[/ul]]` instead (closeout).
        if self.wikidot_li_cross_head_in_region(pair.head_byte)
            && !self.wikidot_li_cross_head_in_region(pair.closer_byte)
            && pair.closer_byte > pair.head_byte
        {
            return false;
        }
        if pair.flat {
            return true;
        }
        if !self.settings().layout.legacy() {
            return false;
        }
        let cache = self.block_end_scan_cache.borrow();
        let runs = &cache.wikidot_line_runs;
        if runs.on_list_item_line(pair.head_byte) {
            return true;
        }
        // L85: a quote crossing is flat when only one end sits in a quote run
        // and the closer side keeps live's shapes:
        // - head quoted, closer outside (q-open-in-quote, q-cross-pair);
        // - closer quoted on its own line (q-close-in-quote) or with
        //   same-line content after it (q-close-in-quote-mid).
        // A closer that merely ends a quoted content line keeps the recursive
        // path, because live keeps that line's break
        // (wikidot_div_closes_at_closer_on_quote_line). Pairs entirely inside
        // one quote run are flat only through the scan's crossing set
        // (q-both-in-quote-cross).
        let head_run = runs.quote_run_id(pair.head_byte);
        let closer_run = runs.quote_run_id(pair.closer_byte);
        if head_run.is_some() && closer_run.is_none() {
            return true;
        }
        if head_run.is_none() && closer_run.is_some() {
            let source = self.full_text().inner();
            if wikidot_quoted_own_line_prefix(source, pair.closer_byte)
                || !wikidot_closer_ends_line(source, pair.closer_byte)
            {
                return true;
            }
        }
        if runs.on_table_row_line(pair.head_byte) {
            let line = runs.line_of(pair.head_byte);
            let line_start = runs.line_start(line);
            if !cache
                .wikidot_block_pairs
                .has_closer_byte_in_range(line_start, pair.head_byte)
            {
                return true;
            }
        }
        let boundaries = &cache.wikidot_li_body_boundaries;
        let (lo, hi) = if pair.head_byte < pair.closer_byte {
            (pair.head_byte, pair.closer_byte)
        } else {
            (pair.closer_byte, pair.head_byte)
        };
        let index = boundaries.partition_point(|pos| *pos <= lo);
        boundaries.get(index).is_some_and(|pos| *pos < hi)
    }

    pub(crate) fn wikidot_li_close_record(
        &self,
        position: usize,
    ) -> Option<WikidotLiClose> {
        let cache = self.block_end_scan_cache.borrow();
        cache
            .wikidot_li_closes
            .binary_search_by_key(&position, |close| close.pos)
            .ok()
            .map(|index| cache.wikidot_li_closes[index])
    }

    pub(crate) fn wikidot_li_head_depth(&self, position: usize) -> Option<usize> {
        let cache = self.block_end_scan_cache.borrow();
        cache
            .wikidot_li_heads
            .binary_search_by_key(&position, |head| head.pos)
            .ok()
            .map(|index| cache.wikidot_li_heads[index].depth)
    }

    pub(crate) fn wikidot_li_last_paired_close(&self) -> Option<usize> {
        self.block_end_scan_cache
            .borrow()
            .wikidot_li_closes
            .iter()
            .rev()
            .find_map(|close| close.paired.map(|_| close.pos))
    }

    pub(crate) fn wikidot_li_head_between(
        &self,
        lower: usize,
        upper: usize,
        depth: usize,
    ) -> bool {
        let cache = self.block_end_scan_cache.borrow();
        let start = cache
            .wikidot_li_heads
            .partition_point(|head| head.pos <= lower);
        cache.wikidot_li_heads[start..]
            .iter()
            .take_while(|head| head.pos < upper)
            .any(|head| head.depth == depth)
    }

    pub(crate) fn wikidot_li_paired_head_after(
        &self,
        position: usize,
        depth: usize,
    ) -> bool {
        let cache = self.block_end_scan_cache.borrow();
        let start = cache
            .wikidot_li_heads
            .partition_point(|head| head.pos <= position);
        cache.wikidot_li_heads[start..].iter().any(|head| {
            head.depth == depth && cache.wikidot_li_paired_heads.contains(&head.pos)
        })
    }

    pub(crate) fn push_wikidot_li_collect(&self, head: Option<usize>) {
        self.block_end_scan_cache
            .borrow_mut()
            .wikidot_li_collect_stack
            .push(head);
    }

    pub(crate) fn pop_wikidot_li_collect(&self) {
        self.block_end_scan_cache
            .borrow_mut()
            .wikidot_li_collect_stack
            .pop();
    }

    pub(crate) fn wikidot_li_collect_top(&self) -> Option<Option<usize>> {
        self.block_end_scan_cache
            .borrow()
            .wikidot_li_collect_stack
            .last()
            .copied()
    }

    pub(crate) fn fused_name_residual(&self) -> Option<usize> {
        self.block_end_scan_cache.borrow().fused_name_residual.get()
    }

    pub(crate) fn note_fused_name_residual(&self, position: usize) {
        self.block_end_scan_cache
            .borrow()
            .fused_name_residual
            .set(Some(position));
    }

    pub(crate) fn clear_fused_name_residual(&self) {
        self.block_end_scan_cache
            .borrow()
            .fused_name_residual
            .set(None);
    }

    pub(crate) fn head_had_segment(&self) -> bool {
        self.block_end_scan_cache.borrow().head_had_segment.get()
    }

    pub(crate) fn set_head_had_segment(&self, value: bool) {
        self.block_end_scan_cache
            .borrow()
            .head_had_segment
            .set(value);
    }

    #[inline]
    pub(crate) fn gate_items_allowed(&self) -> bool {
        self.block_end_scan_cache.borrow().gate_items_allowed.get()
    }

    #[inline]
    pub(crate) fn set_gate_items_allowed(&self, value: bool) {
        self.block_end_scan_cache
            .borrow()
            .gate_items_allowed
            .set(value);
    }

    #[inline]
    pub(crate) fn title_footnote_fused_end(&self) -> bool {
        self.block_end_scan_cache
            .borrow()
            .title_footnote_fused_end
            .get()
    }

    #[inline]
    pub(crate) fn set_title_footnote_fused_end(&self, value: bool) {
        self.block_end_scan_cache
            .borrow()
            .title_footnote_fused_end
            .set(value);
    }

    pub(crate) fn record_degraded_title(&self, position: usize) {
        let mut cache = self.block_end_scan_cache.borrow_mut();
        if !cache.degraded_title_records.contains(&position) {
            cache.degraded_title_records.push(position);
        }
    }

    pub(crate) fn degraded_title_record_count(&self) -> usize {
        self.block_end_scan_cache
            .borrow()
            .degraded_title_records
            .len()
    }

    pub(crate) fn truncate_degraded_title_records(&self, len: usize) {
        self.block_end_scan_cache
            .borrow_mut()
            .degraded_title_records
            .truncate(len);
    }

    #[inline]
    pub(crate) fn wikidot_footnote_zone_until(&self) -> Option<usize> {
        self.block_end_scan_cache
            .borrow()
            .wikidot_footnote_zone_until
            .get()
    }

    #[inline]
    pub(crate) fn set_wikidot_footnote_zone_until(&self, value: Option<usize>) {
        self.block_end_scan_cache
            .borrow()
            .wikidot_footnote_zone_until
            .set(value);
    }

    pub(crate) fn wikidot_footnote_zone_blocks(&self, offset: usize) -> bool {
        self.settings().layout.legacy()
            && self.in_footnote()
            && matches!(
                self.wikidot_footnote_zone_until(),
                Some(until) if offset < until
            )
    }

    #[inline]
    pub(crate) fn push_inline_container_body(&self, name: &'static str) -> usize {
        let mut cache = self.block_end_scan_cache.borrow_mut();
        let len = cache.inline_container_body_names.len();
        cache.inline_container_body_names.push(name);
        len
    }

    #[inline]
    pub(crate) fn restore_inline_container_body(&self, len: usize) {
        self.block_end_scan_cache
            .borrow_mut()
            .inline_container_body_names
            .truncate(len);
    }

    #[inline]
    pub(crate) fn inline_container_body_depth(&self, name: &'static str) -> usize {
        self.block_end_scan_cache
            .borrow()
            .inline_container_body_names
            .iter()
            .filter(|&&recorded| recorded == name)
            .count()
    }

    pub(crate) fn block_head_name_here(&self) -> Option<String> {
        if self.current.token != Token::LeftBlock {
            return None;
        }
        let remaining = self.remaining();
        let mut scan = 0;
        if remaining
            .first()
            .is_some_and(|token| token.token == Token::Whitespace)
        {
            scan = 1;
        }
        let mut name = String::new();
        loop {
            let token = remaining.get(scan)?;
            match token.token {
                Token::Whitespace | Token::RightBlock => break,
                Token::LineBreak
                | Token::ParagraphBreak
                | Token::InputEnd
                | Token::LeftBlock
                | Token::LeftBlockEnd => return None,
                _ => name.push_str(token.slice),
            }
            scan += 1;
        }
        if name.is_empty() {
            return None;
        }
        Some(name.strip_suffix('_').unwrap_or(&name).to_ascii_lowercase())
    }

    pub(crate) fn take_block_head_text(
        &mut self,
    ) -> Result<Option<Element<'t>>, ParseError> {
        if self.current.token != Token::LeftBlock {
            return Ok(None);
        }
        let remaining = self.remaining();
        let mut scan = 1;
        let head_end = loop {
            let Some(token) = remaining.get(scan - 1) else {
                return Ok(None);
            };
            match token.token {
                Token::RightBlock => break token.span.end,
                Token::LineBreak
                | Token::ParagraphBreak
                | Token::InputEnd
                | Token::LeftBlock
                | Token::LeftBlockEnd => return Ok(None),
                _ => {}
            }
            scan += 1;
        };
        let text = &self.full_text().inner()[self.current.span.start..head_end];
        for _ in 0..=scan {
            self.step()?;
        }
        Ok(Some(Element::Text(Cow::Borrowed(text))))
    }

    #[inline]
    pub(crate) fn inside_inline_container_body(&self) -> bool {
        !self
            .block_end_scan_cache
            .borrow()
            .inline_container_body_names
            .is_empty()
    }

    #[inline]
    pub(crate) fn in_quote_body(&self) -> bool {
        self.quote_body_cursor.is_some()
    }

    #[inline]
    pub fn page_info(&self) -> &PageInfo<'t> {
        self.page_info
    }

    #[inline]
    pub fn settings(&self) -> &WikitextSettings {
        self.settings
    }

    #[inline]
    pub fn full_text(&self) -> FullText<'t> {
        self.full_text
    }

    #[inline]
    pub(crate) fn current_generated(&self) -> Option<&GeneratedInput> {
        match self.current.token {
            Token::GeneratedPageLink | Token::GeneratedTagLinks => self
                .delayed_markers
                .get(&self.current.span.start)
                .and_then(DelayedMarker::generated),
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn generated_for(
        &self,
        token: &ExtractedToken<'_>,
    ) -> Option<&GeneratedInput> {
        match token.token {
            Token::GeneratedPageLink | Token::GeneratedTagLinks => self
                .delayed_markers
                .get(&token.span.start)
                .and_then(DelayedMarker::generated),
            _ => None,
        }
    }

    pub(crate) fn generated_until_right_block(&self) -> Vec<GeneratedInput> {
        std::iter::once(self.current())
            .chain(self.remaining())
            .take_while(|token| {
                !matches!(
                    token.token,
                    Token::RightBlock
                        | Token::LineBreak
                        | Token::ParagraphBreak
                        | Token::InputEnd
                )
            })
            .filter_map(|token| self.generated_for(token).cloned())
            .collect()
    }

    pub(crate) fn has_generated_in_range(&self, range: Range<usize>) -> bool {
        self.delayed_markers
            .range(range)
            .any(|(_, marker)| marker.generated().is_some())
    }

    pub(crate) fn generated_in_range(&self, range: Range<usize>) -> Vec<GeneratedInput> {
        self.delayed_markers
            .range(range)
            .filter_map(|(_, marker)| marker.generated().cloned())
            .collect()
    }

    pub(crate) fn has_runtime_literal_in_range(&self, range: Range<usize>) -> bool {
        self.delayed_markers
            .range(range.start..range.end)
            .any(|(_, marker)| marker.runtime_literal().is_some())
            || self
                .delayed_markers
                .range(..range.start)
                .next_back()
                .and_then(|(_, marker)| marker.runtime_literal())
                .is_some_and(|literal| literal.end > range.start)
    }

    pub(crate) fn has_runtime_scalar_in_range(&self, range: Range<usize>) -> bool {
        self.delayed_markers
            .range(range.start..range.end)
            .any(|(_, marker)| marker.runtime_scalar().is_some())
            || self
                .delayed_markers
                .range(..range.start)
                .next_back()
                .and_then(|(_, marker)| marker.runtime_scalar())
                .is_some_and(|scalar| scalar.end > range.start)
    }

    #[inline]
    pub fn rule(&self) -> Rule {
        self.rule
    }

    #[inline]
    pub fn accepts_partial(&self) -> AcceptsPartial {
        self.accepts_partial
    }

    #[inline]
    pub(crate) fn accepts_partial_here(&self, value: AcceptsPartial) -> bool {
        self.accepts_partial == value
            && self.accepts_partial_depth != 0
            && usize::from(self.accepts_partial_depth) == self.depth
    }

    #[inline]
    pub fn in_note(&self) -> bool {
        self.in_note
    }

    #[inline]
    pub fn set_note_flag(&mut self, value: bool) {
        self.in_note = value;
    }

    #[inline]
    pub fn in_color(&self) -> bool {
        self.in_color
    }

    #[inline]
    pub fn set_color_flag(&mut self, value: bool) {
        self.in_color = value;
    }

    #[inline]
    pub fn in_footnote(&self) -> bool {
        self.in_footnote
    }

    #[inline]
    pub fn has_footnote_block(&self) -> bool {
        self.has_footnote_block
    }

    #[inline]
    pub fn start_of_line(&self) -> bool {
        self.start_of_line
    }

    /// Whether the current token is the first non-blank content on its
    /// physical source line.
    ///
    /// `start_of_line` tracks only break tokens in the parser's stream, so a
    /// consumed construct (an elided comment, a kept `[[iftags]]` body) can
    /// hide a physical line start that live Wikidot still honors. Rules that
    /// re-detect line-leading syntax from inside a collect use this instead.
    pub(crate) fn current_at_source_line_start(&self) -> bool {
        let source = self.full_text.inner();
        let bytes = source.as_bytes();
        let mut pos = self.current.span.start;
        loop {
            match pos.checked_sub(1).map(|index| bytes[index]) {
                None => return true,
                Some(b' ' | b'\t') => pos -= 1,
                Some(b'\r' | b'\n') => return true,
                Some(_) => return false,
            }
        }
    }

    #[inline]
    pub fn in_native_blockquote_line(&self) -> bool {
        self.native_blockquote_depth.is_some()
    }

    #[inline]
    pub fn native_blockquote_depth(&self) -> Option<usize> {
        self.native_blockquote_depth
    }

    pub(crate) fn quote_body_line_is_empty_spaced(&self) -> bool {
        if self.current.token != Token::LineBreak {
            return false;
        }
        let Some(QuoteBodyCursor {
            line_state: QuoteBodyLineState::PreparedContent { content_start },
            ..
        }) = self.quote_body_cursor
        else {
            return false;
        };
        if content_start != self.current.span.start {
            return false;
        }

        let source = self.full_text.inner();
        let line_start = source[..content_start]
            .rfind('\n')
            .map_or(0, |offset| offset + 1);
        source[line_start..content_start].ends_with([' ', '\t'])
    }

    pub(crate) fn prepare_quote_body_line(
        &mut self,
    ) -> Result<QuoteBodyLineStatus, ParseError> {
        let Some(cursor) = self.quote_body_cursor else {
            return Ok(QuoteBodyLineStatus::Inactive);
        };

        if self.current().token == Token::ParagraphBreak {
            return Ok(QuoteBodyLineStatus::Boundary);
        }

        if !matches!(cursor.line_state, QuoteBodyLineState::NeedPrefix) {
            return Ok(QuoteBodyLineStatus::Prepared);
        }

        let mut prepared = self.clone();
        let mut remaining_depth = cursor.required_depth;

        loop {
            if prepared.current().token != Token::Quote {
                return Ok(QuoteBodyLineStatus::Boundary);
            }

            let physical_depth = prepared.current().slice.len();
            if physical_depth < remaining_depth {
                remaining_depth -= physical_depth;
                prepared.step()?;
                prepared.get_optional_space()?;
                prepared.mark_virtual_start_of_line();
            } else {
                prepared.mark_virtual_start_of_line();
                if physical_depth == remaining_depth {
                    prepared.step()?;
                    prepared.get_optional_space()?;
                    if !cursor.literal_residual_quotes
                        || prepared.current().token != Token::Quote
                    {
                        prepared.mark_virtual_start_of_line();
                    }
                    prepared.quote_body_cursor = Some(QuoteBodyCursor {
                        required_depth: cursor.required_depth,
                        line_state: QuoteBodyLineState::PreparedContent {
                            content_start: prepared.current().span.start,
                        },
                        literal_residual_quotes: cursor.literal_residual_quotes,
                    });
                } else {
                    prepared.quote_body_cursor = Some(QuoteBodyCursor {
                        required_depth: cursor.required_depth,
                        line_state: QuoteBodyLineState::PreparedResidual {
                            token_start: prepared.current().span.start,
                            stripped_depth: remaining_depth,
                        },
                        literal_residual_quotes: cursor.literal_residual_quotes,
                    });
                }
                self.update(&prepared);
                return Ok(QuoteBodyLineStatus::Prepared);
            }
        }
    }

    #[rustfmt::skip]
    pub(crate) fn native_blockquote_depths(
        &self,
        physical_depth: usize,
    ) -> (usize, usize) {
        if let Some(cursor) = self.quote_body_cursor {
            if let QuoteBodyLineState::PreparedResidual { token_start, stripped_depth } = cursor.line_state && token_start == self.current().span.start {
                return (physical_depth - stripped_depth, cursor.required_depth + physical_depth - stripped_depth);
            }
            if matches!(cursor.line_state, QuoteBodyLineState::PreparedContent { .. }) {
                return (physical_depth, cursor.required_depth + physical_depth);
            }
        }
        (physical_depth, self.native_blockquote_depth.unwrap_or(0) + physical_depth)
    }

    #[rustfmt::skip]
    pub(crate) fn install_quote_body_cursor(&mut self, required_depth: usize) {
        self.quote_body_cursor = Some(QuoteBodyCursor { required_depth, line_state: QuoteBodyLineState::NeedPrefix, literal_residual_quotes: false });
    }

    #[rustfmt::skip]
    pub(crate) fn install_quote_body_cursor_with_literal_residuals(&mut self, required_depth: usize) {
        self.quote_body_cursor = Some(QuoteBodyCursor { required_depth, line_state: QuoteBodyLineState::NeedPrefix, literal_residual_quotes: true });
    }

    #[rustfmt::skip]
    pub(crate) fn install_quote_body_cursor_at_current_line(&mut self, required_depth: usize) {
        self.quote_body_cursor = Some(QuoteBodyCursor { required_depth, line_state: QuoteBodyLineState::PreparedContent { content_start: self.current().span.start }, literal_residual_quotes: false });
    }

    #[rustfmt::skip]
    pub(crate) fn install_quote_body_cursor_at_current_line_with_literal_residuals(&mut self, required_depth: usize) {
        self.quote_body_cursor = Some(QuoteBodyCursor { required_depth, line_state: QuoteBodyLineState::PreparedContent { content_start: self.current().span.start }, literal_residual_quotes: true });
    }

    #[rustfmt::skip]
    pub(crate) fn quote_body_close_allowed_here(&self) -> bool {
        matches!(self.quote_body_cursor, Some(QuoteBodyCursor { line_state: QuoteBodyLineState::PreparedContent { content_start }, .. }) if content_start == self.current().span.start)
    }

    pub(crate) fn quote_body_close_follows_only_literal_quotes(&self) -> bool {
        let Some(QuoteBodyCursor {
            line_state: QuoteBodyLineState::PreparedContent { content_start },
            literal_residual_quotes: true,
            ..
        }) = self.quote_body_cursor
        else {
            return false;
        };
        if content_start >= self.current().span.start {
            return false;
        }
        let prefix = &self.full_text().inner()[content_start..self.current().span.start];
        prefix.contains('>')
            && prefix
                .chars()
                .all(|character| character == '>' || character.is_ascii_whitespace())
    }

    pub(crate) fn quote_body_close_follows_tab_close(&self) -> bool {
        let Some(QuoteBodyCursor {
            line_state: QuoteBodyLineState::PreparedContent { content_start },
            ..
        }) = self.quote_body_cursor
        else {
            return false;
        };
        let here = self.current().span.start;
        if content_start >= here {
            return false;
        }
        let prefix = &self.full_text().inner()[content_start..here];
        let trimmed = prefix.trim_end_matches(|c: char| c.is_ascii_whitespace());
        trimmed
            .get(trimmed.len().saturating_sub(8)..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case("[[/tab]]"))
    }

    #[rustfmt::skip]
    pub(crate) fn quote_body_needs_prefix(&self) -> bool {
        matches!(self.quote_body_cursor, Some(QuoteBodyCursor { line_state: QuoteBodyLineState::NeedPrefix, .. }))
    }

    pub(crate) fn wikidot_tab_quote_ejection(&self) -> bool {
        self.block_end_scan_cache
            .borrow()
            .wikidot_tab_quote_ejection
            .get()
    }

    pub(crate) fn set_wikidot_tab_quote_ejection(&self, value: bool) {
        self.block_end_scan_cache
            .borrow()
            .wikidot_tab_quote_ejection
            .set(value);
    }

    pub(crate) fn set_wikidot_body_soft_ended(&self, value: bool) {
        self.block_end_scan_cache
            .borrow()
            .wikidot_body_soft_ended
            .set(value);
    }

    pub(crate) fn take_wikidot_body_soft_ended(&self) -> bool {
        let cache = self.block_end_scan_cache.borrow();
        let value = cache.wikidot_body_soft_ended.get();
        cache.wikidot_body_soft_ended.set(false);
        value
    }

    pub(crate) fn wikidot_tabview_ejected(&self) -> bool {
        self.block_end_scan_cache
            .borrow()
            .wikidot_tabview_ejected
            .get()
    }

    pub(crate) fn set_wikidot_tabview_ejected(&self, value: bool) {
        self.block_end_scan_cache
            .borrow()
            .wikidot_tabview_ejected
            .set(value);
    }

    pub(crate) fn wikidot_tabview_tab_ordinal(&self) -> usize {
        self.block_end_scan_cache
            .borrow()
            .wikidot_tabview_tab_ordinal
            .get()
    }

    pub(crate) fn set_wikidot_tabview_tab_ordinal(&self, value: usize) {
        self.block_end_scan_cache
            .borrow()
            .wikidot_tabview_tab_ordinal
            .set(value);
    }

    pub(crate) fn push_wikidot_tabview_remote_ordinal(&self, ordinal: usize) {
        self.block_end_scan_cache
            .borrow_mut()
            .wikidot_tabview_remote_ordinals
            .push(ordinal);
    }

    pub(crate) fn take_wikidot_tabview_remote_ordinals(&self) -> Vec<usize> {
        mem::take(
            &mut self
                .block_end_scan_cache
                .borrow_mut()
                .wikidot_tabview_remote_ordinals,
        )
    }

    pub(crate) fn set_wikidot_tabview_remote_ordinals(&self, value: Vec<usize>) {
        self.block_end_scan_cache
            .borrow_mut()
            .wikidot_tabview_remote_ordinals = value;
    }

    pub(crate) fn quote_escaped_units_len(&self) -> usize {
        self.block_end_scan_cache.borrow().quote_escaped_units.len()
    }

    /// Whether a late tab's opener segment can receive a bare panel: the
    /// escaped unit recorded at `anchor` ends in the trailing blockquote chain
    /// the ejection scan leaves behind.
    pub(crate) fn quote_split_segment_available(&self, anchor: usize) -> bool {
        let cache = self.block_end_scan_cache.borrow();
        let Some(index) = anchor.checked_sub(1) else {
            return false;
        };
        let Some(QuoteEscapedUnit::Elements(elements, _)) =
            cache.quote_escaped_units.get(index)
        else {
            return false;
        };
        let mut current: &[Element<'t>] = elements;
        let mut descended = false;
        loop {
            match current.last() {
                Some(Element::Container(container))
                    if container.ctype() == crate::tree::ContainerType::Blockquote =>
                {
                    current = container.elements();
                    descended = true;
                }
                _ => break,
            }
        }
        descended
    }

    /// Appends a block element to the quote segment opened just after the last
    /// escaped unit when a late tab's opener was parsed (`anchor` is the
    /// escaped-unit count captured at that opener). The segment is the trailing
    /// blockquote chain the ejection scan leaves in that unit.
    pub(crate) fn append_quote_split_segment_element(
        &self,
        anchor: usize,
        element: Element<'t>,
    ) -> bool {
        let mut cache = self.block_end_scan_cache.borrow_mut();
        let Some(index) = anchor.checked_sub(1) else {
            return false;
        };
        let Some(QuoteEscapedUnit::Elements(elements, _)) =
            cache.quote_escaped_units.get_mut(index)
        else {
            return false;
        };
        fn descend<'a, 't>(
            elements: &'a mut Vec<Element<'t>>,
            descended: bool,
        ) -> (bool, &'a mut Vec<Element<'t>>) {
            let is_blockquote = matches!(
                elements.last(),
                Some(Element::Container(container))
                    if container.ctype() == crate::tree::ContainerType::Blockquote
            );
            if !is_blockquote {
                return (descended, elements);
            }
            let Some(Element::Container(container)) = elements.last_mut() else {
                unreachable!("match above proved the last element is a container");
            };
            descend(container.elements_mut(), true)
        }
        let (descended, current) = descend(elements, false);
        if !descended {
            return false;
        }
        current.push(element);
        true
    }

    pub(crate) fn push_quote_escaped_unit(&self, unit: QuoteEscapedUnit<'t>) {
        self.block_end_scan_cache
            .borrow_mut()
            .quote_escaped_units
            .push(unit);
    }

    pub(crate) fn take_quote_escaped_units(&self) -> Vec<QuoteEscapedUnit<'t>> {
        mem::take(&mut self.block_end_scan_cache.borrow_mut().quote_escaped_units)
    }

    pub(crate) fn quote_escaped_units_pending(&self) -> bool {
        !self
            .block_end_scan_cache
            .borrow()
            .quote_escaped_units
            .is_empty()
    }

    pub(crate) fn quote_body_cursor(&self) -> Option<QuoteBodyCursor> {
        self.quote_body_cursor
    }

    pub(crate) fn set_quote_body_cursor(&mut self, cursor: Option<QuoteBodyCursor>) {
        self.quote_body_cursor = cursor;
    }

    pub(crate) fn quote_scan_outcome(
        &self,
        key: QuoteScanKey,
    ) -> Option<QuoteScanOutcome> {
        self.quote_scan_cache.borrow().get(&key).copied()
    }

    #[rustfmt::skip]
    pub(crate) fn cache_quote_scan_outcomes(
        &self,
        rule: &'static str,
        required_depth: usize,
        allow_inline_close: bool,
        line_starts: &[usize],
        outcome: QuoteScanOutcome,
    ) {
        let mut cache = self.quote_scan_cache.borrow_mut();
        for &line_start in line_starts {
            cache.insert((rule, required_depth, allow_inline_close, line_start), outcome);
        }
    }

    fn block_end_scan_context(&self) -> BlockEndScanContext {
        BlockEndScanContext {
            discarding_hidden_body: self.discarding_hidden_body(),
            in_wikidot_center_body: self.in_wikidot_center_body(),
        }
    }

    pub(crate) fn block_end_scan_outcome(&self, key: BlockEndScanKey) -> Option<bool> {
        self.block_end_scan_cache
            .borrow()
            .body_end_outcomes
            .get(&(key, self.block_end_scan_context()))
            .copied()
    }

    pub(crate) fn cache_block_end_scan_outcomes(
        &self,
        rule: &'static str,
        token_states: &[(usize, bool)],
        outcome: bool,
    ) {
        let mut cache = self.block_end_scan_cache.borrow_mut();
        let context = self.block_end_scan_context();
        for &(token_start, first_iteration) in token_states {
            cache
                .body_end_outcomes
                .insert(((rule, token_start, first_iteration), context), outcome);
        }
    }

    pub(crate) fn wikidot_composite_span_close_outcome(
        &self,
        token_start: usize,
    ) -> Option<bool> {
        self.block_end_scan_cache
            .borrow()
            .wikidot_composite_span_close
            .get(&(token_start, self.block_end_scan_context()))
            .copied()
    }

    pub(crate) fn cache_wikidot_composite_span_close_outcomes(
        &self,
        token_starts: &[usize],
        outcome: bool,
    ) {
        let mut cache = self.block_end_scan_cache.borrow_mut();
        let context = self.block_end_scan_context();
        for &token_start in token_starts {
            cache
                .wikidot_composite_span_close
                .insert((token_start, context), outcome);
        }
    }

    #[cfg(test)]
    pub(crate) fn composite_span_close_scan_token_visits(&self) -> usize {
        self.block_end_scan_cache
            .borrow()
            .composite_span_close_scan_token_visits
            .get()
    }

    #[cfg(test)]
    pub(crate) fn increment_composite_span_close_scan_token_visits(&self) {
        let cache = self.block_end_scan_cache.borrow();
        cache
            .composite_span_close_scan_token_visits
            .set(cache.composite_span_close_scan_token_visits.get() + 1);
    }

    pub(crate) fn underclosed_block_failure_cached(
        &self,
        rule: &'static str,
        owner_start: usize,
    ) -> bool {
        self.block_end_scan_cache
            .borrow()
            .underclosed_block_failures
            .contains_key(&(rule, owner_start))
    }

    pub(crate) fn cache_underclosed_block_failure(
        &self,
        rule: &'static str,
        owner_start: usize,
    ) {
        self.block_end_scan_cache
            .borrow_mut()
            .underclosed_block_failures
            .insert((rule, owner_start), ());
    }

    pub(crate) fn partial_rejection(
        &self,
        attempted_rule: &'static str,
        parent_rule: &'static str,
        token_start: usize,
    ) -> Option<ParseError> {
        self.block_end_scan_cache
            .borrow()
            .partial_rejections
            .get(&(
                attempted_rule,
                parent_rule,
                token_start,
                self.accepts_partial as u8,
            ))
            .cloned()
    }

    pub(crate) fn cache_partial_rejection(
        &self,
        attempted_rule: &'static str,
        parent_rule: &'static str,
        token_start: usize,
        error: &ParseError,
    ) {
        self.block_end_scan_cache
            .borrow_mut()
            .partial_rejections
            .insert(
                (
                    attempted_rule,
                    parent_rule,
                    token_start,
                    self.accepts_partial as u8,
                ),
                error.clone(),
            );
    }

    /// The context part of a rule attempt key, with depth pinned, for an outcome
    /// shared across positions in one flat sequence.
    pub(crate) fn rule_attempt_context(&self) -> Option<RuleAttemptKey> {
        let mut key = self.rule_attempt_key("")?;
        key.remaining = 0;
        key.start_of_line = false;
        key.depth = Some(self.depth);
        Some(key)
    }

    fn rule_attempt_key(&self, rule: &'static str) -> Option<RuleAttemptKey> {
        #[cfg(test)]
        if RULE_ATTEMPT_MEMO_DISABLED.with(Cell::get) {
            return None;
        }
        if self.discarding_hidden_body
            || !self.hidden_body_boundaries.is_empty()
            || self.native_blockquote_depth.is_some()
            || self.quote_body_cursor.is_some()
            || self.inside_inline_container_body()
        {
            return None;
        }

        let accepts_partial_offset = (self.accepts_partial_depth != 0).then(|| {
            usize::from(self.accepts_partial_depth) as isize - self.depth as isize
        });
        Some(RuleAttemptKey {
            rule,
            remaining: self.remaining.len(),
            depth: None,
            accepts_partial: self.accepts_partial,
            accepts_partial_offset,
            in_footnote: self.in_footnote,
            in_note: self.in_note,
            in_color: self.in_color,
            has_footnote_block: self.has_footnote_block,
            start_of_line: self.start_of_line,
            wikidot: self.wikidot.attempt_key(),
        })
    }

    pub(crate) fn start_rule_attempt(&self) -> usize {
        self.block_end_scan_cache
            .borrow()
            .peak_depth
            .replace(self.depth)
    }

    pub(crate) fn finish_rule_attempt(&self, enclosing_peak: usize) -> usize {
        let cache = self.block_end_scan_cache.borrow();
        let peak = cache.peak_depth.get();
        cache.peak_depth.set(peak.max(enclosing_peak));
        peak - self.depth
    }

    pub(crate) fn body_walk_key(&self, rule: &'static str) -> Option<BodyWalkKey> {
        Some(BodyWalkKey {
            attempt: self.rule_attempt_key(rule)?,
            boundaries: self.body_end_boundary_fingerprint(),
        })
    }

    /// Whether a body walk that reached this position before, in the same
    /// context, ran to `InputEnd` without meeting its closer. The walk from here
    /// on is the same computation, so it ends the same way.
    pub(crate) fn body_walk_reaches_input_end(&self, key: BodyWalkKey) -> bool {
        let cache = self.block_end_scan_cache.borrow();
        if !cache
            .body_walk_failure_positions
            .get(self.remaining.len())
            .is_some_and(|&recorded| recorded)
        {
            return false;
        }
        let height = cache
            .body_walk_failures
            .get(&key)
            .filter(|height| self.depth + **height <= self.max_recursion_depth)
            .or_else(|| {
                let mut at_depth = key;
                at_depth.attempt.depth = Some(self.depth);
                cache.body_walk_failures.get(&at_depth)
            });
        let Some(height) = height else {
            return false;
        };
        cache
            .peak_depth
            .set(cache.peak_depth.get().max(self.depth + height));
        true
    }

    /// Records the positions a body walk passed before it ran to `InputEnd`.
    pub(crate) fn cache_body_walk_failure(&self, visited: &[(usize, BodyWalkKey)]) {
        let mut cache = self.block_end_scan_cache.borrow_mut();
        let height = cache.peak_depth.get().saturating_sub(self.depth);
        let exceeds_limit = self.depth + height > self.max_recursion_depth;
        for &(position, mut key) in visited {
            if exceeds_limit {
                key.attempt.depth = Some(self.depth);
            }
            let positions = &mut cache.body_walk_failure_positions;
            if positions.len() <= position {
                positions.resize(position + 1, false);
            }
            positions[position] = true;
            cache.body_walk_failures.insert(key, height);
        }
    }

    pub(crate) fn rule_attempt_failure(&self, rule: &'static str) -> Option<ParseError> {
        let cache = self.block_end_scan_cache.borrow();
        if !cache
            .rule_attempt_failure_positions
            .get(self.remaining.len())
            .is_some_and(|&recorded| recorded)
        {
            return None;
        }

        let key = self.rule_attempt_key(rule)?;
        let (error, height) = cache
            .rule_attempt_failures
            .get(&key)
            .filter(|(_, height)| self.depth + height <= self.max_recursion_depth)
            .or_else(|| {
                let at_depth = RuleAttemptKey {
                    depth: Some(self.depth),
                    ..key
                };
                cache.rule_attempt_failures.get(&at_depth)
            })?;
        cache
            .peak_depth
            .set(cache.peak_depth.get().max(self.depth + height));
        Some(error.clone())
    }

    pub(crate) fn cache_rule_attempt_failure(
        &self,
        rule: &'static str,
        error: &ParseError,
        height: usize,
    ) {
        let Some(mut key) = self.rule_attempt_key(rule) else {
            return;
        };
        if self.depth + height > self.max_recursion_depth {
            key.depth = Some(self.depth);
        }

        let mut cache = self.block_end_scan_cache.borrow_mut();
        let position = self.remaining.len();
        let positions = &mut cache.rule_attempt_failure_positions;
        if positions.len() <= position {
            positions.resize(position + 1, false);
        }
        positions[position] = true;
        cache
            .rule_attempt_failures
            .insert(key, (error.clone(), height));
    }

    pub(crate) fn deterministic_block_failure(
        &self,
        rule: &'static str,
        owner_start: usize,
    ) -> Option<ParseError> {
        self.block_end_scan_cache
            .borrow()
            .deterministic_block_failures
            .get(&(rule, owner_start))
            .cloned()
    }

    pub(crate) fn cache_deterministic_block_failure(
        &self,
        rule: &'static str,
        owner_start: usize,
        error: &ParseError,
    ) {
        self.block_end_scan_cache
            .borrow_mut()
            .deterministic_block_failures
            .insert((rule, owner_start), error.clone());
    }

    pub(crate) fn wikidot_tabview_failure(
        &self,
        owner_start: usize,
    ) -> Option<ParseError> {
        self.block_end_scan_cache
            .borrow()
            .wikidot_tabview_failures
            .get(&(owner_start, self.in_wikidot_collapsible()))
            .cloned()
    }

    pub(crate) fn cache_wikidot_tabview_failure(
        &self,
        owner_start: usize,
        error: &ParseError,
    ) {
        self.block_end_scan_cache
            .borrow_mut()
            .wikidot_tabview_failures
            .insert((owner_start, self.in_wikidot_collapsible()), error.clone());
    }

    pub(crate) fn wikidot_collapsible_failure(
        &self,
        owner_start: usize,
    ) -> Option<ParseError> {
        self.block_end_scan_cache
            .borrow()
            .wikidot_collapsible_failures
            .get(&owner_start)
            .cloned()
    }

    pub(crate) fn cache_wikidot_collapsible_failure(
        &self,
        owner_start: usize,
        error: &ParseError,
    ) {
        self.block_end_scan_cache
            .borrow_mut()
            .wikidot_collapsible_failures
            .insert(owner_start, error.clone());
    }

    pub(crate) fn wikidot_div_bounded_span_outcome(
        &self,
        owner_start: usize,
    ) -> Option<bool> {
        self.block_end_scan_cache
            .borrow()
            .wikidot_div_bounded_span
            .get(&owner_start)
            .copied()
    }

    pub(crate) fn cache_wikidot_div_bounded_span(
        &self,
        owner_start: usize,
        outcome: bool,
    ) {
        self.block_end_scan_cache
            .borrow_mut()
            .wikidot_div_bounded_span
            .insert(owner_start, outcome);
    }

    pub(crate) fn two_block_end_scan_outcome(&self, key: BlockEndScanKey) -> Option<u8> {
        self.two_block_end_scan_cache
            .borrow()
            .get(&(key, self.block_end_scan_context()))
            .copied()
    }

    pub(crate) fn single_link_label_owner_scan_outcome(
        &self,
        token_start: usize,
    ) -> Option<bool> {
        self.single_link_label_owner_scan_cache
            .borrow()
            .get(&token_start)
            .copied()
    }

    pub(crate) fn cache_single_link_label_owner_scan_outcomes(
        &self,
        token_starts: &[usize],
        outcome: bool,
    ) {
        let mut cache = self.single_link_label_owner_scan_cache.borrow_mut();
        for &token_start in token_starts {
            cache.insert(token_start, outcome);
        }
    }

    pub(crate) fn wikidot_code_candidate(
        &self,
        opener_start: usize,
    ) -> Option<crate::wikidot_code::WikidotCodeCandidate> {
        let source = self.full_text.inner();
        let line_start = source[..opener_start]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        let line_owned = source[line_start..opener_start]
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\t' | b'\0'));
        if !line_owned {
            return crate::wikidot_code::candidate_at_owned_opener(
                source,
                opener_start,
                self.settings().mode == crate::settings::WikitextMode::List,
            );
        }

        let mut cache = self.wikidot_code_candidate_cache.borrow_mut();
        let candidates =
            cache.get_or_insert_with(|| crate::wikidot_code::candidates(source));
        candidates.get(&opener_start).cloned()
    }

    pub(crate) fn cache_two_block_end_scan_outcomes(
        &self,
        rule: &'static str,
        token_states: &[(usize, bool, u8)],
        total_matches: u8,
    ) {
        let mut cache = self.two_block_end_scan_cache.borrow_mut();
        let context = self.block_end_scan_context();
        for &(token_start, first_iteration, preceding_matches) in token_states {
            if total_matches >= 2 && preceding_matches > 0 {
                continue;
            }
            cache.insert(
                ((rule, token_start, first_iteration), context),
                total_matches.saturating_sub(preceding_matches).min(2),
            );
        }
    }

    pub(crate) fn lost_owner_scan_outcome(&self, key: LostOwnerScanKey) -> Option<bool> {
        self.block_end_scan_cache
            .borrow()
            .lost_owner_outcomes
            .get(&key)
            .copied()
    }

    pub(crate) fn cache_lost_owner_scan_outcomes(
        &self,
        close_name: &'static str,
        owner_kind: u8,
        owner_depth: usize,
        token_states: &[(usize, bool)],
        outcome: bool,
    ) {
        let mut cache = self.block_end_scan_cache.borrow_mut();
        for &(token_start, start_of_line) in token_states {
            cache.lost_owner_outcomes.insert(
                (
                    close_name,
                    owner_kind,
                    owner_depth,
                    token_start,
                    start_of_line,
                ),
                outcome,
            );
        }
    }

    pub(crate) fn lost_owner_literal_range_end(&self, offset: usize) -> Option<usize> {
        let mut cache = self.block_end_scan_cache.borrow_mut();
        let ranges = cache.literal_ranges.get_or_insert_with(|| {
            LiteralRegionIndex::new(self.full_text.inner())
                .ranges()
                .to_vec()
        });
        let insertion = ranges.partition_point(|range| range.start <= offset);
        if insertion == 0 || offset >= ranges[insertion - 1].end {
            None
        } else {
            Some(ranges[insertion - 1].end)
        }
    }

    pub(crate) fn cache_wikidot_span_alias_close_score(
        &self,
        close_start: usize,
        scored: bool,
    ) {
        self.block_end_scan_cache
            .borrow_mut()
            .span_alias_close_scores
            .insert(close_start, scored);
    }

    /// Records that the line starting at `head` was joined onto a scored
    /// alias close (live's span rule ate the newline before it), so a
    /// line-anchored block opener there is literal.
    pub(crate) fn mark_wikidot_scored_join_head(&self, head: usize) {
        self.block_end_scan_cache
            .borrow_mut()
            .wikidot_scored_join_heads
            .insert(head);
    }

    pub(crate) fn wikidot_scored_join_head(&self, head: usize) -> bool {
        self.block_end_scan_cache
            .borrow()
            .wikidot_scored_join_heads
            .contains(&head)
    }

    pub(crate) fn wikidot_span_alias_close_is_scored(&self, close_start: usize) -> bool {
        self.block_end_scan_cache
            .borrow()
            .span_alias_close_scores
            .get(&close_start)
            .copied()
            .unwrap_or(false)
    }

    #[cfg(test)]
    pub(crate) fn lost_owner_scan_token_visits(&self) -> usize {
        self.block_end_scan_token_visits()
    }

    #[cfg(test)]
    pub(crate) fn increment_lost_owner_scan_token_visits(&self) {
        self.increment_block_end_scan_token_visits();
    }

    #[cfg(test)]
    pub(crate) fn block_end_scan_token_visits(&self) -> usize {
        self.block_end_scan_token_visits.get()
    }

    #[cfg(test)]
    pub(crate) fn increment_block_end_scan_token_visits(&self) {
        self.block_end_scan_token_visits
            .set(self.block_end_scan_token_visits.get() + 1);
    }

    #[cfg(test)]
    pub(crate) fn quote_scan_token_visits(&self) -> usize {
        self.quote_scan_token_visits.get()
    }

    #[cfg(test)]
    pub(crate) fn increment_quote_scan_token_visits(&self) {
        self.quote_scan_token_visits
            .set(self.quote_scan_token_visits.get() + 1);
    }

    #[cfg(test)]
    pub(crate) fn underline_fast_path_visits(&self) -> usize {
        self.underline_fast_path_visits.get()
    }

    #[cfg(test)]
    pub(crate) fn increment_underline_fast_path_visits(&self) {
        self.underline_fast_path_visits
            .set(self.underline_fast_path_visits.get() + 1);
    }

    #[inline]
    pub(crate) fn mark_virtual_start_of_line(&mut self) {
        self.start_of_line = true;
    }

    /// Mark the current token as logically mid-line. Used where live Wikidot
    /// has consumed the preceding break as part of a scored inline block, so
    /// the line-leading rules must not fire on what follows.
    #[inline]
    pub(crate) fn clear_start_of_line(&mut self) {
        self.start_of_line = false;
    }

    #[inline]
    pub(crate) fn record_virtual_gate_end(&self, gate_end: usize) {
        self.block_end_scan_cache
            .borrow()
            .last_virtual_gate_end
            .set(Some(gate_end));
    }

    pub(crate) fn virtual_gate_chain_reaches(&self, gate_open: usize) -> bool {
        let Some(gate_end) = self
            .block_end_scan_cache
            .borrow()
            .last_virtual_gate_end
            .get()
        else {
            return false;
        };
        if gate_end > gate_open {
            return false;
        }
        self.full_text().inner()[gate_end..gate_open]
            .bytes()
            .all(|byte| byte == b' ' || byte == b'\t')
    }

    #[inline]
    pub(crate) fn set_native_blockquote_depth(&mut self, value: Option<usize>) {
        self.native_blockquote_depth = value;
    }

    #[inline]
    pub fn set_rule(&mut self, rule: Rule) {
        self.rule = rule;
    }

    pub fn clone_with_rule(&self, rule: Rule) -> Self {
        let mut clone = self.clone();
        clone.set_rule(rule);
        clone
    }

    pub fn depth_increment(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        trace!("Incrementing recursion depth to {}", self.depth);
        let cache = self.block_end_scan_cache.borrow();
        if self.depth > cache.peak_depth.get() {
            cache.peak_depth.set(self.depth);
        }
        drop(cache);

        if self.depth > self.max_recursion_depth {
            return Err(self.make_err(ParseErrorKind::RecursionDepthExceeded));
        }

        Ok(())
    }

    #[inline]
    pub fn depth_decrement(&mut self) {
        self.depth -= 1;
        trace!("Decrementing recursion depth to {}", self.depth);
    }

    #[inline]
    pub fn set_accepts_partial(&mut self, value: AcceptsPartial) {
        self.accepts_partial = value;
    }

    #[inline]
    pub(crate) fn set_accepts_partial_child(&mut self, value: AcceptsPartial) {
        self.accepts_partial = value;
        self.accepts_partial_depth =
            u16::try_from(self.depth + 1).expect("parser depth limit fits in u16");
    }

    #[inline]
    pub(crate) fn accepts_partial_depth(&self) -> u16 {
        self.accepts_partial_depth
    }

    #[inline]
    pub(crate) fn set_accepts_partial_depth(&mut self, value: u16) {
        self.accepts_partial_depth = value;
    }

    #[inline]
    pub fn set_footnote_flag(&mut self, value: bool) {
        self.in_footnote = value;
    }

    #[inline]
    pub fn set_footnote_block(&mut self) {
        self.has_footnote_block = true;
    }

    pub fn get_mutable_state(&self) -> ParserMutableState {
        ParserMutableState {
            footnote_index: self.footnotes.borrow().len(),
            html_block_index: self.html_blocks.borrow().len(),
            code_block_index: self.code_blocks.borrow().len(),
            table_of_contents_index: self.table_of_contents.borrow().len(),
            bibliography_index: self.bibliographies.borrow().next_index(),
            has_footnote_block: self.has_footnote_block,
        }
    }

    pub fn reset_mutable_state(
        &mut self,
        ParserMutableState {
            footnote_index,
            html_block_index,
            code_block_index,
            table_of_contents_index,
            bibliography_index,
            has_footnote_block,
        }: ParserMutableState,
    ) {
        self.footnotes.borrow_mut().truncate(footnote_index);
        self.html_blocks.borrow_mut().truncate(html_block_index);
        self.code_blocks.borrow_mut().truncate(code_block_index);
        let mut table_of_contents = self.table_of_contents.borrow_mut();
        table_of_contents.truncate(table_of_contents_index);
        let mut bibliographies = self.bibliographies.borrow_mut();
        bibliographies.truncate(bibliography_index);
        self.has_footnote_block = has_footnote_block;
    }

    pub fn check_page_syntax(&self) -> Result<(), ParseError> {
        if self.settings.enable_page_syntax {
            Ok(())
        } else {
            Err(self.make_err(ParseErrorKind::NotSupportedMode))
        }
    }

    pub fn push_table_of_contents_entry(
        &mut self,
        heading: HeadingLevel,
        name_elements: &[Element],
    ) {
        let level = usize::from(heading.value()) - 1;

        let mut resolved = name_elements.to_vec();
        resolve_static_suppressions(&mut resolved, self.settings.layout.legacy());
        if self.settings.layout.legacy() {
            crate::parsing::inline_format::normalize_wikidot_inline_formats(
                &mut resolved,
            );
        }
        let name = if elements_contain_delayed(&resolved) {
            String::new()
        } else {
            TextRender.render_partial_title(&resolved, self.page_info, self.settings, 0)
        };

        self.table_of_contents.borrow_mut().push((level, name));
    }

    #[cold]
    pub fn remove_html_blocks(&mut self) -> Vec<Cow<'t, str>> {
        mem::take(&mut self.html_blocks.borrow_mut())
    }

    #[cold]
    pub fn remove_code_blocks(&mut self) -> Vec<CodeBlock<'t>> {
        mem::take(&mut self.code_blocks.borrow_mut())
    }

    #[cold]
    pub fn remove_table_of_contents(&mut self) -> Vec<(usize, String)> {
        mem::take(&mut self.table_of_contents.borrow_mut())
    }

    pub fn push_footnote(&mut self, contents: Vec<Element<'t>>) -> usize {
        let mut footnotes = self.footnotes.borrow_mut();
        footnotes.push(contents);
        footnotes.len()
    }

    pub fn footnote_count(&self) -> usize {
        self.footnotes.borrow().len()
    }

    pub fn truncate_footnotes(&mut self, count: usize) {
        self.footnotes.borrow_mut().truncate(count);
    }

    #[cold]
    pub fn remove_footnotes(&mut self) -> Vec<Vec<Element<'t>>> {
        mem::take(&mut self.footnotes.borrow_mut())
    }

    pub fn push_html_block(&mut self, new_block: Cow<'t, str>) {
        self.html_blocks.borrow_mut().push(new_block);
    }

    pub fn push_code_block(&mut self, new_block: CodeBlock<'t>) {
        self.code_blocks.borrow_mut().push(new_block);
    }

    pub fn push_code_block_element(&mut self, code_block: CodeBlock<'t>) -> Element<'t> {
        self.push_code_block(code_block.clone());
        Element::Code(code_block)
    }

    pub fn has_bibliography(&self) -> bool {
        !self.bibliographies.borrow().is_empty()
    }

    pub fn push_bibliography(&mut self, bibliography: Bibliography<'t>) -> usize {
        let mut guard = self.bibliographies.borrow_mut();
        let index = guard.next_index();
        guard.push(bibliography);
        std::convert::identity(index)
    }

    #[cold]
    pub fn remove_bibliographies(&mut self) -> BibliographyList<'t> {
        mem::take(&mut self.bibliographies.borrow_mut())
    }

    pub fn append_shared_items(
        &mut self,
        html_blocks: &mut Vec<Cow<'t, str>>,
        code_blocks: &mut Vec<CodeBlock<'t>>,
        table_of_contents: &mut Vec<(usize, String)>,
        footnotes: &mut Vec<Vec<Element<'t>>>,
        bibliographies: &mut BibliographyList<'t>,
    ) {
        self.html_blocks.borrow_mut().append(html_blocks);

        self.code_blocks.borrow_mut().append(code_blocks);

        let mut table_of_contents_guard = self.table_of_contents.borrow_mut();
        table_of_contents_guard.append(table_of_contents);

        self.footnotes.borrow_mut().append(footnotes);

        self.bibliographies.borrow_mut().append(bibliographies);
    }

    #[inline]
    pub fn evaluate(&self, condition: ParseCondition) -> bool {
        match condition {
            ParseCondition::CurrentToken(token) => self.current.token == token,
            ParseCondition::CurrentTokenAtSourceLineStart(token) => {
                self.current.token == token && self.current_at_source_line_start()
            }
            ParseCondition::TokenPair(current, next) => {
                if self.current.token != current {
                    return false;
                }

                match self.look_ahead(0) {
                    Some(actual) => actual.token == next,
                    None => false,
                }
            }
            ParseCondition::AtSourceByte(byte) => self.current.span.start >= byte,
            ParseCondition::SplitClose(delimiter) => split_delimiter_end(
                TokenWindow::at(self),
                SegmentCtx::from_parser(self),
                delimiter,
            )
            .is_some(),
            ParseCondition::PaddedSplitClose(delimiter) => {
                self.current.token == Token::Whitespace
                    && TokenWindow::from_slice(self.remaining()).is_some_and(|window| {
                        split_delimiter_end(
                            window,
                            SegmentCtx::from_parser(self),
                            delimiter,
                        )
                        .is_some()
                    })
            }
        }
    }

    #[inline]
    pub fn evaluate_any(&self, conditions: &[ParseCondition]) -> bool {
        conditions.iter().any(|&condition| self.evaluate(condition))
    }

    #[inline]
    pub fn evaluate_fn<F>(&self, f: F) -> bool
    where
        F: FnOnce(&mut Parser<'r, 't>) -> Result<bool, ParseError>,
    {
        f(&mut self.clone()).unwrap_or(false)
    }

    pub fn save_evaluate_fn<F>(&mut self, f: F) -> Option<&'r ExtractedToken<'t>>
    where
        F: FnOnce(&mut Parser<'r, 't>) -> Result<bool, ParseError>,
    {
        let mut parser = self.clone();
        if f(&mut parser).unwrap_or(false) {
            let last = self.current;
            self.update(&parser);
            Some(last)
        } else {
            None
        }
    }

    #[inline]
    pub fn current(&self) -> &'r ExtractedToken<'t> {
        self.current
    }

    #[inline]
    pub fn remaining(&self) -> &'r [ExtractedToken<'t>] {
        self.remaining
    }

    #[inline]
    pub fn update(&mut self, parser: &Parser<'r, 't>) {
        self.accepts_partial = parser.accepts_partial;
        self.accepts_partial_depth = parser.accepts_partial_depth;
        self.in_footnote = parser.in_footnote;
        self.in_note = parser.in_note;
        self.in_color = parser.in_color;
        self.has_footnote_block = parser.has_footnote_block;
        self.start_of_line = parser.start_of_line;
        self.native_blockquote_depth = parser.native_blockquote_depth;
        self.quote_body_cursor = parser.quote_body_cursor;
        self.wikidot = parser.wikidot;

        self.current = parser.current;
        self.remaining = parser.remaining;
    }

    #[inline]
    pub fn same_pointer(&self, old_remaining: &'r [ExtractedToken<'t>]) -> bool {
        ptr::eq(self.remaining, old_remaining)
    }

    /// Moves to `remaining[index]`, with the same result as `index + 1` steps.
    pub(crate) fn step_to_remaining(&mut self, index: usize) {
        let previous = index
            .checked_sub(1)
            .map_or(self.current, |before| &self.remaining[before]);
        self.start_of_line = token_starts_line(previous.token);
        if let Some(cursor) = self.quote_body_cursor.as_mut()
            && std::iter::once(self.current)
                .chain(&self.remaining[..index])
                .any(|token| {
                    matches!(token.token, Token::LineBreak | Token::ParagraphBreak)
                })
        {
            cursor.line_state = QuoteBodyLineState::NeedPrefix;
        }
        self.current = &self.remaining[index];
        self.remaining = &self.remaining[index + 1..];
    }

    #[inline]
    pub fn step(&mut self) -> Result<&'r ExtractedToken<'t>, ParseError> {
        self.start_of_line = token_starts_line(self.current.token);

        if matches!(self.current.token, Token::LineBreak | Token::ParagraphBreak)
            && let Some(cursor) = self.quote_body_cursor.as_mut()
        {
            cursor.line_state = QuoteBodyLineState::NeedPrefix;
        }

        match self.remaining.split_first() {
            Some((current, remaining)) => {
                self.current = current;
                self.remaining = remaining;
                Ok(current)
            }
            None => Err(self.make_err(ParseErrorKind::EndOfInput)),
        }
    }

    #[inline]
    pub fn step_n(&mut self, count: usize) -> Result<(), ParseError> {
        for _ in 0..count {
            self.step()?;
        }

        Ok(())
    }

    #[inline]
    pub fn look_ahead(&self, offset: usize) -> Option<&'r ExtractedToken<'t>> {
        self.remaining.get(offset)
    }

    #[inline]
    pub fn look_ahead_err(
        &self,
        offset: usize,
    ) -> Result<&'r ExtractedToken<'t>, ParseError> {
        self.look_ahead(offset)
            .ok_or_else(|| self.make_err(ParseErrorKind::EndOfInput))
    }

    pub fn next_two_tokens(&self) -> (Token, Option<Token>) {
        let current = self.current.token;
        let next = self.look_ahead(0).map(|token| token.token);
        std::convert::identity((current, next))
    }

    pub fn next_three_tokens(&self) -> (Token, Option<Token>, Option<Token>) {
        let first = self.current.token;
        let second = self.look_ahead(0).map(|next| next.token);
        let third = self.look_ahead(1).map(|next| next.token);
        (first, second, third)
    }

    pub fn get_token(
        &mut self,
        token: Token,
        kind: ParseErrorKind,
    ) -> Result<&'t str, ParseError> {
        let current = self.current();
        if current.token == token {
            let text = current.slice;
            self.step()?;
            Ok(text)
        } else {
            Err(self.make_err(kind))
        }
    }

    pub fn get_optional_token(&mut self, token: Token) -> Result<(), ParseError> {
        if self.current().token == token {
            self.step()?;
        }

        Ok(())
    }

    pub fn get_optional_line_break(&mut self) -> Result<(), ParseError> {
        self.get_optional_token(Token::LineBreak)
    }

    #[inline]
    pub fn get_optional_space(&mut self) -> Result<(), ParseError> {
        self.get_optional_token(Token::Whitespace)
    }

    pub fn get_optional_spaces_any(&mut self) -> Result<(), ParseError> {
        loop {
            let current_token = self.current().token;
            if !is_optional_space_token(current_token) {
                return Ok(());
            }

            self.step()?;
        }
    }

    #[cold]
    #[inline]
    pub fn make_err(&self, kind: ParseErrorKind) -> ParseError {
        ParseError::new(kind, self.rule, self.current)
    }

    pub(crate) fn make_end_of_input_err(&self) -> ParseError {
        let input_end = self.remaining.last().unwrap_or(self.current);
        debug_assert_eq!(input_end.token, Token::InputEnd);
        ParseError::new(ParseErrorKind::EndOfInput, self.rule, input_end)
    }

    /// Whether the Wikidot anchor-label walk that starts at byte `start` is
    /// certain to run to `InputEnd` and fail there with `EndOfInput`.
    ///
    /// `collect_wikidot_anchor_label` only succeeds at a `]`-bearing token
    /// (`RightBracket`/`RightBlock`/`RightLink`, or the `RightLink` of a
    /// fused iftags closer) and only fails elsewhere at a line break, a
    /// generated token, a comment sub-parse failure, or `InputEnd`. None of
    /// those tokens can occur at or after `start` when the source tail has
    /// no `]`, `\n`, `\r`, or `\` byte and no generated marker:
    ///
    /// - `RightBracket`/`RightBlock`/`RightLink` slices contain `]`, so a
    ///   fused closer is impossible too.
    /// - `LineBreak`/`ParagraphBreak` slices contain a newline byte, except
    ///   for the tokenizer's terminal-backslash `LineBreak`, which needs a
    ///   `\` byte.
    /// - The tokenizer only emits `LeftComment` when a later `--]` closer
    ///   exists, which contains `]` (also across runtime-literal segments).
    /// - `GeneratedPageLink`/`GeneratedTagLinks` tokens require a generated
    ///   delayed marker at their position.
    ///
    /// Every remaining token therefore falls into the step-over arms (a
    /// plain step, or an iftags segment that cannot fuse and cannot error),
    /// so the walk reaches `InputEnd` and returns the same `EndOfInput`
    /// error. Successive `[#` openers share that unclosed tail; the
    /// monotone slot keeps the total scan work linear instead of walking the
    /// rest of the document once per opener.
    pub(crate) fn anchor_label_reaches_input_end(&self, start: usize) -> bool {
        let source = self.full_text().inner();
        let mut cache = self.block_end_scan_cache.borrow_mut();
        let stop = match cache.anchor_label_stop {
            Some((scanned_from, stop))
                if scanned_from <= start && stop.is_none_or(|stop| stop >= start) =>
            {
                stop
            }
            _ => {
                let tail = &source[start..];
                let byte_stop = [
                    tail.find(']'),
                    tail.find('\n'),
                    tail.find('\r'),
                    tail.find('\\'),
                ]
                .into_iter()
                .flatten()
                .min()
                .map(|offset| start + offset);
                let generated_stop = self.delayed_markers.range(start..).find_map(
                    |(&position, marker)| {
                        marker.generated().is_some().then_some(position)
                    },
                );
                let stop = match (byte_stop, generated_stop) {
                    (Some(byte_stop), Some(generated_stop)) => {
                        Some(byte_stop.min(generated_stop))
                    }
                    (byte_stop, generated_stop) => byte_stop.or(generated_stop),
                };
                cache.anchor_label_stop = Some((start, stop));
                stop
            }
        };
        stop.is_none()
    }

    /// Position this parser at the trailing `InputEnd` token without
    /// stepping through the tokens in between.
    ///
    /// Callers must have proven that a step-by-step walk would end here (see
    /// `Parser::anchor_label_reaches_input_end`), which also means no line
    /// break is skipped, so the quote-body cursor needs no update. Only the
    /// current token is observed after the walk fails: the error is built
    /// from it and the discarded sub-parser position is not committed.
    pub(crate) fn jump_to_input_end(&mut self) {
        let Some(input_end) = self.remaining.last() else {
            debug_assert_eq!(self.current.token, Token::InputEnd);
            return;
        };
        debug_assert_eq!(input_end.token, Token::InputEnd);
        let previous = self
            .remaining
            .len()
            .checked_sub(2)
            .map(|index| &self.remaining[index])
            .unwrap_or(self.current);
        self.start_of_line = token_starts_line(previous.token);
        self.current = input_end;
        self.remaining = &self.remaining[self.remaining.len()..];
    }

    /// Absolute index of this parser's current token in the tokenization.
    ///
    /// Closer-scan memos key on these indices because the token stream is
    /// immutable for the whole parse.
    pub(crate) fn current_token_index(&self) -> usize {
        self.block_end_scan_cache
            .borrow()
            .closer_scan
            .index_of_current(self.remaining().len())
    }

    /// Absolute index of the first token after the current one, which is where
    /// a forward closer scan starting "ahead" of the current token begins.
    pub(crate) fn ahead_token_index(&self) -> usize {
        self.block_end_scan_cache
            .borrow()
            .closer_scan
            .index_ahead(self.remaining().len())
    }

    /// For a split-free `validate_reachable_close` scan for `closer` that
    /// starts at absolute token index `start`: `Some(has_paragraph_break)`
    /// when the walk provably only reaches `InputEnd` (and therefore returns
    /// exactly the recorded error kind), `None` when the scan must run.
    pub(crate) fn delimiter_close_absent(
        &self,
        closer: Token,
        start: usize,
    ) -> Option<bool> {
        self.block_end_scan_cache
            .borrow()
            .closer_scan
            .delimiter_absence(closer, start)
    }

    /// Record that a split-free scan for `closer` from `start` reached
    /// `InputEnd` without touching block bookkeeping.
    pub(crate) fn record_delimiter_close_absent(
        &self,
        closer: Token,
        start: usize,
        last_paragraph_break: Option<usize>,
    ) {
        self.block_end_scan_cache
            .borrow_mut()
            .closer_scan
            .record_delimiter_absence(closer, start, last_paragraph_break);
    }

    /// Whether a Wikidot math scan starting at absolute token index `start`
    /// provably reaches `InputEnd` without a `RightMath` closer.
    pub(crate) fn math_close_absent(&self, start: usize) -> bool {
        self.block_end_scan_cache
            .borrow()
            .closer_scan
            .math_absence(start)
    }

    /// Record that the Wikidot math scan from `start` stepped to `InputEnd`
    /// without consuming a comment or raw region.
    pub(crate) fn record_math_close_absent(&self, start: usize) {
        self.block_end_scan_cache
            .borrow_mut()
            .closer_scan
            .record_math_absence(start);
    }
    /// Whether the Wikidot link-target token walk that starts at byte
    /// `start` is certain to step over every remaining token and fail at
    /// `InputEnd` with `EndOfInput`.
    ///
    /// `collect_comment_elided_keep_from_inner` only stops early at a
    /// close/invalid token, a valid comment, or a fused `[[iftags]]`
    /// segment. `triple` selects the triple-link URL walk; between the two
    /// callers the stopping tokens are:
    ///
    /// - `Whitespace` (' ' or '\t'), `Pipe` ('|');
    /// - `LineBreak` / `ParagraphBreak` ('\n' or '\r', or the
    ///   tokenizer's terminal-backslash `LineBreak`);
    /// - `RightLink` (`]]]`, needs ']');
    /// - a valid comment or a fused segment, both of which need a later
    ///   ']' to close;
    /// - a generated/runtime token, which needs a delayed marker.
    ///
    /// If none of those can occur at or after `start`, every token falls
    /// into the step-over arm and the walk reaches `InputEnd` and returns
    /// the same `EndOfInput` error (at the trailing token, under the same
    /// rule), so the caller can reproduce it without walking.
    ///
    /// The scan is monotone-cached per walk: successive walk starts only
    /// move forward, so a document is scanned once per kind.
    pub(crate) fn link_target_reaches_input_end(
        &self,
        start: usize,
        triple: bool,
    ) -> bool {
        const SINGLE_STOP: &[u8] = b" \t\n\r\\]";
        const TRIPLE_STOP: &[u8] = b"|]\n\r\\";
        if self.delayed_markers.range(start..).next().is_some() {
            return false;
        }
        let (stop, slot) = if triple {
            (TRIPLE_STOP, 1)
        } else {
            (SINGLE_STOP, 0)
        };
        let mut cache = self.block_end_scan_cache.borrow_mut();
        find_stop_byte_forward(
            self.full_text().inner(),
            &mut cache.link_target_stop[slot],
            stop,
            start,
        )
        .is_none()
    }

    /// Whether `single_link_raw_forbids_link`'s probe walk that starts at
    /// byte `start` is certain to reach `InputEnd` and return `false`.
    ///
    /// That walk only returns `true` through a successful `RULE_RAW`
    /// attempt, and it only attempts `RULE_RAW` at a `Raw`/`LeftRaw`
    /// token, which the tokenizer only produces from `@@`/`@<` bytes.
    /// Without a `@` byte at or after `start` the walk can only step to
    /// `InputEnd` (or to a token that already returns `false`), and it
    /// cannot error, so it returns `false`.
    pub(crate) fn raw_forbids_link_reaches_input_end(&self, start: usize) -> bool {
        let mut cache = self.block_end_scan_cache.borrow_mut();
        find_stop_byte_forward(
            self.full_text().inner(),
            &mut cache.raw_forbids_stop,
            b"@",
            start,
        )
        .is_none()
    }
}

/// First byte from `stop` at or after `start`, reusing a monotone scan
/// slot when the previous search already covers `start`.
fn find_stop_byte_forward(
    source: &str,
    slot: &mut Option<(usize, Option<usize>)>,
    stop: &[u8],
    start: usize,
) -> Option<usize> {
    if let Some((scanned_from, found)) = *slot
        && scanned_from <= start
        && found.is_none_or(|found| found >= start)
    {
        return found;
    }
    let bytes = source.as_bytes();
    let found = (start..bytes.len()).find(|&offset| stop.contains(&bytes[offset]));
    *slot = Some((start, found));
    found
}

#[derive(Debug, Copy, Clone)]
pub struct ParserMutableState {
    footnote_index: usize,
    html_block_index: usize,
    code_block_index: usize,
    table_of_contents_index: usize,
    bibliography_index: usize,
    has_footnote_block: bool,
}

#[inline]
fn make_shared_vec<T>() -> Rc<RefCell<Vec<T>>> {
    Rc::new(RefCell::new(Vec::new()))
}

#[cfg(test)]
#[derive(Debug)]
struct TestLogger;

#[cfg(test)]
impl log::Log for TestLogger {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, _record: &log::Record<'_>) {}

    fn flush(&self) {}
}

#[cfg(test)]
static TEST_LOGGER: TestLogger = TestLogger;

#[cfg(test)]
static TEST_LOGGER_INIT: std::sync::Once = std::sync::Once::new();

#[cfg(test)]
fn enable_test_logging() {
    TEST_LOGGER_INIT.call_once(|| {
        let _ = log::set_logger(&TEST_LOGGER);
        log::set_max_level(log::LevelFilter::Trace);
    });
}

#[test]
fn parser_newline_flag() {
    use crate::layout::Layout;
    use crate::settings::WikitextMode;

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

    macro_rules! test {
        ($input:expr, $expected_steps:expr $(,)?) => {{
            let tokens = crate::tokenize($input);
            let mut parser = Parser::new(&tokens, &page_info, &settings);
            let mut actual_steps = Vec::new();

            while let Ok(_) = parser.step() {
                actual_steps.push(parser.start_of_line());
            }

            actual_steps.pop();

            assert_eq!(
                &actual_steps, &$expected_steps,
                "Series of start-of-line flags does not match expected",
            );
        }};
    }

    test!("A", [true]);
    test!("A\nB C", [true, false, true, false, false]);
    test!(
        "A\nB\n\nC D\nE",
        [true, false, true, false, true, false, false, false, true],
    );
    test!(
        "\nA\n\nB\n\n\nC D",
        [true, true, false, true, false, true, false, false],
    );
}

#[test]
fn quote_body_cursor_helpers_cover_split_residual_boundary_and_cache_states() {
    use crate::layout::Layout;
    use crate::settings::WikitextMode;

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

    let tokens = crate::tokenize("> > body");
    let mut parser = Parser::new(&tokens, &page_info, &settings);
    parser.step().unwrap();
    parser.install_quote_body_cursor(2);
    assert!(parser.quote_body_needs_prefix());
    assert_eq!(
        parser.prepare_quote_body_line().unwrap(),
        QuoteBodyLineStatus::Prepared
    );
    assert_eq!(parser.current().slice, "body");
    assert!(parser.quote_body_close_allowed_here());
    assert_eq!(parser.native_blockquote_depths(1), (1, 3));
    assert_eq!(
        parser.prepare_quote_body_line().unwrap(),
        QuoteBodyLineStatus::Prepared
    );

    let saved_cursor = parser.quote_body_cursor();
    parser.set_quote_body_cursor(None);
    assert_eq!(
        parser.prepare_quote_body_line().unwrap(),
        QuoteBodyLineStatus::Inactive
    );
    parser.set_quote_body_cursor(saved_cursor);

    let tokens = crate::tokenize(">> body");
    let mut residual = Parser::new(&tokens, &page_info, &settings);
    residual.step().unwrap();
    residual.install_quote_body_cursor(1);
    assert_eq!(
        residual.prepare_quote_body_line().unwrap(),
        QuoteBodyLineStatus::Prepared
    );
    assert_eq!(residual.native_blockquote_depths(2), (1, 2));
    assert!(!residual.quote_body_close_allowed_here());

    let tokens = crate::tokenize("plain");
    let mut boundary = Parser::new(&tokens, &page_info, &settings);
    boundary.step().unwrap();
    boundary.install_quote_body_cursor(1);
    assert_eq!(
        boundary.prepare_quote_body_line().unwrap(),
        QuoteBodyLineStatus::Boundary
    );

    let tokens = crate::tokenize("plain");
    let mut consume_boundary = Parser::new(&tokens, &page_info, &settings);
    consume_boundary.step().unwrap();
    consume_boundary.install_quote_body_cursor(1);
    assert_eq!(
        crate::parsing::consume::consume(&mut consume_boundary)
            .unwrap_err()
            .kind(),
        ParseErrorKind::EndOfInput
    );

    let tokens = crate::tokenize("plain");
    let mut collect_boundary = Parser::new(&tokens, &page_info, &settings);
    collect_boundary.step().unwrap();
    collect_boundary.install_quote_body_cursor(1);
    assert_eq!(
        crate::parsing::collect::collect_consume(
            &mut collect_boundary,
            RULE_PAGE,
            &[],
            &[],
            None,
        )
        .unwrap_err()
        .kind(),
        ParseErrorKind::EndOfInput
    );

    let tokens = crate::tokenize("\n\n");
    let mut paragraph_boundary = Parser::new(&tokens, &page_info, &settings);
    paragraph_boundary.step().unwrap();
    paragraph_boundary.install_quote_body_cursor(1);
    assert_eq!(
        paragraph_boundary.prepare_quote_body_line().unwrap(),
        QuoteBodyLineStatus::Boundary
    );

    let tokens = crate::tokenize("> body");
    let mut nested = Parser::new(&tokens, &page_info, &settings);
    nested.step().unwrap();
    nested.set_native_blockquote_depth(Some(2));
    assert!(nested.in_native_blockquote_line());
    assert_eq!(nested.native_blockquote_depths(1), (1, 3));

    let key = ("test-rule", 2, true, 17);
    nested.cache_quote_scan_outcomes(
        "test-rule",
        2,
        true,
        &[17],
        QuoteScanOutcome::HasCandidateClose,
    );
    assert_eq!(
        nested.quote_scan_outcome(key),
        Some(QuoteScanOutcome::HasCandidateClose)
    );
}

#[test]
fn parser_token_pair_conditions_cover_all_outcomes() {
    use crate::layout::Layout;
    use crate::settings::WikitextMode;

    enable_test_logging();

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize("a");
    let mut parser = Parser::new(&tokenization, &page_info, &settings);

    assert!(parser.evaluate(ParseCondition::token_pair(
        Token::InputStart,
        Token::Identifier,
    )));
    assert!(!parser.evaluate(ParseCondition::token_pair(
        Token::Identifier,
        Token::InputEnd,
    )));

    parser.step().expect("identifier should follow input start");
    assert!(!parser.evaluate(ParseCondition::token_pair(
        Token::Identifier,
        Token::Whitespace,
    )));

    parser.step().expect("input end should follow identifier");
    assert!(!parser.evaluate(ParseCondition::token_pair(
        Token::InputEnd,
        Token::Identifier,
    )));
    assert!(parser.evaluate_any(&[ParseCondition::current(Token::InputEnd)]));
    assert!(!parser.evaluate_any(&[]));

    let tokenization = crate::tokenize("a b");
    let parser = Parser::new(&tokenization, &page_info, &settings);
    assert_eq!(
        parser.next_two_tokens(),
        (Token::InputStart, Some(Token::Identifier)),
    );
    assert_eq!(
        parser.next_three_tokens(),
        (
            Token::InputStart,
            Some(Token::Identifier),
            Some(Token::Whitespace),
        ),
    );
}

#[test]
fn parser_mutable_state_helpers_reset_and_remove_collections() {
    use crate::layout::Layout;
    use crate::settings::WikitextMode;

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize("body");
    let mut parser = Parser::new(&tokenization, &page_info, &settings);
    let state = parser.get_mutable_state();

    parser.push_footnote(vec![text!("note")]);
    parser.push_html_block(cow!("html"));
    parser.push_code_block(CodeBlock {
        contents: cow!("code"),
        language: Some(cow!("rust")),
        name: Some(cow!("sample")),
        wikidot_block_open_elements: Vec::new(),
        wikidot_label_spans: Vec::new(),
    });
    parser.push_table_of_contents_entry(HeadingLevel::One, &[text!("Heading")]);
    parser.set_footnote_block();
    let mut bibliography = Bibliography::new();
    bibliography.add(cow!("alpha"), vec![text!("reference")]);
    assert_eq!(parser.push_bibliography(bibliography), 0);

    parser.reset_mutable_state(state);

    assert_eq!(parser.footnote_count(), 0);
    assert!(parser.remove_html_blocks().is_empty());
    assert!(parser.remove_code_blocks().is_empty());
    assert!(parser.remove_table_of_contents().is_empty());
    assert!(parser.remove_bibliographies().is_empty());
    assert!(!parser.has_footnote_block);

    parser.push_footnote(vec![text!("after")]);
    assert_eq!(parser.footnote_count(), 1);
    parser.truncate_footnotes(0);
    assert_eq!(parser.footnote_count(), 0);

    parser.push_html_block(cow!("html"));
    parser.push_code_block(CodeBlock {
        contents: cow!("code"),
        language: None,
        name: None,
        wikidot_block_open_elements: Vec::new(),
        wikidot_label_spans: Vec::new(),
    });
    parser.push_table_of_contents_entry(HeadingLevel::Two, &[text!("Heading")]);

    assert_eq!(parser.remove_html_blocks(), vec![cow!("html")]);
    assert_eq!(
        parser.remove_code_blocks(),
        vec![CodeBlock {
            contents: cow!("code"),
            language: None,
            name: None,
            wikidot_block_open_elements: Vec::new(),
            wikidot_label_spans: Vec::new(),
        }],
    );
    assert_eq!(
        parser.remove_table_of_contents(),
        vec![(1, String::from("Heading"))],
    );

    let mut bibliography = Bibliography::new();
    bibliography.add(cow!("alpha"), vec![text!("reference")]);
    assert_eq!(parser.push_bibliography(bibliography), 0);

    let bibliographies = parser.remove_bibliographies();
    assert_eq!(bibliographies.next_index(), 1);
    assert!(parser.remove_bibliographies().is_empty());
}

#[test]
fn parser_table_of_contents_entry_resolves_static_delayed_markers() {
    use crate::delayed::{DelayedElement, GeneratedKind, SlotId};
    use crate::layout::Layout;
    use crate::settings::WikitextMode;

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize("body");
    let mut parser = Parser::new(&tokenization, &page_info, &settings);

    parser.push_table_of_contents_entry(
        HeadingLevel::One,
        &[
            text!("Alp"),
            Element::Delayed(DelayedElement::typography_boundary()),
            text!("ha"),
            Element::Delayed(DelayedElement::typography_boundary()),
        ],
    );

    parser.push_table_of_contents_entry(
        HeadingLevel::Two,
        &[
            text!("Be"),
            Element::Delayed(DelayedElement::suppressed(&[])),
            text!("d"),
        ],
    );

    parser.push_table_of_contents_entry(
        HeadingLevel::One,
        &[Element::Delayed(DelayedElement::active(
            SlotId::new(1),
            GeneratedKind::PageLink,
        ))],
    );

    assert_eq!(
        parser.remove_table_of_contents(),
        vec![
            (0, String::from("Alpha")),
            (1, String::from("Bed")),
            (0, String::new()),
        ],
    );
}

#[test]
fn parser_append_shared_items_and_optional_spaces_cover_helpers() {
    use crate::layout::Layout;
    use crate::settings::WikitextMode;

    enable_test_logging();

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize(" \n\n=tail");
    let mut parser = Parser::new(&tokenization, &page_info, &settings);
    parser.step().expect("whitespace should follow input start");

    parser
        .get_optional_spaces_any()
        .expect("all optional spacing tokens should be consumed");
    assert_eq!(parser.current().token, Token::Identifier);
    assert_eq!(parser.current().slice, "tail");

    parser.step().expect("input end should follow identifier");
    assert_eq!(parser.next_two_tokens(), (Token::InputEnd, None));

    let mut html_blocks = vec![cow!("html")];
    let mut code_blocks = vec![CodeBlock {
        contents: cow!("code"),
        language: None,
        name: None,
        wikidot_block_open_elements: Vec::new(),
        wikidot_label_spans: Vec::new(),
    }];
    let mut table_of_contents = vec![(2, String::from("Heading"))];
    let mut footnotes = vec![vec![text!("note")]];
    let mut bibliography = Bibliography::new();
    bibliography.add(cow!("alpha"), vec![text!("reference")]);
    let mut bibliographies = BibliographyList::new();
    bibliographies.push(bibliography);

    parser.append_shared_items(
        &mut html_blocks,
        &mut code_blocks,
        &mut table_of_contents,
        &mut footnotes,
        &mut bibliographies,
    );

    assert!(html_blocks.is_empty());
    assert!(code_blocks.is_empty());
    assert!(table_of_contents.is_empty());
    assert!(footnotes.is_empty());
    assert!(bibliographies.is_empty());

    assert_eq!(parser.remove_html_blocks(), vec![cow!("html")]);
    assert_eq!(
        parser.remove_code_blocks(),
        vec![CodeBlock {
            contents: cow!("code"),
            language: None,
            name: None,
            wikidot_block_open_elements: Vec::new(),
            wikidot_label_spans: Vec::new(),
        }],
    );
    assert_eq!(
        parser.remove_table_of_contents(),
        vec![(2, String::from("Heading"))],
    );
    assert_eq!(parser.remove_footnotes(), vec![vec![text!("note")]]);
    assert_eq!(parser.remove_bibliographies().next_index(), 1);
}

#[test]
fn parser_state_fits_the_bounded_deep_parse_stack_budget() {
    let size = std::mem::size_of::<Parser<'static, 'static>>();
    assert!(
        size <= 336,
        "Parser grew to {size} bytes; deep recursive parsing requires at most 336 bytes",
    );
}

#[cfg(test)]
fn render_at_recursion_limit(
    input: &str,
    max_recursion_depth: usize,
    remember_failures: bool,
) -> (String, Vec<ParseError>) {
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    let input = input.to_owned();
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            RULE_ATTEMPT_MEMO_DISABLED.with(|disabled| disabled.set(!remember_failures));
            let page_info = PageInfo::dummy();
            let settings =
                WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
            let mut input = input;
            crate::preprocess(&mut input);
            let tokenization = crate::tokenize(&input);
            let (tree, errors) = super::parse_on_current_stack(
                &tokenization,
                &page_info,
                &settings,
                max_recursion_depth,
            )
            .into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;
            (html, errors)
        })
        .expect("parse thread starts")
        .join()
        .expect("parse thread finishes")
}

#[test]
fn remembered_rule_failures_leave_every_parse_unchanged() {
    let stanza = |owner: &str, body: &str| {
        let (open, close) = match owner {
            "div" => ("[[div]]", "[[/div]]"),
            _ => ("[[>]]", "[[/>]]"),
        };
        format!("{open}\n{body}\n{close}\n{{{{TEXT}}}}\n")
    };
    let crossed = "##63ff54|{{@@ >  @@boot##}}";
    let cases = [
        stanza("right", crossed).repeat(10),
        stanza("div", crossed).repeat(10),
        format!(
            "{}\nafter **bold** text\n",
            stanza("right", crossed).repeat(10)
        ),
        format!("* {crossed}\n").repeat(8),
        format!("> {crossed}\n> quoted\n").repeat(6),
        format!("[[footnote]]{crossed}[[/footnote]] text\n").repeat(6),
        format!("||{crossed}||cell||\n").repeat(6),
        "* //**A:**// x //**B**// y\n".repeat(6),
        format!(
            "{crossed}\n--{}\n{}text\n{}b--\n",
            "word ".repeat(20),
            "[[div]]\n".repeat(4),
            "[[/div]]\n".repeat(4),
        ),
        format!(
            "{crossed}\n--{}\n{}text\n{}b--\n",
            "word ".repeat(20),
            "[[div]]\n".repeat(8),
            "[[/div]]\n".repeat(8),
        ),
        concat!(
            "[[/=]]\n[[<]]\n",
            "[[size 140%]][[collapsible show=\"+ Show\" hide=\"- Hide\"]][[/size]]\n",
            "[[/=]]\n\n[[tabview]]\n[[tab A]]\n[[<]]\n\n**Description:** x //y//\n",
            "[[<]]\n\n**Status:** x\n\n[[/tab]]\n[[tab B]]\n[[<]]\n**Status:** x\n\n",
            "[[tab C]]\n[[<]]\n\n**Status:** x\n[[/<]]\n[[/tab]]\n",
        )
        .to_owned(),
    ];

    for input in &cases {
        for max_recursion_depth in [6, 8, 10, 12, 16, 24, 40, DEFAULT_MAX_RECURSION_DEPTH]
        {
            assert_eq!(
                render_at_recursion_limit(input, max_recursion_depth, true),
                render_at_recursion_limit(input, max_recursion_depth, false),
                "{input:?} at recursion limit {max_recursion_depth}",
            );
        }
    }
}
