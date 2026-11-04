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
use crate::tree::{Alignment, PartialElement, Table, TableCell, TableRow, TableType};
use std::borrow::Cow;
use std::mem;
use std::num::NonZeroU32;

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
}

fn clip_wikidot_inline_scopes_at_cell_boundary<'t>(
    elements: &mut Vec<Element<'t>>,
    pending: &mut PendingInlineScopes,
) {
    let mut spans = 0usize;
    let mut sizes = 0usize;
    elements.retain(|element| match element {
        Element::Partial(PartialElement::InlineSpanOpen(_)) => {
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
        Element::Partial(PartialElement::InlineSizeOpen(_)) => {
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
    pending.spans += spans;
    pending.sizes += sizes;
    let mut nested_spans = 0usize;
    let mut nested_sizes = 0usize;
    for element in elements.iter_mut() {
        if let Element::Container(container) = element {
            let (spans, sizes) = close_nested_inline_scopes(container.elements_mut());
            nested_spans += spans;
            nested_sizes += sizes;
        }
    }
    pending.spans += nested_spans;
    pending.sizes += nested_sizes;
}

fn close_wikidot_inline_scopes<'t>(elements: &mut Vec<Element<'t>>) -> (usize, usize) {
    let mut spans = 0usize;
    let mut sizes = 0usize;
    for element in elements.iter() {
        match element {
            Element::Partial(PartialElement::InlineSpanOpen(_)) => spans += 1,
            Element::Partial(PartialElement::InlineSpanClose(_)) => {
                spans = spans.saturating_sub(1)
            }
            Element::Partial(PartialElement::InlineSizeOpen(_)) => sizes += 1,
            Element::Partial(PartialElement::InlineSizeClose(_)) => {
                sizes = sizes.saturating_sub(1)
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
    (spans, sizes)
}

fn swallow_early_closed_scopes<'t>(
    elements: &mut Vec<Element<'t>>,
    start: usize,
    spans: &mut usize,
    sizes: &mut usize,
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
            _ => false,
        };
        if drop {
            elements.remove(index);
        } else {
            index += 1;
        }
    }
}

fn close_nested_inline_scopes<'t>(elements: &mut Vec<Element<'t>>) -> (usize, usize) {
    let mut closed_spans = 0usize;
    let mut closed_sizes = 0usize;
    for element in elements.iter_mut() {
        if let Element::Container(container) = element {
            let (spans, sizes) = close_nested_inline_scopes(container.elements_mut());
            closed_spans += spans;
            closed_sizes += sizes;
        }
    }
    let mut spans = 0usize;
    let mut sizes = 0usize;
    for element in elements.iter() {
        match element {
            Element::Partial(PartialElement::InlineSpanOpen(_)) => spans += 1,
            Element::Partial(PartialElement::InlineSpanClose(_)) => {
                spans = spans.saturating_sub(1)
            }
            Element::Partial(PartialElement::InlineSizeOpen(_)) => sizes += 1,
            Element::Partial(PartialElement::InlineSizeClose(_)) => {
                sizes = sizes.saturating_sub(1)
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
    (closed_spans + spans, closed_sizes + sizes)
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

fn try_consume_fn<'r, 't>(
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

    let mut rows = Vec::new();
    let mut errors = Vec::new();
    let mut paragraph_break = false;
    let mut pending_inline_closers = 0u16;
    let mut pending_inline_scopes = PendingInlineScopes::default();

    loop {
        if parser.native_blockquote_depth().is_some() && !rows.is_empty() {
            return finish_simple_table(rows, errors);
        }
        let mut cells = Vec::new();

        'row: loop {
            let mut elements = Vec::new();
            let mut early_closed_spans = 0usize;
            let mut early_closed_sizes = 0usize;
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
                    let (spans, sizes) = close_wikidot_inline_scopes(&mut elements);
                    early_closed_spans += spans;
                    early_closed_sizes += sizes;
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
                    let (spans, sizes) = close_wikidot_inline_scopes(&mut elements);
                    early_closed_spans += spans;
                    early_closed_sizes += sizes;
                    while parser.current().span.start < split_match.consume_end
                        && parser.current().token != Token::InputEnd
                    {
                        parser.step()?;
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
                                parser.step()?;
                                break 'row;
                            }
                            push_row(&mut rows, &mut cells);
                            return finish_simple_table(rows, errors);
                        }
                        return finish_table_or_fail(parser, rows, errors);
                    }

                    _ => {
                        let wikidot = parser.settings().layout.legacy();
                        let comment = parser.current().token == Token::LeftComment;
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
                increase_span!();
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
