/*
 * parsing/rule/impls/block/blocks/iftags.rs
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
use crate::data::PageInfo;
use crate::delayed::DelayedElement;
use crate::parsing::rule::impls::block::parser::{BlockBodyStart, GateHeadClose};
use crate::parsing::{ElementCondition, ParagraphSplitEntry, ParagraphSplitKind};
use std::borrow::Cow;

pub const BLOCK_IFTAGS: BlockRule = BlockRule {
    name: "block-iftags",
    accepts_names: &["iftags"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn,
};

fn parse_conditions<'r, 't>(
    parser: &Parser<'r, 't>,
    spec: Option<&'t str>,
) -> Result<Vec<ElementCondition<'t>>, ParseError> {
    match spec {
        Some("-") if parser.settings().layout.legacy() => Ok(Vec::new()),
        Some(spec) if !parser.settings().layout.legacy() && spec.trim().is_empty() => {
            Err(parser.make_err(ParseErrorKind::BlockMissingArguments))
        }
        Some(spec) => Ok(ElementCondition::parse(spec)),
        None if parser.settings().layout.legacy() => Ok(Vec::new()),
        None => Err(parser.make_err(ParseErrorKind::BlockMissingArguments)),
    }
}

fn literal_span_gate<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
) -> ParseResult<'r, 't, Elements<'t>> {
    let text = parser.full_text().inner();
    let mut start = parser.current().span.start;
    let bytes = text.as_bytes();
    while start > 0 && matches!(bytes[start - 1], b' ' | b'\t') {
        start -= 1;
    }
    if start < name.len() || !text[start - name.len()..start].eq_ignore_ascii_case(name) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    start -= name.len();
    while start > 0 && matches!(bytes[start - 1], b' ' | b'\t') {
        start -= 1;
    }
    if start < 2 || &text[start - 2..start] != "[[" {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    start -= 2;

    loop {
        match parser.current().token {
            Token::RightBlock => {
                parser.step()?;
                break;
            }
            Token::InputEnd
            | Token::ParagraphBreak
            | Token::LeftBlock
            | Token::LeftBlockEnd => {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            _ => parser.step()?,
        };
    }

    let (pairing_end, _) = parser.wikidot_iftags_pairing_end();
    let designated = parser.wikidot_iftags_body_end();
    let mut line_breaks: Vec<(usize, usize, bool)> = Vec::new();
    match (pairing_end, designated) {
        (Some(pe), Some(d)) if pe >= d => {
            while parser.current().span.start < d
                && parser.current().token != Token::InputEnd
            {
                record_line_break(parser, &mut line_breaks);
                parser.step()?;
            }
        }
        (Some(pe), _) => loop {
            let token = parser.current();
            if token.token == Token::LeftBlockEnd && token.span.start == pe {
                while parser.current().token != Token::RightBlock
                    && parser.current().token != Token::InputEnd
                {
                    record_line_break(parser, &mut line_breaks);
                    parser.step()?;
                }
                if parser.current().token == Token::RightBlock {
                    record_line_break(parser, &mut line_breaks);
                    parser.step()?;
                }
                break;
            }
            if token.span.start > pe || token.token == Token::InputEnd {
                break;
            }
            record_line_break(parser, &mut line_breaks);
            parser.step()?;
        },
        _ => {}
    }

    let end = parser.current().span.start;
    let (elements, paragraph_split) =
        literal_unit_elements(text, start, end, &line_breaks);
    let mut success = crate::parsing::ParseSuccess::new(elements, Vec::new(), true);
    success.paragraph_split = paragraph_split;
    Ok(success)
}

fn record_line_break(
    parser: &Parser<'_, '_>,
    line_breaks: &mut Vec<(usize, usize, bool)>,
) {
    let token = parser.current();
    if matches!(token.token, Token::LineBreak | Token::ParagraphBreak) {
        line_breaks.push((
            token.span.start,
            token.span.end,
            token.token == Token::ParagraphBreak,
        ));
    }
}

fn literal_unit_elements<'t>(
    source: &'t str,
    start: usize,
    end: usize,
    line_breaks: &[(usize, usize, bool)],
) -> (Elements<'t>, Option<Vec<ParagraphSplitEntry>>) {
    if line_breaks.is_empty() {
        return (
            Elements::Single(Element::Text(Cow::Borrowed(&source[start..end]))),
            None,
        );
    }

    let mut elements = Vec::with_capacity(line_breaks.len() * 2 + 1);
    let mut counts: Vec<ParagraphSplitEntry> = Vec::new();
    let mut segment_count = 0_usize;
    let mut segment_split = false;
    let mut at_break_run_end = false;
    let mut chunk_start = start;
    for &(break_start, break_end, paragraph) in line_breaks {
        if break_start > chunk_start {
            elements.push(Element::Text(Cow::Borrowed(
                &source[chunk_start..break_start],
            )));
            segment_count += 1;
            at_break_run_end = false;
        }
        if paragraph {
            if at_break_run_end {
                counts.push((0, true, ParagraphSplitKind::Paragraph));
            } else {
                if !matches!(elements.last(), Some(Element::LineBreak)) {
                    elements.push(Element::LineBreak);
                    segment_count += 1;
                }
                counts.push((segment_count, true, ParagraphSplitKind::Paragraph));
                segment_count = 0;
                at_break_run_end = true;
            }
            segment_split = true;
        } else if !matches!(elements.last(), Some(Element::LineBreak)) {
            elements.push(Element::LineBreak);
            segment_count += 1;
            at_break_run_end = false;
        }
        chunk_start = break_end;
    }
    if chunk_start < end {
        elements.push(Element::Text(Cow::Borrowed(&source[chunk_start..end])));
        segment_count += 1;
    }
    counts.push((segment_count, true, ParagraphSplitKind::Paragraph));

    let paragraph_split = if segment_split && counts.len() >= 2 {
        Some(counts)
    } else {
        None
    };
    (Elements::Multiple(elements), paragraph_split)
}

fn gate_pre_break(source: &[u8], opener_start: usize) -> bool {
    let mut i = opener_start;
    let mut saw_quote = false;
    let mut saw_bare_whitespace = false;
    while i > 0 {
        match source[i - 1] {
            b' ' | b'\t' => {
                saw_bare_whitespace = true;
                i -= 1;
            }
            b'>' => {
                saw_quote = true;
                i -= 1;
            }
            _ => break,
        }
    }
    if saw_bare_whitespace && !saw_quote {
        return false;
    }
    if i == 0 || source[i - 1] != b'\n' {
        return false;
    }
    let mut line_end = i - 1;
    if line_end > 0 && source[line_end - 1] == b'\r' {
        line_end -= 1;
    }
    let mut line_start = line_end;
    while line_start > 0 && source[line_start - 1] != b'\n' {
        line_start -= 1;
    }
    source[line_start..line_end]
        .iter()
        .any(|byte| !matches!(byte, b' ' | b'\t' | b'>'))
}

fn opener_starts_line(parser: &Parser<'_, '_>, name: &str) -> bool {
    let text = parser.full_text().inner();
    let bytes = text.as_bytes();
    let mut start = parser.current().span.start;
    while start > 0 && matches!(bytes[start - 1], b' ' | b'\t') {
        start -= 1;
    }
    if start < name.len() || !text[start - name.len()..start].eq_ignore_ascii_case(name) {
        return false;
    }
    start -= name.len();
    while start > 0 && matches!(bytes[start - 1], b' ' | b'\t') {
        start -= 1;
    }
    if start < 2 || &text[start - 2..start] != "[[" {
        return false;
    }
    start -= 2;
    start == 0 || bytes[start - 1] == b'\n'
}

fn result_is_invisible(result: &ParseResult<'_, '_, Elements<'_>>) -> bool {
    let Ok(success) = result else {
        return false;
    };
    match &success.item {
        Elements::None => true,
        Elements::Single(element) => matches!(element, Element::Delayed(_)),
        Elements::Multiple(elements) => elements
            .iter()
            .all(|element| matches!(element, Element::Delayed(_))),
    }
}

fn spec_contains_complete_comment(spec: &str) -> bool {
    let mut pos = 0;
    while let Some(relative) = spec[pos..].find("[!--") {
        let open = pos + relative;
        if spec[open + "[!--".len()..].contains("--]") {
            return true;
        }
        pos = open + "[!--".len();
    }
    false
}

fn prepend_head_leftover<'r, 't>(
    result: ParseResult<'r, 't, Elements<'t>>,
) -> ParseResult<'r, 't, Elements<'t>> {
    match result {
        Ok(mut success) => {
            if let Some(counts) = &mut success.paragraph_split
                && let Some(first) = counts.first_mut()
            {
                first.0 += 1;
            }
            success.item = match success.item {
                Elements::Multiple(mut elements) => {
                    elements.insert(0, Element::Text(Cow::Borrowed("]")));
                    Elements::Multiple(elements)
                }
                Elements::Single(element) => {
                    Elements::Multiple(vec![Element::Text(Cow::Borrowed("]")), element])
                }
                Elements::None => Elements::Single(Element::Text(Cow::Borrowed("]"))),
            };
            Ok(success)
        }
        Err(error) => Err(error),
    }
}

impl<'r, 't> Parser<'r, 't>
where
    'r: 't,
{
    pub(crate) fn skip_discarded_iftags_segment(&mut self, same_line: bool) -> bool {
        if self.current().token != Token::LeftBlock {
            return false;
        }

        let mut probe = self.clone();
        if probe.step().is_err() {
            return false;
        }
        if probe.current().token == Token::Whitespace && probe.step().is_err() {
            return false;
        }

        let name_token = probe.current();
        if name_token.token != Token::Identifier
            || !name_token.slice.eq_ignore_ascii_case("iftags")
        {
            return false;
        }
        let spec_start = name_token.span.end;
        if probe.step().is_err() {
            return false;
        }

        let spec_end = loop {
            match probe.current().token {
                Token::RightBlock | Token::RightLink => break probe.current().span.start,
                Token::LineBreak
                | Token::ParagraphBreak
                | Token::InputEnd
                | Token::LeftBlock
                | Token::LeftBlockEnd => return false,
                _ => {
                    if probe.step().is_err() {
                        return false;
                    }
                }
            }
        };

        let spec = &self.full_text().inner()[spec_start..spec_end];
        if spec.trim_start().starts_with('*')
            || spec.contains("[!--")
            || spec.contains("[[")
        {
            return false;
        }

        let conditions = match parse_conditions(&probe, Some(spec)) {
            Ok(conditions) => conditions,
            Err(_) => return false,
        };
        let include_body = if conditions.is_empty() && self.settings().layout.legacy() {
            !self.page_info().tags.is_empty()
        } else {
            check_iftags(self.page_info(), &conditions)
        };
        if include_body {
            return false;
        }

        if probe.step().is_err() {
            return false;
        }
        let Some(pairing_end) = probe.wikidot_iftags_pairing_end().0 else {
            return false;
        };

        loop {
            if probe.current().span.start >= pairing_end {
                break;
            }
            if probe.current().token == Token::InputEnd
                || probe.at_hard_body_end_boundary()
                || (probe.discarding_hidden_body() && probe.at_hidden_body_boundary())
                || (same_line
                    && matches!(
                        probe.current().token,
                        Token::LineBreak | Token::ParagraphBreak
                    ))
            {
                return false;
            }
            if probe.step().is_err() {
                return false;
            }
        }
        if probe.current().span.start != pairing_end
            || probe.current().token != Token::LeftBlockEnd
            || probe.at_hard_body_end_boundary()
            || (probe.discarding_hidden_body() && probe.at_hidden_body_boundary())
        {
            return false;
        }

        if probe.step().is_err() {
            return false;
        }
        loop {
            match probe.current().token {
                Token::RightBlock | Token::RightLink => break,
                Token::InputEnd => return false,
                _ => {
                    if probe.step().is_err() {
                        return false;
                    }
                }
            }
        }
        if probe.step().is_err() {
            return false;
        }

        self.update(&probe);
        true
    }
}

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing iftags block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "IfTags doesn't allow star flag");
    assert!(!flag_score, "IfTags doesn't allow score flag");
    assert_block_name(&BLOCK_IFTAGS, name);

    let opener_at_line_start = opener_starts_line(parser, name);

    if parser.in_wikidot_iftags_span() {
        return literal_span_gate(parser, name);
    }

    let (conditions, body_start, head_close) = parser.get_gate_head_with_body_start(
        &BLOCK_IFTAGS,
        in_head,
        |parser, spec| {
            if let Some(spec) = spec
                && spec_contains_complete_comment(spec)
            {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            parse_conditions(parser, spec)
        },
    )?;
    let head_fused = matches!(head_close, GateHeadClose::Fused);

    let spaced_head = !head_fused
        && body_start == BlockBodyStart::Inline
        && parser.current().token == Token::Whitespace
        && matches!(
            parser
                .full_text()
                .inner()
                .as_bytes()
                .get(parser.current().span.end),
            Some(b'\n') | Some(b'\r'),
        );
    let whitespace_only_body = !head_fused
        && body_start == BlockBodyStart::Inline
        && parser.current().token == Token::Whitespace
        && matches!(
            parser.look_ahead(0),
            Some(next) if next.token == Token::LeftBlockEnd,
        );
    if !head_fused
        && body_start == BlockBodyStart::Inline
        && parser.current().token == Token::Whitespace
        && !spaced_head
        && !whitespace_only_body
    {
        parser.step()?;
    }

    let include_body = if conditions.is_empty() && parser.settings().layout.legacy() {
        !parser.page_info().tags.is_empty()
    } else {
        check_iftags(parser.page_info(), &conditions)
    };

    let legacy = parser.settings().layout.legacy();
    let mut designated_end = None;
    if !parser.discarding_hidden_body() && !parser.in_quote_body() {
        let (pairing, _) = parser.wikidot_iftags_pairing_end();
        match pairing {
            Some(end) => {
                designated_end = Some(end);
            }
            None => {
                if legacy {
                    if include_body || !parser.has_body_end_block(&BLOCK_IFTAGS) {
                        return Err(parser.make_err(ParseErrorKind::RuleFailed));
                    }
                } else if !parser.has_body_end_block(&BLOCK_IFTAGS) {
                    return Err(parser.make_err(ParseErrorKind::RuleFailed));
                }
            }
        }
    } else if legacy {
        if parser.discarding_hidden_body() {
            if !parser.has_two_body_end_blocks(&BLOCK_IFTAGS) {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
        } else if !parser.has_body_end_block(&BLOCK_IFTAGS) {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
    } else if !parser.discarding_hidden_body()
        && !parser.has_body_end_block(&BLOCK_IFTAGS)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let previous_body_end = parser.wikidot_iftags_body_end();
    let previous_span = parser.in_wikidot_iftags_span();
    if designated_end.is_some() {
        parser.set_wikidot_iftags_body_end(designated_end);
        parser.set_in_wikidot_iftags_span(true);
    }

    let result = if !include_body {
        trace!("Conditions failed, skipping hidden body");
        let hidden_start = parser.current().span.start;
        match parser
            .discard_body_elements_with_literal_quote_context(&BLOCK_IFTAGS, body_start)
        {
            Ok(_) => {
                let hidden_end = parser.current().span.start;
                let generated = parser.generated_in_range(hidden_start..hidden_end);
                let suppressed = Element::Delayed(DelayedElement::suppressed(&generated));
                let pre_break = !parser
                    .wikidot_body_closed_with_fused_close(&BLOCK_IFTAGS)
                    && gate_pre_break(
                        parser.full_text().inner().as_bytes(),
                        opener_start,
                    );
                let plain_break = parser.current().token == Token::LineBreak
                    && !parser.current().slice.starts_with('\\');
                if pre_break && (plain_break || parser.current().token == Token::Quote) {
                    let mut success = crate::parsing::ParseSuccess::new(
                        Elements::Single(suppressed),
                        Vec::new(),
                        true,
                    );
                    success.paragraph_split = Some(vec![
                        (1, true, ParagraphSplitKind::Paragraph),
                        (0, true, ParagraphSplitKind::Paragraph),
                    ]);
                    Ok(success)
                } else if pre_break
                    && !matches!(
                        parser.current().token,
                        Token::ParagraphBreak
                            | Token::InputEnd
                            | Token::LeftBlockEnd
                            | Token::LineBreak
                    )
                {
                    parser.mark_virtual_start_of_line();
                    ok!(true; Elements::Multiple(vec![Element::LineBreak, suppressed]), Vec::new())
                } else {
                    ok!(true; suppressed, Vec::new())
                }
            }
            Err(error) => Err(error),
        }
    } else if designated_end.is_some() && parser.native_blockquote_depth().is_none() {
        let head_line_break = body_start == BlockBodyStart::NextPhysicalLine
            && parser.current().token != Token::ParagraphBreak;
        let source_bytes = parser.full_text().inner().as_bytes();
        let at_line_start = {
            let mut i = opener_start;
            while i > 0 && matches!(source_bytes[i - 1], b' ' | b'\t') {
                i -= 1;
            }
            i == 0 || source_bytes[i - 1] == b'\n' || (i == 1 && source_bytes[0] == 0)
        };
        let prev_line_contentful = {
            let mut line_start = opener_start;
            while line_start > 0 && source_bytes[line_start - 1] != b'\n' {
                line_start -= 1;
            }
            let mut line_end = line_start;
            if line_end > 0 {
                line_end -= 1;
                if line_end > 0 && source_bytes[line_end - 1] == b'\r' {
                    line_end -= 1;
                }
            }
            let mut prev_start = line_end;
            while prev_start > 0 && source_bytes[prev_start - 1] != b'\n' {
                prev_start -= 1;
            }
            let line = &source_bytes[prev_start..line_end];
            let content_start = line
                .iter()
                .position(|byte| !matches!(byte, b' ' | b'\t'))
                .unwrap_or(line.len());
            let content_end = line
                .iter()
                .rposition(|byte| !matches!(byte, b' ' | b'\t'))
                .map_or(content_start, |index| index + 1);
            let trimmed = &line[content_start..content_end];
            line.iter().any(|byte| !matches!(byte, b' ' | b'\t'))
                && !(trimmed.starts_with(b"[!--") && trimmed.ends_with(b"--]"))
        };
        let lead_paragraph_break = head_line_break && at_line_start && {
            let mut line_end = opener_start.saturating_sub(1);
            if line_end > 0 && source_bytes[line_end - 1] == b'\r' {
                line_end -= 1;
            }
            let mut line_start = line_end;
            while line_start > 0 && source_bytes[line_start - 1] != b'\n' {
                line_start -= 1;
            }
            source_bytes[line_start..line_end]
                .iter()
                .any(|byte| !matches!(byte, b' ' | b'\t'))
        };
        let lead_line_break = head_line_break && !at_line_start;
        let outer_gate_items_allowed = parser.gate_items_allowed();
        match parser.get_body_element_segments_iftags(&BLOCK_IFTAGS) {
            Ok(body) => {
                let (
                    ((mut segments, mut segment_safe, mut segment_kinds), trailing),
                    errors,
                    paragraph_safe,
                ) = body.into();

                if head_fused && let Some(first) = segments.first_mut() {
                    first.insert(0, Element::Text(Cow::Borrowed("]")));
                }

                trace!(
                    "IfTags parsed {} conditions and {} body segments",
                    conditions.len(),
                    segments.len(),
                );

                trace!("Conditions passed, including elements");
                let fused_close =
                    parser.wikidot_body_closed_with_fused_close(&BLOCK_IFTAGS);
                let (closer_indented, tail_para_scan) = match designated_end {
                    Some(end) if !fused_close => {
                        let source = parser.full_text().inner().as_bytes();
                        let mut i = end;
                        while i > 0 && matches!(source[i - 1], b' ' | b'\t') {
                            i -= 1;
                        }
                        if i == end || i == 0 || source[i - 1] != b'\n' {
                            (false, false)
                        } else {
                            let mut j = i - 1;
                            if j > 0 && source[j - 1] == b'\r' {
                                j -= 1;
                            }
                            (true, j > 0 && source[j - 1] == b'\n')
                        }
                    }
                    _ => (false, false),
                };
                let insulated_tail_break = closer_indented
                    && !tail_para_scan
                    && segments.last().is_some_and(|segment| {
                        matches!(segment.last(), Some(Element::LineBreak))
                    });
                let insulated_tail_para = closer_indented
                    && segments.last().is_some_and(|segment| segment.is_empty())
                    && (tail_para_scan
                        || segments.iter().all(|segment| segment.is_empty()));
                let mut trail_elements: Vec<Element<'t>> = Vec::new();
                let mut trail_paragraph_break = false;
                if trailing {
                    if fused_close {
                        trail_elements.push(Element::LineBreak);
                    } else {
                        match parser.current().token {
                            Token::LineBreak
                                if !parser.current().slice.starts_with('\\') =>
                            {
                                parser.step()?;
                                trail_paragraph_break = true;
                            }
                            Token::ParagraphBreak
                            | Token::InputEnd
                            | Token::LeftBlockEnd => {}
                            _ => {
                                trail_elements.push(Element::LineBreak);
                            }
                        }
                    }
                }

                if insulated_tail_para
                    && parser.current().token == Token::LineBreak
                    && !parser.current().slice.starts_with('\\')
                {
                    segments.push(vec![
                        Element::Delayed(DelayedElement::typography_boundary()),
                        Element::LineBreak,
                    ]);
                    segment_safe.push(true);
                    segment_kinds.push(ParagraphSplitKind::Paragraph);
                    parser.step()?;
                } else if !trail_paragraph_break
                    && segments.last().is_some_and(|segment| segment.is_empty())
                    && parser.current().token == Token::LineBreak
                    && !parser.current().slice.starts_with('\\')
                {
                    parser.step()?;
                }

                if insulated_tail_break {
                    match parser.current().token {
                        Token::LineBreak if !parser.current().slice.starts_with('\\') => {
                            parser.step()?;
                            trail_elements.push(Element::LineBreak);
                        }
                        Token::ParagraphBreak | Token::InputEnd | Token::LeftBlockEnd
                            if parser.settings().layout.legacy() =>
                        {
                            trail_elements.push(Element::LineBreak);
                        }
                        _ => {}
                    }
                }

                if closer_indented {
                    parser.mark_virtual_start_of_line();
                }

                if lead_paragraph_break {
                    segments.insert(0, Vec::new());
                    segment_safe.insert(0, true);
                    segment_kinds.insert(0, ParagraphSplitKind::Paragraph);
                }
                if trail_paragraph_break {
                    segments.push(Vec::new());
                    segment_safe.push(true);
                    segment_kinds.push(ParagraphSplitKind::Paragraph);
                }
                if spaced_head && at_line_start {
                    if prev_line_contentful {
                        segments
                            .first_mut()
                            .expect("segments always has a current segment")
                            .insert(0, Element::LineBreak);
                    } else if opener_start == 0
                        && let Some(first) = segments.first_mut()
                        && matches!(first.first(), Some(Element::LineBreak))
                        && let Some(Element::Text(text)) = first.get_mut(1)
                    {
                        *text = Cow::Owned(format!(" {text}"));
                    }
                }

                if segments.len() == 1 && !lead_line_break && trail_elements.is_empty() {
                    let elements =
                        segments.into_iter().next().expect("single segment present");
                    let mut bounded = Vec::with_capacity(elements.len() + 2);
                    bounded.push(Element::Delayed(DelayedElement::typography_boundary()));
                    bounded.extend(elements);
                    bounded.push(Element::Delayed(DelayedElement::typography_boundary()));
                    ok!(paragraph_safe; Elements::Multiple(bounded), errors)
                } else {
                    let last_index = segments.len() - 1;
                    let mut batch = Vec::new();
                    let mut counts = Vec::with_capacity(segments.len());
                    batch.push(Element::Delayed(DelayedElement::typography_boundary()));
                    if lead_line_break {
                        batch.push(Element::LineBreak);
                    }
                    for (index, segment) in segments.into_iter().enumerate() {
                        let mut runs: Vec<(usize, bool)> = Vec::new();
                        if !outer_gate_items_allowed
                            && !segment_safe[index]
                            && segment_kinds[index] == ParagraphSplitKind::Paragraph
                        {
                            for element in &segment {
                                let safe = element.paragraph_safe();
                                match runs.last_mut() {
                                    Some((len, run_safe)) if *run_safe == safe => {
                                        *len += 1;
                                    }
                                    _ => runs.push((1, safe)),
                                }
                            }
                        }
                        if runs.len() <= 1 {
                            runs = vec![(segment.len(), segment_safe[index])];
                        }
                        let run_count = runs.len();
                        batch.extend(segment);
                        if index < last_index {
                            batch.push(Element::LineBreak);
                        }
                        for (run_index, (len, safe)) in runs.into_iter().enumerate() {
                            let mut count = len;
                            if index == 0 && run_index == 0 {
                                count += 1;
                                if lead_line_break {
                                    count += 1;
                                }
                            }
                            if index < last_index && run_index == run_count - 1 {
                                count += 1;
                            }
                            if index == last_index && run_index == run_count - 1 {
                                count += 1;
                                count += trail_elements.len();
                            }
                            counts.push((count, safe, segment_kinds[index]));
                        }
                    }
                    batch.extend(trail_elements);
                    batch.push(Element::Delayed(DelayedElement::typography_boundary()));
                    let mut success = crate::parsing::ParseSuccess::new(
                        Elements::Multiple(batch),
                        errors,
                        paragraph_safe,
                    );
                    success.paragraph_split = Some(counts);
                    Ok(success)
                }
            }
            Err(error) => Err(error),
        }
    } else if designated_end.is_some()
        && let Some(required_depth) = parser.native_blockquote_depth()
    {
        let result = gather_iftags_body_in_quote(parser, required_depth, body_start);
        if head_fused {
            prepend_head_leftover(result)
        } else {
            result
        }
    } else {
        match parser.get_body_elements_with_literal_quote_context(
            &BLOCK_IFTAGS,
            false,
            body_start,
        ) {
            Ok(body) => {
                let (mut elements, errors, paragraph_safe) = body.into();

                trace!(
                    "IfTags parsed {} conditions and {} elements",
                    conditions.len(),
                    elements.len(),
                );

                trace!("Conditions passed, including elements");
                if head_fused {
                    elements.insert(0, Element::Text(Cow::Borrowed("]")));
                }
                let mut bounded = Vec::with_capacity(elements.len() + 2);
                bounded.push(Element::Delayed(DelayedElement::typography_boundary()));
                bounded.extend(elements);
                bounded.push(Element::Delayed(DelayedElement::typography_boundary()));
                ok!(paragraph_safe; Elements::Multiple(bounded), errors)
            }
            Err(error) => Err(error),
        }
    };

    let result = if parser.wikidot_body_closed_with_fused_close(&BLOCK_IFTAGS) {
        match result {
            Ok(mut success) => {
                if let Some(counts) = &mut success.paragraph_split
                    && let Some(last) = counts.last_mut()
                {
                    last.0 += 1;
                }
                success.item = match success.item {
                    Elements::Multiple(mut elements) => {
                        elements.push(Element::Text(Cow::Borrowed("]")));
                        Elements::Multiple(elements)
                    }
                    Elements::Single(element) => Elements::Multiple(vec![
                        element,
                        Element::Text(Cow::Borrowed("]")),
                    ]),
                    Elements::None => Elements::Single(Element::Text(Cow::Borrowed("]"))),
                };
                Ok(success)
            }
            Err(error) => Err(error),
        }
    } else {
        result
    };

    if let Some(designated) = designated_end
        && result.is_ok()
        && parser.native_blockquote_depth().is_none()
        && !parser.discarding_hidden_body()
        && parser.current().token == Token::LeftBlockEnd
        && parser.current().span.start < designated
        && parser.at_body_end_boundary()
    {
        parser.push_pending_iftags_end_swallow(designated);
    }

    if designated_end.is_some() {
        parser.set_wikidot_iftags_body_end(previous_body_end);
        parser.set_in_wikidot_iftags_span(previous_span);
    }
    if opener_at_line_start && result_is_invisible(&result) {
        parser.mark_virtual_start_of_line();
    }
    result
}

fn flush_quote_gate_block<'t>(
    units: &mut Vec<crate::parsing::parser::QuoteEscapedUnit<'t>>,
    open: &mut Option<(usize, bool, crate::parsing::paragraph::ParagraphStack<'t>)>,
    required_depth: usize,
) {
    use crate::parsing::parser::QuoteEscapedUnit;
    use crate::tree::{AttributeMap, Container, ContainerType};

    if let Some((_, stripped, mut stack)) = open.take() {
        stack.pop_line_break();
        let mut elements = stack.into_elements();
        if !elements.is_empty() {
            let wraps = if stripped { required_depth } else { 0 };
            for _ in 0..wraps {
                elements = vec![Element::Container(Container::new(
                    ContainerType::Blockquote,
                    elements,
                    AttributeMap::new(),
                ))];
            }
            units.push(QuoteEscapedUnit::Elements(elements, false));
        }
    }
}

fn gather_iftags_body_in_quote<'r, 't>(
    parser: &mut Parser<'r, 't>,
    required_depth: usize,
    body_start: BlockBodyStart,
) -> ParseResult<'r, 't, Elements<'t>> {
    use crate::parsing::paragraph::ParagraphStack;
    use crate::parsing::parser::QuoteEscapedUnit;
    use crate::parsing::rule::impls::block::parser::{
        QuoteGateBoundary, QuoteGateSegmentKind,
    };

    let previous_cursor = parser.quote_body_cursor();
    match body_start {
        BlockBodyStart::NextPhysicalLine => {
            parser.install_quote_body_cursor_with_literal_residuals(required_depth);
        }
        BlockBodyStart::Inline => {
            parser.install_quote_body_cursor_at_current_line_with_literal_residuals(
                required_depth,
            );
        }
    }
    let gathered =
        parser.get_body_element_segments_iftags_in_quote(&BLOCK_IFTAGS, required_depth);
    parser.set_quote_body_cursor(previous_cursor);
    let body = gathered?;
    let ((gate_segments, _trailing), errors, _body_safe) = body.into();

    let mut in_run = Vec::new();
    let mut escaped = Vec::new();
    let mut seen_escape = false;
    for segment in gate_segments {
        if segment.kind != QuoteGateSegmentKind::InRun {
            seen_escape = true;
        }
        if seen_escape {
            escaped.push(segment);
        } else {
            in_run.push(segment);
        }
    }

    let escaped_present = !escaped.is_empty();
    if escaped_present {
        let mut units: Vec<QuoteEscapedUnit<'t>> = Vec::new();
        let mut open: Option<(usize, bool, ParagraphStack<'t>)> = None;
        for segment in escaped {
            match segment.kind {
                QuoteGateSegmentKind::EscapedPlain => {
                    flush_quote_gate_block(&mut units, &mut open, required_depth);
                    let mut elements = segment.elements;
                    while matches!(elements.last(), Some(Element::LineBreak)) {
                        elements.pop();
                    }
                    if !elements.is_empty() {
                        units.push(QuoteEscapedUnit::Elements(
                            elements,
                            segment.paragraph_safe,
                        ));
                    }
                }
                QuoteGateSegmentKind::EscapedQuote { depth, stripped } => {
                    let continues = segment.boundary == QuoteGateBoundary::SameRun
                        && matches!(
                            &open,
                            Some((open_depth, open_stripped, _))
                                if *open_depth == depth
                                    && *open_stripped == stripped
                        );
                    if !continues {
                        flush_quote_gate_block(&mut units, &mut open, required_depth);
                        open = Some((depth, stripped, ParagraphStack::new_wikidot()));
                    }
                    let stack = &mut open.as_mut().expect("open quote block present").2;
                    let safe = segment.paragraph_safe;
                    for element in segment.elements {
                        stack.push_element(element, safe);
                    }
                }
                QuoteGateSegmentKind::QuoteRow(depth) => {
                    flush_quote_gate_block(&mut units, &mut open, required_depth);
                    if !segment.elements.is_empty() {
                        units.push(QuoteEscapedUnit::Row(
                            depth,
                            segment.elements,
                            segment.paragraph_safe,
                        ));
                    }
                }
                QuoteGateSegmentKind::InRun => {
                    flush_quote_gate_block(&mut units, &mut open, required_depth);
                }
            }
        }
        flush_quote_gate_block(&mut units, &mut open, required_depth);
        for unit in units {
            parser.push_quote_escaped_unit(unit);
        }
    }

    let paragraph_safe = in_run.iter().all(|segment| segment.paragraph_safe);
    let in_run_all_empty = in_run.iter().all(|segment| segment.elements.is_empty());
    if in_run_all_empty && escaped_present {
        return ok!(paragraph_safe; Elements::None, errors);
    }
    if in_run.len() == 1 {
        let segment = in_run.into_iter().next().expect("single segment present");
        let mut bounded = Vec::with_capacity(segment.elements.len() + 2);
        bounded.push(Element::Delayed(DelayedElement::typography_boundary()));
        bounded.extend(segment.elements);
        bounded.push(Element::Delayed(DelayedElement::typography_boundary()));
        return ok!(paragraph_safe; Elements::Multiple(bounded), errors);
    }
    if in_run.is_empty() {
        return ok!(paragraph_safe; Elements::None, errors);
    }
    let last_index = in_run.len() - 1;
    let mut batch = Vec::new();
    let mut counts = Vec::with_capacity(in_run.len());
    batch.push(Element::Delayed(DelayedElement::typography_boundary()));
    for (index, segment) in in_run.into_iter().enumerate() {
        let mut count = segment.elements.len();
        if index == 0 {
            count += 1;
        }
        batch.extend(segment.elements);
        if index < last_index {
            batch.push(Element::LineBreak);
            count += 1;
        }
        if index == last_index {
            count += 1;
        }
        counts.push((count, segment.paragraph_safe, segment.split_kind));
    }
    batch.push(Element::Delayed(DelayedElement::typography_boundary()));
    let mut success = crate::parsing::ParseSuccess::new(
        Elements::Multiple(batch),
        errors,
        paragraph_safe,
    );
    success.paragraph_split = Some(counts);
    Ok(success)
}

pub fn check_iftags(info: &PageInfo, conditions: &[ElementCondition]) -> bool {
    trace!("Checking iftags");
    ElementCondition::check(conditions, &info.tags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};
    use std::borrow::Cow;

    #[test]
    fn iftags_no_arguments_follows_page_tags_and_conditions_check_page_tags() {
        let legacy = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tagless = PageInfo {
            tags: Vec::new(),
            ..PageInfo::dummy()
        };
        let tagged = PageInfo::dummy();
        assert!(!tagged.tags.is_empty());

        fn body_rendered(
            source: &str,
            info: &PageInfo,
            settings: &WikitextSettings,
        ) -> bool {
            let tokenization = crate::tokenize(source);
            let (tree, errors) = crate::parse(&tokenization, info, settings).into();
            assert!(errors.is_empty(), "{errors:?}");
            format!("{tree:?}").contains("body")
        }

        for source in ["[[iftags]]body[[/iftags]]", "[[iftags   ]]body[[/iftags]]"] {
            assert!(!body_rendered(source, &tagless, &legacy), "{source}");
            assert!(body_rendered(source, &tagged, &legacy), "{source}");
        }

        assert!(!body_rendered(
            "[[iftags +]]body[[/iftags]]",
            &tagless,
            &legacy
        ));
        assert!(!body_rendered(
            "[[iftags +]]body[[/iftags]]",
            &tagged,
            &legacy
        ));
        assert!(!body_rendered(
            "[[iftags -]]body[[/iftags]]",
            &tagless,
            &legacy
        ));
        assert!(body_rendered(
            "[[iftags -]]body[[/iftags]]",
            &tagged,
            &legacy
        ));

        let modern = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        for source in ["[[iftags]]body[[/iftags]]", "[[iftags   ]]body[[/iftags]]"] {
            let tokenization = crate::tokenize(source);
            let (_tree, errors) = crate::parse(&tokenization, &tagged, &modern).into();
            assert!(
                errors
                    .iter()
                    .any(|error| error.kind() == ParseErrorKind::BlockMissingArguments),
                "{source}: {errors:?}",
            );
        }

        let info = PageInfo {
            tags: vec![Cow::Borrowed("alpha"), Cow::Borrowed("gamma")],
            ..PageInfo::dummy()
        };
        let included = ElementCondition::parse("+alpha -beta");
        let excluded = ElementCondition::parse("+alpha -gamma");

        assert!(check_iftags(&info, &included));
        assert!(!check_iftags(&info, &excluded));
    }
}
