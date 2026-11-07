/*
 * parsing/rule/impls/list.rs
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

use super::footnote_first::{starts_footnote, unowned_footnote_first};
use super::line_owner::{
    LineOwner, try_consume_literal_list_alignment, try_consume_lost_owner_block,
};
use super::prelude::*;
use crate::parsing::collect::{collect_consume_keep_split, consume_valid_comment};
use crate::parsing::paragraph::ParagraphStack;
use crate::parsing::parser::QuoteBodyLineStatus;
use crate::parsing::{
    DepthItem, DepthList, ParagraphSplitEntry, ParagraphSplitKind, ParseSuccess,
    process_depths,
};
use crate::tree::{AttributeMap, Element, ListItem, ListType, PartialElement};

const MAX_LIST_DEPTH: usize = 128;

const fn get_list_type(token: Token) -> Option<ListType> {
    match token {
        Token::BulletItem => Some(ListType::Bullet),
        Token::NumberedItem => Some(ListType::Numbered),
        _ => None,
    }
}

enum ListItemStep<'t> {
    End,
    Item((usize, ListType, Vec<Element<'t>>), bool),
    Fallback(Vec<Element<'t>>),
    Split {
        control: Option<(usize, ListType, Vec<Element<'t>>)>,
        escaped: Vec<Element<'t>>,
        escaped_split: Option<Vec<ParagraphSplitEntry>>,
        escaped_safe: bool,
    },
    SplitItems {
        control: Option<(usize, ListType, Vec<Element<'t>>)>,
        items: Vec<(usize, ListType, Vec<Element<'t>>)>,
        trailing: Option<(Vec<Element<'t>>, Vec<ParagraphSplitEntry>, bool)>,
    },
}

pub const RULE_LIST: Rule = Rule {
    name: "list",
    position: LineRequirement::StartOfLine,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    if parser.wikidot_footnote_zone_blocks(parser.current().span.start) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if parser.wikidot_scored_div_first_line_suppresses_at(parser.current().span.start) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let entry_parser = parser.clone();
    let entry_state = parser.get_mutable_state();

    let mut start_parser = parser.clone();
    if let Some(start_depth) = parse_list_depth(&mut start_parser)? {
        if start_depth > MAX_LIST_DEPTH {
            return Err(parser.make_err(ParseErrorKind::ListDepthExceeded));
        }
        if start_depth > 0 {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
    }

    let mut depths = Vec::new();
    let mut errors = Vec::new();
    let mut escaped_elements = Vec::new();
    let mut gate_escape: Option<(Vec<ParagraphSplitEntry>, bool)> = None;

    let mut paragraph_safe = false;

    let mut ended = false;
    while !ended {
        if !depths.is_empty() && skip_wikidot_list_comment_lines(parser, &depths)? {
            continue;
        }
        // The item collector stops on the line break, so the next row is still
        // a `>` token. Strip it before deciding the list has ended.
        if !depths.is_empty()
            && parser.in_note()
            && parser.prepare_quote_body_line()? == QuoteBodyLineStatus::Boundary
        {
            ended = true;
            continue;
        }
        match parse_next_list_item(
            parser,
            &mut errors,
            &mut paragraph_safe,
            !depths.is_empty(),
        )? {
            ListItemStep::End => ended = true,
            ListItemStep::Item(item, ends_run) => {
                depths.push(item);
                // A quote-owned note already stripped the `>` prefix, so the
                // depth field must not turn every bullet into its own list.
                ended = ends_run
                    || parser.native_blockquote_depth().is_some() && !parser.in_note();
            }
            ListItemStep::Fallback(elements) => {
                debug_assert!(depths.is_empty());
                return ok!(true; elements, errors);
            }
            ListItemStep::Split {
                control,
                escaped,
                escaped_split,
                escaped_safe,
            } => {
                if let Some(control) = control {
                    depths.push(control);
                }
                escaped_elements.extend(escaped);
                if escaped_split.is_some() {
                    gate_escape = escaped_split.map(|split| (split, escaped_safe));
                }
                ended = true;
            }
            ListItemStep::SplitItems {
                control,
                items,
                trailing,
            } => {
                if let Some(control) = control {
                    depths.push(control);
                }
                depths.extend(items);
                if let Some((escaped, split, safe)) = trailing {
                    escaped_elements.extend(escaped);
                    gate_escape = Some((split, safe));
                    ended = true;
                }
            }
        }
    }

    if escaped_elements.is_empty()
        && wikidot_list_ends_with_scored_alias(
            parser,
            entry_parser.current().span.start,
            &depths,
        )
    {
        parser.update(&entry_parser);
        parser.reset_mutable_state(entry_state);
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if escaped_elements.is_empty()
        && parser.settings().layout.legacy()
        && starts_footnote(parser)
    {
        let only_item = depths.len() == 1;
        if let Some((_, list_type, elements)) = depths.last_mut() {
            let mut candidate = parser.clone_with_rule(RULE_LIST);
            if let Ok(footnote) = super::RULE_BLOCK.try_consume(&mut candidate)
                && let Elements::Single(Element::Footnote(index)) = footnote.item
            {
                if elements.is_empty() && only_item {
                    let marker = match list_type {
                        ListType::Bullet => "*",
                        ListType::Numbered => "#",
                        ListType::Generic => {
                            unreachable!("native item has a concrete type")
                        }
                    };
                    errors.extend(footnote.errors);
                    parser.update(&candidate);
                    return ok!(
                        true;
                        vec![text!(marker), Element::Footnote(index)],
                        errors,
                    );
                }
                if !elements.is_empty() {
                    elements.push(Element::Footnote(index));
                    errors.extend(footnote.errors);
                    parser.update(&candidate);
                }
            }
        }
    }

    if depths.is_empty() && escaped_elements.is_empty() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let depths = retain_active_nested_list_types(depths);
    let depth_lists = process_depths(ListType::Generic, depths);
    let mut elements: Vec<Element> = depth_lists
        .into_iter()
        .filter_map(|(ltype, depth_list)| build_list_element(ltype, depth_list))
        .collect();
    let list_count = elements.len();
    elements.extend(escaped_elements);

    let mut result = ParseSuccess::new(elements.into(), errors, paragraph_safe);
    if let Some((entries, escape_safe)) = gate_escape {
        if list_count > 0 {
            let mut full = vec![(list_count, false, ParagraphSplitKind::Paragraph)];
            full.extend(entries);
            result.paragraph_split = Some(full);
        } else if entries.len() >= 2 {
            result.paragraph_split = Some(entries);
            result.paragraph_safe = escape_safe;
        } else {
            result.paragraph_safe = escape_safe;
        }
    }
    Ok(result)
}

fn wikidot_list_ends_with_scored_alias(
    parser: &Parser<'_, '_>,
    entry_start: usize,
    depths: &[(usize, ListType, Vec<Element<'_>>)],
) -> bool {
    if !parser.settings().layout.legacy() || parser.discarding_hidden_body() {
        return false;
    }
    let Some((_, _, elements)) = depths.last() else {
        return false;
    };
    if !ParagraphStack::wikidot_run_ends_with_scored_alias(elements) {
        return false;
    }
    // A trailing space after the close re-enables the list; live only
    // rejects when the close is followed by a bare line end.
    let source = parser.full_text().inner();
    let after = parser.current().span.start.min(source.len());
    let entry_start = entry_start.min(after);
    let tail = source[entry_start..after].trim_end_matches(['\r', '\n']);
    if tail.ends_with(' ') || tail.ends_with('\t') {
        return false;
    }
    // Live rejects only at end of input; a following block (such as a
    // div closer or the end of an enclosing quote) or line lets the list
    // form.
    if parser.wikidot_byte_in_quote_run(entry_start) {
        return false;
    }
    source[after..]
        .bytes()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

fn retain_active_nested_list_types<T>(
    items: impl IntoIterator<Item = (usize, ListType, T)>,
) -> Vec<(usize, ListType, T)> {
    let mut active_types = Vec::new();
    let mut normalized = Vec::new();

    for (depth, ltype, item) in items {
        let effective_type = if depth == 0 {
            active_types.truncate(1);
            if active_types.is_empty() {
                active_types.push(ltype);
            } else {
                active_types[0] = ltype;
            }
            ltype
        } else if depth >= active_types.len() {
            active_types.resize(depth + 1, ltype);
            ltype
        } else {
            active_types.truncate(depth + 1);
            active_types[depth]
        };
        normalized.push((depth, effective_type, item));
    }

    normalized
}

fn wikidot_escape_batch_breaks_block(element: &Element) -> bool {
    if matches!(
        element,
        Element::Partial(
            PartialElement::WikidotBlockOpen(_) | PartialElement::WikidotBlockClose(_)
        )
    ) {
        return false;
    }
    !element.paragraph_safe()
}

fn wikidot_escape_batches(escaped: &[Element]) -> Vec<ParagraphSplitEntry> {
    let mut entries: Vec<ParagraphSplitEntry> = Vec::new();
    let mut bare = false;
    let mut index = 0usize;
    let mut run_start = 0usize;
    let mut run_safe = true;
    while index < escaped.len() {
        let element = &escaped[index];
        if matches!(
            element,
            Element::Partial(PartialElement::WikidotBlockClose(_))
        ) {
            if index > run_start {
                entries.push((
                    index - run_start,
                    run_safe,
                    ParagraphSplitKind::Paragraph,
                ));
            }
            run_start = index;
            run_safe = false;
            bare = true;
            index += 1;
        } else if wikidot_escape_batch_breaks_block(element) {
            if index > run_start {
                entries.push((
                    index - run_start,
                    run_safe,
                    ParagraphSplitKind::Paragraph,
                ));
            }
            entries.push((1, false, ParagraphSplitKind::Paragraph));
            index += 1;
            run_start = index;
            run_safe = true;
            bare = false;
        } else {
            let safe = !bare;
            if safe != run_safe && index > run_start {
                entries.push((
                    index - run_start,
                    run_safe,
                    ParagraphSplitKind::Paragraph,
                ));
                run_start = index;
                run_safe = safe;
            }
            index += 1;
        }
    }
    if escaped.len() > run_start {
        entries.push((
            escaped.len() - run_start,
            run_safe,
            ParagraphSplitKind::Paragraph,
        ));
    }
    if entries.is_empty() && !escaped.is_empty() {
        entries.push((escaped.len(), true, ParagraphSplitKind::Paragraph));
    }
    entries
}

fn find_wikidot_flat_boundary(elements: &[Element]) -> Option<(usize, bool)> {
    elements.iter().enumerate().find_map(|(index, element)| {
        let Element::Partial(partial) = element else {
            return None;
        };
        match partial {
            PartialElement::WikidotBlockOpen(_) => Some((index, false)),
            PartialElement::WikidotBlockClose(_) => Some((index, true)),
            _ => None,
        }
    })
}

fn trim_wikidot_flat_close_space(elements: &mut Vec<Element>) {
    while let Some(Element::Text(text)) = elements.last_mut() {
        if text.chars().all(|c| c == ' ' || c == '\t') {
            elements.pop();
            continue;
        }
        let trimmed = text.trim_end_matches([' ', '\t']);
        if trimmed.len() == text.len() {
            break;
        }
        if trimmed.is_empty() {
            elements.pop();
        } else {
            *text = trimmed.to_owned().into();
        }
        break;
    }
}

fn parse_next_list_item<'r, 't>(
    parser: &mut Parser<'r, 't>,
    errors: &mut Vec<ParseError>,
    paragraph_safe: &mut bool,
    has_prior_items: bool,
) -> Result<ListItemStep<'t>, ParseError>
where
    'r: 't,
{
    if at_lost_owner_close(parser) {
        return Ok(ListItemStep::End);
    }

    let parser_state = parser.get_mutable_state();
    let mut sub_parser = parser.clone();

    let Some(depth) = parse_list_depth(&mut sub_parser)? else {
        return Ok(ListItemStep::End);
    };

    if depth > MAX_LIST_DEPTH {
        return Err(parser.make_err(ParseErrorKind::ListDepthExceeded));
    }

    let marker = sub_parser.current().slice;
    let Some(list_type) = get_list_type(sub_parser.current().token) else {
        return Ok(ListItemStep::End);
    };
    sub_parser.step()?;

    if sub_parser.current().token != Token::Whitespace {
        return Ok(ListItemStep::End);
    }
    sub_parser.step()?;
    let content_start = sub_parser.current().span.start;

    if parser.settings().layout.legacy() {
        if let Some(literal) =
            try_consume_literal_list_alignment(&mut sub_parser, depth, list_type)?
        {
            parser.update(&sub_parser);
            return Ok(ListItemStep::Item((depth, list_type, vec![literal]), false));
        }
        if let Some(lost) = try_consume_lost_owner_block(
            &mut sub_parser,
            LineOwner::List {
                depth,
                ltype: list_type,
            },
        )? {
            errors.extend(lost.errors);
            let control = lost
                .control
                .map(|control| (depth, list_type, vec![control]));
            parser.update(&sub_parser);
            return Ok(ListItemStep::Split {
                control,
                escaped: lost.body,
                escaped_split: None,
                escaped_safe: false,
            });
        }
    }

    let item_result = collect_list_item_elements(&mut sub_parser)?;
    let content_end = sub_parser.current().span.start;
    let (
        (mut elements, ends_run, ends_at_break, escape),
        mut item_errors,
        item_paragraph_safe,
    ) = item_result.into();
    if escape.is_none()
        && parser.settings().layout.legacy()
        && let Some((boundary, is_close)) = find_wikidot_flat_boundary(&elements)
    {
        errors.append(&mut item_errors);
        *paragraph_safe &= item_paragraph_safe;
        parser.update(&sub_parser);
        if is_close {
            let mut escaped = elements.split_off(boundary);
            trim_wikidot_flat_close_space(&mut elements);
            if ends_at_break
                && !ends_run
                && flat_close_left_inline_residue(&escaped)
                && !next_line_opens_list(parser)
            {
                escaped.push(Element::LineBreak);
            }
            let control = if elements.is_empty() {
                None
            } else {
                Some((depth, list_type, std::mem::take(&mut elements)))
            };
            return Ok(ListItemStep::Split {
                control,
                escaped_split: Some(vec![(
                    escaped.len(),
                    false,
                    ParagraphSplitKind::Paragraph,
                )]),
                escaped,
                escaped_safe: false,
            });
        }
        let escaped = elements.split_off(boundary + 1);
        let open = elements.pop();
        trim_wikidot_flat_close_space(&mut elements);
        if let Some(open) = open {
            elements.push(open);
        }
        if escaped.is_empty() {
            return Ok(ListItemStep::Item((depth, list_type, elements), true));
        }
        let control = Some((depth, list_type, elements));
        // L8.5: partition the escape into paragraph batches. Leading
        // text renders as paragraph(s); a closer flips the rest bare
        // until a swallowed block re-freshens what follows it.
        let entries = wikidot_escape_batches(&escaped);
        return Ok(ListItemStep::Split {
            control,
            escaped_split: Some(entries),
            escaped,
            escaped_safe: false,
        });
    }
    if let Some(escape) = escape {
        errors.append(&mut item_errors);
        *paragraph_safe &= item_paragraph_safe;
        parser.update(&sub_parser);
        let (flat, entries, escape_safe) = escape.into_layout();
        let control = if elements
            .iter()
            .all(|element| matches!(element, Element::Delayed(_)))
        {
            None
        } else {
            Some((depth, list_type, elements))
        };
        let mut offset = 0_usize;
        let mut skip = 0_usize;
        for (count, _, kind) in entries.iter() {
            if matches!(kind, ParagraphSplitKind::Item(_)) {
                break;
            }
            let slice = &flat[offset..offset + count];
            if slice
                .iter()
                .all(|element| matches!(element, Element::LineBreak))
            {
                skip += 1;
                offset += *count;
            } else {
                break;
            }
        }
        let item_count = entries
            .iter()
            .skip(skip)
            .take_while(|(_, _, kind)| matches!(kind, ParagraphSplitKind::Item(_)))
            .count();
        if item_count == 0 {
            return Ok(ListItemStep::Split {
                control,
                escaped: flat,
                escaped_split: Some(entries),
                escaped_safe: escape_safe,
            });
        }
        let mut rest = flat.into_iter().skip(offset);
        let mut items = Vec::new();
        for (count, _, kind) in entries.iter().skip(skip).take(item_count) {
            let &ParagraphSplitKind::Item(item_type) = kind else {
                unreachable!("leading entries are items");
            };
            let mut item_elements: Vec<Element<'t>> =
                rest.by_ref().take(*count).collect();
            while matches!(item_elements.last(), Some(Element::LineBreak)) {
                item_elements.pop();
            }
            items.push((depth, item_type, item_elements));
        }
        let rest_flat: Vec<Element<'t>> = rest.collect();
        let rest_entries: Vec<ParagraphSplitEntry> =
            entries.into_iter().skip(skip + item_count).collect();
        let trailing = if rest_entries.is_empty() {
            None
        } else {
            Some((rest_flat, rest_entries, escape_safe))
        };
        return Ok(ListItemStep::SplitItems {
            control,
            items,
            trailing,
        });
    }
    let (mut elements, mut ends_run, mut item_paragraph_safe) =
        (elements, ends_run, item_paragraph_safe);
    if parser.settings().layout.legacy()
        && !parser.discarding_hidden_body()
        && ends_at_break
    {
        // Live Wikidot eats the break after a paired scored close, so the
        // following line continues the item as inline text (a list marker on
        // that line stops being a marker). A line that opens with a paired
        // scored `[[span_]]`/`[[a_]]` opener joins the item the same way: the
        // opener eats the whitespace run in front of it from its first
        // newline on.
        let absorb_lead = !ParagraphStack::wikidot_run_ends_with_scored_alias(&elements)
            && crate::parsing::paragraph::wikidot_scored_alias_open_here(&sub_parser);
        if absorb_lead || ParagraphStack::wikidot_run_ends_with_scored_alias(&elements) {
            let (absorbed_run, absorbed_break) = absorb_wikidot_scored_alias_line(
                &mut sub_parser,
                &mut elements,
                &mut item_errors,
                &mut item_paragraph_safe,
                absorb_lead,
            )?;
            ends_run |= absorbed_run;
            let _ = absorbed_break;
        }
    }

    if parser.settings().layout.legacy()
        && unowned_footnote_first(&sub_parser, content_start, content_end, &elements)
    {
        if has_prior_items {
            parser.reset_mutable_state(parser_state);
            return Ok(ListItemStep::End);
        }
        let mut fallback = Vec::with_capacity(elements.len() + 1);
        fallback.push(text!(marker));
        fallback.extend(elements);
        errors.append(&mut item_errors);
        *paragraph_safe &= item_paragraph_safe;
        parser.update(&sub_parser);
        return Ok(ListItemStep::Fallback(fallback));
    }

    // The collapsible on this bullet owns no following lines. Ending the run
    // here keeps the next bullets as a sibling list.
    if parser.in_note()
        && sub_parser.pending_wikidot_collapsible_closer()
        && elements
            .iter()
            .any(|element| matches!(element, Element::Collapsible { .. }))
    {
        ends_run = true;
    }

    errors.append(&mut item_errors);
    *paragraph_safe &= item_paragraph_safe;
    parser.update(&sub_parser);
    Ok(ListItemStep::Item((depth, list_type, elements), ends_run))
}

fn next_line_opens_list(parser: &Parser) -> bool {
    let mut probe = parser.clone();
    if probe.current().token == Token::Whitespace {
        if probe.step().is_err() {
            return false;
        }
    }
    matches!(
        probe.current().token,
        Token::BulletItem | Token::NumberedItem
    )
}

fn flat_close_left_inline_residue(escaped: &[Element]) -> bool {
    escaped.iter().any(|element| match element {
        Element::Partial(PartialElement::WikidotBlockClose(_)) => false,
        Element::Text(text) => text.chars().any(|c| c != ' ' && c != '\t'),
        Element::LineBreak | Element::LineBreaks(_) => false,
        _ => true,
    })
}

/// Live Wikidot does not split a list at a comment-only line between items:
/// the comment line disappears and the following item continues the list. The
/// continuation only happens when the next marker is the same list kind; any
/// other next line leaves the comment to the surrounding parser.
fn skip_wikidot_list_comment_lines<'r, 't>(
    parser: &mut Parser<'r, 't>,
    depths: &[(usize, ListType, Vec<Element<'t>>)],
) -> Result<bool, ParseError>
where
    'r: 't,
{
    if !parser.settings().layout.legacy() {
        return Ok(false);
    }
    let Some((_, last_type, _)) = depths.last() else {
        return Ok(false);
    };
    let expected = match last_type {
        ListType::Bullet => Token::BulletItem,
        ListType::Numbered => Token::NumberedItem,
        ListType::Generic => return Ok(false),
    };

    let state = parser.get_mutable_state();
    let mut candidate = parser.clone();
    let mut skipped = false;
    loop {
        let mut probe = candidate.clone();
        if probe.current().token == Token::Whitespace {
            probe.step()?;
        }
        if probe.current().token != Token::LeftComment {
            break;
        }
        if consume_valid_comment(&mut probe).is_err()
            || probe.current().token != Token::LineBreak
        {
            parser.reset_mutable_state(state);
            return Ok(false);
        }
        probe.step()?;
        candidate = probe;
        skipped = true;
    }
    if !skipped {
        parser.reset_mutable_state(state);
        return Ok(false);
    }

    let mut probe = candidate.clone();
    if probe.current().token == Token::Whitespace {
        probe.step()?;
    }
    if probe.current().token != expected {
        parser.reset_mutable_state(state);
        return Ok(false);
    }
    parser.update(&candidate);
    Ok(true)
}

fn at_lost_owner_close<'r, 't>(parser: &Parser<'r, 't>) -> bool
where
    'r: 't,
{
    if !parser.quote_boundary_closes_body()
        || !matches!(
            parser.current().token,
            Token::BulletItem | Token::NumberedItem
        )
    {
        return false;
    }

    let mut close = parser.clone();
    if close.step().is_err() || close.current().token != Token::Whitespace {
        return false;
    }
    if close.step().is_err() {
        return false;
    }
    let Ok(name) = close.get_end_block() else {
        return false;
    };
    let owner_matches = (name.eq_ignore_ascii_case("div")
        && parser.in_wikidot_div_body())
        || (name.eq_ignore_ascii_case("collapsible") && parser.in_wikidot_collapsible());
    if !owner_matches {
        return false;
    }
    let _ = close.get_optional_space();
    matches!(
        close.current().token,
        Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
    )
}

fn parse_list_depth<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<usize>, ParseError> {
    let current = parser.current();
    let depth = match current.token {
        Token::Whitespace => {
            let depth = current.slice.len();
            parser.step()?;
            depth
        }
        Token::BulletItem | Token::NumberedItem => 0,
        _ => {
            return Ok(None);
        }
    };
    Ok(Some(depth))
}

pub(crate) fn list_item_marker_here(parser: &Parser) -> Option<ListType> {
    let list_type = get_list_type(parser.current().token)?;
    let mut step = parser.clone();
    step.step().ok()?;
    if step.current().token != Token::Whitespace || step.current().slice.len() != 1 {
        return None;
    }
    Some(list_type)
}

pub(crate) fn collect_gate_body_list_line<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<
    'r,
    't,
    (
        Vec<Element<'t>>,
        bool,
        Option<crate::parsing::collect::CollectSplitEscape<'t>>,
    ),
> {
    parser.step()?;
    parser.step()?;
    let close_conditions = [
        ParseCondition::current(Token::LineBreak),
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::current(Token::InputEnd),
        ParseCondition::current(Token::BulletItem),
        ParseCondition::current(Token::NumberedItem),
    ];
    let restore_line = if parser.settings().layout.legacy() {
        let previous = parser.in_wikidot_single_line_content();
        parser.set_in_wikidot_single_line_content(true);
        Some(previous)
    } else {
        None
    };
    let result =
        collect_consume_keep_split(parser, RULE_LIST, &close_conditions, &[], None);
    if let Some(previous) = restore_line {
        parser.set_in_wikidot_single_line_content(previous);
    }
    let result = result?;
    Ok(result.map(|(elements, last, escape)| {
        let ends_run = matches!(last.token, Token::ParagraphBreak | Token::InputEnd);
        (elements, ends_run, escape)
    }))
}

fn collect_list_item_elements<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<
    'r,
    't,
    (
        Vec<Element<'t>>,
        bool,
        bool,
        Option<crate::parsing::collect::CollectSplitEscape<'t>>,
    ),
> {
    // A list marker that does not begin its source line is literal text in
    // live Wikidot: `* item# item` is one item whose body reads `item# item`.
    // The preprocessor glues a comment-led continuation onto the previous
    // line, so those markers reach the collect without a break token while
    // sitting mid-line in the source.
    let close_conditions = [
        ParseCondition::current(Token::LineBreak),
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::current(Token::InputEnd),
        ParseCondition::source_line_start(Token::BulletItem),
        ParseCondition::source_line_start(Token::NumberedItem),
        ParseCondition::QuotePreparedListMarker,
    ];

    let restore_line = if parser.settings().layout.legacy() {
        let previous = parser.in_wikidot_single_line_content();
        parser.set_in_wikidot_single_line_content(true);
        Some(previous)
    } else {
        None
    };
    let result =
        collect_consume_keep_split(parser, RULE_LIST, &close_conditions, &[], None);
    if let Some(previous) = restore_line {
        parser.set_in_wikidot_single_line_content(previous);
    }
    let result = result?;
    Ok(result.map(|(elements, last, escape)| {
        let ends_run = matches!(last.token, Token::ParagraphBreak | Token::InputEnd);
        let ends_at_break =
            matches!(last.token, Token::LineBreak | Token::ParagraphBreak);
        (elements, ends_run, ends_at_break, escape)
    }))
}

/// Live Wikidot removes the line break that follows a paired scored inline
/// close, so the next source line joins the current list item as inline text.
/// The line-start rules do not fire on the joined line: a * or # there is
/// literal, not a new list item. `absorb_lead` additionally joins a line that
/// opens with a paired scored alias opener (its own rule eats the break in
/// front of it); afterwards the closer-side loop takes over.
fn absorb_wikidot_scored_alias_line<'r, 't>(
    parser: &mut Parser<'r, 't>,
    elements: &mut Vec<Element<'t>>,
    errors: &mut Vec<ParseError>,
    paragraph_safe: &mut bool,
    absorb_lead: bool,
) -> Result<(bool, bool), ParseError>
where
    'r: 't,
{
    let mut ends_run = false;
    let mut ends_at_break = false;
    let mut first = true;
    loop {
        let lead = absorb_lead && first;
        first = false;
        if !lead && !ParagraphStack::wikidot_run_ends_with_scored_alias(elements) {
            break;
        }
        if parser.current().token == Token::InputEnd {
            break;
        }
        parser.clear_start_of_line();
        while parser.current().token == Token::Whitespace {
            parser.step()?;
        }
        if parser.current().token == Token::InputEnd {
            break;
        }
        if lead && !crate::parsing::paragraph::wikidot_scored_alias_open_here(parser) {
            break;
        }
        let close_conditions = [
            ParseCondition::current(Token::LineBreak),
            ParseCondition::current(Token::ParagraphBreak),
            ParseCondition::current(Token::InputEnd),
        ];
        let restore_line = if parser.settings().layout.legacy() {
            let previous = parser.in_wikidot_single_line_content();
            parser.set_in_wikidot_single_line_content(true);
            Some(previous)
        } else {
            None
        };
        let result =
            collect_consume_keep_split(parser, RULE_LIST, &close_conditions, &[], None);
        if let Some(previous) = restore_line {
            parser.set_in_wikidot_single_line_content(previous);
        }
        let ParseSuccess {
            item: (mut extra, last, escape),
            errors: step_errors,
            paragraph_safe: step_safe,
            ..
        } = result?;
        if let Some(escape) = escape {
            let (flat, _, _) = escape.into_layout();
            extra.extend(flat);
        }
        errors.extend(step_errors);
        *paragraph_safe &= step_safe;
        if extra.is_empty() {
            break;
        }
        elements.extend(extra);
        ends_run = matches!(last.token, Token::ParagraphBreak | Token::InputEnd);
        ends_at_break = matches!(last.token, Token::LineBreak | Token::ParagraphBreak);
        if !ends_at_break {
            break;
        }
    }
    Ok((ends_run, ends_at_break))
}

pub(crate) fn build_list_element(
    top_ltype: ListType,
    list: DepthList<ListType, Vec<Element>>,
) -> Option<Element> {
    let mut items = Vec::new();
    for item in list {
        match item {
            DepthItem::Item(elements) => items.push(ListItem::Elements {
                elements,
                attributes: AttributeMap::new(),
            }),
            DepthItem::List(ltype, list) => {
                let Some(sublist) = build_list_element(ltype, list) else {
                    continue;
                };
                match items.last_mut() {
                    Some(ListItem::Elements { elements, .. }) => elements.push(sublist),
                    _ => items.push(ListItem::SubList {
                        element: Box::new(sublist),
                    }),
                }
            }
        }
    }

    items.retain(|item| match item {
        ListItem::Elements { elements, .. } => !elements.is_empty(),
        ListItem::SubList { .. } => true,
    });
    if items.is_empty() {
        return None;
    }

    let attributes = AttributeMap::new();

    Some(Element::List {
        ltype: top_ltype,
        items,
        attributes,
        wikidot_explicit_block: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};
    use std::sync::Once;

    #[derive(Debug)]
    struct TestLogger;

    impl log::Log for TestLogger {
        fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
            true
        }

        fn log(&self, record: &log::Record<'_>) {
            let _ = record.args().to_string();
        }

        fn flush(&self) {}
    }

    static TEST_LOGGER: TestLogger = TestLogger;
    static INIT_LOGGER: Once = Once::new();

    fn enable_test_logging() {
        INIT_LOGGER.call_once(|| {
            let _ = log::set_logger(&TEST_LOGGER);
            log::set_max_level(log::LevelFilter::Trace);
        });
    }

    fn settings() -> (PageInfo<'static>, WikitextSettings) {
        (
            PageInfo::dummy(),
            WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot),
        )
    }

    #[test]
    fn native_list_rejects_excessive_depth() {
        enable_test_logging();

        let (page_info, settings) = settings();
        let input = format!("{}* too deep", " ".repeat(MAX_LIST_DEPTH + 1));
        let tokenization = crate::tokenize(&input);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("leading whitespace should follow input start");
        parser.set_rule(RULE_LIST);

        let error = RULE_LIST
            .try_consume(&mut parser)
            .expect_err("excessive list depth should fail");
        assert_eq!(error.kind(), ParseErrorKind::ListDepthExceeded);
    }

    #[test]
    fn native_list_rejects_inputs_without_items() {
        enable_test_logging();

        let (page_info, settings) = settings();
        let tokenization = crate::tokenize("plain");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("identifier token should follow input start");
        parser.set_rule(RULE_LIST);

        let error = RULE_LIST
            .try_consume(&mut parser)
            .expect_err("plain text should not produce a list");
        assert_eq!(error.kind(), ParseErrorKind::RuleFailed);

        let tokenization = crate::tokenize("  plain");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("whitespace token should follow input start");
        parser.set_rule(RULE_LIST);

        let error = RULE_LIST
            .try_consume(&mut parser)
            .expect_err("indented text without a bullet should not produce a list");
        assert_eq!(error.kind(), ParseErrorKind::RuleFailed);
    }

    #[test]
    fn native_list_rejects_an_indented_first_item_without_consuming_it() {
        enable_test_logging();

        let (page_info, settings) = settings();
        let tokenization = crate::tokenize(" * orphan\n* root");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("leading whitespace should follow input start");
        parser.set_rule(RULE_LIST);

        let error = RULE_LIST
            .try_consume(&mut parser)
            .expect_err("an indented first item must remain literal");
        assert_eq!(error.kind(), ParseErrorKind::RuleFailed);
        assert_eq!(parser.current().token, Token::Whitespace);
        assert_eq!(parser.current().slice, " ");
    }

    #[test]
    fn native_list_type_is_stable_during_each_nested_depth_activation() {
        let normalized = retain_active_nested_list_types([
            (0, ListType::Bullet, 'a'),
            (1, ListType::Numbered, 'b'),
            (2, ListType::Bullet, 'c'),
            (1, ListType::Bullet, 'd'),
            (0, ListType::Bullet, 'e'),
            (1, ListType::Bullet, 'f'),
            (0, ListType::Numbered, 'g'),
        ]);

        assert_eq!(
            normalized,
            vec![
                (0, ListType::Bullet, 'a'),
                (1, ListType::Numbered, 'b'),
                (2, ListType::Bullet, 'c'),
                (1, ListType::Numbered, 'd'),
                (0, ListType::Bullet, 'e'),
                (1, ListType::Bullet, 'f'),
                (0, ListType::Numbered, 'g'),
            ],
        );
    }

    #[test]
    fn native_list_attaches_a_sublist_to_its_parent_item() {
        let list = vec![
            DepthItem::Item(vec![text!("parent")]),
            DepthItem::List(
                ListType::Numbered,
                vec![DepthItem::Item(vec![text!("child")])],
            ),
        ];

        let Some(Element::List { items, .. }) =
            build_list_element(ListType::Bullet, list)
        else {
            panic!("expected a list element");
        };
        let [ListItem::Elements { elements, .. }] = items.as_slice() else {
            panic!("expected one parent item, got {items:?}");
        };
        let [Element::Text { .. }, Element::List { ltype, items, .. }] =
            elements.as_slice()
        else {
            panic!("expected parent text followed by a nested list, got {elements:?}");
        };
        assert_eq!(*ltype, ListType::Numbered);
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn native_list_stops_on_malformed_and_skips_empty_items() {
        enable_test_logging();

        let (page_info, settings) = settings();
        let tokenization = crate::tokenize("*missing-space");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("bullet token should follow input start");
        parser.set_rule(RULE_LIST);

        let error = RULE_LIST
            .try_consume(&mut parser)
            .expect_err("missing post-bullet whitespace should not produce a list");
        assert_eq!(error.kind(), ParseErrorKind::RuleFailed);
        assert_eq!(parser.current().token, Token::BulletItem);
        assert_eq!(parser.current().slice, "*");

        let tokenization = crate::tokenize("* \n* item");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("bullet token should follow input start");
        parser.set_rule(RULE_LIST);

        let success = RULE_LIST
            .try_consume(&mut parser)
            .expect("list should skip the empty item and keep the non-empty item");
        let Elements::Multiple(elements) = success.item else {
            panic!("expected one list, got {:?}", success.item);
        };
        let [Element::List { ltype, items, .. }] = elements.as_slice() else {
            panic!("expected one list, got {elements:?}");
        };
        assert_eq!(*ltype, ListType::Bullet);
        assert_eq!(items.len(), 1);
        match &items[0] {
            ListItem::Elements { elements, .. } => assert_eq!(elements, &[text!("item")]),
            other => panic!("expected a list item, got {other:?}"),
        }

        let tokenization = crate::tokenize("* good\n*bad");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("bullet token should follow input start");
        parser.set_rule(RULE_LIST);

        let success = RULE_LIST
            .try_consume(&mut parser)
            .expect("valid first item should produce a list");
        let Elements::Multiple(elements) = success.item else {
            panic!("expected one list, got {:?}", success.item);
        };
        let [Element::List { items, .. }] = elements.as_slice() else {
            panic!("expected one list, got {elements:?}");
        };
        assert_eq!(items.len(), 1);
        assert_eq!(parser.current().token, Token::BulletItem);
        assert_eq!(parser.current().slice, "*");
    }

    #[test]
    fn native_list_retains_an_empty_item_that_owns_a_sublist() {
        enable_test_logging();

        let (page_info, settings) = settings();
        let tokenization = crate::tokenize("* \n * child\n  * grandchild\n* after");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("bullet token should follow input start");
        parser.set_rule(RULE_LIST);

        let success = RULE_LIST
            .try_consume(&mut parser)
            .expect("empty parent with descendants should parse as one list run");
        let Elements::Multiple(elements) = success.item else {
            panic!("expected one list, got {:?}", success.item);
        };
        let [Element::List { items, .. }] = elements.as_slice() else {
            panic!("expected one list, got {elements:?}");
        };
        let [
            ListItem::Elements {
                elements: parent_elements,
                ..
            },
            ListItem::Elements {
                elements: after_elements,
                ..
            },
        ] = items.as_slice()
        else {
            panic!("expected an empty parent and a root sibling, got {items:?}");
        };
        let [
            Element::List {
                items: child_items, ..
            },
        ] = parent_elements.as_slice()
        else {
            panic!(
                "expected the empty parent to own a nested list, got {parent_elements:?}"
            );
        };
        let [
            ListItem::Elements {
                elements: child_elements,
                ..
            },
        ] = child_items.as_slice()
        else {
            panic!("expected one child item, got {child_items:?}");
        };
        assert!(matches!(
            child_elements.as_slice(),
            [Element::Text { .. }, Element::List { .. }]
        ));
        assert_eq!(after_elements, &[text!("after")]);
    }

    #[test]
    fn native_list_paragraph_break_starts_a_sibling_run() {
        enable_test_logging();

        let (page_info, settings) = settings();
        let tokenization = crate::tokenize("* first\n\n* second");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("bullet token should follow input start");
        parser.set_rule(RULE_LIST);

        let success = RULE_LIST
            .try_consume(&mut parser)
            .expect("first list run should parse");
        let Elements::Multiple(elements) = success.item else {
            panic!("expected one list, got {:?}", success.item);
        };
        let [Element::List { items, .. }] = elements.as_slice() else {
            panic!("expected one list, got {elements:?}");
        };
        assert_eq!(items.len(), 1);
        assert_eq!(parser.current().token, Token::BulletItem);
        assert_eq!(parser.current().slice, "*");

        let tokenization = crate::tokenize("* first\n* second");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("bullet token should follow input start");
        parser.set_rule(RULE_LIST);
        let success = RULE_LIST
            .try_consume(&mut parser)
            .expect("contiguous list run should parse");
        let Elements::Multiple(elements) = success.item else {
            panic!("expected one list, got {:?}", success.item);
        };
        let [Element::List { items, .. }] = elements.as_slice() else {
            panic!("expected one list, got {elements:?}");
        };
        assert_eq!(items.len(), 2);
    }
}
