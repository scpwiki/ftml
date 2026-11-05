/*
 * parsing/collect/comment.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2026 Wikijump Team
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

use super::iftags::{
    TokenWindow, bracket_close_text, complete_fused_bracket_close,
    consume_iftags_segment_fused, iftags_segment_fused,
};
use super::prelude::*;
use crate::parsing::unmatched_close_scan::LinkTargetScanMemo;
use std::borrow::Cow;
use std::ops::Range;

#[derive(Debug)]
pub(crate) struct CommentElidedText<'t> {
    source: &'t str,
    span: Range<usize>,
    comments: Vec<Range<usize>>,
    segments: Vec<Range<usize>>,
}

impl<'t> CommentElidedText<'t> {
    pub(crate) fn new(
        source: &'t str,
        span: Range<usize>,
        comments: Vec<Range<usize>>,
    ) -> Self {
        debug_assert!(comments.iter().all(|range| {
            range.start >= span.start && range.end <= span.end && range.start <= range.end
        }));
        Self {
            source,
            span,
            comments,
            segments: Vec::new(),
        }
    }

    pub(crate) fn with_segments(mut self, segments: Vec<Range<usize>>) -> Self {
        debug_assert!(segments.iter().all(|range| {
            range.start >= self.span.start
                && range.end <= self.span.end
                && range.start <= range.end
        }));
        self.segments = segments;
        self
    }

    pub(crate) fn source(&self) -> &'t str {
        &self.source[self.span.clone()]
    }

    pub(crate) fn span(&self) -> Range<usize> {
        self.span.clone()
    }

    pub(crate) fn comment_ranges(&self) -> &[Range<usize>] {
        &self.comments
    }

    pub(crate) fn segment_ranges(&self) -> &[Range<usize>] {
        &self.segments
    }

    pub(crate) fn elide_segments_range(&self, range: Range<usize>) -> Cow<'t, str> {
        elide_ranges(self.source, &self.segments, range)
    }

    pub(crate) fn prefix_before_first_comment(&self) -> &'t str {
        let end = self
            .comments
            .first()
            .map_or(self.span.end, |range| range.start);
        &self.source[self.span.start..end]
    }

    pub(crate) fn into_cow(self) -> Cow<'t, str> {
        self.elide_range(self.span.clone())
    }

    pub(crate) fn elide_range(&self, range: Range<usize>) -> Cow<'t, str> {
        self.elide_range_with_seams(range).0
    }

    pub(crate) fn elide_range_with_seams(
        &self,
        range: Range<usize>,
    ) -> (Cow<'t, str>, Vec<usize>) {
        debug_assert!(range.start >= self.span.start && range.end <= self.span.end);
        if self.segments.is_empty() {
            return elide_ranges_with_seams(self.source, &self.comments, range);
        }
        if self.comments.is_empty() {
            return elide_ranges_with_seams(self.source, &self.segments, range);
        }
        let mut merged: Vec<Range<usize>> =
            Vec::with_capacity(self.comments.len() + self.segments.len());
        merged.extend(self.comments.iter().cloned());
        merged.extend(self.segments.iter().cloned());
        merged.sort_by_key(|range| range.start);
        elide_ranges_with_seams(self.source, &merged, range)
    }
}

fn elide_ranges_with_seams<'t>(
    source: &'t str,
    ranges: &[Range<usize>],
    range: Range<usize>,
) -> (Cow<'t, str>, Vec<usize>) {
    let first = ranges.partition_point(|elision| elision.end <= range.start);
    if first == ranges.len() {
        return (Cow::Borrowed(&source[range]), Vec::new());
    }
    if ranges[first].start >= range.end {
        return (Cow::Borrowed(&source[range]), Vec::new());
    }

    let mut output = String::with_capacity(range.len());
    let mut seams = Vec::new();
    let mut cursor = range.start;
    for elision in &ranges[first..] {
        if elision.start >= range.end {
            break;
        }
        let elision_start = elision.start.max(range.start);
        let elision_end = elision.end.min(range.end);
        if cursor < elision_start {
            output.push_str(&source[cursor..elision_start]);
        }
        seams.push(output.len());
        cursor = cursor.max(elision_end);
    }
    if cursor < range.end {
        output.push_str(&source[cursor..range.end]);
    }
    (Cow::Owned(output), seams)
}

fn elide_ranges<'t>(
    source: &'t str,
    ranges: &[Range<usize>],
    range: Range<usize>,
) -> Cow<'t, str> {
    elide_ranges_with_seams(source, ranges, range).0
}

pub(crate) fn collect_comment_elided_keep<'r, 't>(
    parser: &mut Parser<'r, 't>,
    closes: &[ParseCondition],
    invalids: &[ParseCondition],
    kind: Option<ParseErrorKind>,
    link_memo: Option<LinkTargetScanMemo>,
) -> Result<(CommentElidedText<'t>, &'r ExtractedToken<'t>), ParseError>
where
    'r: 't,
{
    let start = parser.current().span.start;
    collect_comment_elided_keep_from(parser, start, closes, invalids, kind, link_memo)
}

pub(crate) fn collect_comment_elided_keep_from<'r, 't>(
    parser: &mut Parser<'r, 't>,
    start: usize,
    closes: &[ParseCondition],
    invalids: &[ParseCondition],
    kind: Option<ParseErrorKind>,
    link_memo: Option<LinkTargetScanMemo>,
) -> Result<(CommentElidedText<'t>, &'r ExtractedToken<'t>), ParseError>
where
    'r: 't,
{
    let mut traversed = Vec::new();
    let result = collect_comment_elided_keep_from_inner(
        parser,
        start,
        closes,
        invalids,
        kind,
        link_memo,
        &mut traversed,
    );
    if let (Some(memo), Err(_)) = (link_memo, &result) {
        parser.cache_link_target_scan_failed(
            &traversed,
            memo.line_break_separator,
            memo.in_iftags_span,
        );
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn collect_comment_elided_keep_from_inner<'r, 't>(
    parser: &mut Parser<'r, 't>,
    start: usize,
    closes: &[ParseCondition],
    invalids: &[ParseCondition],
    kind: Option<ParseErrorKind>,
    link_memo: Option<LinkTargetScanMemo>,
    traversed: &mut Vec<usize>,
) -> Result<(CommentElidedText<'t>, &'r ExtractedToken<'t>), ParseError>
where
    'r: 't,
{
    let mut comments = Vec::new();

    loop {
        if let Some(memo) = link_memo {
            let addr = parser.current() as *const ExtractedToken as usize;
            traversed.push(addr);
            #[cfg(test)]
            parser.add_unmatched_close_scan_visits(1);
            if parser.link_target_scan_failed(
                addr,
                memo.line_break_separator,
                memo.in_iftags_span,
            ) {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
        }

        if parser.current_generated().is_some()
            || matches!(
                parser.current().token,
                Token::GeneratedPageLink | Token::GeneratedTagLinks | Token::RuntimeText
            )
        {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }

        if parser.evaluate_any(closes) {
            let last = parser.current();
            let end = comments
                .iter()
                .map(|range: &Range<usize>| range.end)
                .max()
                .unwrap_or_default()
                .max(last.span.start);
            let field =
                CommentElidedText::new(parser.full_text().inner(), start..end, comments);
            if last.token != Token::InputEnd {
                parser.step()?;
            }
            return Ok((field, last));
        }

        if parser.evaluate_any(invalids) {
            return Err(parser.make_err(kind.unwrap_or(ParseErrorKind::RuleFailed)));
        }
        if parser.current().token == Token::InputEnd {
            return Err(parser.make_err(ParseErrorKind::EndOfInput));
        }

        if parser.current().token == Token::LeftComment {
            comments.push(consume_valid_comment(parser)?);
        } else if parser.current().token == Token::LeftBlock
            && let Some(segment) = iftags_segment_fused(parser)
        {
            let straddle = if segment.fused_closer {
                std::iter::once(parser.current())
                    .chain(parser.remaining())
                    .find(|token| {
                        token.span.start < segment.range.end
                            && token.span.end > segment.range.end
                    })
            } else {
                None
            };
            let segment = consume_iftags_segment_fused(parser)?
                .expect("segment presence was just checked");

            comments.extend(segment.elisions());

            if let Some(straddle) = straddle {
                let fused = closes.iter().find_map(|close| match close {
                    ParseCondition::CurrentToken(token) => {
                        let close_text = bracket_close_text(*token)?;
                        if close_text.len() == 1 {
                            return Some(straddle);
                        }
                        let last = complete_fused_bracket_close(parser, close_text)?;
                        Some(if *token == straddle.token {
                            straddle
                        } else {
                            last
                        })
                    }
                    ParseCondition::TokenPair(..) => None,
                    ParseCondition::CurrentTokenAtSourceLineStart(_)
                    | ParseCondition::SplitClose(_)
                    | ParseCondition::PaddedSplitClose(_) => None,
                });
                if let Some(last) = fused {
                    let end = comments
                        .iter()
                        .map(|range: &Range<usize>| range.end)
                        .max()
                        .unwrap_or_default();
                    let field = CommentElidedText::new(
                        parser.full_text().inner(),
                        start..end,
                        comments,
                    );
                    return Ok((field, last));
                }

                let only_brackets_left = std::iter::once(parser.current())
                    .chain(parser.remaining())
                    .all(|token| {
                        matches!(
                            token.token,
                            Token::RightBracket
                                | Token::RightBlock
                                | Token::RightLink
                                | Token::InputEnd
                        )
                    });
                if only_brackets_left
                    && closes.iter().any(|close| {
                        matches!(
                            close,
                            ParseCondition::CurrentToken(token)
                                if *token == straddle.token
                        )
                    })
                {
                    let end = comments
                        .iter()
                        .map(|range: &Range<usize>| range.end)
                        .max()
                        .unwrap_or_default();
                    let field = CommentElidedText::new(
                        parser.full_text().inner(),
                        start..end,
                        comments,
                    );
                    return Ok((field, straddle));
                }
            }
        } else {
            parser.step()?;
        }
    }
}

pub(crate) fn consume_valid_comment<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Range<usize>, ParseError>
where
    'r: 't,
{
    let start = parser.current().span.start;
    parser.step()?;

    loop {
        if parser.current_generated().is_some()
            || matches!(
                parser.current().token,
                Token::GeneratedPageLink | Token::GeneratedTagLinks | Token::RuntimeText
            )
        {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
        if let Some(token_count) =
            comment_closer_token_count_at(TokenWindow::at(parser), 0)
        {
            parser.step_n(token_count)?;
            return Ok(start..parser.current().span.start);
        }
        if parser.current().token == Token::InputEnd {
            return Err(parser.make_err(ParseErrorKind::EndOfInput));
        }
        parser.step()?;
    }
}

pub(crate) fn comment_closer_token_count_at(
    window: TokenWindow<'_, '_>,
    start: usize,
) -> Option<usize> {
    let current = window.get(start)?;
    let current_is_inert = matches!(
        current.token,
        Token::RuntimeText | Token::GeneratedPageLink | Token::GeneratedTagLinks
    );
    let trailing_hyphens = current
        .slice
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'-')
        .count();
    if !current_is_inert
        && trailing_hyphens >= 2
        && trailing_hyphens < current.slice.len()
        && window
            .get(start + 1)
            .is_some_and(|token| token.token == Token::RightBracket && token.slice == "]")
    {
        return Some(2);
    }

    let mut hyphens = 0;
    let mut index = start;
    loop {
        let token = window.get(index)?;
        if matches!(
            token.token,
            Token::RuntimeText | Token::GeneratedPageLink | Token::GeneratedTagLinks
        ) {
            return None;
        }

        if !token.slice.is_empty() && token.slice.bytes().all(|byte| byte == b'-') {
            hyphens += token.slice.len();
            index += 1;
            continue;
        }

        return (hyphens >= 2
            && token.token == Token::RightBracket
            && token.slice == "]")
            .then_some(index - start + 1);
    }
}

pub(crate) fn elide_comments_from_literal<'t>(text: &'t str) -> Cow<'t, str> {
    if !text.contains("[!--") {
        return Cow::Borrowed(text);
    }
    let tokenization = crate::tokenize(text);
    let tokens = tokenization.tokens();
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let comment_end = if tokens[index].token == Token::LeftComment {
            TokenWindow::from_slice(&tokens[index..])
                .and_then(comment_bridge_end)
                .map(|count| tokens[index + count - 1].span.end)
        } else {
            None
        };
        if let Some(end) = comment_end {
            ranges.push(tokens[index].span.start..end);
            index += 1;
            while index < tokens.len() && tokens[index].span.start < end {
                index += 1;
            }
        } else {
            index += 1;
        }
    }
    if ranges.is_empty() {
        return Cow::Borrowed(text);
    }
    elide_ranges(text, &ranges, 0..text.len())
}

pub(crate) fn comment_bridge_end(window: TokenWindow<'_, '_>) -> Option<usize> {
    let mut index = 1;
    loop {
        let token = window.get(index)?;
        if matches!(
            token.token,
            Token::RuntimeText | Token::GeneratedPageLink | Token::GeneratedTagLinks
        ) {
            return None;
        }
        if let Some(count) = comment_closer_token_count_at(window, index) {
            return Some(index + count);
        }
        if token.token == Token::InputEnd {
            return None;
        }
        index += 1;
    }
}
