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

use super::prelude::*;
use crate::parsing::ElementCondition;
use std::ops::Range;

/// A complete `[[iftags ...]]...[[/iftags]]` segment found inside a raw
/// text field (a link target or label, a block argument value, and other
/// places that collect source text without invoking the iftags block
/// rule).
///
/// The block rule only runs when the parser gathers elements, so these
/// raw collectors used to copy the segment's markup verbatim: the
/// whitespace and brackets inside the head terminated or corrupted the
/// field (ftml discovery R1/R11). Collectors consume a segment
/// atomically through this helper and keep only the resolved text: the
/// body when the conditions hold for the page, nothing when they do
/// not. Condition evaluation mirrors the iftags block rule, including
/// the legacy no-argument gate following the page's tag presence.
#[derive(Debug, Clone)]
pub(crate) struct IftagsSegment {
    /// The whole segment, from `[[` through the closer's `]]`.
    pub range: Range<usize>,
    /// The opener head, `[[iftags ...]]`.
    pub head: Range<usize>,
    /// The body between the opener head and the closer head.
    pub body: Range<usize>,
    /// The closer head, `[[/iftags]]`.
    pub closer: Range<usize>,
    /// Whether the conditions hold and the body text is kept.
    pub include_body: bool,
    /// Whether the closer's `]]` was fused with one following `]`
    /// into a single `RightLink` token, whose span extends one char
    /// past `range.end`. Only the fused scan reports this; see
    /// `iftags_segment_fused`.
    pub fused_closer: bool,
}

impl IftagsSegment {
    /// Source ranges a text collector must drop to materialize the
    /// resolved field: the head and closer when the body is kept, the
    /// whole segment when it is not.
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

/// The number of tokens segment scans have examined on this thread.
/// Test-only instrumentation, kept out of `Parser` because the
/// parser's size is budgeted for deep recursive parsing.
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

/// A borrowed window over the token stream: the token at some
/// position plus every token after it. Segment scans index into a
/// window instead of collecting the remaining tokens into a fresh
/// vector per call, so a scan costs only the tokens it examines.
#[derive(Debug, Copy, Clone)]
pub(crate) struct TokenWindow<'a, 't> {
    first: &'a ExtractedToken<'t>,
    rest: &'a [ExtractedToken<'t>],
}

impl<'a, 't> TokenWindow<'a, 't> {
    /// The window starting at the parser's current token:
    /// `remaining()` starts after the current token, so the window
    /// is the current token followed by the remaining ones.
    pub(crate) fn at<'r>(parser: &Parser<'r, 't>) -> TokenWindow<'r, 't> {
        TokenWindow {
            first: parser.current(),
            rest: parser.remaining(),
        }
    }

    /// The first token in the window.
    pub(crate) fn first(&self) -> &'a ExtractedToken<'t> {
        self.first
    }

    /// The token at `index` within the window, if it exists.
    pub(crate) fn get(&self, index: usize) -> Option<&'a ExtractedToken<'t>> {
        if index == 0 {
            Some(self.first)
        } else {
            self.rest.get(index - 1)
        }
    }
}

/// Gathers the block name starting at token `start` in `window`,
/// where the token at `start` is the opening token (`[[` or `[[/`).
/// Returns the name's source range and the index of the first token
/// after the name.
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

/// Finds a complete iftags segment starting at the parser's current
/// token, without moving the parser. Returns `None` when the current
/// token does not open such a segment, when the segment is unclosed or
/// spans a line break, or when the gate cannot be evaluated the way
/// the block rule evaluates it (a bare gate outside the legacy
/// layout). Inside a matched Wikidot iftags span, nested gates are
/// literal text (#792), so no segment is reported there either.
pub(crate) fn iftags_segment<'r, 't>(parser: &Parser<'r, 't>) -> Option<IftagsSegment>
where
    'r: 't,
{
    scan_segment(parser, false)
}

/// Like `iftags_segment`, but a segment closer whose `]]` directly
/// abuts one more `]` still terminates the segment. The scanner
/// fuses such a `]]]` run into a single `RightLink` token, so a
/// closer at the very end of a block-head value (whose own `]]`
/// supplies the third bracket) is invisible to the default scan.
/// The returned segment ends two chars into that token and is
/// marked `fused_closer`; the caller owns the leftover bracket.
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
    if parser.settings().layout.legacy() && parser.in_wikidot_iftags_span() {
        return None;
    }

    let window = TokenWindow::at(parser);
    if window.first().token != Token::LeftBlock {
        return None;
    }
    let source = parser.full_text().inner();
    let segment_start = window.first().span.start;

    let (name_range, mut index) = head_name(window, 0, false)?;
    #[cfg(test)]
    add_segment_scan_token_visits(index);
    if !source[name_range].eq_ignore_ascii_case("iftags") {
        return None;
    }

    // Opener head: the spec runs until the head's `]]`.
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

    let legacy = parser.settings().layout.legacy();
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
        !parser.page_info().tags.is_empty()
    } else {
        ElementCondition::check(&conditions, &parser.page_info().tags)
    };

    // Body: scan for the matching closer, counting nested iftags
    // openers. Segments in raw fields are single-line; a line break
    // before the closer means this is not a segment of the field.
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

/// Consumes a complete iftags segment at the parser's current token,
/// stepping past its closer. Returns the segment when one was
/// consumed, or `None` (leaving the parser untouched) when the current
/// token does not open a complete segment.
pub(crate) fn consume_iftags_segment<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<IftagsSegment>, ParseError>
where
    'r: 't,
{
    consume_segment(parser, false)
}

/// Like `consume_iftags_segment`, but uses the fused scan, so a
/// segment whose closer abuts one more `]` is consumed as well.
/// The parser stops after the fused `RightLink` token; the bracket
/// it shared with the closer remains the caller's to account for
/// (the segment is marked `fused_closer`).
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

/// The source text of a closing bracket token, when the token is
/// one: `]` for `RightBracket`, `]]` for `RightBlock`, `]]]` for
/// `RightLink`.
pub(crate) fn bracket_close_text(token: Token) -> Option<&'static str> {
    match token {
        Token::RightBracket => Some("]"),
        Token::RightBlock => Some("]]"),
        Token::RightLink => Some("]]]"),
        _ => None,
    }
}

/// Completes a closing bracket run whose first bracket was the
/// leftover char of a fused segment closer (see
/// `iftags_segment_fused`): the parser sits just past the fused
/// `RightLink` token, and the bracket tokens that follow complete
/// the run when they spell the rest of `close_text` exactly.
///
/// Returns the run's final token and advances the parser past the
/// whole run. Returns `None`, leaving the parser untouched, when
/// the following tokens do not spell the run exactly (including a
/// run that would end inside a larger bracket token). Callers use
/// this for closing runs of two or more brackets; a one-bracket
/// close is already complete in the fused token's leftover char.
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
