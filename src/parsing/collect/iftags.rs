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

/// Gathers the block name starting at token `start` in `tokens`, where
/// `tokens[start]` is the opening token (`[[` or `[[/`). Returns the
/// name's source range and the index of the first token after the name.
fn head_name(
    tokens: &[&ExtractedToken<'_>],
    start: usize,
) -> Option<(Range<usize>, usize)> {
    let mut index = start + 1;
    while tokens.get(index)?.token == Token::Whitespace {
        index += 1;
    }
    let name_start = tokens.get(index)?.span.start;
    let mut name_end = name_start;
    loop {
        let token = tokens.get(index)?;
        match token.token {
            Token::Whitespace
            | Token::LineBreak
            | Token::ParagraphBreak
            | Token::RightBlock
            | Token::InputEnd => break,
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
    if parser.settings().layout.legacy() && parser.in_wikidot_iftags_span() {
        return None;
    }

    // `remaining()` starts after the current token, so the scan window
    // is the current token followed by the remaining ones.
    let tokens: Vec<&ExtractedToken<'t>> = std::iter::once(parser.current())
        .chain(parser.remaining().iter())
        .collect();
    let tokens = tokens.as_slice();
    if tokens.first()?.token != Token::LeftBlock {
        return None;
    }
    let source = parser.full_text().inner();
    let segment_start = tokens[0].span.start;

    let (name_range, mut index) = head_name(tokens, 0)?;
    if !source[name_range].eq_ignore_ascii_case("iftags") {
        return None;
    }

    // Opener head: the spec runs until the head's `]]`.
    let spec_start = tokens.get(index)?.span.start;
    let head_end;
    let spec_end;
    loop {
        let token = tokens.get(index)?;
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
    let body_start = tokens.get(index).map_or(head_end, |token| token.span.start);
    let mut depth = 1usize;
    let (body_end, closer_end) = loop {
        let token = tokens.get(index)?;
        match token.token {
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => return None,
            Token::LeftBlock => {
                if let Some((name_range, _)) = head_name(tokens, index)
                    && source[name_range].eq_ignore_ascii_case("iftags")
                {
                    depth += 1;
                }
            }
            Token::LeftBlockEnd => {
                if let Some((name_range, _)) = head_name(tokens, index)
                    && source[name_range].eq_ignore_ascii_case("iftags")
                {
                    depth -= 1;
                    if depth == 0 {
                        let closer_start = token.span.start;
                        let mut end_index = index;
                        let closer_end = loop {
                            let token = tokens.get(end_index)?;
                            match token.token {
                                Token::RightBlock => break token.span.end,
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
    let Some(segment) = iftags_segment(parser) else {
        return Ok(None);
    };
    while parser.current().span.start < segment.range.end
        && parser.current().token != Token::InputEnd
    {
        parser.step()?;
    }
    Ok(Some(segment))
}
