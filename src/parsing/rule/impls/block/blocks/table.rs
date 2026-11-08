/*
 * parsing/rule/impls/block/blocks/table.rs
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
use super::unwrap_wikidot_paragraph_after_explicit_list;
use crate::parsing::ParserWrap;
use crate::parsing::rule::impls::block::RULE_BLOCK;
use crate::settings::WikitextMode;
use crate::tree::{
    AcceptsPartial, AttributeMap, ContainerType, PartialElement, Table, TableCell,
    TableRow, TableType, WikidotBlockShell,
};
use std::borrow::Cow;
use std::num::NonZeroU32;

pub const BLOCK_TABLE: BlockRule = BlockRule {
    name: "block-table",
    accepts_names: &["table"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn: parse_table,
};

pub const BLOCK_TABLE_ROW: BlockRule = BlockRule {
    name: "block-table-row",
    accepts_names: &["row"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn: parse_row,
};

pub const BLOCK_TABLE_CELL_REGULAR: BlockRule = BlockRule {
    name: "block-table-cell-regular",
    accepts_names: &["cell"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn: parse_cell_regular,
};

pub const BLOCK_TABLE_CELL_HEADER: BlockRule = BlockRule {
    name: "block-table-cell-header",
    accepts_names: &["hcell"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn: parse_cell_header,
};

const BLOCK_TABLE_CELL_BODY: BlockRule = BlockRule {
    name: "block-table-cell-body",
    accepts_names: &["hcell", "cell"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn: parse_cell_header,
};

#[derive(Debug)]
struct ParsedBlock<'t> {
    elements: Vec<Element<'t>>,
    attributes: AttributeMap<'t>,
    errors: Vec<ParseError>,
    has_arguments: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdvancedTableElement {
    Table,
    Row,
    Cell,
}

impl AdvancedTableElement {
    fn accepts_attribute(self, key: &str, authored_value: &str) -> bool {
        if authored_value.is_empty() || key == "title" {
            return false;
        }

        match self {
            AdvancedTableElement::Cell => true,
            AdvancedTableElement::Table | AdvancedTableElement::Row => {
                !matches!(key, "colspan" | "rowspan")
            }
        }
    }
}

fn parse_block<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
    (block_rule, description): (&BlockRule, &str),
    attribute_owner: AdvancedTableElement,
) -> Result<ParsedBlock<'t>, ParseError>
where
    'r: 't,
    ParsedBlock<'t>: 't,
{
    debug!("Parsing {description} block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "{description} block doesn't allow star flag");
    assert!(!flag_score, "{description} block doesn't allow score flag");
    assert_block_name(block_rule, name);

    if parser.settings().layout.legacy()
        && in_head
        && std::iter::once(parser.current())
            .chain(parser.remaining())
            .take_while(|token| {
                !matches!(token.token, Token::RightBlock | Token::InputEnd)
            })
            .any(|token| {
                matches!(
                    token.token,
                    Token::GeneratedPageLink
                        | Token::GeneratedTagLinks
                        | Token::RuntimeText
                )
            })
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let (arguments, body_start) =
        parser.get_head_map_with_body_start_wikidot(block_rule, in_head)?;
    let has_arguments = arguments.has_source();
    let attributes = if parser.settings().layout.legacy() {
        arguments.to_attribute_map_without_bare_where(parser.settings(), |key, value| {
            attribute_owner.accepts_attribute(key, value)
        })
    } else {
        arguments.to_attribute_map(parser.settings())
    };

    let as_paragraphs = parser.settings().layout.legacy()
        && attribute_owner == AdvancedTableElement::Cell;
    let advanced_cell_body = parser.settings().layout.legacy()
        && attribute_owner == AdvancedTableElement::Cell;
    let body = {
        if advanced_cell_body {
            parser.set_in_wikidot_advanced_table_cell_body(true);
            parser.arm_wikidot_advanced_cell_paragraph();
        }
        let body = parser.get_body_elements_with_context(
            block_rule,
            as_paragraphs,
            body_start,
            None,
        );
        if advanced_cell_body {
            parser.set_in_wikidot_advanced_table_cell_body(false);
            parser.clear_wikidot_advanced_cell_paragraph();
        }
        body
    }?;
    let (elements, errors, _) = body.into();

    let parsed = ParsedBlock {
        elements,
        attributes,
        errors,
        has_arguments,
    };
    Ok(parsed)
}

fn straddle_tail_text(element: &Element<'_>) -> Option<String> {
    match element {
        Element::Text(piece) => Some(piece.to_string()),
        Element::LineBreak => Some("\n".to_string()),
        Element::Container(container) => {
            let mut text = String::new();
            for child in container.elements() {
                text.push_str(&straddle_tail_text(child)?);
            }
            Some(text)
        }
        _ => None,
    }
}

/// A rescue that fails returns the original tail so the caller can
/// decide whether the un-rescued text still belongs in the splice tail
/// (a spliced row fosters its residue) or is genuinely out of place.
fn rescue_straddle_tail<'t>(
    tail: Vec<Element<'t>>,
) -> Result<Vec<Element<'t>>, Vec<Element<'t>>> {
    let mut text = String::new();
    for element in &tail {
        let Some(piece) = straddle_tail_text(element) else {
            return Err(tail);
        };
        text.push_str(&piece);
    }
    let lower = text.to_ascii_lowercase();
    let mut best: Option<(usize, usize)> = None;
    for name in ["div", "collapsible", "note", "=", "==", "<", ">"] {
        let needle = format!("[[/{name}]]");
        if let Some(at) = lower.rfind(&needle)
            && best.is_none_or(|(best_at, _)| at > best_at)
        {
            best = Some((at, needle.len()));
        }
    }
    let Some((at, len)) = best else {
        return Err(tail);
    };
    let start = if at > 0 && text.as_bytes()[at - 1] == b'\n' {
        at - 1
    } else {
        at
    };
    let mut rest = text[..start].to_string();
    rest.push_str(&text[at + len..]);
    let mut elements = Vec::new();
    for (index, segment) in rest.split('\n').enumerate() {
        if index > 0 {
            elements.push(Element::LineBreak);
        }
        if !segment.is_empty() {
            elements.push(Element::Text(segment.to_string().into()));
        }
    }
    Ok(elements)
}

#[allow(clippy::type_complexity)]
fn extract_table_rows<'r, 't>(
    parser: &Parser<'r, 't>,
    elements: Vec<Element<'t>>,
) -> Result<(Vec<TableRow<'t>>, Vec<Cow<'t, str>>, Vec<Element<'t>>), ParseError> {
    let mut rows = Vec::new();
    let mut residuals = Vec::new();
    let mut foster = Vec::new();
    let mut straddle_tail: Vec<Element<'t>> = Vec::new();

    for element in elements {
        match element {
            Element::Partial(PartialElement::TableRow(row)) => {
                if !straddle_tail.is_empty() {
                    return Err(parser.make_err(ParseErrorKind::TableContainsNonRow));
                }
                rows.push(row);
            }

            Element::Partial(PartialElement::WikidotFoster(_)) => {
                foster.push(element);
            }

            Element::Partial(
                PartialElement::WikidotBlockOpen(_)
                | PartialElement::WikidotBlockClose(_),
            ) => {
                foster.append(&mut straddle_tail);
                foster.push(element);
            }

            Element::Text(text) if is_cell_closer_residual(&text) => {
                residuals.push(text);
            }

            Element::LineBreak if !foster.is_empty() => {
                foster.push(element);
            }

            Element::Text(ref text) if !text.trim().is_empty() => {
                straddle_tail.push(element);
            }

            Element::Container(ref container)
                if container.ctype() == crate::tree::ContainerType::Paragraph =>
            {
                straddle_tail.push(element);
            }

            Element::Delayed(delayed) if delayed.is_suppression_seam() => {}

            element if element.is_whitespace() => {}

            element => {
                return Err(parser.make_err(ParseErrorKind::TableContainsNonRow));
            }
        }
    }
    if !straddle_tail.is_empty() {
        match rescue_straddle_tail(straddle_tail) {
            Ok(fostered) => foster.extend(fostered),
            Err(_) => return Err(parser.make_err(ParseErrorKind::TableContainsNonRow)),
        }
    }

    Ok((rows, residuals, foster))
}

/// Wrap a merged cross-li inner list in the ejected outer list of a
/// separate-list yield. The merged list replaces the outer's innermost
/// trailing list (the cut); without an outer the inner fosters alone.
fn cross_li_wrap_outer<'t>(
    outer: Option<Element<'t>>,
    inner: Element<'t>,
) -> Vec<Element<'t>> {
    let Some(Element::List {
        ltype,
        mut items,
        attributes,
        wikidot_explicit_block,
        wikidot_unclosed,
    }) = outer
    else {
        return vec![inner];
    };
    // Seed the outer's innermost trailing list position: the last item's
    // last list element, or append the inner when it has none.
    let mut slot: Option<(usize, Option<usize>)> = None;
    for (item_index, item) in items.iter().enumerate() {
        let crate::tree::ListItem::Elements { elements, .. } = item else {
            continue;
        };
        let position = elements
            .iter()
            .rposition(|element| matches!(element, Element::List { .. }));
        slot = Some((item_index, position));
    }
    let Some((item_index, position)) = slot else {
        return vec![inner];
    };
    let crate::tree::ListItem::Elements { elements, .. } = &mut items[item_index] else {
        return vec![inner];
    };
    if let Some(position) = position {
        elements[position] = inner;
    } else {
        elements.push(inner);
    }
    vec![Element::List {
        ltype,
        items,
        attributes,
        wikidot_explicit_block,
        wikidot_unclosed,
    }]
}

/// Pop one trailing break from the spliced li's last content item
/// path: the break terminating the spliced li's own last line belongs
/// to the splice (live eats it in every li-yield-tail splice shape).
fn trim_spliced_li_close_break(elements: &mut Vec<Element<'_>>) {
    match elements.last_mut() {
        Some(Element::LineBreak) => {
            elements.pop();
        }
        Some(Element::List { items, .. }) => trim_spliced_li_close_break_items(items),
        _ => {}
    }
}

fn trim_spliced_li_close_break_items(items: &mut [crate::tree::ListItem<'_>]) {
    match items.last_mut() {
        Some(crate::tree::ListItem::Elements { elements, .. }) => {
            trim_spliced_li_close_break(elements)
        }
        Some(crate::tree::ListItem::SubList { element }) => {
            if let Element::List { items, .. } = &mut **element {
                trim_spliced_li_close_break_items(items);
            }
        }
        _ => {}
    }
}

/// One spliced list item's elements in source order (an item's own
/// elements inline; a sublist stays one whole element).
fn flatten_list_item<'t>(item: crate::tree::ListItem<'t>) -> Vec<Element<'t>> {
    match item {
        crate::tree::ListItem::Elements { elements, .. } => elements,
        crate::tree::ListItem::SubList { element } => vec![*element],
    }
}

/// Whether the list carries splice-residue close tokens (`[[row]]` etc.),
/// i.e. whether it is a spliced-items list whose contents were cut (the
/// flatten-and-foster shapes, e.g. bold-control's `* * [[/row]]` items)
/// rather than a fresh tail list block (a list line issued after the
/// splice point, e.g. splice-bare-close-list's `* **` line, which live
/// keeps as a real `<ul>`).
fn list_carries_splice_tokens(element: &Element<'_>) -> bool {
    match element {
        Element::List { items, .. } => items.iter().any(|item| match item {
            crate::tree::ListItem::Elements { elements, .. } => elements
                .iter()
                .any(|e| matches!(e, Element::Text(text) if text.contains("[["))),
            crate::tree::ListItem::SubList { element } => {
                list_carries_splice_tokens(element)
            }
        }),
        _ => false,
    }
}

#[allow(clippy::type_complexity)]
fn extract_table_cells<'r, 't>(
    parser: &Parser<'r, 't>,
    elements: Vec<Element<'t>>,
    mut straddle_list_pending: bool,
    mut li_tail_transfer: Option<Element<'t>>,
    li_yield_tail: bool,
    cross_li_eject: Vec<Element<'t>>,
    mut cross_li_outer: Option<Element<'t>>,
) -> Result<(Vec<TableCell<'t>>, Vec<Cow<'t, str>>, Vec<Element<'t>>), ParseError> {
    let mut cells = Vec::new();
    let mut residuals = Vec::new();
    let mut foster = Vec::new();
    let mut straddle_tail: Vec<Element<'t>> = Vec::new();
    let mut pending_break = false;
    // A cut or an explicit-li yield in this row means the row's
    // closing tokens were spliced: whatever bare text follows belongs
    // to the splice's residue, not to genuine row content.
    let splice_active = li_yield_tail || straddle_list_pending;
    // A `[[div]]` that opens on a spliced list line is a real block on
    // live: the head's list markers are the splice's residue, not
    // nested-item context. It still parses flat (the source line has
    // `*` markers), but the frame builder pairs opens and closes
    // only within one region. Gather the open, the remaining spliced
    // lines, and the matching close into one foster region so the
    // pair forms a `<div>` around the list (live divafter).
    let mut open_div: Option<(Vec<Element<'t>>, usize)> = None;
    // Post-yield row-tail items (li-Foster) swallow the close lines' line
    // breaks: each item-close and kept list closer line's newline belongs
    // to its structural closer, not to the foster.
    let mut absorb_item_close_breaks = false;
    let mut yield_foster_len = 0usize;
    for eject in cross_li_eject {
        foster.push(Element::Partial(PartialElement::WikidotFoster(vec![eject])));
    }
    if !foster.is_empty() {
        yield_foster_len = foster.len();
    }

    for element in elements {
        match element {
            Element::Partial(PartialElement::TableCell(cell)) if residuals.is_empty() => {
                cells.push(cell);
                // A fresh cell starts fresh foster territory with it.
                absorb_item_close_breaks = false;
            }

            // A bullet `[[/row]]` stays a list item. It is inside the row
            // span and tidy pulls that list out in front of the table.
            Element::List { .. } if !cells.is_empty() => {
                if let Some((region, _)) = &mut open_div {
                    // Inside a spliced tail's div the following lines
                    // stay one nested list: live opens the div and
                    // parses its body as block content.
                    region.push(element);
                } else if let Some(transfer) = li_tail_transfer.take() {
                    match (transfer, element) {
                        (
                            Element::List {
                                ltype: head_type,
                                items: mut head_items,
                                ..
                            },
                            Element::List {
                                ltype,
                                items,
                                attributes,
                                wikidot_explicit_block,
                                wikidot_unclosed,
                            },
                        ) if head_type == ltype => {
                            head_items.extend(items);
                            let merged = Element::List {
                                ltype,
                                attributes,
                                items: head_items,
                                wikidot_explicit_block,
                                wikidot_unclosed,
                            };
                            let fostered =
                                cross_li_wrap_outer(cross_li_outer.take(), merged);
                            foster.push(Element::Partial(PartialElement::WikidotFoster(
                                fostered,
                            )));
                        }
                        (transfer, element) => {
                            let wrapped =
                                cross_li_wrap_outer(cross_li_outer.take(), transfer);
                            foster.push(Element::Partial(PartialElement::WikidotFoster(
                                wrapped,
                            )));
                            foster.push(Element::Partial(PartialElement::WikidotFoster(
                                vec![element],
                            )));
                        }
                    }
                    yield_foster_len = foster.len();
                } else if straddle_list_pending || li_yield_tail {
                    straddle_list_pending = false;
                    if !list_carries_splice_tokens(&element) {
                        // A fresh tail list block (a list line issued
                        // after the splice point, no cut tokens inside,
                        // e.g. splice-bare-close-list's `* **` line):
                        // live keeps it as a real `<ul>` in the foster
                        // and never pairs the marker inside it.
                        foster.push(Element::Partial(PartialElement::WikidotFoster(
                            vec![element],
                        )));
                        if li_yield_tail {
                            yield_foster_len = foster.len();
                        }
                    } else {
                        let Element::List { items, .. } = element else {
                            unreachable!()
                        };
                        let mut items = items;
                        let mut bare = Vec::new();
                        let div_head = items.iter().position(|item| {
                            matches!(
                                item,
                                crate::tree::ListItem::Elements { elements, .. }
                                    if elements.iter().any(|e| matches!(
                                        e,
                                        Element::Partial(
                                            PartialElement::WikidotBlockOpen(_)
                                        )
                                    ))
                            )
                        });
                        if let Some(div_at) = div_head {
                            // Split the spliced list at the div head: the
                            // prefix and the head line's own text before
                            // the marker flatten as before; from the
                            // marker on, the line items gather into one
                            // region so the frame builder can pair the
                            // div around the list.
                            let mut rest = items.split_off(div_at);
                            let head_item = rest.remove(0);
                            let crate::tree::ListItem::Elements { elements, .. } =
                                head_item
                            else {
                                unreachable!()
                            };
                            let mut elements = elements;
                            let open_at = elements
                                .iter()
                                .position(|e| {
                                    matches!(
                                        e,
                                        Element::Partial(
                                            PartialElement::WikidotBlockOpen(_)
                                        )
                                    )
                                })
                                .expect("div head position is present");
                            bare.extend(
                                items
                                    .into_iter()
                                    .flat_map(|item| flatten_list_item(item).into_iter()),
                            );
                            bare.extend(elements.drain(..open_at));
                            let mut region = vec![elements.remove(0)];
                            region.extend(elements);
                            for item in rest {
                                region.extend(flatten_list_item(item));
                            }
                            foster.push(Element::Partial(PartialElement::WikidotFoster(
                                bare,
                            )));
                            open_div = Some((region, 1));
                        } else {
                            for item in items {
                                bare.extend(flatten_list_item(item));
                            }
                            foster.push(Element::Partial(PartialElement::WikidotFoster(
                                bare,
                            )));
                        }
                        if li_yield_tail {
                            yield_foster_len = foster.len();
                        }
                    }
                } else {
                    foster.push(Element::Partial(PartialElement::WikidotFoster(vec![
                        element,
                    ])));
                }
            }

            Element::Partial(PartialElement::WikidotFoster(_)) => {
                foster.push(element);
                if li_yield_tail {
                    absorb_item_close_breaks = true;
                    pending_break = false;
                }
            }

            Element::Partial(
                PartialElement::WikidotBlockOpen(_)
                | PartialElement::WikidotBlockClose(_),
            ) => {
                if !straddle_tail.is_empty() {
                    let tail = std::mem::take(&mut straddle_tail);
                    foster.push(Element::Partial(PartialElement::WikidotFoster(tail)));
                }
                if matches!(
                    element,
                    Element::Partial(PartialElement::WikidotBlockOpen(_))
                ) {
                    if let Some((region, depth)) = &mut open_div {
                        // A nested head while one region is open.
                        *depth += 1;
                        region.push(element);
                    } else {
                        foster.push(Element::Partial(PartialElement::WikidotFoster(
                            vec![element],
                        )));
                    }
                } else if let Some((region, depth)) = &mut open_div {
                    // The pair's close ends the region: flush it as a
                    // single foster region (open + list + close).
                    *depth -= 1;
                    region.push(element);
                    if *depth == 0 {
                        let (finished, _) = open_div.take().expect("open div region");
                        foster.push(Element::Partial(PartialElement::WikidotFoster(
                            finished,
                        )));
                        // Live puts one line break after the div and
                        // before the `[[cell]]` residual (`</div><br/>`).
                        foster.push(Element::Partial(PartialElement::WikidotFoster(
                            vec![Element::LineBreak],
                        )));
                        if li_yield_tail {
                            absorb_item_close_breaks = true;
                            pending_break = false;
                            yield_foster_len = foster.len();
                        }
                    }
                } else {
                    foster.push(Element::Partial(PartialElement::WikidotFoster(vec![
                        element,
                    ])));
                }
            }

            Element::Text(text) if !cells.is_empty() && is_cell_closer_residual(&text) => {
                if pending_break && !foster.is_empty() && foster.len() != yield_foster_len
                {
                    foster.push(Element::LineBreak);
                }
                pending_break = false;
                let closer_exact = is_cell_closer(text.trim_end_matches([' ', '\t', '\n', '\r']));
                let same_line_prefix = !straddle_tail.is_empty()
                    && !straddle_tail
                        .iter()
                        .any(|el| matches!(el, Element::LineBreak));
                if closer_exact && same_line_prefix {
                    // Split "text before closer" residual (item 87): the prefix
                    // accumulated in straddle_tail, the closer arrived as a
                    // separate element. Merge into one residual line.
                    let mut prefix = String::new();
                    for el in straddle_tail.drain(..) {
                        if let Some(t) = straddle_tail_text(&el) {
                            prefix.push_str(&t);
                        }
                    }
                    residuals.push(std::borrow::Cow::Owned(format!("{}{}", prefix, text)));
                } else {
                    residuals.push(text);
                }
            }

            Element::Text(ref text) if !cells.is_empty() && !text.trim().is_empty() => {
                straddle_tail.push(element);
            }

            Element::LineBreak if !straddle_tail.is_empty() => {
                straddle_tail.push(element);
            }

            // A post-yield item may close a kept list without a parse-time
            // absorb: the close line's break is structural for every break
            // in the run, so the run is absorbed wholesale and its pending
            // never registers.
            Element::LineBreak if !cells.is_empty() && !foster.is_empty() => {
                if !absorb_item_close_breaks {
                    pending_break = true;
                }
            }

            Element::Container(ref container)
                if !cells.is_empty()
                    && container.ctype() == crate::tree::ContainerType::Paragraph =>
            {
                straddle_tail.push(element);
            }

            Element::Delayed(delayed) if delayed.is_suppression_seam() => {}

            element if element.is_whitespace() => {
                if !cells.is_empty()
                    && !straddle_tail.is_empty()
                    && matches!(element, Element::Text(_))
                {
                    straddle_tail.push(element);
                }
            }

            element => {
                return Err(parser.make_err(ParseErrorKind::TableRowContainsNonCell));
            }
        }
    }
    if let Some(transfer) = li_tail_transfer.take() {
        let wrapped = cross_li_wrap_outer(cross_li_outer.take(), transfer);
        foster.push(Element::Partial(PartialElement::WikidotFoster(wrapped)));
    }
    if let Some((unclosed, _)) = open_div.take() {
        // An unmatched spliced-tail head (its close was spent
        // elsewhere) still flushes as one region.
        foster.push(Element::Partial(PartialElement::WikidotFoster(unclosed)));
    }
    if !straddle_tail.is_empty() {
        match rescue_straddle_tail(straddle_tail) {
            Ok(fostered) => {
                if !fostered.is_empty() {
                    foster
                        .push(Element::Partial(PartialElement::WikidotFoster(fostered)));
                }
            }
            Err(tail) if splice_active => {
                // A splice is active in this row, so the tail is the
                // splice's residue: live fosters it as-is (a bare
                // closer like `**` is the fostered tail's inline
                // closer, so it must survive to the marker pass)
                // instead of rejecting the whole row. Its item-close
                // line breaks are structural (each closers line's
                // newline belongs to its structural closer): live
                // keeps only one line break after the tail's last
                // content line (`**` line, `y` line, one `<br/>` sum).
                let mut collapsed: Vec<Element<'t>> = Vec::with_capacity(tail.len());
                for element in tail {
                    if matches!(element, Element::LineBreak)
                        && matches!(collapsed.last(), Some(Element::LineBreak))
                    {
                        continue;
                    }
                    collapsed.push(element);
                }
                foster.push(Element::Partial(PartialElement::WikidotFoster(collapsed)));
            }
            Err(_) => {
                return Err(parser.make_err(ParseErrorKind::TableRowContainsNonCell));
            }
        }
    }

    if li_yield_tail {
        // The spliced li's last content line's own terminator belongs
        // to the splice, not to the value inside the cell: live eats
        // it (splicenolist `<li><strong>x</strong></li>`, splice-bare-
        // close-list `<li>**x<br>` → `<li>**x</li>`, splice-bare-tail-y
        // `<li>x</li>`).
        if let Some(cell) = cells.last_mut() {
            trim_spliced_li_close_break(&mut cell.elements);
        }
    }

    Ok((cells, residuals, foster))
}

fn split_wikidot_foster<'t>(
    parser: &Parser<'_, '_>,
    elements: Vec<Element<'t>>,
) -> (Vec<Element<'t>>, Vec<Element<'t>>, bool) {
    if !parser.settings().layout.legacy() {
        return (elements, Vec::new(), false);
    }
    let split = elements.iter().position(|element| {
        matches!(
            element,
            Element::Partial(PartialElement::WikidotBlockClose(kind))
                if parser.wikidot_close_is_foster(kind.head_byte())
        )
    });
    let (mut cell, mut foster, mut had) = match split {
        Some(index) => {
            let mut cell = elements;
            let mut foster = cell.split_off(index);
            foster.remove(0);
            let pre = partition_wikidot_foster(foster);
            (cell, pre, true)
        }
        None => (elements, Vec::new(), false),
    };
    if lift_ready_foster(&mut cell, &mut foster) {
        had = true;
    }
    (cell, foster, had)
}

fn lift_ready_foster<'t>(
    cell: &mut Vec<Element<'t>>,
    foster: &mut Vec<Element<'t>>,
) -> bool {
    let mut lifted = false;
    let mut index = 0;
    while index < cell.len() {
        let is_foster = matches!(
            cell[index],
            Element::Partial(PartialElement::WikidotFoster(_))
        );
        if is_foster && !followed_by_table(cell, index) {
            foster.push(cell.remove(index));
            lifted = true;
        } else {
            index += 1;
        }
    }
    lifted
}

fn followed_by_table(cell: &[Element<'_>], index: usize) -> bool {
    cell[index + 1..]
        .iter()
        .find(|element| !element.is_whitespace())
        .is_some_and(|element| matches!(element, Element::Table(_)))
}

fn partition_wikidot_foster<'t>(tail: Vec<Element<'t>>) -> Vec<Element<'t>> {
    let mut pre = Vec::new();
    let mut run = Vec::new();
    for element in tail {
        let hoisted = matches!(
            element,
            Element::Partial(ref partial) if partial.is_wikidot_block_control()
        );
        if hoisted {
            push_foster_run(&mut run, &mut pre);
            pre.push(element);
        } else {
            run.push(element);
        }
    }
    push_foster_run(&mut run, &mut pre);
    pre
}

fn push_foster_run<'t>(run: &mut Vec<Element<'t>>, pre: &mut Vec<Element<'t>>) {
    if run.iter().all(|element| element.is_whitespace()) {
        run.clear();
    } else if !run.is_empty() {
        pre.push(Element::Partial(PartialElement::WikidotFoster(
            std::mem::take(run),
        )));
    }
}

fn is_cell_closer(source: &str) -> bool {
    source.eq_ignore_ascii_case("[[/cell]]") || source.eq_ignore_ascii_case("[[/hcell]]")
}

/// True when `source` is a stray cell/hcell closer residual: either the exact
/// closer token, or text before the closer on the same line with no text after
/// it (item 87). Live keeps the whole residual line and a following newline
/// before the table; text after the closer makes the construction invalid.
fn is_cell_closer_residual(source: &str) -> bool {
    if is_cell_closer(source) {
        return true;
    }
    let content = source.trim_end_matches([' ', '\t', '\n', '\r']);
    let Some((pos, len)) = last_cell_closer(content) else {
        return false;
    };
    pos + len == content.len()
}

fn last_cell_closer(haystack: &str) -> Option<(usize, usize)> {
    let lower = haystack.to_ascii_lowercase();
    let mut best: Option<(usize, usize)> = None;
    for needle in ["[[/cell]]", "[[/hcell]]"] {
        if let Some(pos) = lower.rfind(needle) {
            if best.is_none_or(|(b, _)| pos > b) {
                best = Some((pos, needle.len()));
            }
        }
    }
    best
}

fn next_cell_closer(haystack: &str) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for needle in ["[[/cell]]", "[[/hcell]]"] {
        if let Some(index) = find_ignore_ascii_case(haystack, needle) {
            if best.is_none_or(|(at, _)| index < at) {
                best = Some((index, needle.len()));
            }
        }
    }
    best
}

/// The table's own closer. A `[[/table]]` on a list line is item text, so it
/// does not end the region the cell closer is allowed to reach. Likewise, a
/// `[[/table]]` immediately followed by `||` — the `||` that closes a
/// simple-table row — is cell content for the enclosing advanced table, not
/// that table's end.
fn legacy_table_region_end(parser: &Parser<'_, '_>, from: usize) -> usize {
    let source = parser.full_text().inner();
    let mut search = from;
    while search < source.len() {
        let Some(rel) = find_ignore_ascii_case(&source[search..], "[[/table]]") else {
            return source.len();
        };
        let at = search + rel;
        let after = at + "[[/table]]".len();
        let content_of_cell = source[after..].starts_with("||");
        if !parser.wikidot_byte_on_list_item_line(at) && !content_of_cell {
            return at;
        }
        search = after;
    }
    source.len()
}

/// List runs before Tableadv and wraps a bullet line in list-end tokens.
/// Those tokens sit between a bullet `[[/cell]]` and a `[[/row]]` on a later
/// line, so that closer does not reach the row. A later line-start closer
/// still can. With no reaching closer, the row pattern fails and the markers
/// stay literal.
fn legacy_some_cell_closer_reaches_row(parser: &Parser<'_, '_>, from: usize) -> bool {
    let source = parser.full_text().inner();
    let region_end = legacy_table_region_end(parser, from);
    let mut search = from;
    while search < region_end {
        let Some((rel, len)) = next_cell_closer(&source[search..region_end]) else {
            return false;
        };
        let closer = search + rel;
        let closer_end = closer + len;
        let Some(row_rel) =
            find_ignore_ascii_case(&source[closer_end..region_end], "[[/row]]")
        else {
            return false;
        };
        let row = closer_end + row_rel;
        let gap = &source[closer_end..row];
        if gap_is_space_or_quote_prefix(gap) {
            let on_list = parser.wikidot_byte_on_list_item_line(closer);
            let same_line = !gap.contains('\n') && !gap.contains('\r');
            if !on_list || same_line {
                return true;
            }
        }
        search = closer_end;
    }
    false
}

/// Quote prefixes are eaten before Tableadv runs. They are not list-end tokens.
fn gap_is_space_or_quote_prefix(gap: &str) -> bool {
    gap.split(['\n', '\r']).all(|line| {
        let mut rest = line;
        while let Some(tail) = rest.strip_prefix('>') {
            rest = tail;
        }
        rest.chars().all(char::is_whitespace)
    })
}

fn explicit_closer_name<'r: 't, 't>(
    probe: &Parser<'r, 't>,
    end: usize,
) -> Option<String> {
    let mut probe = (*probe).clone();
    loop {
        if probe.current().token == Token::LeftBlockEnd {
            let mut close = probe.clone();
            if let Ok(name) = close.get_end_block() {
                let close_end = close.current().span.start;
                let gap_is_blank = close_end <= end
                    && probe.full_text().inner()[close_end..end]
                        .bytes()
                        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'));
                if gap_is_blank {
                    return Some(name.into_owned());
                }
            }
        }
        if probe.current().token == Token::InputEnd || probe.step().is_err() {
            return None;
        }
    }
}

fn find_ignore_ascii_case(haystack: &str, needle: &str) -> Option<usize> {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        return Some(0);
    }
    h.windows(n.len()).position(|w| {
        w.iter()
            .zip(n.iter())
            .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
    })
}

fn has_explicit_closer<'r: 't, 't>(
    probe: &Parser<'r, 't>,
    end: usize,
    accepted: &[&str],
) -> bool {
    if let Some(name) = explicit_closer_name(probe, end) {
        return accepted
            .iter()
            .any(|accepted| name.eq_ignore_ascii_case(accepted));
    }
    let full = probe.full_text().inner();
    let source = &full[..end];
    if accepted.iter().any(|name| {
        let close = format!("[[/{name}]]]");
        source
            .get(source.len().saturating_sub(close.len())..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(&close))
    }) {
        return true;
    }
    if probe.settings().layout.legacy() && !probe.discarding_hidden_body() {
        const BIB: &str = "[[/bibliography]]";
        if end + BIB.len() <= full.len()
            && full[end..end + BIB.len()].eq_ignore_ascii_case(BIB)
            && let Some(tail) = full.get(end + BIB.len()..)
        {
            return accepted.iter().any(|name| {
                let needle = format!("[[/{name}]]");
                find_ignore_ascii_case(tail, &needle).is_some()
            });
        }
    }
    false
}

fn table_opener_start(parser: &Parser<'_, '_>, opener_start: usize) -> Option<usize> {
    if parser.discarding_hidden_body() {
        None
    } else {
        Some(opener_start)
    }
}

fn cached_underclosed_table_error(
    parser: &Parser<'_, '_>,
    rule: &'static str,
    owner_start: Option<usize>,
) -> Option<ParseError> {
    let owner_start = owner_start?;
    if let Some(error) = parser.deterministic_block_failure(rule, owner_start) {
        return Some(error);
    }
    if !parser.settings().layout.legacy() && parser.settings().mode != WikitextMode::Page
    {
        return None;
    }
    if !parser.underclosed_block_failure_cached(rule, owner_start) {
        return None;
    }
    Some(if parser.settings().layout.legacy() {
        parser.make_err(ParseErrorKind::RuleFailed)
    } else {
        parser.make_end_of_input_err()
    })
}

fn cache_table_end_of_input_error(
    parser: &Parser<'_, '_>,
    rule: &'static str,
    owner_start: Option<usize>,
    error: &ParseError,
) {
    if error.kind() == ParseErrorKind::EndOfInput
        && let Some(owner_start) = owner_start
    {
        parser.cache_deterministic_block_failure(rule, owner_start, error);
    }
}

fn legacy_opener_start(parser: &Parser<'_, '_>, opener_start: usize) -> Option<usize> {
    if !parser.settings().layout.legacy() || parser.discarding_hidden_body() {
        return None;
    }

    Some(opener_start)
}

fn legacy_block_name_is_spaced(parser: &Parser<'_, '_>, opener_start: usize) -> bool {
    let Some(opener_start) = legacy_opener_start(parser, opener_start) else {
        return false;
    };
    parser.full_text().inner()[opener_start + 2..]
        .bytes()
        .next()
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
}

fn legacy_table_follows_heading_marker(
    parser: &Parser<'_, '_>,
    opener_start: usize,
) -> bool {
    let Some(opener_start) = legacy_opener_start(parser, opener_start) else {
        return false;
    };
    let source = parser.full_text().inner();
    let line_start = source[..opener_start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let mut prefix = &source[line_start..opener_start];

    while let Some(rest) = prefix.strip_prefix('>') {
        prefix = rest.trim_start_matches([' ', '\t']);
    }

    let heading_markers = prefix.bytes().take_while(|byte| *byte == b'+').count();
    let heading_spacing = &prefix[heading_markers..];
    (1..=6).contains(&heading_markers)
        && !heading_spacing.is_empty()
        && heading_spacing
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\t'))
}

fn recover_legacy_attributed_table<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Option<(Vec<TableRow<'t>>, Vec<ParseError>)>
where
    'r: 't,
{
    let mut scan = parser.clone();
    let mut rows = Vec::new();
    let mut errors = Vec::new();

    loop {
        if scan.current().token == Token::InputEnd {
            return None;
        }

        if !rows.is_empty() && scan.current().token == Token::LeftBlockEnd {
            let mut end = scan.clone();
            if end
                .get_end_block()
                .is_ok_and(|name| name.eq_ignore_ascii_case("table"))
            {
                parser.update(&end);
                return Some((rows, errors));
            }
        } else if scan.current().token == Token::LeftBlock {
            let mut head = scan.clone();
            let is_row = head.get_block_name(false).is_ok_and(|(name, _, segment)| {
                name.eq_ignore_ascii_case("row")
                    && head.head_word_is_actionable(
                        &name,
                        matches!(name, Cow::Borrowed(_)),
                        segment,
                    )
            });

            if is_row {
                let mut candidate = scan.clone();
                if let Ok(success) = RULE_BLOCK.try_consume(&mut candidate)
                    && let Elements::Single(Element::Partial(PartialElement::TableRow(
                        row,
                    ))) = success.item
                {
                    rows.push(row);
                    errors.extend(success.errors);
                    scan.update(&candidate);
                    continue;
                }
            }
        }

        scan.step()
            .expect("tokenization always ends with input-end");
    }
}

fn parse_table<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    // Live Wikidot cannot start a table block on a native list-item line: the
    // markers stay literal text in the item.
    if parser.settings().layout.legacy() && parser.in_wikidot_single_line_content() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let owner_start = table_opener_start(parser, opener_start);
    if let Some(error) =
        cached_underclosed_table_error(parser, BLOCK_TABLE.name, owner_start)
    {
        return Err(error);
    }
    if legacy_block_name_is_spaced(parser, opener_start)
        || legacy_table_follows_heading_marker(parser, opener_start)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let parser = &mut ParserWrap::new(parser, AcceptsPartial::TableRow);
    let block = (&BLOCK_TABLE, "table block");
    let legacy = parser.settings().layout.legacy();
    let probe = (**parser).clone();
    if legacy && !parser.has_body_end_block(&BLOCK_TABLE) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let parsed = match parse_block(
        parser,
        name,
        flag_star,
        flag_score,
        in_head,
        block,
        AdvancedTableElement::Table,
    ) {
        Ok(parsed) => parsed,
        Err(error) => {
            cache_table_end_of_input_error(parser, BLOCK_TABLE.name, owner_start, &error);
            return Err(error);
        }
    };
    let source_end = parser.current().span.start;
    if legacy && !has_explicit_closer(&probe, source_end, &["table"]) {
        if let Some(owner_start) = owner_start {
            parser.cache_underclosed_block_failure(BLOCK_TABLE.name, owner_start);
        }
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    // Live loses the table head's line when the whitespace before a
    // `[[footnote]]` reference is stripped and the reference joins the head
    // line: the head no longer owns a line, so both table markers stay
    // literal text and the reference renders on its own. A body that opens
    // with a footnote reference therefore never forms a legacy table.
    if legacy && table_body_opens_with_footnote(&parsed.elements) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let (mut rows, residuals, foster) = match extract_table_rows(parser, parsed.elements)
    {
        Ok(value) => value,
        Err(error) => {
            if let Some(owner_start) = owner_start {
                parser.cache_underclosed_block_failure(BLOCK_TABLE.name, owner_start);
            }
            return Err(error);
        }
    };
    let mut errors = parsed.errors;
    if rows.is_empty()
        && parsed.has_arguments
        && parser.settings().layout.legacy()
        && let Some((recovered_rows, mut recovered_errors)) =
            recover_legacy_attributed_table(parser)
    {
        rows = recovered_rows;
        errors.append(&mut recovered_errors);
    }
    if rows.is_empty() && parser.settings().layout.legacy() {
        if let Some(owner_start) = owner_start {
            parser.cache_underclosed_block_failure(BLOCK_TABLE.name, owner_start);
        }
        return Err(parser.make_err(ParseErrorKind::TableContainsNonRow));
    }
    if legacy {
        clip_wikidot_table_anchor_scopes(&mut rows);
    }
    let attributes = parsed.attributes;

    let table_type = TableType::Advanced;
    let table = Table {
        rows,
        attributes,
        table_type,
    };
    let element = Element::Table(table);
    let fused_close = parser.wikidot_body_closed_with_fused_close(&BLOCK_TABLE);
    if residuals.is_empty() && foster.is_empty() {
        if fused_close {
            ok!(false; vec![element, text!("]")], errors)
        } else {
            ok!(false; element, errors)
        }
    } else {
        let mut elements = Vec::with_capacity(foster.len() + residuals.len() * 2 + 2);
        elements.extend(foster);
        if !residuals.is_empty() {
            let count = residuals.len();
            let mut fostered = Vec::with_capacity(count * 2);
            for (index, residual) in residuals.into_iter().enumerate() {
                if index > 0 {
                    fostered.push(Element::LineBreak);
                }
                if index + 1 == count {
                    fostered.push(Element::Text(std::borrow::Cow::Owned(format!(
                        "{}\n",
                        residual
                    ))));
                } else {
                    fostered.push(Element::Text(residual));
                }
            }
            elements.push(Element::Partial(PartialElement::WikidotFoster(fostered)));
        }
        elements.push(element);
        if fused_close {
            elements.push(text!("]"));
        }
        ok!(false; Elements::Multiple(elements), errors)
    }
}

fn table_body_opens_with_footnote(elements: &[Element<'_>]) -> bool {
    for element in elements {
        match element {
            Element::LineBreak => continue,
            Element::Text(text) if text.chars().all(char::is_whitespace) => continue,
            Element::Footnote(_) => return true,
            _ => return false,
        }
    }
    false
}

fn parse_row<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    // A row opener on a native list-item line is item text. List already
    // claimed the line, so Tableadv never sees a row head there.
    if parser.settings().layout.legacy() && parser.in_wikidot_single_line_content() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let owner_start = table_opener_start(parser, opener_start);
    if let Some(error) =
        cached_underclosed_table_error(parser, BLOCK_TABLE_ROW.name, owner_start)
    {
        return Err(error);
    }
    if legacy_block_name_is_spaced(parser, opener_start) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let parser = &mut ParserWrap::new(parser, AcceptsPartial::TableCell);
    let block = (&BLOCK_TABLE_ROW, "table row");
    let legacy = parser.settings().layout.legacy();
    let probe = (**parser).clone();
    if legacy && !parser.has_body_end_block(&BLOCK_TABLE_ROW) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if legacy {
        parser.clear_wikidot_list_cut_by_cell_end();
        parser.clear_wikidot_li_tail_transfer();
        parser.clear_wikidot_explicit_list_yield_tail();
        parser.clear_wikidot_cell_div_li_opener();
        parser.clear_wikidot_cross_li_eject();
        parser.clear_wikidot_cell_list_count();
    }
    let parsed = match parse_block(
        parser,
        name,
        flag_star,
        flag_score,
        in_head,
        block,
        AdvancedTableElement::Row,
    ) {
        Ok(parsed) => parsed,
        Err(error) => {
            cache_table_end_of_input_error(
                parser,
                BLOCK_TABLE_ROW.name,
                owner_start,
                &error,
            );
            return Err(error);
        }
    };
    let source_end = parser.current().span.start;
    if legacy && !has_explicit_closer(&probe, source_end, &["row"]) {
        if let Some(owner_start) = owner_start {
            parser.cache_underclosed_block_failure(BLOCK_TABLE_ROW.name, owner_start);
        }
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let straddle_list_pending = legacy && parser.take_wikidot_list_cut_by_cell_end();
    let li_tail_transfer = if legacy {
        parser.take_wikidot_li_tail_transfer()
    } else {
        None
    };
    let li_yield_tail = legacy && parser.take_wikidot_explicit_list_yield_tail();
    let cross_li_eject = if legacy {
        parser.wikidot_cross_li_eject()
    } else {
        Vec::new()
    };
    let cross_li_outer = if legacy {
        parser.take_wikidot_cross_li_outer()
    } else {
        None
    };
    let (cells, residuals, foster) = extract_table_cells(
        parser,
        parsed.elements,
        straddle_list_pending,
        li_tail_transfer,
        li_yield_tail,
        cross_li_eject,
        cross_li_outer,
    )?;
    if cells.is_empty() && parser.settings().layout.legacy() {
        return Err(parser.make_err(ParseErrorKind::TableRowContainsNonCell));
    }
    let attributes = parsed.attributes;
    let errors = parsed.errors;

    let row = TableRow { cells, attributes };
    let element = Element::Partial(PartialElement::TableRow(row));
    if residuals.is_empty() && foster.is_empty() {
        ok!(false; element, errors)
    } else {
        let mut elements = residuals.into_iter().map(Element::Text).collect::<Vec<_>>();
        elements.push(element);
        elements.extend(foster);
        ok!(false; Elements::Multiple(elements), errors)
    }
}

fn clip_wikidot_table_anchor_scopes(rows: &mut [TableRow<'_>]) {
    let mut pending = 0usize;
    for row in rows.iter_mut() {
        for cell in row.cells.iter_mut() {
            clip_wikidot_table_anchor_cell(&mut cell.elements, &mut pending);
        }
    }
}

fn clip_wikidot_table_anchor_cell<'t>(
    elements: &mut Vec<Element<'t>>,
    pending: &mut usize,
) {
    let mut anchors = 0usize;
    elements.retain(|element| match element {
        Element::Partial(PartialElement::InlineAnchorOpen(..)) => {
            anchors += 1;
            true
        }
        Element::Partial(PartialElement::InlineAnchorClose(_)) if *pending > 0 => {
            *pending -= 1;
            false
        }
        Element::Partial(PartialElement::InlineAnchorClose(_)) if anchors > 0 => {
            anchors -= 1;
            true
        }
        _ => true,
    });
    let mut nested_anchors = 0usize;
    for element in elements.iter_mut() {
        if let Element::Container(container) = element {
            nested_anchors += close_nested_anchor_scopes(container.elements_mut());
        }
    }
    for _ in 0..anchors {
        elements.push(Element::Partial(PartialElement::InlineAnchorClose(
            Cow::Borrowed(""),
        )));
    }
    *pending += anchors + nested_anchors;
}

fn close_nested_anchor_scopes<'t>(elements: &mut Vec<Element<'t>>) -> usize {
    let mut closed = 0usize;
    for element in elements.iter_mut() {
        if let Element::Container(container) = element {
            closed += close_nested_anchor_scopes(container.elements_mut());
        }
    }
    let mut anchors = 0usize;
    for element in elements.iter() {
        match element {
            Element::Partial(PartialElement::InlineAnchorOpen(..)) => anchors += 1,
            Element::Partial(PartialElement::InlineAnchorClose(_)) => {
                anchors = anchors.saturating_sub(1)
            }
            _ => {}
        }
    }
    for _ in 0..anchors {
        elements.push(Element::Partial(PartialElement::InlineAnchorClose(
            Cow::Borrowed(""),
        )));
    }
    closed + anchors
}

/// Relocate the kept div-item's trailing line break to the foster lead
/// when a cross-li transfer fired in this cell. Only the div list's last
/// (kept) item owns the break; ejected items keep their own breaks.
fn cross_relocate_div_break<'t>(
    parser: &Parser<'_, '_>,
    cell_elements: &mut [Element<'t>],
) {
    if !parser.settings().layout.legacy() || !parser.wikidot_cross_li_fired() {
        return;
    }
    let Some(ordinal) = parser.wikidot_div_list_ordinal() else {
        return;
    };
    let Some(element) = cell_elements
        .iter_mut()
        .filter_map(|element| match element {
            Element::List { .. } => Some(element),
            _ => None,
        })
        .nth(ordinal)
    else {
        return;
    };
    let Element::List { items, .. } = element else {
        return;
    };
    let mut relocated = false;
    if let Some(crate::tree::ListItem::Elements { elements, .. }) = items.last_mut()
        && matches!(elements.last(), Some(Element::LineBreak))
    {
        elements.pop();
        relocated = true;
    }
    if relocated {
        parser.prepend_wikidot_cross_li_eject(Element::LineBreak);
    }
}

fn parse_cell_regular<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    let legacy = parser.settings().layout.legacy();
    if legacy && parser.in_wikidot_single_line_content() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if legacy && !legacy_some_cell_closer_reaches_row(parser, opener_start) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let cache_rule = if legacy {
        BLOCK_TABLE_CELL_BODY.name
    } else {
        BLOCK_TABLE_CELL_REGULAR.name
    };
    let owner_start = table_opener_start(parser, opener_start);
    if let Some(error) = cached_underclosed_table_error(parser, cache_rule, owner_start) {
        return Err(error);
    }
    if legacy_block_name_is_spaced(parser, opener_start) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let block = if legacy {
        (&BLOCK_TABLE_CELL_BODY, "table cell (regular)")
    } else {
        (&BLOCK_TABLE_CELL_REGULAR, "table cell (regular)")
    };
    let source_start = parser.current().span.start;
    let probe = parser.clone();
    if legacy {
        parser.clear_wikidot_cell_div_li_opener();
        parser.clear_wikidot_cell_list_count();
    }
    if legacy && !parser.has_body_end_block(&BLOCK_TABLE_CELL_BODY) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let parsed = match parse_block(
        parser,
        name,
        flag_star,
        flag_score,
        in_head,
        block,
        AdvancedTableElement::Cell,
    ) {
        Ok(parsed) => parsed,
        Err(error) => {
            cache_table_end_of_input_error(parser, cache_rule, owner_start, &error);
            return Err(error);
        }
    };
    let source_end = parser.current().span.start;
    let source = &parser.full_text().inner()[source_start..source_end];
    if legacy && !has_explicit_closer(&probe, source_end, &["cell", "hcell"]) {
        if let Some(owner_start) = owner_start {
            parser
                .cache_underclosed_block_failure(BLOCK_TABLE_CELL_BODY.name, owner_start);
        }
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let (mut cell_elements, foster, had_foster) =
        split_wikidot_foster(parser, parsed.elements);
    if legacy {
        cross_relocate_div_break(parser, &mut cell_elements);
    }
    let preserve_single_paragraph = legacy && (source.contains("\n\n") || had_foster);
    let header = legacy && cell_closer_is_header(&probe, source_end);

    parse_cell(
        cell_elements,
        parsed.attributes,
        parsed.errors,
        header,
        legacy,
        preserve_single_paragraph,
        legacy,
        foster,
    )
}

fn parse_cell_header<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    let legacy = parser.settings().layout.legacy();
    if legacy && parser.in_wikidot_single_line_content() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if legacy && !legacy_some_cell_closer_reaches_row(parser, opener_start) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let cache_rule = BLOCK_TABLE_CELL_BODY.name;
    let owner_start = table_opener_start(parser, opener_start);
    if let Some(error) = cached_underclosed_table_error(parser, cache_rule, owner_start) {
        return Err(error);
    }
    if legacy_block_name_is_spaced(parser, opener_start) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let parser = &mut ParserWrap::new(parser, AcceptsPartial::TableCell);
    let block = (&BLOCK_TABLE_CELL_BODY, "table cell (header)");
    let source_start = parser.current().span.start;
    let probe = (**parser).clone();
    if legacy {
        parser.clear_wikidot_cell_div_li_opener();
        parser.clear_wikidot_cell_list_count();
    }
    if legacy && !parser.has_body_end_block(&BLOCK_TABLE_CELL_BODY) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let parsed = match parse_block(
        parser,
        name,
        flag_star,
        flag_score,
        in_head,
        block,
        AdvancedTableElement::Cell,
    ) {
        Ok(parsed) => parsed,
        Err(error) => {
            cache_table_end_of_input_error(parser, cache_rule, owner_start, &error);
            return Err(error);
        }
    };
    let source_end = parser.current().span.start;
    let source = &parser.full_text().inner()[source_start..source_end];
    if legacy && !has_explicit_closer(&probe, source_end, &["cell", "hcell"]) {
        if let Some(owner_start) = owner_start {
            parser
                .cache_underclosed_block_failure(BLOCK_TABLE_CELL_BODY.name, owner_start);
        }
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let (mut cell_elements, foster, had_foster) =
        split_wikidot_foster(parser, parsed.elements);
    if legacy {
        cross_relocate_div_break(parser, &mut cell_elements);
    }
    let preserve_single_paragraph = legacy && (source.contains("\n\n") || had_foster);
    let header = cell_closer_is_header(&probe, source_end);

    parse_cell(
        cell_elements,
        parsed.attributes,
        parsed.errors,
        header,
        legacy,
        preserve_single_paragraph,
        legacy,
        foster,
    )
}

fn cell_closer_is_header<'r: 't, 't>(probe: &Parser<'r, 't>, end: usize) -> bool {
    explicit_closer_name(probe, end).as_deref() == Some("hcell")
}

#[allow(clippy::too_many_arguments)]
fn parse_cell<'r, 't>(
    mut elements: Vec<Element<'t>>,
    mut attributes: AttributeMap<'t>,
    errors: Vec<ParseError>,
    header: bool,
    wikidot_paragraphs: bool,
    preserve_single_paragraph: bool,
    legacy: bool,
    foster: Vec<Element<'t>>,
) -> ParseResult<'r, 't, Elements<'t>> {
    let dissolved_pre =
        wikidot_paragraphs && legacy && cell_starts_with_dissolved_pre(&elements);
    let dissolved_leading =
        wikidot_paragraphs && legacy && cell_starts_with_dissolved_leading(&elements);
    trim_cell_whitespace(&mut elements);
    if wikidot_paragraphs {
        strip_cell_paragraph_boundaries(&mut elements);
        if dissolved_pre {
            unwrap_cell_paragraph_before_dissolved_break(&mut elements);
        }
        if dissolved_leading {
            elements.insert(0, Element::LineBreak);
        }
        if !preserve_single_paragraph {
            unwrap_single_cell_paragraph(&mut elements);
        }
        if legacy {
            unwrap_wikidot_paragraph_after_explicit_list(&mut elements);
        }
    }

    let column_span = if legacy {
        if attributes
            .get()
            .get("colspan")
            .is_some_and(|value| value == "0")
        {
            attributes.remove("colspan");
        }
        if attributes
            .get()
            .get("rowspan")
            .is_some_and(|value| value == "0")
        {
            attributes.remove("rowspan");
        }
        attributes
            .get()
            .get("colspan")
            .and_then(|value| parse_column_span_semantic(value))
            .unwrap_or_else(column_span_one)
    } else {
        match attributes.remove("colspan") {
            Some(value) => match value.parse() {
                Ok(value) => value,
                Err(_) if value == "0" => column_span_one(),
                Err(_) => {
                    assert!(attributes.insert("colspan", value));
                    column_span_one()
                }
            },
            None => column_span_one(),
        }
    };

    let cell = TableCell {
        header,
        column_span,
        align: None,
        elements,
        attributes,
    };
    let element = Element::Partial(PartialElement::TableCell(cell));

    if foster.is_empty() {
        ok!(false; element, errors)
    } else {
        let mut elements = Vec::with_capacity(foster.len() + 1);
        elements.push(element);
        elements.extend(foster);
        ok!(false; Elements::Multiple(elements), errors)
    }
}

fn column_span_one() -> NonZeroU32 {
    NonZeroU32::MIN
}

struct BoundedU32DecimalLexeme<'a> {
    significant_digits: &'a str,
}

impl<'a> BoundedU32DecimalLexeme<'a> {
    const MAX_SIGNIFICANT_DIGITS: usize = 10;

    fn from_value(value: &'a str) -> Option<Self> {
        let digits = value.strip_prefix('+').unwrap_or(value);
        let significant_digits = digits.trim_start_matches('0');
        if significant_digits.is_empty()
            || significant_digits.len() > Self::MAX_SIGNIFICANT_DIGITS
        {
            return None;
        }
        Some(BoundedU32DecimalLexeme { significant_digits })
    }

    fn parse_non_zero(self) -> Option<NonZeroU32> {
        self.significant_digits.parse().ok()
    }
}

fn parse_column_span_semantic(value: &str) -> Option<NonZeroU32> {
    BoundedU32DecimalLexeme::from_value(value)?.parse_non_zero()
}

fn strip_cell_paragraph_boundaries(elements: &mut Vec<Element<'_>>) {
    let mut leading_empty = 0;
    for index in 0..elements.len() {
        let keep_break_before_align = next_content_is_align(elements, index);
        let Element::Container(paragraph) = &mut elements[index] else {
            break;
        };
        if paragraph.ctype() != ContainerType::Paragraph {
            break;
        }
        trim_cell_paragraph(paragraph.elements_mut(), keep_break_before_align);
        if !paragraph.elements().is_empty() {
            break;
        }
        leading_empty += 1;
    }
    if leading_empty > 0 {
        elements.drain(..leading_empty);
    }

    while let Some(Element::Container(paragraph)) = elements.last_mut() {
        if paragraph.ctype() != ContainerType::Paragraph {
            break;
        }
        trim_cell_whitespace(paragraph.elements_mut());
        if !paragraph.elements().is_empty() {
            break;
        }
        elements.pop();
    }
}

fn next_content_is_align(elements: &[Element<'_>], index: usize) -> bool {
    elements[index + 1..]
        .iter()
        .find(|element| {
            !matches!(element, Element::Delayed(delayed) if delayed.is_suppression_seam())
        })
        .is_some_and(is_align_block)
}

fn is_align_block(element: &Element<'_>) -> bool {
    match element {
        Element::Container(container) => {
            matches!(container.ctype(), ContainerType::Align(_))
        }
        Element::Partial(PartialElement::WikidotBlockOpen(
            WikidotBlockShell::Align { .. },
        )) => true,
        _ => false,
    }
}

fn trim_cell_paragraph(elements: &mut Vec<Element<'_>>, keep_trailing_break: bool) {
    let had_trailing_break = elements
        .iter()
        .rev()
        .find(|element| {
            !matches!(element, Element::Delayed(delayed) if delayed.is_suppression_seam())
        })
        .is_some_and(|element| *element == Element::LineBreak);
    trim_cell_whitespace(elements);
    if keep_trailing_break
        && had_trailing_break
        && !elements.is_empty()
        && !matches!(elements.last(), Some(Element::LineBreak))
    {
        elements.push(Element::LineBreak);
    }
}

fn trim_cell_whitespace(elements: &mut Vec<Element<'_>>) {
    let skippable = |element: &Element<'_>| {
        element.is_whitespace()
            || matches!(
                element,
                Element::Delayed(delayed) if delayed.is_suppression_seam()
            )
    };
    let start = elements
        .iter()
        .position(|element| !skippable(element))
        .unwrap_or(elements.len());
    let end = elements
        .iter()
        .rposition(|element| !skippable(element))
        .map_or(start, |index| index + 1);

    elements.truncate(end);
    if start > 0 {
        elements.drain(..start);
    }
}

fn unwrap_single_cell_paragraph(elements: &mut Vec<Element<'_>>) {
    if !matches!(
        elements.as_slice(),
        [Element::Container(container)] if container.ctype() == ContainerType::Paragraph
    ) {
        return;
    }

    let Some(Element::Container(paragraph)) = elements.pop() else {
        unreachable!("matched one paragraph container");
    };
    *elements = paragraph.into();
}

/// Whether the pre-trim cell body starts with a paragraph ending in a line
/// break that a dissolved (all-empty) explicit list directly follows: the
/// stack kept that paragraph wrapped (body gathers suppress the toplevel
/// dissolved pre-candidate) and the dissolve break sits beside it. A
/// blank-line separation leaves the paragraph ending without the break,
/// which stays wrapped to match live.
fn cell_starts_with_dissolved_pre(elements: &[Element<'_>]) -> bool {
    let Some(Element::Container(paragraph)) = elements.first() else {
        return false;
    };
    if paragraph.ctype() != ContainerType::Paragraph {
        return false;
    }
    let ends_with_break = paragraph
        .elements()
        .iter()
        .rev()
        .find(|element| {
            !matches!(element, Element::Delayed(delayed) if delayed.is_suppression_seam())
        })
        .is_some_and(|element| *element == Element::LineBreak);
    ends_with_break
        && matches!(
            elements.get(1),
            Some(Element::LineBreak) | Some(Element::Text(_))
        )
}

/// Whether the pre-trim cell body starts with the break an all-empty
/// explicit list dissolved to, followed by content (live keeps it in a
/// cell; the cell trims would otherwise drop it).
fn cell_starts_with_dissolved_leading(elements: &[Element<'_>]) -> bool {
    elements.first() == Some(&Element::LineBreak)
        && matches!(elements.get(1), Some(Element::Text(_)))
}

/// Live keeps the text before a dissolved explicit list in an advanced
/// cell bare like the text before a formed list (`x<br>`, or `x<br>y`
/// when content follows). The stack leaves it wrapped, so unwrap the
/// head paragraph and restore the single break the dissolve stands for.
fn unwrap_cell_paragraph_before_dissolved_break(elements: &mut Vec<Element<'_>>) {
    let Some(Element::Container(paragraph)) = elements.first() else {
        return;
    };
    if paragraph.ctype() != ContainerType::Paragraph {
        return;
    }
    let Element::Container(paragraph) = elements.remove(0) else {
        unreachable!("matched paragraph container");
    };
    let contents: Vec<Element<'_>> = paragraph.into();
    let at = contents.len();
    elements.splice(0..0, contents);
    elements.insert(at, Element::LineBreak);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::ParseError;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};
    use std::panic::catch_unwind;

    fn with_parse<R>(
        source: &str,
        check: impl for<'t> FnOnce(Vec<Element<'t>>, Vec<ParseError>) -> R,
    ) -> R {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
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

    fn render(source: &str) -> String {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, _) = crate::parse(&tokenization, &page_info, &settings).into();
        HtmlRender.render(&tree, &page_info, &settings).body
    }

    #[test]
    fn table_parse_block_rejects_disallowed_flags() {
        let parse_with_flags = |flag_star, flag_score| {
            let page_info = PageInfo::dummy();
            let settings =
                WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
            let tokenization = crate::tokenize("[[table]]\n[[/table]]");
            let mut parser = Parser::new(&tokenization, &page_info, &settings);
            let _ = parse_block(
                &mut parser,
                "table",
                flag_star,
                flag_score,
                false,
                (&BLOCK_TABLE, "table block"),
                AdvancedTableElement::Table,
            );
        };

        let star = catch_unwind(|| parse_with_flags(true, false));
        assert!(star.is_err());

        let score = catch_unwind(|| parse_with_flags(false, true));
        assert!(score.is_err());
    }

    #[test]
    fn advanced_table_preserves_rows_cells_headers_and_colspan() {
        with_parse(
            r#"[[table class="grid"]]
[[row class="top"]]
[[hcell colspan="2" class="heading"]]
 Heading 
[[/hcell]]
[[/row]]
[[row]]
[[cell class="body"]]Content[[/cell]]
[[/row]]
[[/table]]"#,
            |tree, errors| {
                assert!(errors.is_empty(), "{errors:?}");
                let [Element::Table(table)] = tree.as_slice() else {
                    panic!("expected one advanced table, got {tree:?}");
                };

                assert_eq!(table.table_type, TableType::Advanced);
                assert_eq!(
                    table
                        .attributes
                        .get()
                        .get("class")
                        .map(|value| value.as_ref()),
                    Some("grid")
                );
                assert_eq!(table.rows.len(), 2);

                let header_row = &table.rows[0];
                assert_eq!(
                    header_row
                        .attributes
                        .get()
                        .get("class")
                        .map(|value| value.as_ref()),
                    Some("top")
                );
                let [header_cell] = header_row.cells.as_slice() else {
                    panic!("expected one header cell, got {:?}", header_row.cells);
                };
                assert!(header_cell.header);
                assert_eq!(header_cell.column_span.get(), 2);
                assert_eq!(
                    header_cell
                        .attributes
                        .get()
                        .get("class")
                        .map(|value| value.as_ref()),
                    Some("heading")
                );
                assert_eq!(element_text(&header_cell.elements), "Heading");

                let body_row = &table.rows[1];
                let [body_cell] = body_row.cells.as_slice() else {
                    panic!("expected one body cell, got {:?}", body_row.cells);
                };
                assert!(!body_cell.header);
                assert_eq!(body_cell.column_span.get(), 1);
                assert_eq!(
                    body_cell
                        .attributes
                        .get()
                        .get("class")
                        .map(|value| value.as_ref()),
                    Some("body")
                );
                assert_eq!(element_text(&body_cell.elements), "Content");
            },
        );
    }

    #[test]
    fn wikidot_quoted_simple_table_keeps_adjacent_rows_in_one_table() {
        let source = concat!(
            "> Quote lead\n",
            ">\n",
            "> ||~ H || V ||\n",
            "> || A || //B// ||\n",
            ">\n",
            "> trailing quote\n",
            "\n",
            "Intro [[footnote]]note[[/footnote]] tail.",
        );

        let html = render(source);

        assert_eq!(
            html.matches("<table class=\"wiki-content-table\">").count(),
            1,
            "{html}"
        );
        assert_eq!(html.matches("<tr>").count(), 2, "{html}");
    }

    #[test]
    fn advanced_header_cell_accepts_wikidot_cell_closer() {
        with_parse(
            "[[table]][[row]][[hcell]]Heading[[/cell]][[/row]][[/table]]",
            |tree, errors| {
                assert!(errors.is_empty(), "{errors:?}");
                let [Element::Table(table)] = tree.as_slice() else {
                    panic!("expected one advanced table, got {tree:?}");
                };
                let [row] = table.rows.as_slice() else {
                    panic!("expected one row, got {:?}", table.rows);
                };
                let [cell] = row.cells.as_slice() else {
                    panic!("expected one cell, got {:?}", row.cells);
                };

                assert!(!cell.header);
                assert_eq!(element_text(&cell.elements), "Heading");
            },
        );
    }

    #[test]
    fn advanced_table_rejects_non_row_body() {
        with_parse("[[table]]plain text[[/table]]", |_tree, errors| {
            assert!(
                errors
                    .iter()
                    .any(|error| error.kind() == ParseErrorKind::TableContainsNonRow)
            );
        });
    }

    #[test]
    fn malformed_table_closes_reenter_wikidot_single_link_lexing() {
        assert_eq!(
            render("[[table]] [[cell]] Also no row [[/cell]] [[/table]]"),
            concat!(
                "<p>[[table]] [[cell]] Also no row ",
                "[<a href=\"/cell]]\">[[/table</a>]</p>",
            ),
        );
    }

    #[test]
    fn advanced_table_row_rejects_non_cell_body() {
        with_parse(
            "[[table]][[row]]plain text[[/row]][[/table]]",
            |_tree, errors| {
                assert!(
                    errors
                        .iter()
                        .any(|error| error.kind()
                            == ParseErrorKind::TableRowContainsNonCell)
                );
            },
        );
    }

    #[test]
    fn attributed_empty_table_recovers_at_later_valid_table() {
        let source = concat!(
            "[[table class=\"outer\"]]\n",
            "[[/table]]\n\n",
            "[[table]]\n",
            "[[row bad=1]]\n",
            "[[/row]]\n",
            "[[/table]]\n\n",
            "[[table class=\"inner\"]]",
            "[[row]][[cell]]x[[/cell]][[/row]]",
            "[[/table]]",
        );

        assert_eq!(
            render(source),
            "<table class=\"outer\">\n<tr>\n<td>x</td>\n</tr>\n</table>",
        );
    }

    #[test]
    fn bare_attributed_empty_table_recovers_without_rendering_bare_attribute() {
        let source = concat!(
            "[[table bad=1]]\n",
            "[[/table]]\n\n",
            "[[table]][[row]][[cell]]x[[/cell]][[/row]][[/table]]",
        );

        assert_eq!(render(source), "<table>\n<tr>\n<td>x</td>\n</tr>\n</table>",);
    }

    #[test]
    fn attributed_empty_table_without_later_rows_remains_literal() {
        assert_eq!(
            render("[[table bad=1]]\n[[/table]]"),
            "<p>[[table bad=1]]<br>\n[[/table]]</p>",
        );
    }

    #[test]
    fn unadorned_empty_table_does_not_consume_later_valid_table() {
        let source = concat!(
            "[[table]]\n",
            "[[/table]]\n\n",
            "[[table]][[row]][[cell]]x[[/cell]][[/row]][[/table]]",
        );

        assert_eq!(
            render(source),
            concat!(
                "<p>[[table]]<br>\n[[/table]]</p>",
                "<table>\n<tr>\n<td>x</td>\n</tr>\n</table>",
            ),
        );
    }

    #[test]
    fn wikidot_nested_table_wraps_blank_line_text_in_paragraph() {
        let source = concat!(
            "[[table]][[row]][[cell]]\n\n",
            "1\n\n",
            "[[table]][[row]][[cell]]2[[/cell]][[/row]][[/table]]",
            "[[/cell]][[/row]][[/table]]",
        );

        assert_eq!(
            render(source),
            concat!(
                "<table>\n<tr>\n<td><p>1</p>",
                "<table>\n<tr>\n<td>2</td>\n</tr>\n</table>",
                "</td>\n</tr>\n</table>",
            ),
        );
    }

    #[test]
    fn parse_cell_strips_whitespace_and_defaults_colspan() {
        let mut attributes = AttributeMap::new();
        assert!(attributes.insert("class", cow!("plain")));
        let elements = vec![
            Element::Text(cow!(" ")),
            Element::Text(cow!("Cell")),
            Element::Text(cow!(" ")),
        ];
        let success = parse_cell(
            elements,
            attributes,
            Vec::new(),
            false,
            false,
            false,
            true,
            Vec::new(),
        )
        .unwrap();
        let Elements::Single(Element::Partial(PartialElement::TableCell(cell))) =
            success.item
        else {
            panic!("expected one table cell, got {:?}", success.item);
        };

        assert!(!cell.header);
        assert_eq!(cell.column_span.get(), 1);
        assert_eq!(
            cell.attributes
                .get()
                .get("class")
                .map(|value| value.as_ref()),
            Some("plain")
        );
        assert_eq!(element_text(&cell.elements), "Cell");
    }
}
