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

use super::comment::consume_valid_comment;
use super::iftags::{
    bracket_close_text, complete_fused_bracket_close, consume_iftags_segment,
    consume_iftags_segment_fused, iftags_segment, iftags_segment_fused,
};
use super::prelude::*;
use std::borrow::Cow;

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

pub fn collect_text_resolved<'p, 'r, 't>(
    parser: &'p mut Parser<'r, 't>,
    rule: Rule,
    closes: &[ParseCondition],
    invalids: &[ParseCondition],
    kind: Option<ParseErrorKind>,
) -> Result<(Cow<'t, str>, &'r ExtractedToken<'t>), ParseError>
where
    'r: 't,
{
    let (text, last, _) =
        collect_text_resolved_impl(parser, rule, closes, invalids, kind, false, false)?;
    Ok((text, last))
}

/// Resolved head text for a bespoke head scan that has already
/// located the head's closing token itself (the standalone-button
/// and social scans). Collects from the parser's current position
/// with the standard block-head conditions and returns the text
/// only when collection terminates at the expected closing token,
/// so a disagreement with the caller's own scan falls back to the
/// raw head instead of changing the head's extent.
pub fn collect_text_resolved_head<'p, 'r, 't>(
    parser: &'p mut Parser<'r, 't>,
    rule: Rule,
    closing_start: usize,
) -> Option<Cow<'t, str>>
where
    'r: 't,
{
    let end_conditions = [ParseCondition::current(Token::RightBlock)];
    let reject_conditions = [
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::current(Token::LineBreak),
    ];
    let (text, last) = collect_text_resolved_fused(
        parser,
        rule,
        &end_conditions,
        &reject_conditions,
        Some(ParseErrorKind::BlockMalformedArguments),
        false,
    )
    .ok()?;
    (last.span.start == closing_start).then_some(text)
}

pub fn collect_text_resolved_fused<'p, 'r, 't>(
    parser: &'p mut Parser<'r, 't>,
    rule: Rule,
    closes: &[ParseCondition],
    invalids: &[ParseCondition],
    kind: Option<ParseErrorKind>,
    allow_fused_name_close: bool,
) -> Result<(Cow<'t, str>, &'r ExtractedToken<'t>), ParseError>
where
    'r: 't,
{
    let (text, last, _) = collect_text_resolved_impl(
        parser,
        rule,
        closes,
        invalids,
        kind,
        true,
        allow_fused_name_close,
    )?;
    Ok((text, last))
}

pub fn collect_text_resolved_fused_tracked<'p, 'r, 't>(
    parser: &'p mut Parser<'r, 't>,
    rule: Rule,
    closes: &[ParseCondition],
    invalids: &[ParseCondition],
    kind: Option<ParseErrorKind>,
) -> Result<(Cow<'t, str>, &'r ExtractedToken<'t>, bool), ParseError>
where
    'r: 't,
{
    collect_text_resolved_impl(parser, rule, closes, invalids, kind, true, true)
}

fn collect_text_resolved_impl<'p, 'r, 't>(
    parser: &'p mut Parser<'r, 't>,
    _rule: Rule,
    closes: &[ParseCondition],
    invalids: &[ParseCondition],
    kind: Option<ParseErrorKind>,
    allow_fused_closer: bool,
    allow_fused_name_close: bool,
) -> Result<(Cow<'t, str>, &'r ExtractedToken<'t>, bool), ParseError>
where
    'r: 't,
{
    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let mut copied_until = start;
    let mut output = None::<String>;
    let mut segment_consumed = false;
    let mut fused_close = None::<(usize, usize)>;

    loop {
        if let Some((bracket_start, segment_end)) = fused_close.take() {
            let current = parser.current();
            if current.token == Token::RightBracket && current.span.start == bracket_start
            {
                let text = match output {
                    Some(mut output) => {
                        output.push_str(&source[copied_until..segment_end]);
                        Cow::Owned(output)
                    }
                    None => Cow::Borrowed(&source[start..segment_end]),
                };

                parser.step()?;
                return Ok((text, current, segment_consumed));
            }
        }

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
                    if end >= copied_until {
                        output.push_str(&source[copied_until..end]);
                    }
                    Cow::Owned(output)
                }
                None => Cow::Borrowed(&source[start..end]),
            };

            if last.token != Token::InputEnd {
                parser.step()?;
            }

            return Ok((text, last, segment_consumed));
        }

        if parser.evaluate_any(invalids) {
            return Err(parser.make_err(kind.unwrap_or(ParseErrorKind::RuleFailed)));
        }

        if allow_fused_name_close
            && parser.settings().layout.legacy()
            && parser.current().token == Token::RightLink
            && (segment_consumed || output.is_some())
            && closes.iter().any(|close| {
                matches!(close, ParseCondition::CurrentToken(Token::RightBlock))
            })
            && !closes.iter().any(|close| {
                matches!(close, ParseCondition::CurrentToken(Token::RightLink))
            })
        {
            let last = parser.current();
            let end = last.span.start;
            let text = match output {
                Some(mut output) => {
                    if end >= copied_until {
                        output.push_str(&source[copied_until..end]);
                    }
                    Cow::Owned(output)
                }
                None => Cow::Borrowed(&source[start..end]),
            };

            parser.note_fused_name_residual(last.span.end);
            parser.step()?;
            return Ok((text, last, segment_consumed));
        }

        if parser.current().token == Token::InputEnd {
            return Err(parser.make_err(ParseErrorKind::EndOfInput));
        }

        if parser.current().token == Token::LeftComment {
            let range = consume_valid_comment(parser)?;
            let output = output.get_or_insert_with(String::new);
            output.push_str(&source[copied_until..range.start]);
            copied_until = range.end;
            continue;
        }

        if parser.current().token == Token::LeftBlock
            && let Some(segment) = (if allow_fused_closer {
                iftags_segment_fused(parser)
            } else {
                iftags_segment(parser)
            })
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
            let segment = (if allow_fused_closer {
                consume_iftags_segment_fused(parser)?
            } else {
                consume_iftags_segment(parser)?
            })
            .expect("segment presence was just checked");
            segment_consumed = true;

            {
                let output = output.get_or_insert_with(String::new);
                output.push_str(&source[copied_until..segment.range.start]);
                if segment.include_body {
                    output.push_str(&source[segment.body.clone()]);
                }
            }
            copied_until = segment.range.end;
            if segment.fused_closer {
                // A block-head value closes with a single `]]`: the
                // fused token's leftover bracket plus a directly
                // following `]` complete it (checked at loop top).
                let head_close = closes.iter().any(|close| {
                    matches!(close, ParseCondition::CurrentToken(Token::RightBlock))
                });
                if head_close {
                    fused_close = Some((segment.range.end + 1, segment.range.end));
                } else if let Some(straddle) = straddle {
                    // Other fields (link targets and labels): the
                    // leftover bracket begins the field's own
                    // closing run; complete it from the bracket
                    // tokens that follow.
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
                        ParseCondition::SplitClose(_)
                        | ParseCondition::PaddedSplitClose(_) => None,
                    });
                    if let Some(last) = fused {
                        return Ok((
                            Cow::Owned(output.take().unwrap()),
                            last,
                            segment_consumed,
                        ));
                    }

                    // The run overshoots the close (extra authored
                    // brackets) but nothing but brackets remains:
                    // the field ends at the fused token, and the
                    // leftover brackets render as following text.
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
                        return Ok((
                            Cow::Owned(output.take().unwrap()),
                            straddle,
                            segment_consumed,
                        ));
                    }
                }
            }
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
