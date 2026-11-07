/*
 * parsing/rule/impls/block/blocks/list.rs
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
use crate::parsing::rule::impls::block::parser::BlockBodyStart;
use crate::parsing::{ParagraphSplitKind, ParserWrap, strip_newlines};
use crate::tree::{
    AcceptsPartial, AttributeMap, Container, ContainerType, ListItem, ListType,
    PartialElement, WikidotBlockCloseKind,
};

pub const BLOCK_UL: BlockRule = BlockRule {
    name: "block-list-unordered",
    accepts_names: &["ul"],
    accepts_star: false,
    accepts_score: true,
    accepts_newlines_outside_body: true,
    parse_fn: parse_unordered_block,
};

pub const BLOCK_OL: BlockRule = BlockRule {
    name: "block-list-ordered",
    accepts_names: &["ol"],
    accepts_star: false,
    accepts_score: true,
    accepts_newlines_outside_body: true,
    parse_fn: parse_ordered_block,
};

pub const BLOCK_LI: BlockRule = BlockRule {
    name: "block-list-item",
    accepts_names: &["li"],
    accepts_star: false,
    accepts_score: true,
    accepts_newlines_outside_body: true,
    parse_fn: parse_list_item,
};

fn parse_unordered_block<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    let block = (&BLOCK_UL, ListType::Bullet);

    parse_list_block(
        block,
        parser,
        name,
        _opener_start,
        flag_star,
        flag_score,
        in_head,
    )
}

fn parse_ordered_block<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    let block = (&BLOCK_OL, ListType::Numbered);

    parse_list_block(
        block,
        parser,
        name,
        _opener_start,
        flag_star,
        flag_score,
        in_head,
    )
}

fn parse_list_block<'r, 't>(
    (block_rule, list_type): (&BlockRule, ListType),
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    let depth = parser.wikidot_li_pairing_depth();
    parser.set_wikidot_li_pairing_depth(depth + 1);
    let track_collect = parser.settings().layout.legacy();
    if track_collect {
        parser.push_wikidot_li_collect(None);
    }
    let result = parse_list_block_impl(
        (block_rule, list_type),
        parser,
        name,
        _opener_start,
        flag_star,
        flag_score,
        in_head,
    );
    if track_collect {
        parser.pop_wikidot_li_collect();
    }
    if result.is_err() && depth == 0 {
        parser.wikidot_li_cross_clear();
    }
    parser.set_wikidot_li_pairing_depth(depth);
    result
}

fn parse_list_block_impl<'r, 't>(
    (block_rule, list_type): (&BlockRule, ListType),
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    let rule_name = block_rule.name;
    debug!(
        "{name}/{rule_name}/{} head={in_head} score={flag_score}",
        list_type.name(),
    );

    let nested = parser.accepts_partial() == AcceptsPartial::ListItem;
    let wikidot = parser.settings().layout.legacy();
    let parser = &mut ParserWrap::new(parser, AcceptsPartial::ListItem);

    assert!(!flag_star, "List block doesn't allow star flag");
    assert_block_name(block_rule, name);

    let arguments = parser.get_head_map_wikidot(block_rule, in_head)?;
    let attributes = arguments.to_attribute_map(parser.settings());
    if wikidot && parser.wikidot_li_pairing_depth() == 1 {
        parser.compute_wikidot_li_pairing();
        parser.compute_wikidot_li_cross_scan();
    }
    if parser.settings().layout.legacy() && !parser.has_body_end_block(block_rule) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    // Live leaves the opener literal when a bare-scheme [[iframe]] head on the
    // line before the closer absorbed every closer for the block.
    if wikidot
        && parser.wikidot_greedy_tail_eats_closers(
            _opener_start,
            &[block_rule.accepts_names[0]],
        )
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if wikidot
        && note_list_would_take_enclosing_closer(
            parser,
            _opener_start,
            block_rule.accepts_names[0],
        )
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let body = if wikidot
        && let Some(depth) = parser.native_blockquote_depth()
        && (parser.has_native_blockquote_body_end(block_rule, depth)
            || parser.has_surplus_quote_list_close(block_rule, depth)
            || parser.has_shallower_quote_list_close(block_rule, depth))
    {
        parser.get_body_elements_with_literal_quote_context(
            block_rule,
            false,
            BlockBodyStart::NextPhysicalLine,
            None,
        )?
    } else {
        parser.get_body_elements(block_rule, false)?
    };
    let fused_close = parser.wikidot_body_closed_with_fused_close(block_rule);
    let closer_has_same_line_residual = !matches!(
        parser.current().token,
        Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
    );
    let (mut elements, errors, _) = body.into();

    if wikidot && name != block_rule.accepts_names[0] {
        strip_newlines(&mut elements);
        if fused_close {
            elements.push(text!("]"));
        }
        if !closer_has_same_line_residual {
            elements.push(Element::LineBreak);
        }
        return ok!(false; elements, errors);
    }

    let mut items = Vec::new();
    let mut literal_elements = Vec::new();
    let mut partial_item_indices: Vec<usize> = Vec::new();
    let mut partial_count = 0usize;
    let mut skipped_empty_partial = false;

    if flag_score {
        strip_newlines(&mut elements);
    }

    if elements.is_empty() {
        if parser.settings().layout.legacy() {
            return if nested {
                ok!(false; Elements::None, errors)
            } else {
                record_wikidot_explicit_list_end(parser);
                if parser.wikidot_preserve_empty_lists()
                    && parser.body_end_boundaries_is_empty()
                {
                    ok!(false; Element::List {
                        ltype: list_type,
                        items: Vec::new(),
                        attributes: attributes.clone(),
                        wikidot_explicit_block: true,
                        wikidot_unclosed: false,
                    }, errors)
                } else {
                    ok!(false; Element::LineBreak, errors)
                }
            };
        }
        return Err(parser.make_err(ParseErrorKind::ListEmpty));
    }

    let content_follows = wikidot_content_follows(parser.remaining());

    for element in elements {
        match element {
            Element::Partial(PartialElement::ListItem(list_item)) => {
                push_literal_list_item(
                    &mut items,
                    &mut literal_elements,
                    wikidot,
                    flag_score,
                    content_follows,
                );
                partial_count += 1;
                if !matches!(&list_item, ListItem::Elements { elements, .. } if elements.is_empty())
                {
                    partial_item_indices.push(items.len());
                    items.push(list_item);
                } else {
                    skipped_empty_partial = true;
                }
            }

            element @ Element::List { .. } => {
                push_literal_list_item(
                    &mut items,
                    &mut literal_elements,
                    wikidot,
                    flag_score,
                    content_follows,
                );
                if wikidot
                    && !flag_score
                    && matches!(items.last(), Some(ListItem::SubList { .. }))
                {
                    // Live puts sibling native lists under one outer item:
                    // each interior list joins the open inline-styled item
                    // instead of opening one of its own.
                    if let Some(ListItem::SubList { element: sub }) = items.pop() {
                        let mut attributes = AttributeMap::new();
                        assert!(
                            attributes.insert(
                                "style",
                                cow!("list-style: none; display: inline")
                            )
                        );
                        items.push(ListItem::Elements {
                            elements: vec![*sub],
                            attributes,
                        });
                    }
                }
                if parser.settings().layout.legacy()
                    && let Some(ListItem::Elements { elements, .. }) = items.last_mut()
                {
                    elements.push(element);
                    continue;
                }
                let element = Box::new(element);
                items.push(ListItem::SubList { element });
            }

            element if element.is_whitespace() && literal_elements.is_empty() => continue,

            element => literal_elements.push(element),
        }
    }
    push_literal_list_item(
        &mut items,
        &mut literal_elements,
        wikidot,
        flag_score,
        content_follows,
    );

    let mut b5_escaped: Option<Vec<Element<'t>>> = None;
    if wikidot
        && !flag_score
        && !skipped_empty_partial
        && items
            .iter()
            .all(|item| matches!(item, ListItem::Elements { .. }))
    {
        let pairing_depth = parser.wikidot_li_pairing_depth();
        let (heads, closes) = parser.wikidot_li_items_at_depth(pairing_depth);
        if heads.len() == partial_count
            && closes.len() == partial_count
            && partial_item_indices.len() == partial_count
            && partial_item_indices
                .windows(2)
                .all(|pair| pair[1] == pair[0] + 1)
        {
            b5_escaped = wikidot_li_apply_b5(
                parser,
                list_type,
                &mut items,
                &partial_item_indices,
                &heads,
                &closes,
            );
        }
    }

    if wikidot && flag_score {
        for item in &mut items {
            if let ListItem::Elements { elements, .. } = item {
                let terminal_inline_raw_line_break = matches!(
                    elements.as_slice(),
                    [.., Element::Raw(_), Element::LineBreak]
                );
                strip_newlines(elements);
                if terminal_inline_raw_line_break {
                    elements.push(Element::LineBreak);
                }
            }
        }
    }

    if items.is_empty() {
        if let Some(escaped) = b5_escaped {
            let count = escaped.len();
            let mut success =
                crate::parsing::ParseSuccess::new(escaped.into(), errors, false);
            success.paragraph_split =
                Some(vec![(count, false, ParagraphSplitKind::Paragraph)]);
            return Ok(success);
        }
        if parser.settings().layout.legacy() {
            return if nested {
                ok!(false; Elements::None, errors)
            } else {
                record_wikidot_explicit_list_end(parser);
                if parser.wikidot_preserve_empty_lists()
                    && parser.body_end_boundaries_is_empty()
                {
                    ok!(false; Element::List {
                        ltype: list_type,
                        items: Vec::new(),
                        attributes: attributes.clone(),
                        wikidot_explicit_block: true,
                        wikidot_unclosed: false,
                    }, errors)
                } else {
                    ok!(false; Element::LineBreak, errors)
                }
            };
        }
        return Err(parser.make_err(ParseErrorKind::ListEmpty));
    }

    let element = Element::List {
        ltype: list_type,
        items,
        attributes,
        wikidot_explicit_block: parser.settings().layout.legacy(),
        wikidot_unclosed: false,
    };
    if let Some(escaped) = b5_escaped {
        let mut output = vec![element];
        if fused_close {
            output.push(text!("]"));
        }
        let list_count = output.len();
        let escaped_count = escaped.len();
        output.extend(escaped);
        let mut success = crate::parsing::ParseSuccess::new(output.into(), errors, false);
        success.paragraph_split = Some(vec![
            (list_count, false, ParagraphSplitKind::Paragraph),
            (escaped_count, false, ParagraphSplitKind::Paragraph),
        ]);
        return Ok(success);
    }
    // L85: live drops the post-list break element before same-para content
    // (ul-post-e, closeout, cross `G`); the break token itself is dropped
    // top-level by gather. Blank lines and input end keep the break, as
    // does content inside a kept iftags gate body (kept-gate pin: boundary
    // or still-pending designated end).
    record_wikidot_explicit_list_end(parser);
    let content_after_list = parser.wikidot_next_line_has_content()
        && !parser.body_end_boundary_accepts("iftags")
        && !parser.has_pending_iftags_end_ahead();
    if parser.settings().layout.legacy()
        && !nested
        && !flag_score
        && !closer_has_same_line_residual
        && !content_after_list
        && !parser.pending_note_list_closer()
    {
        let mut output = vec![element];
        if fused_close {
            // A fused closer's residual `]` occupies the line-end slot: live
            // renders `[[/ul]]]` at EOF as `</ul>]` and a following blank
            // line ends the run without a break, but a single source line
            // break still shows as `<br />` (captured 2026-11-05).
            output.push(text!("]"));
            if parser.current().token == Token::LineBreak {
                output.push(Element::LineBreak);
            }
        } else {
            output.push(Element::LineBreak);
        }
        ok!(false; output, errors)
    } else if fused_close {
        ok!(false; vec![element, text!("]")], errors)
    } else {
        success_elements_with_paragraph_safety(false, element, errors)
    }
}

fn record_wikidot_explicit_list_end(parser: &Parser) {
    if parser.settings().layout.legacy() {
        parser.record_wikidot_explicit_list_end(parser.current().span.start);
    }
}

fn push_literal_list_item<'t>(
    items: &mut Vec<ListItem<'t>>,
    elements: &mut Vec<Element<'t>>,
    append_to_last: bool,
    outer_list_is_scored: bool,
    content_follows: bool,
) {
    while elements.last().is_some_and(Element::is_whitespace) {
        elements.pop();
    }
    if elements.is_empty() {
        return;
    }

    if append_to_last
        && let Some(ListItem::Elements {
            elements: item_elements,
            ..
        }) = items.last_mut()
    {
        // Live joins an un-item'd line after a list with a break: the
        // newline after the list's last line shows as `<br>` before the
        // text, while a following list stays glued to it.
        if matches!(item_elements.last(), Some(Element::List { .. }))
            && elements
                .first()
                .is_some_and(|element| !element.is_whitespace())
        {
            item_elements.push(Element::LineBreak);
        }
        item_elements.append(elements);
    } else {
        if append_to_last && !outer_list_is_scored {
            wrap_wikidot_malformed_scored_items(elements, content_follows);
        }
        let mut attributes = AttributeMap::new();
        assert!(attributes.insert("style", cow!("list-style: none")));
        items.push(ListItem::Elements {
            elements: std::mem::take(elements),
            attributes,
        });
    }
}

fn wikidot_li_flat_spans(
    parser: &Parser<'_, '_>,
    items: &[ListItem],
) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    for item in items {
        let ListItem::Elements { elements, .. } = item else {
            continue;
        };
        for element in elements {
            let Element::Partial(partial) = element else {
                continue;
            };
            let head = match partial {
                PartialElement::WikidotBlockOpen(shell) => shell.head_byte(),
                PartialElement::WikidotBlockClose(kind) => kind.head_byte(),
                _ => continue,
            };
            if let Some(pair) = parser.wikidot_block_pair_at_head_byte(head) {
                let span = (pair.head_byte, pair.closer_byte);
                if !spans.contains(&span) {
                    spans.push(span);
                }
            }
        }
    }
    spans
}

fn wikidot_li_span_covers(spans: &[(usize, usize)], pos: usize) -> bool {
    spans
        .iter()
        .any(|(head, closer)| *head < pos && pos < *closer)
}

fn wikidot_li_open_heads(elements: &[Element]) -> Vec<usize> {
    elements
        .iter()
        .filter_map(|element| match element {
            Element::Partial(PartialElement::WikidotBlockOpen(shell)) => {
                Some(shell.head_byte())
            }
            _ => None,
        })
        .collect()
}

fn wikidot_li_apply_b5<'t>(
    parser: &Parser<'_, '_>,
    list_type: ListType,
    items: &mut Vec<ListItem<'t>>,
    partial_item_indices: &[usize],
    heads: &[usize],
    closes: &[usize],
) -> Option<Vec<Element<'t>>> {
    let spans = wikidot_li_flat_spans(parser, items);
    if spans.is_empty() {
        return None;
    }
    let base = partial_item_indices.first().copied().unwrap_or(0);
    let mut merged: Vec<ListItem<'t>> = Vec::with_capacity(items.len());
    merged.extend(items.drain(..base));
    let mut partials: std::collections::VecDeque<ListItem<'t>> =
        items.drain(..).collect();
    let mut hi = 0usize;
    while let Some(outer) = partials.pop_front() {
        let dropped = wikidot_li_span_covers(&spans, closes[hi]);
        let mut inner_count = 0usize;
        while hi + inner_count + 1 < heads.len()
            && wikidot_li_span_covers(&spans, heads[hi + inner_count + 1])
        {
            inner_count += 1;
        }
        if !dropped || inner_count == 0 {
            merged.push(outer);
            hi += 1;
            continue;
        }
        let (mut outer_elements, outer_attributes) = match outer {
            ListItem::Elements {
                elements,
                attributes,
            } => (elements, attributes),
            other => {
                merged.push(other);
                hi += 1;
                continue;
            }
        };
        let outer_heads = wikidot_li_open_heads(&outer_elements);
        let mut inner_items: Vec<(Vec<Element<'t>>, crate::tree::AttributeMap<'t>)> =
            Vec::new();
        for _ in 0..inner_count {
            let Some(inner) = partials.pop_front() else {
                break;
            };
            let ListItem::Elements {
                elements,
                attributes,
            } = inner
            else {
                continue;
            };
            inner_items.push((elements, attributes));
        }
        let mut post = Vec::new();
        let mut close_element = None;
        if let Some((elements, attributes)) = inner_items.pop() {
            let split_at = elements.iter().position(|element| {
                let Element::Partial(PartialElement::WikidotBlockClose(kind)) = element
                else {
                    return false;
                };
                outer_heads.contains(&kind.head_byte())
            });
            match split_at {
                Some(at) => {
                    let mut elements = elements;
                    post = elements.split_off(at + 1);
                    close_element = elements.pop();
                    let pre = elements;
                    if !pre.is_empty() {
                        inner_items.push((pre, attributes));
                    }
                }
                None => inner_items.push((elements, attributes)),
            }
        }
        if !inner_items.is_empty() {
            let inner_list_items: Vec<ListItem> = inner_items
                .into_iter()
                .map(|(elements, attributes)| ListItem::Elements {
                    elements,
                    attributes,
                })
                .collect();
            outer_elements.push(Element::List {
                ltype: list_type,
                items: inner_list_items,
                attributes: crate::tree::AttributeMap::new(),
                wikidot_explicit_block: true,
                wikidot_unclosed: false,
            });
        }
        if let Some(close) = close_element {
            outer_elements.push(close);
        }
        outer_elements.extend(post);
        merged.push(ListItem::Elements {
            elements: outer_elements,
            attributes: outer_attributes,
        });
        hi += 1 + inner_count;
    }
    *items = merged;
    wikidot_li_split_external_close(items, heads, closes)
}

fn wikidot_li_split_external_close<'t>(
    items: &mut Vec<ListItem<'t>>,
    heads: &[usize],
    closes: &[usize],
) -> Option<Vec<Element<'t>>> {
    let ranges: Vec<(usize, usize)> = heads
        .iter()
        .zip(closes.iter())
        .map(|(head, close)| (*head, *close))
        .collect();
    let head_inside = |head: usize| {
        ranges
            .iter()
            .any(|(start, end)| *start < head && head < *end)
    };
    let found = items.iter().enumerate().find_map(|(item_index, item)| {
        let ListItem::Elements { elements, .. } = item else {
            return None;
        };
        elements
            .iter()
            .position(|element| {
                let Element::Partial(PartialElement::WikidotBlockClose(kind)) = element
                else {
                    return false;
                };
                !head_inside(kind.head_byte())
            })
            .map(|at| (item_index, at))
    });
    let Some((item_index, at)) = found else {
        return None;
    };
    let mut escaped = match &mut items[item_index] {
        ListItem::Elements { elements, .. } => elements.split_off(at),
        _ => return None,
    };
    let mut drained: Vec<Element<'t>> = Vec::new();
    for later in items.drain(item_index + 1..) {
        let ListItem::Elements { elements, .. } = later else {
            continue;
        };
        drained.extend(elements);
    }
    escaped.extend(drained);
    if matches!(
        &items[item_index],
        ListItem::Elements { elements, .. } if elements.is_empty()
    ) {
        items.remove(item_index);
    }
    Some(escaped)
}

fn wikidot_content_follows(tokens: &[ExtractedToken<'_>]) -> bool {
    let mut in_comment = false;
    let mut in_block_end = false;
    let mut comment_tail_dashes = false;
    for extracted in tokens {
        if in_block_end {
            if extracted.token == Token::RightBlock {
                in_block_end = false;
            }
            continue;
        }
        if in_comment {
            if comment_tail_dashes && extracted.token == Token::RightBracket {
                in_comment = false;
                comment_tail_dashes = false;
            } else {
                comment_tail_dashes = extracted.slice.ends_with("--");
            }
            continue;
        }
        match extracted.token {
            Token::InputEnd => return false,
            Token::LineBreak | Token::ParagraphBreak | Token::Whitespace => {}
            Token::LeftComment => in_comment = true,
            Token::LeftBlockEnd => in_block_end = true,
            Token::Other if extracted.slice.trim().is_empty() => {}
            _ => return true,
        }
    }
    false
}

fn wrap_wikidot_malformed_scored_items(
    elements: &mut Vec<Element<'_>>,
    content_follows: bool,
) {
    let mut lines = vec![Vec::new()];
    for element in std::mem::take(elements) {
        if element == Element::LineBreak {
            lines.push(Vec::new());
        } else {
            lines.last_mut().unwrap().push(element);
        }
    }
    let line_text = |line: &[Element]| {
        line.iter()
            .filter_map(|element| match element {
                Element::Text(text) => Some(text.as_ref()),
                _ => None,
            })
            .collect::<String>()
    };
    if line_text(&lines[0]) != "[[li_]]" {
        *elements = join_lines(lines);
        return;
    }
    let Some(paragraph_start) = lines
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(index, line)| (line_text(line) == "[[li_]]").then_some(index))
    else {
        *elements = join_lines(lines);
        return;
    };

    let paragraph_lines = lines.split_off(paragraph_start);
    *elements = join_lines(lines);
    for line in paragraph_lines {
        if line.is_empty() {
            continue;
        }
        if line
            .iter()
            .all(|element| matches!(element, Element::Text(_)))
            && line_text(&line) == "[[/li]]"
            && !content_follows
        {
            elements.extend(line);
            continue;
        }
        let paragraph =
            Container::new(ContainerType::Paragraph, line, AttributeMap::new());
        elements.push(Element::Container(paragraph));
    }
}

fn join_lines<'t>(lines: Vec<Vec<Element<'t>>>) -> Vec<Element<'t>> {
    let mut elements = Vec::new();
    for (index, line) in lines.into_iter().enumerate() {
        if index > 0 {
            elements.push(Element::LineBreak);
        }
        elements.extend(line);
    }
    elements
}

/// Live's Bibliography rule ends its replacement with a newline of its own, so a
/// source line break after the bibliography block is a blank line: no `<br>`
/// is produced for it. The list item raw-body heuristic must not add one.
fn trailing_wikidot_bibliography(elements: &[Element<'_>]) -> bool {
    matches!(
        elements.last(),
        Some(Element::BibliographyBlock { .. })
            | Some(Element::Partial(PartialElement::WikidotBlockClose(
                WikidotBlockCloseKind::BibliographyCrossed { .. }
            )))
    )
}

fn note_list_would_take_enclosing_closer<'r, 't>(
    parser: &Parser<'r, 't>,
    opener_start: usize,
    kind: &str,
) -> bool
where
    'r: 't,
{
    // The enclosing list has to have opened inside this footnote. A list
    // that is still open from outside the footnote keeps its own closer.
    let Some(depth_at_footnote) = parser.list_depth_at_footnote() else {
        return false;
    };
    if !parser.in_note() || parser.wikidot_li_pairing_depth() <= depth_at_footnote + 1 {
        return false;
    }
    if same_kind_list_depth_before(parser.full_text().inner(), opener_start, kind) <= 0 {
        return false;
    }
    same_kind_closers_ahead(parser, kind) < 2
}

fn same_kind_list_depth_before(source: &str, opener_start: usize, kind: &str) -> i32 {
    let end = opener_start.min(source.len());
    let mut depth = 0i32;
    let mut index = 0usize;
    let text = &source[..end];
    while let Some(found) = text[index..].find("[[") {
        let start = index + found;
        match list_marker_at(text, start, kind) {
            Some(true) => depth += 1,
            Some(false) => depth -= 1,
            None => {}
        }
        index = start + 2;
    }
    depth
}

fn same_kind_closers_ahead<'r, 't>(parser: &Parser<'r, 't>, kind: &str) -> usize
where
    'r: 't,
{
    let mut scan = parser.clone();
    let mut nested = 0i32;
    let mut closers = 0usize;
    loop {
        if scan.current().token == Token::InputEnd || scan.at_hard_body_end_boundary() {
            break;
        }
        let before = scan.current().span.start;
        match scan.current().token {
            Token::LeftBlock => {
                let mut probe = scan.clone();
                if let Ok((name, _, _)) = probe.get_block_name(false) {
                    let name = name.strip_suffix('_').unwrap_or(&name);
                    if name.eq_ignore_ascii_case(kind) {
                        nested += 1;
                    }
                }
            }
            Token::LeftBlockEnd => {
                let mut probe = scan.clone();
                if let Ok(name) = probe.get_end_block() {
                    let name = name.strip_suffix('_').unwrap_or(&name);
                    if name.eq_ignore_ascii_case(kind) {
                        if nested == 0 {
                            closers += 1;
                        } else {
                            nested -= 1;
                        }
                    }
                    scan.update(&probe);
                    if scan.current().span.start != before {
                        continue;
                    }
                }
            }
            _ => {}
        }
        if scan.step().is_err() || scan.current().span.start == before {
            break;
        }
    }
    closers
}

/// `Some(true)` is an opener, `Some(false)` a closer, of `kind`.
fn list_marker_at(text: &str, start: usize, kind: &str) -> Option<bool> {
    let rest = text.get(start..)?;
    let (closer, name_at) = if let Some(rest) = rest.strip_prefix("[[/") {
        (true, rest)
    } else {
        (false, rest.strip_prefix("[[")?)
    };
    let name = name_at.get(..kind.len())?;
    if !name.eq_ignore_ascii_case(kind) {
        return None;
    }
    let after = name_at.get(kind.len()..)?;
    if closer {
        after.starts_with("]]").then_some(false)
    } else if after.starts_with("]]") || after.starts_with([' ', '\t']) {
        Some(true)
    } else {
        None
    }
}

fn parse_list_item<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("List item block: name={name}, in_head={in_head}, score={flag_score}");
    let wikidot = parser.settings().layout.legacy();
    assert!(!flag_star, "List item block doesn't allow star flag");
    assert_block_name(&BLOCK_LI, name);
    if flag_score && wikidot {
        return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
    }

    let arguments = parser.get_head_map_wikidot(&BLOCK_LI, in_head)?;
    let attributes = arguments.to_attribute_map(parser.settings());
    let body_start = parser.current().span.start;
    if wikidot && parser.wikidot_li_inside_item_body() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if wikidot && parser.wikidot_greedy_tail_eats_closers(opener_start, &["li"]) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if wikidot && parser.wikidot_li_pairing_depth() > 0 {
        if !parser.wikidot_li_head_paired(opener_start) {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
    } else if wikidot && !parser.has_body_end_block(&BLOCK_LI) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if wikidot
        && parser.wikidot_li_pairing_depth() > 0
        && !parser.discarding_hidden_body()
    {
        parser.push_wikidot_li_collect(Some(opener_start));
        let body = parser.get_wikidot_li_body_segments(&BLOCK_LI);
        parser.pop_wikidot_li_collect();
        let body = body?;
        let source_end = parser.current().span.start;
        let fused_close = parser.wikidot_body_closed_with_fused_close(&BLOCK_LI);
        let (segments, errors, _) = body.into();
        let source = &parser.full_text().inner()[body_start..source_end];
        let mut elements = assemble_wikidot_li_segments(
            segments,
            source,
            parser.in_wikidot_bibliography_body(),
            parser.pending_note_list_closer(),
        );
        if parser.wikidot_li_closed_at_shallower_quote_line()
            && matches!(elements.last(), Some(Element::LineBreak))
        {
            elements.pop();
        }
        if fused_close {
            elements.push(text!("]"));
        }
        let element = Element::Partial(PartialElement::ListItem(ListItem::Elements {
            elements,
            attributes,
        }));
        return success_elements_with_paragraph_safety(false, element, errors);
    }

    let body = parser.get_body_elements(&BLOCK_LI, false)?;
    let source_end = parser.current().span.start;
    let fused_close = parser.wikidot_body_closed_with_fused_close(&BLOCK_LI);
    let (mut elements, errors, _) = body.into();

    if flag_score {
        strip_newlines(&mut elements);
    }
    if wikidot {
        let source = &parser.full_text().inner()[body_start..source_end];
        let body = source
            .to_ascii_lowercase()
            .rfind("[[/li")
            .map(|index| &source[..index])
            .unwrap_or(source);
        if body.starts_with("\n\n") || body.ends_with("\n\n") {
            strip_newlines(&mut elements);
        } else if body.ends_with('\n')
            && !matches!(elements.last(), Some(Element::LineBreak))
            && !parser.in_wikidot_bibliography_body()
            && !trailing_wikidot_bibliography(&elements)
        {
            elements.push(Element::LineBreak);
        }
        while matches!(elements.last(), Some(Element::Text(text)) if text == " ") {
            elements.pop();
        }
        trim_wikidot_block_gaps(&mut elements);
    }

    if fused_close {
        elements.push(text!("]"));
    }

    let element = Element::Partial(PartialElement::ListItem(ListItem::Elements {
        elements,
        attributes,
    }));

    success_elements_with_paragraph_safety(false, element, errors)
}

fn wikidot_li_closer_line_is_quote_prefix(body: &str) -> bool {
    let tail = body.trim_end_matches([' ', '\t']);
    if tail.ends_with('\n') {
        return false;
    }
    let Some(newline) = tail.rfind('\n') else {
        return false;
    };
    let after = &tail[newline + 1..];
    !after.is_empty() && after.bytes().all(|byte| byte == b'>')
}

fn wikidot_li_break_saves_vanishing_item(elements: &[Element<'_>]) -> bool {
    !elements.is_empty()
        && elements.iter().all(|element| match element {
            Element::Container(container) => {
                container.ctype() == ContainerType::Div
                    && container.attributes().get().is_empty()
                    && container.elements().is_empty()
            }
            other => other.is_whitespace(),
        })
}

fn wikidot_li_segment_is_blank(segment: &[Element]) -> bool {
    segment.iter().all(Element::is_whitespace)
}

fn segment_is_inline_content(segment: &[Element]) -> bool {
    !segment.is_empty() && segment.iter().all(Element::paragraph_safe)
}

fn segment_has_block_marker(segment: &[Element]) -> bool {
    segment
        .iter()
        .any(|element| matches!(element, Element::Text(text) if text.contains("[[")))
}

fn wrap_inline_line_after_list(segment: &mut Vec<Element>) {
    let mut index = 0;
    while index < segment.len() {
        if !matches!(
            segment[index],
            Element::List {
                wikidot_explicit_block: false,
                ..
            }
        ) {
            index += 1;
            continue;
        }
        let start = index + 1;
        if start >= segment.len() || !segment[start].paragraph_safe() {
            index += 1;
            continue;
        }
        let mut end = start;
        while end < segment.len() && segment[end].paragraph_safe() {
            end += 1;
        }
        let run: Vec<Element> = segment.drain(start..end).collect();
        if !run.iter().any(
            |element| matches!(element, Element::Text(text) if !text.trim().is_empty()),
        ) {
            segment.splice(start..start, run);
            index = start + 1;
            continue;
        }
        segment.insert(
            start,
            Element::Container(Container::new(
                ContainerType::Paragraph,
                run,
                AttributeMap::new(),
            )),
        );
        index = start + 1;
    }
}

/// L85 scope-5: live keeps a single break after these formed blocks in
/// item bodies (li-div/align/code/coll/note-contained); every other
/// paragraph-unsafe element (tables, rules, lists) still drops it.
fn tabview_break_stays_in_open_span(segment: &[Element<'_>], index: usize) -> bool {
    if !matches!(segment.get(index.wrapping_sub(1)), Some(Element::TabView(_))) {
        return false;
    }
    let mut depth = 0i32;
    for element in &segment[..index] {
        match element {
            Element::Partial(PartialElement::InlineSpanOpen(..)) => depth += 1,
            Element::Partial(PartialElement::InlineSpanClose(_)) => depth -= 1,
            _ => {}
        }
    }
    depth > 0
}

fn wikidot_li_frame_depth_delta(element: &Element) -> i32 {
    match element {
        Element::Partial(PartialElement::WikidotBlockOpen(_)) => 1,
        Element::Partial(PartialElement::WikidotBlockClose(_)) => -1,
        _ => 0,
    }
}

fn wikidot_li_break_kept_after_block(element: &Element) -> bool {
    match element {
        Element::Container(container) => matches!(
            container.ctype(),
            ContainerType::Div | ContainerType::Align(_) | ContainerType::Note
        ),
        Element::Collapsible { .. } | Element::Code { .. } => true,
        _ => false,
    }
}

fn assemble_wikidot_li_segments<'t>(
    mut segments: Vec<Vec<Element<'t>>>,
    source: &str,
    in_bibliography_body: bool,
    note_yielded_list_closer: bool,
) -> Vec<Element<'t>> {
    let mut had_trailing_empty = false;
    while segments
        .last()
        .is_some_and(|segment| wikidot_li_segment_is_blank(segment))
    {
        segments.pop();
        had_trailing_empty = true;
    }
    let mut had_leading_empty = false;
    while segments
        .first()
        .is_some_and(|segment| wikidot_li_segment_is_blank(segment))
    {
        segments.remove(0);
        had_leading_empty = true;
    }
    for segment in &mut segments {
        let mut index = 1;
        let mut frame_depth = segment
            .first()
            .map_or(0, wikidot_li_frame_depth_delta);
        while index < segment.len() {
            let break_after_outermost_close = frame_depth == 0
                && matches!(
                    segment[index - 1],
                    Element::Partial(PartialElement::WikidotBlockClose(_))
                )
                && segment
                    .get(index + 1)
                    .is_some_and(|element| element.paragraph_safe());
            if segment[index] == Element::LineBreak
                && !segment[index - 1].paragraph_safe()
                && !wikidot_li_break_kept_after_block(&segment[index - 1])
                && !break_after_outermost_close
                && !tabview_break_stays_in_open_span(segment, index)
            {
                segment.remove(index);
            } else {
                frame_depth += wikidot_li_frame_depth_delta(&segment[index]);
                index += 1;
            }
        }
        wrap_inline_line_after_list(segment);
    }
    if let Some(first) = segments.first_mut() {
        while matches!(first.first(), Some(Element::LineBreak)) {
            first.remove(0);
        }
    }
    let mut kept: Vec<Vec<Element<'t>>> = Vec::with_capacity(segments.len());
    let last_index = segments.len().saturating_sub(1);
    for (index, segment) in segments.into_iter().enumerate() {
        if index > 0 && index < last_index && wikidot_li_segment_is_blank(&segment) {
            continue;
        }
        kept.push(segment);
    }
    let mut joins: Vec<bool> = Vec::new();
    for pair in kept.windows(2) {
        let boundary = pair[0]
            .last()
            .is_some_and(|element| !element.paragraph_safe())
            && pair[1].first().is_some_and(Element::paragraph_safe);
        joins.push(boundary);
    }
    // The slice includes the closer. A newline before that closer means
    // the closer sits on the next line.
    let lowered = source.to_ascii_lowercase();
    let body_before_close = lowered
        .rfind("[[/li")
        .map(|index| &source[..index])
        .unwrap_or(source);
    let last_plain_run_is_paragraph = match kept.as_slice() {
        [.., penultimate, last]
            if body_before_close.ends_with('\n')
                && segment_is_inline_content(last)
                && !segment_has_block_marker(last)
                && segment_is_inline_content(penultimate)
                && !segment_has_block_marker(penultimate) =>
        {
            true
        }
        _ => false,
    };
    let mut elements: Vec<Element<'t>> = Vec::new();
    let kept_last = kept.len().saturating_sub(1);
    for (index, segment) in kept.into_iter().enumerate() {
        if index > 0 && joins[index - 1] {
            elements.push(Element::LineBreak);
        }
        // A blank line between two plain inline runs, with the closer on the
        // next line, is a paragraph. A run on the closer's own line stays put,
        // and so does a run beside a literal block marker.
        let middle_inline =
            index > 0 && index < kept_last && segment_is_inline_content(&segment);
        let closing_paragraph = last_plain_run_is_paragraph && index == kept_last;
        if middle_inline || closing_paragraph {
            elements.push(Element::Container(Container::new(
                ContainerType::Paragraph,
                segment,
                AttributeMap::new(),
            )));
        } else {
            elements.extend(segment);
        }
    }
    if had_leading_empty || had_trailing_empty {
        strip_newlines(&mut elements);
    }
    // L85 scope-6: a trailing newline after a nested list stays raw on live
    // (no break element). A trailing blockquote does too. Other trailing
    // content keeps the break.
    // L85 scope-5: same for a trailing simple table (T5-item-table-end).
    let trailing_nested_list = matches!(elements.last(), Some(Element::List { .. }));
    let trailing_table = matches!(elements.last(), Some(Element::Table(_)));
    let trailing_blockquote = matches!(
        elements.last(),
        Some(Element::Container(container))
            if container.ctype() == ContainerType::Blockquote
    );
    if had_trailing_empty
        && elements
            .last()
            .is_some_and(|element| !element.paragraph_safe())
        && !trailing_nested_list
    {
        elements.push(Element::LineBreak);
    } else if !had_trailing_empty {
        let body = source
            .to_ascii_lowercase()
            .rfind("[[/li")
            .map(|index| &source[..index])
            .unwrap_or(source);
        let quote_prefix_saves_item = wikidot_li_closer_line_is_quote_prefix(body)
            && wikidot_li_break_saves_vanishing_item(&elements);
        if (body.ends_with('\n') || quote_prefix_saves_item)
            && !matches!(elements.last(), Some(Element::LineBreak))
            && !trailing_nested_list
            && !trailing_table
            && !trailing_blockquote
            && !note_yielded_list_closer
            && !in_bibliography_body
            && !trailing_wikidot_bibliography(&elements)
        {
            if let Some(Element::Container(container)) = elements.last_mut()
                && container.ctype() == ContainerType::Paragraph
            {
                if !matches!(container.elements().last(), Some(Element::LineBreak)) {
                    container.elements_mut().push(Element::LineBreak);
                }
            } else {
                elements.push(Element::LineBreak);
            }
        }
    }
    while matches!(elements.last(), Some(Element::Text(text)) if text == " ") {
        elements.pop();
    }
    elements
}

fn trim_wikidot_block_gaps(elements: &mut Vec<Element<'_>>) {
    let mut index = 1;
    while index + 1 < elements.len() {
        if elements[index] == Element::LineBreak
            && !elements[index - 1].paragraph_safe()
            && !elements[index + 1].paragraph_safe()
        {
            elements.remove(index);
        } else {
            index += 1;
        }
    }
}

/// L85: parse an implicit nested list at a scan-marked `[[li]]` inside a
/// div/collapsible body (l-ul-li-cross). Items reuse the li segment
/// machinery; the list ends at a foreign scan-paired close (propagated),
/// an explicit list close (propagated), or non-`[[li]]` content.
pub(crate) fn parse_wikidot_implicit_list_block<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    let mut items: Vec<ListItem<'t>> = Vec::new();
    let mut errors: Vec<ParseError> = Vec::new();
    loop {
        if parser.current().token == Token::InputEnd {
            break;
        }
        if parser.wikidot_li_cross_foreign_stop_here(&BLOCK_LI) {
            break;
        }
        if parser.current().token == Token::LeftBlockEnd {
            break;
        }
        if parser.current().token != Token::LeftBlock {
            break;
        }
        {
            let mut probe = parser.clone();
            let Ok((name, _, _)) = probe.get_block_name(false) else {
                break;
            };
            let name = name.strip_suffix('_').unwrap_or(&name);
            if !name.eq_ignore_ascii_case("li") {
                break;
            }
        }
        let head_pos = parser.current().span.start;
        let (_, in_head, _) = parser
            .get_block_name(false)
            .map_err(|_| parser.make_err(ParseErrorKind::RuleFailed))?;
        let arguments = parser.get_head_map_wikidot(&BLOCK_LI, in_head)?;
        let attributes = arguments.to_attribute_map(parser.settings());
        let body_start = parser.current().span.start;
        parser.push_wikidot_li_collect(Some(head_pos));
        let body = parser.get_wikidot_li_body_segments(&BLOCK_LI);
        parser.pop_wikidot_li_collect();
        let body = body?;
        let source_end = parser.current().span.start;
        let fused_close = parser.wikidot_body_closed_with_fused_close(&BLOCK_LI);
        let (segments, segment_errors, _) = body.into();
        errors.extend(segment_errors);
        let source = &parser.full_text().inner()[body_start..source_end];
        let mut elements = assemble_wikidot_li_segments(
            segments,
            source,
            parser.in_wikidot_bibliography_body(),
            parser.pending_note_list_closer(),
        );
        if parser.wikidot_li_closed_at_shallower_quote_line()
            && matches!(elements.last(), Some(Element::LineBreak))
        {
            elements.pop();
        }
        if fused_close {
            elements.push(text!("]"));
        }
        items.push(ListItem::Elements {
            elements,
            attributes,
        });
    }
    if items.is_empty() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let element = Element::List {
        ltype: ListType::Bullet,
        items,
        attributes: AttributeMap::new(),
        wikidot_explicit_block: true,
        wikidot_unclosed: false,
    };
    success_elements_with_paragraph_safety(false, element, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::ParseError;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    fn with_parse<R>(
        source: &str,
        check: impl for<'t> FnOnce(Vec<Element<'t>>, Vec<ParseError>) -> R,
    ) -> R {
        with_parse_layout(source, Layout::Wikidot, check)
    }

    fn with_parse_layout<R>(
        source: &str,
        layout: Layout,
        check: impl for<'t> FnOnce(Vec<Element<'t>>, Vec<ParseError>) -> R,
    ) -> R {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        check(tree.elements, errors)
    }

    fn element_text(elements: &[Element]) -> String {
        elements
            .iter()
            .filter_map(|element| match element {
                Element::Text(text) => Some(text.as_ref()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn wikidot_top_level_list_item_remains_literal_in_its_paragraph() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[li]]\nBaz\n[[/li]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(
            errors
                .iter()
                .any(|error| error.kind() == ParseErrorKind::ListItemOutsideList),
            "{errors:?}",
        );
        assert_eq!(html, "<p>[[li]]<br>\nBaz<br>\n[[/li]]</p>");
    }

    #[test]
    fn unordered_block_list_preserves_attributes_and_items() {
        with_parse(
            r#"[[ul class="menu"]]
[[li class="first"]]Alpha[[/li]]
[[li]]Beta[[/li]]
[[/ul]]"#,
            |tree, errors| {
                assert!(errors.is_empty(), "{errors:?}");
                let [
                    Element::List {
                        ltype,
                        attributes,
                        items,
                        ..
                    },
                    Element::LineBreak,
                ] = tree.as_slice()
                else {
                    panic!("expected one unordered list, got {tree:?}");
                };

                assert_eq!(*ltype, ListType::Bullet);
                assert_eq!(
                    attributes.get().get("class").map(|value| value.as_ref()),
                    Some("menu")
                );
                assert_eq!(items.len(), 2);

                let ListItem::Elements {
                    attributes,
                    elements,
                } = &items[0]
                else {
                    panic!("expected first list item, got {:?}", items[0]);
                };
                assert_eq!(
                    attributes.get().get("class").map(|value| value.as_ref()),
                    Some("first")
                );
                assert_eq!(element_text(elements), "Alpha");

                let ListItem::Elements { elements, .. } = &items[1] else {
                    panic!("expected second list item, got {:?}", items[1]);
                };
                assert_eq!(element_text(elements), "Beta");
            },
        );
    }

    #[test]
    fn wikidot_list_item_drops_blank_line_between_adjacent_blocks() {
        let source = "[[ul]]\n[[li]]\n[[div class=\"a\"]]\nA\n[[/div]]\n\n[[div class=\"b\"]]\nB\n[[/div]]\n[[/li]]\n[[/ul]]";
        with_parse(source, |tree, errors| {
            assert!(errors.is_empty(), "{errors:?}");
            let [Element::List { items, .. }, Element::LineBreak] = tree.as_slice()
            else {
                panic!("expected Wikidot list and trailing break, got {tree:?}");
            };
            let [ListItem::Elements { elements, .. }] = items.as_slice() else {
                panic!("expected one list item, got {items:?}");
            };
            let [
                Element::Container(first),
                Element::Container(second),
                Element::LineBreak,
            ] = elements.as_slice()
            else {
                panic!(
                    "expected adjacent block elements and a terminal break, got {elements:?}"
                );
            };
            assert_eq!(first.ctype(), ContainerType::Div);
            assert_eq!(second.ctype(), ContainerType::Div);
        });

        with_parse_layout(source, Layout::Wikijump, |tree, errors| {
            assert!(errors.is_empty(), "{errors:?}");
            let [Element::List { items, .. }] = tree.as_slice() else {
                panic!("expected Wikijump list, got {tree:?}");
            };
            let [ListItem::Elements { elements, .. }] = items.as_slice() else {
                panic!("expected one Wikijump list item, got {items:?}");
            };
            assert!(matches!(
                elements.as_slice(),
                [
                    Element::Container(_),
                    Element::LineBreak,
                    Element::Container(_),
                    ..
                ]
            ));
        });
    }

    #[test]
    fn ordered_block_list_accepts_nested_sublist() {
        with_parse(
            r#"[[ol]]
[[li]]Parent[[/li]]
[[ul]]
[[li]]Child[[/li]]
[[/ul]]
[[/ol]]"#,
            |tree, errors| {
                assert!(errors.is_empty(), "{errors:?}");
                let [Element::List { ltype, items, .. }, Element::LineBreak] =
                    tree.as_slice()
                else {
                    panic!("expected one ordered list, got {tree:?}");
                };

                assert_eq!(*ltype, ListType::Numbered);
                assert_eq!(items.len(), 1);
                let ListItem::Elements { elements, .. } = &items[0] else {
                    panic!("expected parent item, got {:?}", items[0]);
                };
                assert_eq!(element_text(elements), "Parent");

                let Some(element @ Element::List { .. }) = elements.last() else {
                    panic!("expected nested sublist in parent item, got {elements:?}");
                };
                let Element::List {
                    ltype,
                    attributes,
                    items,
                    ..
                } = element
                else {
                    panic!("expected nested list element, got {element:?}");
                };

                assert_eq!(*ltype, ListType::Bullet);
                assert!(attributes.get().is_empty());
                let [ListItem::Elements { elements, .. }] = items.as_slice() else {
                    panic!("expected one nested list item, got {items:?}");
                };
                assert_eq!(element_text(elements), "Child");
            },
        );
    }

    #[test]
    fn scored_list_keeps_nested_list_attached_after_literal_recovery() {
        with_parse(
            concat!(
                "[[ul_]]\n",
                "[[li_]]\n",
                "Parent\n",
                "[[/li]]\n",
                "[[ul]]\n",
                "[[li]]Child[[/li]]\n",
                "[[/ul]]\n",
                "[[/ul_]]",
            ),
            |tree, errors| {
                assert!(!errors.is_empty());
                let [Element::List { items, .. }] = tree.as_slice() else {
                    panic!("expected one list, got {tree:?}");
                };
                let [ListItem::Elements { elements, .. }] = items.as_slice() else {
                    panic!("expected one synthetic parent item, got {items:?}");
                };
                assert!(!elements.iter().any(|element| {
                    matches!(
                        element,
                        Element::Container(container)
                            if container.ctype() == ContainerType::Paragraph
                    )
                }));
                let Some(items) = elements.iter().find_map(|element| match element {
                    Element::List { items, .. } => Some(items),
                    _ => None,
                }) else {
                    panic!(
                        "expected nested list attached to parent item, got {elements:?}"
                    );
                };
                let [ListItem::Elements { elements, .. }] = items.as_slice() else {
                    panic!("expected one nested list item, got {items:?}");
                };
                assert_eq!(element_text(elements), "Child");
            },
        );
    }

    #[test]
    fn scored_block_strips_outer_line_breaks() {
        with_parse(
            r#"[[ul_]]
[[li]]
Alpha
[[/li]]
[[/ul]]"#,
            |tree, errors| {
                assert!(errors.is_empty(), "{errors:?}");
                let [Element::List { items, .. }] = tree.as_slice() else {
                    panic!("expected one list, got {tree:?}");
                };
                let [ListItem::Elements { elements, .. }] = items.as_slice() else {
                    panic!("expected one list item, got {items:?}");
                };

                assert!(
                    !elements
                        .iter()
                        .any(|element| matches!(element, Element::LineBreak))
                );
                assert_eq!(element_text(elements), "Alpha");
            },
        );
    }

    #[test]
    fn wikidot_scored_item_preserves_a_terminal_inline_raw_line_break() {
        let source = concat!(
            "[[ul_]]\n",
            "[[li]]\n",
            "Alpha\n",
            "@@ @@\n",
            "[[/li]]\n",
            "[[/ul]]",
        );
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            html,
            concat!(
                "<ul>\n",
                "<li>Alpha<br>\n",
                r#"<span style="white-space: pre-wrap;">&#32;</span><br>"#,
                "\n</li>\n",
                "</ul>",
            ),
        );
    }

    #[test]
    fn empty_block_list_becomes_a_trailing_line_break_like_wikidot() {
        for input in [
            "[[ul]][[/ul]]",
            "[[ul]]\n[[/ul]]",
            "[[ul]]   [[/ul]]",
            "[[ul]]\n \n[[/ul]]",
        ] {
            with_parse(input, |tree, errors| {
                assert!(errors.is_empty(), "{input:?}: {errors:?}");
                assert_eq!(tree, [Element::LineBreak], "{input:?}");
            });
        }
    }

    #[test]
    fn block_list_wraps_bare_body_like_wikidot() {
        with_parse("[[ul]]_[[/ul]]", |tree, errors| {
            assert!(errors.is_empty(), "{errors:#?}");
            let [Element::List { items, .. }, Element::LineBreak] = tree.as_slice()
            else {
                panic!("expected one list, got {tree:?}");
            };
            let [
                ListItem::Elements {
                    elements,
                    attributes,
                },
            ] = items.as_slice()
            else {
                panic!("expected one synthetic item, got {items:?}");
            };
            assert_eq!(element_text(elements), "_");
            assert_eq!(
                attributes.get().get("style").map(|value| value.as_ref()),
                Some("list-style: none"),
            );
        });
    }

    #[test]
    fn wikidot_rejects_spaced_and_scored_list_closers() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        for input in [
            "[[ul]]\n[[li]]spaced[[/li ]]\n[[/ul ]]",
            "[[ul_]]\n[[li_]]\ntext\n[[/li_]]\n[[/ul_]]",
        ] {
            let tokenization = crate::tokenize(input);
            let (tree, _errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;
            assert!(!html.contains("<ul>"), "{input:?}: {html}");
            assert!(html.contains("[[ul"), "{input:?}: {html}");
        }
    }

    #[test]
    fn malformed_scored_items_are_literal_only_in_wikidot_layout() {
        let source = concat!(
            "[[ul]]\n\n",
            "[[li_]]\nALPHA\n[[/li]]\n\n",
            "[[li_]]\n\nBETA\n\n[[/li]]\n",
            "[[li]]GAMMA[[/li]]\n\n",
            "[[li_]] DELTA [[/li]]\n\n",
            "[[/ul]]",
        );

        with_parse_layout(source, Layout::Wikidot, |tree, errors| {
            assert!(!errors.is_empty());
            let [Element::List { items, .. }, Element::LineBreak] = tree.as_slice()
            else {
                panic!("expected Wikidot list and trailing break, got {tree:?}");
            };
            let [
                ListItem::Elements {
                    elements: malformed,
                    attributes,
                },
                ListItem::Elements {
                    elements: gamma, ..
                },
            ] = items.as_slice()
            else {
                panic!("expected two Wikidot items, got {items:?}");
            };
            assert_eq!(
                attributes.get().get("style").map(|value| value.as_ref()),
                Some("list-style: none"),
            );
            assert_eq!(
                malformed
                    .iter()
                    .filter(|element| {
                        matches!(
                            element,
                            Element::Container(container)
                                if container.ctype() == ContainerType::Paragraph
                        )
                    })
                    .count(),
                2,
            );
            assert_eq!(element_text(gamma), "GAMMA[[li_]] DELTA [[/li]]");
        });

        with_parse_layout(source, Layout::Wikijump, |tree, errors| {
            assert!(errors.is_empty(), "{errors:?}");
            let [Element::List { items, .. }] = tree.as_slice() else {
                panic!("expected one Wikijump list, got {tree:?}");
            };
            assert_eq!(items.len(), 4);
            let text: Vec<_> = items
                .iter()
                .map(|item| match item {
                    ListItem::Elements { elements, .. } => element_text(elements),
                    ListItem::SubList { .. } => panic!("unexpected sublist"),
                })
                .collect();
            assert_eq!(text, ["ALPHA", "BETA", "GAMMA", "DELTA "]);
        });
    }

    #[test]
    fn multiline_item_newline_policy_is_wikidot_only() {
        let source = concat!(
            "[[ol]]\n",
            "[[li]]\nALPHA\n[[/li]]\n",
            "[[li]]\n\nBETA\n\n[[/li]]\n",
            "[[/ol]]",
        );

        with_parse_layout(source, Layout::Wikidot, |tree, errors| {
            assert!(errors.is_empty(), "{errors:?}");
            let [Element::List { items, .. }, Element::LineBreak] = tree.as_slice()
            else {
                panic!("expected Wikidot list and trailing break, got {tree:?}");
            };
            let [
                ListItem::Elements {
                    elements: alpha, ..
                },
                ListItem::Elements { elements: beta, .. },
            ] = items.as_slice()
            else {
                panic!("expected two Wikidot items, got {items:?}");
            };
            assert_eq!(alpha, &[text!("ALPHA"), Element::LineBreak]);
            assert_eq!(beta, &[text!("BETA")]);
        });

        with_parse_layout(source, Layout::Wikijump, |tree, errors| {
            assert!(errors.is_empty(), "{errors:?}");
            let [Element::List { items, .. }] = tree.as_slice() else {
                panic!("expected one Wikijump list, got {tree:?}");
            };
            let [
                ListItem::Elements {
                    elements: alpha, ..
                },
                ListItem::Elements { elements: beta, .. },
            ] = items.as_slice()
            else {
                panic!("expected two Wikijump items, got {items:?}");
            };
            assert_eq!(alpha, &[text!("ALPHA")]);
            assert_eq!(
                beta,
                &[Element::LineBreak, text!("BETA"), Element::LineBreak],
            );
        });
    }

    #[test]
    fn nested_list_attachment_is_wikidot_only() {
        let source = "[[ol]][[li]]Parent[[/li]][[ul]][[li]]Child[[/li]][[/ul]][[/ol]]";

        with_parse_layout(source, Layout::Wikidot, |tree, errors| {
            assert!(errors.is_empty(), "{errors:?}");
            let [Element::List { items, .. }, Element::LineBreak] = tree.as_slice()
            else {
                panic!("expected Wikidot list and trailing break, got {tree:?}");
            };
            let [ListItem::Elements { elements, .. }] = items.as_slice() else {
                panic!("expected one Wikidot parent item, got {items:?}");
            };
            assert!(matches!(elements.last(), Some(Element::List { .. })));
        });

        with_parse_layout(source, Layout::Wikijump, |tree, errors| {
            assert!(errors.is_empty(), "{errors:?}");
            let [Element::List { items, .. }] = tree.as_slice() else {
                panic!("expected one Wikijump list, got {tree:?}");
            };
            assert!(matches!(
                items.as_slice(),
                [ListItem::Elements { .. }, ListItem::SubList { .. }]
            ));
        });
    }
}
