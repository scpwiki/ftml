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
use crate::parsing::ParserWrap;
use crate::parsing::rule::impls::block::RULE_BLOCK;
use crate::settings::WikitextMode;
use crate::tree::{
    AcceptsPartial, AttributeMap, ContainerType, PartialElement, Table, TableCell,
    TableRow, TableType,
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
    let body =
        parser.get_body_elements_with_context(block_rule, as_paragraphs, body_start)?;
    let (elements, errors, _) = body.into();

    let parsed = ParsedBlock {
        elements,
        attributes,
        errors,
        has_arguments,
    };
    Ok(parsed)
}

#[allow(clippy::type_complexity)]
fn extract_table_rows<'r, 't>(
    parser: &Parser<'r, 't>,
    elements: Vec<Element<'t>>,
) -> Result<(Vec<TableRow<'t>>, Vec<Cow<'t, str>>, Vec<Element<'t>>), ParseError> {
    let mut rows = Vec::new();
    let mut residuals = Vec::new();
    let mut foster = Vec::new();

    for element in elements {
        match element {
            Element::Partial(PartialElement::TableRow(row)) => {
                rows.push(row);
            }

            Element::Partial(PartialElement::WikidotFoster(_)) => {
                foster.push(element);
            }

            Element::Partial(
                PartialElement::WikidotBlockOpen(_)
                | PartialElement::WikidotBlockClose(_),
            ) => {
                foster.push(element);
            }

            Element::Text(text) if is_cell_closer(&text) => {
                residuals.push(text);
            }

            Element::Delayed(delayed) if delayed.is_suppression_seam() => {}

            element if element.is_whitespace() => {}

            _ => return Err(parser.make_err(ParseErrorKind::TableContainsNonRow)),
        }
    }

    Ok((rows, residuals, foster))
}

#[allow(clippy::type_complexity)]
fn extract_table_cells<'r, 't>(
    parser: &Parser<'r, 't>,
    elements: Vec<Element<'t>>,
) -> Result<(Vec<TableCell<'t>>, Vec<Cow<'t, str>>, Vec<Element<'t>>), ParseError> {
    let mut cells = Vec::new();
    let mut residuals = Vec::new();
    let mut foster = Vec::new();

    for element in elements {
        match element {
            Element::Partial(PartialElement::TableCell(cell)) if residuals.is_empty() => {
                cells.push(cell);
            }

            Element::Partial(PartialElement::WikidotFoster(_)) => {
                foster.push(element);
            }

            Element::Partial(
                PartialElement::WikidotBlockOpen(_)
                | PartialElement::WikidotBlockClose(_),
            ) => {
                foster.push(element);
            }

            Element::Text(text) if !cells.is_empty() && is_cell_closer(&text) => {
                residuals.push(text);
            }

            Element::Delayed(delayed) if delayed.is_suppression_seam() => {}

            element if element.is_whitespace() => {}

            _ => return Err(parser.make_err(ParseErrorKind::TableRowContainsNonCell)),
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
    match split {
        Some(index) => {
            let mut cell = elements;
            let mut foster = cell.split_off(index);
            foster.remove(0);
            let pre = partition_wikidot_foster(foster);
            (cell, pre, true)
        }
        None => (elements, Vec::new(), false),
    }
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
            if !run.is_empty() {
                pre.push(Element::Partial(PartialElement::WikidotFoster(
                    std::mem::take(&mut run),
                )));
            }
            pre.push(element);
        } else {
            run.push(element);
        }
    }
    if !run.is_empty() {
        pre.push(Element::Partial(PartialElement::WikidotFoster(run)));
    }
    pre
}

fn is_cell_closer(source: &str) -> bool {
    source.eq_ignore_ascii_case("[[/cell]]") || source.eq_ignore_ascii_case("[[/hcell]]")
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
    let source = &probe.full_text().inner()[..end];
    accepted.iter().any(|name| {
        let close = format!("[[/{name}]]]");
        source
            .get(source.len().saturating_sub(close.len())..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(&close))
    })
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
        let mut elements = Vec::with_capacity(foster.len() + residuals.len() * 2 + 1);
        elements.extend(foster);
        let has_residuals = !residuals.is_empty();
        for (index, residual) in residuals.into_iter().enumerate() {
            if index > 0 {
                elements.push(Element::LineBreak);
            }
            elements.push(Element::Text(residual));
        }
        if has_residuals {
            elements.push(text!("\n"));
        }
        elements.push(element);
        if fused_close {
            elements.push(text!("]"));
        }
        ok!(false; Elements::Multiple(elements), errors)
    }
}

fn parse_row<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
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

    let (cells, residuals, foster) = extract_table_cells(parser, parsed.elements)?;
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

fn parse_cell_regular<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    let legacy = parser.settings().layout.legacy();
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
    let (cell_elements, foster, had_foster) =
        split_wikidot_foster(parser, parsed.elements);
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
    let (cell_elements, foster, had_foster) =
        split_wikidot_foster(parser, parsed.elements);
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
    trim_cell_whitespace(&mut elements);
    if wikidot_paragraphs {
        strip_cell_paragraph_boundaries(&mut elements);
        if !preserve_single_paragraph {
            unwrap_single_cell_paragraph(&mut elements);
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
    NonZeroU32::new(1).expect("one is non-zero")
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
    for element in elements.iter_mut() {
        let Element::Container(paragraph) = element else {
            break;
        };
        if paragraph.ctype() != ContainerType::Paragraph {
            break;
        }
        trim_cell_whitespace(paragraph.elements_mut());
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
