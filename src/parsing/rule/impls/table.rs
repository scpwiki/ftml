/*
 * parsing/rule/impls/table.rs
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

use super::block::blocks::BLOCK_TABLE;
use super::prelude::*;
use crate::parsing::collect::{SegmentCtx, TokenWindow, split_delimiter_end};
use crate::tree::{
    Alignment, AttributeMap, Container, ContainerType, PartialElement, Table, TableCell,
    TableRow, TableType,
};
use regex::Regex;
use std::borrow::Cow;
use std::mem;
use std::num::NonZeroU32;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy)]
struct TableCellStart<'t> {
    align: Option<Alignment>,
    header: bool,
    column_span: NonZeroU32,
    literal_prefix: Option<&'t str>,
}

fn table_block_head_crosses_line<'r: 't, 't>(parser: &Parser<'r, 't>) -> bool {
    if parser.current().token != Token::LeftBlock {
        return false;
    }
    let mut probe = parser.clone();
    let Ok((name, _, segment)) = probe.get_block_name(false) else {
        return false;
    };
    if !name.eq_ignore_ascii_case("table")
        || !probe.head_word_is_actionable(
            &name,
            matches!(name, Cow::Borrowed(_)),
            segment,
        )
    {
        return false;
    }
    !parser.has_body_end_block_on_line(&BLOCK_TABLE)
}

pub const RULE_TABLE: Rule = Rule {
    name: "table",
    position: LineRequirement::StartOfLine,
    try_consume_fn,
};

fn take_row<'t>(cells: &mut Vec<TableCell<'t>>) -> TableRow<'t> {
    TableRow {
        cells: mem::take(cells),
        attributes: AttributeMap::new(),
    }
}

fn push_row<'t>(rows: &mut Vec<TableRow<'t>>, cells: &mut Vec<TableCell<'t>>) {
    let row = take_row(cells);
    rows.push(row);
}

fn take_cell<'t>(
    elements: &mut Vec<Element<'t>>,
    cell_start: TableCellStart<'t>,
) -> TableCell<'t> {
    let elements = mem::take(elements);
    let attributes = AttributeMap::new();
    TableCell {
        elements,
        header: cell_start.header,
        column_span: cell_start.column_span,
        align: cell_start.align,
        attributes,
    }
}

fn push_cell<'t>(
    cells: &mut Vec<TableCell<'t>>,
    elements: &mut Vec<Element<'t>>,
    cell_start: TableCellStart<'t>,
    pending_scopes: &mut PendingInlineScopes,
) {
    clip_wikidot_inline_scopes_at_cell_boundary(elements, pending_scopes);
    let cell = take_cell(elements, cell_start);
    cells.push(cell);
}

#[derive(Debug, Default)]
struct PendingInlineScopes {
    spans: usize,
    sizes: usize,
    anchors: usize,
}

fn clip_wikidot_inline_scopes_at_cell_boundary<'t>(
    elements: &mut Vec<Element<'t>>,
    pending: &mut PendingInlineScopes,
) {
    let mut spans = 0usize;
    let mut sizes = 0usize;
    let mut anchors = 0usize;
    elements.retain(|element| match element {
        Element::Partial(PartialElement::InlineSpanOpen(..)) => {
            spans += 1;
            true
        }
        Element::Partial(PartialElement::InlineSpanClose(_)) if pending.spans > 0 => {
            pending.spans -= 1;
            false
        }
        Element::Partial(PartialElement::InlineSpanClose(_)) if spans > 0 => {
            spans -= 1;
            true
        }
        Element::Partial(PartialElement::InlineSizeOpen(..)) => {
            sizes += 1;
            true
        }
        Element::Partial(PartialElement::InlineSizeClose(_)) if pending.sizes > 0 => {
            pending.sizes -= 1;
            false
        }
        Element::Partial(PartialElement::InlineSizeClose(_)) if sizes > 0 => {
            sizes -= 1;
            true
        }
        Element::Partial(PartialElement::InlineAnchorOpen(..)) => {
            anchors += 1;
            true
        }
        Element::Partial(PartialElement::InlineAnchorClose(_)) if pending.anchors > 0 => {
            pending.anchors -= 1;
            false
        }
        Element::Partial(PartialElement::InlineAnchorClose(_)) if anchors > 0 => {
            anchors -= 1;
            true
        }
        _ => true,
    });
    for _ in 0..spans {
        elements.push(Element::Partial(PartialElement::InlineSpanClose(
            Cow::Borrowed(""),
        )));
    }
    for _ in 0..sizes {
        elements.push(Element::Partial(PartialElement::InlineSizeClose(
            Cow::Borrowed(""),
        )));
    }
    for _ in 0..anchors {
        elements.push(Element::Partial(PartialElement::InlineAnchorClose(
            Cow::Borrowed(""),
        )));
    }
    pending.spans += spans;
    pending.sizes += sizes;
    pending.anchors += anchors;
    let mut nested_spans = 0usize;
    let mut nested_sizes = 0usize;
    let mut nested_anchors = 0usize;
    for element in elements.iter_mut() {
        if let Element::Container(container) = element {
            let (spans, sizes, anchors) =
                close_nested_inline_scopes(container.elements_mut());
            nested_spans += spans;
            nested_sizes += sizes;
            nested_anchors += anchors;
        }
    }
    pending.spans += nested_spans;
    pending.sizes += nested_sizes;
    pending.anchors += nested_anchors;
}

fn close_wikidot_inline_scopes<'t>(
    elements: &mut Vec<Element<'t>>,
) -> (usize, usize, usize) {
    let mut spans = 0usize;
    let mut sizes = 0usize;
    let mut anchors = 0usize;
    for element in elements.iter() {
        match element {
            Element::Partial(PartialElement::InlineSpanOpen(..)) => spans += 1,
            Element::Partial(PartialElement::InlineSpanClose(_)) => {
                spans = spans.saturating_sub(1)
            }
            Element::Partial(PartialElement::InlineSizeOpen(..)) => sizes += 1,
            Element::Partial(PartialElement::InlineSizeClose(_)) => {
                sizes = sizes.saturating_sub(1)
            }
            Element::Partial(PartialElement::InlineAnchorOpen(..)) => anchors += 1,
            Element::Partial(PartialElement::InlineAnchorClose(_)) => {
                anchors = anchors.saturating_sub(1)
            }
            _ => {}
        }
    }
    for _ in 0..spans {
        elements.push(Element::Partial(PartialElement::InlineSpanClose(
            Cow::Borrowed(""),
        )));
    }
    for _ in 0..sizes {
        elements.push(Element::Partial(PartialElement::InlineSizeClose(
            Cow::Borrowed(""),
        )));
    }
    for _ in 0..anchors {
        elements.push(Element::Partial(PartialElement::InlineAnchorClose(
            Cow::Borrowed(""),
        )));
    }
    (spans, sizes, anchors)
}

fn swallow_early_closed_scopes<'t>(
    elements: &mut Vec<Element<'t>>,
    start: usize,
    spans: &mut usize,
    sizes: &mut usize,
    anchors: &mut usize,
) {
    let mut index = start;
    while index < elements.len() {
        let drop = match &elements[index] {
            Element::Partial(PartialElement::InlineSpanClose(_)) if *spans > 0 => {
                *spans -= 1;
                true
            }
            Element::Partial(PartialElement::InlineSizeClose(_)) if *sizes > 0 => {
                *sizes -= 1;
                true
            }
            Element::Partial(PartialElement::InlineAnchorClose(_)) if *anchors > 0 => {
                *anchors -= 1;
                true
            }
            _ => false,
        };
        if drop {
            elements.remove(index);
        } else {
            index += 1;
        }
    }
}

fn close_nested_inline_scopes<'t>(
    elements: &mut Vec<Element<'t>>,
) -> (usize, usize, usize) {
    let mut closed_spans = 0usize;
    let mut closed_sizes = 0usize;
    let mut closed_anchors = 0usize;
    for element in elements.iter_mut() {
        if let Element::Container(container) = element {
            let (spans, sizes, anchors) =
                close_nested_inline_scopes(container.elements_mut());
            closed_spans += spans;
            closed_sizes += sizes;
            closed_anchors += anchors;
        }
    }
    let mut spans = 0usize;
    let mut sizes = 0usize;
    let mut anchors = 0usize;
    for element in elements.iter() {
        match element {
            Element::Partial(PartialElement::InlineSpanOpen(..)) => spans += 1,
            Element::Partial(PartialElement::InlineSpanClose(_)) => {
                spans = spans.saturating_sub(1)
            }
            Element::Partial(PartialElement::InlineSizeOpen(..)) => sizes += 1,
            Element::Partial(PartialElement::InlineSizeClose(_)) => {
                sizes = sizes.saturating_sub(1)
            }
            Element::Partial(PartialElement::InlineAnchorOpen(..)) => anchors += 1,
            Element::Partial(PartialElement::InlineAnchorClose(_)) => {
                anchors = anchors.saturating_sub(1)
            }
            _ => {}
        }
    }
    for _ in 0..spans {
        elements.push(Element::Partial(PartialElement::InlineSpanClose(
            Cow::Borrowed(""),
        )));
    }
    for _ in 0..sizes {
        elements.push(Element::Partial(PartialElement::InlineSizeClose(
            Cow::Borrowed(""),
        )));
    }
    for _ in 0..anchors {
        elements.push(Element::Partial(PartialElement::InlineAnchorClose(
            Cow::Borrowed(""),
        )));
    }
    (
        closed_spans + spans,
        closed_sizes + sizes,
        closed_anchors + anchors,
    )
}

fn append_cell_elements<'t>(all_elements: &mut Vec<Element<'t>>, elements: Elements<'t>) {
    match elements {
        Elements::None => {}
        Elements::Single(element) => all_elements.push(element),
        Elements::Multiple(mut elements) => {
            if all_elements.is_empty() {
                *all_elements = elements;
            } else {
                all_elements.append(&mut elements);
            }
        }
    }
}

fn simple_table<'t>(rows: Vec<TableRow<'t>>) -> Element<'t> {
    let attributes = AttributeMap::new();
    let table_type = TableType::Simple;
    let table = Table {
        rows,
        attributes,
        table_type,
    };
    Element::Table(table)
}

fn is_table_column_token(token: Token) -> bool {
    matches!(
        token,
        Token::TableColumn
            | Token::TableColumnTitle
            | Token::TableColumnCenter
            | Token::TableColumnRight
    )
}

enum CellBoundary {
    FinishTable,
    FinishRow,
    ContinueCell,
}

struct CellState<'a, 't> {
    rows: &'a mut Vec<TableRow<'t>>,
    cells: &'a mut Vec<TableCell<'t>>,
    elements: &'a mut Vec<Element<'t>>,
    pending_scopes: &'a mut PendingInlineScopes,
}

impl<'a, 't> CellState<'a, 't> {
    fn new(
        rows: &'a mut Vec<TableRow<'t>>,
        cells: &'a mut Vec<TableCell<'t>>,
        elements: &'a mut Vec<Element<'t>>,
        pending_scopes: &'a mut PendingInlineScopes,
    ) -> Self {
        Self {
            rows,
            cells,
            elements,
            pending_scopes,
        }
    }
}

fn finish_simple_table<'r, 't>(
    rows: Vec<TableRow<'t>>,
    errors: Vec<ParseError>,
) -> ParseResult<'r, 't, Elements<'t>> {
    let table = simple_table(rows);
    ok!(false; table, errors)
}

fn finish_simple_table_then<'r, 't>(
    rows: Vec<TableRow<'t>>,
    interior: Vec<Element<'t>>,
    errors: Vec<ParseError>,
) -> ParseResult<'r, 't, Elements<'t>> {
    let table = simple_table(rows);
    if interior.is_empty() {
        return ok!(false; table, errors);
    }
    let mut elements = Vec::with_capacity(interior.len() + 1);
    elements.push(table);
    elements.extend(interior);
    ok!(false; Elements::Multiple(elements), errors)
}

fn simple_table_row_block_interior<'t>(
    elements: Elements<'t>,
) -> Result<Vec<Element<'t>>, Elements<'t>> {
    fn is_row_block(element: &Element<'_>) -> bool {
        match element {
            Element::Container(container) => container.ctype() == ContainerType::Note,
            Element::Collapsible { .. } => true,
            _ => false,
        }
    }
    fn interior<'t>(element: Element<'t>) -> Vec<Element<'t>> {
        match element {
            Element::Container(container) => container.into(),
            Element::Collapsible { elements, .. } => elements,
            other => vec![other],
        }
    }
    match elements {
        Elements::Single(element) if is_row_block(&element) => Ok(interior(element)),
        Elements::Multiple(items) if items.iter().any(is_row_block) => {
            let mut out = Vec::new();
            for element in items {
                if is_row_block(&element) {
                    out.extend(interior(element));
                } else {
                    out.push(element);
                }
            }
            Ok(out)
        }
        other => Err(other),
    }
}

fn element_is_row_suffix(element: &Element<'_>) -> bool {
    matches!(element, Element::Text(text) if text.as_ref() == "||")
}

fn row_suffix_index<'r, 't>(
    parser: &mut Parser<'r, 't>,
    elements: &[Element<'t>],
) -> Option<usize> {
    let at = parser.wikidot_row_suffix_at()?;
    if let Some(index) = elements.iter().rposition(element_is_row_suffix) {
        return Some(index);
    }
    if parser.current().span.start > at {
        parser.clear_wikidot_row_suffix_at();
    }
    None
}

pub(crate) fn eject_wikidot_row_suffix<'r, 't>(
    parser: &mut Parser<'r, 't>,
    elements: &mut Vec<Element<'t>>,
) -> Vec<Element<'t>> {
    let Some(index) = row_suffix_index(parser, elements) else {
        return Vec::new();
    };
    if elements.len() == index + 2 && matches!(elements.last(), Some(Element::LineBreak))
    {
        elements.pop();
    }
    elements.split_off(index)
}

pub(crate) fn keep_wikidot_row_suffix<'r, 't>(
    parser: &mut Parser<'r, 't>,
    elements: &mut Vec<Element<'t>>,
) -> Option<usize> {
    let index = row_suffix_index(parser, elements)?;
    if elements.len() == index + 2 && matches!(elements.last(), Some(Element::LineBreak))
    {
        elements.pop();
    }
    parser.clear_wikidot_row_suffix_at();
    Some(index)
}

fn take_simple_row_line_suffix<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Vec<Element<'t>>, ParseError> {
    let mut suffix = Vec::new();
    loop {
        match parser.current().token {
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => break,
            Token::LeftBlock | Token::LeftBlockStar | Token::LeftBlockEnd => break,
            _ => {
                let slice = parser.current().slice;
                if !slice.is_empty() {
                    suffix.push(text!(slice));
                }
                parser.step()?;
            }
        }
    }
    Ok(suffix)
}

fn block_opens_on_simple_row(source: &str, opener_byte: usize, end_byte: usize) -> bool {
    if opener_byte >= end_byte || opener_byte > source.len() {
        return false;
    }
    let line_start = source[..opener_byte]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    source[line_start..opener_byte].contains("||")
        && source[opener_byte..end_byte.min(source.len())].contains('\n')
}

fn finish_table_or_fail<'r, 't>(
    parser: &Parser<'r, 't>,
    rows: Vec<TableRow<'t>>,
    errors: Vec<ParseError>,
) -> ParseResult<'r, 't, Elements<'t>> {
    let has_rows = !rows.is_empty();
    if has_rows {
        finish_simple_table(rows, errors)
    } else {
        Err(parser.make_err(ParseErrorKind::RuleFailed))
    }
}

fn finish_cell_and_table<'r, 't>(
    parser: &mut Parser<'r, 't>,
    state: &mut CellState<'_, 't>,
    cell_start: TableCellStart<'t>,
    steps: usize,
) -> Result<CellBoundary, ParseError> {
    push_cell(
        state.cells,
        state.elements,
        cell_start,
        state.pending_scopes,
    );
    push_row(state.rows, state.cells);
    parser.step_n(steps)?;
    let boundary = CellBoundary::FinishTable;
    Ok(boundary)
}

fn finish_cell_and_row<'r, 't>(
    parser: &mut Parser<'r, 't>,
    state: &mut CellState<'_, 't>,
    cell_start: TableCellStart<'t>,
    steps: usize,
) -> Result<CellBoundary, ParseError> {
    push_cell(
        state.cells,
        state.elements,
        cell_start,
        state.pending_scopes,
    );
    parser.step_n(steps)?;
    let boundary = CellBoundary::FinishRow;
    Ok(boundary)
}

fn cell_boundary<'r, 't>(
    parser: &mut Parser<'r, 't>,
    state: &mut CellState<'_, 't>,
    cell_start: TableCellStart<'t>,
    next: Token,
) -> Result<CellBoundary, ParseError> {
    match next {
        Token::ParagraphBreak | Token::InputEnd => {
            finish_cell_and_table(parser, state, cell_start, 1)
        }
        Token::LineBreak => finish_cell_and_row(parser, state, cell_start, 2),
        Token::Whitespace => match parser.look_ahead(1).map(|t| t.token) {
            Some(Token::ParagraphBreak) | Some(Token::InputEnd) | None => {
                finish_cell_and_table(parser, state, cell_start, 2)
            }
            Some(Token::LineBreak) => finish_cell_and_row(parser, state, cell_start, 3),
            _ => Ok(CellBoundary::ContinueCell),
        },
        _ => Ok(CellBoundary::ContinueCell),
    }
}

/// Wikidot keeps a single line break before a simple (unbracketed) table when
/// that break directly follows a div or code closer. In every other setting
/// (start of input, blank line, quote or paragraph content) the break is
/// dropped or absorbed by the surrounding paragraph. Reports whether the
/// table at the current position is in the break-preserving setting.
fn wikidot_simple_table_preserves_break(parser: &Parser<'_, '_>) -> bool {
    if !parser.settings().layout.legacy() {
        return false;
    }
    let pos = parser.current().span.start;
    if pos == 0 {
        return false;
    }
    let before = &parser.full_text().inner()[..pos];
    let line = match before.strip_suffix('\n') {
        Some(line) => line,
        None => return false,
    };
    let line = match line.rsplit('\n').next() {
        Some(line) => line,
        None => return false,
    };
    let line = line.strip_suffix('\r').unwrap_or(line);
    line.eq_ignore_ascii_case("[[/div]]") || line.eq_ignore_ascii_case("[[/code]]")
}

pub(crate) fn wikidot_simple_table_markers_literal_on_line_after_explicit_li(
    parser: &Parser<'_, '_>,
    table_token_start: usize,
) -> bool {
    if !parser.settings().layout.legacy() {
        return false;
    }
    let text = parser.full_text().inner();
    if table_token_start > text.len() {
        return false;
    }
    let line_start = text[..table_token_start]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    if line_start == 0 {
        return false;
    }
    let line_end = text[table_token_start..]
        .find(['\r', '\n'])
        .map(|offset| table_token_start + offset)
        .unwrap_or(text.len());
    let line = text[line_start..line_end].trim_start();
    if !line.starts_with("||") {
        return false;
    }
    let prev_nl = line_start - 1;
    if text.as_bytes()[prev_nl] != b'\n' {
        return false;
    }
    let prev_line_start = text[..prev_nl]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let prev_raw = &text[prev_line_start..prev_nl];
    let prev_line = prev_raw
        .trim_start()
        .strip_suffix('\r')
        .unwrap_or(prev_raw)
        .trim_end();
    if prev_line.is_empty() {
        return false;
    }
    if prev_line.eq_ignore_ascii_case("[[li]]") {
        return true;
    }
    let lower = prev_line.to_ascii_lowercase();
    lower.starts_with("[[li ") || lower.starts_with("[[li\t")
}

struct UnclosedRow {
    resume: usize,
    swallow: Vec<usize>,
}

fn finish_discarded_unclosed_row<'r, 't>(
    parser: &mut Parser<'r, 't>,
    plan: UnclosedRow,
) -> ParseResult<'r, 't, Elements<'t>> {
    while parser.current().token != Token::InputEnd
        && parser.current().span.start < plan.resume
    {
        parser.step()?;
    }
    for closer in plan.swallow {
        parser.push_wikidot_discarded_row_closer(closer);
    }
    let cell = TableCell {
        elements: Vec::new(),
        header: false,
        column_span: NonZeroU32::new(1).unwrap(),
        align: None,
        attributes: AttributeMap::new(),
    };
    let row = TableRow {
        cells: vec![cell],
        attributes: AttributeMap::new(),
    };
    finish_simple_table(vec![row], Vec::new())
}

fn try_glued_source_list_row<'r, 't>(
    parser: &mut Parser<'r, 't>,
    pipe: usize,
) -> Result<Option<crate::parsing::ParseSuccess<'r, 't, Elements<'t>>>, ParseError> {
    let source = parser.full_text().inner();
    let Some((cells, tail)) = glued_source_list_cells(source, pipe) else {
        return Ok(None);
    };
    while parser.current().token != Token::InputEnd {
        parser.step()?;
    }
    let mut row_cells = Vec::with_capacity(cells.len());
    for (span, text) in cells {
        let mut elements = Vec::new();
        if !text.is_empty() {
            elements.push(Element::Text(Cow::Owned(text)));
        }
        row_cells.push(TableCell {
            elements,
            header: false,
            column_span: NonZeroU32::new(span).unwrap_or(NonZeroU32::new(1).unwrap()),
            align: None,
            attributes: AttributeMap::new(),
        });
    }
    let table = simple_table(vec![TableRow {
        cells: row_cells,
        attributes: AttributeMap::new(),
    }]);
    let mut elements = vec![table];
    elements.extend(strong_tail_elements(&tail));
    Ok(Some(crate::parsing::ParseSuccess::new(
        Elements::Multiple(elements),
        Vec::new(),
        false,
    )))
}

fn strong_tail_elements(tail: &str) -> Vec<Element<'static>> {
    let mut elements = Vec::new();
    let bytes = tail.as_bytes();
    let mut index = 0;
    let mut plain = String::new();
    while index < bytes.len() {
        if bytes[index] == b'*'
            && bytes.get(index + 1) == Some(&b'*')
            && let Some(close) = tail[index + 2..].find("**")
        {
            if !plain.is_empty() {
                elements.push(Element::Text(Cow::Owned(mem::take(&mut plain))));
            }
            let body = tail[index + 2..index + 2 + close].to_string();
            elements.push(Element::Container(Container::new(
                ContainerType::Bold,
                vec![Element::Text(Cow::Owned(body))],
                AttributeMap::new(),
            )));
            index += 2 + close + 2;
            continue;
        }
        let ch = tail[index..].chars().next().unwrap();
        plain.push(ch);
        index += ch.len_utf8();
    }
    if !plain.is_empty() {
        elements.push(Element::Text(Cow::Owned(plain)));
    }
    elements
}

// Ullist eats the break after [[ul]], so item text lands on the || line.
// The table rule then splits that one line and drops its last cell.
fn glued_source_list_cells(
    source: &str,
    pipe: usize,
) -> Option<(Vec<(u32, String)>, String)> {
    if !explicit_list_on_pipe_line(source, pipe)
        || !pipes_before_list_closer(source, pipe)
    {
        return None;
    }
    let mut text = String::from("\n");
    text.push_str(&source[pipe..]);
    text.push_str("\n\n");
    text = apply_span(&text);
    text = apply_div(&text);
    text = apply_collapsible(&text);
    text = apply_bullet_list(&text);
    text = apply_ullist(&text);
    let Some((row, rest)) = first_unclosed_table_row(&text) else {
        return None;
    };
    let parts: Vec<&str> = row.split("||").collect();
    if parts.len() < 2 {
        return None;
    }
    let last = parts.len() - 1;
    let mut span = 1u32;
    let mut cells = Vec::new();
    for part in parts.iter().take(last).skip(1) {
        if part.is_empty() {
            span += 1;
            continue;
        }
        cells.push((span, visible_cell(part)));
        span = 1;
    }
    if cells.is_empty() {
        return None;
    }
    Some((cells, visible_tail(rest)))
}

fn first_unclosed_table_row(text: &str) -> Option<(&str, &str)> {
    if !text.starts_with("\n||") {
        return None;
    }
    let bytes = text.as_bytes();
    let mut index = 1;
    while index < bytes.len() {
        if bytes[index] == b'\n' && !text[index + 1..].starts_with("||") {
            return Some((&text[1..index], &text[index + 1..]));
        }
        index += 1;
    }
    None
}

fn explicit_list_on_pipe_line(source: &str, pipe: usize) -> bool {
    let line_end = source[pipe..]
        .find(['\n', '\r'])
        .map(|offset| pipe + offset)
        .unwrap_or(source.len());
    let line = &source[pipe..line_end];
    let lower = line.to_ascii_lowercase();
    lower.contains("[[ul]]")
        || lower.contains("[[ol]]")
        || lower.contains("[[ul ")
        || lower.contains("[[ol ")
}

fn pipes_before_list_closer(source: &str, pipe: usize) -> bool {
    let line_end = source[pipe..]
        .find(['\n', '\r'])
        .map(|offset| pipe + offset)
        .unwrap_or(source.len());
    let line = &source[pipe..line_end];
    let lower = line.to_ascii_lowercase();
    let name = if lower.contains("[[ul]]") || lower.contains("[[ul ") {
        "ul"
    } else if lower.contains("[[ol]]") || lower.contains("[[ol ") {
        "ol"
    } else {
        return false;
    };
    let Some(opener) = line.find("[[").map(|offset| pipe + offset) else {
        return false;
    };
    let Some(opener_end) = source[opener..]
        .find("]]")
        .map(|offset| opener + offset + 2)
    else {
        return false;
    };
    let Some(closer) = find_balanced_closer(source, opener_end, name) else {
        return false;
    };
    let mut index = line_end;
    if index < source.len() && source.as_bytes()[index] == b'\r' {
        index += 1;
    }
    if index < source.len() && source.as_bytes()[index] == b'\n' {
        index += 1;
    }
    while index < closer {
        if source_list_line(source, index) && source[index..closer].contains("||") {
            let line_end = source[index..]
                .find('\n')
                .map(|offset| index + offset)
                .unwrap_or(source.len());
            if source[index..line_end.min(closer)].contains("||") {
                return true;
            }
        }
        if !source_list_line(source, index) {
            return false;
        }
        match source[index..].find('\n') {
            Some(offset) => index += offset + 1,
            None => return false,
        }
    }
    false
}

fn visible_cell(raw: &str) -> String {
    strip_brackets(&strip_marks(raw)).trim().to_string()
}

fn visible_tail(raw: &str) -> String {
    strip_brackets(&strip_marks(raw))
        .chars()
        .filter(|character| *character != '\n' && *character != '\r')
        .collect()
}

fn strip_marks(text: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for character in text.chars() {
        if character == '\u{E000}' {
            inside = !inside;
            continue;
        }
        if !inside {
            out.push(character);
        }
    }
    out
}

fn strip_brackets(text: &str) -> String {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[[^\]]*\]\]").unwrap());
    RE.replace_all(text, "").into_owned()
}

fn marked(kind: &str) -> String {
    format!("\u{E000}{kind}\u{E000}")
}

fn repeat_until_stable(text: &str, step: impl Fn(&str) -> String) -> String {
    let mut current = text.to_string();
    loop {
        let next = step(&current);
        if next == current {
            return current;
        }
        current = next;
    }
}

fn apply_span(text: &str) -> String {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?s)([ \t\n]*)\[\[span(_?)(\s[^\]]*)?\]\](.*?)\[\[/span\]\]([ \t\n]*)",
        )
        .unwrap()
    });
    repeat_until_stable(text, |current| {
        RE.replacen(current, 1, |caps: &regex::Captures| {
            format!(
                "{}{}{}{}{}",
                caps.get(1).map(|m| m.as_str()).unwrap_or(""),
                marked("S"),
                caps.get(4).map(|m| m.as_str()).unwrap_or(""),
                marked("/S"),
                caps.get(5).map(|m| m.as_str()).unwrap_or(""),
            )
        })
        .into_owned()
    })
}

fn apply_div(text: &str) -> String {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?s)(\n)?\[\[div(_?)(\s.*?)?\]\] *\n(.*?)\[\[/div\]\] *").unwrap()
    });
    repeat_until_stable(text, |current| {
        RE.replacen(current, 1, |caps: &regex::Captures| {
            let pre = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            format!(
                "{pre}{pre}{}\n\n{}\n\n{}",
                marked("D"),
                caps.get(4).map(|m| m.as_str()).unwrap_or(""),
                marked("/D"),
            )
        })
        .into_owned()
    })
}

fn apply_collapsible(text: &str) -> String {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?s)(\n)?\[\[collapsible(\s.*?)?\]\](.*?)\[\[/collapsible\]\] *")
            .unwrap()
    });
    repeat_until_stable(text, |current| {
        RE.replacen(current, 1, |caps: &regex::Captures| {
            let pre = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            format!(
                "{pre}{pre}{}\n\n{}\n\n{}",
                marked("C"),
                caps.get(3).map(|m| m.as_str()).unwrap_or(""),
                marked("/C"),
            )
        })
        .into_owned()
    })
}

fn apply_bullet_list(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if list_item_text(lines[index]).is_some() {
            let mut items = Vec::new();
            while index < lines.len() {
                if let Some(item) = list_item_text(lines[index]) {
                    items.push(item);
                    index += 1;
                    continue;
                }
                break;
            }
            out.push(format!(
                "{}{}{}",
                marked("L"),
                items.join(&marked("I")),
                marked("/L")
            ));
        } else {
            out.push(lines[index].to_string());
            index += 1;
        }
    }
    out.join("\n")
}

fn list_item_text(line: &str) -> Option<&str> {
    let trimmed = line.trim_start_matches(' ');
    trimmed
        .strip_prefix("* ")
        .or_else(|| trimmed.strip_prefix("# "))
}

fn apply_ullist(text: &str) -> String {
    static UL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?si)\[\[ul(\s[^\]]*)?\]\]\s*(.*?)\[\[/ul\]\]\n?").unwrap()
    });
    static OL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?si)\[\[ol(\s[^\]]*)?\]\]\s*(.*?)\[\[/ol\]\]\n?").unwrap()
    });
    let step = |current: &str, re: &Regex| -> String {
        re.replacen(current, 1, |caps: &regex::Captures| {
            format!(
                "{}{}{}",
                marked("U"),
                caps.get(2).map(|group| group.as_str()).unwrap_or(""),
                marked("/U"),
            )
        })
        .into_owned()
    };
    let with_ul = repeat_until_stable(text, |current| step(current, &UL));
    repeat_until_stable(&with_ul, |current| step(current, &OL))
}

fn legacy_unclosed_row(text: &str, pipe: usize) -> Option<UnclosedRow> {
    let bytes = text.as_bytes();
    if pipe.checked_add(2).is_none_or(|end| end > bytes.len())
        || &bytes[pipe..pipe + 2] != b"||"
    {
        return None;
    }
    let mut index = pipe + 2;
    let mut pending_closers = Vec::new();
    let mut saw_explicit_list = false;
    while index < bytes.len() {
        if let Some(next) = skip_score_div(text, index) {
            index = next;
            continue;
        }
        if let Some(next) = eat_ullist_opener(text, index, &mut pending_closers) {
            saw_explicit_list = true;
            index = next;
            continue;
        }
        if let Some(next) = eat_ullist_closer(text, index) {
            index = next;
            continue;
        }
        if source_list_line(text, index) {
            if !saw_explicit_list {
                return None;
            }
            let end = end_of_source_list(text, index);
            if text[index..end].contains("||") {
                return None;
            }
            return Some(unclosed_plan(end, &pending_closers));
        }
        if let Some((closer, next)) = plain_span_opener(text, index) {
            pending_closers.push(closer);
            index = next;
            continue;
        }
        if let Some(block) = cutting_block(text, index) {
            if !saw_explicit_list {
                return None;
            }
            pending_closers.push(block.closer);
            return Some(unclosed_plan(block.body, &pending_closers));
        }
        if bytes[index] == b'|' && bytes.get(index + 1) == Some(&b'|') {
            return None;
        }
        if bytes[index] == b'\n' {
            if index >= 2 && bytes[index - 2] == b' ' && bytes[index - 1] == b'_' {
                index += 1;
                continue;
            }
            if !saw_explicit_list {
                return None;
            }
            let after = index + 1;
            if text[after..].starts_with("||") {
                return None;
            }
            return Some(unclosed_plan(after, &pending_closers));
        }
        index += utf8_len(bytes[index]);
    }
    None
}

fn unclosed_plan(resume: usize, closers: &[usize]) -> UnclosedRow {
    UnclosedRow {
        resume,
        swallow: closers
            .iter()
            .copied()
            .filter(|closer| *closer >= resume)
            .collect(),
    }
}

struct CuttingBlock {
    body: usize,
    closer: usize,
}

fn utf8_len(byte: u8) -> usize {
    if byte < 0x80 {
        1
    } else if byte < 0xE0 {
        2
    } else if byte < 0xF0 {
        3
    } else {
        4
    }
}

fn bracket_inner(text: &str, index: usize) -> Option<(&str, usize)> {
    let bytes = text.as_bytes();
    if index + 4 > bytes.len() || bytes[index] != b'[' || bytes[index + 1] != b'[' {
        return None;
    }
    let close = text[index + 2..].find("]]")?;
    let inner = &text[index + 2..index + 2 + close];
    if inner.contains('\n') {
        return None;
    }
    Some((inner, index + 2 + close + 2))
}

fn tag_word(inner: &str) -> &str {
    inner.split_whitespace().next().unwrap_or(inner)
}

fn find_balanced_closer(text: &str, from: usize, name: &str) -> Option<usize> {
    let mut depth = 1u32;
    let mut index = from;
    while index < text.len() {
        if let Some((inner, end)) = bracket_inner(text, index) {
            let word = tag_word(inner);
            let closing = word.starts_with('/');
            let base = word.trim_start_matches('/').trim_end_matches('_');
            if base.eq_ignore_ascii_case(name) {
                if closing {
                    depth -= 1;
                    if depth == 0 {
                        return Some(index);
                    }
                } else {
                    depth += 1;
                }
            }
            index = end;
            continue;
        }
        index += utf8_len(text.as_bytes()[index]);
    }
    None
}

fn eat_ullist_opener(
    text: &str,
    index: usize,
    pending: &mut Vec<usize>,
) -> Option<usize> {
    let (inner, end) = bracket_inner(text, index)?;
    let word = tag_word(inner);
    if word.starts_with('/') {
        return None;
    }
    let base = word.trim_end_matches('_');
    if !matches!(base.to_ascii_lowercase().as_str(), "ul" | "ol" | "li") {
        return None;
    }
    let closer = find_balanced_closer(text, end, base)?;
    pending.push(closer);
    let bytes = text.as_bytes();
    let mut next = end;
    while next < bytes.len() && bytes[next].is_ascii_whitespace() {
        next += 1;
    }
    Some(next)
}

fn eat_ullist_closer(text: &str, index: usize) -> Option<usize> {
    let (inner, end) = bracket_inner(text, index)?;
    let word = tag_word(inner);
    let base = word.trim_start_matches('/');
    if !word.starts_with('/') {
        return None;
    }
    let bytes = text.as_bytes();
    match base.to_ascii_lowercase().as_str() {
        "li" => {
            let mut next = end;
            while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                next += 1;
            }
            Some(next)
        }
        "ul" | "ol" => {
            let next = if bytes.get(end) == Some(&b'\n') {
                end + 1
            } else {
                end
            };
            Some(next)
        }
        _ => None,
    }
}

fn plain_span_opener(text: &str, index: usize) -> Option<(usize, usize)> {
    let (inner, end) = bracket_inner(text, index)?;
    let word = tag_word(inner);
    if !word.eq_ignore_ascii_case("span") {
        return None;
    }
    let closer = find_balanced_closer(text, end, "span")?;
    Some((closer, end))
}

fn skip_score_div(text: &str, index: usize) -> Option<usize> {
    let (inner, end) = bracket_inner(text, index)?;
    let word = tag_word(inner);
    if !word.eq_ignore_ascii_case("div_") {
        return None;
    }
    let closer = find_balanced_closer(text, end, "div")?;
    let (_, after) = bracket_inner(text, closer)?;
    let bytes = text.as_bytes();
    let mut next = after;
    while next < bytes.len() && bytes[next] == b' ' {
        next += 1;
    }
    Some(next)
}

fn cutting_block(text: &str, index: usize) -> Option<CuttingBlock> {
    let (inner, end) = bracket_inner(text, index)?;
    let word = tag_word(inner);
    if word.starts_with('/') || word.eq_ignore_ascii_case("div_") {
        return None;
    }
    let bytes = text.as_bytes();
    if word.eq_ignore_ascii_case("div") {
        let mut cursor = end;
        while bytes.get(cursor) == Some(&b' ') {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'\n') {
            return None;
        }
        let closer = find_balanced_closer(text, end, "div")?;
        return Some(CuttingBlock {
            body: cursor + 1,
            closer,
        });
    }
    if word.eq_ignore_ascii_case("collapsible") {
        let closer = find_balanced_closer(text, end, "collapsible")?;
        return Some(CuttingBlock { body: end, closer });
    }
    if word.eq_ignore_ascii_case("note") {
        if bytes.get(end) != Some(&b'\n') {
            return None;
        }
        let closer = find_balanced_closer(text, end, "note")?;
        return Some(CuttingBlock {
            body: end + 1,
            closer,
        });
    }
    None
}

fn source_list_line(text: &str, index: usize) -> bool {
    if index > 0 && text.as_bytes().get(index - 1) != Some(&b'\n') {
        return false;
    }
    let bytes = text.as_bytes();
    let mut cursor = index;
    while bytes.get(cursor) == Some(&b' ') {
        cursor += 1;
    }
    matches!(
        (bytes.get(cursor), bytes.get(cursor + 1)),
        (Some(b'*') | Some(b'#'), Some(b' '))
    )
}

fn end_of_source_list(text: &str, mut index: usize) -> usize {
    while source_list_line(text, index) {
        match text[index..].find('\n') {
            Some(offset) => index += offset + 1,
            None => return text.len(),
        }
    }
    index
}

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    let preserve_break = wikidot_simple_table_preserves_break(parser);
    let success = try_consume_inner(parser);
    if !preserve_break {
        return success;
    }
    success.map(|success| {
        success.map(|elements| match elements {
            Elements::Single(table @ Element::Table(_)) => {
                Elements::Multiple(vec![Element::LineBreak, table])
            }
            elements => elements,
        })
    })
}

fn try_consume_inner<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    if parser.settings().layout.legacy() && parser.in_wikidot_literal_alias_body() {
        let literal = parser.current().slice;
        parser.step()?;
        return ok!(text!(literal));
    }

    if parser.wikidot_footnote_zone_blocks(parser.current().span.start) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if parser.settings().layout.legacy()
        && parser.wikidot_line_follows_explicit_list(parser.current().span.start)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if parser.settings().layout.legacy()
        && parser.current().token == Token::TableColumn
        && parser.current().slice == "||"
    {
        let pipe = parser.current().span.start;
        if let Some(plan) = legacy_unclosed_row(parser.full_text().inner(), pipe) {
            return finish_discarded_unclosed_row(parser, plan);
        }
        if let Some(success) = try_glued_source_list_row(parser, pipe)? {
            return Ok(success);
        }
    }

    let mut rows = Vec::new();
    let mut errors = Vec::new();
    let mut paragraph_break = false;
    let mut pending_inline_closers = 0u16;
    let mut pending_inline_scopes = PendingInlineScopes::default();
    parser.clear_wikidot_table_cut_boundary();

    loop {
        if parser.native_blockquote_depth().is_some() && !rows.is_empty() {
            return finish_simple_table(rows, errors);
        }
        let mut cells = Vec::new();
        let mut saw_open = false;

        'row: loop {
            let mut elements = Vec::new();
            let mut early_closed_spans = 0usize;
            let mut early_closed_sizes = 0usize;
            let mut early_closed_anchors = 0usize;
            let cell_start = match parse_cell_start(parser)? {
                Some(cell_start) => cell_start,
                None => return finish_table_or_fail(parser, rows, errors),
            };
            if let Some(prefix) = cell_start.literal_prefix {
                elements.push(text!(prefix));
            }

            'cell: loop {
                let current_closer =
                    Parser::wikidot_simple_table_closer_bit(parser.current().token);
                if pending_inline_closers & current_closer != 0 {
                    let (spans, sizes, anchors) =
                        close_wikidot_inline_scopes(&mut elements);
                    early_closed_spans += spans;
                    early_closed_sizes += sizes;
                    early_closed_anchors += anchors;
                    parser.step()?;
                    pending_inline_closers &= !current_closer;
                    continue;
                }

                let mut swallowed_split = false;
                for delimiter in [SplitDelimiter::Bold, SplitDelimiter::Italics] {
                    let bit = Parser::wikidot_simple_table_split_closer_bit(delimiter);
                    if pending_inline_closers & bit == 0
                        || !delimiter.is_half(parser.current())
                    {
                        continue;
                    }
                    let Some(split_match) = split_delimiter_end(
                        TokenWindow::at(parser),
                        SegmentCtx::from_parser(parser),
                        delimiter,
                    ) else {
                        continue;
                    };
                    let (spans, sizes, anchors) =
                        close_wikidot_inline_scopes(&mut elements);
                    early_closed_spans += spans;
                    early_closed_sizes += sizes;
                    early_closed_anchors += anchors;
                    while parser.current().span.start < split_match.consume_end
                        && parser.current().token != Token::InputEnd
                    {
                        parser.step()?;
                    }
                    if let Some(leftover) = &split_match.leftover {
                        let text = &parser.full_text().inner()[leftover.clone()];
                        if !text.is_empty() {
                            elements.push(text!(text));
                        }
                    }
                    pending_inline_closers &= !bit;
                    swallowed_split = true;
                    break;
                }
                if swallowed_split {
                    continue;
                }

                if !parser.discarding_hidden_body() && parser.at_body_end_boundary() {
                    if cells.is_empty() {
                        elements.clear();
                        push_cell(
                            &mut cells,
                            &mut elements,
                            TableCellStart {
                                align: None,
                                header: false,
                                column_span: NonZeroU32::new(1).unwrap(),
                                literal_prefix: None,
                            },
                            &mut pending_inline_scopes,
                        );
                    }
                    push_row(&mut rows, &mut cells);
                    return finish_simple_table(rows, errors);
                }

                match parser.next_two_tokens() {
                    (current, Some(next)) if is_table_column_token(current) => {
                        let (r, c, e) = (&mut rows, &mut cells, &mut elements);
                        let p = std::convert::identity(&mut *parser);
                        let mut state =
                            CellState::new(r, c, e, &mut pending_inline_scopes);
                        let boundary = cell_boundary(p, &mut state, cell_start, next)?;
                        let (finish_row, finish_cell) = match boundary {
                            CellBoundary::FinishTable => {
                                return finish_simple_table(rows, errors);
                            }
                            CellBoundary::FinishRow => (true, false),
                            CellBoundary::ContinueCell => (false, true),
                        };
                        match (finish_row, finish_cell) {
                            (true, _) => break 'row std::convert::identity(()),
                            (_, true) => break 'cell std::convert::identity(()),
                            _ => unreachable!("cell boundary must finish row or cell"),
                        }
                    }

                    (Token::Whitespace, _) if elements.is_empty() => {
                        parser.step()?;
                    }

                    (Token::Whitespace, Some(next)) if is_table_column_token(next) => {
                        parser.step()?;
                    }

                    (Token::LineBreak | Token::ParagraphBreak, _)
                        if elements.is_empty()
                            && parser.settings().layout.legacy()
                            && !cells.is_empty() =>
                    {
                        if parser.current().token == Token::LineBreak
                            && parser
                                .look_ahead(0)
                                .is_some_and(|token| is_table_column_token(token.token))
                        {
                            parser.step()?;
                            break 'row;
                        }
                        push_row(&mut rows, &mut cells);
                        return finish_simple_table(rows, errors);
                    }

                    (Token::LineBreak, _) if elements.is_empty() => {
                        push_cell(
                            &mut cells,
                            &mut elements,
                            cell_start,
                            &mut pending_inline_scopes,
                        );
                        parser.step()?;
                        break 'row;
                    }

                    (Token::ParagraphBreak, _) if elements.is_empty() => {
                        push_cell(
                            &mut cells,
                            &mut elements,
                            cell_start,
                            &mut pending_inline_scopes,
                        );
                        push_row(&mut rows, &mut cells);
                        parser.step()?;
                        return finish_simple_table(rows, errors);
                    }

                    (Token::InputEnd, _) if elements.is_empty() => {
                        if parser.settings().layout.legacy() && !cells.is_empty() {
                            push_row(&mut rows, &mut cells);
                            return finish_simple_table(rows, errors);
                        }
                        push_cell(
                            &mut cells,
                            &mut elements,
                            cell_start,
                            &mut pending_inline_scopes,
                        );
                        push_row(&mut rows, &mut cells);
                        return finish_simple_table(rows, errors);
                    }

                    (Token::LineBreak | Token::ParagraphBreak | Token::InputEnd, _)
                        if cells.is_empty() =>
                    {
                        if parser.current().token == Token::LineBreak
                            && parser
                                .look_ahead(0)
                                .is_some_and(|token| is_table_column_token(token.token))
                        {
                            if saw_open {
                                parser.set_wikidot_table_cut_boundary();
                                elements.clear();
                                let empty_cell_start = TableCellStart {
                                    align: None,
                                    header: false,
                                    column_span: NonZeroU32::new(1).unwrap(),
                                    literal_prefix: None,
                                };
                                push_cell(
                                    &mut cells,
                                    &mut elements,
                                    empty_cell_start,
                                    &mut pending_inline_scopes,
                                );
                                push_row(&mut rows, &mut cells);
                                return finish_simple_table(rows, errors);
                            }
                            elements.clear();
                            let empty_cell_start = TableCellStart {
                                align: None,
                                header: false,
                                column_span: NonZeroU32::new(1).unwrap(),
                                literal_prefix: None,
                            };
                            push_cell(
                                &mut cells,
                                &mut elements,
                                empty_cell_start,
                                &mut pending_inline_scopes,
                            );
                            parser.step()?;
                            break 'row;
                        }
                        if saw_open {
                            parser.set_wikidot_table_cut_boundary();
                        }
                        elements.clear();
                        let empty_cell_start = TableCellStart {
                            align: None,
                            header: false,
                            column_span: NonZeroU32::new(1).unwrap(),
                            literal_prefix: None,
                        };
                        push_cell(
                            &mut cells,
                            &mut elements,
                            empty_cell_start,
                            &mut pending_inline_scopes,
                        );
                        push_row(&mut rows, &mut cells);
                        return finish_simple_table(rows, errors);
                    }

                    (Token::LineBreak | Token::ParagraphBreak | Token::InputEnd, _) => {
                        if !cells.is_empty() {
                            if parser.current().token == Token::LineBreak
                                && parser.look_ahead(0).is_some_and(|token| {
                                    is_table_column_token(token.token)
                                })
                            {
                                if saw_open {
                                    parser.set_wikidot_table_cut_boundary();
                                    push_row(&mut rows, &mut cells);
                                    return finish_simple_table(rows, errors);
                                }
                                parser.step()?;
                                break 'row;
                            }
                            if saw_open {
                                parser.set_wikidot_table_cut_boundary();
                            }
                            push_row(&mut rows, &mut cells);
                            return finish_simple_table(rows, errors);
                        }
                        return finish_table_or_fail(parser, rows, errors);
                    }

                    _ => {
                        let wikidot = parser.settings().layout.legacy();
                        if wikidot
                            && parser.current().token == Token::LeftBlockEnd
                            && wikidot_table_closer_cuts(parser)
                        {
                            if cells.is_empty() {
                                elements.clear();
                                let empty_cell_start = TableCellStart {
                                    align: None,
                                    header: false,
                                    column_span: NonZeroU32::new(1).unwrap(),
                                    literal_prefix: None,
                                };
                                push_cell(
                                    &mut cells,
                                    &mut elements,
                                    empty_cell_start,
                                    &mut pending_inline_scopes,
                                );
                            }
                            push_row(&mut rows, &mut cells);
                            parser.set_wikidot_table_cut_boundary();
                            return finish_simple_table(rows, errors);
                        }
                        if wikidot
                            && parser.current().token == Token::LeftBlock
                            && wikidot_table_head_abandons(parser)
                        {
                            parser.set_in_wikidot_simple_table_cell(true);
                            let consumed = consume(parser);
                            parser.set_in_wikidot_simple_table_cell(false);
                            let _ = consumed?.chain(&mut errors, &mut paragraph_break);
                            elements.clear();
                            saw_open = true;
                            if cells.is_empty() {
                                let empty_cell_start = TableCellStart {
                                    align: None,
                                    header: false,
                                    column_span: NonZeroU32::new(1).unwrap(),
                                    literal_prefix: None,
                                };
                                push_cell(
                                    &mut cells,
                                    &mut elements,
                                    empty_cell_start,
                                    &mut pending_inline_scopes,
                                );
                            }
                            push_row(&mut rows, &mut cells);
                            parser.set_wikidot_table_cut_boundary();
                            return finish_simple_table(rows, errors);
                        }
                        let comment = parser.current().token == Token::LeftComment;
                        let row_block_byte = parser.current().span.start;
                        let consumed = if table_block_head_crosses_line(parser) {
                            let literal = parser.current().slice;
                            parser.step()?;
                            ok!(text!(literal))
                        } else {
                            if wikidot {
                                parser.set_in_wikidot_simple_table_cell(true);
                            }
                            let consumed = consume(parser);
                            if wikidot {
                                parser.set_in_wikidot_simple_table_cell(false);
                            }
                            consumed
                        };
                        let consumed = consumed?;
                        if wikidot {
                            pending_inline_closers |=
                                parser.take_wikidot_simple_table_crossed_closers();
                        }
                        let new_items = consumed.chain(&mut errors, &mut paragraph_break);
                        let end_byte = parser.current().span.start;
                        let new_items = if wikidot
                            && block_opens_on_simple_row(
                                parser.full_text().inner(),
                                row_block_byte,
                                end_byte,
                            ) {
                            match simple_table_row_block_interior(new_items) {
                                Ok(mut interior) => {
                                    if !parser.in_wikidot_span_body() {
                                        let suffix_at = parser.current().span.start;
                                        let suffix = take_simple_row_line_suffix(parser)?;
                                        // The opener sat in the ignored last cell, so its
                                        // end tags are unmatched. Tidy moves the same-line
                                        // suffix before the table only when a div-like
                                        // block still wraps that table.
                                        if parser.in_wikidot_advanced_table_cell_body()
                                            && !suffix.is_empty()
                                            && parser.wikidot_flat_ancestor_wraps_table(
                                                suffix_at,
                                            )
                                        {
                                            interior.push(Element::Partial(
                                                PartialElement::WikidotFoster(suffix),
                                            ));
                                        } else {
                                            if !suffix.is_empty() {
                                                interior.extend(suffix);
                                            }
                                            if interior.iter().any(element_is_row_suffix) {
                                                parser.set_wikidot_row_suffix_at(suffix_at);
                                            }
                                        }
                                    }
                                    elements.clear();
                                    push_cell(
                                        &mut cells,
                                        &mut elements,
                                        cell_start,
                                        &mut pending_inline_scopes,
                                    );
                                    push_row(&mut rows, &mut cells);
                                    parser.set_wikidot_table_cut_boundary();
                                    return finish_simple_table_then(
                                        rows, interior, errors,
                                    );
                                }
                                Err(items) => items,
                            }
                        } else {
                            new_items
                        };
                        if wikidot && !saw_open && cell_holds_flat_open(&new_items) {
                            saw_open = true;
                            // L8.5: a cross-line pipe-head pair renders its
                            // interior as normal siblings. Finish the prefix
                            // row now (the parser sits right after the head)
                            // so the head-line suffix and body parse outside
                            // the table. Same-line pairs keep legacy flow.
                            if let Some(head_byte) = flat_open_head_byte(&new_items)
                                && let Some(pair) =
                                    parser.wikidot_block_pair_at_head_byte(head_byte)
                                && pair.head_byte < pair.closer_byte
                                && parser.full_text().inner()
                                    [pair.head_byte..pair.closer_byte]
                                    .contains('\n')
                            {
                                if cells.is_empty() {
                                    elements.clear();
                                    let empty_cell_start = TableCellStart {
                                        align: None,
                                        header: false,
                                        column_span: NonZeroU32::new(1).unwrap(),
                                        literal_prefix: None,
                                    };
                                    push_cell(
                                        &mut cells,
                                        &mut elements,
                                        empty_cell_start,
                                        &mut pending_inline_scopes,
                                    );
                                }
                                push_row(&mut rows, &mut cells);
                                if parser.current().token == Token::LineBreak {
                                    parser.step()?;
                                }
                                parser.set_wikidot_table_cut_boundary();
                                return finish_simple_table(rows, errors);
                            }
                        }

                        if comment
                            && comment_left_nothing(&new_items)
                            && matches!(
                                parser.current().token,
                                Token::Whitespace
                                    | Token::TableColumn
                                    | Token::TableColumnTitle
                                    | Token::TableColumnCenter
                                    | Token::TableColumnRight
                                    | Token::LineBreak
                                    | Token::ParagraphBreak
                                    | Token::InputEnd
                            )
                        {
                            trim_one_trailing_ascii_space(&mut elements);
                        }

                        let elements_before = elements.len();
                        append_cell_elements(&mut elements, new_items);
                        swallow_early_closed_scopes(
                            &mut elements,
                            elements_before,
                            &mut early_closed_spans,
                            &mut early_closed_sizes,
                            &mut early_closed_anchors,
                        );
                    }
                }
            }

            push_cell(
                &mut cells,
                &mut elements,
                cell_start,
                &mut pending_inline_scopes,
            );
        }

        push_row(&mut rows, &mut cells);
    }
}

fn wikidot_table_closer_cuts(parser: &Parser<'_, '_>) -> bool {
    let byte = parser.current().span.start;
    let Some(pair) = parser.wikidot_block_pair_at_closer_byte(byte) else {
        return false;
    };
    if !pair.kind.emits_flat() {
        return false;
    }
    parser.wikidot_head_on_table_row_line(pair.head_byte)
}

fn wikidot_table_head_abandons(parser: &Parser<'_, '_>) -> bool {
    let byte = parser.current().span.start;
    let Some(pair) = parser.wikidot_block_pair_at_head_byte(byte) else {
        return false;
    };
    if !pair.kind.emits_flat() {
        return false;
    }
    let mut depth = 1u32;
    let mut index = 0usize;
    loop {
        let Some(token) = parser.look_ahead(index) else {
            return false;
        };
        index += 1;
        match token.token {
            Token::LeftBlock | Token::LeftBlockEnd => depth += 1,
            Token::RightBlock => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => return false,
            _ => {}
        }
    }
    loop {
        let Some(token) = parser.look_ahead(index) else {
            return false;
        };
        index += 1;
        match token.token {
            Token::TableColumn
            | Token::TableColumnTitle
            | Token::TableColumnCenter
            | Token::TableColumnRight => return true,
            Token::LeftBlockEnd => {
                if parser
                    .wikidot_block_pair_at_closer_byte(token.span.start)
                    .is_some()
                {
                    return true;
                }
            }
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => return false,
            _ => {}
        }
    }
}

fn flat_open_head_byte(elements: &Elements<'_>) -> Option<usize> {
    let head = |element: &Element| match element {
        Element::Partial(PartialElement::WikidotBlockOpen(shell)) => {
            Some(shell.head_byte())
        }
        _ => None,
    };
    match elements {
        Elements::None => None,
        Elements::Single(element) => head(element),
        Elements::Multiple(items) => items.iter().find_map(head),
    }
}

fn cell_holds_flat_open(elements: &Elements<'_>) -> bool {
    let holds = |element: &Element| {
        matches!(
            element,
            Element::Partial(PartialElement::WikidotBlockOpen(_))
        )
    };
    match elements {
        Elements::None => false,
        Elements::Single(element) => holds(element),
        Elements::Multiple(items) => items.iter().any(holds),
    }
}

fn comment_left_nothing(elements: &Elements<'_>) -> bool {
    match elements {
        Elements::None => true,
        Elements::Single(Element::Delayed(delayed)) => delayed.is_empty_suppression(),
        _ => false,
    }
}

fn trim_one_trailing_ascii_space(elements: &mut Vec<Element<'_>>) {
    let Some(Element::Text(text)) = elements.last_mut() else {
        return;
    };
    if text.ends_with(' ') {
        text.to_mut().pop();
        if text.is_empty() {
            elements.pop();
        }
    }
}

/// Decide whether a `||=` / `||~` / `||>` marker applies.
///
/// Live Wikidot only treats the marker as an alignment or header marker when
/// a space or tab follows it. At the end of a line (or input) the marker is
/// dead and the cell is plain and empty; glued to other text the marker
/// character itself is literal text and the cell is plain. Returns `None`
/// when a space follows (existing handling applies), `Some(None)` for the
/// dead end-of-line marker, and `Some(Some(marker))` for the glued marker.
fn wikidot_cell_marker_without_space<'t>(
    parser: &Parser<'_, 't>,
    marker: &'t str,
) -> Option<Option<&'t str>> {
    if matches!(parser.current().token, Token::Whitespace) {
        let mut lookahead = parser.clone();
        while lookahead.current().token == Token::Whitespace {
            if lookahead.step().is_err() {
                break;
            }
        }
        if matches!(
            lookahead.current().token,
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
        ) {
            return Some(None);
        }
        return None;
    }
    if matches!(
        parser.current().token,
        Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
    ) {
        return Some(None);
    }
    Some(Some(marker))
}

fn parse_cell_start<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<TableCellStart<'t>>, ParseError> {
    let mut span = 0;
    let mut literal_prefix = None;

    macro_rules! increase_span {
        () => {{
            span += 1;
            parser.step()?;
        }};
    }

    let (align, header) = loop {
        match parser.current().token {
            Token::TableColumnTitle => {
                let marker_start = parser.current().span.end - 1;
                increase_span!();
                if parser.settings().layout.legacy()
                    && let Some(plain) = wikidot_cell_marker_without_space(parser, "~")
                {
                    if plain.is_some() {
                        // A marker glued to more markers is one literal run.
                        literal_prefix = Some(
                            take_repeated_cell_marker(parser, marker_start, b'~')?
                                .unwrap_or("~"),
                        );
                    }
                    break (None, false);
                }
                if parser.settings().layout.legacy()
                    && let Some(prefix) =
                        take_repeated_cell_marker(parser, marker_start, b'~')?
                {
                    literal_prefix = Some(prefix);
                    break (None, false);
                }
                if parser.settings().layout.legacy()
                    && matches!(parser.current().slice, ">" | "=")
                {
                    literal_prefix = Some("~");
                    break (None, false);
                }
                break (None, true);
            }
            Token::TableColumnCenter => {
                let marker_start = parser.current().span.end - 1;
                increase_span!();
                if parser.settings().layout.legacy()
                    && let Some(plain) = wikidot_cell_marker_without_space(parser, "=")
                {
                    if plain.is_some() {
                        // A marker glued to more markers is one literal run.
                        literal_prefix = Some(
                            take_repeated_cell_marker(parser, marker_start, b'=')?
                                .unwrap_or("="),
                        );
                    }
                    break (None, false);
                }
                if parser.settings().layout.legacy()
                    && let Some(prefix) =
                        take_repeated_cell_marker(parser, marker_start, b'=')?
                {
                    literal_prefix = Some(prefix);
                    break (None, false);
                }
                if parser.settings().layout.legacy() && parser.current().slice == "~" {
                    literal_prefix = Some("=");
                    break (None, false);
                }
                break (Some(Alignment::Center), false);
            }
            Token::TableColumnRight => {
                let marker_start = parser.current().span.end - 1;
                increase_span!();
                if parser.settings().layout.legacy()
                    && let Some(plain) = wikidot_cell_marker_without_space(parser, ">")
                {
                    if plain.is_some() {
                        // A marker glued to more markers is one literal run.
                        literal_prefix = Some(
                            take_repeated_cell_marker(parser, marker_start, b'>')?
                                .unwrap_or(">"),
                        );
                    }
                    break (None, false);
                }
                if parser.settings().layout.legacy() && parser.current().slice == "~" {
                    literal_prefix = Some(">");
                    break (None, false);
                }
                break (Some(Alignment::Right), false);
            }

            Token::TableColumn => increase_span!(),

            _ if span > 0
                && parser.settings().layout.legacy()
                && parser.current().slice == "<"
                && {
                    let mut lookahead = parser.clone();
                    lookahead.step().is_ok()
                        && lookahead.current().token == Token::Whitespace
                } =>
            {
                parser.step()?;
                break (Some(Alignment::Left), false);
            }
            _ if span > 0 => break (None, false),

            _ => return Ok(None),
        }
    };

    if parser.settings().layout.legacy()
        && span > 1
        && parser.current().token == Token::InputEnd
    {
        span = 1;
    }
    let column_span =
        NonZeroU32::new(span).expect("Cell start exited without column span");

    Ok(Some(TableCellStart {
        align,
        header,
        column_span,
        literal_prefix,
    }))
}

fn take_repeated_cell_marker<'r, 't>(
    parser: &mut Parser<'r, 't>,
    marker_start: usize,
    marker: u8,
) -> Result<Option<&'t str>, ParseError> {
    let mut marker_end = marker_start + 1;
    let mut repeated = false;
    while !parser.current().slice.is_empty()
        && parser.current().slice.bytes().all(|byte| byte == marker)
    {
        repeated = true;
        marker_end = parser.current().span.end;
        parser.step()?;
    }
    if repeated {
        Ok(Some(&parser.full_text().inner()[marker_start..marker_end]))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    fn with_parser<R>(
        input: &str,
        page_info: &PageInfo<'_>,
        settings: &WikitextSettings,
        run: impl FnOnce(&mut Parser<'_, '_>) -> R,
    ) -> R {
        let tokenization = crate::tokenize(input);
        let mut parser = Parser::new(&tokenization, page_info, settings);
        parser
            .step()
            .expect("first token should follow input start");
        parser.set_rule(RULE_TABLE);
        run(&mut parser)
    }

    #[test]
    fn table_cell_start_parses_alignment_headers_and_colspan() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        with_parser("|||| body", &page_info, &settings, |parser| {
            let start = parse_cell_start(parser)
                .expect("cell start should parse")
                .expect("colspan should produce a cell start");
            assert_eq!(start.align, None);
            assert!(!start.header);
            assert_eq!(start.column_span.get(), 2);
        });

        with_parser("||~ body", &page_info, &settings, |parser| {
            let start = parse_cell_start(parser)
                .expect("header cell start should parse")
                .expect("header should produce a cell start");
            assert!(start.header);
            assert_eq!(start.align, None);
            assert_eq!(start.column_span.get(), 1);
        });

        with_parser("||= body", &page_info, &settings, |parser| {
            let start = parse_cell_start(parser)
                .expect("center cell start should parse")
                .expect("center marker should produce a cell start");
            assert_eq!(start.align, Some(Alignment::Center));
            assert!(!start.header);
        });

        with_parser("||> body", &page_info, &settings, |parser| {
            let start = parse_cell_start(parser)
                .expect("right cell start should parse")
                .expect("right marker should produce a cell start");
            assert_eq!(start.align, Some(Alignment::Right));
            assert!(!start.header);
        });

        with_parser("plain", &page_info, &settings, |parser| {
            assert!(
                parse_cell_start(parser)
                    .expect("invalid start is not a parser error")
                    .is_none()
            );
        });
    }

    #[test]
    fn table_cell_boundary_updates_rows_and_cells() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let cell_start = TableCellStart {
            align: None,
            header: false,
            column_span: NonZeroU32::new(1).unwrap(),
            literal_prefix: None,
        };

        with_parser("||", &page_info, &settings, |parser| {
            let mut rows = Vec::new();
            let mut cells = Vec::new();
            let mut elements = Vec::new();
            let mut pending_scopes = PendingInlineScopes::default();
            let mut state =
                CellState::new(&mut rows, &mut cells, &mut elements, &mut pending_scopes);
            let boundary = cell_boundary(parser, &mut state, cell_start, Token::InputEnd)
                .expect("input end should finish table");
            assert!(matches!(boundary, CellBoundary::FinishTable));
            assert_eq!(state.rows.len(), 1);
            assert!(state.cells.is_empty());
        });

        with_parser("||\n||", &page_info, &settings, |parser| {
            let mut rows = Vec::new();
            let mut cells = Vec::new();
            let mut elements = Vec::new();
            let mut pending_scopes = PendingInlineScopes::default();
            let mut state =
                CellState::new(&mut rows, &mut cells, &mut elements, &mut pending_scopes);
            let boundary =
                cell_boundary(parser, &mut state, cell_start, Token::LineBreak)
                    .expect("line break should finish row");
            assert!(matches!(boundary, CellBoundary::FinishRow));
            assert!(state.rows.is_empty());
            assert_eq!(state.cells.len(), 1);
        });

        with_parser("|| next", &page_info, &settings, |parser| {
            let mut rows = Vec::new();
            let mut cells = Vec::new();
            let mut elements = Vec::new();
            let mut pending_scopes = PendingInlineScopes::default();
            let mut state =
                CellState::new(&mut rows, &mut cells, &mut elements, &mut pending_scopes);
            let boundary =
                cell_boundary(parser, &mut state, cell_start, Token::Whitespace)
                    .expect("ordinary whitespace should continue cell");
            assert!(matches!(boundary, CellBoundary::ContinueCell));
            assert!(state.rows.is_empty());
            assert!(state.cells.is_empty());
        });
    }

    #[test]
    fn simple_table_consume_loop_finishes_rows_and_cells() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        let tokenization = crate::tokenize("|| A || B ||\n|| C || D ||");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("first token should follow input start");
        parser.set_rule(RULE_TABLE);

        let parsed = try_consume_fn(&mut parser).expect("simple table should parse");
        let (elements, errors, paragraph_safe) = parsed.into();

        assert!(errors.is_empty(), "{errors:?}");
        assert!(!paragraph_safe);
        let Elements::Single(Element::Table(table)) = elements else {
            panic!("expected one simple table, got {elements:?}");
        };
        assert_eq!(table.table_type, TableType::Simple);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0].cells.len(), 2);
        assert_eq!(table.rows[1].cells.len(), 2);
    }

    #[test]
    fn wikidot_explicit_li_body_keeps_simple_table_syntax_literal() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = concat!(
            "[[div]]\n",
            "[[note]]\n",
            "[[table]]\n",
            "[[row]]\n",
            "[[cell]]\n",
            "[[ul]]\n",
            "[[li]]\n",
            "||c||\n",
            "[[/li]]\n",
            "[[/ul]]\n",
            "[[/cell]]\n",
            "[[/row]]\n",
            "[[/table]]\n",
            "[[/note]]\n",
            "[[/div]]",
        );
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(
            html.contains("||c||"),
            "expected literal cell markers, got {html:?}",
        );
        assert!(
            !html.contains("wiki-content-table"),
            "simple table must not form inside explicit [[li]] line-owned body: {html}",
        );
    }

    #[test]
    fn wikidot_empty_simple_table_rows_render_empty_cells() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for (source, expected_rows) in [("||", 1), ("||\n||", 2)] {
            let tokenization = crate::tokenize(source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;

            assert!(errors.is_empty(), "{source:?}: {errors:#?}");
            assert_eq!(html.matches("<tr>").count(), expected_rows, "{html}");
            assert_eq!(html.matches("<td></td>").count(), expected_rows, "{html}");
        }
    }

    #[test]
    fn discarded_row_footnote_break_stays_outside_the_paragraph() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = concat!(
            "||[[ul]]\n",
            "[[li]]\n",
            "[[div]]\n",
            "n[[footnote]]x[[/footnote]]\n",
            "[[/div]]\n",
            "[[/li]]\n",
            "[[/ul]]||\n",
            "[[footnoteblock]]",
        );
        let tokenization = crate::tokenize(source);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(
            html.contains("</sup></p><br>\n||"),
            "break and row marker stay outside the footnote paragraph: {html}",
        );
        assert!(
            !html.contains("</sup><br>"),
            "break must not stay inside the footnote paragraph: {html}",
        );
        let ordinary = "n[[footnote]]x[[/footnote]]\ny\n[[footnoteblock]]";
        let tokenization = crate::tokenize(ordinary);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(
            html.contains("</sup><br>\ny</p>"),
            "an ordinary footnote newline stays inside the paragraph: {html}",
        );
    }

    #[test]
    fn wikidot_unclosed_simple_table_rows_render_one_empty_cell() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for source in [
            "|| next",
            "||> body",
            "|| Missing end",
            "||= body",
            "||~ body",
            "|||| body",
        ] {
            let tokenization = crate::tokenize(source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;

            assert!(errors.is_empty(), "{source:?}: {errors:#?}");
            assert_eq!(
                html,
                "<table class=\"wiki-content-table\">\n<tr>\n<td></td>\n</tr>\n</table>",
                "{source:?}",
            );
        }
    }

    #[test]
    fn wikidot_simple_table_left_alignment_marker_sets_inline_style() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("||< left ||");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            "<table class=\"wiki-content-table\">\n<tr>\n<td style=\"text-align: left;\">left</td>\n</tr>\n</table>",
        );
    }

    #[test]
    fn wikidot_simple_table_less_than_word_remains_cell_text() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("||<div>||");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            "<table class=\"wiki-content-table\">\n<tr>\n<td>&lt;div&gt;</td>\n</tr>\n</table>",
        );
    }

    #[test]
    fn wikidot_combined_header_alignment_markers_remain_cell_text() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = concat!(
            "||~> header right ||\n",
            "||~= header center ||\n",
            "||>~ right header ||\n",
            "||=~ center header ||",
        );
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            concat!(
                "<table class=\"wiki-content-table\">\n",
                "<tr>\n<td>~&gt; header right</td>\n</tr>\n",
                "<tr>\n<td>~= header center</td>\n</tr>\n",
                "<tr>\n<td>&gt;~ right header</td>\n</tr>\n",
                "<tr>\n<td>=~ center header</td>\n</tr>\n",
                "</table>",
            ),
        );
    }

    #[test]
    fn wikidot_discards_incomplete_trailing_simple_cells() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        for (source, expected) in [
            ("|| Another || missing end", "Another"),
            ("|| durian ||||", "durian"),
        ] {
            let tokenization = crate::tokenize(source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;

            assert!(errors.is_empty(), "{source:?}: {errors:#?}");
            assert_eq!(
                html,
                format!(
                    "<table class=\"wiki-content-table\">\n<tr>\n<td>{expected}</td>\n</tr>\n</table>"
                ),
                "{source:?}",
            );
        }
    }

    #[test]
    fn table_cell_elements_adopt_first_multiple_result() {
        let mut all_elements = Vec::new();
        append_cell_elements(
            &mut all_elements,
            Elements::Multiple(vec![text!("a"), text!("b")]),
        );
        let capacity = all_elements.capacity();

        append_cell_elements(&mut all_elements, Elements::Single(text!("c")));

        assert_eq!(all_elements, vec![text!("a"), text!("b"), text!("c")]);
        assert!(all_elements.capacity() >= capacity);
    }
}
