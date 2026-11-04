/*
 * parsing/collect/iftags.rs
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

use super::comment::comment_bridge_end;
use super::prelude::*;
use crate::parsing::ElementCondition;
use std::borrow::Cow;
use std::ops::Range;

#[derive(Debug, Clone)]
pub(crate) struct IftagsSegment {
    pub range: Range<usize>,
    pub head: Range<usize>,
    pub body: Range<usize>,
    pub closer: Range<usize>,
    pub include_body: bool,
    pub fused_closer: bool,
}

impl IftagsSegment {
    pub fn elisions(&self) -> Vec<Range<usize>> {
        if self.include_body {
            vec![self.head.clone(), self.closer.clone()]
        } else {
            vec![self.range.clone()]
        }
    }
}

#[cfg(test)]
thread_local! {
    static SEGMENT_SCAN_TOKEN_VISITS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn segment_scan_token_visits() -> usize {
    SEGMENT_SCAN_TOKEN_VISITS.with(|visits| visits.get())
}

#[cfg(test)]
pub(crate) fn reset_segment_scan_token_visits() {
    SEGMENT_SCAN_TOKEN_VISITS.with(|visits| visits.set(0));
}

#[cfg(test)]
fn add_segment_scan_token_visits(count: usize) {
    SEGMENT_SCAN_TOKEN_VISITS.with(|visits| visits.set(visits.get() + count));
}

#[cfg(test)]
fn increment_segment_scan_token_visits() {
    add_segment_scan_token_visits(1);
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct TokenWindow<'a, 't> {
    first: &'a ExtractedToken<'t>,
    rest: &'a [ExtractedToken<'t>],
}

impl<'a, 't> TokenWindow<'a, 't> {
    pub(crate) fn at<'r>(parser: &Parser<'r, 't>) -> TokenWindow<'r, 't> {
        TokenWindow {
            first: parser.current(),
            rest: parser.remaining(),
        }
    }

    pub(crate) fn first(&self) -> &'a ExtractedToken<'t> {
        self.first
    }

    pub(crate) fn get(&self, index: usize) -> Option<&'a ExtractedToken<'t>> {
        if index == 0 {
            Some(self.first)
        } else {
            self.rest.get(index - 1)
        }
    }

    pub(crate) fn from_slice(
        tokens: &'a [ExtractedToken<'t>],
    ) -> Option<TokenWindow<'a, 't>> {
        let (first, rest) = tokens.split_first()?;
        Some(TokenWindow { first, rest })
    }

    pub(crate) fn offset(&self, index: usize) -> Option<TokenWindow<'a, 't>> {
        if index == 0 {
            return Some(*self);
        }
        let first = self.rest.get(index - 1)?;
        Some(TokenWindow {
            first,
            rest: &self.rest[index..],
        })
    }
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct SegmentCtx<'a, 't> {
    source: &'t str,
    tags: &'a [Cow<'t, str>],
    legacy: bool,
    in_iftags_span: bool,
}

impl<'a, 't> SegmentCtx<'a, 't> {
    pub(crate) fn from_parser<'r>(parser: &'r Parser<'r, 't>) -> SegmentCtx<'r, 't> {
        SegmentCtx {
            source: parser.full_text().inner(),
            tags: &parser.page_info().tags,
            legacy: parser.settings().layout.legacy(),
            in_iftags_span: parser.in_wikidot_iftags_span(),
        }
    }
}

fn head_name(
    window: TokenWindow<'_, '_>,
    start: usize,
    break_at_right_link: bool,
) -> Option<(Range<usize>, usize)> {
    let mut index = start + 1;
    while window.get(index)?.token == Token::Whitespace {
        index += 1;
    }
    let name_start = window.get(index)?.span.start;
    let mut name_end = name_start;
    loop {
        let token = window.get(index)?;
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

pub(crate) fn iftags_segment<'r, 't>(parser: &Parser<'r, 't>) -> Option<IftagsSegment>
where
    'r: 't,
{
    scan_segment(parser, false)
}

pub(crate) fn iftags_segment_fused<'r, 't>(
    parser: &Parser<'r, 't>,
) -> Option<IftagsSegment>
where
    'r: 't,
{
    scan_segment(parser, true)
}

fn scan_segment<'r, 't>(
    parser: &Parser<'r, 't>,
    allow_fused_closer: bool,
) -> Option<IftagsSegment>
where
    'r: 't,
{
    scan_segment_ctx(
        TokenWindow::at(parser),
        SegmentCtx::from_parser(parser),
        allow_fused_closer,
    )
}

fn scan_segment_ctx<'a, 't>(
    window: TokenWindow<'a, 't>,
    ctx: SegmentCtx<'_, 't>,
    allow_fused_closer: bool,
) -> Option<IftagsSegment> {
    if ctx.legacy && ctx.in_iftags_span {
        return None;
    }

    if window.first().token != Token::LeftBlock {
        return None;
    }
    let source = ctx.source;
    let segment_start = window.first().span.start;

    let (name_range, mut index) = head_name(window, 0, false)?;
    #[cfg(test)]
    add_segment_scan_token_visits(index);
    if !source[name_range].eq_ignore_ascii_case("iftags") {
        return None;
    }

    let spec_start = window.get(index)?.span.start;
    let head_end;
    let spec_end;
    loop {
        #[cfg(test)]
        increment_segment_scan_token_visits();
        let token = window.get(index)?;
        match token.token {
            Token::RightBlock => {
                spec_end = token.span.start;
                head_end = token.span.end;
                index += 1;
                break;
            }
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => return None,
            _ => index += 1,
        }
    }
    let spec = source[spec_start..spec_end].trim();

    let legacy = ctx.legacy;
    let conditions = if legacy && spec == "-" {
        Vec::new()
    } else if spec.is_empty() {
        if legacy {
            Vec::new()
        } else {
            return None;
        }
    } else {
        ElementCondition::parse(spec)
    };
    let include_body = if conditions.is_empty() && legacy {
        !ctx.tags.is_empty()
    } else {
        ElementCondition::check(&conditions, ctx.tags)
    };

    let body_start = window.get(index).map_or(head_end, |token| token.span.start);
    let mut depth = 1usize;
    let mut fused_closer = false;
    let (body_end, closer_end) = loop {
        #[cfg(test)]
        increment_segment_scan_token_visits();
        let token = window.get(index)?;
        match token.token {
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => return None,
            Token::LeftBlock => {
                if let Some((name_range, _after_name)) =
                    head_name(window, index, allow_fused_closer)
                {
                    #[cfg(test)]
                    add_segment_scan_token_visits(_after_name - index);
                    if source[name_range].eq_ignore_ascii_case("iftags") {
                        depth += 1;
                    }
                }
            }
            Token::LeftBlockEnd => {
                if let Some((name_range, _after_name)) =
                    head_name(window, index, allow_fused_closer)
                {
                    #[cfg(test)]
                    add_segment_scan_token_visits(_after_name - index);
                    if source[name_range].eq_ignore_ascii_case("iftags") {
                        depth -= 1;
                        if depth == 0 {
                            let closer_start = token.span.start;
                            let mut end_index = index;
                            let closer_end = loop {
                                #[cfg(test)]
                                increment_segment_scan_token_visits();
                                let token = window.get(end_index)?;
                                match token.token {
                                    Token::RightBlock => break token.span.end,
                                    Token::RightLink if allow_fused_closer => {
                                        fused_closer = true;
                                        break token.span.start + 2;
                                    }
                                    Token::LineBreak
                                    | Token::ParagraphBreak
                                    | Token::InputEnd => return None,
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
        if index > 8192 {
            return None;
        }
    };

    Some(IftagsSegment {
        range: segment_start..closer_end,
        head: segment_start..head_end,
        body: body_start..body_end,
        closer: body_end..closer_end,
        include_body,
        fused_closer,
    })
}

pub(crate) fn consume_iftags_segment<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<IftagsSegment>, ParseError>
where
    'r: 't,
{
    consume_segment(parser, false)
}

pub(crate) fn consume_iftags_segment_fused<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<IftagsSegment>, ParseError>
where
    'r: 't,
{
    consume_segment(parser, true)
}

fn consume_segment<'r, 't>(
    parser: &mut Parser<'r, 't>,
    allow_fused_closer: bool,
) -> Result<Option<IftagsSegment>, ParseError>
where
    'r: 't,
{
    let Some(segment) = (if allow_fused_closer {
        iftags_segment_fused(parser)
    } else {
        iftags_segment(parser)
    }) else {
        return Ok(None);
    };
    while parser.current().span.start < segment.range.end
        && parser.current().token != Token::InputEnd
    {
        parser.step()?;
    }
    Ok(Some(segment))
}

pub(crate) fn bracket_close_text(token: Token) -> Option<&'static str> {
    match token {
        Token::RightBracket => Some("]"),
        Token::RightBlock => Some("]]"),
        Token::RightLink => Some("]]]"),
        _ => None,
    }
}

pub(crate) fn complete_fused_bracket_close<'r, 't>(
    parser: &mut Parser<'r, 't>,
    close_text: &str,
) -> Option<&'r ExtractedToken<'t>> {
    debug_assert!(close_text.len() >= 2);
    let mut probe = parser.clone();
    let mut last = parser.current();
    let mut matched = 1;
    while matched < close_text.len() {
        let token = probe.current();
        let text = bracket_close_text(token.token)?;
        if !close_text[matched..].starts_with(text) {
            return None;
        }
        matched += text.len();
        last = token;
        probe.step().ok()?;
    }
    parser.update(&probe);
    Some(last)
}

#[derive(Debug, Clone)]
pub(crate) struct SplitDelimiterMatch {
    pub consume_end: usize,
    pub leftover: Option<Range<usize>>,
}

pub(crate) fn segment_is_textless(segment: &IftagsSegment) -> bool {
    !segment.include_body || segment.body.is_empty()
}

fn segment_token_count(
    window: TokenWindow<'_, '_>,
    start: usize,
    range_end: usize,
) -> usize {
    let mut count = 0;
    while let Some(token) = window.get(start + count) {
        if token.span.start >= range_end {
            break;
        }
        count += 1;
    }
    count
}

pub(crate) fn split_delimiter_end<'a, 't>(
    window: TokenWindow<'a, 't>,
    ctx: SegmentCtx<'_, 't>,
    delimiter: SplitDelimiter,
) -> Option<SplitDelimiterMatch> {
    if !delimiter.is_half(window.first()) {
        return None;
    }
    let half = delimiter.half_char();
    let mut index = 1usize;
    let mut bridges = 0usize;
    loop {
        let token = window.get(index)?;
        if token.token == Token::LeftBlock {
            let segment = scan_segment_ctx(window.offset(index)?, ctx, false)?;
            bridges += 1;
            if segment_is_textless(&segment) {
                index += segment_token_count(window, index, segment.range.end);
                continue;
            }
            let contribution = &ctx.source[segment.body.clone()];
            let first_char = contribution.chars().next()?;
            if first_char != half {
                return None;
            }
            let end = segment.body.start + first_char.len_utf8();
            let leftover = (end < segment.body.end).then_some(end..segment.body.end);
            return Some(SplitDelimiterMatch {
                consume_end: segment.range.end,
                leftover,
            });
        }
        if token.token == Token::LeftComment {
            let count = comment_bridge_end(window.offset(index)?)?;
            bridges += 1;
            index += count;
            continue;
        }
        break;
    }
    if bridges == 0 {
        return None;
    }
    let token = window.get(index)?;
    if delimiter.is_half(token) {
        return Some(SplitDelimiterMatch {
            consume_end: token.span.end,
            leftover: None,
        });
    }
    if token.token == delimiter.delimiter_token() {
        let mid = token.span.start + half.len_utf8();
        return Some(SplitDelimiterMatch {
            consume_end: token.span.end,
            leftover: Some(mid..token.span.end),
        });
    }
    None
}
