/*
 * parsing/rule/impls/block/blocks/gallery.rs
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
use crate::delayed::DelayedElement;
use crate::parsing::ParagraphSplitKind;
use crate::parsing::collect::{
    collect_text_resolved, collect_text_resolved_fused_tracked,
};
use crate::parsing::condition::ParseCondition;
use crate::tree::{
    Gallery, GalleryArgument, GalleryEntry, GallerySelection, gallery_head_end,
    parse_explicit_gallery_entries, parse_gallery_entry, parse_gallery_entry_owned,
    parse_gallery_head_arguments,
};
use std::borrow::Cow;

pub const BLOCK_GALLERY: BlockRule = BlockRule {
    name: "block-gallery",
    accepts_names: &["gallery"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn,
};

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    assert!(!flag_star, "Gallery doesn't allow star flag");
    assert!(!flag_score, "Gallery doesn't allow score flag");
    assert_block_name(&BLOCK_GALLERY, name);

    let source = parser.full_text().inner();
    if !gallery_owns_physical_line(
        source,
        opener_start,
        parser.in_native_blockquote_line(),
        parser.settings().layout.legacy(),
    ) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let word_start = opener_start
        + 2
        + source[opener_start + 2..]
            .bytes()
            .take_while(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
            .count();
    let rest = &source[word_start..];
    let prefix_matches = rest
        .get(..name.len())
        .is_some_and(|word| word.eq_ignore_ascii_case(name))
        && match rest.as_bytes().get(name.len()) {
            None => true,
            Some(byte) => matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | b']'),
        };
    if !prefix_matches {
        let cur_start = parser.current().span.start;
        let mut back = cur_start;
        while back > opener_start && matches!(source.as_bytes()[back - 1], b' ' | b'\t') {
            back -= 1;
        }
        let seam_positions = if in_head {
            seamed_head_end(parser).map(|head_end| (head_end, back))
        } else if back >= opener_start + 2 && &source[back - 2..back] == "]]" {
            Some((back, back - 2))
        } else {
            None
        };
        if let Some((head_end, name_raw_end)) = seam_positions {
            return parse_seamed_gallery(
                parser,
                name,
                opener_start,
                name_raw_end,
                head_end,
                in_head,
            );
        }
    }

    if parser.settings().layout.legacy() && in_head {
        let line_end = source[opener_start..]
            .find('\n')
            .map_or(source.len(), |offset| opener_start + offset);
        if source[opener_start + 2..line_end].contains("[[")
            && let Some(result) = parse_segmented_arg_gallery(
                parser,
                name,
                opener_start,
                word_start + name.len(),
                in_head,
            )
        {
            return result;
        }
    }

    let relative_head_end = gallery_head_end(&source[opener_start..])
        .ok_or_else(|| parser.make_err(ParseErrorKind::BlockMissingCloseBrackets))?;
    let head_end = opener_start + relative_head_end;
    if range_has_non_authored(parser, opener_start..head_end) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let wikidot = parser.settings().layout.legacy();
    let arguments =
        parse_gallery_head_arguments(&source[opener_start..head_end], wikidot)
            .ok_or_else(|| parser.make_err(ParseErrorKind::BlockMalformedArguments))?;
    finish_gallery(
        parser,
        opener_start,
        head_end,
        arguments,
        None,
        in_head,
        false,
    )
}

fn parse_segmented_arg_gallery<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    name_end: usize,
    in_head: bool,
) -> Option<ParseResult<'r, 't, Elements<'t>>> {
    let mut probe = parser.clone();
    while probe.current().span.start < name_end {
        if probe.current().token == Token::InputEnd {
            return None;
        }
        probe.step().ok()?;
    }
    let collect_start = probe.current().span.start;
    if collect_start < name_end {
        return None;
    }
    let closes = [ParseCondition::current(Token::RightBlock)];
    let invalids = [
        ParseCondition::current(Token::LineBreak),
        ParseCondition::current(Token::ParagraphBreak),
    ];
    let plain = collect_text_resolved(
        &mut probe,
        parser.rule(),
        &closes,
        &invalids,
        Some(ParseErrorKind::BlockMalformedArguments),
    );
    let (text, last) = match plain {
        Ok((text, last)) if !text.contains("[[") => (text, last),
        _ => {
            let mut probe = parser.clone();
            while probe.current().span.start < name_end {
                if probe.current().token == Token::InputEnd {
                    return None;
                }
                probe.step().ok()?;
            }
            match collect_text_resolved_fused_tracked(
                &mut probe,
                parser.rule(),
                &closes,
                &invalids,
                Some(ParseErrorKind::BlockMalformedArguments),
            ) {
                Ok((text, last, true)) if !text.contains("[[") => (text, last),
                _ => return None,
            }
        }
    };
    if !matches!(last.token, Token::RightBlock | Token::RightBracket) {
        return None;
    }
    let head_end = last.span.end;
    let source = parser.full_text().inner();
    if text.as_ref() == &source[collect_start..last.span.start] {
        return None;
    }
    if range_has_non_authored(parser, opener_start..head_end) {
        return Some(Err(parser.make_err(ParseErrorKind::RuleFailed)));
    }
    let mut resolved_head = String::with_capacity(head_end - opener_start);
    resolved_head.push_str("[[");
    resolved_head.push_str(name);
    resolved_head.push_str(&source[name_end..collect_start]);
    resolved_head.push_str(&text);
    resolved_head.push_str("]]");
    if gallery_head_end(&resolved_head) != Some(resolved_head.len()) {
        return Some(Err(parser.make_err(ParseErrorKind::RuleFailed)));
    }
    let arguments: Vec<GalleryArgument<'static>> =
        match parse_gallery_head_arguments(&resolved_head, true) {
            Some(arguments) => arguments.iter().map(GalleryArgument::to_owned).collect(),
            None => {
                return Some(Err(
                    parser.make_err(ParseErrorKind::BlockMalformedArguments)
                ));
            }
        };
    Some(finish_gallery(
        parser,
        opener_start,
        head_end,
        arguments,
        Some(resolved_head),
        in_head,
        true,
    ))
}

fn seamed_head_end(parser: &Parser<'_, '_>) -> Option<usize> {
    for token in std::iter::once(parser.current()).chain(parser.remaining()) {
        match token.token {
            Token::RightBlock => return Some(token.span.end),
            Token::InputEnd => return None,
            Token::LeftBlock
            | Token::LeftBlockStar
            | Token::LeftBlockEnd
            | Token::LeftBlockAnchor
            | Token::LeftComment
            | Token::LeftLink
            | Token::LeftLinkStar
            | Token::RightLink => return None,
            _ => {}
        }
    }
    None
}

fn parse_seamed_gallery<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    name_raw_end: usize,
    head_end: usize,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    let source = parser.full_text().inner();
    if range_has_non_authored(parser, opener_start..head_end) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let mut resolved_head = String::with_capacity(head_end - opener_start);
    resolved_head.push_str("[[");
    resolved_head.push_str(name);
    resolved_head.push_str(&source[name_raw_end..head_end - 2]);
    resolved_head.push_str("]]");
    if gallery_head_end(&resolved_head) != Some(resolved_head.len()) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let wikidot = parser.settings().layout.legacy();
    let arguments: Vec<GalleryArgument<'static>> =
        parse_gallery_head_arguments(&resolved_head, wikidot)
            .ok_or_else(|| parser.make_err(ParseErrorKind::BlockMalformedArguments))?
            .iter()
            .map(GalleryArgument::to_owned)
            .collect();
    finish_gallery(
        parser,
        opener_start,
        head_end,
        arguments,
        Some(resolved_head),
        in_head,
        false,
    )
}

fn finish_gallery<'r, 't>(
    parser: &mut Parser<'r, 't>,
    opener_start: usize,
    head_end: usize,
    arguments: Vec<GalleryArgument<'t>>,
    resolved_head: Option<String>,
    in_head: bool,
    known_unfused: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    let source = parser.full_text().inner();
    let snapshot = parser.clone();
    let _ = parser.get_head_map_with_body_start_wikidot(&BLOCK_GALLERY, in_head)?;

    let wikidot = parser.settings().layout.legacy();
    let residual_opener_bracket = source[head_end..].starts_with(']');
    let fused_opener = !known_unfused
        && (residual_opener_bracket || source[opener_start..head_end].ends_with("]]]"));
    let raw_explicit = parse_explicit_gallery_entries(source, head_end).map(
        |(entries, owner_end, residual)| (entries, owner_end, residual, None, Vec::new()),
    );
    let mut explicit = if wikidot && fused_opener {
        None
    } else {
        raw_explicit.or_else(|| parse_resolved_gallery_entries(parser, source, head_end))
    };
    if wikidot
        && !fused_opener
        && let Some((_, owner_end, _, _, _)) = &explicit
    {
        let owner_end = *owner_end;
        if gallery_body_has_seamed_entries(source, head_end, owner_end)
            && let Some(resolved) =
                parse_resolved_gallery_entries(parser, source, head_end)
        {
            explicit = Some(resolved);
        }
    }
    if let Some((entries, owner_end, residual_close_bracket, closer_fix, splices)) =
        explicit
    {
        let non_authored = range_has_non_authored(parser, head_end..owner_end);
        if resolved_head.is_some() && non_authored {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
        let generated =
            non_authored.then(|| parser.generated_in_range(opener_start..owner_end));
        if closer_fix.is_none() || !non_authored {
            let mut after_owner = parser.clone();
            while after_owner.current().span.start < owner_end {
                if after_owner.current().token == Token::InputEnd {
                    return Err(parser.make_err(ParseErrorKind::EndOfInput));
                }
                after_owner.step()?;
            }
            parser.update(&after_owner);
            let element = if let Some(generated) = generated {
                Element::Delayed(DelayedElement::shell(
                    source,
                    opener_start..owner_end,
                    &generated,
                ))
            } else {
                Element::Gallery(Gallery::new_cow(
                    gallery_stored_source(
                        source,
                        opener_start,
                        head_end,
                        owner_end,
                        resolved_head.as_deref(),
                        closer_fix,
                        &splices,
                    ),
                    arguments,
                    GallerySelection::Explicit(entries),
                ))
            };
            if residual_close_bracket && wikidot {
                parser.mark_wikidot_mixed_paragraph_ownership();
            }
            let residual_count = usize::from(residual_opener_bracket)
                + usize::from(residual_close_bracket);
            return if residual_count == 0 {
                success_elements(element)
            } else {
                ok!(Elements::Multiple(vec![
                    element,
                    Element::Text(Cow::Owned("]".repeat(residual_count))),
                ]))
            };
        }
    }

    let mut entries_consumed = false;
    if wikidot && fused_opener {
        normalize_degraded_position(parser, &snapshot, source, head_end);
        entries_consumed = consume_fused_gallery_entries(parser, source, head_end);
    }

    let gallery = Element::Gallery(Gallery::new_cow(
        gallery_stored_source(
            source,
            opener_start,
            head_end,
            head_end,
            resolved_head.as_deref(),
            None,
            &[],
        ),
        arguments,
        GallerySelection::CurrentPageFiles,
    ));
    let channel_residual = parser
        .fused_name_residual()
        .is_some_and(|position| position > opener_start && position <= head_end);
    if residual_opener_bracket || channel_residual {
        if parser
            .fused_name_residual()
            .is_some_and(|position| position > opener_start && position <= head_end + 1)
        {
            parser.clear_fused_name_residual();
        }
        let bracket_end = if residual_opener_bracket {
            head_end + 1
        } else {
            head_end
        };
        let mut elements = vec![gallery, text!("]")];
        if matches!(source.as_bytes().get(bracket_end), Some(b'\r' | b'\n'))
            && !gallery_following_line_blank(source, bracket_end)
        {
            elements.push(Element::LineBreak);
            if entries_consumed {
                elements.push(text!(""));
                let count = elements.len();
                let mut success =
                    ParseSuccess::new(Elements::Multiple(elements), Vec::new(), false);
                success.paragraph_split = Some(vec![
                    (count, false, ParagraphSplitKind::Paragraph),
                    (0, true, ParagraphSplitKind::Paragraph),
                ]);
                return Ok(success);
            }
            parser.mark_wikidot_mixed_paragraph_ownership();
        } else if wikidot && gallery_line_continues(source, bracket_end) {
            parser.mark_wikidot_mixed_paragraph_ownership();
        }
        ok!(Elements::Multiple(elements))
    } else {
        success_elements(gallery)
    }
}

fn gallery_following_line_blank(source: &str, terminator: usize) -> bool {
    let bytes = source.as_bytes();
    let mut index = terminator;
    if bytes.get(index) == Some(&b'\r') && bytes.get(index + 1) == Some(&b'\n') {
        index += 2;
    } else {
        index += 1;
    }
    matches!(bytes.get(index), Some(b'\r' | b'\n'))
}

fn gallery_line_continues(source: &str, start: usize) -> bool {
    source[start..]
        .lines()
        .next()
        .is_some_and(|line| !line.trim().is_empty())
}

fn normalize_degraded_position<'r, 't>(
    parser: &mut Parser<'r, 't>,
    snapshot: &Parser<'r, 't>,
    source: &str,
    head_end: usize,
) {
    let body_start = source[head_end..]
        .find('\n')
        .map_or(source.len(), |offset| head_end + offset + 1);
    let current = parser.current();
    if current.span.start == body_start
        || (body_start == source.len() && current.token == Token::InputEnd)
        || current.span.start < body_start
    {
        return;
    }
    let mut probe = snapshot.clone();
    while probe.current().span.start < body_start {
        if probe.current().token == Token::InputEnd {
            return;
        }
        if probe.step().is_err() {
            return;
        }
    }
    parser.update(&probe);
}

fn consume_fused_gallery_entries(
    parser: &mut Parser<'_, '_>,
    source: &str,
    head_end: usize,
) -> bool {
    let Some(first_break) = source[head_end..].find('\n') else {
        return false;
    };
    let body_start = head_end + first_break + 1;
    let mut cursor = body_start;
    loop {
        let line_end = source[cursor..]
            .find('\n')
            .map_or(source.len(), |offset| cursor + offset);
        let line = source[cursor..line_end]
            .strip_suffix('\r')
            .unwrap_or(&source[cursor..line_end]);
        if parse_gallery_entry(line).is_none() {
            break;
        }
        cursor = line_end;
        if line_end == source.len() {
            break;
        }
        cursor = line_end + 1;
    }
    if cursor == body_start {
        return false;
    }
    loop {
        let line_end = source[cursor..]
            .find('\n')
            .map_or(source.len(), |offset| cursor + offset);
        let line = source[cursor..line_end]
            .strip_suffix('\r')
            .unwrap_or(&source[cursor..line_end]);
        if !line.trim().is_empty() {
            break;
        }
        cursor = line_end;
        if line_end == source.len() {
            break;
        }
        cursor = line_end + 1;
    }
    let mut probe = parser.clone();
    while probe.current().span.start < cursor {
        if probe.current().token == Token::InputEnd {
            return false;
        }
        if probe.step().is_err() {
            return false;
        }
    }
    parser.update(&probe);
    true
}

type ExplicitGallery<'t> = (
    Vec<GalleryEntry<'t>>,
    usize,
    bool,
    Option<(usize, usize, bool)>,
    Vec<(usize, usize, String)>,
);

fn parse_resolved_gallery_entries<'r: 't, 't>(
    parser: &Parser<'r, 't>,
    source: &'t str,
    head_end: usize,
) -> Option<ExplicitGallery<'t>> {
    let wikidot = parser.settings().layout.legacy();
    let mut cursor = if source[head_end..].starts_with("\r\n") {
        head_end + 2
    } else if source[head_end..].starts_with('\n') {
        head_end + 1
    } else {
        return None;
    };
    let mut entries = Vec::new();
    let mut splices: Vec<(usize, usize, String)> = Vec::new();

    loop {
        let line_start = cursor;
        let line_end = source[cursor..]
            .find('\n')
            .map_or(source.len(), |offset| cursor + offset);
        let line = source[line_start..line_end]
            .strip_suffix('\r')
            .unwrap_or(&source[line_start..line_end]);
        let trimmed = line.trim_matches([' ', '\t']);
        let residual = trimmed
            .get(.."[[/gallery]]]".len())
            .is_some_and(|close| close.eq_ignore_ascii_case("[[/gallery]]]"))
            && trimmed.len() == "[[/gallery]]]".len();
        let normal = trimmed.eq_ignore_ascii_case("[[/gallery]]");
        if normal || residual {
            if entries.is_empty() {
                return None;
            }
            let owner_end = if residual {
                line_start + line.len() - 1
            } else {
                line_end
            };
            return Some((entries, owner_end, residual, None, splices));
        }
        if trimmed.starts_with("[[/") {
            if entries.is_empty() {
                return None;
            }
            let (owner_end, residual, closer_fix) =
                resolved_gallery_closer(parser, source, line_start, line, line_end)?;
            return Some((entries, owner_end, residual, Some(closer_fix), splices));
        }

        if wikidot && gallery_comment_only_line(line) {
            splices.push((line_start, (line_end + 1).min(source.len()), String::new()));
            if line_end == source.len() {
                return None;
            }
            cursor = line_end + 1;
            continue;
        }

        let entry = match parse_gallery_entry(line) {
            Some(entry) if !(wikidot && gallery_entry_line_has_seam(line)) => Some(entry),
            _ => resolve_gallery_entry(parser, line, line_start, line_end, &mut splices)
                .or_else(|| parse_gallery_entry(line)),
        };
        entries.push(entry?);
        if line_end == source.len() {
            return None;
        }
        cursor = line_end + 1;
    }
}

fn gallery_comment_only_line(line: &str) -> bool {
    let mut rest = line.trim();
    let mut found = false;
    while let Some(start) = rest.find("[!--") {
        if !rest[..start].trim().is_empty() {
            return false;
        }
        let Some(end) = rest[start..].find("--]") else {
            return false;
        };
        found = true;
        rest = rest[start + end + 3..].trim_start();
    }
    found && rest.is_empty()
}

fn gallery_entry_source_span(line: &str) -> Option<(usize, usize)> {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() && matches!(bytes[index], b' ' | b'\t') {
        index += 1;
    }
    if bytes.get(index) != Some(&b':') {
        return None;
    }
    index += 1;
    while index < bytes.len() && matches!(bytes[index], b' ' | b'\t') {
        index += 1;
    }
    let start = index;
    let first = *bytes.get(start)?;
    if matches!(first, b'\'' | b'"') {
        let mut end = start + 1;
        while end < bytes.len() && bytes[end] != first {
            end += 1;
        }
        if end >= bytes.len() {
            return None;
        }
        return Some((start, end + 1));
    }
    let mut end = start;
    while end < bytes.len() {
        if matches!(bytes[end], b' ' | b'\t') {
            break;
        }
        if line[end..].starts_with("[[")
            && let Some(segment_end) = gallery_segment_span_end(line, end)
        {
            end = segment_end;
            continue;
        }
        end += line[end..].chars().next()?.len_utf8();
    }
    Some((start, end))
}

fn gallery_segment_span_end(line: &str, start: usize) -> Option<usize> {
    let lowered = line[start..].to_ascii_lowercase();
    let close = lowered.find("[[/iftags]]")?;
    Some(start + close + "[[/iftags]]".len())
}

fn gallery_entry_line_has_seam(line: &str) -> bool {
    let Some((start, end)) = gallery_entry_source_span(line) else {
        return false;
    };
    let span = &line[start..end];
    span.contains("[!--") || span.contains("[[")
}

fn resolve_gallery_entry<'r: 't, 't>(
    parser: &Parser<'r, 't>,
    line: &'t str,
    line_start: usize,
    line_end: usize,
    splices: &mut Vec<(usize, usize, String)>,
) -> Option<GalleryEntry<'t>> {
    if !parser.settings().layout.legacy() {
        return None;
    }
    let (span_start, span_end) = gallery_entry_source_span(line)?;
    if matches!(line.as_bytes()[span_start], b'\'' | b'"') {
        return None;
    }
    let token_start = line_start + span_start;
    let mut probe = parser.clone();
    while probe.current().span.start < token_start {
        if probe.current().token == Token::InputEnd {
            return None;
        }
        probe.step().ok()?;
    }
    if probe.current().span.start != token_start {
        return None;
    }
    let closes = [
        ParseCondition::current(Token::Whitespace),
        ParseCondition::current(Token::LineBreak),
    ];
    let invalids = [ParseCondition::current(Token::ParagraphBreak)];
    let (resolved, _, _) = collect_text_resolved_fused_tracked(
        &mut probe,
        parser.rule(),
        &closes,
        &invalids,
        None,
    )
    .ok()?;
    if resolved.is_empty()
        || resolved
            .chars()
            .any(|ch| matches!(ch, ' ' | '\t' | '\r' | '\n'))
    {
        return None;
    }
    let mut resolved_line = String::with_capacity(line.len());
    resolved_line.push_str(&line[..span_start]);
    resolved_line.push_str(&resolved);
    resolved_line.push_str(&line[span_end..]);
    if resolved_line == line {
        return parse_gallery_entry(line);
    }
    let entry = parse_gallery_entry_owned(resolved_line.clone())?;
    splices.push((line_start, line_end, resolved_line));
    Some(entry)
}

fn gallery_body_has_seamed_entries(
    source: &str,
    head_end: usize,
    owner_end: usize,
) -> bool {
    let mut cursor = if source[head_end..].starts_with("\r\n") {
        head_end + 2
    } else if source[head_end..].starts_with('\n') {
        head_end + 1
    } else {
        return false;
    };
    while cursor < owner_end {
        let line_end = source[cursor..]
            .find('\n')
            .map_or(source.len(), |offset| cursor + offset);
        let line = source[cursor..line_end]
            .strip_suffix('\r')
            .unwrap_or(&source[cursor..line_end]);
        if parse_gallery_entry(line).is_some() && gallery_entry_line_has_seam(line) {
            return true;
        }
        if line_end == source.len() {
            break;
        }
        cursor = line_end + 1;
    }
    false
}

fn resolved_gallery_closer<'r: 't, 't>(
    parser: &Parser<'r, 't>,
    source: &str,
    line_start: usize,
    line: &str,
    line_end: usize,
) -> Option<(usize, bool, (usize, usize, bool))> {
    let leading = line.len() - line.trim_start_matches([' ', '\t']).len();
    let content_start = line_start + leading;
    let mut probe = parser.clone();
    while probe.current().span.start < content_start {
        if probe.current().token == Token::InputEnd {
            return None;
        }
        probe.step().ok()?;
    }
    if probe.current().token != Token::LeftBlockEnd
        || probe.current().span.start != content_start
    {
        return None;
    }
    let name = probe.get_end_block().ok()?;
    if !name.eq_ignore_ascii_case("gallery") {
        return None;
    }
    let closer_end = probe.current().span.start;
    let rest = source[closer_end..line_end].trim_matches([' ', '\t', '\r']);
    if rest.is_empty() {
        Some((line_end, false, (content_start, closer_end, true)))
    } else if rest == "]" {
        Some((closer_end, true, (content_start, closer_end, false)))
    } else {
        None
    }
}

fn gallery_stored_source<'s>(
    source: &'s str,
    opener_start: usize,
    head_end: usize,
    owner_end: usize,
    resolved_head: Option<&str>,
    closer_fix: Option<(usize, usize, bool)>,
    splices: &[(usize, usize, String)],
) -> Cow<'s, str> {
    if resolved_head.is_none() && closer_fix.is_none() && splices.is_empty() {
        return Cow::Borrowed(&source[opener_start..owner_end]);
    }
    let mut stored = String::with_capacity(owner_end - opener_start + 2);
    match resolved_head {
        Some(head) => stored.push_str(head),
        None => stored.push_str(&source[opener_start..head_end]),
    }
    if owner_end > head_end {
        match closer_fix {
            Some((content_start, closer_end, normal)) => {
                push_spliced(&mut stored, source, head_end, content_start, splices);
                stored.push_str("[[/gallery]]");
                if normal {
                    stored.push_str(&source[closer_end..owner_end]);
                }
            }
            None => push_spliced(&mut stored, source, head_end, owner_end, splices),
        }
    }
    Cow::Owned(stored)
}

fn push_spliced(
    stored: &mut String,
    source: &str,
    start: usize,
    end: usize,
    splices: &[(usize, usize, String)],
) {
    let mut cursor = start;
    for (splice_start, splice_end, replacement) in splices {
        if *splice_start < start || *splice_end > end || *splice_start < cursor {
            continue;
        }
        stored.push_str(&source[cursor..*splice_start]);
        stored.push_str(replacement);
        cursor = *splice_end;
    }
    stored.push_str(&source[cursor..end]);
}

fn range_has_non_authored(
    parser: &Parser<'_, '_>,
    range: std::ops::Range<usize>,
) -> bool {
    parser.has_generated_in_range(range.clone())
        || std::iter::once(parser.current())
            .chain(parser.remaining())
            .take_while(|token| token.span.start < range.end)
            .any(|token| {
                token.span.end > range.start
                    && matches!(
                        token.token,
                        Token::RuntimeText
                            | Token::GeneratedPageLink
                            | Token::GeneratedTagLinks
                    )
            })
}

fn gallery_owns_physical_line(
    source: &str,
    opener_start: usize,
    quoted: bool,
    wikidot: bool,
) -> bool {
    let line_start = source[..opener_start]
        .rfind(['\n', '\r'])
        .map_or(0, |newline| newline + 1);
    let prefix = &source[line_start..opener_start];
    if prefix.is_empty()
        || !wikidot
            && prefix
                .bytes()
                .all(|byte| matches!(byte, b' ' | b'\t' | b'\0'))
    {
        return true;
    }
    if !quoted {
        return false;
    }
    prefix
        .trim_matches([' ', '\t'])
        .bytes()
        .all(|byte| byte == b'>')
}

pub(super) fn authored_gallery_line_offset(source: &str, wikidot: bool) -> Option<usize> {
    let mut line_start = 0;
    loop {
        let line_end = source[line_start..]
            .find('\n')
            .map_or(source.len(), |offset| line_start + offset);
        let line = source[line_start..line_end]
            .strip_suffix('\r')
            .unwrap_or(&source[line_start..line_end]);
        let leading = line.len() - line.trim_start_matches([' ', '\t']).len();
        let candidate = &line[leading..];
        if candidate.get(..2).is_some_and(|prefix| prefix == "[[")
            && gallery_head_end(candidate)
                .and_then(|end| parse_gallery_head_arguments(&candidate[..end], wikidot))
                .is_some()
        {
            return Some(line_start + leading);
        }
        if line_end == source.len() {
            return None;
        }
        line_start = line_end + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::rule::impls::block::RULE_BLOCK;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn gallery_line_detection_requires_a_complete_authored_head() {
        assert_eq!(
            authored_gallery_line_offset("x\n[[Gallery size=\"small\"]]", false),
            Some(2)
        );
        assert_eq!(authored_gallery_line_offset("A[[gallery]]B", false), None);
        assert_eq!(authored_gallery_line_offset("+ [[gallery]]", false), None);
        assert_eq!(authored_gallery_line_offset("[[gallery", false), None);
    }

    #[test]
    fn dense_non_line_gallery_candidates_stay_bounded() {
        let source = "A[[gallery]]B\n".repeat(2_048);
        let tokenization = crate::tokenize(&source);
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        while parser.current().token != Token::InputEnd {
            if parser.current().token == Token::LeftBlock {
                let _ = RULE_BLOCK.try_consume(&mut parser);
            }
            parser.step().expect("input end remains available");
        }
    }
}
