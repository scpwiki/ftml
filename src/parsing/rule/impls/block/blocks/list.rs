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
use std::borrow::Cow;

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
        // BRIEF-MF65b: enclosing-list kind for the multi-formation
        // escape decision (nested lists push/pop their own kind).
        parser.push_enclosing_list_numbered(list_type == ListType::Numbered);
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
        parser.pop_enclosing_list_numbered();
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
    opener_start: usize,
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

    // Drop anchor-chrome strays from lists whose returns never ran;
    // entries of still-open outer lists are shallower and survive.
    if wikidot {
        parser.drop_wikidot_list_anchor_chromes_at(parser.wikidot_li_pairing_depth());
        parser.drop_wikidot_run_formations_at(parser.wikidot_li_pairing_depth());
    }

    let arguments = parser.get_head_map_wikidot(block_rule, in_head)?;
    let attributes = arguments.to_attribute_map(parser.settings());
    // lihoist p6 (discriminated per LT46 and MF65b probes): a list head on
    // an item line stays literal in live only when the enclosing explicit
    // list is UNORDERED, the head is not an `ol`, and the body has no deep
    // marker line; otherwise the head opens a lost-owner formation.
    let head_name = block_rule.accepts_names[0];
    let li_head_for_gate = parser.wikidot_li_collect_head();
    let gate_body_end = li_head_for_gate.and_then(|head| {
        crate::parsing::rule::impls::line_owner::own_line_li_close_after(
            parser.full_text().inner(),
            head,
        )
        .or_else(|| parser.wikidot_li_body_end_by_head(head))
    });
    let gate_has_deep = gate_body_end.is_some_and(|end| {
        crate::parsing::rule::impls::line_owner::body_has_deep_marker(
            parser.full_text().inner(),
            opener_start,
            end,
        )
    });
    if wikidot
        && !parser.wikidot_enclosing_list_numbered()
        && !head_name.eq_ignore_ascii_case("ol")
        && crate::parsing::rule::impls::line_owner::head_on_list_item_line(
            parser.full_text().inner(),
            opener_start,
        )
        && li_head_for_gate.is_some()
        && !gate_has_deep
        && !crate::parsing::rule::impls::line_owner::list_closer_in_range(
            parser,
            opener_start,
            gate_body_end.unwrap_or(parser.full_text().inner().len()),
        )
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if wikidot && parser.wikidot_li_pairing_depth() == 1 {
        parser.compute_wikidot_li_pairing();
        parser.compute_wikidot_li_cross_scan();
    }
    if parser.settings().layout.legacy() && !parser.has_body_end_block(block_rule) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    // Live leaves the opener literal when a bare-scheme [[iframe]] head on the
    // line before the closer absorbed every closer for the block. Item 88: an
    // unowned-dissolve candidate accepts either list close, so both names
    // count against the greedy-tail veto.
    let greedy_names: &[&str] = if parser.wikidot_li_cross_list_close() {
        &["ul", "ol"]
    } else {
        &[block_rule.accepts_names[0]]
    };
    if wikidot && parser.wikidot_greedy_tail_eats_closers(opener_start, greedy_names) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if wikidot
        && note_list_would_take_enclosing_closer(
            parser,
            opener_start,
            block_rule.accepts_names[0],
        )
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    // Depth-1 explicit lists in an advanced table cell get a stable
    // ordinal for cross-li break relocation (see `wikidot_div_list_ordinal`).
    let this_list_ordinal = if wikidot
        && parser.wikidot_li_pairing_depth() == 1
        && parser.in_wikidot_advanced_table_cell_body()
    {
        Some(parser.wikidot_cell_list_ordinal())
    } else {
        None
    };

    let body = if wikidot
        && let Some(depth) = parser.native_blockquote_depth()
        && (parser.has_native_blockquote_body_end(block_rule, depth)
            || parser.has_surplus_quote_list_close(block_rule, depth)
            || parser.has_shallower_quote_list_close(block_rule, depth)
            || (matches!(
                block_rule.name,
                "block-list-unordered" | "block-list-ordered"
            ) && parser.has_shallower_quote_li_close(depth))
            || (matches!(
                block_rule.name,
                "block-list-unordered" | "block-list-ordered"
            ) && parser.has_shallower_quote_text_line(block_rule, depth))
            || (matches!(
                block_rule.name,
                "block-list-unordered" | "block-list-ordered"
            ) && parser.has_shallower_quote_evaporation(block_rule, depth))
            || (matches!(
                block_rule.name,
                "block-list-unordered" | "block-list-ordered"
            ) && parser.has_li_close_before_list_close(depth).is_some()))
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
    let list_end = parser.current().span.start;
    let closer_has_same_line_residual = !matches!(
        parser.current().token,
        Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
    );
    // A closer on a list-item line (`* [[/ul]]`, `# [[/ul]]`) leaves no
    // post-list break in live: with a bullet marker the break is dropped,
    // with a numbered marker an empty unclosed anchor list takes its
    // place (LT46 s-family probes).
    let closer_line_marker = if wikidot && !fused_close {
        crate::parsing::rule::impls::line_owner::consumed_closer_line_marker(
            parser,
            list_end,
            block_rule.accepts_names[0],
        )
    } else {
        None
    };
    let (mut elements, mut errors, _) = body.into();
    // BRIEF-UE57b: pre-li soft-end left the body empty. Live dissolves
    // the list entirely — emit nothing instead of the
    // dissolve-to-break the ordinary empty body uses.
    if parser.take_wikidot_evaporated_list() {
        return ok!(false; Elements::None, errors);
    }

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

    // BRIEF-MF65b: anchor chromes stamped at this list's depth by numbered
    // lost-owner formations closing in its body; counted before the take,
    // because the escaped case moves them to the pending slot.
    let hoist_count = if wikidot {
        parser.wikidot_list_anchor_chromes_len_at(parser.wikidot_li_pairing_depth())
    } else {
        0
    };
    // Anchor lists for decomposed numbered lost-owner formations inside
    // this list's body (LT46 p23-ord): re-emitted after this list's
    // close when it is numbered (the L85 slot below), dropped otherwise.
    let anchor_chromes = if wikidot {
        parser.take_wikidot_list_anchor_chromes_at(parser.wikidot_li_pairing_depth())
    } else {
        Vec::new()
    };

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

    // Cross-li splice transfer keeps the emptied yielding item in `items`
    // so same-list truncation and whole-list ejection below can address
    // it. Only when the fallback transfer actually fired.
    let cross_keep_empty = wikidot
        && parser.wikidot_explicit_list_yield_tail_peek()
        && parser.has_wikidot_li_tail_transfer()
        && parser.wikidot_cell_div_li_opener().is_some()
        && parser.wikidot_table_wrapped_by_flat_block();

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
                    || cross_keep_empty
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

    // The div list's ordinal is recorded as soon as any list owning the
    // div item assembles, even before the yielding list is parsed, so
    // cell-stage break relocation can find it. The pairing scan starts
    // just after the `[[ul]]` head, so this list's direct li heads sit
    // at scan depth 0.
    if wikidot
        && this_list_ordinal.is_some()
        && let Some(div_opener) = parser.wikidot_cell_div_li_opener()
    {
        let (all_heads, _) = parser.wikidot_li_items_at_depth(0);
        if all_heads
            .iter()
            .any(|head| *head > opener_start && *head < list_end && *head == div_opener)
        {
            parser.record_wikidot_div_list_ordinal(this_list_ordinal.unwrap());
        }
    }

    // Cross-li splice transfer (list-assembly half): eject the yielding
    // list's content strictly after the first div item. Same-list keeps
    // the prefix and stashes the intermediates; a separate yielding list
    // ejects whole under the outer-wrap slot.
    if wikidot
        && parser.wikidot_explicit_list_yield_tail_peek()
        && parser.has_wikidot_li_tail_transfer()
        && parser.wikidot_table_wrapped_by_flat_block()
        && let Some(div_opener) = parser.wikidot_cell_div_li_opener()
    {
        let (all_heads, _) = parser.wikidot_li_items_at_depth(0);
        let heads: Vec<usize> = all_heads
            .iter()
            .copied()
            .filter(|head| *head > opener_start && *head < list_end)
            .collect();
        if heads.len() == items.len() {
            if let Some(ordinal) = this_list_ordinal
                && heads.iter().any(|head| *head == div_opener)
            {
                parser.record_wikidot_div_list_ordinal(ordinal);
            }
            match heads.iter().position(|head| *head == div_opener) {
                Some(div_index) if div_index + 1 < items.len() => {
                    let ejected = items.split_off(div_index + 1);
                    let mut eject_vec: Vec<Element<'t>> = Vec::new();
                    for item in ejected {
                        match item {
                            ListItem::Elements { elements, .. } => {
                                eject_vec.extend(elements);
                            }
                            ListItem::SubList { element } => {
                                eject_vec.push(*element);
                            }
                        }
                    }
                    parser.extend_wikidot_cross_li_eject(eject_vec);
                    parser.set_wikidot_cross_li_fired();
                }
                Some(_) => {}
                None => {
                    // Separate yielding list: eject whole, suppress the
                    // cell element, stash the outer for re-wrapping.
                    let outer = Element::List {
                        ltype: list_type,
                        items: std::mem::take(&mut items),
                        attributes: attributes.clone(),
                        wikidot_explicit_block: true,
                        wikidot_unclosed: false,
                    };
                    parser.set_wikidot_cross_li_outer(outer);
                    parser.set_wikidot_cross_li_fired();
                    // Keep the yielding out of the cell: return no element.
                    return ok!(false; Elements::None, errors);
                }
            }
        } else if matches!(
            items.last(),
            Some(ListItem::Elements { elements, .. }) if elements.is_empty()
        ) {
            // Heads do not line up (literal items in play): restore the
            // pre-cross behaviour by dropping the kept empty yielding item.
            items.pop();
        }
    }

    // BRIEF-MF65b: the multi-formation escape — live closes the outer
    // numbered list at the first numbered lost-owner formation's consumed
    // starred close; the remaining items and elements render at top level.
    let mut escape_tail: Option<Vec<Element<'t>>> = None;
    if wikidot && !flag_score && list_type == ListType::Numbered && b5_escaped.is_none() {
        escape_tail = multi_formation_escape(parser, &mut items, hoist_count);
        if escape_tail.is_some() {
            parser.set_wikidot_tail_mode(0);
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

    let mut element = Element::List {
        ltype: list_type,
        items,
        attributes,
        wikidot_explicit_block: parser.settings().layout.legacy(),
        wikidot_unclosed: false,
    };
    // s7 (65d): run items after a matching closer rejoin the list item
    // set. A `* [[/ul]]` closer on a Bullet list absorbs following `*`
    // run items; a `# [[/ol]]` closer on a Numbered list absorbs `#`
    // items. No blank line may separate them (the blank-line follower
    // case stays split). Absorption uses the native list rule so each
    // item's content is parsed identically to a normal run item.
    // The opener must not be on a list-item line: when `* [[ul]]`
    // appears inside a native bullet run (pool idx 3732), the following
    // `*` lines belong to the outer run, not to this block's rejoin.
    let opener_on_list_item_line = wikidot
        && crate::parsing::rule::impls::line_owner::list_item_line_marker(
            parser.full_text().inner(),
            opener_start,
        )
        .is_some();
    let rejoin_marker =
        if wikidot && !nested && !flag_score && !fused_close && !opener_on_list_item_line
        {
            match (closer_line_marker, &element) {
                (
                    Some('*'),
                    Element::List {
                        ltype: ListType::Bullet,
                        ..
                    },
                ) => Some(ListType::Bullet),
                (
                    Some('#'),
                    Element::List {
                        ltype: ListType::Numbered,
                        ..
                    },
                ) => Some(ListType::Numbered),
                _ => None,
            }
        } else {
            None
        };
    if let Some(rejoin_ltype) = rejoin_marker
        && !closer_has_same_line_residual
        && !parser.body_end_boundary_accepts("iftags")
        && !parser.has_pending_iftags_end_ahead()
    {
        // Try to absorb consecutive native run items of the same type.
        // Skip only the closer's own line break (not a paragraph break).
        let mut absorb = (**parser).clone();
        // Position: after the closer was consumed by get_body_elements.
        // If we're at a line break, step past it to reach the next line.
        if absorb.current().token == Token::LineBreak {
            let _ = absorb.step();
        }
        // Only rejoin if the next line starts a matching native list item
        // (no paragraph break intervened).
        if absorb.start_of_line()
            && matches!(
                absorb.current().token,
                Token::BulletItem | Token::NumberedItem
            )
        {
            let next_ltype = match absorb.current().token {
                Token::BulletItem => ListType::Bullet,
                Token::NumberedItem => ListType::Numbered,
                _ => ListType::Generic,
            };
            if next_ltype == rejoin_ltype
                && let Ok(success) =
                    crate::parsing::rule::impls::list::RULE_LIST.try_consume(&mut absorb)
            {
                let (extra_elements, extra_errors, _): (
                    Elements<'t>,
                    Vec<ParseError>,
                    bool,
                ) = success.into();
                let extra_elements: Vec<Element<'t>> =
                    extra_elements.into_iter().collect();
                if let Element::List {
                    items: ref mut base_items,
                    ..
                } = element
                {
                    for extra in extra_elements {
                        match extra {
                            Element::List {
                                items: extra_items,
                                ltype: extra_lt,
                                ..
                            } if extra_lt == rejoin_ltype => {
                                base_items.extend(extra_items);
                            }
                            _ => {
                                // Unexpected structure from RULE_LIST; do not absorb.
                            }
                        }
                    }
                    if !extra_errors.is_empty() {
                        errors.extend(extra_errors);
                    }
                    let _ = &mut *parser;
                    parser.update(&absorb);
                }
            }
        }
    }
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
    // The escaped tail renders right after the list's close element.
    if let Some(escape_tail) = escape_tail {
        // BRIEF-MF65b: the closing list's anchor chrome — the first
        // formation's — renders after the escaped region in live (input
        // end). Anchors of escaped formations render nothing in live.
        // Item 86(a): with a post-list follower at absolute end, live
        // suppresses the chrome and paragraph-wraps the follower (the
        // pw65a follower-wrap family reaches the escaped path here).
        let stash_formed = list_type == ListType::Numbered && !anchor_chromes.is_empty();
        let follower_wrap = closer_line_marker.is_some() || stash_formed;
        let content_after_list = (parser.wikidot_next_line_has_content()
            || (follower_wrap
                && parser.wikidot_next_line_has_content_skipping_blanks()))
            && !parser.body_end_boundary_accepts("iftags")
            && !parser.has_pending_iftags_end_ahead();
        if wikidot && !nested && follower_wrap {
            parser.record_wikidot_post_list_follower_wrap(stash_formed);
        }
        if stash_formed && !content_after_list {
            let mut chromes = anchor_chromes;
            let first = chromes.remove(0);
            parser.stash_pending_tail_chromes(vec![first]);
        }
        let mut output = vec![element];
        if fused_close {
            output.push(text!("]"));
        }
        output.extend(escape_tail);
        return ok!(false; output, errors);
    }
    // pw65a: a marker-line closer or a stashed numbered lost-owner anchor
    // chrome marks this list as re-attaching to its follower as a paragraph
    // in live — even across blank lines (probe rounds pw65a-1/2/3, o5/r3c/
    // r3e with blank-separated prose). The blank-tolerant peek applies only
    // to those shapes; plain/degraded lists keep the untolerant check.
    let stash_formed = !anchor_chromes.is_empty();
    let follower_wrap = closer_line_marker.is_some() || stash_formed;
    let content_after_list = (parser.wikidot_next_line_has_content()
        || (follower_wrap && parser.wikidot_next_line_has_content_skipping_blanks()))
        && !parser.body_end_boundary_accepts("iftags")
        && !parser.has_pending_iftags_end_ahead();
    if wikidot && !nested && follower_wrap {
        parser.record_wikidot_post_list_follower_wrap(stash_formed);
    }
    if parser.settings().layout.legacy()
        && !nested
        && !flag_score
        && !closer_has_same_line_residual
        && !content_after_list
        && !parser.pending_note_list_closer()
        && !parser.wikidot_cross_list_terminated_body()
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
            // s7 (65d): a type-matching closer (`*` on ul, `#` on ol) drops
            // the break and enables rejoin; a `#` closer on a ul keeps the
            // anchor (s6). An unmatched `*` on ol drops the break without
            // rejoin/anchor. BL79 (e): live drops the closer-line anchor
            // before a following block (code, div, table, …); keep it only
            // when no block follows (EOF / heading takes the content path).
            let closer_anchor = matches!(
                (closer_line_marker, list_type),
                (Some('#'), ListType::Bullet)
            ) && !parser.wikidot_next_skipping_blanks_is_block();
            let closer_drops_break = closer_line_marker.is_some() && !closer_anchor;
            if closer_anchor {
                output.push(Element::List {
                    ltype: ListType::Bullet,
                    items: Vec::new(),
                    attributes: AttributeMap::new(),
                    wikidot_explicit_block: true,
                    wikidot_unclosed: true,
                });
            }
            // A numbered list re-emits the anchor lists stashed by
            // decomposed numbered lost-owner formations in the break's
            // place (LT46 p23-ord). With neither suppressed nor stashed
            // material, the ordinary post-list break remains.
            if list_type == ListType::Numbered && !anchor_chromes.is_empty() {
                output.extend(anchor_chromes);
            } else if !closer_drops_break && !closer_anchor {
                output.push(Element::LineBreak);
            }
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
        // Item 91(c), Wikidot layout only (`append_to_last` carries the
        // legacy-layout flag at every call site): a degraded literal item
        // whose first element is a blockquote — i.e. a quote row deeper
        // than the list's own quote level preceded the degraded head in
        // source — renders with `display: inline` on live (saved-page
        // probes muse-probe-di91-*: content-agnostic across blockquote
        // paragraphs, native/explicit lists, tables, headings; order
        // sensitive — a deeper row after the head, or any same-depth row
        // before it, keeps the plain none style). Same-depth block
        // children (e.g. a formed table first) do not trigger it. The
        // gate keeps the Wikijump-layout degraded trees (which form such
        // items for shapes Wikidot pairs cleanly) byte-identical to base.
        let style = if append_to_last
            && elements.first().is_some_and(|element| {
                matches!(
                    element,
                    Element::Container(container)
                        if container.ctype() == ContainerType::Blockquote
                )
            }) {
            "list-style: none; display: inline"
        } else {
            "list-style: none"
        };
        let mut attributes = AttributeMap::new();
        assert!(attributes.insert("style", cow!(style)));
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
/// Whether an explicit `[[li]]` body's last non-blank line is a block closer
/// for a NON-list block (e.g. `* [[/div]]` whose formation dissolved into a
/// native run). In that case the dissolved formation's trailing break is
/// kept after the resulting list, unlike after a `* [[/ul]]`/`[[li]]` close.
/// Item 65(c): unwrap a dissolved unowned-formation run list in an
/// explicit li body. Returns per-li element vecs (first = this li's
/// new content, rest = sibling split items), or None if not applicable.
fn unowned_dissolve_unwrap<'t>(
    elements: &mut Vec<Element<'t>>,
    _li_attributes: &AttributeMap<'t>,
) -> Option<Vec<Vec<Element<'t>>>> {
    // Sole-element: the entire li body is the dissolved run list.
    if elements.len() == 1
        && let Some(Element::List {
            items,
            wikidot_explicit_block: false,
            wikidot_unclosed: false,
            ..
        }) = elements.first()
    {
        let items = items.clone();
        let out: Vec<Vec<Element<'t>>> = items
            .into_iter()
            .map(|item| match item {
                ListItem::Elements { elements, .. } => elements,
                ListItem::SubList { element } => vec![*element],
            })
            .collect();
        return if out.is_empty() { None } else { Some(out) };
    }
    // Mixed: find a dissolved run list element and split around it.
    // The dissolve marker was taken for this body range, so a
    // non-explicit run-formed list here is the dissolution product.
    let pos = elements.iter().rposition(|e| {
        matches!(
            e,
            Element::List {
                wikidot_explicit_block: false,
                wikidot_unclosed: false,
                ..
            }
        )
    })?;
    let Element::List { items, .. } = elements.remove(pos) else {
        return None;
    };
    let dissolved: Vec<Vec<Element<'t>>> = items
        .into_iter()
        .map(|item| match item {
            ListItem::Elements { elements, .. } => elements,
            ListItem::SubList { element } => vec![*element],
        })
        .collect();
    if dissolved.is_empty() {
        return None;
    }
    // Everything before the dissolved list stays in this li (possibly
    // mixed with trailing content after `pos` which is rare; keep it).
    // For the after-text shape, preceding bare text in the li body
    // becomes this li's content, and dissolved items follow as siblings.
    let mut result = Vec::with_capacity(1 + dissolved.len());
    let mut first = std::mem::take(elements);
    // If `before` content is just bare text/inline that should form its
    // own li, keep it as-is (the li's attributes apply). The dissolved
    // items become sibling lis.
    result.push(first);
    result.extend(dissolved);
    Some(result)
}

fn wikidot_li_body_ends_with_flat_block_close(source: &str) -> bool {
    let lowered = source.to_ascii_lowercase();
    let body = lowered
        .rfind("[[/li")
        .map(|index| &source[..index])
        .unwrap_or(source);
    let Some(line) = body.lines().rev().find(|line| !line.trim().is_empty()) else {
        return false;
    };
    let line = line.trim_end_matches([' ', '\t']);
    if !line.ends_with("]]") {
        return false;
    }
    let Some(head) = line.rfind("[[") else {
        return false;
    };
    let Some(close) = line[head..].strip_prefix("[[") else {
        return false;
    };
    let name = close.strip_prefix('/').unwrap_or(close);
    let name = name.strip_suffix("]]").unwrap_or(name);
    let name = name.split_whitespace().next().unwrap_or(name);
    !matches!(name, "ul" | "ol" | "li")
}

/// NL81 (D): an empty native marker line (`* ` / `# ` with no content)
/// takes the item body's closing break with it on live.
fn wikidot_li_body_ends_with_empty_native_marker(source: &str) -> bool {
    let lowered = source.to_ascii_lowercase();
    let body = lowered
        .rfind("[[/li")
        .map(|index| &source[..index])
        .unwrap_or(source);
    let Some(line) = body.lines().rev().find(|line| !line.trim().is_empty()) else {
        return false;
    };
    let line = line.trim_end_matches('\r');
    let trimmed_start = line.trim_start_matches([' ', '\t']);
    // `* ` or `# ` (marker + at least one space) with no further content.
    // `*` / `#` alone is NOT an empty marker line that takes the break.
    let rest = if let Some(r) = trimmed_start.strip_prefix("* ") {
        r
    } else if let Some(r) = trimmed_start.strip_prefix("# ") {
        r
    } else {
        return false;
    };
    rest.trim().is_empty()
}

fn wikidot_li_text_is_bare_block_marker(text: &str) -> bool {
    let text = text.trim();
    if !text.starts_with("[[") || !text.ends_with("]]") {
        return false;
    }
    let inner = &text[2..text.len() - 2];
    let inner = inner.strip_prefix('/').unwrap_or(inner);
    let name = inner.split_whitespace().next().unwrap_or(inner);
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn wikidot_li_is_inline_marker_partial(element: &Element<'_>) -> bool {
    matches!(
        element,
        Element::Partial(
            PartialElement::InlineSizeOpen(..)
                | PartialElement::InlineSizeClose(_)
                | PartialElement::InlineSpanOpen(..)
                | PartialElement::InlineSpanClose(_)
                | PartialElement::InlineFormatOpen(_)
                | PartialElement::InlineFormatClose(_)
                | PartialElement::InlineColorOpen(..)
                | PartialElement::InlineColorClose
                | PartialElement::InlineAnchorOpen(..)
                | PartialElement::InlineAnchorClose(_)
        )
    )
}

/// NL81 (C): marker-only nested native list at item end — every direct
/// item is only a bare block marker (possibly split across text nodes or
/// inline marker partials). Live drops the trailing break.
fn wikidot_li_marker_only_nested_list(element: &Element<'_>) -> bool {
    let Element::List { items, .. } = element else {
        return false;
    };
    !items.is_empty()
        && items.iter().all(|item| match item {
            ListItem::Elements { elements, .. } => {
                let mut joined = String::new();
                let mut saw_marker_partial = false;
                for e in elements {
                    match e {
                        Element::Text(t) => joined.push_str(t),
                        Element::LineBreak => {}
                        _ if wikidot_li_is_inline_marker_partial(e) => {
                            saw_marker_partial = true;
                        }
                        _ => return false,
                    }
                }
                let joined = joined.trim();
                if joined.is_empty() {
                    saw_marker_partial
                } else {
                    wikidot_li_text_is_bare_block_marker(joined)
                }
            }
            ListItem::SubList { .. } => false,
        })
}

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

    // Post-yield row-tail items: once an explicit list yields at the splice,
    // the row-tail [[li]] heads after it are live-structural items whose
    // content ejects to the foster as lists (wrapper- and div-independent;
    // probes l1-plain/l2-nowrap/l4-li3 and the frozen laterli shape). They
    // assemble like list items (li segment assembly, so trailing-break
    // handling matches list membership) but return as foster content
    // instead of a ListItem, which row extract does not accept.
    let post_yield_scope = wikidot
        && parser.wikidot_li_pairing_depth() == 0
        && parser.wikidot_explicit_list_yield_tail_peek();

    if wikidot
        && (parser.wikidot_li_pairing_depth() > 0 || post_yield_scope)
        && !parser.discarding_hidden_body()
    {
        parser.push_wikidot_li_collect(Some(opener_start));
        let body = parser.get_wikidot_li_body_segments(&BLOCK_LI);
        parser.pop_wikidot_li_collect();
        let body = body?;
        let source_end = parser.current().span.start;
        let fused_close = parser.wikidot_body_closed_with_fused_close(&BLOCK_LI);
        let (segments, errors, _) = body.into();
        // Cross-li splice transfer trigger (first-wins, cell-scoped): a
        // completed `[[div]]` pair inside this item's body arms the later
        // yielding item in the same cell.
        if wikidot
            && parser.in_wikidot_advanced_table_cell_body()
            && parser.wikidot_cell_div_li_opener().is_none()
            && segments.iter().any(|segment| {
                segment.iter().any(|element| {
                    matches!(
                        element,
                        Element::Partial(PartialElement::WikidotBlockClose(
                            WikidotBlockCloseKind::Div { .. }
                        ))
                    )
                })
            })
        {
            parser.record_wikidot_cell_div_li_opener(opener_start);
        }
        let source = &parser.full_text().inner()[body_start..source_end];
        let consumed = parser.wikidot_consumed_li_close();
        let terminated_at_cross_close = (parser.current().token == Token::LeftBlockEnd
            && parser
                .wikidot_li_cross_pair_at(parser.current().span.start)
                .is_some_and(|(_, crossed)| crossed))
            || consumed.is_some();
        let enclosing_block = parser.wikidot_flat_pair_open_at(body_start);
        // NL81 (E): take shadowed-div-region signal before assembly so the
        // region-end break can be positioned. Clear swallowed-block's
        // all-linebreaks flag state separately.
        let shadowed_div_region = !post_yield_scope
            && parser.take_wikidot_li_shadowed_div_region_at(body_start, source_end);
        let mut elements = assemble_wikidot_li_segments(
            segments,
            source,
            parser.in_wikidot_bibliography_body(),
            parser.pending_note_list_closer()
                || parser.wikidot_cross_list_terminated_body(),
            terminated_at_cross_close,
            parser.in_wikidot_advanced_table_cell_body(),
            enclosing_block,
            shadowed_div_region,
        );
        // NL81 (E, all-linebreaks-body): no content survived inside the
        // shadowed region — the region-end break goes before any content
        // that followed the region (or keeps the item alive when empty).
        // Only prepend when the li does not already start with a formed
        // block (a swallowed div nested inside other structures must not
        // inject a break at the outer li's start).
        if !post_yield_scope
            && parser.take_wikidot_li_swallowed_block_at(body_start, source_end)
        {
            let starts_with_formed_block = elements.first().is_some_and(|element| {
                matches!(
                    element,
                    Element::Container(_)
                        | Element::Collapsible { .. }
                        | Element::Code { .. }
                        | Element::Table(_)
                        | Element::Module(_)
                        | Element::HorizontalRule
                        | Element::ContentSeparator
                        | Element::Math { .. }
                        | Element::TabView(_)
                        | Element::List { .. }
                        | Element::Html { .. }
                        | Element::Iframe { .. }
                        | Element::Include { .. }
                        | Element::BibliographyBlock { .. }
                        | Element::Partial(PartialElement::WikidotBlockOpen(_))
                        | Element::Partial(PartialElement::WikidotFoster(_))
                )
            });
            if elements.is_empty() {
                elements.push(Element::LineBreak);
            } else if !starts_with_formed_block {
                elements.insert(0, Element::LineBreak);
            }
        }
        if consumed.is_some() {
            parser.clear_wikidot_consumed_li_close();
        }
        if parser.wikidot_li_closed_at_shallower_quote_line()
            && matches!(elements.last(), Some(Element::LineBreak))
        {
            elements.pop();
        }
        if fused_close {
            elements.push(text!("]"));
        }
        // Post-yield row-tail items eject as foster content (see the
        // post_yield_scope decision above); keep placement before the
        // run-formation splits, which are list-item specific.
        if post_yield_scope {
            let element = Element::Partial(PartialElement::WikidotFoster(elements));
            return success_elements_with_paragraph_safety(false, element, errors);
        }
        // Item 65(c): an unowned lost-owner list formation dissolved its
        // content into this explicit li. Unwrap the dissolved run list so
        // its items become this li's content / split into sibling items.
        if parser.take_wikidot_li_unowned_dissolve_at(body_start, source_end) {
            if let Some(mut output) = unowned_dissolve_unwrap(&mut elements, &attributes)
            {
                // output[0] replaces this li's elements; rest are sibling partials
                if output.len() == 1 {
                    elements = output.pop().unwrap();
                } else {
                    let first = output.remove(0);
                    let element =
                        Element::Partial(PartialElement::ListItem(ListItem::Elements {
                            elements: first,
                            attributes: attributes.clone(),
                        }));
                    let mut result = vec![element];
                    for elems in output {
                        result.push(Element::Partial(PartialElement::ListItem(
                            ListItem::Elements {
                                elements: elems,
                                attributes: AttributeMap::new(),
                            },
                        )));
                    }
                    return success_elements_with_paragraph_safety(
                        false,
                        Elements::Multiple(result),
                        errors,
                    );
                }
            }
        }
        // lihoist p23: a body folding to a single native run list whose only
        // items are depth-2+ marker lines is no formation in live — the
        // lines dissolve to a paragraph when a starred list pair formation
        // closes inside the body, and to bare text otherwise.
        if elements.len() == 1
            && let Some(Element::List { items, .. }) = elements.first()
            && !items.is_empty()
            && items.iter().all(|item| {
                matches!(
                    item,
                    ListItem::Elements { elements: inner, .. }
                        if !inner.is_empty()
                            && deep_marker_head(&inner[0])
                            && inner.iter().all(|element| matches!(element, Element::Text(_) | Element::LineBreak))
                )
            })
        {
            let deep_formation =
                crate::parsing::rule::impls::line_owner::starred_list_closer_in_source(
                    source,
                );
            let [Element::List { items, .. }] = elements.as_slice() else {
                unreachable!()
            };
            let inner: Vec<Element<'t>> = items
                .iter()
                .flat_map(|item| match item {
                    ListItem::Elements { elements: inner, .. } => inner.clone(),
                    ListItem::SubList { .. } => Vec::new(),
                })
                .collect();
            if deep_formation {
                elements = vec![Element::Container(Container::new(
                    ContainerType::Paragraph,
                    inner,
                    AttributeMap::new(),
                ))];
            } else {
                elements = inner;
            }
        }
        // lihoist p23-early (LT46): an explicit-list body item that is a
        // depth-2+ marker line (a `(none)`-styled literal item starting at
        // the deep line) escapes the single-list body into paragraph
        // segments in live: the deep line becomes its own paragraph, the
        // remaining item lines reassemble into native lists, dissolving
        // single-list wrappers (the run's fuse/promote semantics).
        // Starred or unstarred closers behave the same (p23-early/e3/e4
        // probes); the rule keys on the literal item, not the closer.
        let mixed_deep = elements.len() == 1
            && matches!(elements.first(), Some(Element::List { ltype: ListType::Bullet, items, .. })
            if items.iter().any(|item| {
                matches!(item, ListItem::Elements { attributes, elements }
                    if attributes.get().get("style").is_some_and(|style| style == "list-style: none")
                        && elements.first().is_some_and(deep_marker_head))
            }));
        if mixed_deep {
            let single = elements
                .first()
                .cloned()
                .expect("checked single element above");
            if let Some(segments) = split_mixed_deep_marker_list(single) {
                elements = segments;
            }
        }
        // Live ends a run formation inside an explicit [[li]] body at the
        // completing pair's closer: run items after the closer escape to
        // the enclosing list as its items instead of re-forming a second
        // list inside this item (lihoist p12/p15/d6-24.3). When the
        // assembled body is exactly [formation list, native tail list],
        // split the tail list's items out as sibling items. The escape
        // requires a recorded explicit-list end inside this body (the
        // completing pair's closer); without one the tail list is a
        // re-typed continuation of the run (e.g. `* a` then `# b`) and live
        // keeps both lists nested inside the item.
        let core_end = elements
            .iter()
            .rposition(|element| !element.is_whitespace())
            .map_or(0, |index| index + 1);
        let plain_native = |items: &Vec<ListItem<'t>>| {
            !items.is_empty()
                && items.iter().all(|item| {
                    matches!(
                        item,
                        ListItem::Elements { elements, .. }
                            if elements
                                .iter()
                                .all(|element| matches!(element, Element::Text(_)))
                    )
                })
        };
        let escaped_items: Option<Vec<ListItem<'t>>> = match &elements[..core_end] {
            [
                Element::List {
                    items: head_items,
                    wikidot_explicit_block: false,
                    ..
                },
                Element::List {
                    items: tail_items,
                    wikidot_explicit_block: false,
                    ..
                },
            ] if plain_native(head_items) && plain_native(tail_items) => {
                Some(tail_items.clone())
            }
            _ => None,
        };
        // The escape requires a recorded explicit-list end inside this body
        // (the completing pair's closer); see the comment above.
        let completing_pair_in_body = !parser
            .wikidot_explicit_list_ends_between(body_start, source_end)
            .is_empty();
        let escaped_items = escaped_items.filter(|_| completing_pair_in_body);
        if let Some(tail_items) = escaped_items {
            elements.truncate(core_end - 1);
            // The removed tail list's leading line break joins the escaped
            // items, matching the break the enclosing list puts between items.
            let element =
                Element::Partial(PartialElement::ListItem(ListItem::Elements {
                    elements,
                    attributes,
                }));
            let mut output = vec![element];
            output.extend(
                tail_items
                    .into_iter()
                    .map(|item| Element::Partial(PartialElement::ListItem(item))),
            );
            return success_elements_with_paragraph_safety(
                false,
                Elements::Multiple(output),
                errors,
            );
        }
        // Item-level closer escape (d6-24.3): the completing explicit-list
        // pair can end a run formation in the middle of the tail list, with
        // the items after its closer belonging to the enclosing list while
        // the items before it stay in this item. Unlike the whole-tail
        // escape above, the head list may carry a block frame (the d6-24
        // note), so only the tail is required to be plain; the split itself
        // is purely positional — the first recorded explicit-list end inside
        // this body that straddles the tail items' borrowed-text offsets.
        // No end (a lone closer, lihoist p19) or no straddle (the note-split
        // pin) means no escape.
        let partial_escape: Option<(usize, Vec<ListItem<'t>>)> =
            match &elements[..core_end] {
                [
                    Element::List {
                        wikidot_explicit_block: false,
                        ..
                    },
                    Element::List {
                        items: tail_items,
                        wikidot_explicit_block: false,
                        ..
                    },
                    rest @ ..,
                ] if plain_native(tail_items)
                    && rest.iter().all(|element| {
                        element.is_whitespace()
                            || matches!(
                                element,
                                Element::Partial(PartialElement::WikidotBlockClose(_))
                            )
                    }) =>
                {
                    super::wikidot_straddle_split_at_explicit_list_end(
                        parser, tail_items, body_start, source_end,
                    )
                    .map(|split| (split, tail_items[split..].to_vec()))
                }
                _ => None,
            };
        if let Some((split, escaped_items)) = partial_escape {
            if let Some(Element::List { items, .. }) = elements.get_mut(1) {
                items.truncate(split);
            }
            let element =
                Element::Partial(PartialElement::ListItem(ListItem::Elements {
                    elements,
                    attributes,
                }));
            let mut output = vec![element];
            output.extend(
                escaped_items
                    .into_iter()
                    .map(|item| Element::Partial(PartialElement::ListItem(item))),
            );
            return success_elements_with_paragraph_safety(
                false,
                Elements::Multiple(output),
                errors,
            );
        }
        // A run formation that never fused (no complete explicit list pair)
        // leaves its lone list-closer line as an escaped paragraph; live
        // keeps the closer as an item of the run list instead (lihoist
        // p19). Reattach it when the escaped element right after a plain
        // native list is a paragraph holding one literal list closer.
        if elements.len() == 2
            && let [
                Element::List {
                    items,
                    wikidot_explicit_block: false,
                    ..
                },
                Element::Container(closer),
            ] = &elements[..]
            && closer.ctype() == ContainerType::Paragraph
            && items.iter().all(|item| {
                matches!(
                    item,
                    ListItem::Elements { elements, .. }
                        if !elements.is_empty()
                            && elements
                                .iter()
                                .all(|element| matches!(element, Element::Text(_)))
                )
            })
        {
            let text: String = closer
                .elements()
                .iter()
                .filter_map(|element| match element {
                    Element::Text(text) => Some(text.as_ref()),
                    _ => None,
                })
                .collect();
            if matches!(text.as_str(), "[[/li]]" | "[[/ul]]" | "[[/ol]]") {
                let paragraph = elements.pop().expect("paragraph just matched");
                let Element::Container(paragraph) = paragraph else {
                    unreachable!()
                };
                let closer_text: String = paragraph
                    .elements()
                    .iter()
                    .filter_map(|element| match element {
                        Element::Text(text) => Some(text.as_ref()),
                        _ => None,
                    })
                    .collect();
                if let Some(Element::List { items: target, .. }) = elements.last_mut() {
                    target.push(ListItem::Elements {
                        attributes: AttributeMap::new(),
                        elements: vec![Element::Text(Cow::Owned(closer_text))],
                    });
                }
            }
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

fn deep_marker_head(element: &Element) -> bool {
    matches!(element, Element::Text(text) if text.starts_with("**") || text.starts_with("##"))
}

/// lihoist p23-early (LT46): splits one list whose items mix deep-marker
/// literal lines with other item lines into li-body segments: each deep
/// line becomes its own paragraph, the remaining items regroup into
/// native lists (run fuse/promote semantics, so single-list wrappers
/// dissolve). Returns None (keep the body as-is) for line interiors the
/// saved-page probes did not cover, e.g. text after a nested list or a
/// non-inline block inside a deep line's item.
fn split_mixed_deep_marker_list<'t>(list: Element<'t>) -> Option<Vec<Element<'t>>> {
    let Element::List {
        ltype,
        items,
        attributes,
        ..
    } = list
    else {
        unreachable!("checked bullet list above")
    };
    let mut segments: Vec<Element<'t>> = Vec::new();
    let mut group: Vec<ListItem<'t>> = Vec::new();
    let flush_group = |group: &mut Vec<ListItem<'t>>, segments: &mut Vec<Element<'t>>| {
        if !group.is_empty() {
            segments.push(Element::List {
                ltype,
                items: std::mem::take(group),
                attributes: attributes.clone(),
                wikidot_explicit_block: false,
                wikidot_unclosed: false,
            });
        }
    };
    for item in items {
        let is_deep = matches!(&item, ListItem::Elements { attributes, elements }
            if attributes.get().get("style").is_some_and(|style| style == "list-style: none")
                && elements.first().is_some_and(deep_marker_head));
        if !is_deep {
            group.extend(crate::parsing::rule::impls::list::fuse_wikidot_run_item(
                item,
            ));
            continue;
        }
        let ListItem::Elements { elements, .. } = item else {
            unreachable!("checked deep item above")
        };
        let mut paragraph: Vec<Element<'t>> = Vec::new();
        let mut seen_nested_list = false;
        let mut valid = true;
        for element in elements {
            match element {
                Element::List {
                    items: inner_items, ..
                } => {
                    if !paragraph.is_empty() {
                        let mut para = std::mem::take(&mut paragraph);
                        while matches!(para.last(), Some(Element::LineBreak)) {
                            para.pop();
                        }
                        if !para.is_empty() {
                            flush_group(&mut group, &mut segments);
                            segments.push(Element::Container(Container::new(
                                ContainerType::Paragraph,
                                para,
                                AttributeMap::new(),
                            )));
                        }
                    }
                    for inner in inner_items {
                        group.extend(
                            crate::parsing::rule::impls::list::fuse_wikidot_run_item(
                                inner,
                            ),
                        );
                    }
                    seen_nested_list = true;
                }
                element if element.paragraph_safe() => {
                    if seen_nested_list {
                        valid = false;
                        break;
                    }
                    paragraph.push(element);
                }
                _ => {
                    valid = false;
                    break;
                }
            }
        }
        if !valid {
            return None;
        }
        if !paragraph.is_empty() {
            let mut para = paragraph;
            while matches!(para.last(), Some(Element::LineBreak)) {
                para.pop();
            }
            if !para.is_empty() {
                flush_group(&mut group, &mut segments);
                segments.push(Element::Container(Container::new(
                    ContainerType::Paragraph,
                    para,
                    AttributeMap::new(),
                )));
            }
        }
    }
    flush_group(&mut group, &mut segments);
    (!segments.is_empty()).then_some(segments)
}

/// lihoist p23: a depth-2+ marker line (e.g. `** deep`) becomes its own
/// paragraph segment when a list follows it in the segment or a starred
/// list pair formation closes inside the body; otherwise it stays a bare
/// continuation (g2-style).
fn wrap_inline_line_after_list(
    segment: &mut Vec<Element>,
    formation_present: bool,
    in_cell: bool,
) {
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
        let mut run: Vec<Element> = segment.drain(start..end).collect();
        let deep_paragraph = !run.is_empty()
            && deep_marker_head(&run[0])
            && !segment_has_block_marker(&run)
            && (formation_present
                || matches!(
                    segment.get(start),
                    Some(Element::List {
                        wikidot_explicit_block: false,
                        ..
                    })
                ));
        if !deep_paragraph
            && ((!in_cell && !segment_has_block_marker(&run))
                || !run.iter().any(
                    |element| matches!(element, Element::Text(text) if !text.trim().is_empty()),
                ))
        {
            segment.splice(start..start, run);
            index = start + 1;
            continue;
        }
        if deep_paragraph {
            // The break that ended the deep line does not belong inside its
            // paragraph (p23-after-item).
            while matches!(run.last(), Some(Element::LineBreak)) {
                run.pop();
            }
        }
        // Outside a table cell, only an inline run carrying a literal block
        // marker (e.g. a lone `[[/hidden]]` line after a list) is
        // paragraph-wrapped; a plain continuation line stays bare beside
        // the list. Inside an advanced table cell, live paragraph-wraps
        // every post-list text run (LP42: tail/zzz/pre**/midlexer shapes).
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
    if !matches!(
        segment.get(index.wrapping_sub(1)),
        Some(Element::TabView(_))
    ) {
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

/// L85: live emits no break after a trailing block in an item body
/// L85: live emits no break after a trailing math block in an item body
/// (saved-page probes muse-probe-mi6x2-item-math). Formed blocks drop
/// the trailing break with a trailing blank before [[/li]] (divblank/noteblank/codeblank/collblank/alignblank; probes muse-probe-mi74-*),
/// and [[toc]] drops it in both cases (item-toc/tocblank; probes muse-probe-mi74-toc-*).
fn wikidot_li_trailing_break_dropped_after_math(element: &Element) -> bool {
    matches!(element, Element::Math { .. })
}

fn wikidot_li_trailing_break_dropped_after_block_blank(element: &Element) -> bool {
    match element {
        Element::Math { .. } => true,
        Element::Collapsible { .. } | Element::Code { .. } => true,
        Element::TableOfContents { .. } => true,
        Element::Container(container) => matches!(
            container.ctype(),
            ContainerType::Div | ContainerType::Align(_) | ContainerType::Note
        ),
        _ => false,
    }
}

/// Drop trailing break in normal case (no trailing blank)
fn wikidot_li_trailing_break_dropped_after_block_always(element: &Element) -> bool {
    match element {
        Element::Math { .. } => true,
        Element::TableOfContents { .. } => true,
        _ => false,
    }
}

fn assemble_wikidot_li_segments<'t>(
    mut segments: Vec<Vec<Element<'t>>>,
    source: &str,
    in_bibliography_body: bool,
    note_yielded_list_closer: bool,
    terminated_at_cross_close: bool,
    in_cell: bool,
    enclosing_block: bool,
    shadowed_div_region: bool,
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
        let mut frame_depth = segment.first().map_or(0, wikidot_li_frame_depth_delta);
        while index < segment.len() {
            let break_after_outermost_close = frame_depth == 0
                && matches!(
                    segment[index - 1],
                    Element::Partial(PartialElement::WikidotBlockClose(_))
                )
                && segment
                    .get(index + 1)
                    .is_some_and(|element| element.paragraph_safe());
            // NL81 (E): a shadowed div region's end break (live puts <br>
            // where the region ended, before text after the region).
            let shadowed_region_end_break = shadowed_div_region
                && matches!(segment[index - 1], Element::List { .. })
                && segment
                    .get(index + 1)
                    .is_some_and(|element| element.paragraph_safe());
            let shadowed_insert_break = shadowed_div_region
                && matches!(segment[index - 1], Element::List { .. })
                && segment[index] != Element::LineBreak
                && segment[index].paragraph_safe();
            if shadowed_insert_break {
                segment.insert(index, Element::LineBreak);
                frame_depth += wikidot_li_frame_depth_delta(&segment[index]);
                index += 1;
            } else if segment[index] == Element::LineBreak
                && !segment[index - 1].paragraph_safe()
                && !wikidot_li_break_kept_after_block(&segment[index - 1])
                && !break_after_outermost_close
                && !tabview_break_stays_in_open_span(segment, index)
                && !shadowed_region_end_break
            {
                segment.remove(index);
            } else {
                frame_depth += wikidot_li_frame_depth_delta(&segment[index]);
                index += 1;
            }
        }
        wrap_inline_line_after_list(
            segment,
            crate::parsing::rule::impls::line_owner::starred_list_closer_in_source(
                source,
            ),
            in_cell,
        );
    }
    if let Some(first) = segments.first_mut() {
        while matches!(first.first(), Some(Element::LineBreak)) {
            first.remove(0);
        }
    }
    let mut kept: Vec<Vec<Element<'t>>> = Vec::with_capacity(segments.len());
    // A segment boundary in this path is always a swallowed blank line (the
    // li body consumer splits segments only at ParagraphBreak). Live emits no
    // break across the boundary for paragraph-unsafe formed blocks (probes
    // muse-probe-mi6x4/5-*: div/note/code/coll/align/math, blank, then text),
    // but keeps one after an inner list (test/list/item-blank-after-block-break).
    let last_index = segments.len().saturating_sub(1);
    for (index, segment) in segments.into_iter().enumerate() {
        if index > 0 && index < last_index && wikidot_li_segment_is_blank(&segment) {
            continue;
        }
        kept.push(segment);
    }
    let mut joins: Vec<bool> = Vec::new();
    for pair in kept.windows(2) {
        // Only an inner list keeps the break across a swallowed blank line
        // (test/list/item-blank-after-block-break); formed blocks and math
        // join with nothing (probes muse-probe-mi6x4/5-*).
        let boundary = matches!(pair[0].last(), Some(Element::List { .. }))
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
        } else if index > 0 {
            // Blank-line segment boundaries inside a flat-shell body are
            // paragraph boundaries for the post-heading wrap below; the
            // empty-text marker is consumed there and never renders.
            elements.push(Element::Text(std::borrow::Cow::Borrowed("")));
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
    // Exception: when the li body's last line closes a NON-list block (for
    // example a `* [[/div]]` line whose formation dissolved into the list),
    // live keeps the break after the list (lihoist p5).
    // NL81 (C): marker-only nested native list drops the trailing break.
    let marker_only_trailing_nested_list = {
        let effective = elements.iter().rev().find(|element| {
            !matches!(
                element,
                Element::Partial(PartialElement::WikidotBlockClose(_))
            )
        });
        effective.is_some_and(wikidot_li_marker_only_nested_list)
    };
    let trailing_nested_list = {
        let effective = elements.iter().rev().find(|element| {
            !matches!(
                element,
                Element::Partial(PartialElement::WikidotBlockClose(_))
            )
        });
        match effective {
            Some(Element::List { .. }) => {
                marker_only_trailing_nested_list
                    || !wikidot_li_body_ends_with_flat_block_close(source)
            }
            _ => false,
        }
    };
    let trailing_table = matches!(elements.last(), Some(Element::Table(_)));
    let trailing_blockquote = matches!(
        elements.last(),
        Some(Element::Container(container))
            if container.ctype() == ContainerType::Blockquote
    );
    // Live drops the trailing break after trailing blocks in an item body
    let trailing_block_drops_break_blank = elements
        .last()
        .is_some_and(wikidot_li_trailing_break_dropped_after_block_blank);
    let trailing_block_drops_break_always = elements
        .last()
        .is_some_and(wikidot_li_trailing_break_dropped_after_block_always);
    // d446-neighbors: after a heading, rule, or separator at item top level
    // live puts the trailing break after the whole list, not inside the item
    // (a4/a16/a17, h3/h4/h10); [[...]]-formed blocks and quote/table keep it
    // inside (h1-h8).
    let trailing_line_block = matches!(
        elements.last(),
        Some(Element::Container(container))
            if matches!(container.ctype(), ContainerType::Header(_))
    ) || matches!(
        elements.last(),
        Some(Element::HorizontalRule | Element::ContentSeparator)
    );
    let empty_marker_takes_break = !terminated_at_cross_close
        && wikidot_li_body_ends_with_empty_native_marker(source);
    if !terminated_at_cross_close
        && had_trailing_empty
        && elements
            .last()
            .is_some_and(|element| !element.paragraph_safe())
        && !trailing_nested_list
        && !trailing_line_block
        && !trailing_table
        && !trailing_block_drops_break_blank
    {
        elements.push(Element::LineBreak);
    } else if !had_trailing_empty && !terminated_at_cross_close {
        let body = source
            .to_ascii_lowercase()
            .rfind("[[/li")
            .map(|index| &source[..index])
            .unwrap_or(source);
        let quote_prefix_saves_item = wikidot_li_closer_line_is_quote_prefix(body)
            && wikidot_li_break_saves_vanishing_item(&elements);
        let deep_trailing_paragraph = elements.last().is_some_and(|element| {
            matches!(
                element,
                Element::Container(container)
                    if container.ctype() == ContainerType::Paragraph
                        && container
                            .elements()
                            .first()
                            .is_some_and(deep_marker_head)
            )
        });
        if (body.ends_with('\n') || quote_prefix_saves_item)
            && !matches!(elements.last(), Some(Element::LineBreak))
            && !trailing_nested_list
            && !trailing_table
            && !trailing_blockquote
            && !trailing_line_block
            && !trailing_block_drops_break_always
            && !note_yielded_list_closer
            && !in_bibliography_body
            && !trailing_wikidot_bibliography(&elements)
            && !deep_trailing_paragraph
            && !empty_marker_takes_break
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
    if empty_marker_takes_break && matches!(elements.last(), Some(Element::LineBreak)) {
        elements.pop();
    }
    if marker_only_trailing_nested_list
        && !terminated_at_cross_close
        && matches!(elements.last(), Some(Element::LineBreak))
    {
        elements.pop();
    }
    while matches!(elements.last(), Some(Element::Text(text)) if text == " ") {
        elements.pop();
    }
    wrap_post_heading_segments(&mut elements, enclosing_block);
    elements
}

/// BRIEF-F7W63 (ledger item 63): live wraps the inline runs that follow a
/// heading inside a list item in `<p>`s when the item sits inside a block
/// body (note/div/collapsible/align; crossed or contained), matching the
/// top-level block-body rule (probe n4-n7/k1/u1). The gate, per live
/// saved-page evidence (probe-f7w63.json):
///
/// - `soft`: a chunk terminated by a block that can open a paragraph after
///   itself is wrapped even without an enclosing block frame (hr, separator,
///   math, quote, note/div formed in the item, or another heading).
/// - `enclosing_block`: without a following block, a trailing chunk is
///   wrapped only when an enclosing block frame is open across the item
///   (`wikidot_flat_pair_open_at`); a table line directly after a chunk
///   keeps it bare in both cases, and a code/align block suppresses the
///   wrap for the rest of the item.
///
/// Empty-text markers (`Text("")`) left by the segment merge are blank-line
/// paragraph boundaries consumed here; no marker survives this pass.
fn wrap_post_heading_segments<'t>(
    elements: &mut Vec<Element<'t>>,
    enclosing_block: bool,
) {
    fn has_content(chunk: &[Element<'_>]) -> bool {
        chunk.iter().any(|element| {
            !matches!(element, Element::LineBreak | Element::LineBreaks(_))
                && !matches!(element, Element::Text(text) if text.trim().is_empty())
        })
    }

    enum Flush {
        /// A block that can resume paragraph flow (hr, separator, math, quote,
        /// note/div/collapsible formed in the item, or another heading).
        Soft,
        /// A table line: the chunk directly before it stays bare.
        BeforeTable,
        /// A blank-line segment boundary: wrapped only under a frame.
        SegmentGap,
        /// A code or align block: wraps the chunk before it, then latches.
        CodeOrAlign,
        /// End of the item body.
        End,
    }

    let mut out: Vec<Element<'t>> = Vec::with_capacity(elements.len() + 4);
    let mut chunk: Vec<Element<'t>> = Vec::new();
    let mut after_heading = false;
    let mut latched = false;
    let flush = |chunk: &mut Vec<Element<'t>>,
                 out: &mut Vec<Element<'t>>,
                 after_heading: bool,
                 latched: bool,
                 enclosing_block: bool,
                 kind: Flush| {
        if chunk.is_empty() {
            return;
        }
        let wrap = after_heading
            && !latched
            && has_content(chunk)
            && match kind {
                Flush::BeforeTable => false,
                Flush::SegmentGap | Flush::End => enclosing_block,
                Flush::Soft | Flush::CodeOrAlign => true,
            };
        if wrap {
            out.push(Element::Container(Container::new(
                ContainerType::Paragraph,
                std::mem::take(chunk),
                AttributeMap::new(),
            )));
        } else {
            out.append(chunk);
        }
    };
    for element in std::mem::take(elements) {
        let is_boundary_marker =
            matches!(&element, Element::Text(text) if text.is_empty());
        let is_heading = matches!(
            &element,
            Element::Container(container) if matches!(container.ctype(), ContainerType::Header(_))
        );
        let is_table = matches!(&element, Element::Table(_));
        let is_code_or_align = matches!(&element, Element::Code(_))
            || matches!(
                &element,
                Element::Container(container)
                    if matches!(container.ctype(), ContainerType::Align(_))
            );
        if is_boundary_marker {
            flush(
                &mut chunk,
                &mut out,
                after_heading,
                latched,
                enclosing_block,
                Flush::SegmentGap,
            );
            continue;
        }
        if is_heading {
            flush(
                &mut chunk,
                &mut out,
                after_heading,
                latched,
                enclosing_block,
                Flush::Soft,
            );
            out.push(element);
            after_heading = true;
            continue;
        }
        if is_table {
            flush(
                &mut chunk,
                &mut out,
                after_heading,
                latched,
                enclosing_block,
                Flush::BeforeTable,
            );
            out.push(element);
            continue;
        }
        if is_code_or_align {
            flush(
                &mut chunk,
                &mut out,
                after_heading,
                latched,
                enclosing_block,
                Flush::CodeOrAlign,
            );
            out.push(element);
            latched = true;
            continue;
        }
        if !element.paragraph_safe() {
            flush(
                &mut chunk,
                &mut out,
                after_heading,
                latched,
                enclosing_block,
                Flush::Soft,
            );
            out.push(element);
            continue;
        }
        chunk.push(element);
    }
    flush(
        &mut chunk,
        &mut out,
        after_heading,
        latched,
        enclosing_block,
        Flush::End,
    );
    *elements = out;
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
        let consumed = parser.wikidot_consumed_li_close();
        let terminated_at_cross_close = (parser.current().token == Token::LeftBlockEnd
            && parser
                .wikidot_li_cross_pair_at(parser.current().span.start)
                .is_some_and(|(_, crossed)| crossed))
            || consumed.is_some();
        let enclosing_block = parser.wikidot_flat_pair_open_at(body_start);
        let shadowed_div_region = parser.take_wikidot_li_shadowed_div_region_at(body_start, source_end);
        let mut elements = assemble_wikidot_li_segments(
            segments,
            source,
            parser.in_wikidot_bibliography_body(),
            parser.pending_note_list_closer()
                || parser.wikidot_cross_list_terminated_body(),
            terminated_at_cross_close,
            parser.in_wikidot_advanced_table_cell_body(),
            enclosing_block,
            shadowed_div_region,
        );
        if parser.take_wikidot_li_swallowed_block_at(body_start, source_end) {
            if elements.is_empty() {
                elements.push(Element::LineBreak);
            } else {
                elements.insert(0, Element::LineBreak);
            }
        }
        if consumed.is_some() {
            parser.clear_wikidot_consumed_li_close();
        }
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
/// BRIEF-MF65b: performs the multi-formation escape restructure.
/// Returns the elements that render at top level after this list's close,
/// or `None` when the restructure does not apply (no numbered lost-owner
/// hoist, or nothing follows the first one).
fn multi_formation_escape<'t>(
    parser: &Parser<'_, 't>,
    items: &mut Vec<ListItem<'t>>,
    hoists: usize,
) -> Option<Vec<Element<'t>>> {
    if hoists == 0 {
        return None;
    }
    // The escape fires only when the FIRST list formation in this body is a
    // numbered lost-owner formation (live closes the enclosing numbered
    // list there). A leading bullet formation or explicit list keeps the
    // choreography nested.
    if parser.wikidot_first_formation_numbered_at(parser.wikidot_li_pairing_depth())
        != Some(true)
    {
        return None;
    }
    // The first numbered lost-owner formation sits inside the first item;
    // its flipped list is the item's first non-explicit list.
    let first_list = match items.first() {
        Some(ListItem::Elements { elements, .. }) => {
            elements.iter().position(|element| {
                matches!(
                    element,
                    Element::List {
                        wikidot_explicit_block: false,
                        ..
                    }
                )
            })
        }
        _ => return None,
    };
    let Some(first_list) = first_list else {
        return None;
    };
    let split = first_list + 1;
    let kept_len = match &items[0] {
        ListItem::Elements { elements, .. } => elements.len(),
        _ => return None,
    };
    if kept_len == split && items.len() == 1 {
        // Nothing follows the first formation.
        return None;
    }
    let (elements, attributes) = match std::mem::replace(
        &mut items[0],
        ListItem::Elements {
            elements: Vec::new(),
            attributes: AttributeMap::new(),
        },
    ) {
        ListItem::Elements {
            elements,
            attributes,
        } => (elements, attributes),
        _ => unreachable!("the split requires elements items"),
    };
    let mut elements = elements;
    let rest_elements = elements.split_off(split);
    items[0] = ListItem::Elements {
        elements,
        attributes,
    };
    let rest_items = items.split_off(1);
    Some(multi_formation_escaped_tail(
        rest_elements,
        rest_items,
        hoists - 1,
    ))
}

/// BRIEF-MF65b: regroups the escaped tail into renderable elements.
/// Nested flipped formation lists render as numbered lists unless a plain
/// text-like element has already appeared in the tail (then the bullet run
/// wins, matching live) or every numbered hoist is consumed. Items fused
/// into the would-be outer list regroup into one escaped list per hoist.
fn multi_formation_escaped_tail<'t>(
    rest_elements: Vec<Element<'t>>,
    rest_items: Vec<ListItem<'t>>,
    mut hoists: usize,
) -> Vec<Element<'t>> {
    let mut out: Vec<Element<'t>> = Vec::new();
    let mut pending: Vec<ListItem<'t>> = Vec::new();
    let mut text_seen = false;

    fn flush<'t>(
        pending: &mut Vec<ListItem<'t>>,
        hoists: &mut usize,
        text_seen: bool,
        out: &mut Vec<Element<'t>>,
    ) {
        if pending.is_empty() {
            return;
        }
        let ltype = if *hoists > 0 && !text_seen {
            *hoists -= 1;
            ListType::Numbered
        } else {
            ListType::Bullet
        };
        out.push(Element::List {
            ltype,
            items: std::mem::take(pending),
            attributes: AttributeMap::new(),
            wikidot_explicit_block: false,
            wikidot_unclosed: false,
        });
    }

    for element in rest_elements {
        match element {
            Element::List {
                ltype,
                items,
                attributes,
                wikidot_explicit_block,
                wikidot_unclosed,
            } => {
                flush(&mut pending, &mut hoists, text_seen, &mut out);
                let ltype = if hoists > 0 && !text_seen {
                    hoists -= 1;
                    ListType::Numbered
                } else {
                    ltype
                };
                out.push(Element::List {
                    ltype,
                    items,
                    attributes,
                    wikidot_explicit_block,
                    wikidot_unclosed,
                });
            }
            other => {
                text_seen = true;
                out.push(other);
            }
        }
    }
    for item in rest_items {
        pending.push(item);
    }
    flush(&mut pending, &mut hoists, text_seen, &mut out);
    out
}
