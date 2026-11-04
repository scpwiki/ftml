/*
 * parsing/collect/text.rs
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

use super::iftags::consume_iftags_segment;
use super::prelude::*;
use std::borrow::Cow;

/// Generic function to consume all tokens into a single string slice.
///
/// This is a subset of the functionality provided by `collect()`,
/// as it specifically gathers all the extracted tokens into a string slice,
/// rather than considering them as special elements.
#[inline]
pub fn collect_text<'p, 'r, 't>(
    parser: &'p mut Parser<'r, 't>,
    rule: Rule,
    closes: &[ParseCondition],
    invalids: &[ParseCondition],
    kind: Option<ParseErrorKind>,
) -> Result<&'t str, ParseError>
where
    'r: 't,
{
    let result = collect_text_keep(parser, rule, closes, invalids, kind)?;
    let (slice, _) = result;
    Ok(slice)
}

/// Modified form of `collect_text()` that also returns the last token.
///
/// The last token terminating the collection is kept, and returned
/// to the caller alongside the string slice.
///
/// Compare with `collect_consume_keep()`.
pub fn collect_text_keep<'p, 'r, 't>(
    parser: &'p mut Parser<'r, 't>,
    _rule: Rule,
    closes: &[ParseCondition],
    invalids: &[ParseCondition],
    kind: Option<ParseErrorKind>,
) -> Result<(&'t str, &'r ExtractedToken<'t>), ParseError>
where
    'r: 't,
{
    let (start, mut end) = (parser.current(), None);

    loop {
        if matches!(
            parser.current().token,
            Token::GeneratedPageLink | Token::GeneratedTagLinks
        ) {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }

        if parser.evaluate_any(closes) {
            let last = parser.current();
            let slice = match (start, end) {
                (start, Some(end)) => parser.full_text().slice(start, end),

                (_, None) => "",
            };

            if parser.current().token != Token::InputEnd {
                parser.step()?;
            }

            return Ok((slice, last));
        }

        if parser.evaluate_any(invalids) {
            return Err(parser.make_err(kind.unwrap_or(ParseErrorKind::RuleFailed)));
        }

        if parser.current().token == Token::InputEnd {
            return Err(parser.make_err(ParseErrorKind::EndOfInput));
        }

        end = Some(parser.current());
        parser.step()?;
    }
}

/// Seam-resolving form of `collect_text_keep()` for raw fields that
/// the iftags block rule cannot reach (link targets and labels in the
/// Wikijump layout).
///
/// A complete iftags segment inside the field is consumed atomically,
/// so the whitespace and brackets in its head never terminate or
/// corrupt the collection, and the returned text keeps only the
/// segment's body when its conditions hold for the page. When no
/// segment is present the result is the same borrowed slice
/// `collect_text_keep()` would return.
pub fn collect_text_resolved<'p, 'r, 't>(
    parser: &'p mut Parser<'r, 't>,
    _rule: Rule,
    closes: &[ParseCondition],
    invalids: &[ParseCondition],
    kind: Option<ParseErrorKind>,
) -> Result<(Cow<'t, str>, &'r ExtractedToken<'t>), ParseError>
where
    'r: 't,
{
    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let mut copied_until = start;
    let mut output = None::<String>;

    loop {
        if matches!(
            parser.current().token,
            Token::GeneratedPageLink | Token::GeneratedTagLinks
        ) {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }

        if parser.evaluate_any(closes) {
            let last = parser.current();
            let end = last.span.start;
            let text = match output {
                Some(mut output) => {
                    output.push_str(&source[copied_until..end]);
                    Cow::Owned(output)
                }
                None => Cow::Borrowed(&source[start..end]),
            };

            if last.token != Token::InputEnd {
                parser.step()?;
            }

            return Ok((text, last));
        }

        if parser.evaluate_any(invalids) {
            return Err(parser.make_err(kind.unwrap_or(ParseErrorKind::RuleFailed)));
        }

        if parser.current().token == Token::InputEnd {
            return Err(parser.make_err(ParseErrorKind::EndOfInput));
        }

        if parser.current().token == Token::LeftBlock
            && let Some(segment) = consume_iftags_segment(parser)?
        {
            let output = output.get_or_insert_with(String::new);
            output.push_str(&source[copied_until..segment.range.start]);
            if segment.include_body {
                output.push_str(&source[segment.body.clone()]);
            }
            copied_until = segment.range.end;
            continue;
        }

        parser.step()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::rule::impls::RULE_TEXT;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn collect_text_entrypoints_return_slice_and_terminator() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        let tokenization = crate::tokenize("alpha]]tail");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().expect("identifier should follow input start");

        let slice = collect_text(
            &mut parser,
            RULE_TEXT,
            &[ParseCondition::current(Token::RightBlock)],
            &[],
            None,
        )
        .expect("text should collect until the right block token");

        assert_eq!(slice, "alpha");
        assert_eq!(parser.current().slice, "tail");

        let tokenization = crate::tokenize("]]tail");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("right block should follow input start");

        let (slice, last) = collect_text_keep(
            &mut parser,
            RULE_TEXT,
            &[ParseCondition::current(Token::RightBlock)],
            &[],
            None,
        )
        .expect("empty text should still stop at the right block token");

        assert_eq!(slice, "");
        assert_eq!(last.token, Token::RightBlock);
        assert_eq!(parser.current().slice, "tail");

        let tokenization = crate::tokenize("alpha");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().expect("identifier should follow input start");

        let (slice, last) = collect_text_keep(
            &mut parser,
            RULE_TEXT,
            &[ParseCondition::current(Token::InputEnd)],
            &[],
            None,
        )
        .expect("input end may terminate text collection");

        assert_eq!(slice, "alpha");
        assert_eq!(last.token, Token::InputEnd);
        assert_eq!(parser.current().token, Token::InputEnd);
    }

    #[test]
    fn collect_text_keep_reports_invalid_tokens_and_end_of_input() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        let tokenization = crate::tokenize("alpha\n\n");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().expect("identifier should follow input start");
        let error = collect_text_keep(
            &mut parser,
            RULE_TEXT,
            &[ParseCondition::current(Token::RightBlock)],
            &[ParseCondition::current(Token::ParagraphBreak)],
            None,
        )
        .expect_err("paragraph break should abort text collection");
        assert_eq!(error.kind(), ParseErrorKind::RuleFailed);

        let tokenization = crate::tokenize("alpha");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().expect("identifier should follow input start");
        let error = collect_text_keep(
            &mut parser,
            RULE_TEXT,
            &[ParseCondition::current(Token::RightBlock)],
            &[],
            None,
        )
        .expect_err("missing close token should reach end of input");
        assert_eq!(error.kind(), ParseErrorKind::EndOfInput);
    }
}
