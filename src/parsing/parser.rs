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
use super::condition::ParseCondition;
use super::hidden_body::HiddenBodyBoundary;
use super::prelude::*;
use super::rule::Rule;
use crate::data::PageInfo;
use crate::delayed::{GeneratedInput, elements_contain_delayed};
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

mod wikidot_state;
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
    has_footnote_block: bool,
    start_of_line: bool,
    wikidot: WikidotState,
}

#[cfg(test)]
thread_local! {
    static RULE_ATTEMPT_MEMO_DISABLED: Cell<bool> = const { Cell::new(false) };
}

#[derive(Debug, Default)]
struct BlockScanCache {
    peak_depth: Cell<usize>,
    rule_attempt_failures: HashMap<RuleAttemptKey, (ParseError, usize)>,
    rule_attempt_failure_positions: Vec<bool>,
    body_end_outcomes: BTreeMap<ContextualBlockEndScanKey, bool>,
    underclosed_block_failures: BTreeMap<(&'static str, usize), ()>,
    deterministic_block_failures: BTreeMap<(&'static str, usize), ParseError>,
    partial_rejections: BTreeMap<PartialRejectionKey, ParseError>,
    wikidot_tabview_failures: BTreeMap<(usize, bool), ParseError>,
    wikidot_collapsible_failures: BTreeMap<usize, ParseError>,
    wikidot_div_bounded_span: BTreeMap<usize, bool>,
    lost_owner_outcomes: BTreeMap<LostOwnerScanKey, bool>,
    span_alias_close_scores: BTreeMap<usize, bool>,
    literal_ranges: Option<Vec<Range<usize>>>,
    // Designated `[[/iftags]]` byte offset for the Wikidot pairing gather
    // currently in progress (#792); see `Parser::wikidot_iftags_body_end`.
    wikidot_iftags_body_end: Cell<Option<usize>>,
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

/// Parser for a set of tokens.
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
    block_end_scan_cache: Rc<RefCell<BlockScanCache>>,
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
    has_footnote_block: bool,
    start_of_line: bool,
    native_blockquote_depth: Option<usize>,
    quote_body_cursor: Option<QuoteBodyCursor>,
    wikidot: WikidotState,
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
            block_end_scan_cache: Rc::new(RefCell::new(BlockScanCache::default())),
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
            has_footnote_block: false,
            start_of_line: true,
            native_blockquote_depth: None,
            quote_body_cursor: None,
            wikidot: WikidotState::default(),
        }
    }

    // Wikidot [[iftags]] pairing state (legacy layout only, #792). While a
    // matched iftags body is gathered, this is the byte offset of the one
    // `[[/iftags]]` head the pairing search designated as the body's end;
    // other iftags end blocks inside the span are literal text. It lives in
    // the shared scan cache, not in the parser value, because the parser
    // state has a hard size budget for deep recursive parsing; access is
    // stack-disciplined (the owning iftags rule sets it around its body
    // gather and restores the previous value, and nested iftags rules
    // consume their unit literally without touching it, via
    // `WikidotState`'s iftags-span flag).
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

    // Whether this parser is currently consuming blockquote body content
    // under a quote body cursor. Wikidot [[iftags]] pairing (#792) does
    // not apply there: a pairing span must not cross a quote boundary, so
    // gates in quote bodies keep the legacy nesting behavior.
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

    #[rustfmt::skip]
    pub(crate) fn quote_body_needs_prefix(&self) -> bool {
        matches!(self.quote_body_cursor, Some(QuoteBodyCursor { line_state: QuoteBodyLineState::NeedPrefix, .. }))
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

    fn rule_attempt_key(&self, rule: &'static str) -> Option<RuleAttemptKey> {
        #[cfg(test)]
        if RULE_ATTEMPT_MEMO_DISABLED.with(Cell::get) {
            return None;
        }
        if self.discarding_hidden_body
            || !self.hidden_body_boundaries.is_empty()
            || self.native_blockquote_depth.is_some()
            || self.quote_body_cursor.is_some()
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
            return crate::wikidot_code::candidate_at_owned_opener(source, opener_start);
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

    /// Gets the parser's mutable state to enable resetting later if needed.
    ///
    /// See `reset_mutable_state()`.
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

    /// Reset the parser's mutable state to the point described in the struct.
    ///
    /// This structure should be retrieved from `get_mutable_state()`.
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

    /// Add heading element to table of contents.
    pub fn push_table_of_contents_entry(
        &mut self,
        heading: HeadingLevel,
        name_elements: &[Element],
    ) {
        let level = usize::from(heading.value()) - 1;

        let name = if elements_contain_delayed(name_elements) {
            String::new()
        } else {
            TextRender.render_partial(name_elements, self.page_info, self.settings, 0)
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
            ParseCondition::TokenPair(current, next) => {
                if self.current.token != current {
                    return false;
                }

                match self.look_ahead(0) {
                    Some(actual) => actual.token == next,
                    None => false,
                }
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

    /// Move the token pointer forward one step.
    ///
    /// # Returns
    /// Returns the new current token.
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

    /// Move the token pointer forward `count` steps.
    #[inline]
    pub fn step_n(&mut self, count: usize) -> Result<(), ParseError> {
        for _ in 0..count {
            self.step()?;
        }

        Ok(())
    }

    /// Look for the token `offset + 1` beyond the current one.
    ///
    /// For instance, submitting `0` will yield the first item of `parser.remaining()`.
    #[inline]
    pub fn look_ahead(&self, offset: usize) -> Option<&'r ExtractedToken<'t>> {
        self.remaining.get(offset)
    }

    /// Like `look_ahead`, except returns an error if the token isn't found.
    #[inline]
    pub fn look_ahead_err(
        &self,
        offset: usize,
    ) -> Result<&'r ExtractedToken<'t>, ParseError> {
        self.look_ahead(offset)
            .ok_or_else(|| self.make_err(ParseErrorKind::EndOfInput))
    }

    /// Retrieves the current and next tokens.
    pub fn next_two_tokens(&self) -> (Token, Option<Token>) {
        let current = self.current.token;
        let next = self.look_ahead(0).map(|token| token.token);
        std::convert::identity((current, next))
    }

    /// Retrieves the current, second, and third tokens.
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
}

/// This struct stores the state of the mutable fields in `Parser`.
///
/// This way, on rule failure, we can revert to the state these
/// fields were in prior to rule execution.
///
/// See the "revert" tests for examples of how this reset data is
/// needed to ensure proper AST formation:
/// * `test/footnotes/revert`
/// * `test/html/revert`
/// * `test/code/revert`
/// * `test/toc/revert`
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
    });
    parser.push_table_of_contents_entry(HeadingLevel::Two, &[text!("Heading")]);

    assert_eq!(parser.remove_html_blocks(), vec![cow!("html")]);
    assert_eq!(
        parser.remove_code_blocks(),
        vec![CodeBlock {
            contents: cow!("code"),
            language: None,
            name: None,
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
        size <= 320,
        "Parser grew to {size} bytes; deep recursive parsing requires at most 320 bytes",
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
