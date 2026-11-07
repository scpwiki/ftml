/*
 * parsing/rule/impls/block/parser.rs
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

use super::BlockRule;
use super::arguments::Arguments;
use crate::layout::Layout;
use crate::parsing::collect::{
    CommentElidedText, collect_comment_elided_keep, collect_text_keep,
    collect_text_resolved_fused, collect_text_resolved_fused_tracked,
    collect_text_resolved_fused_tracked_quoted, consume_iftags_segment,
    consume_valid_comment,
};
use crate::parsing::condition::ParseCondition;
use crate::parsing::consume::consume;
use crate::parsing::parser::{
    QuoteBodyLineStatus, QuoteScanOutcome, WikidotLiClose, WikidotLiHead,
    wikidot_first_block_close_after_from_heads,
};
use crate::parsing::{
    ExtractedToken, ParagraphSplitEntry, ParagraphSplitKind, ParseError, ParseErrorKind,
    ParseResult, Parser, Token, gather_paragraphs, trim_wikidot_ascii,
};
use crate::tree::{AttributeMap, Container, ContainerType, Element, Elements};
use crate::wikidot_raw_escape::{PLACEHOLDER_CLOSE, PLACEHOLDER_OPEN, raw_escapes};
use regex::Regex;
use std::borrow::Cow;
use std::sync::LazyLock;

static ARGUMENT_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9_\-]+").unwrap());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockBodyStart {
    Inline,
    NextPhysicalLine,
}

#[derive(Debug)]
pub(crate) enum WikidotHeadSpaceRegion {
    SingleSeparator,
    WhitespaceOnly,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GateHeadClose {
    Block,
    Fused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscardedBodyOutcome {
    Discarded,
    FailedClosedThroughInputEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WikidotLiCloseDisposition {
    End,
    Silent,
    Literal,
}

/// Token window the `[[iftags]]` pairing scan may look at, matching the
/// window the previous pairing dynamic program was built over.
const IFTAGS_PAIRING_WINDOW: usize = 8192;

#[cfg(test)]
thread_local! {
    static IFTAGS_PAIRING_SCAN_STEPS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn reset_iftags_pairing_scan_steps() {
    IFTAGS_PAIRING_SCAN_STEPS.with(|steps| steps.set(0));
}

#[cfg(test)]
fn iftags_pairing_scan_steps() -> usize {
    IFTAGS_PAIRING_SCAN_STEPS.with(std::cell::Cell::get)
}

#[cfg(test)]
fn count_iftags_pairing_scan_step() {
    IFTAGS_PAIRING_SCAN_STEPS.with(|steps| steps.set(steps.get() + 1));
}

/// The token window `[[iftags]]` pairing scans, materialized lazily.
///
/// The pairing scan walks the body left to right and only needs the tokens it
/// visits, so the window grows on demand instead of copying the whole
/// 8192-token window once per gate head. Extension stops at exactly the same
/// places the old eager window did (input end, the 8192-token cap, or a failed
/// step), so truncated windows keep behaving identically.
struct IftagsPairingWindow<'r, 't> {
    tokens: Vec<&'r ExtractedToken<'t>>,
    scan: Parser<'r, 't>,
    done: bool,
}

impl<'r, 't> IftagsPairingWindow<'r, 't> {
    fn new(parser: &Parser<'r, 't>) -> Self {
        Self {
            tokens: Vec::new(),
            scan: parser.clone(),
            done: false,
        }
    }

    /// Append tokens until `index` is inside the window or it cannot grow.
    fn extend_to(&mut self, index: usize) {
        while self.tokens.len() <= index && !self.done {
            let token = self.scan.current();
            let at_end = token.token == Token::InputEnd;
            self.tokens.push(token);
            #[cfg(test)]
            count_iftags_pairing_scan_step();
            if at_end || self.tokens.len() >= IFTAGS_PAIRING_WINDOW {
                self.done = true;
            } else if self.scan.step().is_err() {
                self.done = true;
            }
        }
    }

    /// Materialize the rest of the window, for the span probes that scan it.
    fn fill(&mut self) {
        self.extend_to(usize::MAX);
    }

    /// Materialize token `index` when the window can still grow; true if
    /// the token is inside the window afterwards.
    fn ensure(&mut self, index: usize) -> bool {
        self.extend_to(index);
        self.tokens.get(index).is_some()
    }

    fn token(&self, index: usize) -> &'r ExtractedToken<'t> {
        self.tokens[index]
    }
}

/// [`named_block_head_end`] over a lazily materialized window.
///
/// The head probe walks until the head closes (or until a token that cannot
/// appear in a head), so its lookahead is not bounded by a fixed constant;
/// the window grows with it instead of being copied up front.
fn lazy_named_block_head_end<'r, 't>(
    window: &mut IftagsPairingWindow<'r, 't>,
    start: usize,
    name: &str,
) -> Option<usize> {
    if !window.ensure(start) || window.token(start).token != Token::LeftBlock {
        return None;
    }
    let mut i = start + 1;
    if !window.ensure(i) {
        return None;
    }
    if window.token(i).token == Token::Whitespace {
        i += 1;
    }
    if !window.ensure(i) {
        return None;
    }
    let ident = window.token(i);
    if ident.token != Token::Identifier || !ident.slice.eq_ignore_ascii_case(name) {
        return None;
    }
    i += 1;
    loop {
        if !window.ensure(i) {
            return None;
        }
        match window.token(i).token {
            Token::RightBlock => return Some(i + 1),
            Token::LeftBlock
            | Token::LeftBlockEnd
            | Token::InputEnd
            | Token::ParagraphBreak => return None,
            _ => i += 1,
        }
    }
}

/// [`named_block_end_head_end`] over a lazily materialized window.
fn lazy_named_block_end_head_end<'r, 't>(
    window: &mut IftagsPairingWindow<'r, 't>,
    start: usize,
    name: &str,
) -> Option<usize> {
    if !window.ensure(start) || window.token(start).token != Token::LeftBlockEnd {
        return None;
    }
    let mut i = start + 1;
    if !window.ensure(i) {
        return None;
    }
    if window.token(i).token == Token::Whitespace {
        i += 1;
    }
    if !window.ensure(i) {
        return None;
    }
    let ident = window.token(i);
    if ident.token != Token::Identifier || !ident.slice.eq_ignore_ascii_case(name) {
        return None;
    }
    i += 1;
    if !window.ensure(i) {
        return None;
    }
    if window.token(i).token == Token::Whitespace {
        i += 1;
    }
    if !window.ensure(i) {
        return None;
    }
    if window.token(i).token == Token::RightBlock {
        Some(i + 1)
    } else {
        None
    }
}

/// [`iftags_end_head_end`] over a lazily materialized window.
fn lazy_iftags_end_head_end<'r, 't>(
    window: &mut IftagsPairingWindow<'r, 't>,
    start: usize,
) -> Option<usize> {
    if let Some(end) = lazy_named_block_end_head_end(window, start, "iftags") {
        return Some(end);
    }
    if !window.ensure(start) || window.token(start).token != Token::LeftBlockEnd {
        return None;
    }
    let mut i = start + 1;
    if !window.ensure(i) {
        return None;
    }
    if window.token(i).token == Token::Whitespace {
        i += 1;
    }
    if !window.ensure(i) {
        return None;
    }
    let ident = window.token(i);
    if ident.token != Token::Identifier || !ident.slice.eq_ignore_ascii_case("iftags") {
        return None;
    }
    i += 1;
    if !window.ensure(i) {
        return None;
    }
    if window.token(i).token == Token::Whitespace {
        i += 1;
    }
    if !window.ensure(i) {
        return None;
    }
    let close = window.token(i);
    if close.token == Token::RightLink && close.slice == "]]]" {
        Some(i + 1)
    } else {
        None
    }
}

/// [`protected_span_end`] over a lazily materialized window.
///
/// Only raw spans (and raw block heads) scan ahead, and those need the whole
/// window; every other token ends the span check immediately, so a gate-heavy
/// page never pays for the window it does not look at.
fn lazy_protected_span_end<'r, 't>(
    source: &str,
    window: &mut IftagsPairingWindow<'r, 't>,
    start: usize,
) -> Option<usize> {
    if !window.ensure(start) {
        return None;
    }
    match window.token(start).token {
        Token::Raw | Token::LeftRaw => {
            window.fill();
            protected_span_end(source, &window.tokens, start)
        }
        Token::LeftBlock => {
            if !raw_block_span_name(window, start) {
                return None;
            }
            window.fill();
            protected_span_end(source, &window.tokens, start)
        }
        _ => None,
    }
}

/// Whether the block head at `start` opens one of the raw block spans
/// [`raw_block_span_end`] handles (same name test, without the span scan).
fn raw_block_span_name<'r, 't>(
    window: &mut IftagsPairingWindow<'r, 't>,
    start: usize,
) -> bool {
    let mut i = start + 1;
    if !window.ensure(i) {
        return false;
    }
    if window.token(i).token == Token::Whitespace {
        i += 1;
    }
    if !window.ensure(i) {
        return false;
    }
    let ident = window.token(i);
    if ident.token != Token::Identifier {
        return false;
    }
    let name = ident.slice;
    name.eq_ignore_ascii_case("code")
        || name.eq_ignore_ascii_case("html")
        || name.eq_ignore_ascii_case("module")
}

fn named_block_head_end(
    tokens: &[&ExtractedToken<'_>],
    start: usize,
    name: &str,
) -> Option<usize> {
    if tokens.get(start)?.token != Token::LeftBlock {
        return None;
    }
    let mut i = start + 1;
    if tokens.get(i)?.token == Token::Whitespace {
        i += 1;
    }
    let ident = tokens.get(i)?;
    if ident.token != Token::Identifier || !ident.slice.eq_ignore_ascii_case(name) {
        return None;
    }
    i += 1;
    loop {
        match tokens.get(i)?.token {
            Token::RightBlock => return Some(i + 1),
            Token::LeftBlock
            | Token::LeftBlockEnd
            | Token::InputEnd
            | Token::ParagraphBreak => return None,
            _ => i += 1,
        }
    }
}

fn named_block_end_head_end(
    tokens: &[&ExtractedToken<'_>],
    start: usize,
    name: &str,
) -> Option<usize> {
    if tokens.get(start)?.token != Token::LeftBlockEnd {
        return None;
    }
    let mut i = start + 1;
    if tokens.get(i)?.token == Token::Whitespace {
        i += 1;
    }
    let ident = tokens.get(i)?;
    if ident.token != Token::Identifier || !ident.slice.eq_ignore_ascii_case(name) {
        return None;
    }
    i += 1;
    if tokens.get(i)?.token == Token::Whitespace {
        i += 1;
    }
    if tokens.get(i)?.token == Token::RightBlock {
        Some(i + 1)
    } else {
        None
    }
}

fn iftags_block_head_end(tokens: &[&ExtractedToken<'_>], start: usize) -> Option<usize> {
    named_block_head_end(tokens, start, "iftags")
}

fn iftags_end_head_end(tokens: &[&ExtractedToken<'_>], start: usize) -> Option<usize> {
    if let Some(end) = named_block_end_head_end(tokens, start, "iftags") {
        return Some(end);
    }

    if tokens.get(start)?.token != Token::LeftBlockEnd {
        return None;
    }
    let mut i = start + 1;
    if tokens.get(i)?.token == Token::Whitespace {
        i += 1;
    }
    let ident = tokens.get(i)?;
    if ident.token != Token::Identifier || !ident.slice.eq_ignore_ascii_case("iftags") {
        return None;
    }
    i += 1;
    if tokens.get(i)?.token == Token::Whitespace {
        i += 1;
    }
    let close = tokens.get(i)?;
    if close.token == Token::RightLink && close.slice == "]]]" {
        Some(i + 1)
    } else {
        None
    }
}

fn span_token_at_line_start(
    source: &str,
    tokens: &[&ExtractedToken<'_>],
    index: usize,
) -> bool {
    let start = tokens[index].span.start;
    start == 0 || matches!(source.as_bytes()[start - 1], b'\n' | b'\r')
}

fn span_closer_ends_construct(tokens: &[&ExtractedToken<'_>], index: usize) -> bool {
    match tokens.get(index) {
        None => true,
        Some(token) => matches!(
            token.token,
            Token::Whitespace
                | Token::LineBreak
                | Token::ParagraphBreak
                | Token::InputEnd
        ),
    }
}

fn span_token_ends_line(tokens: &[&ExtractedToken<'_>], index: usize) -> bool {
    match tokens.get(index) {
        None => true,
        Some(token) => match token.token {
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => true,
            Token::Whitespace => span_token_ends_line(tokens, index + 1),
            _ => false,
        },
    }
}

fn protected_span_end(
    source: &str,
    tokens: &[&ExtractedToken<'_>],
    start: usize,
) -> Option<usize> {
    match tokens.get(start)?.token {
        Token::Raw => {
            let mut i = start + 1;
            while i < tokens.len() {
                match tokens[i].token {
                    Token::Raw => return Some(i + 1),
                    Token::LineBreak | Token::ParagraphBreak => return None,
                    _ => i += 1,
                }
            }
            None
        }
        Token::LeftRaw => {
            let mut i = start + 1;
            while i < tokens.len() {
                match tokens[i].token {
                    Token::RightRaw => return Some(i + 1),
                    Token::LineBreak | Token::ParagraphBreak => return None,
                    _ => i += 1,
                }
            }
            None
        }
        Token::LeftBlock => raw_block_span_end(source, tokens, start),
        _ => None,
    }
}

pub(crate) fn head_word_never_resolves(block: &BlockRule) -> bool {
    matches!(
        block.name,
        "block-code"
            | "block-raw"
            | "block-html"
            | "block-math"
            | "block-module"
            | "block-iftags"
            | "block-ifcategory"
    )
}

pub(crate) fn head_seam_forms_block(block: &BlockRule, layout: Layout) -> bool {
    if block.name == "block-math" {
        return layout.legacy();
    }
    if head_word_never_resolves(block) {
        return false;
    }
    !layout.legacy()
        || matches!(
            block.name,
            "block-footnote"
                | "block-footnote-block"
                | "block-div"
                | "block-table"
                | "block-table-row"
                | "block-table-cell-regular"
                | "block-table-cell-header"
                | "block-span"
                | "block-size"
                | "block-collapsible"
                | "block-tabview"
                | "block-tab"
                | "block-iframe"
                | "block-image"
                | "block-bibliography"
                | "block-gallery"
                | "block-embedvideo"
                | "block-list-unordered"
                | "block-list-ordered"
                | "block-list-item"
                | "block-user"
                | "block-date"
                | "block-toc"
        )
}

fn raw_block_span_end(
    source: &str,
    tokens: &[&ExtractedToken<'_>],
    start: usize,
) -> Option<usize> {
    if tokens.get(start)?.token != Token::LeftBlock {
        return None;
    }
    let mut i = start + 1;
    if tokens.get(i)?.token == Token::Whitespace {
        i += 1;
    }
    let ident = tokens.get(i)?;
    if ident.token != Token::Identifier {
        return None;
    }
    let name = ident.slice;
    let line_owned =
        name.eq_ignore_ascii_case("code") || name.eq_ignore_ascii_case("html");
    let whole_line = name.eq_ignore_ascii_case("module");
    if !line_owned && !whole_line {
        return None;
    }
    i += 1;
    loop {
        match tokens.get(i)?.token {
            Token::RightBlock => {
                i += 1;
                break;
            }
            Token::LineBreak
            | Token::LeftBlock
            | Token::LeftBlockEnd
            | Token::InputEnd
            | Token::ParagraphBreak => {
                return None;
            }
            _ => i += 1,
        }
    }
    if !span_token_at_line_start(source, tokens, start) {
        return None;
    }
    if whole_line && !span_token_ends_line(tokens, i) {
        return None;
    }
    while i < tokens.len() {
        if let Some(end) = named_block_end_head_end(tokens, i, name) {
            let qualifies = if whole_line {
                span_token_at_line_start(source, tokens, i)
            } else {
                span_closer_ends_construct(tokens, end)
            };
            if qualifies {
                return Some(end);
            }
        }
        i += 1;
    }
    None
}

fn offset_inside_comment(source: &str, offset: usize) -> bool {
    let before = &source[..offset.min(source.len())];
    match (before.rfind("[!--"), before.rfind("--]")) {
        (Some(open), Some(close)) => open > close,
        (Some(_), None) => true,
        _ => false,
    }
}

fn token_ends_argument_key(token: Token) -> bool {
    matches!(
        token,
        Token::Whitespace | Token::LineBreak | Token::ParagraphBreak | Token::Equals,
    )
}

fn token_is_argument_spacing(token: Token) -> bool {
    [Token::Whitespace, Token::LineBreak, Token::ParagraphBreak].contains(&token)
}

fn token_is_body_boundary(token: Token) -> bool {
    matches!(
        token,
        Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
    )
}

fn block_rule_end_names(
    block_rule: &BlockRule,
    wikidot: bool,
) -> &'static [&'static str] {
    if wikidot && matches!(block_rule.name, "block-anchor" | "block-module") {
        &block_rule.accepts_names[..1]
    } else {
        block_rule.accepts_names
    }
}

fn wikidot_body_starts_nested_module(body: &str) -> bool {
    body.lines().any(|line| {
        let Some(head) = line.get(.."[[module".len()) else {
            return false;
        };
        head.eq_ignore_ascii_case("[[module")
            && line["[[module".len()..].starts_with([' ', '\t'])
    })
}

fn block_rule_accepts_end_name(
    block_rule: &BlockRule,
    name: &str,
    wikidot: bool,
) -> bool {
    block_rule_end_names(block_rule, wikidot)
        .iter()
        .any(|accepted| name.eq_ignore_ascii_case(accepted))
}

/// Wikidot block bodies that must close with an explicit `[[/name]]` end
/// head. Without one the rule cannot succeed: the body scan runs to the end
/// of input and fails with `EndOfInput`, and the head renders literally.
///
/// Repeated unclosed openers of these blocks used to rescan the rest of the
/// page once per opener (quadratic). When no end head exists after the head,
/// the same failure is known in advance, so the body scan is skipped. Blocks
/// that may legally end at a boundary instead of an end head (`[[tab]]`,
/// `[[li]]`), and the bibliography block whose speculative unclosed path
/// reports a different diagnostic, are deliberately absent.
fn wikidot_body_requires_end_head(block_rule: &BlockRule) -> bool {
    matches!(
        block_rule.name,
        "block-div"
            | "block-collapsible"
            | "block-note"
            | "block-align-left"
            | "block-align-right"
            | "block-align-center"
            | "block-align-justify"
            | "block-html"
            | "block-math"
            | "block-code"
            | "block-raw"
            | "block-embed"
            | "block-footnote"
            | "block-ruby"
            | "block-paragraph"
            | "block-blockquote"
            | "block-ifcategory"
            | "block-tabview"
    )
}

/// The next `\r` or `\n` at or after `start`, with the same monotone reuse as
/// the block-close scan.

fn find_line_break_marker(
    source: &str,
    slot: &mut Option<(usize, Option<usize>)>,
    start: usize,
) -> Option<usize> {
    if let Some((scanned_from, found)) = *slot
        && scanned_from <= start
        && found.is_none_or(|found| found >= start)
    {
        return found;
    }

    let found = source.as_bytes()[start..]
        .iter()
        .position(|byte| matches!(byte, b'\r' | b'\n'))
        .map(|offset| start + offset);
    *slot = Some((start, found));
    found
}

fn wikidot_alias_pair_cache_name(block_rule: &BlockRule) -> &'static str {
    match block_rule.name {
        "block-div" => "block-div-alias-pair",
        "block-span" => "block-span-alias-pair",
        _ => panic!("alias pairing is only defined for div and span"),
    }
}

fn wikidot_alias_close_is_exact(source: &str, name: &str) -> bool {
    source
        .strip_prefix("[[/")
        .and_then(|inner| inner.strip_suffix("]]"))
        .is_some_and(|inner| inner.eq_ignore_ascii_case(name))
}

fn wikidot_requires_next_physical_line(block_rule: &BlockRule) -> bool {
    matches!(
        block_rule.name,
        "block-div"
            | "block-note"
            | "block-align-left"
            | "block-align-right"
            | "block-align-center"
            | "block-align-justify"
    )
}

pub(crate) fn wikidot_physical_line_has_textual_div_closer(
    source: &str,
    marker_byte: usize,
) -> bool {
    let line_start = source[..marker_byte.min(source.len())]
        .rfind('\n')
        .map_or(0, |offset| offset + 1);
    let rest = source.get(line_start..).unwrap_or("");
    let line_end_rel = rest.find('\n').map_or(rest.len(), |i| i);
    let line = &rest[..line_end_rel];
    line.as_bytes()
        .windows(9)
        .any(|w| w.eq_ignore_ascii_case(b"[[/div]]"))
}

pub(crate) fn wikidot_trim_argument_fragment(value: &str) -> &str {
    value.trim_matches([' ', '\t', '\n', '\r', '\0', '\u{000B}'])
}

fn wikidot_stripslashes(value: Cow<'_, str>) -> Cow<'_, str> {
    if !value.contains('\\') {
        return value;
    }

    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }

        match chars.next() {
            Some('0') => output.push('\0'),
            Some(escaped) => output.push(escaped),
            None => {}
        }
    }

    Cow::Owned(output)
}

type WikidotHeadArgumentsFn = for<'t> fn(&CommentElidedText<'t>) -> Arguments<'t>;

/// How a Wikidot block head ends, derived from live Wikidot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WikidotHeadScan {
    /// The opener must be alone on its line: the head closes at the first `]]`
    /// whose line remainder is whitespace, so a `]]` inside a value (or the
    /// middle of a longer line) is kept as head text.
    LineEndGreedy,
    /// Double-quoted values run to their closing quote; the head closes at the
    /// first `]]` outside them.
    QuotedValuesAreLiteral,
    /// A lone `]` in the head ends it early, which invalidates the opener.
    EndsAtSingleBracket,
    /// The head ends at the first `]]`, whatever the quoting.
    Plain,
    /// A `]]` ends the head anywhere, but inside a double-quoted value a
    /// `]]]` is a link close and is kept as head text; outside quotes a
    /// `]]]` ends the head like a `]]`.
    QuotedLinksPass,
}

fn wikidot_head_scan(block_rule: &BlockRule) -> WikidotHeadScan {
    match block_rule.name() {
        "block-div" => WikidotHeadScan::LineEndGreedy,
        "block-table" | "block-toc" => WikidotHeadScan::QuotedValuesAreLiteral,
        "block-collapsible" => WikidotHeadScan::QuotedLinksPass,
        "block-anchor"
        | "block-span"
        | "block-code"
        | "block-gallery"
        | "block-footnote-block"
        | "block-list-unordered"
        | "block-list-ordered"
        | "block-list-item"
        | "block-table-row"
        | "block-table-cell-regular"
        | "block-table-cell-header"
        | "block-table-cell-body" => WikidotHeadScan::EndsAtSingleBracket,
        _ => WikidotHeadScan::Plain,
    }
}

/// Whether the token after the current one leaves only whitespace before the
/// next line break.
fn wikidot_head_terminator_ends_line(parser: &Parser<'_, '_>) -> bool {
    let mut offset = 0;
    loop {
        match parser.look_ahead(offset) {
            None => return true,
            Some(token) => match token.token {
                Token::Whitespace => offset += 1,
                Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                    return true;
                }
                _ => return false,
            },
        }
    }
}

/// Whether live's line-end-greedy div opener can close on this line before any
/// `[[/` closer: live pairs the block first, so a head that would swallow its
/// own closer instead falls back to the plain first-`]]` scan.
fn wikidot_line_greedy_head_is_usable(parser: &Parser<'_, '_>) -> bool {
    let mut probe = parser.clone();
    loop {
        match probe.current().token {
            Token::InputEnd => return true,
            Token::LeftBlockEnd | Token::LeftLink => return false,
            Token::RightBlock | Token::RightLink => {
                if wikidot_head_terminator_ends_line(&probe) {
                    return true;
                }
                if probe.step().is_err() {
                    return true;
                }
            }
            _ => {
                if probe.step().is_err() {
                    return true;
                }
            }
        }
    }
}

/// Whether a quoted Wikidot attribute value contains complete raw inline
/// markup (`@@...@@` or `@<...>@`), which live evaluates to an empty value.
fn wikidot_attribute_value_raw_elides(value: &str) -> bool {
    let mut rest = value;
    while let Some(offset) = rest.find('@') {
        rest = &rest[offset..];
        if rest.starts_with("@@") {
            if let Some(end) = rest[2..].find("@@") {
                let inner = &rest[2..2 + end];
                if !inner.contains('\n') && !inner.contains('\r') {
                    return true;
                }
            }
            rest = &rest[2..];
            continue;
        }
        if rest.starts_with("@<") {
            if let Some(end) = rest[2..].find(">@") {
                let inner = &rest[2..2 + end];
                if !inner.contains('\n') && !inner.contains('\r') {
                    return true;
                }
            }
            rest = &rest[2..];
            continue;
        }
        rest = &rest[1..];
    }
    false
}

pub(crate) fn parse_wikidot_attributes(value: &str) -> Arguments<'_> {
    let field = CommentElidedText::new(value, 0..value.len(), Vec::new());
    parse_wikidot_attribute_field(&field)
}

fn parse_wikidot_attribute_field<'t>(field: &CommentElidedText<'t>) -> Arguments<'t> {
    let source = field.source();
    let span = field.span();
    let mut comment_mask = vec![false; source.len()];
    for comment in field.comment_ranges() {
        let start = comment.start.saturating_sub(span.start).min(source.len());
        let end = comment.end.saturating_sub(span.start).min(source.len());
        comment_mask[start..end].fill(true);
    }
    let mut elision_mask = comment_mask.clone();
    for segment in field.segment_ranges() {
        let start = segment.start.saturating_sub(span.start).min(source.len());
        let end = segment.end.saturating_sub(span.start).min(source.len());
        elision_mask[start..end].fill(true);
    }
    for range in wikidot_head_formfeed_garble_ranges(source) {
        comment_mask[range.clone()].fill(true);
        elision_mask[range].fill(true);
    }

    let bytes = source.as_bytes();
    let mut delimiters = Vec::new();
    for index in 0..bytes.len().saturating_sub(1) {
        if bytes[index] == b'='
            && bytes[index + 1] == b'"'
            && elision_mask
                .get(index..)
                .is_some_and(|mask| mask.starts_with(&[false, false]))
        {
            delimiters.push(index);
        }
    }

    let mut arguments = Arguments::new_case_sensitive();
    let trimmed_source = wikidot_trim_argument_fragment(source);
    if !trimmed_source.is_empty() {
        arguments.mark_source_present();
    }
    if trimmed_source.starts_with('=') {
        arguments.mark_empty_key_present();
    }

    let first_delimiter = delimiters.first().copied().unwrap_or(source.len());
    let mut key = wikidot_attribute_key(field, &comment_mask, 0..first_delimiter);
    for (delimiter_index, delimiter) in delimiters.iter().copied().enumerate() {
        let segment_start = delimiter + 2;
        let segment_end = delimiters
            .get(delimiter_index + 1)
            .copied()
            .unwrap_or(source.len());
        let quote = (segment_start..segment_end)
            .rev()
            .find(|index| bytes[*index] == b'"' && !elision_mask[*index]);
        let (value_range, next_key_range) = match quote {
            Some(quote) => (segment_start..quote, quote + 1..segment_end),
            None => {
                let next_key_start = source
                    .get(segment_start..segment_end)
                    .and_then(|segment| segment.chars().next())
                    .map_or(segment_end, |character| {
                        segment_start + character.len_utf8()
                    });
                (segment_start..segment_start, next_key_start..segment_end)
            }
        };
        let absolute_range = span.start + value_range.start..span.start + value_range.end;
        let value = field.elide_range(absolute_range.clone());
        let raw_elided = wikidot_attribute_value_raw_elides(&value);
        if key.is_empty() {
            arguments.mark_empty_key_present();
        } else {
            let value = if raw_elided {
                wikidot_encoded_attribute_value(field, absolute_range)
                    .map_or(Cow::Borrowed(""), Cow::Owned)
            } else {
                wikidot_stripslashes(value)
            };
            let whitespace_only = !value.is_empty()
                && value.chars().all(|character| {
                    character.is_whitespace()
                        || matches!(character, '\u{0000}'..='\u{001F}' | '\u{007F}')
                });
            arguments.insert_cow(key.clone(), value);
            if raw_elided {
                arguments.mark_explicit_empty(key);
            } else if whitespace_only {
                arguments.mark_whitespace_empty(key);
            }
        }
        key = wikidot_attribute_key(field, &comment_mask, next_key_range);
    }
    if key.starts_with('=') {
        arguments.mark_empty_key_present();
    }

    arguments
}

/// The runs a formfeed garbles in a Wikidot head. A formfeed outside a quoted
/// value drops the following two quoted runs, which is why
/// `[[span class="a"\x0cstyle="b"]]` keeps `class` and drops `style` while
/// `[[span\x0cclass="a"]]` has no attributes.
fn wikidot_head_formfeed_garble_ranges(head: &str) -> Vec<std::ops::Range<usize>> {
    let bytes = head.as_bytes();
    let mut ranges = Vec::new();
    let mut index = 0;
    let mut in_quote = false;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                in_quote = !in_quote;
                index += 1;
            }
            0x0C if !in_quote => {
                let mut cursor = index + 1;
                let mut quotes = 0;
                while cursor < bytes.len() && quotes < 2 {
                    if bytes[cursor] == b'"' {
                        quotes += 1;
                    }
                    cursor += 1;
                }
                ranges.push(index..cursor);
                index = cursor;
                in_quote = false;
            }
            _ => index += 1,
        }
    }
    ranges
}

/// Encode the complete raw constructs inside an absolute source range as
/// placeholder sentinels, eliding comment and iftags-segment text, so the value
/// keeps live's placeholder bytes (used by the date format class).
fn wikidot_encoded_attribute_value(
    field: &CommentElidedText<'_>,
    range: std::ops::Range<usize>,
) -> Option<String> {
    let full = field.full_source();
    let escapes = raw_escapes(full);
    let mut contained: Vec<(usize, &crate::wikidot_raw_escape::RawEscape)> = escapes
        .iter()
        .enumerate()
        .filter(|(_, escape)| escape.start >= range.start && escape.end <= range.end)
        .collect();
    if contained.is_empty() {
        return None;
    }
    // Ordinals follow live's pass order (`@<...>@` first), the replacement
    // itself follows source order.
    contained.sort_by_key(|(_, escape)| escape.start);

    let mut elisions: Vec<std::ops::Range<usize>> = Vec::new();
    elisions.extend(field.comment_ranges().iter().cloned());
    elisions.extend(field.segment_ranges().iter().cloned());
    elisions.sort_by_key(|elision| elision.start);

    let mut output = String::with_capacity(range.len());
    let mut cursor = range.start;
    for (ordinal, escape) in contained {
        push_wikidot_attribute_plain(&mut output, full, cursor..escape.start, &elisions);
        output.push(PLACEHOLDER_OPEN);
        output.push_str(&(ordinal + 1).to_string());
        output.push(PLACEHOLDER_CLOSE);
        cursor = escape.end;
    }
    push_wikidot_attribute_plain(&mut output, full, cursor..range.end, &elisions);
    Some(output)
}

fn push_wikidot_attribute_plain(
    output: &mut String,
    source: &str,
    range: std::ops::Range<usize>,
    elisions: &[std::ops::Range<usize>],
) {
    let mut cursor = range.start;
    for elision in elisions {
        if elision.end <= cursor || elision.start >= range.end {
            continue;
        }
        let start = elision.start.max(cursor);
        if start > cursor {
            output.push_str(&source[cursor..start]);
        }
        cursor = elision.end.min(range.end);
    }
    if cursor < range.end {
        output.push_str(&source[cursor..range.end]);
    }
}

fn parse_wikidot_module_argument_field<'t>(
    field: &CommentElidedText<'t>,
) -> Arguments<'t> {
    let source = field.source();
    let span = field.span();
    let mut mask = vec![false; source.len()];
    for comment in field.comment_ranges() {
        let start = comment.start.saturating_sub(span.start).min(source.len());
        let end = comment.end.saturating_sub(span.start).min(source.len());
        mask[start..end].fill(true);
    }
    for segment in field.segment_ranges() {
        let start = segment.start.saturating_sub(span.start).min(source.len());
        let end = segment.end.saturating_sub(span.start).min(source.len());
        mask[start..end].fill(true);
    }

    let bytes = source.as_bytes();
    let mut arguments = Arguments::new();
    let trimmed_source = wikidot_trim_argument_fragment(source);
    if !trimmed_source.is_empty() {
        arguments.mark_source_present();
    }
    if trimmed_source.starts_with('=') {
        arguments.mark_empty_key_present();
    }

    let mut index = 0;
    while index + 1 < bytes.len() {
        let quote_open = bytes[index] == b'='
            && bytes[index + 1] == b'"'
            && !mask[index]
            && !mask[index + 1];
        if !quote_open {
            index += 1;
            continue;
        }

        let mut key_start = index;
        while key_start > 0 {
            let byte = bytes[key_start - 1];
            let key_byte = byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-');
            if !key_byte || mask[key_start - 1] {
                break;
            }
            key_start -= 1;
        }
        if key_start == index {
            index += 1;
            continue;
        }

        let mut cursor = index + 2;
        let mut closing = None;
        while cursor < bytes.len() {
            if mask[cursor] {
                cursor += 1;
                continue;
            }
            if bytes[cursor] == b'\\' && cursor + 1 < bytes.len() {
                cursor += 2;
                continue;
            }
            if bytes[cursor] == b'"' {
                closing = Some(cursor);
                break;
            }
            cursor += 1;
        }

        let Some(closing) = closing else {
            index += 1;
            continue;
        };

        let key: &'t str = &source[key_start..index];
        let value_range = span.start + index + 2..span.start + closing;
        let value = field.elide_range(value_range);
        arguments.insert_cow(Cow::Borrowed(key), wikidot_stripslashes(value));
        index = closing + 1;
    }

    arguments
}

fn wikidot_attribute_key<'t>(
    field: &CommentElidedText<'t>,
    comment_mask: &[bool],
    range: std::ops::Range<usize>,
) -> Cow<'t, str> {
    let fragment_start = comment_mask[range.clone()]
        .iter()
        .rposition(|masked| *masked)
        .map_or(range.start, |position| range.start + position + 1);
    let span = field.span();
    let fragment_end = range.end.min(span.len());
    let elided = field
        .elide_segments_range(span.start + fragment_start..span.start + fragment_end);
    match elided {
        Cow::Borrowed(fragment) => {
            Cow::Borrowed(wikidot_trim_argument_fragment(fragment))
        }
        Cow::Owned(text) => {
            let trimmed = wikidot_trim_argument_fragment(&text);
            if trimmed.len() == text.len() {
                Cow::Owned(text)
            } else {
                Cow::Owned(trimmed.to_owned())
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum QuoteGateSegmentKind {
    InRun,
    EscapedPlain,
    EscapedQuote { depth: usize, stripped: bool },
    QuoteRow(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum QuoteGateBoundary {
    Start,
    SameRun,
    NewRun,
}

#[derive(Debug)]
pub(crate) struct QuoteGateSegment<'t> {
    pub(crate) elements: Vec<Element<'t>>,
    pub(crate) paragraph_safe: bool,
    pub(crate) kind: QuoteGateSegmentKind,
    pub(crate) boundary: QuoteGateBoundary,
    pub(crate) split_kind: ParagraphSplitKind,
}

fn quote_line_depth(text: &str, position: usize) -> usize {
    let line_start = text[..position]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let bytes = &text.as_bytes()[line_start..];
    let mut depth = 0;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'>' => {
                depth += 1;
                index += 1;
            }
            b' ' | b'\t' if depth > 0 => {
                let mut next = index;
                while next < bytes.len() && matches!(bytes[next], b' ' | b'\t') {
                    next += 1;
                }
                if next < bytes.len() && bytes[next] == b'>' {
                    index = next;
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    depth
}

fn distribute_quote_gate_split<'t>(
    segments: &mut Vec<QuoteGateSegment<'t>>,
    elements: Vec<Element<'t>>,
    counts: &[ParagraphSplitEntry],
) {
    if counts.iter().map(|entry| entry.0).sum::<usize>() != elements.len() {
        let current = segments
            .last_mut()
            .expect("segments always has a current segment");
        current.elements.extend(elements);
        for (_, safe, _) in counts {
            current.paragraph_safe &= safe;
        }
        return;
    }

    let kind = segments
        .last()
        .expect("segments always has a current segment")
        .kind;
    let mut elements = elements.into_iter();
    for (index, (count, safe, split_kind)) in counts.iter().enumerate() {
        if index > 0 {
            segments.push(QuoteGateSegment {
                elements: Vec::new(),
                paragraph_safe: true,
                kind,
                boundary: QuoteGateBoundary::SameRun,
                split_kind: *split_kind,
            });
        }
        let current = segments
            .last_mut()
            .expect("segments always has a current segment");
        current.elements.extend(elements.by_ref().take(*count));
        current.paragraph_safe &= safe;
    }
}

fn segment_edge_blank(element: &Element) -> bool {
    match element {
        Element::LineBreak => true,
        Element::Text(text) => text.chars().all(|ch| matches!(ch, ' ' | '\t')),
        _ => false,
    }
}

fn trim_segment_leading_blanks(segment: &mut Vec<Element>) {
    let count = segment
        .iter()
        .take_while(|element| segment_edge_blank(element))
        .count();
    segment.drain(..count);
}

fn trim_segment_trailing_blanks(segment: &mut Vec<Element>) {
    while segment.last().is_some_and(segment_edge_blank) {
        segment.pop();
    }
}

fn distribute_paragraph_split<'t>(
    segments: &mut Vec<Vec<Element<'t>>>,
    mut segment_safe: Option<&mut Vec<bool>>,
    mut segment_kinds: Option<&mut Vec<ParagraphSplitKind>>,
    elements: Vec<Element<'t>>,
    counts: &[ParagraphSplitEntry],
) {
    if counts.iter().map(|(count, _, _)| count).sum::<usize>() != elements.len() {
        segments
            .last_mut()
            .expect("segments always has a current segment")
            .extend(elements);
        if let Some(segment_safe) = segment_safe.as_deref_mut()
            && let Some(current) = segment_safe.last_mut()
        {
            for (_, safe, _) in counts {
                *current &= safe;
            }
        }
        return;
    }

    let mut elements = elements.into_iter();
    for (index, (count, safe, _)) in counts.iter().enumerate() {
        if index > 0 {
            segments.push(Vec::new());
            if let Some(segment_safe) = segment_safe.as_deref_mut() {
                segment_safe.push(true);
            }
            if let Some(segment_kinds) = segment_kinds.as_deref_mut() {
                segment_kinds.push(ParagraphSplitKind::Paragraph);
            }
        }
        segments
            .last_mut()
            .expect("segments always has a current segment")
            .extend(elements.by_ref().take(*count));
        if let Some(segment_safe) = segment_safe.as_deref_mut()
            && let Some(current) = segment_safe.last_mut()
        {
            *current &= safe;
        }
    }
}

impl<'r, 't> Parser<'r, 't>
where
    'r: 't,
{
    /// The `\n`-delimited physical line around `position`, cached so a
    /// document with no line break does not rescan its whole tail per
    /// caller. Mirrors `source[..position].rfind('\n')` and
    /// `source[position..].find('\n')` exactly (only `\n` splits).
    pub(crate) fn wikidot_lf_line_bounds(&self, position: usize) -> (usize, usize) {
        let source = self.full_text().inner();
        let mut cache = self.block_end_scan_cache.borrow_mut();
        if let Some((line_start, line_end)) = cache.wikidot_lf_line_bounds
            && line_start <= position
            && position <= line_end
        {
            return (line_start, line_end);
        }
        let line_start = source[..position]
            .rfind('\n')
            .map_or(0, |offset| offset + 1);
        let line_end = source[position..]
            .find('\n')
            .map_or(source.len(), |offset| position + offset);
        cache.wikidot_lf_line_bounds = Some((line_start, line_end));
        (line_start, line_end)
    }

    /// Whether a block body could still close: any `[[/` marker ahead.
    pub(crate) fn wikidot_block_close_marker_ahead(&self) -> bool {
        let start = self.current().span.start;
        self.block_end_scan_cache
            .borrow_mut()
            .wikidot_first_block_close_after(start)
            .is_some()
    }

    /// The error an unclosed Wikidot body would produce after scanning to the
    /// end of input, or `None` when the body can still close or the rule may
    /// legitimately end at a boundary. Skipping the doomed scan keeps
    /// repeated unclosed openers linear instead of quadratic.
    fn wikidot_unclosed_body_error(&self, block_rule: &BlockRule) -> Option<ParseError> {
        if self.discarding_hidden_body() || self.wikidot_block_close_marker_ahead() {
            return None;
        }
        let requires_end_head = if self.settings().layout.legacy() {
            wikidot_body_requires_end_head(block_rule)
        } else {
            // The native-layout div body has the same failure shape: without an
            // end head anywhere ahead it scans to the end of input and reports
            // an end-of-input error on the div rule and the input-end token,
            // which is exactly what this preflight returns. Repeated unclosed
            // native div runs would otherwise replay the document once per
            // opener. Every other native block stays on the scanning path: the
            // fixture corpus shows their unclosed diagnostics differ (tabview
            // can end at a boundary, embed and ifcategory report their own head
            // errors first).
            block_rule.name == "block-div"
        };
        if !requires_end_head {
            return None;
        }
        Some(self.make_end_of_input_err())
    }

    pub(crate) fn take_wikidot_crossed_list_break_sentinel(
        &mut self,
    ) -> Result<bool, ParseError> {
        self.take_wikidot_crossed_tail_sentinel(
            crate::preproc::WIKIDOT_CROSSED_LIST_BREAK_SENTINEL,
        )
    }

    pub(crate) fn take_wikidot_crossed_ol_tail_sentinel(
        &mut self,
    ) -> Result<bool, ParseError> {
        self.take_wikidot_crossed_tail_sentinel(
            crate::preproc::WIKIDOT_CROSSED_OL_TAIL_SENTINEL,
        )
    }

    fn take_wikidot_crossed_tail_sentinel(
        &mut self,
        expected: &str,
    ) -> Result<bool, ParseError> {
        if !self.settings().layout.legacy() || self.current().token != Token::LeftComment
        {
            return Ok(false);
        }
        let mut candidate = self.clone();
        let range = match consume_valid_comment(&mut candidate) {
            Ok(range) => range,
            Err(_) => return Ok(false),
        };
        if &self.full_text().inner()[range] != expected {
            return Ok(false);
        }
        if candidate.current().token == Token::LineBreak {
            candidate.step()?;
        }
        self.update(&candidate);
        Ok(true)
    }

    /// The raw offset where a Wikidot block head starts, including discarded
    /// control bytes the name collector consumed before the current token.
    pub(crate) fn wikidot_head_control_start(&self) -> usize {
        fn is_control(byte: u8) -> bool {
            matches!(
                byte,
                0x00..=0x08 | 0x0B..=0x0C | 0x0E..=0x1A | 0x1C..=0x1F
            )
        }

        let bytes = self.full_text().inner().as_bytes();
        let mut position = self.current().span.start;
        if self.current().token == Token::DiscardedControl {
            while position > 0 && matches!(bytes[position - 1], b' ' | b'\t') {
                position -= 1;
            }
            return position;
        }
        while position > 0 && matches!(bytes[position - 1], b' ' | b'\t') {
            position -= 1;
        }
        let mut saw_control = false;
        while position > 0 && is_control(bytes[position - 1]) {
            position -= 1;
            saw_control = true;
        }
        if !saw_control {
            return self.current().span.start;
        }
        while position > 0 && matches!(bytes[position - 1], b' ' | b'\t') {
            position -= 1;
        }
        position
    }

    /// Consume discarded control tokens (and the spaces around them) before a
    /// Wikidot head word. Live treats formfeed-like controls as separators when
    /// reading block names and head fields.
    fn skip_head_discarded_controls(&mut self) -> Result<(), ParseError> {
        let mut seen = false;
        loop {
            match self.current().token {
                Token::DiscardedControl => {
                    self.step()?;
                    seen = true;
                }
                Token::Whitespace if seen => {
                    self.step()?;
                }
                _ => break,
            }
        }
        Ok(())
    }

    pub fn get_block_name(
        &mut self,
        flag_star: bool,
    ) -> Result<(Cow<'t, str>, bool, bool), ParseError> {
        self.get_block_name_bracketed(flag_star, true)
    }

    pub(crate) fn get_block_name_after_end_marker(
        &mut self,
    ) -> Result<(Cow<'t, str>, bool, bool), ParseError> {
        self.get_block_name_bracketed(false, false)
    }

    fn get_block_name_bracketed(
        &mut self,
        flag_star: bool,
        consume_bracket: bool,
    ) -> Result<(Cow<'t, str>, bool, bool), ParseError> {
        if consume_bracket {
            if flag_star {
                self.get_optional_token(Token::LeftBlockStar)?;
            } else {
                self.get_optional_token(Token::LeftBlock)?;
            }
        }

        // Live Wikidot requires the block name immediately after the opener
        // (`[[ span]]` stays literal); only the native layout tolerates
        // whitespace between the brackets and the name.
        if !consume_bracket || !self.settings().layout.legacy() {
            self.get_optional_space()?;
        }

        if !self.settings().layout.legacy() {
            return self.get_head_word_tracked(ParseErrorKind::BlockMissingName);
        }

        // Live's opener regexes accept a vertical tab or formfeed as the
        // separator after the name (`[[span\x0cclass="a"]]`); the scanner
        // keeps those in the collected word, so split the name there and
        // rewind the parser to the control token.
        let checkpoint = self.clone();
        let start_offset = self.current().span.start;
        let (name, in_head, segment_consumed) =
            self.get_head_word_tracked(ParseErrorKind::BlockMissingName)?;
        if let Some((split, _)) = name
            .char_indices()
            .find(|(_, character)| matches!(*character, '\u{000B}' | '\u{000C}'))
        {
            let split = start_offset + split;
            let source = self.full_text().inner();
            let prefix = source
                .get(start_offset..split)
                .filter(|prefix| !prefix.is_empty());
            if let Some(prefix) = prefix {
                self.update(&checkpoint);
                while self.current().span.start < split {
                    self.step()?;
                }
                return Ok((Cow::Borrowed(prefix), true, segment_consumed));
            }
        }
        Ok((name, in_head, segment_consumed))
    }

    pub(crate) fn head_word_is_actionable(
        &self,
        name: &str,
        borrowed: bool,
        segment: bool,
    ) -> bool {
        self.head_word_view_is_actionable(borrowed, segment, name)
    }

    pub(crate) fn head_word_view_is_actionable(
        &self,
        borrowed: bool,
        segment: bool,
        view: &str,
    ) -> bool {
        if borrowed {
            return true;
        }
        let block = if self.discarding_hidden_body() {
            super::mapping::get_block_rule_with_name(view)
        } else {
            super::mapping::get_block_rule_with_name_for_layout(
                view,
                self.settings().layout,
            )
        };
        let Some(block) = block else {
            return false;
        };
        if segment {
            head_seam_forms_block(block, self.settings().layout)
        } else {
            !head_word_never_resolves(block)
        }
    }

    fn get_head_word_raw(
        &mut self,
        kind: ParseErrorKind,
    ) -> Result<(&'t str, bool), ParseError> {
        let legacy = self.settings().layout.legacy();
        if legacy {
            self.skip_head_discarded_controls()?;
        }
        let end_conditions = [
            ParseCondition::current(Token::Whitespace),
            ParseCondition::current(Token::LineBreak),
            ParseCondition::current(Token::ParagraphBreak),
            ParseCondition::current(Token::RightBlock),
        ];
        let stops = &end_conditions[..];
        let rule = self.rule();
        collect_text_keep(self, rule, stops, &[], Some(kind)).map(|(name, last)| {
            let name = if self.settings().layout.legacy() {
                trim_wikidot_ascii(name)
            } else {
                name.trim()
            };
            let in_head = !matches!(last.token, Token::RightBlock);

            (name, in_head)
        })
    }

    pub fn get_head_word(
        &mut self,
        kind: ParseErrorKind,
    ) -> Result<(Cow<'t, str>, bool), ParseError> {
        let (name, in_head, _) = self.get_head_word_impl(kind, true)?;
        Ok((name, in_head))
    }

    fn get_head_word_tracked(
        &mut self,
        kind: ParseErrorKind,
    ) -> Result<(Cow<'t, str>, bool, bool), ParseError> {
        self.get_head_word_impl(kind, false)
    }

    fn get_head_word_impl(
        &mut self,
        kind: ParseErrorKind,
        skip_leading_comments: bool,
    ) -> Result<(Cow<'t, str>, bool, bool), ParseError> {
        if skip_leading_comments && !self.settings().layout.legacy() {
            while self.current().token == Token::LeftComment {
                let mut probe = self.clone();
                if consume_valid_comment(&mut probe).is_err() {
                    break;
                }
                self.update(&probe);
                if self.current().token == Token::Whitespace {
                    self.step()?;
                }
            }
        }
        let legacy = self.settings().layout.legacy();
        if legacy {
            self.skip_head_discarded_controls()?;
        }
        let end_conditions = [
            ParseCondition::current(Token::Whitespace),
            ParseCondition::current(Token::LineBreak),
            ParseCondition::current(Token::ParagraphBreak),
            ParseCondition::current(Token::RightBlock),
        ];
        let rule = self.rule();
        let (name, last, segment_consumed) = collect_text_resolved_fused_tracked(
            self,
            rule,
            &end_conditions,
            &[],
            Some(kind),
        )?;
        let legacy = self.settings().layout.legacy();
        fn trim_head_word(name: &str, legacy: bool) -> &str {
            if legacy {
                trim_wikidot_ascii(name)
            } else {
                name.trim()
            }
        }
        let mut name = match name {
            Cow::Borrowed(name) => Cow::Borrowed(trim_head_word(name, legacy)),
            Cow::Owned(name) => Cow::Owned(trim_head_word(&name, legacy).to_owned()),
        };
        if matches!(name, Cow::Owned(_))
            && (name.contains(char::is_whitespace) || name.contains("]]"))
        {
            // A head word assembled across a gate seam can carry the
            // whitespace that separated it from the rest of the head. Live
            // reads such a head as its first token plus trailing tokens, so
            // keep the first token instead of failing the whole block.
            match name.find(char::is_whitespace) {
                Some(index)
                    if self.settings().layout.legacy()
                        && !name.contains("]]")
                        && self.rule().name() == "block-date" =>
                {
                    name = Cow::Owned(name[..index].to_owned());
                }
                _ => return Err(self.make_err(ParseErrorKind::RuleFailed)),
            }
        }
        let in_head = !matches!(
            last.token,
            Token::RightBlock | Token::RightBracket | Token::RightLink
        );

        Ok((name, in_head, segment_consumed))
    }

    pub(crate) fn get_wikidot_block_name_with_residual_opener(
        &mut self,
    ) -> Result<(&'t str, bool), ParseError> {
        debug_assert!(self.settings().layout.legacy());
        self.get_optional_token(Token::LeftBlock)?;
        self.get_optional_space()?;

        let end_conditions = [
            ParseCondition::current(Token::Whitespace),
            ParseCondition::current(Token::LineBreak),
            ParseCondition::current(Token::ParagraphBreak),
            ParseCondition::current(Token::RightBlock),
            ParseCondition::current(Token::RightLink),
        ];
        let rule = self.rule();
        collect_text_keep(
            self,
            rule,
            &end_conditions,
            &[],
            Some(ParseErrorKind::BlockMissingName),
        )
        .and_then(|(name, last)| {
            if last.token != Token::RightLink || last.slice != "]]]" {
                return Err(self.make_err(ParseErrorKind::BlockExpectedEnd));
            }
            Ok((trim_wikidot_ascii(name), false))
        })
    }

    pub(crate) fn get_wikidot_end_block_with_residual(
        &mut self,
    ) -> Result<(&'t str, bool), ParseError> {
        debug_assert!(self.settings().layout.legacy());
        self.get_token(Token::LeftBlockEnd, ParseErrorKind::BlockExpectedEnd)?;
        self.get_optional_space()?;

        let end_conditions = [
            ParseCondition::current(Token::Whitespace),
            ParseCondition::current(Token::LineBreak),
            ParseCondition::current(Token::ParagraphBreak),
            ParseCondition::current(Token::RightBlock),
            ParseCondition::current(Token::RightLink),
        ];
        let rule = self.rule();
        let (name, last) = collect_text_keep(
            self,
            rule,
            &end_conditions,
            &[],
            Some(ParseErrorKind::BlockMissingName),
        )?;
        let residual_close_bracket = match last.token {
            Token::RightLink if last.slice == "]]]" => true,
            Token::RightBlock => false,
            Token::Whitespace => {
                return Err(self.make_err(ParseErrorKind::BlockExpectedEnd));
            }
            _ => return Err(self.make_err(ParseErrorKind::BlockExpectedEnd)),
        };
        Ok((trim_wikidot_ascii(name), residual_close_bracket))
    }

    pub fn get_end_block(&mut self) -> Result<Cow<'t, str>, ParseError> {
        self.get_token(Token::LeftBlockEnd, ParseErrorKind::BlockExpectedEnd)?;
        self.get_optional_space()?;

        let (name, in_head, segment) = self.get_block_name_after_end_marker()?;
        if matches!(name, Cow::Owned(_))
            && !self.head_word_is_actionable(
                &name,
                matches!(name, Cow::Borrowed(_)),
                segment,
            )
        {
            return Err(self.make_err(ParseErrorKind::BlockExpectedEnd));
        }
        if in_head {
            if self.settings().layout.legacy()
                && self.current().token == Token::Whitespace
            {
                return Err(self.make_err(ParseErrorKind::BlockExpectedEnd));
            }
            self.get_optional_space()?;
            self.get_token(Token::RightBlock, ParseErrorKind::BlockExpectedEnd)?;
        }

        Ok(name)
    }

    /// Whether the byte directly after the just-parsed close marker is ASCII
    /// whitespace or the end of input, which is what makes a Wikidot
    /// `[[/html]]` marker a real block close rather than body text.
    pub(crate) fn wikidot_html_closer_boundary_ahead(&self) -> bool {
        let text = self.full_text().inner();
        match text.as_bytes().get(self.current().span.start) {
            None => true,
            Some(byte) => byte.is_ascii_whitespace(),
        }
    }

    pub(crate) fn wikidot_fused_end_block_ahead(&self, block_rule: &BlockRule) -> bool {
        let source = &self.full_text().inner()[self.current().span.start..];
        let Some(rest) = source.strip_prefix("[[/") else {
            return false;
        };
        block_rule_end_names(block_rule, true).iter().any(|name| {
            let Some(after) = rest
                .get(..name.len())
                .filter(|part| part.eq_ignore_ascii_case(name))
                .map(|_| &rest[name.len()..])
            else {
                return false;
            };
            after.starts_with("]]]")
                || after
                    .strip_prefix('_')
                    .is_some_and(|stripped| stripped.starts_with("]]]"))
        })
    }

    pub(crate) fn wikidot_body_closed_with_fused_close(
        &self,
        block_rule: &BlockRule,
    ) -> bool {
        let source = &self.full_text().inner()[..self.current().span.start];
        block_rule_end_names(block_rule, true).iter().any(|name| {
            [&format!("[[/{name}]]]"), &format!("[[/{name}_]]]")]
                .iter()
                .any(|close| {
                    source
                        .get(source.len().saturating_sub(close.len())..)
                        .is_some_and(|tail| tail.eq_ignore_ascii_case(close))
                })
        })
    }

    pub(crate) fn get_wikidot_fused_end_block(
        &mut self,
        block_rule: &BlockRule,
    ) -> Result<&'t str, ParseError> {
        self.get_token(Token::LeftBlockEnd, ParseErrorKind::BlockExpectedEnd)?;
        self.get_optional_space()?;

        let end_conditions = [
            ParseCondition::current(Token::Whitespace),
            ParseCondition::current(Token::LineBreak),
            ParseCondition::current(Token::ParagraphBreak),
            ParseCondition::current(Token::RightBlock),
            ParseCondition::current(Token::RightLink),
        ];
        let rule = self.rule();
        let (name, last) = collect_text_keep(
            self,
            rule,
            &end_conditions,
            &[],
            Some(ParseErrorKind::BlockMissingName),
        )?;
        let name = trim_wikidot_ascii(name);

        let base_name = name.strip_suffix('_').unwrap_or(name);
        if last.token == Token::RightLink
            && block_rule_accepts_end_name(block_rule, base_name, true)
        {
            Ok(name)
        } else {
            Err(self.make_err(ParseErrorKind::BlockExpectedEnd))
        }
    }

    fn at_wikidot_code_close_terminator(&self) -> bool {
        // The parser sits on the token after a `[[/code]]` end block. Live
        // Wikidot ends the code body there when the byte after the marker
        // is whitespace (the text after it is outer content) or the input
        // ends; a byte like a letter or `[` leaves the marker as body text.
        let source = self.full_text().inner();
        match source[self.current().span.start..].bytes().next() {
            None => true,
            Some(byte) => matches!(byte, b' ' | b'\t' | b'\r' | b'\n'),
        }
    }

    /// Whether the parser sits at a math closer that starts its own line.
    ///
    /// Wikidot closes a math block at the first `[[/math]]` that starts its
    /// line, even when blank lines or trailing text follow. A closer on the
    /// same line as the opener or body text never closes (those shapes merge
    /// forward instead). The byte check covers every line-break token
    /// (including a lone-backslash break), which a token check cannot see
    /// once the break has been stepped over.
    fn math_closer_starts_line(&self) -> bool {
        if self.current().token != Token::LeftBlockEnd {
            return false;
        }
        self.full_text()
            .inner()
            .as_bytes()
            .get(..self.current().span.start)
            .is_some_and(|before| before.ends_with(b"\n"))
    }

    fn at_blank_line_remainder(&self) -> bool {
        if self.current().token != Token::Whitespace {
            return false;
        }
        let source = &self.full_text().inner()[self.current().span.start..];
        let line = match source.find(['\r', '\n']) {
            Some(index) => &source[..index],
            None => source,
        };
        line.bytes().all(|byte| matches!(byte, b' ' | b'\t'))
    }

    /// After a line-anchored `[[/math]]`, live sometimes keeps the **next**
    /// wrapped paragraph's first visible line with a leading `<br />` when
    /// the closer line's suffix (bytes after `]]` before the newline) is
    /// whitespace but not exactly one ASCII space. Measured on
    /// sandbox-for-codex PagePreview 2026-11-06.
    fn wikidot_note_math_closer_line_suffix_for_br(&mut self) {
        if !self.settings().layout.legacy()
            || self.discarding_hidden_body()
            || self.in_footnote()
        {
            return;
        }
        let text = self.full_text().inner();
        let tail = text.get(self.current().span.start..).unwrap_or("");
        let rest_line = tail.split(|c| c == '\n' || c == '\r').next().unwrap_or("");
        let wants_br = !rest_line.is_empty()
            && rest_line.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\r'))
            && rest_line != " ";
        self.set_wikidot_math_closer_line_br_prefix(wants_br);
    }

    pub(crate) fn wikidot_at_joined_bibliography_closer(&self) -> bool {
        if self.discarding_hidden_body() || self.current().token != Token::LeftBlockEnd {
            return false;
        }
        let in_bibliography_body = if self.settings().layout.legacy() {
            self.in_wikidot_bibliography_body()
        } else {
            self.in_bibliography_parse_body()
        };
        if !in_bibliography_body {
            return false;
        }
        const CLOSER: &str = "[[/bibliography]]";
        let source = self.full_text().inner();
        let start = self.current().span.start;
        source
            .get(start..start + CLOSER.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(CLOSER))
    }

    pub(crate) fn wikidot_bibliography_split_at_enclosing_closer(&mut self) -> bool {
        if !self.settings().layout.legacy()
            || self.discarding_hidden_body()
            || self.current().token != Token::LeftBlockEnd
        {
            return false;
        }
        let mut probe = self.clone();
        let Ok(raw_name) = probe.get_end_block() else {
            return false;
        };
        let name = raw_name.strip_suffix('_').unwrap_or(&raw_name);
        if ![
            "cell", "hcell", "row", "table", "li", "ul", "ol", "tab", "tabview", "tabs",
        ]
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
        {
            return false;
        }
        if !self.wikidot_ancestor_boundary_accepts(name) {
            return false;
        }
        self.wikidot_note_bibliography_split_closer()
    }

    fn wikidot_note_bibliography_split_closer(&mut self) -> bool {
        if !self.later_line_bibliography_closer() {
            return false;
        }
        self.set_bibliography_split_tail(true);
        true
    }

    fn later_line_bibliography_closer(&self) -> bool {
        const CLOSER: &str = "[[/bibliography]]";
        let source = self.full_text().inner();
        let mut rest = &source[self.current().span.start..];
        loop {
            let Some(index) = rest
                .as_bytes()
                .windows(CLOSER.len())
                .position(|window| window.eq_ignore_ascii_case(CLOSER.as_bytes()))
            else {
                return false;
            };
            let after = &rest[index + CLOSER.len()..];
            let line = after.split(['\n', '\r']).next().unwrap_or("");
            if line.bytes().all(|byte| matches!(byte, b' ' | b'\t')) {
                return true;
            }
            rest = &rest[index + CLOSER.len()..];
        }
    }

    fn verify_end_block(
        &mut self,
        first_iteration: bool,
        block_rule: &BlockRule,
        restrict_quote_close: bool,
        allow_inline_quote_close: bool,
    ) -> Option<&'r ExtractedToken<'t>> {
        if block_rule.name == "block-iftags"
            && let Some(designated) = self.wikidot_iftags_body_end()
            && !self.at_designated_iftags_end(designated)
        {
            return None;
        }

        self.save_evaluate_fn(|parser| {
            if restrict_quote_close
                && allow_inline_quote_close
                && parser.settings().layout.legacy()
                && !parser.discarding_hidden_body()
                && block_rule.name == "block-collapsible"
                && parser.wikidot_collapsible_closed_at_deeper_quote()
            {
                parser.set_wikidot_collapsible_closed_at_deeper_quote(false);
                return Ok(true);
            }

            if block_rule.accepts_newlines_outside_body {
                if parser.settings().layout.legacy()
                    && !parser.discarding_hidden_body()
                    && block_rule.name == "block-math"
                    && !parser.math_closer_starts_line()
                {
                    return Ok(false);
                } else if !first_iteration {
                    parser.get_optional_line_break()?;
                }
            }

            if restrict_quote_close
                && parser.prepare_quote_body_line()? == QuoteBodyLineStatus::Boundary
            {
                return Ok(false);
            }
            if restrict_quote_close && !parser.quote_body_close_allowed_here() {
                let follows_only_literal_quotes =
                    parser.quote_body_close_follows_only_literal_quotes();
                let follows_tab_close = block_rule.name == "block-tabview"
                    && parser.settings().layout.legacy()
                    && !parser.discarding_hidden_body()
                    && parser.quote_body_close_follows_tab_close();
                if !follows_only_literal_quotes
                    && !follows_tab_close
                    && (!allow_inline_quote_close
                        || !matches!(
                            parser.current().token,
                            Token::Quote | Token::LeftBlockEnd
                        ))
                {
                    return Ok(false);
                }
                if follows_tab_close {
                    parser.get_optional_space()?;
                }
                while parser.current().token == Token::Quote {
                    parser.step()?;
                    parser.get_optional_space()?;
                }
            }

            if restrict_quote_close
                && parser.settings().layout.legacy()
                && !parser.discarding_hidden_body()
                && matches!(
                    block_rule.name,
                    "block-align-center"
                        | "block-align-left"
                        | "block-align-right"
                        | "block-align-justify"
                )
                && parser.quote_body_close_allowed_here()
                && matches!(
                    parser.current().token,
                    Token::BulletItem | Token::NumberedItem
                )
            {
                let mut probe = parser.clone();
                if probe.step().is_ok()
                    && probe.get_optional_space().is_ok()
                    && probe.current().token == Token::LeftBlockEnd
                {
                    parser.step()?;
                    parser.get_optional_space()?;
                }
            }

            let end_start = parser.current().span.start;
            if parser.settings().layout.legacy()
                && !parser.discarding_hidden_body()
                && parser.current().token == Token::LeftBlockEnd
                && let Some(close_offset) =
                    parser.full_text().inner()[end_start..].find("]]")
            {
                let raw_close =
                    &parser.full_text().inner()[end_start..end_start + close_offset];
                if raw_close.chars().last().is_some_and(char::is_whitespace) {
                    return Ok(false);
                }
            }
            let fused_close = (parser.settings().layout.legacy()
                || block_rule.name == "block-iftags"
                || (block_rule.name == "block-footnote"
                    && parser.title_footnote_fused_end()))
                && matches!(
                    block_rule.name,
                    "block-span"
                        | "block-size"
                        | "block-anchor"
                        | "block-footnote"
                        | "block-div"
                        | "block-collapsible"
                        | "block-table"
                        | "block-list-unordered"
                        | "block-list-ordered"
                        | "block-list-item"
                        | "block-iftags"
                )
                && parser.wikidot_fused_end_block_ahead(block_rule);
            let name = if fused_close {
                parser
                    .get_wikidot_fused_end_block(block_rule)
                    .map(Cow::Borrowed)?
            } else {
                parser.get_end_block()?
            };

            // Live pairs a tabview closer only when it attaches directly after
            // the body's last tab closer. A closer after prose stays literal
            // content and the body keeps scanning for a later attached closer
            // (tabview/tab-stray-fails). Quoted bodies apply the same rule
            // through quote_body_close_follows_tab_close below.
            if parser.settings().layout.legacy()
                && !parser.discarding_hidden_body()
                && block_rule.name == "block-tabview"
                && !restrict_quote_close
                && !parser.wikidot_tabview_close_follows_tab_close(end_start)
            {
                return Ok(false);
            }

            // A Wikidot [[html]] closer only ends the block when the next byte
            // is ASCII whitespace or the end of input; otherwise the closer text
            // is part of the raw captured body. Pinned by the saved-page
            // end-of-block matrix ([[html]]x[[/html]][[/html]] captures
            // "x[[/html]]").
            if parser.settings().layout.legacy()
                && !parser.discarding_hidden_body()
                && block_rule.name == "block-html"
                && !parser.wikidot_html_closer_boundary_ahead()
            {
                return Ok(false);
            }

            if parser.settings().layout.legacy()
                && !parser.discarding_hidden_body()
                && name.ends_with('_')
            {
                return Ok(false);
            }

            if parser.settings().layout.legacy()
                && !fused_close
                && !parser.discarding_hidden_body()
                && block_rule.name.starts_with("block-list-")
                && matches!(name, Cow::Borrowed(_))
            {
                let end = parser.current().span.start;
                let source = &parser.full_text().inner()[end_start..end];
                let expected = format!("[[/{name}]]");
                if !source.eq_ignore_ascii_case(&expected) {
                    return Ok(false);
                }
            }

            if parser.settings().layout.legacy()
                && !fused_close
                && !parser.discarding_hidden_body()
                && block_rule.name == "block-bibliography"
                && matches!(name, Cow::Borrowed(_))
                && &parser.full_text().inner()[end_start..parser.current().span.start]
                    != "[[/bibliography]]"
            {
                return Ok(false);
            }

            let score_close = name.ends_with('_');
            let name = name.strip_suffix('_').unwrap_or(&name);
            if parser.settings().layout.legacy()
                && !parser.discarding_hidden_body()
                && block_rule.name != "block-bibliography"
            {
                let flat_closer = parser
                    .wikidot_block_pair_at_closer_byte(end_start)
                    .is_some_and(|pair| pair.flat && pair.kind.emits_flat());
                if flat_closer {
                    return Ok(false);
                }
            }
            // List wraps a bullet line before Tableadv runs. The list-end
            // tokens sit between that line's `[[/row]]` or `[[/table]]` and
            // a closer on a later line, so the bullet line stays item text.
            // A cell closer earlier on the same line still reaches it.
            if parser.settings().layout.legacy()
                && !parser.discarding_hidden_body()
                && matches!(block_rule.name, "block-table" | "block-table-row")
                && parser.wikidot_list_line_table_closer_is_item_text(end_start)
            {
                return Ok(false);
            }
            if parser.settings().layout.legacy()
                && score_close
                && block_rule.name == "block-iftags"
            {
                return Ok(false);
            }

            if parser.settings().layout.legacy()
                && !parser.discarding_hidden_body()
                && block_rule.name == "block-collapsible"
                && parser.in_wikidot_center_body()
                && name == "="
            {
                if restrict_quote_close {
                    parser.get_optional_space()?;
                    if !allow_inline_quote_close
                        && !token_is_body_boundary(parser.current().token)
                    {
                        return Ok(false);
                    }
                }
                parser.mark_wikidot_collapsible_content_closed();
                return Ok(true);
            }

            for end_block_name in
                block_rule_end_names(block_rule, parser.settings().layout.legacy())
            {
                if name.eq_ignore_ascii_case(end_block_name) {
                    // Live Wikidot gives code, math, bibliography and the
                    // alignment blocks line-owned closers: trailing text
                    // after the closer voids the block. The alignment
                    // closers are strict (not even trailing blanks), while
                    // the other blocks tolerate a blank remainder.
                    let strict_line_end = matches!(
                        block_rule.name,
                        "block-align-center"
                            | "block-align-left"
                            | "block-align-right"
                            | "block-align-justify"
                    );
                    if parser.settings().layout.legacy()
                        && !parser.discarding_hidden_body()
                        && (matches!(
                            block_rule.name,
                            "block-code" | "block-math" | "block-bibliography"
                        ) || strict_line_end)
                        && !(block_rule.name == "block-math" && parser.in_footnote())
                        && !(matches!(
                            parser.current().token,
                            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
                        ) || (!strict_line_end
                            && (parser.at_blank_line_remainder()
                                || (block_rule.name == "block-code"
                                    && parser.at_wikidot_code_close_terminator())
                                || (block_rule.name == "block-math"
                                    && parser.current().token == Token::Whitespace))))
                    {
                        return Ok(false);
                    }
                    if restrict_quote_close {
                        parser.get_optional_space()?;
                        if !allow_inline_quote_close
                            && !token_is_body_boundary(parser.current().token)
                        {
                            return Ok(false);
                        }
                    }
                    return Ok(true);
                }
            }

            Ok(false)
        })
    }

    pub(crate) fn terminate_at_wikidot_list_close(
        &mut self,
        block_rule: &BlockRule,
        first_iteration: bool,
    ) -> bool {
        if !self.settings().layout.legacy()
            || self.discarding_hidden_body()
            || matches!(
                block_rule.name,
                "block-list-unordered" | "block-list-ordered"
            )
            || self.accepts_partial() != crate::tree::AcceptsPartial::ListItem
        {
            return false;
        }

        let mut probe = self.clone();
        let mut stepped_line_break = false;
        if !first_iteration
            && block_rule.accepts_newlines_outside_body
            && probe.current().token == Token::LineBreak
        {
            if probe.step().is_err() {
                return false;
            }
            stepped_line_break = true;
        }
        if probe.current().token != Token::LeftBlockEnd {
            return false;
        }
        let start = probe.current().span.start;
        let Ok(raw_name) = probe.get_end_block() else {
            return false;
        };
        let end = probe.current().span.start;
        let source = &self.full_text().inner()[start..end];
        if !source.eq_ignore_ascii_case(&format!("[[/{raw_name}]]")) {
            return false;
        }
        let name = raw_name.strip_suffix('_').unwrap_or(&raw_name);
        let terminates = if block_rule.name == "block-list-item" {
            name.eq_ignore_ascii_case("ul") || name.eq_ignore_ascii_case("ol")
        } else {
            name.eq_ignore_ascii_case("li")
                || name.eq_ignore_ascii_case("ul")
                || name.eq_ignore_ascii_case("ol")
        };
        if !terminates {
            return false;
        }

        if block_rule.name == "block-bibliography" {
            self.wikidot_note_bibliography_split_closer();
        }

        if stepped_line_break && self.step().is_err() {
            return false;
        }
        self.record_wikidot_crossed_close(block_rule.name);
        if block_rule.name == "block-note" {
            self.set_pending_note_list_closer(true);
        }
        true
    }

    /// A later `[[/li]]` still inside the oldest open item pairs this close
    /// with the innermost item. The last close pairs with the oldest item.
    fn take_wikidot_li_pair(
        queue: &mut std::collections::VecDeque<usize>,
        heads: &[crate::parsing::parser::WikidotLiHead],
        after_close: &Self,
        depth: usize,
    ) -> Option<usize> {
        let floor = queue.front().and_then(|pos| {
            heads
                .iter()
                .find(|head| head.pos == *pos)
                .map(|head| head.depth)
        });
        let innermost = floor
            .is_some_and(|floor| after_close.wikidot_li_close_before_floor(depth, floor));
        if innermost {
            queue.pop_back()
        } else {
            queue.pop_front()
        }
    }

    fn wikidot_li_close_before_floor(&self, mut depth: usize, floor: usize) -> bool {
        let mut scan = self.clone();
        loop {
            if scan.current().token == Token::InputEnd || scan.at_hard_body_end_boundary()
            {
                return false;
            }
            match scan.current().token {
                Token::LeftBlock => {
                    let mut probe = scan.clone();
                    if let Ok((name, _, _)) = probe.get_block_name(false) {
                        let name = name.strip_suffix('_').unwrap_or(&name);
                        if name.eq_ignore_ascii_case("ul")
                            || name.eq_ignore_ascii_case("ol")
                        {
                            depth += 1;
                        }
                    }
                    if scan.step().is_err() {
                        return false;
                    }
                }
                Token::LeftBlockEnd => {
                    let mut probe = scan.clone();
                    if let Ok(name) = probe.get_end_block() {
                        let name = name.strip_suffix('_').unwrap_or(&name);
                        if name.eq_ignore_ascii_case("ul")
                            || name.eq_ignore_ascii_case("ol")
                        {
                            if depth == 0 || depth - 1 < floor {
                                return false;
                            }
                            depth -= 1;
                            scan.update(&probe);
                        } else if name.eq_ignore_ascii_case("li") {
                            return true;
                        } else if scan.step().is_err() {
                            return false;
                        }
                    } else if scan.step().is_err() {
                        return false;
                    }
                }
                _ => {
                    if scan.step().is_err() {
                        return false;
                    }
                }
            }
        }
    }

    pub(crate) fn compute_wikidot_li_pairing(&self) {
        use std::collections::{BTreeSet, VecDeque};

        let mut scan = self.clone();
        let mut depth = 0usize;
        let mut queue: VecDeque<usize> = VecDeque::new();
        let mut paired: BTreeSet<usize> = BTreeSet::new();
        let mut heads: Vec<WikidotLiHead> = Vec::new();
        let mut closes: Vec<WikidotLiClose> = Vec::new();
        loop {
            if scan.current().token == Token::InputEnd || scan.at_hard_body_end_boundary()
            {
                break;
            }
            if scan.skip_discarded_iftags_segment(false) {
                continue;
            }
            match scan.current().token {
                Token::LeftBlock => {
                    let start = scan.current().span.start;
                    let mut probe = scan.clone();
                    if let Ok((name, _, _)) = probe.get_block_name(false) {
                        let name = name.strip_suffix('_').unwrap_or(&name);
                        if name.eq_ignore_ascii_case("ul")
                            || name.eq_ignore_ascii_case("ol")
                        {
                            depth += 1;
                        } else if name.eq_ignore_ascii_case("li") {
                            queue.push_back(start);
                            heads.push(WikidotLiHead { pos: start, depth });
                        }
                    }
                    if scan.step().is_err() {
                        break;
                    }
                }
                Token::LeftBlockEnd => {
                    let mut probe = scan.clone();
                    match probe.get_end_block() {
                        Ok(name) => {
                            let name = name.strip_suffix('_').unwrap_or(&name);
                            if name.eq_ignore_ascii_case("ul")
                                || name.eq_ignore_ascii_case("ol")
                            {
                                if depth == 0 {
                                    break;
                                }
                                depth -= 1;
                                scan.update(&probe);
                            } else if name.eq_ignore_ascii_case("li") {
                                let close_pos = scan.current().span.start;
                                let paired_head = Self::take_wikidot_li_pair(
                                    &mut queue, &heads, &probe, depth,
                                );
                                if let Some(head) = paired_head {
                                    paired.insert(head);
                                }
                                closes.push(WikidotLiClose {
                                    pos: close_pos,
                                    depth,
                                    paired: paired_head,
                                    prev_paired: None,
                                });
                                scan.update(&probe);
                            } else if scan.step().is_err() {
                                break;
                            }
                        }
                        Err(_) => {
                            let mut name_probe = scan.clone();
                            let recovered = if name_probe.step().is_ok() {
                                let token = name_probe.current();
                                Some(
                                    scan.full_text().inner()
                                        [token.span.start..token.span.end]
                                        .to_owned(),
                                )
                            } else {
                                None
                            };
                            match recovered.as_deref() {
                                Some(raw) => {
                                    let name =
                                        raw.strip_suffix(char::from(95u8)).unwrap_or(raw);
                                    if name.eq_ignore_ascii_case("ul")
                                        || name.eq_ignore_ascii_case("ol")
                                    {
                                        if depth == 0 {
                                            break;
                                        }
                                        depth -= 1;
                                    } else if name.eq_ignore_ascii_case("li") {
                                        if let Some(head) = Self::take_wikidot_li_pair(
                                            &mut queue,
                                            &heads,
                                            &name_probe,
                                            depth,
                                        ) {
                                            paired.insert(head);
                                        }
                                    }
                                    if scan.step().is_err() {
                                        break;
                                    }
                                }
                                None => {
                                    if scan.step().is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {
                    if scan.step().is_err() {
                        break;
                    }
                }
            }
        }
        let mut prev_paired = None;
        for close in &mut closes {
            close.prev_paired = prev_paired;
            if close.paired.is_some() {
                prev_paired = Some(close.pos);
            }
        }
        self.set_wikidot_li_paired_heads(paired);
        self.set_wikidot_li_stream(heads, closes);
    }

    /// L85: region scan for one outer explicit list. Runs a force-close stack
    /// over div/collapsible/ul/ol/li events from after the `[[ul]]`/`[[ol]]`
    /// head to its closer. Live evidence: l-ul-li-cross, l-ul-li-closeout,
    /// l-ul-li-closein, l-ul-li-closein-c.
    pub(crate) fn compute_wikidot_li_cross_scan(&self) {
        use crate::parsing::parser::block_pairs::{
            WikidotLiCrossKind, WikidotLiCrossScan,
        };
        use std::collections::{BTreeMap, BTreeSet, VecDeque};

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum CrossFrame {
            List,
            Li,
            Div,
            Coll,
        }

        let mut scan = self.clone();
        let scan_start = scan.current().span.start;
        // Seed the region's own list frame: breaches must force-pop it so
        // its later redundant closer is marked.
        let mut stack: Vec<(CrossFrame, usize, bool)> =
            vec![(CrossFrame::List, scan_start, false)];
        let mut li_events: BTreeSet<usize> = BTreeSet::new();
        let mut depth = 0usize;
        let mut sat: VecDeque<CrossFrame> = VecDeque::new();
        let mut pairs: BTreeMap<usize, (WikidotLiCrossKind, bool)> = BTreeMap::new();
        let mut redundant: BTreeSet<usize> = BTreeSet::new();
        let mut implicit: BTreeSet<usize> = BTreeSet::new();

        // Pop through `index` (inclusive); inner frames with unseen closers
        // leave pending satisfactions for their later redundant closers.
        macro_rules! pop_through {
            ($index:expr) => {{
                while stack.len() > $index + 1 {
                    let (kind, _, seen) = stack.pop().expect("len checked");
                    if !seen {
                        sat.push_back(kind);
                    }
                }
                stack.pop();
            }};
        }

        loop {
            if scan.current().token == Token::InputEnd || scan.at_hard_body_end_boundary()
            {
                break;
            }
            if scan.skip_discarded_iftags_segment(false) {
                continue;
            }
            if scan.current().token == Token::LeftComment {
                use super::super::comment::comment_closer_token_count;
                loop {
                    if comment_closer_token_count(&scan).is_some() {
                        break;
                    }
                    if scan.step().is_err() {
                        break;
                    }
                    if scan.current().token == Token::InputEnd {
                        break;
                    }
                }
                continue;
            }
            match scan.current().token {
                Token::LeftBlock => {
                    let start = scan.current().span.start;
                    let mut probe = scan.clone();
                    if let Ok((name, _, _)) = probe.get_block_name(false) {
                        let name = name.strip_suffix('_').unwrap_or(&name);
                        let opaque = self.wikidot_li_cross_byte_opaque(start);
                        if name.eq_ignore_ascii_case("ul")
                            || name.eq_ignore_ascii_case("ol")
                        {
                            depth += 1;
                            if !opaque {
                                li_events.insert(start);
                                stack.push((CrossFrame::List, start, false));
                            }
                        } else if name.eq_ignore_ascii_case("li") {
                            if !opaque {
                                li_events.insert(start);
                                match stack.last().map(|(kind, _, _)| *kind) {
                                    Some(CrossFrame::Li) => {
                                        stack.pop();
                                        stack.push((CrossFrame::Li, start, false));
                                    }
                                    Some(CrossFrame::Div) | Some(CrossFrame::Coll) => {
                                        stack.push((CrossFrame::List, start, false));
                                        stack.push((CrossFrame::Li, start, false));
                                        implicit.insert(start);
                                    }
                                    _ => stack.push((CrossFrame::Li, start, false)),
                                }
                            }
                        } else if name.eq_ignore_ascii_case("div") {
                            if !opaque {
                                stack.push((CrossFrame::Div, start, false));
                            }
                        } else if name.eq_ignore_ascii_case("collapsible") {
                            if !opaque {
                                stack.push((CrossFrame::Coll, start, false));
                            }
                        }
                    }
                    if scan.step().is_err() {
                        break;
                    }
                }
                Token::LeftBlockEnd => {
                    let close_pos = scan.current().span.start;
                    let mut probe = scan.clone();
                    let recovered: Option<(std::borrow::Cow<'_, str>, bool)> =
                        match probe.get_end_block() {
                            Ok(name) => Some((name, true)),
                            Err(_) => {
                                let mut name_probe = scan.clone();
                                if name_probe.step().is_err() {
                                    None
                                } else {
                                    let token = name_probe.current();
                                    Some((
                                        std::borrow::Cow::Owned(
                                            scan.full_text().inner()
                                                [token.span.start..token.span.end]
                                                .to_owned(),
                                        ),
                                        false,
                                    ))
                                }
                            }
                        };
                    let Some((raw_name, clean)) = recovered else {
                        if scan.step().is_err() {
                            break;
                        }
                        continue;
                    };
                    let name = raw_name.strip_suffix('_').unwrap_or(&raw_name);
                    let opaque = self.wikidot_li_cross_byte_opaque(close_pos);
                    if name.eq_ignore_ascii_case("ul") || name.eq_ignore_ascii_case("ol")
                    {
                        if depth == 0 {
                            if !opaque && sat.front() == Some(&CrossFrame::List) {
                                sat.pop_front();
                                redundant.insert(close_pos);
                            }
                            break;
                        }
                        depth -= 1;
                        if !opaque {
                            li_events.insert(close_pos);
                            if let Some(index) = stack
                                .iter()
                                .rposition(|(kind, _, _)| *kind == CrossFrame::List)
                            {
                                pop_through!(index);
                            } else if sat.front() == Some(&CrossFrame::List) {
                                sat.pop_front();
                                redundant.insert(close_pos);
                            }
                        }
                        // L85: the region ends at its own list's closer;
                        // later blocks belong to later scans (a following
                        // `[[div]]` is not this region's breach).
                        if depth == 0 {
                            break;
                        }
                        if clean {
                            scan.update(&probe);
                        } else if scan.step().is_err() {
                            break;
                        }
                    } else if name.eq_ignore_ascii_case("li") {
                        if !opaque {
                            li_events.insert(close_pos);
                            if stack.last().map(|(kind, _, _)| *kind)
                                == Some(CrossFrame::Li)
                            {
                                stack.pop();
                            } else if sat.front() == Some(&CrossFrame::Li) {
                                sat.pop_front();
                                redundant.insert(close_pos);
                            } else if let Some(index) = stack
                                .iter()
                                .rposition(|(kind, _, _)| *kind == CrossFrame::Li)
                            {
                                stack[index].2 = true;
                                redundant.insert(close_pos);
                            }
                        }
                        if clean {
                            scan.update(&probe);
                        } else if scan.step().is_err() {
                            break;
                        }
                    } else if name.eq_ignore_ascii_case("div")
                        || name.eq_ignore_ascii_case("collapsible")
                    {
                        if !opaque {
                            let (frame, kind) = if name.eq_ignore_ascii_case("div") {
                                (CrossFrame::Div, WikidotLiCrossKind::Div)
                            } else {
                                (CrossFrame::Coll, WikidotLiCrossKind::Collapsible)
                            };
                            if let Some(index) = stack
                                .iter()
                                .rposition(|(stack_kind, _, _)| *stack_kind == frame)
                            {
                                let open_byte = stack[index].1;
                                let crossed = li_events
                                    .range(open_byte + 1..close_pos)
                                    .next()
                                    .is_some();
                                pop_through!(index);
                                pairs.insert(close_pos, (kind, crossed));
                            } else if sat.front() == Some(&frame) {
                                sat.pop_front();
                            } else {
                                while let Some((stack_kind, _, seen)) = stack.pop() {
                                    if !seen {
                                        sat.push_back(stack_kind);
                                    }
                                }
                                pairs.insert(close_pos, (kind, true));
                            }
                        }
                        if clean {
                            scan.update(&probe);
                        } else if scan.step().is_err() {
                            break;
                        }
                    } else if scan.step().is_err() {
                        break;
                    }
                }
                _ => {
                    if scan.step().is_err() {
                        break;
                    }
                }
            }
        }
        let region_end = scan.current().span.start;
        self.block_end_scan_cache.borrow_mut().wikidot_li_cross_scan =
            Some(WikidotLiCrossScan {
                divcoll_pairs: pairs,
                redundant,
                implicit_li: implicit,
                region_start: scan_start,
                region_end,
            });
    }

    fn wikidot_li_cross_byte_opaque(&self, byte: usize) -> bool {
        if self.lost_owner_literal_range_end(byte).is_some() {
            return true;
        }
        if self.wikidot_byte_on_list_item_line(byte) {
            return true;
        }
        if self.wikidot_head_on_table_row_line(byte) {
            return true;
        }
        if self.wikidot_byte_in_quote_run(byte) {
            return true;
        }
        false
    }

    pub(crate) fn wikidot_li_cross_scan_ran(&self) -> bool {
        self.block_end_scan_cache
            .borrow()
            .wikidot_li_cross_scan
            .is_some()
    }

    pub(crate) fn wikidot_li_cross_pair_at(
        &self,
        closer_byte: usize,
    ) -> Option<(
        crate::parsing::parser::block_pairs::WikidotLiCrossKind,
        bool,
    )> {
        self.block_end_scan_cache
            .borrow()
            .wikidot_li_cross_scan
            .as_ref()?
            .divcoll_pairs
            .get(&closer_byte)
            .copied()
    }

    pub(crate) fn wikidot_li_cross_redundant_at(&self, byte: usize) -> bool {
        self.block_end_scan_cache
            .borrow()
            .wikidot_li_cross_scan
            .as_ref()
            .is_some_and(|scan| scan.redundant.contains(&byte))
    }

    pub(crate) fn wikidot_li_cross_implicit_li_at(&self, byte: usize) -> bool {
        self.block_end_scan_cache
            .borrow()
            .wikidot_li_cross_scan
            .as_ref()
            .is_some_and(|scan| scan.implicit_li.contains(&byte))
    }

    pub(crate) fn wikidot_li_cross_clear(&self) {
        self.block_end_scan_cache.borrow_mut().wikidot_li_cross_scan = None;
    }

    /// L85: whether the line starting at the current line break carries
    /// content. Used for post-list break/bareness decisions. A line led by
    /// a block closer is a structural boundary, not same-para content
    /// (live keeps the break before an orphan closer: div/list-item-end).
    pub(crate) fn wikidot_next_line_has_content(&self) -> bool {
        if self.current().token != Token::LineBreak {
            return false;
        }
        for extracted in self.remaining() {
            match extracted.token {
                Token::Whitespace => {}
                Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                    return false;
                }
                // A line led by a block head is a structural boundary, not
                // same-paragraph content (live keeps the post-list break
                // before a following div: explicit-pre-dissolves-before-div).
                Token::LeftBlock | Token::LeftBlockStar | Token::LeftBlockEnd => {
                    return false;
                }
                _ => return true,
            }
        }
        false
    }

    /// L85: stop a list body at a scan-paired div/collapsible closer whose
    /// opener is genuinely outside (closein shapes). Scoped to list rules.
    pub(crate) fn wikidot_li_cross_foreign_stop_here(
        &self,
        block_rule: &BlockRule,
    ) -> bool {
        if !self.settings().layout.legacy() || self.discarding_hidden_body() {
            return false;
        }
        if !matches!(
            block_rule.name,
            "block-list-unordered" | "block-list-ordered" | "block-list-item"
        ) {
            return false;
        }
        if self.current().token != Token::LeftBlockEnd {
            return false;
        }
        let pos = self.current().span.start;
        let Some((kind, _)) = self.wikidot_li_cross_pair_at(pos) else {
            return false;
        };
        if !self.wikidot_ancestor_boundary_accepts(kind.boundary_name()) {
            return false;
        }
        true
    }

    /// L85: whether an implicit-list `[[li]]` stands here inside a
    /// div/collapsible body (cross shapes).
    pub(crate) fn wikidot_li_cross_implicit_li_here(&self, rule_name: &str) -> bool {
        if !self.settings().layout.legacy() || self.discarding_hidden_body() {
            return false;
        }
        if !matches!(rule_name, "block-div" | "block-collapsible") {
            return false;
        }
        if self.current().token != Token::LeftBlock {
            return false;
        }
        self.wikidot_li_cross_implicit_li_at(self.current().span.start)
    }

    /// L85: skip (drop) a scan-redundant `[[/li]]` inside div/collapsible
    /// bodies (cross shapes). Consumes the closer on success.
    pub(crate) fn wikidot_li_cross_skip_redundant_here(
        &mut self,
        rule_name: &str,
    ) -> bool {
        if !self.settings().layout.legacy() || self.discarding_hidden_body() {
            return false;
        }
        if !matches!(rule_name, "block-div" | "block-collapsible") {
            return false;
        }
        if self.exact_wikidot_li_close_end().is_none() {
            return false;
        }
        let pos = self.current().span.start;
        if !self.wikidot_li_cross_redundant_at(pos) {
            return false;
        }
        let mut probe = self.clone();
        if probe.get_end_block().is_err() {
            return false;
        }
        self.update(&probe);
        // L85: the redundant closer absorbs its line break (cross drops
        // `B`'s break before the implicit list).
        if self.current().token == Token::LineBreak {
            let _ = self.step();
        }
        true
    }

    pub(crate) fn wikidot_li_close_disposition_for_item(
        &self,
        head_pos: usize,
        close_pos: usize,
    ) -> WikidotLiCloseDisposition {
        let Some(record) = self.wikidot_li_close_record(close_pos) else {
            return WikidotLiCloseDisposition::End;
        };
        let Some(pair) = record.paired else {
            return WikidotLiCloseDisposition::Literal;
        };
        let Some(head_depth) = self.wikidot_li_head_depth(head_pos) else {
            return WikidotLiCloseDisposition::End;
        };
        if record.depth != head_depth {
            return WikidotLiCloseDisposition::End;
        }
        let Some(pair_depth) = self.wikidot_li_head_depth(pair) else {
            return WikidotLiCloseDisposition::End;
        };
        if pair_depth < record.depth {
            return WikidotLiCloseDisposition::Silent;
        }
        if self.wikidot_li_last_paired_close() == Some(close_pos) {
            return WikidotLiCloseDisposition::End;
        }
        let base = record.prev_paired.unwrap_or(0).max(head_pos);
        let intervening = self.wikidot_li_head_between(base, close_pos, head_depth);
        if pair == head_pos {
            if intervening {
                return WikidotLiCloseDisposition::Literal;
            }
            return WikidotLiCloseDisposition::End;
        }
        if intervening {
            return WikidotLiCloseDisposition::Literal;
        }
        if self.wikidot_li_paired_head_after(close_pos, head_depth) {
            return WikidotLiCloseDisposition::End;
        }
        WikidotLiCloseDisposition::Literal
    }

    pub(crate) fn wikidot_li_listbody_close_silent(&self, close_pos: usize) -> bool {
        let Some(record) = self.wikidot_li_close_record(close_pos) else {
            return false;
        };
        let Some(pair) = record.paired else {
            return false;
        };
        self.wikidot_li_head_depth(pair)
            .is_some_and(|pair_depth| pair_depth < record.depth)
    }

    pub(crate) fn exact_wikidot_li_close_end(&self) -> Option<usize> {
        if self.current().token != Token::LeftBlockEnd {
            return None;
        }
        let start = self.current().span.start;
        let mut probe = self.clone();
        let name = probe.get_end_block().ok()?;
        if !matches!(name, Cow::Borrowed(_)) {
            return None;
        }
        if !name.eq_ignore_ascii_case("li") {
            return None;
        }
        let end = probe.current().span.start;
        if !self.full_text().inner()[start..end].eq_ignore_ascii_case("[[/li]]") {
            return None;
        }
        Some(end)
    }

    pub(crate) fn wikidot_nested_list_opener_ahead(&self) -> bool {
        if self.current().token != Token::LineBreak {
            return false;
        }
        if !matches!(
            self.look_ahead(0).map(|token| token.token),
            Some(Token::LeftBlock)
        ) {
            return false;
        }
        let mut probe = self.clone();
        if probe.step().is_err() {
            return false;
        }
        let Ok((name, _, _)) = probe.get_block_name(false) else {
            return false;
        };
        let name = name.strip_suffix('_').unwrap_or(&name);
        name.eq_ignore_ascii_case("ul") || name.eq_ignore_ascii_case("ol")
    }

    fn wikidot_li_non_ending_close_here(&self) -> Option<WikidotLiCloseDisposition> {
        if self.discarding_hidden_body() {
            return None;
        }
        if self.current().token != Token::LeftBlockEnd {
            return None;
        }
        let close_pos = self.current().span.start;
        self.exact_wikidot_li_close_end()?;
        let Some(Some(head)) = self.wikidot_li_collect_top() else {
            return None;
        };
        match self.wikidot_li_close_disposition_for_item(head, close_pos) {
            WikidotLiCloseDisposition::End => None,
            disposition => Some(disposition),
        }
    }

    fn wikidot_li_non_ending_close_past_break(&self) -> bool {
        if self.discarding_hidden_body() {
            return false;
        }
        if self.current().token != Token::LineBreak {
            return false;
        }
        let mut probe = self.clone();
        if probe.step().is_err() {
            return false;
        }
        if probe.current().token != Token::LeftBlockEnd {
            return false;
        }
        let close_pos = probe.current().span.start;
        if probe.exact_wikidot_li_close_end().is_none() {
            return false;
        }
        let Some(Some(head)) = self.wikidot_li_collect_top() else {
            return false;
        };
        !matches!(
            self.wikidot_li_close_disposition_for_item(head, close_pos),
            WikidotLiCloseDisposition::End
        )
    }

    pub(crate) fn get_wikidot_li_body_segments(
        &mut self,
        block_rule: &BlockRule,
    ) -> ParseResult<'r, 't, Vec<Vec<Element<'t>>>> {
        self.push_body_end_boundary(
            block_rule_end_names(block_rule, self.settings().layout.legacy()),
            block_rule.name == "block-footnote",
        );
        let result = self.consume_wikidot_li_body_segments(block_rule);
        self.pop_body_end_boundary();
        result
    }

    fn consume_wikidot_li_body_segments(
        &mut self,
        block_rule: &BlockRule,
    ) -> ParseResult<'r, 't, Vec<Vec<Element<'t>>>> {
        let mut segments: Vec<Vec<Element<'t>>> = vec![Vec::new()];
        let mut all_errors = Vec::new();
        let mut paragraph_safe = true;
        let mut first = true;

        loop {
            if self.quotes_then_block_end(block_rule) {
                let prefix = self.take_quoted_closer_marks()?;
                segments
                    .last_mut()
                    .expect("segments always has a current segment")
                    .extend(prefix);
                if self
                    .verify_end_block(first, block_rule, false, false)
                    .is_some()
                {
                    return Ok(crate::parsing::ParseSuccess::new(
                        segments,
                        all_errors,
                        paragraph_safe,
                    ));
                }
            } else if self.prepare_quote_body_line()? == QuoteBodyLineStatus::Boundary {
                if self.quote_body_literal_residuals() == Some(true)
                    && self.current().token == Token::LineBreak
                {
                    self.step()?;
                    continue;
                }
                return Err(self.make_err(ParseErrorKind::EndOfInput));
            }

            if self.current().token == Token::ParagraphBreak
                && !self.discarding_hidden_body()
                && !self.in_wikidot_span_body()
                && !self.in_wikidot_literal_alias_body()
                && !self.in_wikidot_anchor_body()
            {
                self.step()?;
                first = false;
                segments.push(Vec::new());
                continue;
            }

            if !self.discarding_hidden_body() && self.wikidot_nested_list_opener_ahead() {
                self.step()?;
                first = false;
                segments
                    .last_mut()
                    .expect("segments always has a current segment")
                    .push(Element::LineBreak);
                continue;
            }

            // L85 scope-6: drop a break right after a nested explicit list
            // before same-para content (live keeps a raw newline there).
            if self.settings().layout.legacy()
                && !self.discarding_hidden_body()
                && self.current().token == Token::LineBreak
                && segments.last().is_some_and(|segment| {
                    matches!(
                        segment.last(),
                        Some(Element::List {
                            wikidot_explicit_block: true,
                            ..
                        })
                    )
                })
                && self.wikidot_next_line_has_content()
                && !self.body_end_boundary_accepts("iftags")
                && !self.has_pending_iftags_end_ahead()
            {
                self.step()?;
                first = false;
                continue;
            }

            if let Some(disposition) = self.wikidot_li_non_ending_close_here() {
                match disposition {
                    WikidotLiCloseDisposition::Silent => {
                        if self.get_end_block().is_err() {
                            self.step()?;
                        }
                        first = false;
                        continue;
                    }
                    WikidotLiCloseDisposition::Literal => {
                        let start = self.current().span.start;
                        let Some(end) = self.exact_wikidot_li_close_end() else {
                            return Err(self.make_err(ParseErrorKind::BlockExpectedEnd));
                        };
                        if self.get_end_block().is_err() {
                            self.step()?;
                            first = false;
                            continue;
                        }
                        first = false;
                        segments
                            .last_mut()
                            .expect("segments always has a current segment")
                            .push(Element::Text(Cow::Borrowed(
                                &self.full_text().inner()[start..end],
                            )));
                        continue;
                    }
                    WikidotLiCloseDisposition::End => {}
                }
            }

            if !self.wikidot_li_non_ending_close_past_break() {
                let result = self.verify_end_block(first, block_rule, false, false);
                if result.is_some() {
                    return Ok(crate::parsing::ParseSuccess::new(
                        segments,
                        all_errors,
                        paragraph_safe,
                    ));
                }
            }

            if self.terminate_at_wikidot_list_close(block_rule, first) {
                return Ok(crate::parsing::ParseSuccess::new(
                    segments,
                    all_errors,
                    paragraph_safe,
                ));
            }

            if self.wikidot_li_cross_foreign_stop_here(block_rule) {
                return Ok(crate::parsing::ParseSuccess::new(
                    segments,
                    all_errors,
                    paragraph_safe,
                ));
            }

            if self.discarding_hidden_body() && self.at_hidden_body_ancestor_boundary() {
                return Err(self.make_err(ParseErrorKind::BlockExpectedEnd));
            }

            if block_rule.name == "block-iftags"
                && let Some(designated) = self.wikidot_iftags_body_end()
                && self.current().span.start > designated
            {
                return Ok(crate::parsing::ParseSuccess::new(
                    segments,
                    all_errors,
                    paragraph_safe,
                ));
            }

            if block_rule.name != "block-bibliography"
                && !self.discarding_hidden_body()
                && self.wikidot_at_joined_bibliography_closer()
            {
                return Ok(crate::parsing::ParseSuccess::new(
                    segments,
                    all_errors,
                    paragraph_safe,
                ));
            }

            if !self.discarding_hidden_body() && self.at_ancestor_body_end_boundary() {
                return Err(self.make_err(ParseErrorKind::BlockExpectedEnd));
            }

            let wikidot_input_end = self.current().token == Token::InputEnd
                && self.settings().layout.legacy()
                && !self.discarding_hidden_body();
            if wikidot_input_end {
                return Ok(crate::parsing::ParseSuccess::new(
                    segments,
                    all_errors,
                    paragraph_safe,
                ));
            }

            first = false;
            match consume(self) {
                Ok(consumed) => {
                    self.take_wikidot_mixed_paragraph_ownership();
                    let elements = consumed.chain(&mut all_errors, &mut paragraph_safe);
                    segments
                        .last_mut()
                        .expect("segments always has a current segment")
                        .extend(elements);
                }
                Err(_error)
                    if self.discarding_hidden_body()
                        && self.wikidot_iftags_body_end().is_some_and(|designated| {
                            self.at_designated_iftags_end(designated)
                                && offset_inside_comment(
                                    self.full_text().inner(),
                                    designated,
                                )
                        }) => {}
                Err(_error)
                    if self.discarding_hidden_body()
                        && self.at_hidden_body_boundary() => {}
                Err(error) => return Err(error),
            }
        }
    }

    fn get_body_generic<F>(
        &mut self,
        block_rule: &BlockRule,
        mut process: F,
    ) -> Result<(&'r ExtractedToken<'t>, &'r ExtractedToken<'t>), ParseError>
    where
        F: FnMut(&mut Parser<'r, 't>) -> Result<(), ParseError>,
    {
        if let Some(error) = self.wikidot_unclosed_body_error(block_rule) {
            return Err(error);
        }

        let has_end_names = !block_rule.accepts_names.is_empty();
        debug_assert!(has_end_names, "block body has no valid end names");

        let mut first = true;
        let wikidot_math = self.settings().layout.legacy()
            && !self.discarding_hidden_body()
            && block_rule.name == "block-math";
        let mut nested_wikidot_math = 0;
        let mut suppress_nested_wikidot_math = false;
        let mut last_nested_close = None;
        let start = self.current();
        let designated_math_end = if wikidot_math {
            Self::wikidot_math_designated_end(self.full_text().inner(), start.span.start)
        } else {
            None
        };

        loop {
            let before_end = self.clone();
            let crossed_math_close =
                wikidot_math && self.at_wikidot_crossed_math_close(first);
            let at_end_block = self.verify_end_block(first, block_rule, false, false);

            if let Some(end) = at_end_block {
                if let Some(designated) = designated_math_end {
                    if end.span.start == designated {
                        self.wikidot_note_math_closer_line_suffix_for_br();
                        return Ok((start, end));
                    }
                    last_nested_close = Some((end, self.clone()));
                    self.update(&before_end);
                } else if nested_wikidot_math > 0 {
                    if Self::wikidot_math_closer_closes_nested_body(
                        self.full_text().inner(),
                        end.span.start,
                    ) {
                        self.wikidot_note_math_closer_line_suffix_for_br();
                        return Ok((start, end));
                    }
                    nested_wikidot_math -= 1;
                    last_nested_close = Some((end, self.clone()));
                    self.update(&before_end);
                } else if wikidot_math
                    && Self::math_body_has_unclosed_opener(
                        self.full_text().inner(),
                        start.span.start,
                        end.span.start,
                    )
                    && !Self::wikidot_math_closer_closes_nested_body(
                        self.full_text().inner(),
                        end.span.start,
                    )
                {
                    // With an unclosed opener still in the body (including one
                    // live does not count as a nested opener), a closer that
                    // is not the clean closer ending the last run of clean
                    // closer lines stays body text, so a later closer can
                    // still merge the whole span.
                    last_nested_close = Some((end, self.clone()));
                    self.update(&before_end);
                } else {
                    if wikidot_math {
                        self.wikidot_note_math_closer_line_suffix_for_br();
                    }
                    return Ok((start, end));
                }
            } else if crossed_math_close {
                suppress_nested_wikidot_math = true;
            } else if wikidot_math
                && !suppress_nested_wikidot_math
                && self.at_wikidot_nested_math_opener()
                && !self.at_wikidot_math_body_head_break(start)
            {
                nested_wikidot_math += 1;
            }

            if let Some(designated) = self.wikidot_iftags_body_end()
                && self.current().token == Token::LeftBlockEnd
                && self.current().span.start == designated
            {
                return Err(self.make_err(ParseErrorKind::BlockExpectedEnd));
            }

            if self.discarding_hidden_body()
                && self.at_hidden_body_boundary()
                && (!self.has_body_end_block(block_rule)
                    || matches!(block_rule.name, "block-math" | "block-raw"))
            {
                return Err(self.make_err(ParseErrorKind::BlockExpectedEnd));
            }

            let step_result = process(self).and_then(|()| self.step());

            match step_result {
                Ok(_) => first = false,
                Err(error) => {
                    if let Some((end, resume)) = last_nested_close.take() {
                        self.update(&resume);
                        return Ok((start, end));
                    }

                    return Err(error);
                }
            }
        }
    }

    /// Whether a math body span holds a line-start `[[math]]` opener without
    /// a matching line-start `[[/math]]` closer.
    ///
    /// This mirrors `at_wikidot_nested_math_opener` for body text the token
    /// scan never visits as an opener (notably an opener consumed as the
    /// body's first line), so that a trailing-text closer with a pending
    /// opener nests instead of ending the block. Indented markers and
    /// same-line closers do not count, exactly as in the token scan.
    fn math_body_has_unclosed_opener(text: &str, start: usize, end: usize) -> bool {
        let Some(body) = text.get(start..end) else {
            return false;
        };
        let mut depth = 0i32;
        let mut first = true;
        for line in body.split('\n') {
            let line_start = !first
                || start == 0
                || text
                    .get(..start)
                    .is_some_and(|before| before.ends_with('\n'));
            first = false;
            if !line_start {
                continue;
            }
            if let Some(head) = line.strip_prefix("[[")
                && !head.starts_with('/')
                && let Some(marker) = head.split("]]").next()
            {
                let mut parts = marker.split_whitespace();
                if parts
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case("math"))
                {
                    let name = parts.next();
                    if parts.next().is_none()
                        && name.is_none_or(super::blocks::wikidot_math_name)
                    {
                        depth += 1;
                        continue;
                    }
                }
            }
            if let Some(close) = line.strip_prefix("[[/")
                && close
                    .get(..4)
                    .is_some_and(|name| name.eq_ignore_ascii_case("math"))
                && let Some(after) = close.get(4..)
                && let Some(trailer) = after.strip_prefix("]]")
                && (trailer.is_empty() || trailer.starts_with([' ', '\t']))
            {
                depth -= 1;
            }
        }
        depth > 0
    }

    fn at_wikidot_nested_math_opener(&self) -> bool {
        if !matches!(
            self.current().token,
            Token::LineBreak | Token::ParagraphBreak
        ) {
            return false;
        }
        let start = self.current().span.end;
        let source = &self.full_text().inner()[start..];
        let Some(end) = source.find("]]") else {
            return false;
        };
        let mut parts = source[..end].trim_start_matches("[[").split_whitespace();
        if !parts
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("math"))
        {
            return false;
        }
        let name = parts.next();
        parts.next().is_none() && name.is_none_or(super::blocks::wikidot_math_name)
    }

    /// Whether the text at `rest` begins with a clean math closer line.
    ///
    /// Clean means the closer is the whole line: no trailing blanks or
    /// text before the line break. Live Wikidot only ends a body that has
    /// an unclosed nested opener pending at the first closer of the last
    /// contiguous run of clean closer lines; a closer with trailing blanks
    /// stays body text.
    fn wikidot_math_is_ws(byte: u8) -> bool {
        matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
    }

    fn wikidot_math_fossil_args_end(bytes: &[u8], start: usize) -> bool {
        let mut j = start;
        while j < bytes.len() && Self::wikidot_math_is_ws(bytes[j]) {
            j += 1;
        }
        if j == bytes.len() {
            return true;
        }
        let mut k = start;
        if k >= bytes.len() || !Self::wikidot_math_is_ws(bytes[k]) {
            return false;
        }
        while k < bytes.len() && Self::wikidot_math_is_ws(bytes[k]) {
            k += 1;
        }
        let key_start = k;
        while k < bytes.len() && (bytes[k].is_ascii_alphanumeric()) {
            k += 1;
        }
        let mut key_end = k;
        while key_end > key_start {
            if bytes.get(key_end) == Some(&b'=') && bytes.get(key_end + 1) == Some(&b'"')
            {
                let mut v = key_end + 2;
                while v < bytes.len() && bytes[v] != b'"' {
                    v += 1;
                }
                if v < bytes.len() && Self::wikidot_math_fossil_args_end(bytes, v + 1) {
                    return true;
                }
            }
            key_end -= 1;
        }
        false
    }

    fn wikidot_math_fossil_head_parses(head: &str) -> bool {
        let bytes = head.as_bytes();
        if !bytes.is_empty() && Self::wikidot_math_is_ws(bytes[0]) {
            let mut j = 0;
            while j < bytes.len() && Self::wikidot_math_is_ws(bytes[j]) {
                j += 1;
            }
            let mut k = j;
            loop {
                if Self::wikidot_math_fossil_args_end(bytes, k) {
                    return true;
                }
                if k < bytes.len()
                    && (bytes[k].is_ascii_alphanumeric() || bytes[k] == b'_')
                {
                    k += 1;
                } else {
                    break;
                }
            }
        }
        Self::wikidot_math_fossil_args_end(bytes, 0)
    }

    fn wikidot_math_fossil_close_at(text: &str, p: usize) -> Option<usize> {
        let bytes = text.as_bytes();
        if p >= bytes.len() || bytes[p] != b'\n' {
            return None;
        }
        if !text[p + 1..]
            .get(..9)
            .is_some_and(|part| part.eq_ignore_ascii_case("[[/math]]"))
        {
            return None;
        }
        let after = p + 10;
        if after >= bytes.len() {
            return Some(after);
        }
        if Self::wikidot_math_is_ws(bytes[after]) {
            return Some(after + 1);
        }
        None
    }

    fn wikidot_math_fossil_match_at(
        text: &str,
        pos: usize,
        memo: &mut std::collections::HashMap<usize, Option<(usize, usize, usize)>>,
        budget: &mut usize,
    ) -> Option<(usize, usize, usize)> {
        if let Some(cached) = memo.get(&pos) {
            return *cached;
        }
        let mut result = None;
        if text[pos..]
            .get(..6)
            .is_some_and(|part| part.eq_ignore_ascii_case("[[math"))
        {
            let line_end = text[pos..].find('\n').map_or(text.len(), |idx| pos + idx);
            let mut search = pos + 6;
            while let Some(rel) = text[search..line_end].find("]]") {
                let head_end = search + rel;
                if Self::wikidot_math_fossil_head_parses(&text[pos + 6..head_end])
                    && let Some(found) = Self::wikidot_math_fossil_search_body(
                        text,
                        head_end + 2,
                        memo,
                        budget,
                    )
                {
                    result = Some((found.0, found.1, head_end + 2));
                    break;
                }
                search = head_end + 1;
            }
        }
        memo.insert(pos, result);
        result
    }

    fn wikidot_math_fossil_search_body(
        text: &str,
        start: usize,
        memo: &mut std::collections::HashMap<usize, Option<(usize, usize, usize)>>,
        budget: &mut usize,
    ) -> Option<(usize, usize)> {
        struct Frame {
            q: usize,
            stage: u8,
        }
        let bytes = text.as_bytes();
        let mut dead = std::collections::HashSet::new();
        let mut stack: Vec<Frame> = vec![Frame { q: start, stage: 0 }];
        let mut outcome: Option<(usize, usize)> = None;
        let mut have_outcome = false;
        while let Some(frame) = stack.last_mut() {
            if have_outcome {
                let value = outcome;
                have_outcome = false;
                outcome = None;
                match frame.stage {
                    1 => {
                        if value.is_some() {
                            let done = stack.pop().unwrap();
                            if stack.is_empty() {
                                return value;
                            }
                            let _ = done;
                            outcome = value;
                            have_outcome = true;
                        } else {
                            frame.stage = 2;
                        }
                        continue;
                    }
                    _ => {
                        let done = stack.pop().unwrap();
                        if value.is_none() {
                            dead.insert(done.q);
                        }
                        if stack.is_empty() {
                            return value;
                        }
                        outcome = value;
                        have_outcome = true;
                        continue;
                    }
                }
            }
            let q = frame.q;
            if dead.contains(&q) {
                let done = stack.pop().unwrap();
                let _ = done;
                if stack.is_empty() {
                    return None;
                }
                outcome = None;
                have_outcome = true;
                continue;
            }
            match frame.stage {
                0 => {
                    if let Some(end) = Self::wikidot_math_fossil_close_at(text, q) {
                        let value = Some((q + 1, end));
                        let done = stack.pop().unwrap();
                        let _ = done;
                        if stack.is_empty() {
                            return value;
                        }
                        outcome = value;
                        have_outcome = true;
                        continue;
                    }
                    frame.stage = 1;
                }
                1 => {
                    let line_start = q == 0 || bytes.get(q - 1) == Some(&b'\n');
                    let sub = if line_start {
                        Self::wikidot_math_fossil_match_at(text, q, memo, budget)
                    } else {
                        None
                    };
                    match sub {
                        Some((_, sub_end, _)) => {
                            if *budget == 0 {
                                return None;
                            }
                            *budget -= 1;
                            stack.push(Frame {
                                q: sub_end,
                                stage: 0,
                            });
                        }
                        None => {
                            frame.stage = 2;
                        }
                    }
                }
                _ => {
                    if q < bytes.len() {
                        if *budget == 0 {
                            return None;
                        }
                        *budget -= 1;
                        frame.stage = 3;
                        stack.push(Frame { q: q + 1, stage: 0 });
                    } else {
                        let done = stack.pop().unwrap();
                        dead.insert(done.q);
                        if stack.is_empty() {
                            return None;
                        }
                        outcome = None;
                        have_outcome = true;
                    }
                }
            }
        }
        None
    }

    fn wikidot_math_prev_valid_closer(text: &str, before: usize) -> Option<usize> {
        let mut found = None;
        let mut offset = 0;
        for line in text[..before].split('\n') {
            if line
                .get(..9)
                .is_some_and(|part| part.eq_ignore_ascii_case("[[/math]]"))
            {
                let rest = &line[9.min(line.len())..];
                if rest.is_empty() || rest.starts_with([' ', '\t', '\r']) {
                    found = Some(offset);
                }
            }
            offset += line.len() + 1;
        }
        found
    }

    fn wikidot_math_designated_end(text: &str, body_start: usize) -> Option<usize> {
        let mut j = body_start;
        if j > 0 && text.as_bytes().get(j - 1) == Some(&b'\n') {
            j -= 1;
        }
        if j > 0 && text.as_bytes().get(j - 1) == Some(&b'\r') {
            j -= 1;
        }
        let opener = text[..j].rfind('\n').map_or(0, |idx| idx + 1);
        if !text[opener..]
            .get(..6)
            .is_some_and(|part| part.eq_ignore_ascii_case("[[math"))
        {
            return None;
        }
        let mut memo = std::collections::HashMap::new();
        let mut budget: usize = 4_000_000;
        let (close_start, _match_end, fossil_body_start) =
            Self::wikidot_math_fossil_match_at(text, opener, &mut memo, &mut budget)?;
        let body = &text[fossil_body_start..close_start - 1];
        let mut trailing_clean = 0usize;
        for line in body.split('\n').rev() {
            let trimmed = line.strip_suffix('\r').unwrap_or(line);
            if trimmed.eq_ignore_ascii_case("[[/math]]") {
                trailing_clean += 1;
            } else {
                break;
            }
        }
        if trailing_clean % 2 == 1 {
            return Self::wikidot_math_prev_valid_closer(text, close_start);
        }
        Some(close_start)
    }

    fn wikidot_math_clean_closer_line(rest: &str) -> bool {
        let line = rest.split('\n').next().unwrap_or(rest);
        let line = line.strip_suffix('\r').unwrap_or(line);
        line.eq_ignore_ascii_case("[[/math]]")
    }

    /// Whether a closer with an unclosed opener still in the body is the
    /// one live Wikidot ends the block at: a clean closer line directly
    /// followed by another clean closer line, with no clean closer after
    /// that run.
    fn wikidot_math_closer_closes_nested_body(text: &str, closer_start: usize) -> bool {
        if !Self::wikidot_math_clean_closer_line(&text[closer_start..]) {
            return false;
        }
        let Some(newline) = text[closer_start..].find('\n') else {
            return false;
        };
        let next = closer_start + newline + 1;
        if next >= text.len() || !Self::wikidot_math_clean_closer_line(&text[next..]) {
            return false;
        }
        !Self::wikidot_math_later_clean_closer(text, closer_start)
    }

    /// Whether the parser sits on the math body's head break: the line break
    /// that ends the head's own line.
    ///
    /// An opener right after it directly continues the head, so live Wikidot
    /// does not count it as a nested opener. An opener on any later line
    /// counts, so a clean closer merges forward instead. The span check
    /// covers both body-start shapes (at the head's newline, or on the token
    /// right after the head).
    fn at_wikidot_math_body_head_break(&self, start: &ExtractedToken<'_>) -> bool {
        self.current().span.start == start.span.start
            || self.current().span.start == start.span.end
    }

    /// Whether a clean math closer line follows the run of clean closer
    /// lines that starts at `closer_start`.
    fn wikidot_math_later_clean_closer(text: &str, closer_start: usize) -> bool {
        let mut position = closer_start;
        loop {
            let Some(newline) = text[position..].find('\n') else {
                return false;
            };
            position += newline + 1;
            if !Self::wikidot_math_clean_closer_line(&text[position..]) {
                break;
            }
        }
        while position < text.len() {
            if Self::wikidot_math_clean_closer_line(&text[position..]) {
                return true;
            }
            let Some(newline) = text[position..].find('\n') else {
                return false;
            };
            position += newline + 1;
        }
        false
    }

    fn at_wikidot_crossed_math_close(&self, first_iteration: bool) -> bool {
        let mut close = self.clone();
        if !first_iteration
            && (close.current().token != Token::LineBreak || close.step().is_err())
        {
            return false;
        }
        if close.current().token != Token::LeftBlockEnd {
            return false;
        }
        let Ok(name) = close.get_end_block() else {
            return false;
        };
        name.eq_ignore_ascii_case("math")
            && !matches!(
                close.current().token,
                Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
            )
    }

    pub fn get_body_text(
        &mut self,
        block_rule: &BlockRule,
    ) -> Result<Cow<'t, str>, ParseError> {
        if self.settings().layout.legacy() && block_rule.name == "block-math" {
            return self.get_math_body_text(block_rule);
        }

        if let Some(required_depth) = self.native_blockquote_depth()
            && (self.quote_body_needs_prefix()
                || self.current().token == Token::LineBreak)
        {
            return self.get_native_blockquote_body_text(block_rule, required_depth);
        }

        let (start, end) = self.get_body_generic(block_rule, |_| Ok(()))?;
        let slice = self.full_text().slice_partial(start, end);
        Ok(Cow::Borrowed(slice))
    }

    pub(crate) fn get_wikidot_module_optional_body(
        &mut self,
        block_rule: &BlockRule,
    ) -> Result<Cow<'t, str>, ParseError> {
        if self.take_wikidot_module_empty_body() {
            return Ok(Cow::Borrowed(""));
        }
        if !self.has_body_end_block(block_rule) {
            return Ok(Cow::Borrowed(""));
        }
        let mut probe = self.clone();
        match probe.get_body_text(block_rule) {
            Ok(body)
                if body.trim().is_empty() || wikidot_body_starts_nested_module(&body) =>
            {
                Ok(Cow::Borrowed(""))
            }
            Ok(body) => {
                self.update(&probe);
                Ok(body)
            }
            Err(error) if error.kind() == ParseErrorKind::EndOfInput => {
                Ok(Cow::Borrowed(""))
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn skip_wikidot_module_body(&mut self, block_rule: &BlockRule) {
        if self.take_wikidot_module_empty_body() {
            return;
        }
        if !self.has_body_end_block(block_rule) {
            return;
        }
        let mut probe = self.clone();
        if let Ok(body) = probe.get_body_text(block_rule)
            && !body.trim().is_empty()
            && !wikidot_body_starts_nested_module(&body)
        {
            self.update(&probe);
        }
    }

    pub(crate) fn wikidot_module_head_has_separator(
        &self,
        opener_start: usize,
        name: &str,
        subname: &str,
    ) -> bool {
        let source = self.full_text().inner();
        let head_end = self.current().span.start;
        let Some(inner) = source
            .get(opener_start..head_end)
            .and_then(|head| head.strip_prefix("[["))
            .and_then(|head| head.strip_suffix("]]"))
        else {
            return false;
        };
        let Some(keyword) = inner.get(..name.len()) else {
            return false;
        };
        if !keyword.eq_ignore_ascii_case(name) {
            return false;
        }
        let rest = inner[name.len()..].trim_start_matches([' ', '\t']);
        let Some(rest) = rest.strip_prefix(subname) else {
            return false;
        };
        !rest.is_empty()
    }

    pub(crate) fn wikidot_module_head_rest_is_line_end(
        &self,
    ) -> Result<bool, ParseError> {
        let mut probe = self.clone();
        while probe.current().token == Token::Whitespace {
            probe.step()?;
        }
        Ok(matches!(
            probe.current().token,
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
        ))
    }

    pub(crate) fn consume_wikidot_module_same_line_close(
        &mut self,
        block_rule: &BlockRule,
    ) -> Result<bool, ParseError> {
        let mut probe = self.clone();
        while probe.current().token == Token::Whitespace {
            probe.step()?;
        }
        if probe.current().token != Token::LeftBlockEnd {
            return Ok(false);
        }
        let Ok(close_name) = probe.get_end_block() else {
            return Ok(false);
        };
        let close_name = close_name.strip_suffix('_').unwrap_or(&close_name);
        if !block_rule_accepts_end_name(block_rule, close_name, true) {
            return Ok(false);
        }
        while probe.current().token == Token::Whitespace {
            probe.step()?;
        }
        if !matches!(
            probe.current().token,
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
        ) {
            return Ok(false);
        }
        if probe.current().token == Token::LineBreak {
            probe.step()?;
        }
        self.update(&probe);
        Ok(true)
    }

    /// Extracts a block body that must keep a trailing escaped line break.
    ///
    /// The tokenizer fuses a continuation backslash into the following line
    /// break token (its slice is `"\\\n"`), so that token's span starts
    /// *before* the backslash. Verbatim block bodies keep the backslash, so
    /// the body has to extend through the token slice; the caller's trim
    /// removes the resulting trailing newline.
    pub fn get_body_text_keeping_continued_break(
        &mut self,
        block_rule: &BlockRule,
    ) -> Result<Cow<'t, str>, ParseError> {
        let (start, end) = self.get_body_generic(block_rule, |_| Ok(()))?;
        let end = if end.slice.starts_with('\\') {
            end.span.end
        } else {
            end.span.start
        };
        Ok(Cow::Borrowed(
            self.full_text().slice_range(start.span.start, end),
        ))
    }

    fn get_math_body_text(
        &mut self,
        block_rule: &BlockRule,
    ) -> Result<Cow<'t, str>, ParseError> {
        let _ = self.take_wikidot_math_closer_line_br_prefix();
        let walk_start = self.clone();
        let (start, end) = self.get_body_generic(block_rule, |_| Ok(()))?;
        let closer_line_br_prefix = self.take_wikidot_math_closer_line_br_prefix();
        let body_start = start.span.start;
        let body_end = end.span.start;
        let mut removals: Vec<(usize, usize)> = Vec::new();
        let mut gated: Vec<(usize, usize)> = Vec::new();
        let mut walk = walk_start;
        loop {
            let token = walk.current();
            if token.token == Token::InputEnd || token.span.start >= body_end {
                break;
            }
            if token.token == Token::LeftBlock {
                let head_start = token.span.start;
                if gated
                    .iter()
                    .any(|&(lo, hi)| head_start >= lo && head_start < hi)
                {
                    if walk.step().is_err() {
                        break;
                    }
                    continue;
                }
                let mut probe = walk.clone();
                if probe.skip_discarded_iftags_segment(false) {
                    let segment_end = probe.current().span.start;
                    if body_end >= head_start && body_end < segment_end {
                        return Err(self.make_err(ParseErrorKind::RuleFailed));
                    }
                    removals.push((head_start, segment_end));
                    walk.update(&probe);
                    continue;
                }
                let mut head_probe = walk.clone();
                let is_iftags = match head_probe.get_block_name(false) {
                    Ok((name, _, _)) => name.eq_ignore_ascii_case("iftags"),
                    Err(_) => false,
                };
                if is_iftags {
                    let mut past_head = walk.clone();
                    let mut head_end = None;
                    loop {
                        let current = past_head.current();
                        if past_head.step().is_err() {
                            break;
                        }
                        if current.token == Token::RightBlock {
                            head_end = Some(past_head.current().span.start);
                            break;
                        }
                    }
                    if let Some(head_end) = head_end {
                        let (pairing, _) = past_head.wikidot_iftags_pairing_end();
                        if let Some(designated) = pairing {
                            removals.push((head_start, head_end));
                            if designated < body_end {
                                let mut end_probe = past_head.clone();
                                let mut end_span_end = None;
                                while end_probe.current().span.start < designated {
                                    if end_probe.step().is_err() {
                                        break;
                                    }
                                }
                                if end_probe.current().span.start == designated {
                                    let mut close = end_probe.clone();
                                    if close.get_end_block().is_ok() {
                                        end_span_end = Some(close.current().span.start);
                                    }
                                }
                                if let Some(end_span_end) = end_span_end {
                                    removals.push((designated, end_span_end));
                                    gated.push((head_start, end_span_end));
                                } else {
                                    gated.push((head_start, designated));
                                }
                            } else if designated > body_end {
                                self.push_pending_iftags_end_swallow(designated);
                                gated.push((head_start, designated));
                            } else {
                                gated.push((head_start, designated));
                            }
                        }
                    }
                }
            }
            if walk.step().is_err() {
                break;
            }
        }
        self.set_wikidot_math_closer_line_br_prefix(closer_line_br_prefix);
        if removals.is_empty() {
            let slice = self.full_text().slice_partial(start, end);
            return Ok(Cow::Borrowed(slice));
        }
        removals.sort();
        let source = self.full_text().inner();
        let mut out = String::new();
        let mut cursor = body_start;
        for (span_start, span_end) in removals {
            if span_start > cursor {
                out.push_str(&source[cursor..span_start]);
            }
            cursor = cursor.max(span_end);
        }
        if cursor < body_end {
            out.push_str(&source[cursor..body_end]);
        }
        Ok(Cow::Owned(out))
    }

    pub(crate) fn get_body_text_after_skipping_end_blocks(
        &mut self,
        block_rule: &BlockRule,
        mut end_blocks_to_skip: usize,
    ) -> Result<Cow<'t, str>, ParseError> {
        let start = self.current();
        let mut first = true;

        loop {
            if let Some(end) = self.verify_end_block(first, block_rule, false, false) {
                if end_blocks_to_skip == 0 {
                    let end = if end.slice.starts_with('\\') {
                        end.span.end
                    } else {
                        end.span.start
                    };
                    let slice = self.full_text().slice_range(start.span.start, end);
                    return Ok(Cow::Borrowed(slice));
                }
                end_blocks_to_skip -= 1;
                first = false;
                continue;
            }

            self.step()?;
            first = false;
        }
    }

    pub(crate) fn body_has_delayed_input(&self, block_rule: &BlockRule) -> bool {
        let mut probe = self.clone();
        let Ok((start, end)) = probe.get_body_generic(block_rule, |_| Ok(())) else {
            return false;
        };
        let range = start.span.start..end.span.end;
        probe.has_generated_in_range(range.clone())
            || probe.has_runtime_literal_in_range(range)
    }

    fn scan_absolute_quote_prefix(
        &self,
        required_depth: usize,
    ) -> Option<(usize, usize, Self)> {
        let mut parser = self.clone();
        let mut absolute_depth = 0;
        let mut content_start = None;

        while parser.current().token == Token::Quote {
            let quote = parser.current();
            let depth_before = absolute_depth;
            absolute_depth += quote.slice.len();
            parser.step().ok()?;
            parser.get_optional_space().ok()?;

            if content_start.is_none() && absolute_depth >= required_depth {
                content_start = Some(if absolute_depth == required_depth {
                    parser.current().span.start
                } else {
                    quote.span.start + (required_depth - depth_before)
                });
            }
        }

        Some((content_start?, absolute_depth, parser))
    }

    #[rustfmt::skip]
    fn get_native_blockquote_body_text(
        &mut self,
        block_rule: &BlockRule,
        required_depth: usize,
    ) -> Result<Cow<'t, str>, ParseError> {
        let mut close_scan = self.clone();
        if close_scan.current().token == Token::LineBreak {
            close_scan.step()?;
        }
        let has_close = close_scan.has_native_blockquote_body_end_with_mode(block_rule, required_depth, true);
        if !has_close {
            return Err(self.make_err(ParseErrorKind::RuleFailed));
        }

        let mut body = String::new();
        let mut trailing_line_break_len = 0;

        if self.current().token == Token::LineBreak {
            self.step()?;
        }

        loop {
            let mut outer_prepared = self.clone();
            if outer_prepared.prepare_quote_body_line()? != QuoteBodyLineStatus::Prepared
            {
                return Err(self.make_err(ParseErrorKind::EndOfInput));
            }

            let (content_start, absolute_depth, mut scan) = self.scan_absolute_quote_prefix(required_depth).ok_or_else(|| self.make_err(ParseErrorKind::EndOfInput))?;
            scan.set_quote_body_cursor(outer_prepared.quote_body_cursor());

            while !token_is_body_boundary(scan.current().token) {
                let marker_start = scan.current().span.start;
                let mut end = scan.clone();
                if absolute_depth == required_depth
                    && let Ok(name) = end.get_end_block()
                {
                    let name = name.strip_suffix('_').unwrap_or(&name);
                    if block_rule_accepts_end_name(
                        block_rule,
                        name,
                        self.settings().layout.legacy(),
                    ) {
                        if marker_start == content_start {
                            body.truncate(body.len() - trailing_line_break_len);
                        } else {
                            body.push_str(
                                &self.full_text().inner()[content_start..marker_start],
                            );
                        }
                        self.update(&end);
                        return Ok(Cow::Owned(body));
                    }
                }

                scan.step()?;
            }

            if scan.current().token != Token::LineBreak {
                return Err(self.make_err(ParseErrorKind::EndOfInput));
            }

            let content_end = scan.current().span.start;
            body.push_str(&self.full_text().inner()[content_start..content_end]);
            let line_break = scan.current().slice;
            body.push_str(line_break);
            trailing_line_break_len = line_break.len();
            self.update(&scan);
            self.step()?;
        }
    }

    #[inline]
    pub fn get_body_elements(
        &mut self,
        block_rule: &BlockRule,
        as_paragraphs: bool,
    ) -> ParseResult<'r, 't, Vec<Element<'t>>> {
        self.get_body_elements_internal(block_rule, as_paragraphs, false, false)
    }

    fn get_body_elements_internal(
        &mut self,
        block_rule: &BlockRule,
        as_paragraphs: bool,
        restrict_quote_close: bool,
        allow_inline_quote_close: bool,
    ) -> ParseResult<'r, 't, Vec<Element<'t>>> {
        if let Some(error) = self.wikidot_unclosed_body_error(block_rule) {
            return Err(error);
        }

        let track_boundary = self.discarding_hidden_body();
        if track_boundary {
            self.push_hidden_body_boundary(
                block_rule_end_names(block_rule, self.settings().layout.legacy()),
                block_rule.accepts_newlines_outside_body,
            );
        }

        self.push_body_end_boundary(
            block_rule_end_names(block_rule, self.settings().layout.legacy()),
            block_rule.name == "block-footnote",
        );

        let result = if as_paragraphs {
            self.get_body_elements_paragraphs(
                block_rule,
                restrict_quote_close,
                allow_inline_quote_close,
            )
        } else {
            self.get_body_elements_no_paragraphs(
                block_rule,
                restrict_quote_close,
                allow_inline_quote_close,
            )
        };

        self.pop_body_end_boundary();

        if track_boundary {
            self.pop_hidden_body_boundary();
        }

        result
    }

    pub(crate) fn get_body_elements_with_context(
        &mut self,
        block_rule: &BlockRule,
        as_paragraphs: bool,
        body_start: BlockBodyStart,
        wikidot_body_opener_byte: Option<usize>,
    ) -> ParseResult<'r, 't, Vec<Element<'t>>> {
        self.get_body_elements_with_context_policy(
            block_rule,
            as_paragraphs,
            body_start,
            false,
            wikidot_body_opener_byte,
        )
    }

    pub(crate) fn get_body_elements_with_literal_quote_context(
        &mut self,
        block_rule: &BlockRule,
        as_paragraphs: bool,
        body_start: BlockBodyStart,
        wikidot_body_opener_byte: Option<usize>,
    ) -> ParseResult<'r, 't, Vec<Element<'t>>> {
        self.get_body_elements_with_context_policy(
            block_rule,
            as_paragraphs,
            body_start,
            true,
            wikidot_body_opener_byte,
        )
    }

    fn get_body_elements_with_context_policy(
        &mut self,
        block_rule: &BlockRule,
        as_paragraphs: bool,
        body_start: BlockBodyStart,
        literal_residual_quotes: bool,
        wikidot_body_opener_byte: Option<usize>,
    ) -> ParseResult<'r, 't, Vec<Element<'t>>> {
        let opener_for_line =
            wikidot_body_opener_byte.unwrap_or_else(|| self.current().span.start);
        if self.settings().layout.legacy()
            && !self.discarding_hidden_body()
            && body_start == BlockBodyStart::Inline
            && wikidot_requires_next_physical_line(block_rule)
            && !(block_rule.name == "block-div"
                && wikidot_physical_line_has_textual_div_closer(
                    self.full_text().inner(),
                    opener_for_line,
                ))
        {
            return Err(self.make_err(ParseErrorKind::RuleFailed));
        }

        let Some(mut required_depth) = self.native_blockquote_depth() else {
            return self.get_body_elements(block_rule, as_paragraphs);
        };
        let inline_quoted_collapsible = body_start == BlockBodyStart::Inline
            && self.settings().layout.legacy()
            && !self.discarding_hidden_body()
            && block_rule.name == "block-collapsible";
        if body_start != BlockBodyStart::NextPhysicalLine && !inline_quoted_collapsible {
            return self.get_body_elements(block_rule, as_paragraphs);
        }
        let allow_inline_quote_close = self.settings().layout.legacy()
            && !self.discarding_hidden_body()
            && block_rule.name == "block-collapsible";
        let tabview_quote_ejection = self.settings().layout.legacy()
            && !self.discarding_hidden_body()
            && block_rule.name == "block-tabview";
        let mut has_close = if tabview_quote_ejection {
            self.has_native_blockquote_body_end_tolerant(block_rule)
        } else {
            self.has_native_blockquote_body_end_with_mode(
                block_rule,
                required_depth,
                allow_inline_quote_close,
            )
        };
        // A note inside a footnote or tab still sits on one physical `>`.
        // The stacked depth counts that marker twice, so the strict scan
        // misses `> [[/note]]`. The line's own marker depth is the cursor.
        if !has_close
            && block_rule.name == "block-note"
            && required_depth > 1
            && self.has_native_blockquote_body_end_with_mode(block_rule, 1, false)
        {
            required_depth = 1;
            has_close = true;
        }
        let has_later_close =
            allow_inline_quote_close && !has_close && self.has_body_end_block(block_rule);
        let has_surplus_list_close = !has_close
            && !has_later_close
            && literal_residual_quotes
            && self.has_surplus_quote_list_close(block_rule, required_depth);
        if !has_close && !has_later_close && !has_surplus_list_close {
            if block_rule.name == "block-note" {
                return self.get_body_elements(block_rule, as_paragraphs);
            }
            return Err(self.make_err(ParseErrorKind::RuleFailed));
        }

        let previous_cursor = self.quote_body_cursor();
        if literal_residual_quotes {
            self.install_quote_body_cursor_with_literal_residuals(required_depth);
        } else {
            self.install_quote_body_cursor(required_depth);
        }
        let previous_boundary_policy = self.quote_boundary_closes_body();
        self.set_quote_boundary_closes_body(has_later_close || tabview_quote_ejection);
        let result = self.get_body_elements_internal(
            block_rule,
            as_paragraphs,
            true,
            allow_inline_quote_close,
        );
        let ended_at_boundary = if has_later_close && result.is_ok() {
            let mut boundary = self.clone();
            boundary.prepare_quote_body_line()? == QuoteBodyLineStatus::Boundary
        } else {
            false
        };
        if ended_at_boundary {
            self.set_pending_wikidot_collapsible_closer(true);
        }
        self.set_quote_boundary_closes_body(previous_boundary_policy);
        self.set_quote_body_cursor(previous_cursor);

        match result {
            Ok(success) => {
                // Stepping this break deletes the parent's `<br>` and is what
                // lets the parent strip the next `>`. Holding the line as
                // already prepared keeps the break and still arms that strip
                // when the parent steps it. Restoring `NeedPrefix` here makes
                // the break a boundary and the enclosing center stays literal.
                if block_rule.name == "block-note"
                    && self.current().token == Token::LineBreak
                {
                    self.hold_quote_body_through_parent_break();
                } else if self.current().token == Token::LineBreak {
                    self.step()?;
                }
                Ok(success)
            }
            Err(error) => Err(error),
        }
    }

    pub fn discard_body_elements(
        &mut self,
        block_rule: &BlockRule,
    ) -> Result<DiscardedBodyOutcome, ParseError> {
        let mutable_state = self.get_mutable_state();
        let mut fork = self.clone();
        let was_discarding = fork.discarding_hidden_body();
        fork.set_discarding_hidden_body(true);
        fork.push_hidden_body_boundary(
            block_rule_end_names(block_rule, fork.settings().layout.legacy()),
            block_rule.accepts_newlines_outside_body,
        );
        fork.push_body_end_boundary(
            block_rule_end_names(block_rule, fork.settings().layout.legacy()),
            block_rule.name == "block-footnote",
        );

        let discard_result =
            fork.consume_body_no_paragraphs(block_rule, false, false, |_| {});
        fork.pop_body_end_boundary();
        fork.pop_hidden_body_boundary();

        let outcome = if discard_result.is_err() {
            fork.skip_to_input_end()?;
            DiscardedBodyOutcome::FailedClosedThroughInputEnd
        } else {
            DiscardedBodyOutcome::Discarded
        };

        fork.set_discarding_hidden_body(was_discarding);
        self.update(&fork);
        self.reset_mutable_state(mutable_state);
        Ok(outcome)
    }

    pub(crate) fn discard_body_elements_with_literal_quote_context(
        &mut self,
        block_rule: &BlockRule,
        body_start: BlockBodyStart,
    ) -> Result<DiscardedBodyOutcome, ParseError> {
        self.discard_body_elements_with_context_policy(block_rule, body_start, true)
    }

    fn discard_body_elements_with_context_policy(
        &mut self,
        block_rule: &BlockRule,
        body_start: BlockBodyStart,
        literal_residual_quotes: bool,
    ) -> Result<DiscardedBodyOutcome, ParseError> {
        let Some(required_depth) = self.native_blockquote_depth() else {
            return self.discard_body_elements(block_rule);
        };
        if body_start != BlockBodyStart::NextPhysicalLine {
            return self.discard_body_elements(block_rule);
        }
        if !self.has_native_blockquote_body_end(block_rule, required_depth) {
            return Err(self.make_err(ParseErrorKind::RuleFailed));
        }

        let previous_cursor = self.quote_body_cursor();
        if literal_residual_quotes {
            self.install_quote_body_cursor_with_literal_residuals(required_depth);
        } else {
            self.install_quote_body_cursor(required_depth);
        }
        let result = self.discard_body_elements(block_rule);
        self.set_quote_body_cursor(previous_cursor);

        if result.is_ok() && self.current().token == Token::LineBreak {
            self.step()?;
        }
        result
    }

    fn get_body_elements_paragraphs(
        &mut self,
        block_rule: &BlockRule,
        restrict_quote_close: bool,
        allow_inline_quote_close: bool,
    ) -> ParseResult<'r, 't, Vec<Element<'t>>> {
        let mut first = true;
        let wikidot_tab = self.settings().layout.legacy()
            && !self.discarding_hidden_body()
            && block_rule.name == "block-tab";
        let mut nested_wikidot_tabs = 0_usize;
        let mut last_nested_wikidot_tab = None;
        let mut inner_tabview_text_seen = false;
        let rule = self.rule();

        let is_end = move |parser: &mut Parser<'r, 't>| {
            if wikidot_tab
                && !inner_tabview_text_seen
                && let Some(opener) = parser.wikidot_nested_tab_opener()
                && last_nested_wikidot_tab != Some(opener)
            {
                last_nested_wikidot_tab = Some(opener);
                nested_wikidot_tabs += 1;
            }
            if wikidot_tab && !inner_tabview_text_seen {
                inner_tabview_text_seen = parser.wikidot_tabview_opener_in_line_text();
            }
            let before_end = parser.clone();
            let first_iteration = first;
            let result = parser.verify_end_block(
                first,
                block_rule,
                restrict_quote_close,
                allow_inline_quote_close,
            );
            first = false;

            if result.is_some()
                && nested_wikidot_tabs > 0
                && parser.wikidot_later_tab_close_exists()
            {
                parser.update(&before_end);
                if matches!(
                    before_end.current().token,
                    Token::LineBreak | Token::ParagraphBreak
                ) {
                    return Ok(false);
                }
                nested_wikidot_tabs -= 1;
                return Ok(false);
            }

            if result.is_none()
                && parser.discarding_hidden_body()
                && parser.at_hidden_body_ancestor_boundary()
            {
                return Err(parser.make_err(ParseErrorKind::BlockExpectedEnd));
            }

            if result.is_none()
                && block_rule.name != "block-bibliography"
                && !parser.discarding_hidden_body()
                && parser.wikidot_at_joined_bibliography_closer()
            {
                return Ok(true);
            }

            if result.is_none()
                && block_rule.name == "block-div"
                && parser.settings().layout.legacy()
                && !parser.discarding_hidden_body()
                && parser.current().token == Token::LeftBlockEnd
            {
                let mut probe = parser.clone();
                if let Ok(name) = probe.get_end_block() {
                    let bare = name.strip_suffix('_').unwrap_or(name.as_ref());
                    if matches!(
                        bare.to_ascii_lowercase().as_str(),
                        "tab" | "tabview" | "tabs"
                    ) && parser.wikidot_ancestor_boundary_accepts(bare)
                    {
                        parser.set_wikidot_div_yielded_to_tab();
                        return Ok(true);
                    }
                }
            }

            if result.is_none()
                && !parser.discarding_hidden_body()
                && parser.at_ancestor_body_end_boundary()
            {
                return Err(parser.make_err(ParseErrorKind::BlockExpectedEnd));
            }

            if result.is_none()
                && parser.terminate_at_wikidot_list_close(block_rule, first_iteration)
            {
                return Ok(true);
            }

            Ok(result.is_some())
        };
        gather_paragraphs(self, rule, Some(is_end))
    }

    fn wikidot_later_tab_close_exists(&self) -> bool {
        let mut scan = self.clone();
        loop {
            match scan.current().token {
                Token::InputEnd => return false,
                Token::LeftBlockEnd => {
                    let mut end = scan.clone();
                    if end.get_end_block().is_ok_and(|name| {
                        name.strip_suffix('_')
                            .unwrap_or(&name)
                            .eq_ignore_ascii_case("tab")
                    }) {
                        return true;
                    }
                }
                _ => {}
            }
            if scan.step().is_err() {
                return false;
            }
        }
    }

    fn wikidot_nested_tab_opener(&self) -> Option<usize> {
        let start = match self.current().token {
            Token::LeftBlock => self.current().span.start,
            Token::LineBreak | Token::ParagraphBreak => self.current().span.end,
            _ => return None,
        };
        let full_text = self.full_text().inner();
        let suffix = &full_text[start..];
        let leading_space = suffix.len() - suffix.trim_start_matches([' ', '\t']).len();
        let opener = start + leading_space;
        let source = &full_text[opener..];
        let physical_line = source.split_once('\n').map_or(source, |(line, _)| line);
        let end = physical_line.find("]]")?;
        let head = source[..end].strip_prefix("[[")?;
        let mut parts = head.split_whitespace();
        parts
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("tab"))
            .then_some(opener)
    }

    fn wikidot_tabview_opener_in_line_text(&self) -> bool {
        let full_text = self.full_text().inner();
        let pos = self.current().span.start.min(full_text.len());
        let line_start = full_text[..pos].rfind('\n').map_or(0, |index| index + 1);
        let line_end = full_text[pos..]
            .find('\n')
            .map_or(full_text.len(), |index| pos + index);
        let line = &full_text[line_start..line_end];
        let head_offset = line.len() - line.trim_start_matches([' ', '\t']).len();
        let mut search = 0_usize;
        while let Some(found) = line[search..].find("[[") {
            let open = search + found;
            let rest = &line[open + 2..];
            let name_len = rest
                .find(|c: char| c.is_whitespace() || c == ']')
                .unwrap_or(rest.len());
            if rest[..name_len].eq_ignore_ascii_case("tabview") {
                return open != head_offset;
            }
            search = open + 2;
        }
        false
    }

    fn get_body_elements_no_paragraphs(
        &mut self,
        block_rule: &BlockRule,
        restrict_quote_close: bool,
        allow_inline_quote_close: bool,
    ) -> ParseResult<'r, 't, Vec<Element<'t>>> {
        let mut all_elements = Vec::new();
        let mut all_errors = Vec::new();
        let mut paragraph_safe = true;

        let process = |consumed: crate::parsing::ParseSuccess<'r, 't, Elements<'t>>| {
            let elements = consumed.chain(&mut all_errors, &mut paragraph_safe);
            all_elements.extend(elements);
        };
        self.consume_body_no_paragraphs(
            block_rule,
            restrict_quote_close,
            allow_inline_quote_close,
            process,
        )?;

        ok!(paragraph_safe; all_elements, all_errors)
    }

    pub(crate) fn get_body_element_segments(
        &mut self,
        block_rule: &BlockRule,
    ) -> ParseResult<'r, 't, Vec<Vec<Element<'t>>>> {
        let track_boundary = self.discarding_hidden_body();
        if track_boundary {
            self.push_hidden_body_boundary(
                block_rule_end_names(block_rule, self.settings().layout.legacy()),
                block_rule.accepts_newlines_outside_body,
            );
        }

        let body_names_len = self.push_inline_container_body(block_rule.name);

        self.push_body_end_boundary(
            block_rule_end_names(block_rule, self.settings().layout.legacy()),
            block_rule.name == "block-footnote",
        );

        let result = self.get_body_element_segments_internal(block_rule);
        self.restore_inline_container_body(body_names_len);

        self.pop_body_end_boundary();

        if track_boundary {
            self.pop_hidden_body_boundary();
        }

        result
    }

    fn literal_block_head_in_inline_body(
        &mut self,
        block_rule: &BlockRule,
    ) -> Result<Option<Element<'t>>, ParseError> {
        if self.settings().layout.legacy() {
            return Ok(None);
        }
        let Some(name) = self.block_head_name_here() else {
            return Ok(None);
        };
        let literal = match block_rule.name {
            "block-hidden" | "block-invisible" => false,
            _ => {
                matches!(name.as_str(), "div" | "blockquote" | "code")
                    || (matches!(block_rule.name, "block-span" | "block-size")
                        && block_rule
                            .accepts_names
                            .iter()
                            .any(|accepted| accepted.eq_ignore_ascii_case(&name))
                        && !self.inline_body_same_name_may_form(block_rule, &name))
            }
        };
        if !literal {
            return Ok(None);
        }
        self.take_block_head_text()
    }

    fn inline_body_same_name_may_form(&self, block_rule: &BlockRule, name: &str) -> bool {
        let needed = self.inline_container_body_depth(block_rule.name) + 1;
        let remaining = self.remaining();
        let mut closers = 0_usize;
        let mut index = 0;
        while let Some(token) = remaining.get(index) {
            if token.token == Token::LeftBlockEnd {
                let mut probe = index + 1;
                if remaining
                    .get(probe)
                    .is_some_and(|token| token.token == Token::Whitespace)
                {
                    probe += 1;
                }
                if let Some(name_token) = remaining.get(probe) {
                    let candidate = name_token
                        .slice
                        .strip_suffix('_')
                        .unwrap_or(name_token.slice);
                    if candidate.eq_ignore_ascii_case(name) {
                        closers += 1;
                    }
                }
            }
            index += 1;
        }
        closers >= needed
    }

    fn get_body_element_segments_internal(
        &mut self,
        block_rule: &BlockRule,
    ) -> ParseResult<'r, 't, Vec<Vec<Element<'t>>>> {
        debug_assert!(
            !self.settings().layout.legacy(),
            "paragraph-split body segments are a wikijump-layout mechanism",
        );

        let mut segments: Vec<Vec<Element<'t>>> = vec![Vec::new()];
        let mut all_errors = Vec::new();
        let mut paragraph_safe = true;
        let mut first = true;

        loop {
            if self.prepare_quote_body_line()? == QuoteBodyLineStatus::Boundary {
                return Err(self.make_err(ParseErrorKind::EndOfInput));
            }

            let result = self.verify_end_block(first, block_rule, false, false);
            if result.is_some() {
                break;
            }

            if self.terminate_at_wikidot_list_close(block_rule, first) {
                break;
            }

            if self.discarding_hidden_body() && self.at_hidden_body_ancestor_boundary() {
                return Err(self.make_err(ParseErrorKind::BlockExpectedEnd));
            }

            if block_rule.name != "block-bibliography"
                && !self.discarding_hidden_body()
                && self.wikidot_at_joined_bibliography_closer()
            {
                return Ok(crate::parsing::ParseSuccess::new(
                    segments,
                    all_errors,
                    paragraph_safe,
                ));
            }

            if !self.discarding_hidden_body() && self.at_ancestor_body_end_boundary() {
                return Err(self.make_err(ParseErrorKind::BlockExpectedEnd));
            }

            if self.current().token == Token::ParagraphBreak {
                self.step()?;
                first = false;
                segments.push(Vec::new());
                continue;
            }

            first = false;
            if let Some(element) = self.literal_block_head_in_inline_body(block_rule)? {
                segments
                    .last_mut()
                    .expect("segments always has a current segment")
                    .push(element);
                continue;
            }

            self.set_wikidot_line_follows_block(false);
            match consume(self) {
                Ok(consumed) => {
                    let crate::parsing::ParseSuccess {
                        item,
                        errors,
                        paragraph_safe: safe,
                        paragraph_split,
                        ..
                    } = consumed;
                    all_errors.extend(errors);
                    paragraph_safe &= safe;
                    let elements = match item {
                        Elements::Multiple(elements) => elements,
                        Elements::Single(element) => vec![element],
                        Elements::None => Vec::new(),
                    };
                    match paragraph_split {
                        Some(counts) => {
                            distribute_paragraph_split(
                                &mut segments,
                                None,
                                None,
                                elements,
                                &counts,
                            );
                        }
                        None => {
                            segments
                                .last_mut()
                                .expect("segments always has a current segment")
                                .extend(elements);
                        }
                    }
                }
                Err(_error)
                    if self.discarding_hidden_body()
                        && !self.settings().layout.legacy()
                        && self.at_hidden_body_boundary() => {}
                Err(error) => return Err(error),
            }
        }

        Ok(crate::parsing::ParseSuccess::new(
            segments,
            all_errors,
            paragraph_safe,
        ))
    }

    pub(crate) fn get_body_element_segments_iftags(
        &mut self,
        block_rule: &BlockRule,
    ) -> ParseResult<
        'r,
        't,
        (
            (Vec<Vec<Element<'t>>>, Vec<bool>, Vec<ParagraphSplitKind>),
            bool,
        ),
    > {
        let mut segments: Vec<Vec<Element<'t>>> = vec![Vec::new()];
        let mut segment_safe: Vec<bool> = vec![true];
        let mut segment_kinds: Vec<ParagraphSplitKind> =
            vec![ParagraphSplitKind::Paragraph];
        let mut all_errors = Vec::new();
        let mut paragraph_safe = true;
        let mut first = true;
        let mut trailing = false;
        let gate_items_allowed = self.gate_items_allowed();
        self.set_gate_items_allowed(false);
        let items_context = self.settings().layout.legacy()
            && !self.discarding_hidden_body()
            && (self.in_wikidot_single_line_content()
                || self.in_wikidot_definition_list_line())
            && gate_items_allowed;
        let mut items_open = items_context;
        let mut after_item = false;
        let mut escape_split: Option<usize> = None;
        let mut line_began = self.start_of_line();

        loop {
            if self.prepare_quote_body_line()? == QuoteBodyLineStatus::Boundary {
                return Err(self.make_err(ParseErrorKind::EndOfInput));
            }

            let before_end_token = self.current().token;
            if self
                .verify_end_block(first, block_rule, false, false)
                .is_some()
            {
                trailing = before_end_token == Token::LineBreak;
                break;
            }

            if self.terminate_at_wikidot_list_close(block_rule, first) {
                break;
            }

            if self.discarding_hidden_body() && self.at_hidden_body_ancestor_boundary() {
                return Err(self.make_err(ParseErrorKind::BlockExpectedEnd));
            }

            if !self.discarding_hidden_body() && self.at_body_end_boundary() {
                if let Some(last) = segments.last_mut() {
                    while matches!(last.last(), Some(Element::LineBreak)) {
                        last.pop();
                    }
                }
                break;
            }

            if let Some(designated) = self.wikidot_iftags_body_end()
                && self.current().span.start > designated
            {
                break;
            }

            if self.current().token == Token::InputEnd
                && self.settings().layout.legacy()
                && !self.discarding_hidden_body()
            {
                break;
            }

            if self.current().token == Token::ParagraphBreak {
                self.step()?;
                first = false;
                segments.push(Vec::new());
                segment_safe.push(true);
                segment_kinds.push(ParagraphSplitKind::Paragraph);
                after_item = false;
                items_open = false;
                line_began = true;
                continue;
            }

            first = false;
            let marker_here = if line_began {
                crate::parsing::rule::impls::list_item_marker_here(self)
            } else {
                None
            };
            if let Some(list_type) = marker_here {
                if items_open {
                    segments.push(Vec::new());
                    segment_safe.push(true);
                    segment_kinds.push(ParagraphSplitKind::Item(list_type));
                    let line_consumed =
                        crate::parsing::rule::impls::collect_gate_body_list_line(self)?;
                    let crate::parsing::ParseSuccess {
                        item: (line_elements, line_ends_run, line_escape),
                        errors: line_errors,
                        paragraph_safe: line_safe,
                        ..
                    } = line_consumed;
                    all_errors.extend(line_errors);
                    paragraph_safe &= line_safe;
                    segments
                        .last_mut()
                        .expect("segments always has a current segment")
                        .extend(line_elements);
                    match line_escape {
                        Some(escape) => {
                            let (flat, entries, escape_safe) = escape.into_layout();
                            paragraph_safe &= escape_safe;
                            segments.push(Vec::new());
                            segment_safe.push(true);
                            segment_kinds.push(ParagraphSplitKind::Paragraph);
                            distribute_paragraph_split(
                                &mut segments,
                                Some(&mut segment_safe),
                                Some(&mut segment_kinds),
                                flat,
                                &entries,
                            );
                            items_open = false;
                            after_item = false;
                        }
                        None => {
                            after_item = true;
                            if line_ends_run {
                                items_open = false;
                            }
                        }
                    }
                    continue;
                }
                if after_item {
                    segments.push(Vec::new());
                    segment_safe.push(true);
                    segment_kinds.push(ParagraphSplitKind::Paragraph);
                    after_item = false;
                } else if segments.len() == 1 {
                    match escape_split {
                        Some(position) if position < segments[0].len() => {
                            let tail = segments[0].split_off(position);
                            segments.push(tail);
                            segment_safe.push(true);
                            segment_kinds.push(ParagraphSplitKind::Paragraph);
                            segments.push(Vec::new());
                            segment_safe.push(true);
                            segment_kinds.push(ParagraphSplitKind::Paragraph);
                        }
                        Some(_) => {
                            segments.push(Vec::new());
                            segment_safe.push(true);
                            segment_kinds.push(ParagraphSplitKind::Paragraph);
                        }
                        None => {}
                    }
                } else if !segments
                    .last()
                    .expect("segments always has a current segment")
                    .is_empty()
                {
                    segments.push(Vec::new());
                    segment_safe.push(true);
                    segment_kinds.push(ParagraphSplitKind::Paragraph);
                }
            } else if line_began {
                if items_open {
                    items_open = false;
                    if segments.len() == 1 && !after_item {
                        escape_split = Some(segments[0].len());
                    }
                }
                if after_item {
                    segments.push(Vec::new());
                    segment_safe.push(true);
                    segment_kinds.push(ParagraphSplitKind::Paragraph);
                    after_item = false;
                }
            }
            let before = self.remaining();
            let before_token = self.current().token;
            self.set_wikidot_line_follows_block(false);
            match consume(self) {
                Ok(consumed) => {
                    self.take_wikidot_mixed_paragraph_ownership();
                    let crate::parsing::ParseSuccess {
                        item,
                        errors,
                        paragraph_safe: safe,
                        paragraph_split,
                        ..
                    } = consumed;
                    all_errors.extend(errors);
                    paragraph_safe &= safe;
                    let elements = match item {
                        Elements::Multiple(elements) => elements,
                        Elements::Single(element) => vec![element],
                        Elements::None => Vec::new(),
                    };
                    let steps = before.len() - self.remaining().len();
                    let absorbed_trailing_break = paragraph_split.is_none()
                        && match steps {
                            0 => false,
                            1 => before_token == Token::ParagraphBreak,
                            _ => before[steps - 2].token == Token::ParagraphBreak,
                        };
                    match paragraph_split {
                        Some(counts) => {
                            distribute_paragraph_split(
                                &mut segments,
                                Some(&mut segment_safe),
                                Some(&mut segment_kinds),
                                elements,
                                &counts,
                            );
                        }
                        None => {
                            if let Some(current) = segment_safe.last_mut() {
                                *current &= safe;
                            }
                            segments
                                .last_mut()
                                .expect("segments always has a current segment")
                                .extend(elements);
                            if segments.len() > 1
                                && self.take_wikidot_div_literal_recovered()
                                && !segment_safe.last().copied().unwrap_or(true)
                            {
                                let segment = segments
                                    .last_mut()
                                    .expect("segments always has a current segment");
                                if segment.len() == 1
                                    && let Some(Element::Text(text)) = segment.first_mut()
                                {
                                    if let Some(stripped) = text.strip_prefix('\n') {
                                        *text = Cow::Owned(stripped.to_owned());
                                    }
                                    if let Some(current) = segment_safe.last_mut() {
                                        *current = true;
                                    }
                                }
                            }
                        }
                    }
                    if absorbed_trailing_break {
                        segments.push(Vec::new());
                        segment_safe.push(true);
                        segment_kinds.push(ParagraphSplitKind::Paragraph);
                    }
                    if steps >= 1 {
                        let last_token = if steps == 1 {
                            before_token
                        } else {
                            before[steps - 1].token
                        };
                        line_began = last_token == Token::LineBreak
                            || last_token == Token::ParagraphBreak;
                    }
                }
                Err(error) => return Err(error),
            }
        }

        if segments.len() > 1 {
            let last_index = segments.len() - 1;
            for (index, segment) in segments.iter_mut().enumerate() {
                if index > 0 {
                    trim_segment_leading_blanks(segment);
                }
                if index < last_index {
                    trim_segment_trailing_blanks(segment);
                }
            }
        }

        Ok(crate::parsing::ParseSuccess::new(
            ((segments, segment_safe, segment_kinds), trailing),
            all_errors,
            paragraph_safe,
        ))
    }

    pub(crate) fn get_body_element_segments_iftags_in_quote(
        &mut self,
        block_rule: &BlockRule,
        required_depth: usize,
    ) -> ParseResult<'r, 't, (Vec<QuoteGateSegment<'t>>, bool)> {
        let original_cursor = self.quote_body_cursor();
        let mut segments: Vec<QuoteGateSegment<'t>> = vec![QuoteGateSegment {
            elements: Vec::new(),
            paragraph_safe: true,
            kind: QuoteGateSegmentKind::InRun,
            boundary: QuoteGateBoundary::Start,
            split_kind: ParagraphSplitKind::Paragraph,
        }];
        let mut all_errors = Vec::new();
        let mut paragraph_safe = true;
        let mut first = true;
        let trailing = false;
        let mut escaped = false;
        let mut consumed_any = false;
        let mut resolve_kind = false;
        let mut resolve_from_blank = false;
        let mut row_depth: Option<usize> = None;

        loop {
            if self.current().token == Token::ParagraphBreak {
                let current_kind = segments
                    .last()
                    .expect("segments always has a current segment")
                    .kind;
                let same_run = consumed_any
                    && matches!(
                        current_kind,
                        QuoteGateSegmentKind::InRun
                            | QuoteGateSegmentKind::QuoteRow(_)
                            | QuoteGateSegmentKind::EscapedQuote { .. }
                    )
                    && self.quote_body_close_allowed_here();
                self.step()?;
                first = false;
                if same_run {
                    segments.push(QuoteGateSegment {
                        elements: Vec::new(),
                        paragraph_safe: true,
                        kind: current_kind,
                        boundary: QuoteGateBoundary::SameRun,
                        split_kind: ParagraphSplitKind::Paragraph,
                    });
                } else {
                    if row_depth.is_some() {
                        self.set_quote_body_cursor(original_cursor);
                        row_depth = None;
                    }
                    escaped = true;
                    resolve_kind = true;
                    resolve_from_blank = true;
                    segments.push(QuoteGateSegment {
                        elements: Vec::new(),
                        paragraph_safe: true,
                        kind: QuoteGateSegmentKind::EscapedPlain,
                        boundary: QuoteGateBoundary::NewRun,
                        split_kind: ParagraphSplitKind::Paragraph,
                    });
                }
                continue;
            }

            let position_before = self.current().span.start;
            let status = self.prepare_quote_body_line()?;
            let moved = self.current().span.start != position_before;
            let scan_depth =
                quote_line_depth(self.full_text().inner(), self.current().span.start);
            let line_kind = match status {
                QuoteBodyLineStatus::Boundary => {
                    if scan_depth == 0 {
                        Some(QuoteGateSegmentKind::EscapedPlain)
                    } else if resolve_from_blank {
                        Some(QuoteGateSegmentKind::EscapedQuote {
                            depth: scan_depth,
                            stripped: false,
                        })
                    } else if let Some(depth) = row_depth {
                        if scan_depth < depth {
                            Some(QuoteGateSegmentKind::QuoteRow(scan_depth))
                        } else {
                            Some(QuoteGateSegmentKind::EscapedQuote {
                                depth: scan_depth,
                                stripped: false,
                            })
                        }
                    } else if scan_depth < required_depth {
                        Some(QuoteGateSegmentKind::QuoteRow(scan_depth))
                    } else {
                        Some(QuoteGateSegmentKind::EscapedQuote {
                            depth: scan_depth,
                            stripped: false,
                        })
                    }
                }
                QuoteBodyLineStatus::Prepared if moved => {
                    if let Some(depth) = row_depth {
                        Some(QuoteGateSegmentKind::QuoteRow(depth))
                    } else if escaped {
                        Some(QuoteGateSegmentKind::EscapedQuote {
                            depth: scan_depth,
                            stripped: true,
                        })
                    } else {
                        Some(QuoteGateSegmentKind::InRun)
                    }
                }
                _ => None,
            };

            let mut restart_line = false;
            if resolve_kind {
                if let Some(kind) = line_kind {
                    segments
                        .last_mut()
                        .expect("segments always has a current segment")
                        .kind = kind;
                    match kind {
                        QuoteGateSegmentKind::QuoteRow(depth) => {
                            self.install_quote_body_cursor_with_literal_residuals(depth);
                            row_depth = Some(depth);
                            restart_line = true;
                        }
                        _ => {
                            row_depth = None;
                        }
                    }
                    if !matches!(
                        kind,
                        QuoteGateSegmentKind::InRun | QuoteGateSegmentKind::QuoteRow(_)
                    ) {
                        escaped = true;
                    }
                    resolve_kind = false;
                    resolve_from_blank = false;
                }
            } else if let Some(kind) = line_kind {
                let current_kind = segments
                    .last()
                    .expect("segments always has a current segment")
                    .kind;
                if kind != current_kind {
                    let segment = segments
                        .last_mut()
                        .expect("segments always has a current segment");
                    if matches!(segment.elements.last(), Some(Element::LineBreak)) {
                        segment.elements.pop();
                    }
                    if row_depth.is_some() {
                        self.set_quote_body_cursor(original_cursor);
                        row_depth = None;
                    }
                    if let QuoteGateSegmentKind::QuoteRow(depth) = kind {
                        self.install_quote_body_cursor_with_literal_residuals(depth);
                        row_depth = Some(depth);
                        restart_line = true;
                    }
                    if !matches!(
                        kind,
                        QuoteGateSegmentKind::InRun | QuoteGateSegmentKind::QuoteRow(_)
                    ) {
                        escaped = true;
                    }
                    segments.push(QuoteGateSegment {
                        elements: Vec::new(),
                        paragraph_safe: true,
                        kind,
                        boundary: QuoteGateBoundary::NewRun,
                        split_kind: ParagraphSplitKind::Paragraph,
                    });
                }
            }
            if restart_line {
                continue;
            }

            if self.current().token == Token::ParagraphBreak {
                continue;
            }

            if self.current().token == Token::LineBreak
                && matches!(
                    segments
                        .last()
                        .expect("segments always has a current segment")
                        .kind,
                    QuoteGateSegmentKind::InRun
                        | QuoteGateSegmentKind::QuoteRow(_)
                        | QuoteGateSegmentKind::EscapedQuote { .. }
                )
                && self.quote_body_close_allowed_here()
            {
                let kind = segments
                    .last()
                    .expect("segments always has a current segment")
                    .kind;
                self.step()?;
                first = false;
                segments.push(QuoteGateSegment {
                    elements: Vec::new(),
                    paragraph_safe: true,
                    kind,
                    boundary: QuoteGateBoundary::SameRun,
                    split_kind: ParagraphSplitKind::Paragraph,
                });
                continue;
            }

            if self
                .verify_end_block(first, block_rule, false, false)
                .is_some()
            {
                break;
            }

            if self.terminate_at_wikidot_list_close(block_rule, first) {
                break;
            }

            if self.discarding_hidden_body() && self.at_hidden_body_ancestor_boundary() {
                return Err(self.make_err(ParseErrorKind::BlockExpectedEnd));
            }

            if let Some(designated) = self.wikidot_iftags_body_end()
                && self.current().span.start > designated
            {
                break;
            }

            if self.current().token == Token::InputEnd
                && self.settings().layout.legacy()
                && !self.discarding_hidden_body()
            {
                break;
            }

            first = false;
            let before = self.remaining();
            let before_token = self.current().token;
            let clear_cursor = matches!(
                segments
                    .last()
                    .expect("segments always has a current segment")
                    .kind,
                QuoteGateSegmentKind::EscapedPlain
                    | QuoteGateSegmentKind::EscapedQuote {
                        stripped: false,
                        ..
                    }
            );
            let saved_cursor = self.quote_body_cursor();
            if clear_cursor {
                self.set_quote_body_cursor(None);
            }
            self.set_wikidot_line_follows_block(false);
            let consume_result = consume(self);
            if clear_cursor {
                self.set_quote_body_cursor(saved_cursor);
            }
            match consume_result {
                Ok(consumed) => {
                    self.take_wikidot_mixed_paragraph_ownership();
                    let crate::parsing::ParseSuccess {
                        item,
                        errors,
                        paragraph_safe: safe,
                        paragraph_split,
                        ..
                    } = consumed;
                    all_errors.extend(errors);
                    paragraph_safe &= safe;
                    let elements = match item {
                        Elements::Multiple(elements) => elements,
                        Elements::Single(element) => vec![element],
                        Elements::None => Vec::new(),
                    };
                    let steps = before.len() - self.remaining().len();
                    if steps > 0 || !elements.is_empty() {
                        consumed_any = true;
                    }
                    let absorbed_trailing_break = paragraph_split.is_none()
                        && match steps {
                            0 => false,
                            1 => before_token == Token::ParagraphBreak,
                            _ => before[steps - 2].token == Token::ParagraphBreak,
                        };
                    match paragraph_split {
                        Some(counts) => {
                            distribute_quote_gate_split(&mut segments, elements, &counts);
                        }
                        None => {
                            let current = segments
                                .last_mut()
                                .expect("segments always has a current segment");
                            current.paragraph_safe &= safe;
                            current.elements.extend(elements);
                            if segments.len() > 1
                                && self.take_wikidot_div_literal_recovered()
                                && !segments
                                    .last()
                                    .expect("segments always has a current segment")
                                    .paragraph_safe
                            {
                                let segment = segments
                                    .last_mut()
                                    .expect("segments always has a current segment");
                                if segment.elements.len() == 1
                                    && let Some(Element::Text(text)) =
                                        segment.elements.first_mut()
                                {
                                    if let Some(stripped) = text.strip_prefix('\n') {
                                        *text = Cow::Owned(stripped.to_owned());
                                    }
                                    segment.paragraph_safe = true;
                                }
                            }
                        }
                    }
                    if absorbed_trailing_break {
                        if row_depth.is_some() {
                            self.set_quote_body_cursor(original_cursor);
                            row_depth = None;
                        }
                        resolve_kind = true;
                        segments.push(QuoteGateSegment {
                            elements: Vec::new(),
                            paragraph_safe: true,
                            kind: QuoteGateSegmentKind::EscapedPlain,
                            boundary: QuoteGateBoundary::NewRun,
                            split_kind: ParagraphSplitKind::Paragraph,
                        });
                    }
                }
                Err(error) => return Err(error),
            }
        }

        if segments.len() > 1 {
            let last_index = segments.len() - 1;
            for (index, segment) in segments.iter_mut().enumerate() {
                if index > 0 {
                    trim_segment_leading_blanks(&mut segment.elements);
                }
                if index < last_index {
                    trim_segment_trailing_blanks(&mut segment.elements);
                }
            }
        }

        Ok(crate::parsing::ParseSuccess::new(
            (segments, trailing),
            all_errors,
            paragraph_safe,
        ))
    }

    pub(crate) fn wrap_body_segments(
        &mut self,
        segments: Vec<Vec<Element<'t>>>,
        ctype: ContainerType,
        attributes: AttributeMap<'t>,
        paragraph_safe: bool,
    ) -> (Elements<'t>, Option<Vec<ParagraphSplitEntry>>) {
        if segments.len() == 1 {
            let elements = segments.into_iter().next().expect("single segment present");
            let container = Container::new(ctype, elements, attributes);
            return (Elements::Single(Element::Container(container)), None);
        }

        let last_index = segments.len() - 1;
        let mut counts = Vec::with_capacity(segments.len());
        let mut containers = Vec::new();
        for (index, mut segment) in segments.into_iter().enumerate() {
            if index > 0 {
                trim_segment_leading_blanks(&mut segment);
            }
            if index < last_index {
                trim_segment_trailing_blanks(&mut segment);
            }

            if segment.is_empty() {
                counts.push((0, paragraph_safe, ParagraphSplitKind::Paragraph));
            } else {
                counts.push((1, paragraph_safe, ParagraphSplitKind::Paragraph));
                containers.push(Element::Container(Container::new(
                    ctype,
                    segment,
                    attributes.clone(),
                )));
            }
        }

        let elements = match containers.len() {
            0 => Elements::None,
            1 => Elements::Single(
                containers
                    .into_iter()
                    .next()
                    .expect("single container present"),
            ),
            _ => Elements::Multiple(containers),
        };
        (elements, Some(counts))
    }

    pub fn get_body_elements_inline_container(
        &mut self,
        block_rule: &BlockRule,
        ctype: ContainerType,
        attributes: AttributeMap<'t>,
    ) -> ParseResult<'r, 't, Elements<'t>> {
        if self.settings().layout.legacy() {
            let body = self.get_body_elements(block_rule, false)?;
            let (elements, errors, paragraph_safe) = body.into();
            let element = Element::Container(Container::new(ctype, elements, attributes));
            return ok!(paragraph_safe; element, errors);
        }

        let body = self.get_body_element_segments(block_rule)?;
        let (segments, errors, paragraph_safe) = body.into();
        let (elements, paragraph_split) =
            self.wrap_body_segments(segments, ctype, attributes, paragraph_safe);
        let mut success =
            crate::parsing::ParseSuccess::new(elements, errors, paragraph_safe);
        success.paragraph_split = paragraph_split;
        Ok(success)
    }

    fn tab_structure_follows_blank(&self) -> bool {
        if self.current().token != Token::ParagraphBreak {
            return false;
        }
        let required = self.native_blockquote_depth().unwrap_or(0);
        if required == 0 {
            return false;
        }
        let mut probe = self.clone();
        if probe.step().is_err() {
            return false;
        }
        while matches!(probe.current().token, Token::LineBreak) {
            if probe.step().is_err() {
                return false;
            }
        }
        let mut depth = 0usize;
        while probe.current().token == Token::Quote {
            depth += probe.current().slice.len();
            if probe.step().is_err() {
                return false;
            }
            if probe.get_optional_space().is_err() {
                return false;
            }
        }
        if depth != required {
            return false;
        }
        if matches!(
            probe.current().token,
            Token::LeftBlock | Token::LeftBlockStar
        ) {
            return probe
                .get_block_name(false)
                .is_ok_and(|(name, _, _)| name.eq_ignore_ascii_case("tab"));
        }
        if probe.current().token == Token::LeftBlockEnd {
            return probe.get_end_block().is_ok_and(|name| {
                let name = name.as_ref();
                name.eq_ignore_ascii_case("tab")
                    || name.eq_ignore_ascii_case("tabview")
                    || name.eq_ignore_ascii_case("tabs")
            });
        }
        false
    }

    fn consume_body_no_paragraphs<F>(
        &mut self,
        block_rule: &BlockRule,
        restrict_quote_close: bool,
        allow_inline_quote_close: bool,
        mut process: F,
    ) -> Result<(), ParseError>
    where
        F: FnMut(crate::parsing::ParseSuccess<'r, 't, Elements<'t>>),
    {
        let mut first = true;

        loop {
            if block_rule.name.starts_with("block-list-")
                && self.quotes_then_block_end(block_rule)
            {
                let mut probe = self.clone();
                if probe.take_quoted_closer_marks().is_ok()
                    && probe
                        .verify_end_block(first, block_rule, false, false)
                        .is_some()
                {
                    self.update(&probe);
                    return Ok(());
                }
            }
            if self.prepare_quote_body_line()? == QuoteBodyLineStatus::Boundary {
                if self.quote_body_literal_residuals() == Some(true)
                    && self.current().token == Token::LineBreak
                    && block_rule.name.starts_with("block-list-")
                {
                    self.step()?;
                    continue;
                }
                if self.wikidot_tab_quote_ejection() && !self.discarding_hidden_body() {
                    if block_rule.name == "block-tabview"
                        && self.wikidot_tabview_ejected()
                        && self.tab_structure_follows_blank()
                    {
                        self.step()?;
                        continue;
                    }
                    self.set_wikidot_body_soft_ended(true);
                    return Ok(());
                }
                return Err(self.make_err(ParseErrorKind::EndOfInput));
            }

            let result = self.verify_end_block(
                first,
                block_rule,
                restrict_quote_close,
                allow_inline_quote_close,
            );
            if result.is_some() {
                return Ok(());
            }

            if self.wikidot_li_cross_skip_redundant_here(block_rule.name) {
                first = false;
                continue;
            }

            if self.terminate_at_wikidot_list_close(block_rule, first) {
                return Ok(());
            }

            if block_rule.name == "block-bibliography"
                && self.wikidot_bibliography_split_at_enclosing_closer()
            {
                return Ok(());
            }

            if self.wikidot_li_cross_foreign_stop_here(block_rule) {
                return Ok(());
            }

            if self.discarding_hidden_body() && self.at_hidden_body_ancestor_boundary() {
                return Err(self.make_err(ParseErrorKind::BlockExpectedEnd));
            }

            if block_rule.name == "block-iftags"
                && let Some(designated) = self.wikidot_iftags_body_end()
                && self.current().span.start > designated
            {
                return Ok(());
            }

            if block_rule.name != "block-bibliography"
                && !self.discarding_hidden_body()
                && self.wikidot_at_joined_bibliography_closer()
            {
                return Ok(());
            }

            if !self.discarding_hidden_body() && self.at_ancestor_body_end_boundary() {
                return Err(self.make_err(ParseErrorKind::BlockExpectedEnd));
            }

            let wikidot_input_end = self.current().token == Token::InputEnd
                && self.settings().layout.legacy()
                && !self.discarding_hidden_body();
            if wikidot_input_end {
                return Ok(());
            }

            first = false;
            self.set_wikidot_line_follows_block(false);
            match consume(self) {
                Ok(consumed) => {
                    self.take_wikidot_mixed_paragraph_ownership();
                    process(consumed);
                }
                Err(_error)
                    if self.discarding_hidden_body()
                        && self.wikidot_iftags_body_end().is_some_and(|designated| {
                            self.at_designated_iftags_end(designated)
                                && offset_inside_comment(
                                    self.full_text().inner(),
                                    designated,
                                )
                        }) => {}
                Err(_error)
                    if self.discarding_hidden_body()
                        && self.at_hidden_body_boundary() => {}
                Err(error) => return Err(error),
            }
        }
    }

    pub(crate) fn at_designated_iftags_end(&self, designated: usize) -> bool {
        let mut probe = self.clone();
        if probe.current().token == Token::LineBreak && probe.step().is_err() {
            return false;
        }
        probe.current().token == Token::LeftBlockEnd
            && probe.current().span.start == designated
    }

    /// The end head that designates where the current `[[iftags]]` body ends,
    /// plus whether a nested `[[iftags]]` head was crossed before it.
    ///
    /// The body is scanned once, left to right, over the same token chain the
    /// pairing used to walk (raw and line-owned block spans are skipped whole,
    /// exactly as `protected_span_end` decides). The designated end head is the
    /// first `[[/iftags]]` that closes the body at nesting depth zero; nested
    /// heads raise the depth and their own end heads lower it. If the body
    /// never returns to depth zero inside the window, the last reachable end
    /// head designates the end, which is the gate-transparency case: an
    /// unclosed body keeps the closer it can still reach.
    ///
    /// That is exactly the first element of the ordered set of reachable end
    /// heads the previous pairing dynamic program materialized at every token.
    /// The diffusion it computed is one left-to-right balance walk; the DP's
    /// ordering put the depth-zero end head first and the last reachable end
    /// head first when no depth-zero one existed. Because the set was
    /// materialized per token (and paired again per nested head) pairing cost
    /// was cubic in the gate count; the walk below is linear.
    ///
    /// `wikidot_iftags_pairing_end_reference` keeps the DP verbatim. It is the
    /// fallback for the one shape the walk cannot reproduce (a protected span
    /// that crosses a gate head, see its doc comment) and the reference for
    /// the differential test that locks the equivalence.
    pub(crate) fn wikidot_iftags_pairing_end(&self) -> (Option<usize>, bool) {
        let source = self.full_text().inner();
        let mut window = IftagsPairingWindow::new(self);
        let mut depth = 0_usize;
        let mut last_end_head = None;
        let mut nested = false;
        let mut nested_at_last_end_head = false;
        let mut open_head_end = None;
        let mut index = 0_usize;

        loop {
            if !window.ensure(index) {
                break;
            }
            #[cfg(test)]
            count_iftags_pairing_scan_step();
            let token = window.token(index);
            if open_head_end.is_some_and(|head_end| index >= head_end) {
                open_head_end = None;
            }

            if lazy_iftags_end_head_end(&mut window, index).is_some() {
                if depth == 0 {
                    return (Some(token.span.start), nested);
                }
                depth -= 1;
                last_end_head = Some(token.span.start);
                nested_at_last_end_head = nested;
            } else if let Some(head_end) =
                lazy_named_block_head_end(&mut window, index, "iftags")
            {
                depth += 1;
                nested = true;
                open_head_end = Some(head_end);
            }

            let step_to =
                lazy_protected_span_end(source, &mut window, index).unwrap_or(index + 1);
            // A protected span that opens inside an `[[iftags]]` head and
            // closes past that head's `]]` makes the tokens reachable from
            // inside the head differ from the ones reachable from the body
            // start, so the balance walk no longer reproduces the reference
            // dynamic program. That corner needs the general program; it is a
            // bounded, documented fallback (see its doc comment).
            if open_head_end.is_some_and(|head_end| step_to > head_end) {
                return self.wikidot_iftags_pairing_end_reference();
            }
            index = step_to;
        }

        match last_end_head {
            Some(end_head) => (Some(end_head), nested_at_last_end_head),
            None => (None, false),
        }
    }

    /// The cubic reachable-end-head dynamic program.
    ///
    /// `wikidot_iftags_pairing_end` uses it as the exact fallback for the one
    /// shape its linear walk cannot reproduce: a protected span (`@@`/`@<`)
    /// that opens inside an `[[iftags ...]]` head and closes after that head's
    /// `]]`. There the components reachable from inside the head differ from
    /// the ones reachable from the body start, so the pairing reachable from
    /// the head is the concatenation of two different suffixes. The corner is
    /// bounded by the 8192-token window (at most one window scan and one
    /// reachable-end-head table per affected call), so a crafted page needs an
    /// `@@`/`@<` inside a gate head to reach it.
    ///
    /// The differential test `pairing_scan_matches_reference_dynamic_program`
    /// also uses it as the reference for every other shape.
    fn wikidot_iftags_pairing_end_reference(&self) -> (Option<usize>, bool) {
        const WINDOW: usize = IFTAGS_PAIRING_WINDOW;

        let mut tokens: Vec<&ExtractedToken<'t>> = Vec::new();
        let mut scan = self.clone();
        loop {
            let token = scan.current();
            let at_end = token.token == Token::InputEnd;
            tokens.push(token);
            if at_end || tokens.len() >= WINDOW {
                break;
            }
            if scan.step().is_err() {
                break;
            }
        }

        let source = self.full_text().inner();
        let n = tokens.len();
        let mut ends: Vec<Vec<usize>> = vec![Vec::new(); n + 1];
        let mut seen: Vec<usize> = vec![0; n + 1];
        for i in (0..n).rev() {
            let stamp = i + 1;
            let mut out: Vec<usize> = Vec::new();
            let mut push = |out: &mut Vec<usize>, end: usize| {
                if seen[end] != stamp {
                    seen[end] = stamp;
                    out.push(end);
                }
            };
            if iftags_end_head_end(&tokens, i).is_some() {
                push(&mut out, i);
            }
            if let Some(head_end) = iftags_block_head_end(&tokens, i) {
                for inner in ends[head_end].clone() {
                    let after = iftags_end_head_end(&tokens, inner)
                        .expect("ends entries are iftags end heads");
                    for end in ends[after].clone() {
                        push(&mut out, end);
                    }
                }
            }
            let step_to = protected_span_end(source, &tokens, i).unwrap_or(i + 1);
            for end in ends[step_to].clone() {
                push(&mut out, end);
            }
            ends[i] = out;
        }

        let Some(&closer) = ends[0].first() else {
            return (None, false);
        };
        let mut nested = false;
        let mut i = 0;
        while i < closer {
            if iftags_block_head_end(&tokens, i).is_some() {
                nested = true;
                break;
            }
            i = protected_span_end(source, &tokens, i).unwrap_or(i + 1);
        }
        (Some(tokens[closer].span.start), nested)
    }

    pub(crate) fn skip_raw_segment(&mut self) -> bool {
        if self.current().token != Token::Raw {
            return false;
        }
        let mut probe = self.clone();
        if probe.step().is_err() {
            return false;
        }
        loop {
            match probe.current().token {
                Token::Raw => {
                    if probe.step().is_err() {
                        return false;
                    }
                    self.update(&probe);
                    return true;
                }
                Token::InputEnd => return false,
                _ => {
                    if probe.step().is_err() {
                        return false;
                    }
                }
            }
        }
    }

    fn skip_wikidot_bibliography_opaque_segment(&mut self) -> bool {
        let start = self.current().span.start;
        let end = match self.current().token {
            Token::Raw => {
                let mut raw = self.clone();
                if raw.skip_raw_segment()
                    && !self.full_text().inner()[start..raw.current().span.start]
                        .contains('\n')
                {
                    self.update(&raw);
                    return true;
                }
                return false;
            }
            Token::LeftComment => {
                let mut comment = self.clone();
                if crate::parsing::collect::consume_valid_comment(&mut comment).is_ok() {
                    self.update(&comment);
                    return true;
                }
                return false;
            }
            Token::LeftBlock => {
                let source = self.full_text().inner();
                let rest = &source[start..];
                let name = rest
                    .strip_prefix("[[")
                    .unwrap_or(rest)
                    .split(|c: char| c.is_whitespace() || c == ']')
                    .next()
                    .unwrap_or("");
                if name.eq_ignore_ascii_case("code") {
                    self.wikidot_code_candidate(start)
                        .map(|candidate| candidate.owner_end)
                } else if name.eq_ignore_ascii_case("math") {
                    rest.match_indices("[[/")
                        .map(|(index, _)| index)
                        .find(|&index| {
                            rest[index..].get(..9).is_some_and(|slice| {
                                slice.eq_ignore_ascii_case("[[/math]]")
                            })
                        })
                        .map(|close| start + close + 9)
                } else {
                    None
                }
            }
            _ => None,
        };
        let Some(end) = end else {
            return false;
        };
        while self.current().span.start < end
            && self.current().token != Token::InputEnd
            && self.step().is_ok()
        {}
        true
    }

    pub fn has_body_end_block(&self, block_rule: &BlockRule) -> bool {
        // An end head needs a `[[/` token, so a source with none left cannot
        // close this body. Checking the cached marker first keeps repeated
        // unclosed openers from rescanning the rest of the token stream.
        {
            let start = self.current().span.start;
            let mut cache = self.block_end_scan_cache.borrow_mut();
            let found = cache.wikidot_first_block_close_after(start);
            if found.is_none() {
                drop(cache);
                // No end head can occur from here, in either iteration mode.
                self.cache_block_end_scan_outcomes(
                    block_rule.name,
                    &[(start, true), (start, false)],
                    false,
                );
                return false;
            }
        }

        let mut parser = self.clone();
        let mut first = true;
        let mut traversed_token_states = Vec::new();

        loop {
            let token_start = parser.current().span.start;
            let exact_key = (block_rule.name, token_start, first);
            let equivalent_key = (block_rule.name, token_start, !first);
            let cached = self.block_end_scan_outcome(exact_key).or_else(|| {
                (parser.current().token != Token::LineBreak)
                    .then(|| self.block_end_scan_outcome(equivalent_key))
                    .flatten()
            });
            if let Some(outcome) = cached {
                let states = &traversed_token_states;
                self.cache_block_end_scan_outcomes(block_rule.name, states, outcome);
                return outcome;
            }
            traversed_token_states.push((token_start, first));
            #[cfg(test)]
            self.increment_block_end_scan_token_visits();

            if block_rule.name != "block-iftags"
                && parser.skip_discarded_iftags_segment(false)
            {
                first = false;
                continue;
            }

            if block_rule.name == "block-bibliography"
                && parser.settings().layout.legacy()
            {
                if parser.skip_wikidot_bibliography_opaque_segment() {
                    first = false;
                    continue;
                }
            } else if parser.skip_raw_segment() {
                first = false;
                continue;
            }

            if parser
                .verify_end_block(first, block_rule, false, false)
                .is_some()
            {
                self.cache_block_end_scan_outcomes(
                    block_rule.name,
                    &traversed_token_states,
                    true,
                );
                return true;
            }

            if parser.current().token == Token::InputEnd {
                self.cache_block_end_scan_outcomes(
                    block_rule.name,
                    &traversed_token_states,
                    false,
                );
                return false;
            }

            if parser.at_hard_body_end_boundary() {
                self.cache_block_end_scan_outcomes(
                    block_rule.name,
                    &traversed_token_states,
                    false,
                );
                return false;
            }

            parser.step().expect("missing input end");
            first = false;
        }
    }

    pub(crate) fn wikidot_alias_has_compatible_close(
        &self,
        block_rule: &BlockRule,
        owner_start: usize,
    ) -> bool {
        debug_assert!(self.settings().layout.legacy());
        debug_assert!(matches!(block_rule.name, "block-div" | "block-span"));

        let cache_name = wikidot_alias_pair_cache_name(block_rule);
        let root_key = (cache_name, owner_start, false);
        if let Some(outcome) = self.block_end_scan_outcome(root_key) {
            return outcome;
        }

        let mut scan = self.clone();
        let mut owners = vec![(owner_start, true)];

        loop {
            if scan.skip_raw_segment() {
                continue;
            }

            if scan.current().token == Token::LeftComment {
                let mut comment = scan.clone();
                if crate::parsing::collect::consume_valid_comment(&mut comment).is_ok() {
                    scan.update(&comment);
                    continue;
                }
            }

            if scan.current().token == Token::LeftBlockEnd {
                let close_start = scan.current().span.start;
                let mut close = scan.clone();
                if let Ok(name) = close.get_end_block() {
                    let close_end = close.current().span.start;
                    if !matches!(name, Cow::Owned(_))
                        && !wikidot_alias_close_is_exact(
                            &scan.full_text().inner()[close_start..close_end],
                            &name,
                        )
                    {
                        scan.update(&close);
                        continue;
                    }
                    let scored_close = name.ends_with('_');
                    let base_name = name.strip_suffix('_').unwrap_or(&name);
                    if block_rule_accepts_end_name(block_rule, base_name, true) {
                        if scored_close {
                            if owners.last().is_some_and(|(_, scored)| *scored) {
                                let (resolved_start, _) = owners
                                    .pop()
                                    .expect("scored closer requires an open owner");
                                self.cache_block_end_scan_outcomes(
                                    cache_name,
                                    &[(resolved_start, false)],
                                    false,
                                );
                                if owners.is_empty() {
                                    return false;
                                }
                            }
                        } else if let Some((resolved_start, scored)) = owners.pop() {
                            if block_rule.name == "block-span" {
                                self.cache_wikidot_span_alias_close_score(
                                    close_start,
                                    scored,
                                );
                            }
                            if scored {
                                self.cache_block_end_scan_outcomes(
                                    cache_name,
                                    &[(resolved_start, false)],
                                    true,
                                );
                            }
                            if owners.is_empty() {
                                return true;
                            }
                        }
                        scan.update(&close);
                        continue;
                    }
                }
            }

            if scan.current().token == Token::LeftBlock {
                let opener_start = scan.current().span.start;
                let mut opener = scan.clone();
                let scored_span_residual_opener = block_rule.name == "block-span"
                    && scan
                        .full_text()
                        .inner()
                        .get(opener_start..)
                        .and_then(|source| source.get(.."[[span_]]]".len()))
                        .is_some_and(|source| source.eq_ignore_ascii_case("[[span_]]]"));
                let parsed_name = if scored_span_residual_opener {
                    opener
                        .get_wikidot_block_name_with_residual_opener()
                        .map(|(name, in_head)| (Cow::Borrowed(name), in_head, false))
                } else {
                    opener.get_block_name(false)
                };
                if let Ok((name, in_head, segment)) = parsed_name {
                    let scored = name.ends_with('_');
                    let base_name = name.strip_suffix('_').unwrap_or(&name);
                    let scored_span_empty_spaced_head =
                        block_rule.name == "block-span" && scored && in_head && {
                            let mut head = opener.clone();
                            head.get_optional_space().is_ok()
                                && head.current().token == Token::RightBlock
                        };
                    if block_rule
                        .accepts_names
                        .iter()
                        .any(|accepted| base_name.eq_ignore_ascii_case(accepted))
                        && opener.head_word_view_is_actionable(
                            matches!(name, Cow::Borrowed(_)),
                            segment,
                            base_name,
                        )
                        && !scored_span_empty_spaced_head
                        && (scored_span_residual_opener
                            || opener.get_optional_space().is_ok()
                                && opener
                                    .get_head_map_with_body_start_wikidot(
                                        block_rule, in_head,
                                    )
                                    .is_ok_and(|(arguments, _)| {
                                        !arguments.has_empty_key()
                                    }))
                    {
                        owners.push((opener_start, scored));
                        scan.update(&opener);
                        continue;
                    }
                }
            }

            if scan.current().token == Token::InputEnd {
                for (unclosed_start, scored) in owners {
                    if scored {
                        self.cache_block_end_scan_outcomes(
                            cache_name,
                            &[(unclosed_start, false)],
                            false,
                        );
                    }
                }
                return false;
            }

            scan.step().expect("missing input end");
        }
    }

    pub(crate) fn has_two_body_end_blocks(&self, block_rule: &BlockRule) -> bool {
        let mut parser = self.clone();
        let mut first = true;
        let mut matches = 0_u8;
        let mut traversed_token_states = Vec::new();

        loop {
            let token_start = parser.current().span.start;
            let key = (block_rule.name, token_start, first);
            let cached = self.two_block_end_scan_outcome(key).or_else(|| {
                self.two_block_end_scan_outcome((block_rule.name, token_start, !first))
            });
            if let Some(suffix_matches) = cached {
                let total_matches = (matches + suffix_matches).min(2);
                self.cache_two_block_end_scan_outcomes(
                    block_rule.name,
                    &traversed_token_states,
                    total_matches,
                );
                return total_matches == 2;
            }
            traversed_token_states.push((token_start, first, matches));
            #[cfg(test)]
            self.increment_block_end_scan_token_visits();

            if parser
                .verify_end_block(first, block_rule, false, false)
                .is_some()
            {
                matches += 1;
                if matches == 2 {
                    self.cache_two_block_end_scan_outcomes(
                        block_rule.name,
                        &traversed_token_states,
                        matches,
                    );
                    return true;
                }
            } else if parser.current().token == Token::InputEnd {
                self.cache_two_block_end_scan_outcomes(
                    block_rule.name,
                    &traversed_token_states,
                    matches,
                );
                return false;
            } else {
                parser.step().expect("missing input end");
            }
            first = false;
        }
    }

    pub(crate) fn consume_body_end_block(
        &mut self,
        first: bool,
        block_rule: &BlockRule,
    ) -> bool {
        self.verify_end_block(first, block_rule, false, false)
            .is_some()
    }

    pub fn has_body_end_block_on_line(&self, block_rule: &BlockRule) -> bool {
        // An end head needs a `[[/` on this physical line. Checking the source
        // first keeps a document with no line break from rescanning the whole
        // remaining token stream once per candidate opener.
        {
            let source = self.full_text().inner();
            let start = self.current().span.start;
            let mut cache = self.block_end_scan_cache.borrow_mut();
            let Some(close) = cache.wikidot_first_block_close_after(start) else {
                return false;
            };
            let line_break =
                find_line_break_marker(source, &mut cache.wikidot_line_break_scan, start);
            if line_break.is_some_and(|line_break| line_break < close) {
                return false;
            }
        }

        let mut parser = self.clone();

        loop {
            if block_rule.name != "block-iftags"
                && parser.skip_discarded_iftags_segment(true)
            {
                continue;
            }
            if parser.skip_raw_segment() {
                continue;
            }
            match parser.current().token {
                Token::InputEnd | Token::LineBreak | Token::ParagraphBreak => {
                    return false;
                }
                Token::LeftBlockEnd => {
                    let mut end = parser.clone();
                    if let Ok(name) = end.get_end_block() {
                        let name = name.strip_suffix('_').unwrap_or(&name);
                        if block_rule_accepts_end_name(
                            block_rule,
                            name,
                            self.settings().layout.legacy(),
                        ) {
                            return true;
                        }
                    }
                    if parser.at_hard_body_end_boundary() {
                        return false;
                    }
                }
                _ => {}
            }
            parser.step().expect("missing input end");
        }
    }

    pub(crate) fn has_native_blockquote_body_end(
        &self,
        block_rule: &BlockRule,
        required_depth: usize,
    ) -> bool {
        self.has_native_blockquote_body_end_with_mode(block_rule, required_depth, false)
    }

    fn quotes_then_block_end(&self, block_rule: &BlockRule) -> bool {
        if self.quote_body_literal_residuals() != Some(true) || self.current().token != Token::Quote {
            return false;
        }
        let required = self.native_blockquote_depth().unwrap_or(0);
        let mut probe = self.clone();
        let mut depth = 0usize;
        while probe.current().token == Token::Quote {
            depth += probe.current().slice.len();
            if probe.step().is_err() || probe.get_optional_space().is_err() {
                return false;
            }
        }
        if depth <= required {
            return false;
        }
        let matching_close = probe.get_end_block().is_ok_and(|name| {
            block_rule_accepts_end_name(
                block_rule,
                name.strip_suffix('_').unwrap_or(&name),
                self.settings().layout.legacy(),
            )
        });
        let _ = probe.get_optional_space();
        matching_close && token_is_body_boundary(probe.current().token)
    }

    fn take_quoted_closer_marks(&mut self) -> Result<Vec<Element<'t>>, ParseError> {
        let elements = self.take_literal_quote_prefix()?;
        if self.current().token == Token::Whitespace {
            let mut probe = self.clone();
            if probe.step().is_ok() && probe.current().token == Token::LeftBlockEnd {
                self.step()?;
            }
        }
        Ok(elements)
    }

    fn take_literal_quote_prefix(&mut self) -> Result<Vec<Element<'t>>, ParseError> {
        let mut elements = Vec::new();
        while matches!(self.current().token, Token::Quote | Token::Whitespace) {
            if self.current().token == Token::Whitespace {
                let mut probe = self.clone();
                if probe.step().is_err() || probe.current().token != Token::Quote {
                    break;
                }
            }
            elements.push(Element::Text(Cow::Borrowed(self.current().slice)));
            self.step()?;
        }
        Ok(elements)
    }

    pub(crate) fn has_surplus_quote_list_close(
        &self,
        block_rule: &BlockRule,
        required_depth: usize,
    ) -> bool {
        let mut parser = self.clone();
        loop {
            let Some((_, _, after_prefix)) =
                parser.scan_absolute_quote_prefix(required_depth)
            else {
                return false;
            };
            parser.update(&after_prefix);
            let mut end = parser.clone();
            let matching_close = end.get_end_block().is_ok_and(|name| {
                block_rule_accepts_end_name(
                    block_rule,
                    name.strip_suffix('_').unwrap_or(&name),
                    self.settings().layout.legacy(),
                )
            });
            if matching_close {
                let _ = end.get_optional_space();
                if token_is_body_boundary(end.current().token) {
                    return true;
                }
            }
            while !token_is_body_boundary(parser.current().token) {
                if parser.step().is_err() {
                    return false;
                }
            }
            if parser.current().token != Token::LineBreak {
                return false;
            }
            if parser.step().is_err() {
                return false;
            }
        }
    }

    #[rustfmt::skip]
    fn cache_native_quote_scan(
        &self,
        block_rule: &BlockRule,
        required_depth: usize,
        allow_inline_close: bool,
        line_starts: &[usize],
        outcome: QuoteScanOutcome,
    ) {
        self.cache_quote_scan_outcomes(block_rule.name, required_depth, allow_inline_close, line_starts, outcome);
    }

    #[rustfmt::skip]
    fn has_native_blockquote_body_end_with_mode(
        &self,
        block_rule: &BlockRule,
        required_depth: usize,
        allow_inline_close: bool,
    ) -> bool {
        let mut parser = self.clone();
        let mut traversed_line_starts = Vec::new();

        loop {
            let line_start = parser.current().span.start;
            let key = (block_rule.name, required_depth, allow_inline_close, line_start);
            if let Some(outcome) = self.quote_scan_outcome(key) {
                self.cache_native_quote_scan(block_rule, required_depth, allow_inline_close, &traversed_line_starts, outcome);
                return outcome == QuoteScanOutcome::HasCandidateClose;
            }
            traversed_line_starts.push(line_start);

            #[cfg(test)]
            self.increment_quote_scan_token_visits();

            let Some((_, absolute_depth, parser_after_prefix)) = parser.scan_absolute_quote_prefix(required_depth) else {
                self.cache_native_quote_scan(block_rule, required_depth, allow_inline_close, &traversed_line_starts, QuoteScanOutcome::Missing);
                return false;
            };
            parser.update(&parser_after_prefix);

            let mut first_candidate = true;
            while first_candidate
                || (allow_inline_close && !token_is_body_boundary(parser.current().token))
            {
                first_candidate = false;
                let mut end = parser.clone();
                if self.settings().layout.legacy()
                    && matches!(
                        block_rule.name,
                        "block-align-center"
                            | "block-align-left"
                            | "block-align-right"
                            | "block-align-justify"
                    )
                    && matches!(
                        end.current().token,
                        Token::BulletItem | Token::NumberedItem
                    )
                {
                    let _ = end.step();
                    let _ = end.get_optional_space();
                }
                let matching_close = end.get_end_block().is_ok_and(|name| block_rule_accepts_end_name(block_rule, name.strip_suffix('_').unwrap_or(&name), self.settings().layout.legacy()));
                let valid_close = if matching_close
                    && absolute_depth == required_depth
                    && allow_inline_close
                {
                    true
                } else if matching_close && absolute_depth == required_depth {
                    let _ = end.get_optional_space();
                    token_is_body_boundary(end.current().token)
                } else {
                    false
                };
                if valid_close {
                    self.cache_native_quote_scan(block_rule, required_depth, allow_inline_close, &traversed_line_starts, QuoteScanOutcome::HasCandidateClose);
                    return true;
                }

                if allow_inline_close && !token_is_body_boundary(parser.current().token) {
                    #[cfg(test)]
                    self.increment_quote_scan_token_visits();
                    parser.step().expect("tokenization always ends with input-end");
                }
            }

            while !token_is_body_boundary(parser.current().token) {
                #[cfg(test)]
                self.increment_quote_scan_token_visits();
                parser.step().expect("tokenization always ends with input-end");
            }

            if parser.current().token != Token::LineBreak {
                self.cache_native_quote_scan(block_rule, required_depth, allow_inline_close, &traversed_line_starts, QuoteScanOutcome::Missing);
                return false;
            }

            #[cfg(test)]
            self.increment_quote_scan_token_visits();
            parser.step().expect("line break must precede input-end");
        }
    }

    fn has_native_blockquote_body_end_tolerant(&self, block_rule: &BlockRule) -> bool {
        let depth_key = usize::MAX;
        let mut parser = self.clone();
        let mut traversed_line_starts = Vec::new();
        loop {
            let line_start = parser.current().span.start;
            if let Some(outcome) =
                self.quote_scan_outcome((block_rule.name, depth_key, false, line_start))
            {
                self.cache_native_quote_scan(
                    block_rule,
                    depth_key,
                    false,
                    &traversed_line_starts,
                    outcome,
                );
                return outcome == QuoteScanOutcome::HasCandidateClose;
            }
            traversed_line_starts.push(line_start);
            match parser.current().token {
                Token::InputEnd => {
                    self.cache_native_quote_scan(
                        block_rule,
                        depth_key,
                        false,
                        &traversed_line_starts,
                        QuoteScanOutcome::Missing,
                    );
                    return false;
                }
                Token::ParagraphBreak => {
                    parser
                        .step()
                        .expect("tokenization always ends with input-end");
                    continue;
                }
                _ => {}
            }
            if let Some((_, _, after_prefix)) = parser.scan_absolute_quote_prefix(1) {
                parser.update(&after_prefix);
            }
            loop {
                let mut end = parser.clone();
                let matching_close = end.get_end_block().is_ok_and(|name| {
                    block_rule_accepts_end_name(
                        block_rule,
                        name.strip_suffix('_').unwrap_or(&name),
                        self.settings().layout.legacy(),
                    )
                });
                if matching_close {
                    let _ = end.get_optional_space();
                    if token_is_body_boundary(end.current().token) {
                        self.cache_native_quote_scan(
                            block_rule,
                            depth_key,
                            false,
                            &traversed_line_starts,
                            QuoteScanOutcome::HasCandidateClose,
                        );
                        return true;
                    }
                }
                if token_is_body_boundary(parser.current().token) {
                    break;
                }
                parser
                    .step()
                    .expect("tokenization always ends with input-end");
            }
            match parser.current().token {
                Token::LineBreak | Token::ParagraphBreak => {
                    parser.step().expect("line break must precede input-end");
                }
                _ => {
                    self.cache_native_quote_scan(
                        block_rule,
                        depth_key,
                        false,
                        &traversed_line_starts,
                        QuoteScanOutcome::Missing,
                    );
                    return false;
                }
            }
        }
    }

    pub fn get_head_map(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
    ) -> Result<Arguments<'t>, ParseError> {
        self.get_head_map_with_body_start(block_rule, in_head)
            .map(|(arguments, _)| arguments)
    }

    pub(crate) fn get_head_map_with_body_start(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
    ) -> Result<(Arguments<'t>, BlockBodyStart), ParseError> {
        let mut map = Arguments::new();
        if in_head {
            loop {
                while token_is_argument_spacing(self.current().token) {
                    self.step()?;
                }

                if self.current().token == Token::LeftComment {
                    let mut candidate = self.clone();
                    if consume_valid_comment(&mut candidate).is_ok() {
                        self.update(&candidate);
                        continue;
                    }
                }

                let start = self.current();
                let mut args_finished = false;
                let mut key_elisions: Vec<std::ops::Range<usize>> = Vec::new();
                loop {
                    let current = self.current();
                    if current.token == Token::LeftBlock
                        && let Some(segment) = consume_iftags_segment(self)?
                    {
                        key_elisions.extend(segment.elisions());
                        continue;
                    }
                    if current.token == Token::LeftComment {
                        let mut candidate = self.clone();
                        if let Ok(range) = consume_valid_comment(&mut candidate) {
                            key_elisions.push(range);
                            self.update(&candidate);
                            continue;
                        }
                    }
                    match current.token {
                        Token::RightBlock => {
                            args_finished = true;
                            break;
                        }

                        token if token_ends_argument_key(token) => break,

                        _ if ARGUMENT_KEY.is_match(current.slice) => {
                            self.step()?;
                        }

                        _ => {
                            return Err(
                                self.make_err(ParseErrorKind::BlockMalformedArguments)
                            );
                        }
                    }
                }

                if args_finished {
                    break std::convert::identity(());
                }

                let end = self.current();
                let key: Cow<'t, str> = if key_elisions.is_empty() {
                    Cow::Borrowed(self.full_text().slice_partial(start, end))
                } else {
                    let source = self.full_text().inner();
                    let mut resolved = String::new();
                    let mut cursor = start.span.start;
                    for elision in &key_elisions {
                        if cursor < elision.start {
                            resolved.push_str(&source[cursor..elision.start]);
                        }
                        cursor = cursor.max(elision.end);
                    }
                    if cursor < end.span.start {
                        resolved.push_str(&source[cursor..end.span.start]);
                    }
                    Cow::Owned(resolved)
                };
                if key.is_empty()
                    || !key.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
                    })
                {
                    return Err(self.make_err(ParseErrorKind::BlockMalformedArguments));
                }

                let space_before_equals = self.current().token == Token::Whitespace;
                self.get_optional_space()?;
                self.get_token(Token::Equals, ParseErrorKind::BlockMalformedArguments)?;

                let space_after_equals = self.current().token == Token::Whitespace;
                self.get_optional_space()?;
                let bare = self.current().token != Token::DoubleQuote;
                let value = self.get_block_argument_value(block_rule, &key)?;

                if bare {
                    map.insert_bare_cow(key, value);
                } else {
                    map.insert_cow(key, value);
                }
                if space_before_equals || space_after_equals {
                    map.mark_spaced_equals();
                }
            }
        }

        let body_start = self.get_head_block_with_body_start(block_rule, in_head)?;
        Ok((map, body_start))
    }

    pub fn get_head_map_wikidot(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
    ) -> Result<Arguments<'t>, ParseError> {
        self.get_head_map_with_body_start_wikidot(block_rule, in_head)
            .map(|(arguments, _)| arguments)
    }

    pub(crate) fn get_head_map_with_body_start_wikidot(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
    ) -> Result<(Arguments<'t>, BlockBodyStart), ParseError> {
        self.get_head_map_with_body_start_wikidot_custom(
            block_rule,
            in_head,
            parse_wikidot_attribute_field,
            false,
        )
    }

    pub(crate) fn get_module_head_name_map_with_body_start_wikidot(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
    ) -> Result<(&'t str, Arguments<'t>, BlockBodyStart), ParseError> {
        if !self.settings().layout.legacy() {
            return self.get_head_name_map_with_body_start(block_rule, in_head);
        }
        if !in_head {
            return Err(self.make_err(ParseErrorKind::BlockMissingName));
        }
        let missing_name = ParseErrorKind::ModuleMissingName;
        let (subname, in_head) = self.get_head_word_raw(missing_name)?;
        let (arguments, body_start) = self.get_head_map_with_body_start_wikidot_custom(
            block_rule,
            in_head,
            parse_wikidot_module_argument_field,
            true,
        )?;
        Ok((subname, arguments, body_start))
    }

    fn get_head_map_with_body_start_wikidot_custom(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
        parse_arguments: WikidotHeadArgumentsFn,
        same_line: bool,
    ) -> Result<(Arguments<'t>, BlockBodyStart), ParseError> {
        if !self.settings().layout.legacy() {
            return self.get_head_map_with_body_start(block_rule, in_head);
        }

        let (arguments, right_block_consumed) = if in_head {
            let mut head_tokens = std::iter::once(self.current())
                .chain(self.remaining())
                .take_while(|token| {
                    !matches!(token.token, Token::RightBlock | Token::InputEnd)
                });
            let head_has_comment = head_tokens
                .clone()
                .any(|token| token.token == Token::LeftComment);
            let head_has_non_authored_input = head_tokens.any(|token| {
                matches!(
                    token.token,
                    Token::GeneratedPageLink
                        | Token::GeneratedTagLinks
                        | Token::RuntimeText
                )
            });
            let scan = wikidot_head_scan(block_rule);
            if head_has_comment
                && !head_has_non_authored_input
                && scan == WikidotHeadScan::Plain
            {
                let closes = [
                    ParseCondition::current(Token::RightBlock),
                    ParseCondition::current(Token::RightLink),
                    ParseCondition::current(Token::LineBreak),
                    ParseCondition::current(Token::ParagraphBreak),
                ];
                let closes: &[ParseCondition] =
                    if same_line { &closes } else { &closes[..2] };
                let (head, last) =
                    collect_comment_elided_keep(self, closes, &[], None, None)?;
                if last.token == Token::RightLink {
                    self.note_fused_name_residual(last.span.end);
                }
                if same_line
                    && matches!(last.token, Token::LineBreak | Token::ParagraphBreak)
                {
                    return Err(self.make_err(ParseErrorKind::BlockMalformedArguments));
                }
                (parse_arguments(&head), true)
            } else {
                let start = self.current();
                // The block-name collector consumes a discarded control that
                // the scanner dropped; restore it into the head span so the
                // formfeed attribute rule below sees it.
                let control_start = self.wikidot_head_control_start();
                let start = if control_start < start.span.start {
                    let source = self.full_text().inner();
                    crate::parsing::ExtractedToken {
                        token: start.token,
                        slice: &source[control_start..start.span.end],
                        span: control_start..start.span.end,
                    }
                } else {
                    start.clone()
                };
                let mut comments = Vec::new();
                let mut elisions = Vec::new();
                let raw_head = block_rule.name == "block-code";
                let scan = wikidot_head_scan(block_rule);
                let scan = if !same_line
                    && scan == WikidotHeadScan::LineEndGreedy
                    && !wikidot_line_greedy_head_is_usable(self)
                {
                    WikidotHeadScan::Plain
                } else {
                    scan
                };
                let mut in_quote = false;
                let mut head_terminated = false;
                loop {
                    let current = self.current();
                    if current.token == Token::InputEnd {
                        break;
                    }

                    if !same_line {
                        match scan {
                            WikidotHeadScan::LineEndGreedy => match current.token {
                                Token::LeftComment => {
                                    comments.push(consume_valid_comment(self)?);
                                    continue;
                                }
                                Token::RightBlock | Token::RightLink => {
                                    if !wikidot_head_terminator_ends_line(self) {
                                        self.step()?;
                                        continue;
                                    }
                                    break;
                                }
                                _ => {}
                            },
                            WikidotHeadScan::QuotedValuesAreLiteral => {
                                match current.token {
                                    Token::LeftComment => {
                                        comments.push(consume_valid_comment(self)?);
                                        continue;
                                    }
                                    Token::DoubleQuote => {
                                        in_quote = !in_quote;
                                        self.step()?;
                                        continue;
                                    }
                                    Token::RightBlock | Token::RightLink if in_quote => {
                                        self.step()?;
                                        continue;
                                    }
                                    // A quoted value may run over a line break.
                                    Token::LineBreak | Token::ParagraphBreak
                                        if in_quote =>
                                    {
                                        self.step()?;
                                        continue;
                                    }
                                    Token::RightBlock | Token::RightLink => break,
                                    _ => {}
                                }
                            }
                            WikidotHeadScan::EndsAtSingleBracket => match current.token {
                                Token::LeftComment => {
                                    comments.push(consume_valid_comment(self)?);
                                    continue;
                                }
                                Token::RightBlock
                                | Token::RightLink
                                | Token::RightBracket => break,
                                _ => {}
                            },
                            WikidotHeadScan::Plain => match current.token {
                                Token::RightBlock | Token::RightLink => break,
                                _ => {}
                            },
                            WikidotHeadScan::QuotedLinksPass => match current.token {
                                Token::LeftComment => {
                                    comments.push(consume_valid_comment(self)?);
                                    continue;
                                }
                                Token::DoubleQuote => in_quote = !in_quote,
                                Token::RightBlock => break,
                                Token::RightLink if !in_quote => break,
                                _ => {}
                            },
                        }
                        if block_rule.name == "block-collapsible"
                            && current.token == Token::LeftBlock
                            && let Some((range, head_closed)) =
                                self.try_consume_collapsible_head_footnote()?
                        {
                            elisions.push(range);
                            if head_closed {
                                head_terminated = true;
                                break;
                            }
                            continue;
                        }
                        if !raw_head
                            && current.token == Token::LeftBlock
                            && let Some(segment) = consume_iftags_segment(self)?
                        {
                            elisions.extend(segment.elisions());
                            continue;
                        }
                        self.step()?;
                        continue;
                    }

                    if current.token == Token::RightLink {
                        break;
                    }
                    if current.token == Token::RightBlock {
                        if !(same_line && in_quote) {
                            break;
                        }
                        self.step()?;
                        continue;
                    }
                    if matches!(current.token, Token::LineBreak | Token::ParagraphBreak) {
                        break;
                    }
                    if current.token == Token::DoubleQuote {
                        in_quote = !in_quote;
                    }
                    if block_rule.name == "block-collapsible"
                        && current.token == Token::LeftBlock
                        && let Some((range, head_closed)) =
                            self.try_consume_collapsible_head_footnote()?
                    {
                        elisions.push(range);
                        if head_closed {
                            head_terminated = true;
                            break;
                        }
                        continue;
                    }
                    if !raw_head
                        && current.token == Token::LeftBlock
                        && let Some(segment) = consume_iftags_segment(self)?
                    {
                        elisions.extend(segment.elisions());
                        continue;
                    }
                    self.step()?;
                }
                let span = start.span.start..self.current().span.start;
                let begins_with_control = start.span.start < self.current().span.start
                    && self
                        .full_text()
                        .inner()
                        .as_bytes()
                        .get(start.span.start)
                        .is_some_and(|byte| {
                            matches!(
                                byte,
                                0x00..=0x08 | 0x0B..=0x0C | 0x0E..=0x1A | 0x1C..=0x1F
                            )
                        });
                let head_begins_with_close =
                    matches!(start.token, Token::RightBlock | Token::RightLink);
                if !same_line
                    && scan == WikidotHeadScan::LineEndGreedy
                    && head_begins_with_close
                    && !begins_with_control
                    && !span.is_empty()
                {
                    return Err(self.make_err(ParseErrorKind::BlockMalformedArguments));
                }
                // Live's per-kind opener regexes need something after the
                // whitespace separator: a head that is exactly one discarded
                // control fails the opener for most kinds (a doubled control
                // still forms it).
                if !same_line
                    && block_rule.name != "block-div"
                    && block_rule.name != "block-code"
                    && block_rule.name != "block-date"
                    && block_rule.name != "block-image"
                    && block_rule.name != "block-tab"
                    && block_rule.name != "block-tabview"
                    && span.end > span.start
                    && span.end - span.start == 1
                    && self
                        .full_text()
                        .inner()
                        .as_bytes()
                        .get(span.start)
                        .is_some_and(|byte| {
                            matches!(
                                byte,
                                0x00..=0x08 | 0x0B..=0x0C | 0x0E..=0x1A | 0x1C..=0x1F
                            )
                        })
                {
                    return Err(self.make_err(ParseErrorKind::RuleFailed));
                }
                let head =
                    CommentElidedText::new(self.full_text().inner(), span, comments)
                        .with_segments(elisions);
                let fused_close =
                    head_terminated || self.current().token == Token::RightLink;
                if fused_close && !head_terminated {
                    self.note_fused_name_residual(self.current().span.end);
                    self.step()?;
                }
                (parse_arguments(&head), fused_close)
            }
        } else {
            (Arguments::new_case_sensitive(), false)
        };
        let body_start = self.get_head_block_with_body_start(
            block_rule,
            in_head && !right_block_consumed,
        )?;
        Ok((arguments, body_start))
    }

    fn try_consume_collapsible_head_footnote(
        &mut self,
    ) -> Result<Option<(std::ops::Range<usize>, bool)>, ParseError> {
        if self.current().token != Token::LeftBlock {
            return Ok(None);
        }
        let start = self.current().span.start;
        let mut probe = self.clone();
        probe.get_optional_space()?;
        let Ok((name, in_head, _)) = probe.get_block_name(false) else {
            return Ok(None);
        };
        if !name.eq_ignore_ascii_case("footnote") {
            return Ok(None);
        }
        let before = self.footnote_count();
        probe.set_block(&super::blocks::BLOCK_FOOTNOTE);
        let parsed = super::blocks::parse_footnote_ref(
            &mut probe, "footnote", start, false, false, in_head,
        );
        let Ok(parsed) = parsed else {
            self.truncate_footnotes(before);
            return Ok(None);
        };
        if self.footnote_count() == before {
            self.truncate_footnotes(before);
            return Ok(None);
        }
        let fused = matches!(
            &parsed.item,
            Elements::Multiple(elements)
                if matches!(elements.last(), Some(Element::Text(text)) if text.as_ref() == "]")
        );
        self.update(&probe);
        let head_closed = fused && self.current().token == Token::RightBracket;
        if head_closed {
            self.step()?;
        }
        Ok(Some((start..self.current().span.start, head_closed)))
    }

    pub fn get_head_name_map(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
    ) -> Result<(Cow<'t, str>, Arguments<'t>), ParseError> {
        if !in_head {
            return Err(self.make_err(ParseErrorKind::BlockMissingName));
        }

        let missing_name = ParseErrorKind::ModuleMissingName;
        let (subname, in_head) = self.get_head_word(missing_name)?;

        let (arguments, _) = self.get_head_map_with_body_start(block_rule, in_head)?;

        Ok((subname, arguments))
    }

    pub(crate) fn get_head_name_map_with_body_start(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
    ) -> Result<(&'t str, Arguments<'t>, BlockBodyStart), ParseError> {
        if !in_head {
            return Err(self.make_err(ParseErrorKind::BlockMissingName));
        }

        let missing_name = ParseErrorKind::ModuleMissingName;
        let (subname, in_head) = self.get_head_word_raw(missing_name)?;

        let (arguments, body_start) =
            self.get_head_map_with_body_start(block_rule, in_head)?;

        Ok((subname, arguments, body_start))
    }

    pub fn get_head_name_map_wikidot(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
    ) -> Result<(Cow<'t, str>, Arguments<'t>), ParseError> {
        if !self.settings().layout.legacy() {
            return self.get_head_name_map(block_rule, in_head);
        }
        if !in_head {
            return Err(self.make_err(ParseErrorKind::BlockMissingName));
        }

        let missing_name = ParseErrorKind::ModuleMissingName;
        let (subname, in_head) = self.get_head_word(missing_name)?;
        let arguments = self.get_head_map_wikidot(block_rule, in_head)?;

        Ok((subname, arguments))
    }

    pub(crate) fn get_head_field_map_wikidot(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
    ) -> Result<(CommentElidedText<'t>, Arguments<'t>), ParseError> {
        debug_assert!(self.settings().layout.legacy());
        if !in_head {
            return Err(self.make_err(ParseErrorKind::BlockMissingName));
        }

        let closes = [
            ParseCondition::current(Token::Whitespace),
            ParseCondition::current(Token::RightBlock),
        ];
        let invalids = [
            ParseCondition::current(Token::LineBreak),
            ParseCondition::current(Token::ParagraphBreak),
        ];
        let (field, last) = collect_comment_elided_keep(
            self,
            &closes,
            &invalids,
            Some(ParseErrorKind::recoverable_local_failure()),
            None,
        )?;
        let head_ended = matches!(last.token, Token::RightBlock | Token::RightBracket);
        let arguments = self.get_head_map_wikidot(block_rule, !head_ended)?;
        Ok((field, arguments))
    }

    pub(crate) fn get_head_field_map(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
    ) -> Result<(Cow<'t, str>, Arguments<'t>), ParseError> {
        if !in_head {
            return Err(self.make_err(ParseErrorKind::BlockMissingName));
        }

        while self.current().token == Token::LeftComment {
            let mut probe = self.clone();
            if consume_valid_comment(&mut probe).is_err() {
                break;
            }
            self.update(&probe);
            if self.current().token == Token::Whitespace {
                self.step()?;
            }
        }

        let end_conditions = [
            ParseCondition::current(Token::Whitespace),
            ParseCondition::current(Token::LineBreak),
            ParseCondition::current(Token::ParagraphBreak),
            ParseCondition::current(Token::RightBlock),
        ];
        let rule = self.rule();
        let fused_name_close = self.head_had_segment();
        let (field, last) = collect_text_resolved_fused(
            self,
            rule,
            &end_conditions,
            &[],
            Some(ParseErrorKind::BlockMissingName),
            fused_name_close,
        )?;
        let (arguments, _) = self.get_head_map_with_body_start(
            block_rule,
            !matches!(last.token, Token::RightBlock | Token::RightLink),
        )?;
        Ok((field, arguments))
    }

    pub fn get_head_value<F, T>(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
        convert: F,
    ) -> Result<T, ParseError>
    where
        F: FnOnce(&Self, Option<&'t str>) -> Result<T, ParseError>,
    {
        self.get_head_value_with_body_start(block_rule, in_head, convert)
            .map(|(value, _)| value)
    }

    pub fn get_head_value_resolved<F, T>(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
        convert: F,
    ) -> Result<T, ParseError>
    where
        F: FnOnce(&Self, Option<Cow<'t, str>>) -> Result<T, ParseError>,
    {
        self.get_head_value_resolved_with_body_start(block_rule, in_head, convert)
            .map(|(value, _)| value)
    }

    pub(crate) fn get_head_value_resolved_with_body_start<F, T>(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
        convert: F,
    ) -> Result<(T, BlockBodyStart), ParseError>
    where
        F: FnOnce(&Self, Option<Cow<'t, str>>) -> Result<T, ParseError>,
    {
        let argument = if in_head {
            let end_conditions = [
                ParseCondition::current(Token::RightBlock),
                ParseCondition::current(Token::RightLink),
            ];
            let reject_conditions = [
                ParseCondition::current(Token::ParagraphBreak),
                ParseCondition::current(Token::LineBreak),
            ];
            let rule = self.rule();
            let fused_name_close = self.head_had_segment();
            let quoted_head =
                self.settings().layout.legacy() && block_rule.name() == "block-math";
            let (slice, last, segment_consumed) = if quoted_head {
                collect_text_resolved_fused_tracked_quoted(
                    self,
                    rule,
                    &end_conditions,
                    &reject_conditions,
                    Some(ParseErrorKind::BlockMalformedArguments),
                )?
            } else {
                collect_text_resolved_fused_tracked(
                    self,
                    rule,
                    &end_conditions,
                    &reject_conditions,
                    Some(ParseErrorKind::BlockMalformedArguments),
                )?
            };
            if last.token == Token::RightLink {
                if !(self.settings().layout.legacy()
                    && (fused_name_close || segment_consumed))
                {
                    return Err(self.make_err(ParseErrorKind::BlockMalformedArguments));
                }
                self.note_fused_name_residual(last.span.end);
            }

            Some(slice)
        } else {
            None
        };

        let value = convert(self, argument)?;
        let body_start = self.get_head_block_with_body_start(block_rule, false)?;
        Ok((value, body_start))
    }

    pub(crate) fn get_head_value_with_body_start<F, T>(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
        convert: F,
    ) -> Result<(T, BlockBodyStart), ParseError>
    where
        F: FnOnce(&Self, Option<&'t str>) -> Result<T, ParseError>,
    {
        let argument = if in_head {
            let reject_conditions = [
                ParseCondition::current(Token::ParagraphBreak),
                ParseCondition::current(Token::LineBreak),
            ];
            let rule = self.rule();
            let kind = ParseErrorKind::BlockMalformedArguments;
            let malformed_arguments = Some(kind);
            let stops = [
                ParseCondition::current(Token::RightBlock),
                ParseCondition::current(Token::RightLink),
            ];
            let rejects = &reject_conditions;
            let (slice, last) =
                collect_text_keep(self, rule, &stops, rejects, malformed_arguments)?;
            if last.token == Token::RightLink {
                if !(self.settings().layout.legacy() && self.head_had_segment()) {
                    return Err(self.make_err(kind));
                }
                self.note_fused_name_residual(last.span.end);
            }

            Some(slice)
        } else {
            None
        };

        let value = convert(self, argument)?;

        let body_start = self.get_head_block_with_body_start(block_rule, false)?;
        Ok((value, body_start))
    }

    pub(crate) fn get_gate_head_with_body_start<F, T>(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
        convert: F,
    ) -> Result<(T, BlockBodyStart, GateHeadClose), ParseError>
    where
        F: FnOnce(&Self, Option<&'t str>) -> Result<T, ParseError>,
    {
        let (argument, close) = if in_head {
            let end_conditions = [
                ParseCondition::current(Token::RightBlock),
                ParseCondition::current(Token::RightLink),
            ];
            let reject_conditions = [
                ParseCondition::current(Token::ParagraphBreak),
                ParseCondition::current(Token::LineBreak),
            ];
            let rule = self.rule();
            let (slice, last) = collect_text_keep(
                self,
                rule,
                &end_conditions,
                &reject_conditions,
                Some(ParseErrorKind::BlockMalformedArguments),
            )?;
            match last.token {
                Token::RightLink => (Some(slice), GateHeadClose::Fused),
                _ => match slice.strip_suffix(']') {
                    Some(trimmed) => (Some(trimmed), GateHeadClose::Fused),
                    None => (Some(slice), GateHeadClose::Block),
                },
            }
        } else {
            (None, GateHeadClose::Block)
        };

        let value = convert(self, argument)?;

        let body_start = match close {
            GateHeadClose::Fused => BlockBodyStart::Inline,
            GateHeadClose::Block => {
                self.get_head_block_with_body_start(block_rule, false)?
            }
        };
        Ok((value, body_start, close))
    }

    pub(crate) fn wikidot_head_space_region(
        &self,
        opener_start: usize,
        flag_star: bool,
    ) -> WikidotHeadSpaceRegion {
        if flag_star {
            return WikidotHeadSpaceRegion::Other;
        }
        let source = self.full_text().inner();
        let bytes = source.as_bytes();
        let mut word_end = opener_start + 2;
        while let Some(&byte) = bytes.get(word_end) {
            if matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b']' | b'[') {
                break;
            }
            word_end += 1;
        }
        let mut probe = self.clone();
        let head_end = loop {
            match probe.current().token {
                Token::Whitespace | Token::LineBreak | Token::ParagraphBreak => {
                    if probe.step().is_err() {
                        return WikidotHeadSpaceRegion::Other;
                    }
                }
                Token::LeftComment => {
                    let mut candidate = probe.clone();
                    if consume_valid_comment(&mut candidate).is_err() {
                        return WikidotHeadSpaceRegion::Other;
                    }
                    probe.update(&candidate);
                }
                Token::RightBlock => break probe.current().span.start,
                _ => return WikidotHeadSpaceRegion::Other,
            }
        };
        if head_end < word_end {
            return WikidotHeadSpaceRegion::Other;
        }
        let mut region = String::new();
        let mut rest = &source[word_end..head_end];
        loop {
            match rest.find("[!--") {
                None => {
                    region.push_str(rest);
                    break;
                }
                Some(open) => {
                    region.push_str(&rest[..open]);
                    let after = &rest[open + 4..];
                    match after.find("--]") {
                        Some(close) => {
                            rest = &after[close + 3..];
                        }
                        None => return WikidotHeadSpaceRegion::Other,
                    }
                }
            }
        }
        if region == " " || region == "\n" {
            WikidotHeadSpaceRegion::SingleSeparator
        } else if !region.is_empty()
            && region
                .chars()
                .all(|c| matches!(c, ' ' | '\t' | '\n' | '\r'))
        {
            WikidotHeadSpaceRegion::WhitespaceOnly
        } else {
            WikidotHeadSpaceRegion::Other
        }
    }

    pub(crate) fn wikidot_head_region_is_empty(
        &self,
        opener_start: usize,
    ) -> Option<bool> {
        let source = self.full_text().inner();
        let bytes = source.as_bytes();
        let mut word_end = opener_start + 2;
        while matches!(bytes.get(word_end), Some(b' ' | b'\t')) {
            word_end += 1;
        }
        while let Some(&byte) = bytes.get(word_end) {
            if matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b']' | b'[') {
                break;
            }
            word_end += 1;
        }
        let mut probe = self.clone();
        let head_end = loop {
            match probe.current().token {
                Token::Whitespace | Token::LineBreak | Token::ParagraphBreak => {
                    if probe.step().is_err() {
                        return None;
                    }
                }
                Token::LeftComment => {
                    let mut candidate = probe.clone();
                    if consume_valid_comment(&mut candidate).is_err() {
                        return None;
                    }
                    probe.update(&candidate);
                }
                Token::RightBlock => break probe.current().span.start,
                Token::RightLink
                | Token::LeftBlock
                | Token::LeftBlockStar
                | Token::LeftBlockEnd
                | Token::InputEnd => return None,
                _ => {
                    if probe.step().is_err() {
                        return None;
                    }
                }
            }
        };
        if head_end < word_end {
            return None;
        }
        let mut region = String::new();
        let mut rest = &source[word_end..head_end];
        loop {
            match rest.find("[!--") {
                None => {
                    region.push_str(rest);
                    break;
                }
                Some(open) => {
                    region.push_str(&rest[..open]);
                    let after = &rest[open + 4..];
                    let close = after.find("--]")?;
                    rest = &after[close + 3..];
                }
            }
        }
        Some(region.is_empty())
    }

    pub fn get_head_none(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
    ) -> Result<(), ParseError> {
        self.get_head_none_with_body_start(block_rule, in_head)
            .map(drop)
    }

    pub(crate) fn get_head_none_with_body_start(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
    ) -> Result<BlockBodyStart, ParseError> {
        self.get_optional_space()?;
        self.get_head_block_with_body_start(block_rule, in_head)
    }

    fn get_head_block_with_body_start(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
    ) -> Result<BlockBodyStart, ParseError> {
        if in_head {
            self.get_token(Token::RightBlock, ParseErrorKind::BlockMissingCloseBrackets)?;
        }

        if block_rule.accepts_newlines_outside_body {
            if self.current().token == Token::LineBreak {
                self.step()?;
                return Ok(BlockBodyStart::NextPhysicalLine);
            }
            if self.current().token == Token::ParagraphBreak {
                return Ok(BlockBodyStart::NextPhysicalLine);
            }
        }

        Ok(BlockBodyStart::Inline)
    }

    #[inline]
    pub fn set_block(&mut self, block_rule: &BlockRule) {
        self.set_rule(block_rule.rule());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::ParseErrorKind;
    use crate::parsing::rule::impls::block::blocks::{
        BLOCK_COLLAPSIBLE, BLOCK_DIV, BLOCK_IFTAGS,
    };
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn close_marker_search_matches_uncached_textual_scan() {
        let sources = [
            "[[note]]body[[/note]]",
            "[[note]]body[[/ NOTE]]",
            "[[note]]body[[/notex]]",
            "[[note]]body[!--[[/note]]--]",
            "[[note]]body[[/div]]",
            "[[div]][[/div]][[note]]",
            "[[note]]body [[/ note ]]",
            "[[note]]body",
            "[[/[[footnote]]",
            "[[/foot[[iftags -z]][[/iftags]]note]]",
        ];
        fn end_heads(source: &str) -> Vec<(usize, usize)> {
            let mut heads = Vec::new();
            let mut search = 0usize;
            while let Some(rel) = source.get(search..).and_then(|s| s.find("[[/")) {
                let close_start = search + rel;
                let name_start = close_start + 3;
                let name_end = source
                    .get(name_start..)
                    .map(|after_slash| {
                        name_start
                            + after_slash
                                .find(|c: char| c.is_whitespace() || c == ']')
                                .unwrap_or(after_slash.len())
                    })
                    .unwrap_or(name_start);
                heads.push((close_start, name_end));
                search = close_start + 3;
            }
            heads
        }

        for source in sources {
            let heads = end_heads(source);
            for start in 0..=source.len() {
                if !source.is_char_boundary(start) {
                    continue;
                }
                let mut slot = None;
                let found =
                    wikidot_first_block_close_after_from_heads(&heads, &mut slot, start);
                assert_eq!(
                    found,
                    source[start..].find("[[/").map(|offset| start + offset),
                    "{source:?} @{start}",
                );
            }
        }
    }

    #[test]
    fn end_head_requirement_whitelist_excludes_boundary_closed_blocks() {
        use crate::parsing::rule::impls::block::blocks::{BLOCK_LI, BLOCK_TAB};

        assert!(super::wikidot_body_requires_end_head(&BLOCK_DIV));
        assert!(super::wikidot_body_requires_end_head(&BLOCK_COLLAPSIBLE));
        assert!(!super::wikidot_body_requires_end_head(&BLOCK_TAB));
        assert!(!super::wikidot_body_requires_end_head(&BLOCK_LI));
    }

    #[test]
    fn wikidot_attribute_offsets_preserve_multibyte_malformed_recovery() {
        let mut arguments = parse_wikidot_attributes("a=\"éb=\"value\"");
        assert_eq!(arguments.get("a").as_deref(), Some(""));
        assert_eq!(arguments.get("b").as_deref(), Some("value"));
        assert!(
            arguments.is_empty(),
            "unexpected recovered keys: {arguments:?}"
        );
    }

    #[test]
    fn wikidot_attribute_key_starts_after_the_last_comment_byte() {
        let source = "prefix [!--drop--] class=\"日本語\"";
        let comment_start = source.find("[!--").unwrap();
        let comment_end = source.find("--]").unwrap() + 3;
        let comment_ranges: Vec<_> =
            std::iter::once(comment_start..comment_end).collect();
        let field = CommentElidedText::new(source, 0..source.len(), comment_ranges);
        let mut arguments = parse_wikidot_attribute_field(&field);

        assert_eq!(arguments.get("class").as_deref(), Some("日本語"));
        assert!(
            arguments.is_empty(),
            "comment bytes leaked into a key: {arguments:?}"
        );
    }

    #[test]
    fn wikijump_block_head_rejects_invalid_argument_key_token() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        for input in [
            "[[collapsible @=\"value\"]]body[[/collapsible]]",
            "[[collapsible =\"value\"]]body[[/collapsible]]",
        ] {
            let tokenization = crate::tokenize(input);
            let (_, errors) = crate::parse(&tokenization, &page_info, &settings).into();

            assert!(
                errors
                    .iter()
                    .any(|error| error.kind() == ParseErrorKind::BlockMalformedArguments),
                "{input} should report BlockMalformedArguments: {errors:?}",
            );
        }
    }

    #[test]
    fn block_head_allows_line_break_before_argument() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[div\nclass=\"value\"]]\nbody\n[[/div]]");
        let (_, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn native_quote_close_scan_caches_every_traversed_line_start() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = (0..256)
            .map(|index| format!("> body-{index}\n"))
            .collect::<String>();
        let tokenization = crate::tokenize(&input);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("first quote should follow input start");

        assert!(!parser.has_native_blockquote_body_end(&BLOCK_COLLAPSIBLE, 1));
        let first_scan_visits = parser.quote_scan_token_visits();
        assert!(first_scan_visits > 0);
        assert!(first_scan_visits <= tokenization.tokens().len() * 2);

        loop {
            if parser.current().token == Token::InputEnd {
                break;
            }
            assert_eq!(parser.current().token, Token::Quote);
            assert!(!parser.has_native_blockquote_body_end(&BLOCK_COLLAPSIBLE, 1));
            assert_eq!(parser.quote_scan_token_visits(), first_scan_visits);

            while !matches!(parser.current().token, Token::LineBreak | Token::InputEnd) {
                parser.step().expect("input end must remain available");
            }
            if parser.current().token == Token::LineBreak {
                parser.step().expect("next line or input end must exist");
            }
        }
    }

    #[test]
    fn quote_scan_operation_count_scales_with_input_tokens() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for line_count in [32, 64, 128, 256, 512] {
            let input = (0..line_count)
                .map(|index| format!("> body-{index}\n"))
                .collect::<String>();
            let tokenization = crate::tokenize(&input);
            let mut parser = Parser::new(&tokenization, &page_info, &settings);
            parser
                .step()
                .expect("first quote should follow input start");

            assert!(!parser.has_native_blockquote_body_end(&BLOCK_COLLAPSIBLE, 1));
            assert!(
                parser.quote_scan_token_visits() <= tokenization.tokens().len() * 2,
                "{line_count} lines visited {} tokens for {} input tokens",
                parser.quote_scan_token_visits(),
                tokenization.tokens().len(),
            );
        }
    }

    #[test]
    fn two_block_end_scan_operation_count_scales_with_input_tokens() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for depth in [32, 64, 128, 256, 512] {
            let input = format!(
                "{}hidden\n[[/iftags]]",
                "[[iftags +missing]]\n".repeat(depth),
            );
            let tokenization = crate::tokenize(&input);
            let mut parser = Parser::new(&tokenization, &page_info, &settings);
            parser.step().expect("the first nested opener should exist");
            assert!(!parser.has_two_body_end_blocks(&BLOCK_IFTAGS));
            assert!(
                parser.block_end_scan_token_visits() <= tokenization.tokens().len(),
                "{depth} nested openers visited {} tokens for {} input tokens",
                parser.block_end_scan_token_visits(),
                tokenization.tokens().len(),
            );
        }
    }

    #[test]
    fn body_end_scan_operation_count_reuses_unmatched_suffixes() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let input = "[[div]]\n".repeat(512);
        let tokenization = crate::tokenize(&input);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);

        while parser.current().token != Token::InputEnd {
            if parser.current().token == Token::LeftBlock {
                assert!(!parser.has_body_end_block(&BLOCK_DIV));
            }
            parser.step().expect("input end must remain available");
        }

        assert!(
            parser.block_end_scan_token_visits() <= tokenization.tokens().len() * 2,
            "visited {} tokens for {} input tokens",
            parser.block_end_scan_token_visits(),
            tokenization.tokens().len(),
        );
    }

    #[test]
    fn segment_scan_operation_count_scales_with_input_tokens() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for repeat in [32, 64, 128, 256, 512] {
            for input in [
                "[[iftags -zzz]]body[[/iftags]] ".repeat(repeat),
                "[[x]] ".repeat(repeat),
            ] {
                let tokenization = crate::tokenize(&input);
                let mut parser = Parser::new(&tokenization, &page_info, &settings);
                crate::parsing::collect::reset_segment_scan_token_visits();

                while parser.current().token != Token::InputEnd {
                    if parser.current().token == Token::LeftBlock {
                        let _ = crate::parsing::collect::iftags_segment(&parser);
                    }
                    parser.step().expect("input end must remain available");
                }

                assert!(
                    crate::parsing::collect::segment_scan_token_visits()
                        <= tokenization.tokens().len() * 4,
                    "{repeat} repeats visited {} tokens for {} input tokens",
                    crate::parsing::collect::segment_scan_token_visits(),
                    tokenization.tokens().len(),
                );
            }
        }
    }

    #[test]
    fn block_end_scan_propagates_cached_suffix_outcomes() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("prefix suffix");

        let mut suffix = Parser::new(&tokenization, &page_info, &settings);
        suffix.step_n(3).expect("suffix token should exist");
        assert_eq!(suffix.current().slice, "suffix");
        assert!(!suffix.has_body_end_block(&BLOCK_DIV));

        let mut prefix = Parser::new(&tokenization, &page_info, &settings);
        prefix.step().expect("prefix token should exist");
        assert_eq!(prefix.current().slice, "prefix");
        assert!(!prefix.has_body_end_block(&BLOCK_DIV));
        assert_eq!(
            prefix.block_end_scan_outcome((
                BLOCK_DIV.name,
                prefix.current().span.start,
                true
            )),
            Some(false),
        );
    }

    #[test]
    fn block_end_scan_cache_keeps_hidden_body_context_separate() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[/div_]]");

        let normal_first = Parser::new(&tokenization, &page_info, &settings);
        assert!(!normal_first.has_body_end_block(&BLOCK_DIV));

        let mut hidden_after_normal = normal_first.clone();
        hidden_after_normal.set_discarding_hidden_body(true);
        assert!(
            hidden_after_normal.has_body_end_block(&BLOCK_DIV),
            "normal-body result poisoned hidden-body close recognition"
        );

        let mut hidden_first = Parser::new(&tokenization, &page_info, &settings);
        hidden_first.set_discarding_hidden_body(true);
        assert!(hidden_first.has_body_end_block(&BLOCK_DIV));

        let mut normal_after_hidden = hidden_first.clone();
        normal_after_hidden.set_discarding_hidden_body(false);
        assert!(
            !normal_after_hidden.has_body_end_block(&BLOCK_DIV),
            "hidden-body result poisoned normal-body close recognition"
        );
    }

    #[test]
    fn block_end_scan_cache_keeps_center_body_context_separate() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[/=]]");

        let normal_first = Parser::new(&tokenization, &page_info, &settings);
        assert!(!normal_first.has_body_end_block(&BLOCK_COLLAPSIBLE));

        let mut centered_after_normal = normal_first.clone();
        centered_after_normal.enter_wikidot_center_body();
        assert!(
            centered_after_normal.has_body_end_block(&BLOCK_COLLAPSIBLE),
            "normal-body result poisoned centered collapsible close recognition"
        );

        let mut centered_first = Parser::new(&tokenization, &page_info, &settings);
        centered_first.enter_wikidot_center_body();
        assert!(centered_first.has_body_end_block(&BLOCK_COLLAPSIBLE));

        let mut normal_after_centered = centered_first.clone();
        normal_after_centered.leave_wikidot_center_body();
        assert!(
            !normal_after_centered.has_body_end_block(&BLOCK_COLLAPSIBLE),
            "center-body result poisoned normal collapsible close recognition"
        );
    }

    #[test]
    fn two_block_end_scan_reuses_cached_suffix_outcomes() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input =
            format!("{}hidden\n[[/iftags]]", "[[iftags +missing]]\n".repeat(256),);
        let tokenization = crate::tokenize(&input);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().expect("the first nested opener should exist");

        assert!(!parser.has_two_body_end_blocks(&BLOCK_IFTAGS));
        let first_scan_visits = parser.block_end_scan_token_visits();
        assert!(first_scan_visits > 0);
        assert!(first_scan_visits <= tokenization.tokens().len());

        while parser.current().token != Token::InputEnd {
            assert!(!parser.has_two_body_end_blocks(&BLOCK_IFTAGS));
            parser.step().expect("input end must remain available");
        }
        assert!(parser.block_end_scan_token_visits() <= tokenization.tokens().len() * 2);
    }

    #[test]
    fn native_quote_prefix_and_close_scan_cover_modes_boundaries_and_depths() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for input in [">> body", "> > body"] {
            let tokenization = crate::tokenize(input);
            let mut parser = Parser::new(&tokenization, &page_info, &settings);
            parser.step().unwrap();
            let (content_start, depth, after_prefix) =
                parser.scan_absolute_quote_prefix(2).unwrap();
            assert_eq!(depth, 2);
            assert_eq!(&input[content_start..], "body");
            assert_eq!(after_prefix.current().slice, "body");
        }

        let tokenization = crate::tokenize("> body");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().unwrap();
        assert!(parser.scan_absolute_quote_prefix(2).is_none());

        for (input, inline, expected) in [
            ("> before [[/collapsible]] trailing\n", true, true),
            ("> before [[/collapsible]] trailing\n", false, false),
            ("> [[/collapsible]]\n", false, true),
            ("> body", false, false),
            ("> body\n\n", false, false),
            (">> [[/collapsible]]\n> [[/collapsible]]\n", false, true),
            ("plain", false, false),
        ] {
            let tokenization = crate::tokenize(input);
            let mut parser = Parser::new(&tokenization, &page_info, &settings);
            parser.step().unwrap();
            assert_eq!(
                parser.has_native_blockquote_body_end_with_mode(
                    &BLOCK_COLLAPSIBLE,
                    1,
                    inline,
                ),
                expected,
                "{input:?}, inline={inline}",
            );
            assert_eq!(
                parser.has_native_blockquote_body_end_with_mode(
                    &BLOCK_COLLAPSIBLE,
                    1,
                    inline,
                ),
                expected,
                "cached {input:?}, inline={inline}",
            );
        }
    }

    #[test]
    fn restricted_quote_close_rejects_trailing_content_and_no_paragraph_boundary() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("> [[/collapsible]] trailing");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().unwrap();
        parser.install_quote_body_cursor(1);
        assert_eq!(
            parser.prepare_quote_body_line().unwrap(),
            QuoteBodyLineStatus::Prepared
        );
        assert!(
            parser
                .verify_end_block(true, &BLOCK_COLLAPSIBLE, true, false)
                .is_none()
        );

        let tokenization = crate::tokenize("plain");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().unwrap();
        parser.install_quote_body_cursor(1);
        let error = parser
            .get_body_elements_no_paragraphs(&BLOCK_DIV, true, false)
            .expect_err("unquoted content must end an adapted body");
        assert_eq!(error.kind(), ParseErrorKind::EndOfInput);
    }

    #[test]
    fn block_body_generic_accepts_matching_end_block() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[/div]]");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().expect("end block should follow input start");

        let (start, end) = parser
            .get_body_generic(&BLOCK_DIV, |_| Ok(()))
            .expect("matching end block should terminate the body");

        assert_eq!(start.slice, "[[/");
        assert_eq!(end.slice, "[[/");
    }

    fn head_word(
        input: &str,
        layout: Layout,
    ) -> Result<(String, bool, bool), ParseErrorKind> {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
        let tokenization = crate::tokenize(input);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().map_err(|error| error.kind())?;
        parser
            .get_optional_token(Token::LeftBlock)
            .map_err(|error| error.kind())?;
        parser.get_optional_space().map_err(|error| error.kind())?;
        parser
            .get_head_word(ParseErrorKind::BlockMissingName)
            .map(|(name, in_head)| {
                let owned = matches!(name, Cow::Owned(_));
                (name.into_owned(), owned, in_head)
            })
            .map_err(|error| error.kind())
    }

    fn head_word_raw(
        input: &str,
        layout: Layout,
    ) -> Result<(String, bool), ParseErrorKind> {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
        let tokenization = crate::tokenize(input);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().map_err(|error| error.kind())?;
        parser
            .get_optional_token(Token::LeftBlock)
            .map_err(|error| error.kind())?;
        parser.get_optional_space().map_err(|error| error.kind())?;
        parser
            .get_head_word_raw(ParseErrorKind::BlockMissingName)
            .map(|(name, in_head)| (name.to_owned(), in_head))
            .map_err(|error| error.kind())
    }

    #[test]
    fn head_word_without_segment_stays_borrowed() {
        for layout in [Layout::Wikidot, Layout::Wikijump] {
            assert_eq!(
                head_word("[[footnote]]", layout),
                Ok(("footnote".to_owned(), false, false)),
            );
            assert_eq!(
                head_word("[[span class=\"c\"]]", layout),
                Ok(("span".to_owned(), false, true)),
            );
            assert_eq!(
                head_word_raw("[[footnote]]", layout),
                Ok(("footnote".to_owned(), false)),
            );
            assert_eq!(
                head_word_raw("[[span class=\"c\"]]", layout),
                Ok(("span".to_owned(), true)),
            );
        }
    }

    #[test]
    fn head_word_resolves_iftags_segments() {
        assert_eq!(
            head_word("[[foot[[iftags -zzz]]x[[/iftags]]note]]", Layout::Wikidot),
            Ok(("footxnote".to_owned(), true, false)),
        );
        assert_eq!(
            head_word("[[foot[[iftags +zzz]]x[[/iftags]]note]]", Layout::Wikidot),
            Ok(("footnote".to_owned(), true, false)),
        );
    }

    #[test]
    fn head_word_rejects_kept_body_with_whitespace() {
        assert_eq!(
            head_word("[[foot[[iftags -zzz]]a b[[/iftags]]note]]", Layout::Wikidot),
            Err(ParseErrorKind::RuleFailed),
        );
    }

    #[test]
    fn head_word_fused_segment_close_ends_the_head() {
        assert_eq!(
            head_word("[[foot[[iftags -zzz]]note[[/iftags]]]]", Layout::Wikidot),
            Ok(("footnote".to_owned(), true, false)),
        );
    }

    /// Fixed hand-picked bodies plus deterministic random gate streams, used
    /// by the differential pairing test.
    fn pairing_sources() -> Vec<String> {
        let mut sources: Vec<String> = [
            "",
            "plain",
            "[[iftags -x]]shown[[/iftags]]",
            "[[iftags +x]]hidden[[/iftags]]",
            "[[iftags -x]]a[[iftags -y]]b[[/iftags]]c[[/iftags]]",
            "[[iftags -x]]a[[iftags -y]]b[[/iftags]]c",
            "[[iftags -x]]a[[iftags -y]]b",
            "a[[iftags +x]]b[[/iftags]]c [[iftags +y]][[/iftags]] ",
            "[[iftags +x]]\n[[ifcategory y]]\n[[/iftags]]\n[[/ifcategory]]\n",
            "[[iftags -x]][[/iftags]][[/iftags]]",
            "[[iftags -x]][[code]]x[[/code]][[/iftags]]",
            "[[iftags -x]][[code]]x[[/code]]",
            "[[iftags -x]][[code]]x\n[[/iftags]]",
            "[[iftags -x]][[html]]x[[/html]][[/iftags]]",
            "[[iftags -x]][[module]]x[[/module]][[/iftags]]",
            "[[iftags -x]]@@raw@@[[/iftags]]",
            "[[iftags -x]]@<raw@>[[/iftags]]",
            "[[iftags -x]][!-- [[/iftags]] --][[/iftags]]",
            "[[/iftags]][[iftags -x]][[/iftags]]",
            "[[iftags -x]][[/ iftags ]][[iftags -y]]",
            "[[iftags -x]][[/iftags]]]",
            "[[iftags -x]][[iftags -y]][[/iftags]][[/iftags]][[/iftags]]",
            "[[iftags -x]]\n[[module]]x[[/module]]\n[[/iftags]]",
            "[[iftags -x]]\n[[html]]x\n[[/html]]\n[[/iftags]]",
            "[[iftags -x]][[div]]x[[/div]][[/iftags]]",
            "[[iftags -x]][[iftags]]x[[/iftags]][[/iftags]]",
            "[[iftags -x]][[iftagsx]]x[[/iftagsx]][[/iftags]]",
            "[[iftags -x]][[ iftags -y]]x[[/iftags]][[/iftags]]",
            "[[iftags -x]][[iftags -y]][[iftags -z]][[/iftags]][[/iftags]][[/iftags]]",
            "\n[[iftags -x]]\nshown\n[[/iftags]]\n",
            "[[iftags -zz-no-such-tag]][[iftags @@]][[/iftags]]",
            "[[iftags -zz-no-such-tag]][[iftags @@]][[/iftags]]@@",
            "[[iftags @@]][[/iftags]]@@",
            "[[iftags -x]][[iftags @<a]][[/iftags]]@>",
            "[[iftags -x]][[iftags @<a]][[/iftags]]",
            "[[iftags @@]]x[[/iftags]]@@y[[/iftags]]",
            "[[iftags -zzz]]A[[iftags -zzz]]B[[/iftags]]C[[/iftags]]",
            "[[iftags -zzz]]\nouter-visible[[iftags +zzz]]\nhidden\n[[/iftags]]\n[[/iftags]]\n",
            "[[iftags -zzz]]A[[iftags +zzz]]B\n\nC[[/iftags]]D[[/iftags]]",
            "[[math]]\na[[iftags -zzz]]b[[iftags -zzz]]c\n[[/math]]\nd[[/iftags]]e[[/iftags]]f",
            "[[math]]\na[[iftags -zzz]]b[[iftags +zzz]]c[[/iftags]]d[[/iftags]]e\n[[/math]]\nf",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();

        let vocabulary = [
            "[[iftags -x]]",
            "[[iftags +y]]",
            "[[iftags]]",
            "[[/iftags]]",
            "[[/ iftags ]]",
            "[[/iftags]]]",
            "text",
            "\n",
            " ",
            "[[code]]",
            "[[/code]]",
            "[[html]]",
            "[[/html]]",
            "[[module]]",
            "[[/module]]",
            "[[div]]",
            "[[/div]]",
            "@@raw@@",
            "@<x@>",
            "[!--c--]",
            "]]",
            "[[",
        ];
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        for _ in 0..600 {
            let mut next = || {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state
            };
            let length = 1 + (next() % 9) as usize;
            let mut source = String::new();
            for _ in 0..length {
                let pick = (next() % vocabulary.len() as u64) as usize;
                source.push_str(vocabulary[pick]);
            }
            sources.push(source);
        }
        sources
    }

    #[test]
    fn pairing_scan_matches_reference_dynamic_program() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        for source in pairing_sources() {
            let tokenization = crate::tokenize(&source);
            let mut parser = Parser::new(&tokenization, &page_info, &settings);
            loop {
                let scan = parser.wikidot_iftags_pairing_end();
                let reference = parser.wikidot_iftags_pairing_end_reference();
                assert_eq!(
                    scan,
                    reference,
                    "{source:?} at {:?}",
                    parser.current().span.start,
                );
                if parser.current().token == Token::InputEnd || parser.step().is_err() {
                    break;
                }
            }
        }
    }

    #[test]
    fn pairing_scan_matches_reference_for_crossing_protected_spans() {
        // A raw span that opens inside an [[iftags]] head and closes after that
        // head makes the tokens reachable from inside the head differ from the
        // ones reachable from the body start. Those bodies must fall back to
        // the reference program and stay identical, at every position.
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut sources = Vec::new();
        for opener in ["@@", "@<a"] {
            let closer = if opener == "@@" { "@@" } else { "@>" };
            for head in [
                format!("[[iftags -x]]{opener}[[iftags -y]]"),
                format!("[[iftags {opener}]]"),
                format!("[[iftags -x{opener}]]"),
                format!("[[iftags -x]]a{opener}[[iftags -y]]b"),
            ] {
                for middle in ["[[/iftags]]", "[[/iftags]]x", "x[[/iftags]]", ""] {
                    for tail in [closer.to_owned(), format!("{closer}[[/iftags]]")] {
                        sources.push(format!("{head}{middle}{tail}"));
                    }
                }
            }
        }
        assert!(sources.len() >= 32);
        for source in sources {
            let tokenization = crate::tokenize(&source);
            let mut parser = Parser::new(&tokenization, &page_info, &settings);
            loop {
                assert_eq!(
                    parser.wikidot_iftags_pairing_end(),
                    parser.wikidot_iftags_pairing_end_reference(),
                    "{source:?} at {:?}",
                    parser.current().span.start,
                );
                if parser.current().token == Token::InputEnd || parser.step().is_err() {
                    break;
                }
            }
        }
    }

    #[test]
    fn pairing_scan_work_is_linear_in_gate_count() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut previous = None;
        for count in [64_usize, 256, 1024] {
            let input = "[[iftags -x]]\nshown\n[[/iftags]]\n".repeat(count);
            let tokenization = crate::tokenize(&input);
            reset_iftags_pairing_scan_steps();
            let (_, errors): (_, Vec<ParseError>) =
                crate::parse(&tokenization, &page_info, &settings).into();
            assert!(errors.is_empty(), "{count}: {errors:?}");
            let steps = iftags_pairing_scan_steps();
            assert!(
                steps <= input.len() * 2,
                "{count} gates took {steps} scan steps for {} bytes",
                input.len(),
            );
            if let Some(previous) = previous {
                assert!(
                    steps <= previous * 6,
                    "{count} gates took {steps} scan steps after {previous}",
                );
            }
            previous = Some(steps);
        }
    }

    /// The pairing result for a body that starts at byte offset.
    fn pairing_at(source: &str, offset: usize) -> (Option<usize>, bool) {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        while parser.current().span.start < offset {
            parser.step().expect("body start must be inside the source");
        }
        assert_eq!(parser.current().span.start, offset, "{source:?}");
        let result = parser.wikidot_iftags_pairing_end();
        assert_eq!(
            result,
            parser.wikidot_iftags_pairing_end_reference(),
            "{source:?}",
        );
        result
    }

    #[test]
    fn iftags_pairing_designates_the_balanced_end_head() {
        // A nested gate consumes its own end head; the outer body pairs with
        // the next balanced one.
        let source = "[[iftags -x]]a[[iftags -y]]b[[/iftags]]c[[/iftags]]";
        let body = source.find("a[[iftags -y]]").unwrap();
        assert_eq!(
            pairing_at(source, body),
            (Some(source.rfind("[[/iftags]]").unwrap()), true),
        );

        // Two gate levels need two end heads.
        let source =
            "[[iftags -x]]a[[iftags -y]]b[[iftags -z]]c[[/iftags]][[/iftags]][[/iftags]]";
        let body = source.find("a[[iftags -y]]").unwrap();
        assert_eq!(
            pairing_at(source, body),
            (Some(source.rfind("[[/iftags]]").unwrap()), true),
        );

        // An unclosed nested gate leaves the body with the last end head it
        // can reach (gate transparency).
        let source = "[[iftags -x]]a[[iftags -y]]b[[/iftags]]c";
        let body = source.find("a[[iftags -y]]").unwrap();
        assert_eq!(
            pairing_at(source, body),
            (Some(source.rfind("[[/iftags]]").unwrap()), true),
        );

        // A body with no reachable end head has no designated end.
        let source = "[[iftags -x]]a[[iftags -y]]b";
        let body = source.find("a[[iftags -y]]").unwrap();
        assert_eq!(pairing_at(source, body), (None, false));

        // The body of a closed gate is not marked nested.
        let source = "[[iftags -x]]a[[/iftags]]";
        let body = source.find("a[[/iftags]]").unwrap();
        assert_eq!(
            pairing_at(source, body),
            (Some(source.find("[[/iftags]]").unwrap()), false),
        );
    }
}
