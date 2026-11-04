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
    bracket_close_text, complete_fused_bracket_close, consume_iftags_segment_fused,
    iftags_segment_fused,
};
use super::prelude::*;
use crate::parsing::rule::impls::comment_closer_token_count;
use std::borrow::Cow;
use std::ops::Range;

#[derive(Debug)]
pub(crate) struct CommentElidedText<'t> {
    source: &'t str,
    span: Range<usize>,
    comments: Vec<Range<usize>>,
    /// Elision ranges for resolved iftags segments, kept apart from
    /// comments: a comment resets the text that follows it (the
    /// `getAttrs` key fragment starts after the last comment), while a
    /// segment merely joins the parts around it.
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

    /// Elide only the resolved iftags segment ranges in `range`,
    /// joining the parts around each one; comments are left in place.
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
) -> Result<(CommentElidedText<'t>, &'r ExtractedToken<'t>), ParseError>
where
    'r: 't,
{
    let start = parser.current().span.start;
    let mut comments = Vec::new();

    loop {
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
            // A resolved segment at the field's trailing edge can
            // end inside the closing token, its closer fused with
            // the field's own closing bracket: the field then ends
            // at the segment's end, not at the token's start.
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
            // The token a fused segment ends inside, captured before
            // consumption moves past it: a caller matching on the
            // terminating token may need it below.
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

            // An iftags segment inside the field resolves where the
            // block rule cannot run: its elision ranges join the
            // comments so the materialized text keeps only the body
            // when the conditions hold, and nothing when they do not.
            comments.extend(segment.elisions());

            // A segment at the field's trailing edge ends inside a
            // fused bracket token: the token's leftover bracket,
            // extended by any bracket tokens after it, is this
            // field's closing run when it spells one of the closing
            // tokens exactly.
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

                // The run overshoots the close (extra authored
                // brackets) but nothing but brackets remains: the
                // field ends at the fused token, and the leftover
                // brackets render as following text.
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
        if let Some(token_count) = comment_closer_token_count(parser) {
            parser.step_n(token_count)?;
            return Ok(start..parser.current().span.start);
        }
        if parser.current().token == Token::InputEnd {
            return Err(parser.make_err(ParseErrorKind::EndOfInput));
        }
        parser.step()?;
    }
}
