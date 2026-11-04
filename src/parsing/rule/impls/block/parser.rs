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
use crate::parsing::collect::{
    CommentElidedText, collect_comment_elided_keep, collect_text, collect_text_keep,
    collect_text_resolved_fused, consume_iftags_segment, consume_valid_comment,
};
use crate::parsing::condition::ParseCondition;
use crate::parsing::consume::consume;
use crate::parsing::parser::{QuoteBodyLineStatus, QuoteScanOutcome};
use crate::parsing::{
    ExtractedToken, ParseError, ParseErrorKind, ParseResult, Parser, Token,
    gather_paragraphs,
};
use crate::tree::{AttributeMap, Container, ContainerType, Element, Elements};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscardedBodyOutcome {
    Discarded,
    FailedClosedThroughInputEnd,
}

/// Token index just past a `[[name ...]]` block head starting at `start`,
/// or `None` if the tokens at `start` are not such a head. Used by the
/// Wikidot iftags pairing search (#792), which classifies heads
/// syntactically over the token stream.
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

/// Token index just past a `[[/name]]` end head starting at `start`.
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

    // A fused close (`[[/iftags]]]`) lexes its three brackets as one
    // RightLink token. Live Wikidot's pairing scan is text-level and
    // matches the `[[/iftags]]` prefix, closing the gate and leaving
    // the extra bracket as following text, so the walk designates the
    // fused head as an end as well.
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

/// Token index just past a region the Wikidot iftags pairing walk cannot
/// see into, starting at `start`: a raw-bodied block span (`[[code]]`,
/// `[[html]]`, `[[raw]]`, `[[math]]`, `[[module]]`, through the first end
/// head of the same name) or a raw span (`@@...@@`, `@<...>@`). `None` when
/// no such region starts here (including an unterminated block or `@@`,
/// which are literal text, not regions).
fn protected_span_end(tokens: &[&ExtractedToken<'_>], start: usize) -> Option<usize> {
    match tokens.get(start)?.token {
        Token::Raw => {
            let mut i = start + 1;
            while i < tokens.len() {
                if tokens[i].token == Token::Raw {
                    return Some(i + 1);
                }
                i += 1;
            }
            None
        }
        Token::LeftRaw => {
            let mut i = start + 1;
            while i < tokens.len() {
                if tokens[i].token == Token::RightRaw {
                    return Some(i + 1);
                }
                i += 1;
            }
            None
        }
        Token::LeftBlock => raw_block_span_end(tokens, start),
        _ => None,
    }
}

// Blocks whose bodies are raw text for the pairing scan: an `[[/iftags]]`
// inside one is body content, never a pairing candidate. Live Wikidot
// steps over code blocks (probe p13b); the rest of the set matches the
// opacity FTML's hidden-body security behavior has always given these
// constructs, so their end markers stay hidden inside a false gate.
const IFTAGS_OPAQUE_BLOCKS: [&str; 5] = ["code", "html", "math", "module", "raw"];

/// If a raw-bodied block head starts at `start`, the token index just past
/// its first end head. An unterminated block is not a span.
fn raw_block_span_end(tokens: &[&ExtractedToken<'_>], start: usize) -> Option<usize> {
    if tokens.get(start)?.token != Token::LeftBlock {
        return None;
    }
    let mut i = start + 1;
    if tokens.get(i)?.token == Token::Whitespace {
        i += 1;
    }
    let ident = tokens.get(i)?;
    if ident.token != Token::Identifier
        || !IFTAGS_OPAQUE_BLOCKS
            .iter()
            .any(|name| ident.slice.eq_ignore_ascii_case(name))
    {
        return None;
    }
    let name = ident.slice;
    i += 1;
    loop {
        match tokens.get(i)?.token {
            Token::RightBlock => {
                i += 1;
                break;
            }
            Token::LeftBlock
            | Token::LeftBlockEnd
            | Token::InputEnd
            | Token::ParagraphBreak => {
                return None;
            }
            _ => i += 1,
        }
    }
    while i < tokens.len() {
        if let Some(end) = named_block_end_head_end(tokens, i, name) {
            return Some(end);
        }
        i += 1;
    }
    None
}

/// Whether a byte offset sits inside a `[!-- --]` comment, judging by
/// the nearest comment opener and closer before it in the source.
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

fn block_rule_accepts_end_name(
    block_rule: &BlockRule,
    name: &str,
    wikidot: bool,
) -> bool {
    block_rule_end_names(block_rule, wikidot)
        .iter()
        .any(|accepted| name.eq_ignore_ascii_case(accepted))
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

fn wikidot_trim_argument_fragment(value: &str) -> &str {
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

pub(crate) fn parse_wikidot_attributes(value: &str) -> Arguments<'_> {
    let field = CommentElidedText::new(value, 0..value.len(), Vec::new());
    parse_wikidot_attribute_field(&field)
}

fn parse_wikidot_attribute_field<'t>(field: &CommentElidedText<'t>) -> Arguments<'t> {
    // Wikidot's getAttrs grammar splits only on the exact ASCII delimiter
    // `="`, then treats the last quote before the next delimiter as the
    // current value terminator. Valid comments are absent for value parsing,
    // but a comment cannot manufacture a new key or `="` delimiter.
    let source = field.source();
    let span = field.span();
    let mut comment_mask = vec![false; source.len()];
    for comment in field.comment_ranges() {
        let start = comment.start.saturating_sub(span.start).min(source.len());
        let end = comment.end.saturating_sub(span.start).min(source.len());
        comment_mask[start..end].fill(true);
    }
    // Resolved iftags segments hide `="` delimiters just like comments,
    // but they do not reset an attribute key the way a comment does, so
    // they get their own mask for the delimiter search.
    let mut elision_mask = comment_mask.clone();
    for segment in field.segment_ranges() {
        let start = segment.start.saturating_sub(span.start).min(source.len());
        let end = segment.end.saturating_sub(span.start).min(source.len());
        elision_mask[start..end].fill(true);
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
        let value_range = span.start + value_range.start..span.start + value_range.end;
        let value = field.elide_range(value_range);
        if key.is_empty() {
            arguments.mark_empty_key_present();
        } else {
            arguments.insert_cow(key, wikidot_stripslashes(value));
        }
        key = wikidot_attribute_key(field, &comment_mask, next_key_range);
    }
    if key.starts_with('=') {
        arguments.mark_empty_key_present();
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
    // The fragment is addressed in field-relative offsets; elision works
    // in absolute source offsets. A resolved iftags segment inside the
    // key joins the parts around it (live Wikidot resolves the segment
    // before getAttrs runs), rather than dropping the part before it.
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

/// Whether an element is blank content that a paragraph break absorbs
/// at a segment edge: a line break, or text holding only spaces and tabs.
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

/// Distributes the elements of a nested block result that itself split
/// at paragraph breaks into the segment list being collected: the first
/// segment's elements join the current segment, and each subsequent
/// segment starts a new segment here, so a nested split splits its
/// ancestors' bodies at the same breaks. Falls back to appending every
/// element to the current segment if the counts do not match the
/// element batch (defensive; well-formed batches always match).
fn distribute_paragraph_split<'t>(
    segments: &mut Vec<Vec<Element<'t>>>,
    elements: Vec<Element<'t>>,
    counts: &[usize],
) {
    if counts.iter().sum::<usize>() != elements.len() {
        segments
            .last_mut()
            .expect("segments always has a current segment")
            .extend(elements);
        return;
    }

    let mut elements = elements.into_iter();
    for (index, count) in counts.iter().enumerate() {
        if index > 0 {
            segments.push(Vec::new());
        }
        segments
            .last_mut()
            .expect("segments always has a current segment")
            .extend(elements.by_ref().take(*count));
    }
}

impl<'r, 't> Parser<'r, 't>
where
    'r: 't,
{
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

    pub fn get_block_name(
        &mut self,
        flag_star: bool,
    ) -> Result<(&'t str, bool), ParseError> {
        if flag_star {
            self.get_optional_token(Token::LeftBlockStar)?;
        } else {
            self.get_optional_token(Token::LeftBlock)?;
        }

        self.get_optional_space()?;

        self.get_head_word_raw(ParseErrorKind::BlockMissingName)
    }

    fn get_head_word_raw(
        &mut self,
        kind: ParseErrorKind,
    ) -> Result<(&'t str, bool), ParseError> {
        let end_conditions = [
            ParseCondition::current(Token::Whitespace),
            ParseCondition::current(Token::LineBreak),
            ParseCondition::current(Token::ParagraphBreak),
            ParseCondition::current(Token::RightBlock),
        ];
        let rule = self.rule();
        let stops = &end_conditions;
        collect_text_keep(self, rule, stops, &[], Some(kind)).map(|(name, last)| {
            let name = if self.settings().layout.legacy() {
                name.trim_matches([' ', '\t', '\r', '\n'])
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
        let end_conditions = [
            ParseCondition::current(Token::Whitespace),
            ParseCondition::current(Token::LineBreak),
            ParseCondition::current(Token::ParagraphBreak),
            ParseCondition::current(Token::RightBlock),
        ];
        let rule = self.rule();
        let (name, last) =
            collect_text_resolved_fused(self, rule, &end_conditions, &[], Some(kind))?;
        let legacy = self.settings().layout.legacy();
        fn trim_head_word(name: &str, legacy: bool) -> &str {
            if legacy {
                name.trim_matches([' ', '\t', '\r', '\n'])
            } else {
                name.trim()
            }
        }
        let name = match name {
            Cow::Borrowed(name) => Cow::Borrowed(trim_head_word(name, legacy)),
            Cow::Owned(name) => Cow::Owned(trim_head_word(&name, legacy).to_owned()),
        };
        if name.contains(char::is_whitespace) || name.contains("]]") {
            return Err(self.make_err(ParseErrorKind::RuleFailed));
        }
        let in_head = !matches!(last.token, Token::RightBlock | Token::RightBracket);

        Ok((name, in_head))
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
            Ok((name.trim_matches([' ', '\t', '\r', '\n']), false))
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
        Ok((
            name.trim_matches([' ', '\t', '\r', '\n']),
            residual_close_bracket,
        ))
    }

    pub fn get_end_block(&mut self) -> Result<&'t str, ParseError> {
        self.get_token(Token::LeftBlockEnd, ParseErrorKind::BlockExpectedEnd)?;
        self.get_optional_space()?;

        let (name, in_head) = self.get_block_name(false)?;
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
        let name = name.trim_matches([' ', '\t', '\r', '\n']);

        let base_name = name.strip_suffix('_').unwrap_or(name);
        if last.token == Token::RightLink
            && block_rule_accepts_end_name(block_rule, base_name, true)
        {
            Ok(name)
        } else {
            Err(self.make_err(ParseErrorKind::BlockExpectedEnd))
        }
    }

    /// Whether the current token is trailing whitespace that ends its
    /// physical line (only spaces/tabs before the line break or input
    /// end). Wikidot accepts a code, math or bibliography closer followed
    /// by such a blank line remainder (scp-9844's `[[/code]] ` closers render
    /// and are hosted; scp-099-fr's `[[/math]] ` renders an equation;
    /// scp-7451's `[[/bibliography]]  ` renders its bibitems).
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

    fn verify_end_block(
        &mut self,
        first_iteration: bool,
        block_rule: &BlockRule,
        restrict_quote_close: bool,
        allow_inline_quote_close: bool,
    ) -> Option<&'r ExtractedToken<'t>> {
        // IfTags pairing (#792): while a matched iftags body is gathered,
        // only the end block the pairing search designated may close it.
        // Any other `[[/iftags]]` in the span is literal text. The
        // designated end is only ever set under the pairing model, which
        // both layouts now use, so no layout check is needed here.
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

            // Check that the end block is on a new line, if required
            //
            // The legacy math line-ownership requirements model how Wikidot
            // merges consecutive top-level equations. Footnote bodies are
            // gathered as their own element stream, and Wikidot closes a
            // footnote's math at its first [[/math]] even mid-line (e.g.
            // scp-7079's table-header footnote), so they do not apply there.
            if block_rule.accepts_newlines_outside_body {
                if !first_iteration
                    && parser.settings().layout.legacy()
                    && !parser.discarding_hidden_body()
                    && block_rule.name == "block-math"
                    && !parser.in_footnote()
                {
                    parser
                        .get_token(Token::LineBreak, ParseErrorKind::BlockExpectedEnd)?;
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
                if !follows_only_literal_quotes
                    && (!allow_inline_quote_close
                        || !matches!(
                            parser.current().token,
                            Token::Quote | Token::LeftBlockEnd
                        ))
                {
                    return Ok(false);
                }
                while parser.current().token == Token::Quote {
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
            // IfTags takes the fused path in either layout: its
            // pairing model (which designates fused closes as ends)
            // is layout-independent, unlike the legacy-only blocks
            // below whose fused acceptance live Wikidot grants per
            // block type.
            let fused_close = (parser.settings().layout.legacy()
                || block_rule.name == "block-iftags")
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
                parser.get_wikidot_fused_end_block(block_rule)?
            } else {
                parser.get_end_block()?
            };

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
            {
                let end = parser.current().span.start;
                let source = &parser.full_text().inner()[end_start..end];
                let expected = format!("[[/{name}]]");
                if !source.eq_ignore_ascii_case(&expected) {
                    return Ok(false);
                }
            }

            let score_close = name.ends_with('_');
            let name = name.strip_suffix('_').unwrap_or(name);
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
                    if parser.settings().layout.legacy()
                        && !parser.discarding_hidden_body()
                        && matches!(
                            block_rule.name,
                            "block-code" | "block-math" | "block-bibliography"
                        )
                        && !(block_rule.name == "block-math" && parser.in_footnote())
                        && !(matches!(
                            parser.current().token,
                            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
                        ) || parser.at_blank_line_remainder())
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
        let name = raw_name.strip_suffix('_').unwrap_or(raw_name);
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

        if stepped_line_break && self.step().is_err() {
            return false;
        }
        self.record_wikidot_crossed_close(block_rule.name);
        true
    }

    pub(crate) fn has_wikidot_list_close_ahead(&self) -> bool {
        let mut probe = self.clone();
        loop {
            match probe.current().token {
                Token::InputEnd => return false,
                Token::LeftBlockEnd => {
                    let mut end = probe.clone();
                    if let Ok(name) = end.get_end_block() {
                        let name = name.strip_suffix('_').unwrap_or(name);
                        if name.eq_ignore_ascii_case("ul")
                            || name.eq_ignore_ascii_case("ol")
                        {
                            return true;
                        }
                    }
                }
                _ => {}
            }
            if probe.step().is_err() {
                return false;
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

        loop {
            let before_end = self.clone();
            let crossed_math_close =
                wikidot_math && self.at_wikidot_crossed_math_close(first);
            let at_end_block = self.verify_end_block(first, block_rule, false, false);

            if let Some(end) = at_end_block {
                if nested_wikidot_math > 0 {
                    nested_wikidot_math -= 1;
                    last_nested_close = Some((end, self.clone()));
                    self.update(&before_end);
                } else {
                    return Ok((start, end));
                }
            } else if crossed_math_close {
                suppress_nested_wikidot_math = true;
            } else if wikidot_math
                && !suppress_nested_wikidot_math
                && self.at_wikidot_nested_math_opener()
            {
                nested_wikidot_math += 1;
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
                    let slice = self.full_text().slice_partial(start, end);
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
                    let name = name.strip_suffix('_').unwrap_or(name);
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
    ) -> ParseResult<'r, 't, Vec<Element<'t>>> {
        self.get_body_elements_with_context_policy(
            block_rule,
            as_paragraphs,
            body_start,
            false,
        )
    }

    pub(crate) fn get_body_elements_with_literal_quote_context(
        &mut self,
        block_rule: &BlockRule,
        as_paragraphs: bool,
        body_start: BlockBodyStart,
    ) -> ParseResult<'r, 't, Vec<Element<'t>>> {
        self.get_body_elements_with_context_policy(
            block_rule,
            as_paragraphs,
            body_start,
            true,
        )
    }

    fn get_body_elements_with_context_policy(
        &mut self,
        block_rule: &BlockRule,
        as_paragraphs: bool,
        body_start: BlockBodyStart,
        literal_residual_quotes: bool,
    ) -> ParseResult<'r, 't, Vec<Element<'t>>> {
        if self.settings().layout.legacy()
            && !self.discarding_hidden_body()
            && body_start == BlockBodyStart::Inline
            && wikidot_requires_next_physical_line(block_rule)
        {
            return Err(self.make_err(ParseErrorKind::RuleFailed));
        }

        let Some(required_depth) = self.native_blockquote_depth() else {
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
        let has_close = self.has_native_blockquote_body_end_with_mode(
            block_rule,
            required_depth,
            allow_inline_quote_close,
        );
        let has_later_close =
            allow_inline_quote_close && !has_close && self.has_body_end_block(block_rule);
        if !has_close && !has_later_close {
            return Err(self.make_err(ParseErrorKind::RuleFailed));
        }

        let previous_cursor = self.quote_body_cursor();
        if literal_residual_quotes {
            self.install_quote_body_cursor_with_literal_residuals(required_depth);
        } else {
            self.install_quote_body_cursor(required_depth);
        }
        let previous_boundary_policy = self.quote_boundary_closes_body();
        self.set_quote_boundary_closes_body(has_later_close);
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
                if self.current().token == Token::LineBreak {
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

        let discard_result =
            fork.consume_body_no_paragraphs(block_rule, false, false, |_| {});
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
        let rule = self.rule();

        let is_end = move |parser: &mut Parser<'r, 't>| {
            if wikidot_tab
                && let Some(opener) = parser.wikidot_nested_tab_opener()
                && last_nested_wikidot_tab != Some(opener)
            {
                last_nested_wikidot_tab = Some(opener);
                nested_wikidot_tabs += 1;
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

            if result.is_some() && nested_wikidot_tabs > 0 {
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

    /// Collects a block body like `get_body_elements_no_paragraphs`, but
    /// splits the collected elements into segments at each top-level
    /// paragraph break within the body.
    ///
    /// Inline container blocks consume their whole body in one call, so a
    /// paragraph break inside the body would otherwise be flattened into
    /// a line break element, merging the authored paragraphs on either
    /// side of the block. Splitting the body here lets the caller wrap
    /// each segment in its own container (see `wrap_body_segments`) so the
    /// paragraph gather can re-establish the breaks between them.
    ///
    /// Wikijump layout only: legacy-layout callers must use the unsplit
    /// collection, whose behavior this method does not reproduce.
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

            if !self.discarding_hidden_body() && self.at_ancestor_body_end_boundary() {
                return Err(self.make_err(ParseErrorKind::BlockExpectedEnd));
            }

            if self.current().token == Token::ParagraphBreak {
                // A paragraph break at the body's top level ends the
                // current segment instead of becoming a line break.
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
                            distribute_paragraph_split(&mut segments, elements, &counts);
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

    /// Collects the body of an iftags gate whose pairing designated an
    /// end, splitting at top-level paragraph breaks like
    /// `get_body_element_segments` but in either layout: live Wikidot
    /// splices a kept gate body into the text before paragraph
    /// formation, so the body's breaks are real paragraph breaks in
    /// both layouts, and the caller re-establishes them through the
    /// split layout on its result. Apart from the split, the gather
    /// mirrors `consume_body_no_paragraphs` (end verification, list
    /// close, hidden-body boundaries, the legacy input-end return), so
    /// a kept body gathers exactly as it did unsplit.
    pub(crate) fn get_body_element_segments_iftags(
        &mut self,
        block_rule: &BlockRule,
    ) -> ParseResult<'r, 't, Vec<Vec<Element<'t>>>> {
        let mut segments: Vec<Vec<Element<'t>>> = vec![Vec::new()];
        let mut all_errors = Vec::new();
        let mut paragraph_safe = true;
        let mut first = true;

        loop {
            if self.prepare_quote_body_line()? == QuoteBodyLineStatus::Boundary {
                return Err(self.make_err(ParseErrorKind::EndOfInput));
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

            // The designated end sat inside a construct the previous
            // iteration consumed whole (a comment holding the closer):
            // the body is complete, with the end absorbed into that
            // construct's elements.
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
                continue;
            }

            first = false;
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
                    match paragraph_split {
                        Some(counts) => {
                            distribute_paragraph_split(&mut segments, elements, &counts);
                        }
                        None => {
                            segments
                                .last_mut()
                                .expect("segments always has a current segment")
                                .extend(elements);
                        }
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
            segments,
            all_errors,
            paragraph_safe,
        ))
    }

    /// Wraps body segments collected by `get_body_element_segments` into
    /// one inline container per non-empty segment, returning the segment
    /// layout (see `ParseSuccess::paragraph_split`) alongside the wrapped
    /// elements so the caller can attach it to its parse result.
    ///
    /// A body that did not cross a paragraph break (a single segment)
    /// produces exactly the container the unsplit collection produced,
    /// including for an empty body, and returns no layout.
    pub(crate) fn wrap_body_segments(
        &mut self,
        segments: Vec<Vec<Element<'t>>>,
        ctype: ContainerType,
        attributes: AttributeMap<'t>,
    ) -> (Elements<'t>, Option<Vec<usize>>) {
        if segments.len() == 1 {
            let elements = segments.into_iter().next().expect("single segment present");
            let container = Container::new(ctype, elements, attributes);
            return (Elements::Single(Element::Container(container)), None);
        }

        let last_index = segments.len() - 1;
        let mut counts = Vec::with_capacity(segments.len());
        let mut containers = Vec::new();
        for (index, mut segment) in segments.into_iter().enumerate() {
            // Content blank at a segment edge adjacent to a paragraph
            // break belongs to the break, not to either segment: just as
            // a paragraph's own edges shed leading and trailing line
            // breaks and whitespace, a segment sheds them where it meets
            // a break. Edges at the body's own start and end keep their
            // content, as before.
            if index > 0 {
                trim_segment_leading_blanks(&mut segment);
            }
            if index < last_index {
                trim_segment_trailing_blanks(&mut segment);
            }

            if segment.is_empty() {
                counts.push(0);
            } else {
                counts.push(1);
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

    /// Collects the body of an inline container block and wraps it,
    /// splitting at paragraph breaks as described on
    /// `get_body_element_segments`. In the legacy layout this is exactly
    /// the unsplit collection and wrap the block rules used before.
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
            self.wrap_body_segments(segments, ctype, attributes);
        let mut success =
            crate::parsing::ParseSuccess::new(elements, errors, paragraph_safe);
        success.paragraph_split = paragraph_split;
        Ok(success)
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
            if self.prepare_quote_body_line()? == QuoteBodyLineStatus::Boundary {
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

            if self.terminate_at_wikidot_list_close(block_rule, first) {
                return Ok(());
            }

            if self.discarding_hidden_body() && self.at_hidden_body_ancestor_boundary() {
                return Err(self.make_err(ParseErrorKind::BlockExpectedEnd));
            }

            // An iftags gather whose designated end sat inside a
            // construct an earlier iteration consumed whole (a comment
            // holding the closer) is complete once the parser has moved
            // past the designated end: the end was absorbed into that
            // construct, and live Wikidot ends the gate there.
            if block_rule.name == "block-iftags"
                && let Some(designated) = self.wikidot_iftags_body_end()
                && self.current().span.start > designated
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
            match consume(self) {
                Ok(consumed) => {
                    self.take_wikidot_mixed_paragraph_ownership();
                    process(consumed);
                }
                // A comment that stopped at the designated iftags end
                // while discarding ends the hidden body there: the
                // next loop iteration verifies and consumes the
                // designated end block, and the comment's remainder
                // past the closer is ordinary following content, as on
                // live Wikidot. The stop is only honored when the
                // designated end is comment-internal; any other
                // construct's error at the same position (an unclosed
                // raw block, above all) still fails the discard closed.
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

    /// Whether the end block at the current position (after the one line
    /// break `verify_end_block` may step over) is the designated
    /// `[[/iftags]]` at byte offset `designated`.
    pub(crate) fn at_designated_iftags_end(&self, designated: usize) -> bool {
        let mut probe = self.clone();
        if probe.current().token == Token::LineBreak && probe.step().is_err() {
            return false;
        }
        probe.current().token == Token::LeftBlockEnd
            && probe.current().span.start == designated
    }

    /// Wikidot `[[iftags]]` pairing search (ftml #792), run from the first
    /// body token of an iftags opener whose head has just been parsed.
    /// Returns the byte offset of the `[[/iftags]]` head that pairs with
    /// this opener (`None` = unpairable, renders literally), plus whether
    /// the paired span contains a nested iftags opener head. The search is
    /// bounded to a window of tokens after the opener.
    pub(crate) fn wikidot_iftags_pairing_end(&self) -> (Option<usize>, bool) {
        const WINDOW: usize = 8192;

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

        let n = tokens.len();
        // ends[i] = token indices of `[[/iftags]]` heads (their LeftBlockEnd
        // token) that a body starting at token i can end at, in the walk's
        // preference order, de-duplicated. All dependencies are at larger
        // indices, so fill the table back to front.
        let mut ends: Vec<Vec<usize>> = vec![Vec::new(); n + 1];
        // De-duplication stamps: seen[end] holds i + 1 while position i's
        // list is being built, so no separate set is needed.
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
            let step_to = protected_span_end(&tokens, i).unwrap_or(i + 1);
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
            i = protected_span_end(&tokens, i).unwrap_or(i + 1);
        }
        (Some(tokens[closer].span.start), nested)
    }

    pub fn has_body_end_block(&self, block_rule: &BlockRule) -> bool {
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

            parser.step().expect("missing input end");
            first = false;
        }
    }

    /// Determine whether a scored Wikidot `div_` or `span_` opener owns a
    /// compatible ordinary closer.
    ///
    /// Wikidot does not normalize underscore closers. A scored opener accepts
    /// the ordinary closer, while its same-spelling underscore closer rejects
    /// that opener. Nested ordinary and scored owners still pair independently,
    /// so this scan records every scored opener it resolves instead of retrying
    /// the recursive block parser for each suffix.
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
                    if !wikidot_alias_close_is_exact(
                        &scan.full_text().inner()[close_start..close_end],
                        name,
                    ) {
                        scan.update(&close);
                        continue;
                    }
                    let scored_close = name.ends_with('_');
                    let base_name = name.strip_suffix('_').unwrap_or(name);
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
                    opener.get_wikidot_block_name_with_residual_opener()
                } else {
                    opener.get_block_name(false)
                };
                if let Ok((name, in_head)) = parsed_name {
                    let scored = name.ends_with('_');
                    let base_name = name.strip_suffix('_').unwrap_or(name);
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
        let mut parser = self.clone();

        loop {
            match parser.current().token {
                Token::InputEnd | Token::LineBreak | Token::ParagraphBreak => {
                    return false;
                }
                Token::LeftBlockEnd => {
                    let mut end = parser.clone();
                    if let Ok(name) = end.get_end_block() {
                        let name = name.strip_suffix('_').unwrap_or(name);
                        if block_rule_accepts_end_name(
                            block_rule,
                            name,
                            self.settings().layout.legacy(),
                        ) {
                            return true;
                        }
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
                let matching_close = end.get_end_block().is_ok_and(|name| block_rule_accepts_end_name(block_rule, name.strip_suffix('_').unwrap_or(name), self.settings().layout.legacy()));
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
                    // An iftags segment inside the key resolves the way
                    // it does inside a value: consume it whole and join
                    // the key parts around its elisions below.
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

    /// Parses a key-value block head using Wikidot's legacy `getAttrs`
    /// grammar in `Layout::Wikidot`, while retaining FTML's strict grammar in
    /// other layouts.
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
            if head_has_comment && !head_has_non_authored_input {
                let closes = [ParseCondition::current(Token::RightBlock)];
                let (head, _) = collect_comment_elided_keep(self, &closes, &[], None)?;
                (parse_wikidot_attribute_field(&head), true)
            } else {
                let start = self.current();
                let mut elisions = Vec::new();
                // A code head is raw on Wikidot: it ends at the first
                // close marker, even when that marker closes an iftags
                // segment opener, and the truncated head text is what
                // getAttrs sees. Segments in a code head are therefore
                // stepped over raw rather than resolved.
                let raw_head = block_rule.name == "block-code";
                while !matches!(self.current().token, Token::RightBlock | Token::InputEnd)
                {
                    // An iftags segment in the head is consumed whole:
                    // its own `]]` must not end the head early, and its
                    // elisions resolve the segment in the parsed values.
                    if !raw_head
                        && self.current().token == Token::LeftBlock
                        && let Some(segment) = consume_iftags_segment(self)?
                    {
                        elisions.extend(segment.elisions());
                        continue;
                    }
                    self.step()?;
                }
                let span = start.span.start..self.current().span.start;
                let head =
                    CommentElidedText::new(self.full_text().inner(), span, Vec::new())
                        .with_segments(elisions);
                (parse_wikidot_attribute_field(&head), false)
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

    pub fn get_head_name_map(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
    ) -> Result<(&'t str, Arguments<'t>), ParseError> {
        self.get_head_name_map_with_body_start(block_rule, in_head)
            .map(|(name, arguments, _)| (name, arguments))
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

    pub(crate) fn get_head_name_map_with_body_start_wikidot(
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
        let head_has_comment = std::iter::once(self.current())
            .chain(self.remaining())
            .take_while(|token| {
                !matches!(token.token, Token::RightBlock | Token::InputEnd)
            })
            .any(|token| token.token == Token::LeftComment);
        if !head_has_comment {
            return self.get_head_name_map_with_body_start(block_rule, in_head);
        }

        let missing_name = ParseErrorKind::ModuleMissingName;
        let (subname, in_head) = self.get_head_word_raw(missing_name)?;
        let (arguments, body_start) =
            self.get_head_map_with_body_start_wikidot(block_rule, in_head)?;
        Ok((subname, arguments, body_start))
    }

    /// Parses a positional block-head value followed by Wikidot `getAttrs`
    /// arguments in `Layout::Wikidot`.
    pub fn get_head_name_map_wikidot(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
    ) -> Result<(&'t str, Arguments<'t>), ParseError> {
        if !self.settings().layout.legacy() {
            return self.get_head_name_map(block_rule, in_head);
        }
        if !in_head {
            return Err(self.make_err(ParseErrorKind::BlockMissingName));
        }

        let missing_name = ParseErrorKind::ModuleMissingName;
        let (subname, in_head) = self.get_head_word_raw(missing_name)?;
        let arguments = self.get_head_map_wikidot(block_rule, in_head)?;

        Ok((subname, arguments))
    }

    /// Parses one already-owned positional field with valid comments elided,
    /// followed by Wikidot `getAttrs` arguments. This does not participate in
    /// block-name recognition, so joined text cannot create a new block owner.
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
        )?;
        // A trailing-edge iftags segment can fuse the head
        // closing into a bracket run whose final token is a
        // single bracket; the head still ended there.
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
        let (field, last) = collect_text_resolved_fused(
            self,
            rule,
            &end_conditions,
            &[],
            Some(ParseErrorKind::BlockMissingName),
        )?;
        let (arguments, _) = self
            .get_head_map_with_body_start(block_rule, last.token != Token::RightBlock)?;
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

    /// Like `get_head_value`, except the head text is collected with
    /// conditional segments resolved, so a segment inside the value
    /// cannot terminate or corrupt it.
    pub fn get_head_value_resolved<F, T>(
        &mut self,
        block_rule: &BlockRule,
        in_head: bool,
        convert: F,
    ) -> Result<T, ParseError>
    where
        F: FnOnce(&Self, Option<Cow<'t, str>>) -> Result<T, ParseError>,
    {
        let argument = if in_head {
            let end_conditions = [ParseCondition::current(Token::RightBlock)];
            let reject_conditions = [
                ParseCondition::current(Token::ParagraphBreak),
                ParseCondition::current(Token::LineBreak),
            ];
            let rule = self.rule();
            let (slice, _) = collect_text_resolved_fused(
                self,
                rule,
                &end_conditions,
                &reject_conditions,
                Some(ParseErrorKind::BlockMalformedArguments),
            )?;

            Some(slice)
        } else {
            None
        };

        let value = convert(self, argument)?;
        self.get_head_block_with_body_start(block_rule, false)?;
        Ok(value)
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
            let end_conditions = [ParseCondition::current(Token::RightBlock)];
            let reject_conditions = [
                ParseCondition::current(Token::ParagraphBreak),
                ParseCondition::current(Token::LineBreak),
            ];
            let rule = self.rule();
            let kind = ParseErrorKind::BlockMalformedArguments;
            let malformed_arguments = Some(kind);
            let stops = &end_conditions;
            let rejects = &reject_conditions;
            let slice = collect_text(self, rule, stops, rejects, malformed_arguments)?;

            Some(slice)
        } else {
            None
        };

        let value = convert(self, argument)?;

        let body_start = self.get_head_block_with_body_start(block_rule, false)?;
        Ok((value, body_start))
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
    fn wikidot_attribute_offsets_preserve_multibyte_malformed_recovery() {
        // The first value intentionally has no terminating quote before the
        // next `="` delimiter. Wikidot drops exactly the first UTF-8 scalar of
        // that malformed segment before recovering the next key.
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
}
