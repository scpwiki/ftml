/*
 * parsing/rule/impls/block/blocks/div.rs
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

use super::BLOCK_SPAN;
use super::prelude::*;
use super::{
    consume_wikidot_flat_head_break, wikidot_flat_pair_for_head, wikidot_scan_pairs_head,
};
use crate::delayed::DelayedElement;
use crate::parsing::collect::{consume_valid_comment, elide_comments_from_literal};
use crate::parsing::consume::consume;
use crate::parsing::parser::block_pairs::WikidotBlockKind;
use crate::parsing::rule::impls::block::parser::BlockBodyStart;
use crate::settings::WikitextMode;
use crate::tree::{
    AcceptsPartial, AttributeMap, ListItem, ListType, PartialElement, WikidotBlockShell,
};
use std::borrow::Cow;

const WIKIJUMP_DIV_FAILURE_CACHE: &str = "wikijump-div-parse-failure";

pub const BLOCK_DIV: BlockRule = BlockRule {
    name: "block-div",
    accepts_names: &["div"],
    accepts_star: false,
    accepts_score: true,
    accepts_newlines_outside_body: true,
    parse_fn,
};

fn wikidot_div_head_started_physical_line(
    parser: &Parser<'_, '_>,
    body_start: BlockBodyStart,
) -> bool {
    let source = parser.full_text().inner();
    let head_end = parser.current().span.start;
    let head_line_end = if body_start == BlockBodyStart::NextPhysicalLine {
        source[..head_end].trim_end_matches(['\r', '\n']).len()
    } else {
        head_end
    };
    let line_start = source[..head_line_end]
        .rfind('\n')
        .map_or(0, |offset| offset + 1);
    let physical_line_end = if body_start == BlockBodyStart::NextPhysicalLine {
        head_line_end
    } else {
        source[head_end..]
            .find('\n')
            .map_or(source.len(), |offset| head_end + offset)
    };
    wikidot_line_head_resolves_to_div(&source[line_start..physical_line_end])
}

fn wikidot_line_head_resolves_to_div(line: &str) -> bool {
    let trimmed = line.trim_start_matches([' ', '\t']);
    if trimmed
        .get(..5)
        .is_some_and(|head| head.eq_ignore_ascii_case("[[div"))
    {
        return true;
    }
    let mut rest = trimmed;
    if !rest.starts_with("[[") {
        return false;
    }
    rest = &rest[2..];
    let mut name = String::new();
    loop {
        if let Some(after) = rest.strip_prefix("[!--") {
            let Some(end) = after.find("--]") else {
                return false;
            };
            rest = &after[end + 3..];
            continue;
        }
        if rest.starts_with("[[") {
            let inner = &rest[2..];
            let keyword = ["iftags", "ifcategory"].into_iter().find(|kw| {
                inner.len() > kw.len()
                    && inner[..kw.len()].eq_ignore_ascii_case(kw)
                    && !matches!(inner.as_bytes()[kw.len()], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.')
            });
            let Some(keyword) = keyword else {
                return false;
            };
            let closer = if keyword == "iftags" {
                "[[/iftags]]"
            } else {
                "[[/ifcategory]]"
            };
            let Some(head_end) = inner.find("]]") else {
                return false;
            };
            let after_head = &inner[head_end + 2..];
            let Some(close_pos) = find_ascii_case_insensitive(after_head, closer) else {
                return false;
            };
            rest = &after_head[close_pos + closer.len()..];
            continue;
        }
        let mut chars = rest.chars();
        match chars.next() {
            Some(ch) if ch.is_alphanumeric() || matches!(ch, '-' | '.') => {
                name.push(ch);
                rest = chars.as_str();
            }
            _ => break,
        }
    }
    if !name.eq_ignore_ascii_case("div") {
        return false;
    }
    let rest = rest.strip_prefix(['_', '=']).unwrap_or(rest);
    let Some(head_end) = rest.find("]]") else {
        return false;
    };
    rest[head_end + 2..]
        .chars()
        .all(|ch| matches!(ch, ' ' | '\t'))
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let hay = haystack.as_bytes();
    let needle = needle.as_bytes();
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
}

fn wikidot_div_follows_inline_structural_close(
    parser: &Parser<'_, '_>,
    body_start: BlockBodyStart,
) -> bool {
    if parser.accepts_partial() != AcceptsPartial::ListItem {
        return false;
    }
    let source = parser.full_text().inner();
    let head_end = parser.current().span.start;
    let literal_end = if body_start == BlockBodyStart::NextPhysicalLine {
        source[..head_end].trim_end_matches(['\r', '\n']).len()
    } else {
        head_end
    };
    let line_start = source[..literal_end]
        .rfind('\n')
        .map_or(0, |offset| offset + 1);
    let opener_start = source[line_start..literal_end]
        .rfind("[[")
        .map_or(line_start, |offset| line_start + offset);
    let prefix = source[line_start..opener_start].trim_end();
    prefix
        .get(prefix.len().saturating_sub(8)..)
        .is_some_and(|close| close.eq_ignore_ascii_case("[[/div]]"))
        || prefix
            .get(prefix.len().saturating_sub(7)..)
            .is_some_and(|close| close.eq_ignore_ascii_case("[[/ul]]"))
}

fn wikidot_div_is_inline_scored_line_literal(
    parser: &Parser<'_, '_>,
    arguments_empty: bool,
    body_start: BlockBodyStart,
    owner_start: usize,
    flag_score: bool,
) -> bool {
    if !flag_score
        || !arguments_empty
        || body_start != BlockBodyStart::Inline
        || parser.current().token == Token::LeftBlockEnd
    {
        return false;
    }
    let source = parser.full_text().inner();
    let line_start = source[..owner_start]
        .rfind('\n')
        .map_or(0, |offset| offset + 1);
    source[line_start..owner_start]
        .trim_matches([' ', '\t'])
        .is_empty()
}

fn wikidot_next_line_opens_div(source: &str, mut position: usize) -> bool {
    let bytes = source.as_bytes();
    while matches!(bytes.get(position), Some(b'\n' | b'\r')) {
        position += 1;
    }
    let line_end = source[position..]
        .find('\n')
        .map_or(source.len(), |offset| position + offset);
    let line = source[position..line_end].trim_start_matches([' ', '\t']);
    let Some(head) = line.get(..5) else {
        return false;
    };
    head.eq_ignore_ascii_case("[[div")
        && matches!(
            line.as_bytes().get(5),
            Some(b']' | b' ' | b'\t' | b'_' | b'=')
        )
}

fn wikidot_div_line_has_segment(parser: &Parser<'_, '_>, owner_start: usize) -> bool {
    let source = parser.full_text().inner();
    let line_start = source[..owner_start]
        .rfind('\n')
        .map_or(0, |offset| offset + 1);
    let line_end = source[owner_start..]
        .find('\n')
        .map_or(source.len(), |offset| owner_start + offset);
    let line = source[line_start..line_end].to_ascii_lowercase();
    line.matches("[[iftags").count() > usize::from(parser.head_had_segment())
        || line.contains("[[ifcategory")
}

fn try_recover_wikidot_div_literal<'r, 't>(
    parser: &mut Parser<'r, 't>,
    owner_start: usize,
    arguments_empty: bool,
    body_start: BlockBodyStart,
    flag_score: bool,
    head_started_physical_line: bool,
    follows_inline_structural_close: bool,
) -> Result<Option<ParseSuccess<'r, 't, Elements<'t>>>, ParseError>
where
    'r: 't,
{
    if !parser.settings().layout.legacy() {
        return Ok(None);
    }
    if parser.head_had_segment() && body_start == BlockBodyStart::Inline {
        return Ok(None);
    }
    let inline_scored_line_literal = wikidot_div_is_inline_scored_line_literal(
        parser,
        arguments_empty,
        body_start,
        owner_start,
        flag_score,
    );
    let source = parser.full_text().inner();

    if parser.settings().mode != WikitextMode::List
        && !parser.in_wikidot_div_body()
        && !parser.in_native_blockquote_line()
        && body_start == BlockBodyStart::Inline
        && parser.has_body_end_block_on_line(&BLOCK_DIV)
        && (!flag_score || inline_scored_line_literal)
        && !wikidot_div_line_has_segment(parser, owner_start)
    {
        let line_start = source[..owner_start]
            .rfind('\n')
            .map_or(0, |offset| offset + 1);
        let line_started_after_physical_newline = line_start > 0
            && source[line_start..owner_start]
                .trim_matches([' ', '\t'])
                .is_empty();
        let starts_with_bounded_span = if flag_score {
            false
        } else if let Some(outcome) = parser.wikidot_div_bounded_span_outcome(owner_start)
        {
            outcome
        } else {
            let mutable_state = parser.get_mutable_state();
            let mut outer = parser.clone();
            let outer_end = outer
                .get_body_elements(&BLOCK_DIV, false)
                .ok()
                .map(|_| outer.current().span.start);
            parser.reset_mutable_state(mutable_state);

            let mutable_state = parser.get_mutable_state();
            let mut span = parser.clone();
            let outcome = if span.get_optional_space().is_ok()
                && span.current().token == Token::LeftBlock
            {
                span.get_block_name(false)
                    .ok()
                    .map(|(name, in_head, segment)| {
                        (
                            name.eq_ignore_ascii_case("span")
                                && span.head_word_is_actionable(
                                    &name,
                                    matches!(name, Cow::Borrowed(_)),
                                    segment,
                                ),
                            in_head,
                        )
                    })
                    .is_some_and(|(is_span, in_head)| {
                        is_span
                            && {
                                span.set_block(&BLOCK_SPAN);
                                span.get_head_map_with_body_start_wikidot(
                                    &BLOCK_SPAN,
                                    in_head,
                                )
                                .and_then(|_| span.get_body_elements(&BLOCK_SPAN, false))
                                .is_ok()
                            }
                            && outer_end.is_some_and(|outer_end| {
                                span.current().span.start < outer_end
                            })
                    })
            } else {
                false
            };
            parser.reset_mutable_state(mutable_state);
            parser.cache_wikidot_div_bounded_span(owner_start, outcome);
            outcome
        };
        if !flag_score && starts_with_bounded_span {
            let literal_end = parser.current().span.start;
            let literal = Element::Text(elide_comments_from_literal(
                &source[owner_start..literal_end],
            ));
            let elements = if line_started_after_physical_newline {
                Elements::Multiple(vec![Element::LineBreak, literal])
            } else {
                literal.into()
            };
            return Ok(Some(ParseSuccess::new(elements, Vec::new(), true)));
        }
        let body_start = parser.current().span.start;
        let mut probe = parser.clone();
        let body_text = probe.get_body_text(&BLOCK_DIV)?;
        let owner_end = probe.current().span.start;
        let closer_start = body_start + body_text.len();
        let literal_start = if line_started_after_physical_newline {
            line_start - 1
        } else {
            owner_start
        };
        let doc_start_bare = line_start == 0
            && source[..owner_start].trim_matches([' ', '\t']).is_empty()
            && wikidot_next_line_opens_div(source, owner_end);
        let literal_end = if doc_start_bare {
            owner_end
        } else if source.as_bytes().get(owner_end) == Some(&b'\n') {
            owner_end + 1
        } else {
            owner_end
        };
        let paragraph_safe = !line_started_after_physical_newline && !doc_start_bare;
        if !paragraph_safe {
            parser.mark_wikidot_div_literal_recovered();
        }
        let mut elements = vec![Element::Text(elide_comments_from_literal(
            &source[literal_start..body_start],
        ))];
        let mut errors = Vec::new();
        let mut degraded_from = None;
        while parser.current().span.start < closer_start {
            let before = parser.current().span.start;
            let checkpoint = parser.clone();
            match consume(parser) {
                Ok(success) => {
                    if parser.current().span.start == before {
                        parser.update(&checkpoint);
                        degraded_from = Some(before);
                        break;
                    }
                    let (item, item_errors, _) = success.into();
                    elements.extend(item.as_ref().iter().cloned());
                    errors.extend(item_errors);
                }
                Err(error) => {
                    parser.update(&checkpoint);
                    errors.push(error);
                    degraded_from = Some(before);
                    break;
                }
            }
        }
        if let Some(from) = degraded_from {
            while parser.current().span.start < owner_end {
                if parser.step().is_err() {
                    break;
                }
            }
            elements.push(Element::Text(elide_comments_from_literal(
                &source[from..literal_end],
            )));
        } else {
            if parser.current().span.start == closer_start {
                while parser.current().span.start < owner_end {
                    if parser.step().is_err() {
                        break;
                    }
                }
                elements.push(Element::Text(elide_comments_from_literal(
                    &source[closer_start..owner_end],
                )));
            }
            if literal_end > owner_end && parser.current().span.start == owner_end {
                elements.push(text!("\n"));
            }
        }
        let elements = if paragraph_safe {
            elements
        } else {
            let mut merged: Vec<Element<'t>> = Vec::with_capacity(elements.len());
            for element in elements {
                match element {
                    Element::Text(text) => {
                        if let Some(Element::Text(previous)) = merged.last_mut() {
                            *previous = Cow::Owned(format!("{previous}{text}"));
                        } else {
                            merged.push(Element::Text(text));
                        }
                    }
                    element => merged.push(element),
                }
            }
            merged
        };
        let elements = match elements.len() {
            1 => Elements::Single(elements.into_iter().next().unwrap()),
            _ => Elements::Multiple(elements),
        };
        return Ok(Some(ParseSuccess::new(elements, errors, paragraph_safe)));
    }

    if parser.in_wikidot_div_body()
        && (!head_started_physical_line
            || inline_scored_line_literal
                && (parser.in_wikidot_literal_alias_body()
                    || parser.has_body_end_block_on_line(&BLOCK_DIV)))
        && !follows_inline_structural_close
    {
        let head_end = parser.current().span.start;
        let literal_end = if body_start == BlockBodyStart::NextPhysicalLine {
            source[..head_end].trim_end_matches(['\r', '\n']).len()
        } else {
            head_end
        };
        let line_start = source[..literal_end]
            .rfind('\n')
            .map_or(0, |offset| offset + 1);
        let opener_start = if owner_start >= line_start
            && owner_start <= literal_end
            && source[owner_start..].starts_with("[[")
        {
            owner_start
        } else {
            source[line_start..literal_end]
                .rfind("[[")
                .map_or(line_start, |offset| line_start + offset)
        };
        if inline_scored_line_literal {
            let body_start = parser.current().span.start;
            let mut body = parser.clone();
            let body_text = body.get_body_text(&BLOCK_DIV)?;
            let body_end = body_start + body_text.len();
            let mut close = parser.clone();
            while close.current().span.start < body_end {
                close.step()?;
            }
            parser.update(&close);
            return Ok(Some(ParseSuccess::new(
                Element::Text(elide_comments_from_literal(
                    &source[line_start - 1..body_end],
                ))
                .into(),
                Vec::new(),
                false,
            )));
        }
        let literal = Element::Text(elide_comments_from_literal(
            &source[opener_start..literal_end],
        ));
        let elements = if body_start == BlockBodyStart::NextPhysicalLine {
            Elements::Multiple(vec![literal, Element::LineBreak])
        } else {
            literal.into()
        };
        let paragraph_safe = elements.paragraph_safe();
        return Ok(Some(ParseSuccess::new(
            elements,
            Vec::new(),
            paragraph_safe,
        )));
    }

    Ok(None)
}

#[inline(never)]
fn normalize_wikidot_div_elements(elements: &mut Vec<Element<'_>>, flag_score: bool) {
    if flag_score {
        let item_count = elements.len();
        let mut unwrapped = Vec::with_capacity(item_count.saturating_mul(2));
        for (index, element) in elements.drain(..).enumerate() {
            if index > 0 {
                unwrapped.push(text!("\n"));
            }
            match element {
                Element::Container(container)
                    if container.ctype() == ContainerType::Paragraph
                        && (index == 0 || index + 1 == item_count) =>
                {
                    unwrapped.extend(Vec::<Element>::from(container));
                }
                element => unwrapped.push(element),
            }
        }
        *elements = unwrapped;
        while matches!(
            elements.last(),
            Some(Element::LineBreak | Element::LineBreaks(_))
        ) {
            elements.pop();
        }
        let mut previous_was_scored_div = false;
        elements.retain(|element| {
            if previous_was_scored_div
                && (matches!(element, Element::LineBreak | Element::LineBreaks(_))
                    || matches!(element, Element::Text(text) if text == "\n"))
            {
                return false;
            }
            previous_was_scored_div = matches!(element, Element::Container(container)
                if container.ctype() == ContainerType::Div
                    && !container.elements().iter().any(|child| matches!(child,
                        Element::Container(paragraph)
                            if paragraph.ctype() == ContainerType::Paragraph)));
            true
        });
        let mut cleaned = Vec::with_capacity(elements.len());
        for element in elements.drain(..) {
            let redundant_newline_after_nested_div =
                matches!(&element, Element::Text(text) if text == "\n")
                    && matches!(cleaned.last(), Some(Element::LineBreak | Element::LineBreaks(_)))
                    && cleaned
                        .iter()
                        .rev()
                        .skip(1)
                        .find(|previous| {
                            !matches!(previous, Element::Text(text) if text == "\n")
                        })
                        .is_some_and(|previous| {
                            matches!(previous, Element::Container(container)
                                if container.ctype() == ContainerType::Div)
                        });
            if !redundant_newline_after_nested_div {
                cleaned.push(element);
            }
        }
        *elements = cleaned;
        return;
    }

    if matches!(elements.last(), Some(Element::LineBreak)) {
        elements.pop();
    }
    for index in 1..elements.len() {
        if !matches!(elements[index], Element::LineBreak | Element::LineBreaks(_))
            || !matches!(&elements[index - 1], Element::Container(container)
                if container.ctype() == ContainerType::Div)
        {
            continue;
        }
        let inline_scored_div_follows = matches!(elements.get(index + 1), Some(Element::Text(text))
                if text.to_ascii_lowercase().starts_with("[[div_]]"))
            || matches!(
                elements.get(index + 1..index + 5),
                Some([
                    Element::Text(open),
                    Element::Text(name),
                    Element::Text(score),
                    Element::Text(close),
                ]) if open == "[["
                    && name.eq_ignore_ascii_case("div")
                    && score == "_"
                    && close == "]]"
            );
        if inline_scored_div_follows {
            elements[index] = text!("\n");
        }
    }
}

fn skip_trailing_head_space<'r, 't>(
    parser: &mut Parser<'r, 't>,
    body_start: BlockBodyStart,
) -> Result<BlockBodyStart, ParseError> {
    if parser.current().token != Token::Whitespace {
        return Ok(body_start);
    }
    let mut lookahead = parser.clone();
    lookahead.step()?;
    match lookahead.current().token {
        Token::LineBreak => {
            lookahead.step()?;
            parser.update(&lookahead);
            Ok(BlockBodyStart::NextPhysicalLine)
        }
        Token::ParagraphBreak => {
            parser.update(&lookahead);
            Ok(BlockBodyStart::NextPhysicalLine)
        }
        _ => Ok(body_start),
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
    debug!("Parsing div block (name '{name}', in-head {in_head}, score {flag_score})");
    assert!(!flag_star, "Div doesn't allow star flag");
    assert_block_name(&BLOCK_DIV, name);
    let source = parser.full_text().inner();

    let head = parser.get_head_map_with_body_start_wikidot(&BLOCK_DIV, in_head)?;
    let (mut arguments, mut body_start) = head;
    if parser.settings().layout.legacy() && body_start == BlockBodyStart::Inline {
        body_start = skip_trailing_head_space(parser, body_start)?;
    }
    if let Some(pair) = wikidot_flat_pair_for_head(parser, opener_start)
        && pair.kind == WikidotBlockKind::Div
    {
        consume_wikidot_flat_head_break(parser, false)?;
        let attributes = wikidot_div_head_attributes(parser, &mut arguments);
        let shell = WikidotBlockShell::Div {
            head: opener_start,
            attributes,
            scored: flag_score,
        };
        return ok!(Element::Partial(PartialElement::WikidotBlockOpen(shell)));
    }
    let cacheable_wikijump_failure = !parser.settings().layout.legacy();
    if cacheable_wikijump_failure
        && parser.block_end_scan_outcome((
            WIKIJUMP_DIV_FAILURE_CACHE,
            opener_start,
            false,
        )) == Some(false)
    {
        return Err(parser.make_end_of_input_err());
    }
    if parser.settings().layout.legacy()
        && flag_score
        && !parser.wikidot_alias_has_compatible_close(&BLOCK_DIV, opener_start)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.settings().layout.legacy()
        && parser.in_wikidot_simple_table_cell()
        && body_start == BlockBodyStart::Inline
        && let Some(delimiter) = wikidot_inline_div_table_delimiter(parser)
    {
        let literal_end = delimiter.current().span.start;
        parser.update(&delimiter);
        return ok!(true; Element::Text(elide_comments_from_literal(
            &source[opener_start..literal_end],
        )));
    }
    let head_started_physical_line =
        wikidot_div_head_started_physical_line(parser, body_start);
    let follows_inline_structural_close =
        wikidot_div_follows_inline_structural_close(parser, body_start);
    if parser.settings().layout.legacy()
        && parser.settings().mode != WikitextMode::List
        && !parser.in_wikidot_div_body()
        && !parser.in_native_blockquote_line()
        && !flag_score
        && body_start == BlockBodyStart::Inline
        && (!arguments.is_empty() || arguments.has_empty_key())
        && parser.has_body_end_block_on_line(&BLOCK_DIV)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.settings().layout.legacy()
        && parser.settings().mode != WikitextMode::List
        && !parser.in_wikidot_div_body()
        && !parser.in_native_blockquote_line()
        && !flag_score
        && body_start == BlockBodyStart::Inline
        && parser.head_had_segment()
        && parser.has_body_end_block_on_line(&BLOCK_DIV)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if let Some(recovered) = try_recover_wikidot_div_literal(
        parser,
        opener_start,
        arguments.is_empty(),
        body_start,
        flag_score,
        head_started_physical_line,
        follows_inline_structural_close,
    )? {
        return Ok(recovered);
    }
    if parser.settings().layout.legacy()
        && parser.in_wikidot_div_body()
        && flag_score
        && body_start == BlockBodyStart::Inline
        && parser.current().token == Token::LeftBlockEnd
    {
        let mut close = parser.clone();
        if close
            .get_end_block()
            .is_ok_and(|name| name.eq_ignore_ascii_case("div"))
        {
            parser.update(&close);
            return ok!(false; Elements::None);
        }
    }
    if parser.settings().layout.legacy()
        && body_start == BlockBodyStart::Inline
        && parser.in_wikidot_div_body()
        && head_started_physical_line
        && !parser.in_native_blockquote_line()
        && !flag_score
    {
        while !matches!(
            parser.current().token,
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
        ) {
            parser.step()?;
        }
        if parser.current().token == Token::LineBreak {
            parser.step()?;
        }
        body_start = BlockBodyStart::NextPhysicalLine;
    }
    if parser.settings().layout.legacy()
        && !parser.in_wikidot_div_body()
        && !parser.has_body_end_block(&BLOCK_DIV)
    {
        return Err(parser.make_err(ParseErrorKind::BlockExpectedEnd));
    }
    if parser.settings().layout.legacy()
        && !head_started_physical_line
        && !follows_inline_structural_close
        && (parser
            .has_runtime_literal_in_range(opener_start..parser.current().span.start)
            || parser.body_has_delayed_input(&BLOCK_DIV))
    {
        let _ = parser.get_body_text(&BLOCK_DIV)?;
        let owner_end = parser.current().span.start;
        let generated = parser.generated_in_range(opener_start..owner_end);
        return success_elements(Element::Delayed(DelayedElement::shell(
            source,
            opener_start..owner_end,
            &generated,
        )));
    }

    if parser.settings().layout.legacy()
        && body_start == BlockBodyStart::NextPhysicalLine
        && head_started_physical_line
        && parser.in_wikidot_div_body()
        && !parser.discarding_hidden_body()
        && parser.native_blockquote_depth().is_none()
        && parser.accepts_partial() != AcceptsPartial::ListItem
        && parser.wikidot_div_opener_is_excess(opener_start)
    {
        let head_end = parser.current().span.start;
        let literal_end = source[..head_end].trim_end_matches(['\r', '\n']).len();
        let literal = Element::Text(elide_comments_from_literal(
            &source[opener_start..literal_end],
        ));
        let elements = Elements::Multiple(vec![literal, Element::LineBreak]);
        let paragraph_safe = elements.paragraph_safe();
        return Ok(ParseSuccess::new(elements, Vec::new(), paragraph_safe));
    }

    if parser.settings().layout.legacy()
        && !wikidot_scan_pairs_head(parser, opener_start)
        && parser.wikidot_div_opener_is_excess(opener_start)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let wrap_paragraphs = !flag_score;
    if parser.settings().layout.legacy() {
        parser.enter_wikidot_div_body();
        if flag_score {
            parser.enter_wikidot_scored_div_body();
        }
    }
    let parse_as_paragraphs =
        wrap_paragraphs || parser.settings().layout.legacy() && flag_score;
    let body = parser.get_body_elements_with_context(
        &BLOCK_DIV,
        parse_as_paragraphs,
        body_start,
    );
    if parser.settings().layout.legacy() {
        if flag_score {
            parser.leave_wikidot_scored_div_body();
        }
        parser.leave_wikidot_div_body();
    }
    let (mut elements, errors, _) = match body {
        Ok(body) => body.into(),
        Err(error) => {
            if cacheable_wikijump_failure && error.kind() == ParseErrorKind::EndOfInput {
                parser.cache_block_end_scan_outcomes(
                    WIKIJUMP_DIV_FAILURE_CACHE,
                    &[(opener_start, false)],
                    false,
                );
            }
            return Err(error);
        }
    };
    if parser.settings().layout.legacy() {
        normalize_wikidot_div_elements(&mut elements, flag_score);
    }

    let attributes = wikidot_div_head_attributes(parser, &mut arguments);
    let element =
        Element::Container(Container::new(ContainerType::Div, elements, attributes));
    let fused_close = parser.wikidot_body_closed_with_fused_close(&BLOCK_DIV);

    if parser.take_wikidot_crossed_ol_tail_sentinel()? {
        let mut output = vec![element];
        if fused_close {
            output.push(text!("]"));
        }
        output.push(wikidot_crossed_ol_tail());
        return ok!(false; output, errors);
    }
    if parser.take_wikidot_crossed_list_break_sentinel()? {
        let mut output = vec![element];
        if fused_close {
            output.push(text!("]"));
        }
        output.push(Element::LineBreak);
        ok!(false; output, errors)
    } else if fused_close {
        ok!(false; vec![element, text!("]")], errors)
    } else {
        ok!(element, errors)
    }
}

fn wikidot_div_head_attributes<'r, 't>(
    parser: &Parser<'r, 't>,
    arguments: &mut Arguments<'t>,
) -> AttributeMap<'t> {
    let mut attributes = arguments.to_attribute_map(parser.settings());
    if parser.settings().layout.legacy()
        && !attributes.get().contains_key("class")
        && let Some(class) = arguments.take_normalized_wikidot_class()
    {
        attributes.insert("class", class);
    }
    if parser.settings().layout.legacy()
        && let Some(class) = attributes.remove("class")
    {
        let class = class.trim_end_matches([' ', '\t']).to_owned();
        attributes.insert("class", Cow::Owned(class));
    }
    attributes
}

fn wikidot_crossed_ol_tail<'t>() -> Element<'t> {
    let mut item_attributes = AttributeMap::new();
    item_attributes.insert("style", cow!("list-style: none"));
    Element::List {
        ltype: ListType::Bullet,
        items: vec![ListItem::Elements {
            elements: vec![Element::LineBreak],
            attributes: item_attributes,
        }],
        attributes: AttributeMap::new(),
        wikidot_explicit_block: false,
    }
}

fn wikidot_inline_div_table_delimiter<'r, 't>(
    parser: &Parser<'r, 't>,
) -> Option<Parser<'r, 't>>
where
    'r: 't,
{
    let mut scan = parser.clone();
    let mut raw = false;
    let mut alternate_raw = false;
    let mut triple_link_depth = 0usize;

    loop {
        if matches!(
            scan.current().token,
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
        ) {
            return None;
        }

        if !raw
            && !alternate_raw
            && triple_link_depth == 0
            && scan.current().token == Token::LeftComment
        {
            let mut comment = scan.clone();
            if consume_valid_comment(&mut comment).is_ok() {
                scan.update(&comment);
                continue;
            }
        }

        match scan.current().token {
            Token::Raw => raw = !raw,
            Token::LeftRaw if !raw => alternate_raw = true,
            Token::RightRaw if alternate_raw => alternate_raw = false,
            Token::LeftLink | Token::LeftLinkStar if !raw && !alternate_raw => {
                triple_link_depth += 1;
            }
            Token::RightLink if triple_link_depth > 0 => {
                triple_link_depth -= 1;
            }
            Token::TableColumn
            | Token::TableColumnTitle
            | Token::TableColumnCenter
            | Token::TableColumnRight
                if !raw && !alternate_raw && triple_link_depth == 0 =>
            {
                return Some(scan);
            }
            _ => {}
        }

        scan.step().ok()?;
    }
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::ParseErrorKind;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    fn render(input: &str, layout: Layout) -> String {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:#?}");
        HtmlRender.render(&tree, &page_info, &settings).body
    }

    #[test]
    fn wikidot_div_drops_the_preceding_line_break_unlike_embed() {
        assert_eq!(
            render("text\n[[div]]\nx\n[[/div]]", Layout::Wikidot),
            "<p>text</p><div><p>x</p></div>",
        );
    }

    #[test]
    fn wikidot_div_trims_trailing_class_whitespace_only_in_legacy_layout() {
        let input = "[[div class=\"box \"]]\nbody\n[[/div]]";

        assert_eq!(
            render(input, Layout::Wikidot),
            "<div class=\"box\"><p>body</p></div>",
        );
        assert_eq!(
            render(input, Layout::Wikijump),
            "<div class=\"box \"><p>body</p></div>",
        );
    }

    #[test]
    fn wikidot_div_closes_at_closer_on_quote_line() {
        let source = "[[div]]\nbody text\n> quoted line [[/div]]\nafter text";
        assert_eq!(
            render_preprocessed_wikidot(source),
            "<div><p>body text</p><blockquote><p>quoted line</p></blockquote></div><br>\nafter text",
        );

        let source = "[[div]]\nbody text\n> [[/div]]\nafter text";
        assert_eq!(
            render_preprocessed_wikidot(source),
            "<div><p>body text</p></div><br>\nafter text",
        );
    }

    fn render_preprocessed_wikidot_allowing_errors(source: &str) -> String {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = source.to_owned();
        crate::preprocess_for_layout(&mut source, settings.layout);
        let tokenization = crate::tokenize(&source);
        let (tree, _) = crate::parse(&tokenization, &page_info, &settings).into();
        HtmlRender.render(&tree, &page_info, &settings).body
    }

    #[test]
    fn wikidot_div_unbalanced_fallback_keeps_raw_join_and_literal_opener() {
        let source = concat!(
            "[[div]]inline[[/div]]\n",
            "\n",
            "\n",
            "[[div]]inline, closing tag is on a new line\n",
            "[[/div]]\n",
            "\n",
            "\n",
            "\n",
            "[[div]]\n",
            "[[div]]\n",
            "div opening tag is missing\n",
            "[[/div]]\n",
            "[[/div]]\n",
            "[[/div]]\n",
            "\n",
            "\n",
            "\n",
            "[[div]]\n",
            "[[div]]\n",
            "[[div]]\n",
            "div closing tag is missing\n",
            "[[/div]]\n",
            "[[/div]]\n",
        );
        assert_eq!(
            render_preprocessed_wikidot_allowing_errors(source),
            concat!(
                "[[div]]inline[[/div]]",
                "[[div]]inline, closing tag is on a new line<br>\n",
                "[[/div]]\n",
                "<div><div><p>div opening tag is missing</p></div></div><br>\n",
                "[[/div]]\n",
                "<div><div><p>[[div]]<br>\n",
                "div closing tag is missing</p></div></div>",
            ),
        );
    }

    #[test]
    fn wikidot_failed_div_lines_share_one_paragraph() {
        let source = "[[div]]inline[[/div]]\n\n\n[[div]]inline[[/div]]\n";
        assert_eq!(
            render_preprocessed_wikidot_allowing_errors(source),
            "<p>[[div]]inline[[/div]][[div]]inline[[/div]]</p>",
        );
    }

    #[test]
    fn wikidot_failed_div_lines_wrap_when_no_real_div_is_adjacent() {
        let source = concat!(
            "[[div]]inline[[/div]]\n",
            "\n",
            "\n",
            "[[div]]inline, closing tag is on a new line\n",
            "[[/div]]\n",
        );
        assert_eq!(
            render_preprocessed_wikidot_allowing_errors(source),
            concat!(
                "<p>[[div]]inline[[/div]]",
                "[[div]]inline, closing tag is on a new line<br>\n",
                "[[/div]]</p>",
            ),
        );

        let source = "[[div]]inline, closing tag is on a new line\n[[/div]]\n";
        assert_eq!(
            render_preprocessed_wikidot_allowing_errors(source),
            concat!(
                "<p>[[div]]inline, closing tag is on a new line<br>\n",
                "[[/div]]</p>",
            ),
        );

        let source = concat!(
            "[[div]]inline, closing tag is on a new line\n",
            "[[/div]]\n",
            "\n",
            "\n",
            "[[div]]inline[[/div]]\n",
        );
        assert_eq!(
            render_preprocessed_wikidot_allowing_errors(source),
            concat!(
                "<p>[[div]]inline, closing tag is on a new line<br>\n",
                "[[/div]][[div]]inline[[/div]]</p>",
            ),
        );
    }

    #[test]
    fn wikidot_failed_div_line_wraps_away_from_real_div_blocks() {
        let source = concat!(
            "[[div]]\n",
            "x\n",
            "[[/div]]\n",
            "\n",
            "\n",
            "plain text\n",
            "\n",
            "\n",
            "[[div]]inline[[/div]]\n",
        );
        assert_eq!(
            render_preprocessed_wikidot_allowing_errors(source),
            concat!(
                "<div><p>x</p></div>",
                "<p>plain text</p>",
                "<p>[[div]]inline[[/div]]</p>",
            ),
        );

        let source = "BEFORE\n\n[[div]]inline[[/div]]\n\n\nafter text\n";
        assert_eq!(
            render_preprocessed_wikidot_allowing_errors(source),
            "<p>BEFORE</p><p>[[div]]inline[[/div]]\n</p><p>after text</p>",
        );
    }

    #[test]
    fn wikidot_failed_div_lines_stay_bare_next_to_a_real_div() {
        let source = concat!(
            "[[div]]inline[[/div]]\n",
            "\n",
            "\n",
            "[[div]]inline[[/div]]\n",
            "\n",
            "\n",
            "[[div]]\n",
            "x\n",
            "[[/div]]\n",
        );
        assert_eq!(
            render_preprocessed_wikidot_allowing_errors(source),
            "[[div]]inline[[/div]][[div]]inline[[/div]]\n<div><p>x</p></div>",
        );
    }

    #[test]
    fn wikidot_div_excess_nested_opener_stays_literal() {
        let source = "[[div]]\n[[div]]\nx\n[[/div]]\n";
        assert_eq!(
            render_preprocessed_wikidot(source),
            "<div><p>[[div]]<br>\nx</p></div>",
        );
    }

    #[test]
    fn wikidot_div_excess_opener_after_blank_line_stays_literal() {
        let source = "[[div]]\na\n\n[[div]]\nb\n[[/div]]\n";
        assert_eq!(
            render_preprocessed_wikidot(source),
            "<div><p>a</p><p>[[div]]<br>\nb</p></div>",
        );
    }

    #[test]
    fn wikidot_div_surplus_close_stays_in_its_own_stream_region() {
        let source = "[[div]]\nx\n[[/div]]\n[[/div]]\n[[div]]\n[[div]]\ny\n[[/div]]\n";
        assert_eq!(
            render_preprocessed_wikidot_allowing_errors(source),
            "<div><p>x</p></div><br>\n[[/div]]<div><p>[[div]]<br>\ny</p></div>",
        );
    }

    #[test]
    fn wikidot_div_deficit_opener_inside_open_region_stays_literal() {
        let source = "[[div]]\n[[div]]\nx\n[[/div]]\n[[div]]\ny\n[[/div]]\n";
        assert_eq!(
            render_preprocessed_wikidot(source),
            "<div><div><p>x</p></div><p>[[div]]<br>\ny</p></div>",
        );
    }

    #[test]
    fn wikidot_div_crossed_center_close_keeps_following_div() {
        let source = concat!(
            "[[=]]\n",
            "[[collapsible]]\n",
            "[[/=]]\n",
            "[[div]]\n",
            "INNER\n",
            "[[/div]]\n",
            "[[/collapsible]]\n",
        );
        let html = render_preprocessed_wikidot(source);
        assert!(html.contains("<div><p>INNER</p></div>"), "{html}");
        assert!(!html.contains("[[div]]"), "{html}");
    }

    #[test]
    fn wikidot_continuation_before_a_div_preserves_its_block_boundary() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = concat!(
            "[[div id=\"outer\"]]\n",
            "[[a href=\"x\"]]x[[/a]]\\\n",
            "[[div_ class=\"image\"]]\n",
            "x\n",
            "[[/div]]\n",
            "[[/div]]",
        )
        .to_owned();
        crate::preprocess_for_layout(&mut source, settings.layout);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            "<div id=\"u-outer\"><a href=\"x\">x</a>\n<div class=\"image\">x</div></div>",
        );
    }

    #[test]
    fn wikidot_continuation_before_a_div_like_name_remains_inline() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = concat!(
            "[[a href=\"x\"]]x[[/a]]\\\n",
            "[[division]]\n",
            "x\n",
            "[[/div]]",
        )
        .to_owned();
        crate::preprocess_for_layout(&mut source, settings.layout);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(!errors.is_empty());
        assert_eq!(
            html,
            "<p><a href=\"x\">x</a>[[division]]<br>\nx<br>\n[[/div]]</p>",
        );
    }

    #[test]
    fn wikidot_exact_div_names_discard_malformed_arguments() {
        for (name, normal, continued) in [
            (
                "div",
                "<p>before</p><div><p>x</p></div><br>\nafter",
                "<a href=\"x\">x</a>\n<div><p>x</p></div>",
            ),
            (
                "div_",
                "<p>before</p><div>x</div><br>\nafter",
                "<a href=\"x\">x</a>\n<div>x</div>",
            ),
        ] {
            let normal_source =
                format!("before\n[[{name} @=\"value\"]]\nx\n[[/div]]\nafter",);
            assert_eq!(render_preprocessed_wikidot(&normal_source), normal);

            let continued_source = format!(
                "[[a href=\"x\"]]x[[/a]]\\\n[[{name} @=\"value\"]]\nx\n[[/div]]",
            );
            assert_eq!(render_preprocessed_wikidot(&continued_source), continued,);
        }

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let mut source = "before\n[[div @=\"value\"]]\nx\n[[/div]]\nafter".to_owned();
        crate::preprocess_for_layout(&mut source, settings.layout);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(
            errors
                .iter()
                .any(|error| { error.kind() == ParseErrorKind::BlockMalformedArguments })
        );
        assert!(html.contains("[[div @=&quot;value&quot;]]"), "{html}");
        assert!(!html.contains("<div>"), "{html}");
    }

    fn render_preprocessed_wikidot(source: &str) -> String {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = source.to_owned();
        crate::preprocess_for_layout(&mut source, settings.layout);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:#?}");
        HtmlRender.render(&tree, &page_info, &settings).body
    }

    #[test]
    fn wikidot_div_drops_the_terminal_break_after_a_nested_list() {
        let input = concat!(
            "[[div id=\"fruit\" class=\"box\"]]\n",
            "  [[ul]]\n",
            "    [[li]] 1  [[/li]]\n",
            "    [[li]] 2  [[/li]]\n",
            "  [[/ul]]\n",
            "[[/div]]",
        );

        assert_eq!(
            render(input, Layout::Wikidot),
            "<div class=\"box\" id=\"u-fruit\"><ul>\n<li>1</li>\n<li>2</li>\n</ul></div>",
        );
        assert_eq!(
            render(input, Layout::Wikijump),
            "<div class=\"box\" id=\"fruit\"><ul><li>1 </li><li>2 </li></ul></div>",
        );
    }

    #[test]
    fn quoted_multiline_div_with_quoted_close_remains_native_and_bounded() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = concat!(
            "> [[div style=\"font-weight: bold;\"]]\n",
            "> First quoted line.\n",
            "> \n",
            "> Second quoted line.\n",
            "> [[/div]]\n",
        );
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            "<blockquote><div style=\"font-weight: bold;\"><p>First quoted line.</p><p>Second quoted line.</p></div></blockquote>",
        );
    }

    #[test]
    fn quoted_inline_div_remains_literal_like_wikidot() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = "> [[div class=\"notice\"]]Quoted body.[[/div]]\n";
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(!errors.is_empty());
        assert_eq!(
            html,
            "<blockquote><p>[[div class=&quot;notice&quot;]]Quoted body.[[/div]]</p></blockquote>",
        );
    }

    #[test]
    fn inline_div_remains_literal_in_wikidot_layout() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = "[[div id=\"credit-view\"]]X[[/div]]";
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(
            !errors.is_empty(),
            "literal fallback should retain diagnostics"
        );
        assert_eq!(html, "<p>[[div id=&quot;credit-view&quot;]]X[[/div]]</p>",);
    }

    #[test]
    fn prose_adjacent_inline_div_remains_one_literal_paragraph() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = "BEFORE|[[div]]X[[/div]]|AFTER";
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html, "<p>BEFORE|[[div]]X[[/div]]|AFTER</p>");
    }

    #[test]
    fn wikidot_divs_follow_inline_list_and_div_closers() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for row_count in [1, 2] {
            let mut input = String::new();
            for _ in 0..row_count {
                input.push_str(
                    "[[div_ class=\"colmod-block\"]]\n\
                     [[ul]][[li class=\"folded\"]][[ul]]_[[/ul]][[div class=\"colmod-link-top\"]]\n\
                     [[div_ class=\"foldable-list-container\"]]\n\
                     link[[/div]][[/div]][[div class=\"colmod-content\"]]\n",
                );
            }
            for _ in 0..row_count {
                input.push_str(
                    "[[/div]][[div]]\n\
                     [[div_ class=\"foldable-list-container\"]]\n\
                     link[[/div]][[/div]][[/li]][[/ul]][[/div]]\n",
                );
            }

            let tokenization = crate::tokenize(&input);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;

            assert!(errors.is_empty(), "{errors:#?}");
            assert_eq!(html.matches("colmod-link-top").count(), row_count);
            assert_eq!(html.matches("colmod-content").count(), row_count);
            assert_eq!(
                html.matches("foldable-list-container").count(),
                row_count * 2,
            );
            assert!(!html.contains("[[div"), "{html}");
            assert!(!html.contains("[[/div]]"), "{html}");
        }
    }

    #[test]
    fn wikijump_unclosed_div_runs_fail_before_recursive_body_parsing() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        for closer_count in [0, 1, 2, 4] {
            let mut input = "[[div]]\n".repeat(256);
            input.push_str(&"[[/div]]\n".repeat(closer_count));
            let tokenization = crate::tokenize(&input);
            let started = std::time::Instant::now();
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();

            assert!(
                started.elapsed() < std::time::Duration::from_secs(2),
                "{closer_count} closers took {:?}",
                started.elapsed(),
            );
            assert!(!errors.is_empty());
            assert!(!tree.elements.is_empty());
        }
    }

    #[test]
    fn wikijump_malformed_div_fuzz_shape_reuses_nested_failure_cache() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let mut input = "[[div]]\n".repeat(6);
        input.push_str("\u{1}\0\0\0\0\0\0\u{2}");
        input.push_str(&"[[div]]\n".repeat(25));
        input.push_str("[[div][[tabview]][div]]\n");
        input.push_str(&"[[div]]\n".repeat(3));
        input.push_str("[d[iv]]\n");
        input.push_str(&"[[div]]\n".repeat(15));
        input.push('\u{13}');
        input.push_str(&"[[div]]\n".repeat(3));
        input.push_str("/div]]\n");
        input.push_str(&"[[/div]]\n".repeat(13));

        let tokenization = crate::tokenize(&input);
        let started = std::time::Instant::now();
        let (_tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "fuzz-derived malformed div retry took {:?}",
            started.elapsed(),
        );
        assert!(!errors.is_empty());
    }

    #[test]
    fn unclosed_normal_div_lines_remain_in_a_paragraph() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = "[[div]]\n[[div]]\n[[div]]";
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(
            errors.iter().any(|error| {
                error.rule() == "block-div"
                    && error.kind() == ParseErrorKind::BlockExpectedEnd
            }),
            "{errors:#?}",
        );
        assert_eq!(html, "<p>[[div]]<br>\n[[div]]<br>\n[[div]]</p>");
    }

    #[test]
    fn quoted_scored_div_closes_without_absorbing_following_page() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = concat!(
            "> [[div_ class=\"notice\"]]\n",
            "> body\n",
            "> [[/div]]\n",
            "following page\n",
        );
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("<div class=\"notice\">body</div>"), "{html}");
        assert!(html.contains("following page"), "{html}");
        assert!(!html.contains("[[/div]]"), "{html}");
        let div_end = html.find("</div>").expect("div close missing");
        let following = html.find("following page").expect("following text missing");
        assert!(div_end < following, "{html}");
    }

    #[test]
    fn scored_div_does_not_accept_scored_close_in_wikidot_layout() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[div_]]\nbody\n[[/div_]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(!errors.is_empty());
        assert_eq!(html, "<p>[[div_]]<br>\nbody<br>\n[[/div_]]</p>");
    }

    #[test]
    fn wikidot_scored_div_discards_formatting_breaks_around_nested_divs() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = concat!(
            "[[div_ class=\"outer\"]]\n",
            "    \t[[div_ class=\"left\"]]\n",
            "    \t\t[[span]]alpha[[/span]] [[span]]beta[[/span]]\n",
            "    \t[[/div]]\n",
            "    \t[[div_ class=\"empty\"]]\n",
            "    \t[[/div]]\n",
            "[[/div]]",
        );
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            "<div class=\"outer\"><div class=\"left\"><span>alpha</span> <span>beta</span></div><div class=\"empty\"></div></div>",
        );
    }

    #[test]
    fn wikidot_nested_div_line_and_scored_paragraph_boundaries_match_live_dom() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = concat!(
            "[[div]]\n",
            "cherry\n",
            "[[div id=\"tasty\"]]mango[[/div]]\n",
            "melon\n",
            "[[/div]]\n",
            "[[div_]][[/div]]\n",
            "[[div_ class=\"blockquote\"]]\n",
            "Apple\n",
            "[[/div]]\n",
            "[[div_]]Banana[[/div]]\n",
            "[[div_]]\n",
            "alpha\n\nbeta\n\ngamma\n",
            "[[/div]]",
        );
        let tokenization = crate::tokenize(source);
        let (tree, _) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert_eq!(
            html,
            "<div><p>cherry</p><div id=\"u-tasty\"><p>melon</p></div><div class=\"blockquote\">Apple</div>\n[[div_]]Banana</div><div>alpha\n<p>beta</p>\ngamma</div>",
        );
    }
}
