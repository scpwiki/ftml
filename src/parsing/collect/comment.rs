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

use super::prelude::*;
use crate::parsing::rule::impls::comment_closer_token_count;
use std::borrow::Cow;
use std::ops::Range;

#[derive(Debug)]
pub(crate) struct CommentElidedText<'t> {
    source: &'t str,
    span: Range<usize>,
    comments: Vec<Range<usize>>,
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
        }
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
        let first = self
            .comments
            .partition_point(|comment| comment.end <= range.start);
        if first == self.comments.len() {
            return (Cow::Borrowed(&self.source[range]), Vec::new());
        }
        if self.comments[first].start >= range.end {
            return (Cow::Borrowed(&self.source[range]), Vec::new());
        }

        let mut output = String::with_capacity(range.len());
        let mut seams = Vec::new();
        let mut cursor = range.start;
        for comment in &self.comments[first..] {
            if comment.start >= range.end {
                break;
            }
            let comment_start = comment.start.max(range.start);
            let comment_end = comment.end.min(range.end);
            if cursor < comment_start {
                output.push_str(&self.source[cursor..comment_start]);
            }
            seams.push(output.len());
            cursor = cursor.max(comment_end);
        }
        if cursor < range.end {
            output.push_str(&self.source[cursor..range.end]);
        }
        (Cow::Owned(output), seams)
    }
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
            let end = last.span.start;
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
