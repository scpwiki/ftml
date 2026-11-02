/*
 * parsing/rule/impls/blockquote.rs
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

use super::footnote_first::{
    remove_first_footnote, starts_footnote, unowned_footnote_first,
};
use super::line_owner::{LineOwner, try_consume_lost_owner_block};
use super::prelude::*;
use crate::parsing::paragraph::{
    ParagraphStack, collapsible_has_direct_literal_nested_opener,
};
use crate::parsing::parser::QuoteBodyLineStatus;
use crate::parsing::{DepthItem, DepthList, process_depths};
use crate::tree::{AttributeMap, Container, ContainerType};

const MAX_BLOCKQUOTE_DEPTH: usize = 128;

#[derive(Debug)]
struct NativeQuoteRow<'t> {
    elements: Vec<Element<'t>>,
    paragraph_safe: bool,
    empty_spaced: bool,
}

fn consume_deeper_collapsible_close<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<bool, ParseError>
where
    'r: 't,
{
    if !parser.settings().layout.legacy()
        || !parser.in_wikidot_collapsible()
        || parser.native_blockquote_depth().is_none()
        || parser.quote_body_cursor().is_none()
        || parser.current().token != Token::Quote
    {
        return Ok(false);
    }

    let mut close = parser.clone();
    while close.current().token == Token::Quote {
        close.step()?;
        close.get_optional_space()?;
    }
    if !close
        .get_end_block()
        .is_ok_and(|name| name.eq_ignore_ascii_case("collapsible"))
    {
        return Ok(false);
    }
    close.get_optional_space()?;
    if !matches!(
        close.current().token,
        Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
    ) {
        return Ok(false);
    }

    parser.update(&close);
    parser.set_wikidot_collapsible_closed_at_deeper_quote(true);
    Ok(true)
}

fn consumed_deeper_collapsible_close(
    parser: &Parser<'_, '_>,
    start: usize,
    end: usize,
    required_depth: usize,
) -> bool {
    parser.full_text().inner()[start..end].lines().any(|line| {
        let quoted = line.bytes().take_while(|byte| *byte == b'>').count();
        quoted > required_depth
            && line[quoted..]
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("[[/collapsible]]")
    })
}

fn consumed_unquoted_collapsible_close(
    parser: &Parser<'_, '_>,
    start: usize,
    end: usize,
) -> bool {
    parser.full_text().inner()[start..end].lines().any(|line| {
        line.trim_start()
            .to_ascii_lowercase()
            .starts_with("[[/collapsible]]")
            && !line.trim_start().starts_with('>')
    })
}

pub const RULE_BLOCKQUOTE: Rule = Rule {
    name: "blockquote",
    position: LineRequirement::StartOfLine,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    if consume_deeper_collapsible_close(parser)? {
        return ok!(false; Elements::None);
    }

    let mut depths = Vec::new();
    let mut escaped_rows = Vec::new();
    let mut escaped_elements = Vec::new();
    let mut errors = Vec::new();
    let mut consumed_pruned_row = false;
    let mut quote_run_active = true;
    let mut flatten_following_rows = false;
    let mut append_unquoted_close_break = false;

    while quote_run_active
        && parser.prepare_quote_body_line()? != QuoteBodyLineStatus::Boundary
        && parser.current().token == Token::Quote
    {
        if consume_deeper_collapsible_close(parser)? {
            break;
        }
        let current = parser.current();

        let physical_depth = current.slice.len();
        let (depth, absolute_depth) = parser.native_blockquote_depths(physical_depth);
        debug_assert!(depth > 0, "residual quote depth must be positive");
        parser.step()?;
        // Wikidot distinguishes an empty `> ` row, which separates quoted
        // paragraphs, from an empty `>` row, which has no rendering effect.
        let single_space_after_marker = parser.current().slice == " ";
        let spaced_after_marker = parser.current().token == Token::Whitespace;
        parser.get_optional_space()?;
        if parser.current().token != Token::Quote {
            // Wikidot only counts contiguous quote markers toward native depth.
            // A marker after horizontal space is literal quoted content.
            parser.mark_virtual_start_of_line();
        }
        let content_start = parser.current().span.start;

        if absolute_depth > MAX_BLOCKQUOTE_DEPTH {
            return Err(parser.make_err(ParseErrorKind::BlockquoteDepthExceeded));
        }

        if parser.settings().layout.legacy()
            && let Some(lost) = try_consume_lost_owner_block(
                parser,
                LineOwner::Quote {
                    depth: physical_depth,
                },
            )?
        {
            errors.extend(lost.errors);
            if let Some(control) = lost.control {
                depths.push((
                    depth - 1,
                    (),
                    NativeQuoteRow {
                        elements: vec![control],
                        paragraph_safe: false,
                        empty_spaced: false,
                    },
                ));
            } else {
                consumed_pruned_row = true;
            }
            escaped_elements.extend(lost.body);
            if lost.append_unquoted_close_break {
                escaped_elements.push(Element::LineBreak);
            }
            break;
        }

        let close_conditions = [
            ParseCondition::current(Token::LineBreak),
            ParseCondition::current(Token::ParagraphBreak),
            ParseCondition::current(Token::InputEnd),
        ];
        let close = &close_conditions;
        let mut paragraph_safe = true;
        let original_depth = parser.native_blockquote_depth();
        let (physical_line_end, ends_quote_run) = std::iter::once(parser.current())
            .chain(parser.remaining().iter())
            .find(|token| {
                matches!(
                    token.token,
                    Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
                )
            })
            .map(|token| (token.span.end, token.token == Token::ParagraphBreak))
            .expect("tokenization always ends with input-end");
        parser.set_native_blockquote_depth(Some(absolute_depth));
        let result = collect_native_blockquote_line(parser, close);
        parser.set_native_blockquote_depth(original_depth);
        let errors_before = errors.len();
        let mut elements = result?.chain(&mut errors, &mut paragraph_safe);
        if parser.settings().layout.legacy()
            && unowned_footnote_first(parser, content_start, physical_line_end, &elements)
        {
            let removed = remove_first_footnote(&mut elements);
            debug_assert!(removed, "footnote-first classification found no footnote");
        }

        let stopped_at_footnote_close = wikidot_footnote_close_ahead(parser);
        if stopped_at_footnote_close && elements.is_empty() {
            consumed_pruned_row = true;
            continue;
        }

        let stopped_at_div_close = wikidot_div_close_ahead(parser);
        if stopped_at_div_close && elements.is_empty() {
            consumed_pruned_row = true;
            continue;
        }

        quote_run_active =
            !ends_quote_run && !stopped_at_footnote_close && !stopped_at_div_close;

        // A multiline inline child can finish on a later quoted row. Wikidot keeps the next quoted row in the same native blockquote after that child's trailing line break.
        let row_is_empty = elements.is_empty() && errors.len() == errors_before;
        let mut absorbed_unquoted_footnote = false;
        if parser.settings().layout.legacy() && starts_footnote(parser) {
            let mut candidate = parser.clone_with_rule(RULE_BLOCKQUOTE);
            if let Ok(footnote) = super::RULE_BLOCK.try_consume(&mut candidate)
                && let Elements::Single(Element::Footnote(index)) = footnote.item
            {
                errors.extend(footnote.errors);
                parser.update(&candidate);
                if row_is_empty {
                    consumed_pruned_row = true;
                    continue;
                }
                elements.push(Element::Footnote(index));
                absorbed_unquoted_footnote = true;
            }
        }
        let consumed_past_line = parser.current().span.start > physical_line_end;
        let escaped_after_deeper_close = parser.settings().layout.legacy()
            && consumed_past_line
            && matches!(elements.first(), Some(Element::Collapsible { .. }))
            && consumed_deeper_collapsible_close(
                parser,
                physical_line_end,
                parser.current().span.start,
                absolute_depth,
            );
        append_unquoted_close_break |= parser.settings().layout.legacy()
            && consumed_past_line
            && matches!(elements.first(), Some(Element::Collapsible { .. }))
            && consumed_unquoted_collapsible_close(
                parser,
                physical_line_end,
                parser.current().span.start,
            );
        if parser.settings().layout.legacy()
            && consumed_past_line
            && parser.current().token == Token::LineBreak
        {
            parser.step()?;
        } else if parser.settings().layout.legacy()
            && consumed_past_line
            && parser.current().token == Token::ParagraphBreak
        {
            quote_run_active = false;
        }
        let empty_spaced_row =
            row_is_empty && spaced_after_marker && single_space_after_marker;
        if row_is_empty && (consumed_past_line || !spaced_after_marker) {
            consumed_pruned_row = true;
            continue;
        }

        let alignment_block = elements.iter().any(|element| {
            matches!(
                element,
                Element::Container(container)
                    if matches!(container.ctype(), ContainerType::Align(_))
            )
        });
        let collapsible_row =
            matches!(elements.first(), Some(Element::Collapsible { .. }));

        let keep_line_break = if parser.settings().layout.legacy() {
            paragraph_safe || alignment_block || collapsible_row
        } else {
            !consumed_past_line || paragraph_safe
        };

        if !empty_spaced_row
            && keep_line_break
            && !absorbed_unquoted_footnote
            && !parser.pending_wikidot_collapsible_closer()
        {
            elements.push(Element::LineBreak);
        }

        let row = NativeQuoteRow {
            elements,
            paragraph_safe,
            empty_spaced: empty_spaced_row,
        };
        if flatten_following_rows {
            escaped_rows.push(row);
        } else {
            depths.push((depth - 1, (), row));
        }
        flatten_following_rows |= escaped_after_deeper_close;
    }

    if depths.is_empty() && escaped_elements.is_empty() {
        if consumed_pruned_row {
            return ok!(false; Elements::None, errors);
        }
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let depth_lists = process_depths((), depths);
    let wikidot = parser.settings().layout.legacy();
    let mut elements: Vec<Element> = depth_lists
        .into_iter()
        .filter_map(|(_, depth_list)| build_blockquote_element(depth_list, wikidot))
        .collect();
    if !escaped_rows.is_empty() {
        elements.extend(build_flattened_quote_rows(escaped_rows, wikidot));
    }
    elements.extend(escaped_elements);
    if append_unquoted_close_break {
        elements.push(Element::LineBreak);
    }

    ok!(false; elements, errors)
}

/// Whether the parser sits at a `[[/footnote]]` end block that belongs to
/// an enclosing footnote body.
///
/// In the Wikidot layout a footnote body may span quote lines, and its
/// closer may sit on one of those lines. The quote line's own collection
/// must stop there so the enclosing footnote's end-block check can see
/// the closer; consuming it as quote content loses the footnote, and the
/// failed body gather then degrades the rest of the page.
fn wikidot_footnote_close_ahead<'r, 't>(parser: &Parser<'r, 't>) -> bool
where
    'r: 't,
{
    parser.in_footnote() && wikidot_named_close_ahead(parser, "footnote")
}

/// Whether the parser sits at a `[[/div]]` end block that belongs to an
/// enclosing div body.
///
/// In the Wikidot layout a div body may span quote lines, and its closer
/// may sit on one of those lines. The quote line's own collection must
/// stop there so the enclosing div's end-block check can see the closer;
/// consuming it as quote content loses the div, which then closes only
/// at the end of the input and pulls everything after the closer into
/// the div.
fn wikidot_div_close_ahead<'r, 't>(parser: &Parser<'r, 't>) -> bool
where
    'r: 't,
{
    parser.in_wikidot_div_body() && wikidot_named_close_ahead(parser, "div")
}

/// Whether the parser sits at an end block with the given name, in the
/// Wikidot layout.
fn wikidot_named_close_ahead<'r, 't>(parser: &Parser<'r, 't>, name: &str) -> bool
where
    'r: 't,
{
    if !parser.settings().layout.legacy() {
        return false;
    }
    if parser.current().token != Token::LeftBlockEnd {
        return false;
    }

    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let Some(close_offset) = source[start..].find("]]") else {
        return false;
    };
    if source[start..start + close_offset]
        .chars()
        .last()
        .is_some_and(char::is_whitespace)
    {
        return false;
    }

    let mut probe = (*parser).clone();
    matches!(probe.get_end_block(), Ok(found) if found.eq_ignore_ascii_case(name))
}

fn collect_native_blockquote_line<'r, 't>(
    parser: &mut Parser<'r, 't>,
    close: &[ParseCondition],
) -> ParseResult<'r, 't, Vec<Element<'t>>> {
    let line_end_token = std::iter::once(parser.current())
        .chain(parser.remaining().iter())
        .find(|token| {
            token.token == Token::LineBreak
                || token.token == Token::ParagraphBreak
                || token.token == Token::InputEnd
        })
        .expect("tokenization always ends with input-end");
    let line_end = line_end_token.span.end;
    let mut elements = Vec::new();
    let mut errors = Vec::new();
    let mut paragraph_safe = true;

    loop {
        if parser.evaluate_any(close) {
            if parser.current().token != Token::InputEnd {
                parser.step()?;
            }
            return ok!(paragraph_safe; elements, errors);
        }

        if wikidot_footnote_close_ahead(parser) {
            return ok!(paragraph_safe; elements, errors);
        }

        if wikidot_div_close_ahead(parser) {
            return ok!(paragraph_safe; elements, errors);
        }

        let consumed = consume(parser)?.chain(&mut errors, &mut paragraph_safe);
        elements.extend(consumed);

        if parser.current().span.start >= line_end {
            return ok!(paragraph_safe; elements, errors);
        }
    }
}

fn build_blockquote_element(
    list: DepthList<(), NativeQuoteRow>,
    wikidot: bool,
) -> Option<Element> {
    let list = if wikidot {
        merge_adjacent_native_list_rows(list)
    } else {
        list
    };
    let mut stack = if wikidot {
        ParagraphStack::new_wikidot()
    } else {
        ParagraphStack::new()
    };

    for item in list {
        match item {
            DepthItem::Item(row) => push_native_quote_row(&mut stack, row, wikidot),
            DepthItem::List(_, list) => {
                if let Some(blockquote) = build_blockquote_element(list, wikidot) {
                    stack.pop_line_break();
                    stack.push_element(blockquote, false);
                }
            }
        }
    }

    stack.pop_line_break();
    let elements = stack.into_elements();
    if elements.is_empty() {
        return None;
    }

    Some(Element::Container(Container::new(
        ContainerType::Blockquote,
        elements,
        AttributeMap::new(),
    )))
}

fn merge_adjacent_native_list_rows(
    list: DepthList<(), NativeQuoteRow>,
) -> DepthList<(), NativeQuoteRow> {
    let mut merged = Vec::with_capacity(list.len());
    for item in list {
        match item {
            DepthItem::Item(mut row) => {
                if let Some(DepthItem::Item(previous)) = merged.last_mut()
                    && merge_native_list_rows(previous, &mut row)
                {
                    continue;
                }
                merged.push(DepthItem::Item(row));
            }
            DepthItem::List((), nested) => {
                merged.push(DepthItem::List((), merge_adjacent_native_list_rows(nested)));
            }
        }
    }
    merged
}

fn merge_native_list_rows<'t>(
    previous: &mut NativeQuoteRow<'t>,
    current: &mut NativeQuoteRow<'t>,
) -> bool {
    if previous.empty_spaced || current.empty_spaced {
        return false;
    }
    let Some(previous_list) = native_list_element(previous) else {
        return false;
    };
    let Some(current_list) = native_list_element(current) else {
        return false;
    };
    let Element::List {
        ltype: previous_type,
        items: previous_items,
        ..
    } = previous_list
    else {
        return false;
    };
    let Element::List {
        ltype: current_type,
        items: current_items,
        ..
    } = current_list
    else {
        return false;
    };
    if previous_type != current_type {
        return false;
    }
    previous_items.append(current_items);
    true
}

fn native_list_element<'a, 't>(
    row: &'a mut NativeQuoteRow<'t>,
) -> Option<&'a mut Element<'t>> {
    match row.elements.as_mut_slice() {
        [Element::List { .. }] | [Element::List { .. }, Element::LineBreak] => {
            row.elements.first_mut()
        }
        _ => None,
    }
}

fn build_flattened_quote_rows(
    rows: Vec<NativeQuoteRow<'_>>,
    wikidot: bool,
) -> Vec<Element<'_>> {
    let mut stack = if wikidot {
        ParagraphStack::new_wikidot()
    } else {
        ParagraphStack::new()
    };
    for row in rows {
        push_native_quote_row(&mut stack, row, wikidot);
    }
    stack.pop_line_break();
    stack.into_elements()
}

fn push_native_quote_row<'t>(
    stack: &mut ParagraphStack<'t>,
    row: NativeQuoteRow<'t>,
    wikidot: bool,
) {
    if row.empty_spaced {
        stack.pop_line_break();
        stack.end_paragraph();
        stack.mark_wikidot_simple_table_boundary();
        return;
    }
    let alignment_block = row.elements.iter().any(|element| {
        matches!(
            element,
            Element::Container(container)
                if matches!(container.ctype(), ContainerType::Align(_))
        )
    });
    if wikidot && alignment_block {
        stack.ensure_wikidot_trailing_line_break();
    }
    if wikidot && !row.paragraph_safe && !alignment_block {
        stack.pop_line_break();
    }
    let collapsible_row =
        matches!(row.elements.first(), Some(Element::Collapsible { .. }));
    if wikidot && collapsible_row {
        let mut elements = row.elements.into_iter();
        let collapsible = elements.next().unwrap();
        stack.push_element(collapsible, false);
        stack.mark_next_unwrapped();
        for (index, element) in elements.enumerate() {
            if index == 0 && element != Element::LineBreak {
                stack.push_element(text!("\n"), true);
            }
            let paragraph_safe = element.paragraph_safe();
            stack.push_element(element, paragraph_safe);
        }
        return;
    }
    let leaves_following_content_unwrapped = row
        .elements
        .iter()
        .any(collapsible_has_direct_literal_nested_opener)
        || (wikidot && alignment_block);
    let has_simple_table = row.elements.iter().any(|element| {
        matches!(
            element,
            Element::Table(table) if table.table_type == crate::tree::TableType::Simple
        )
    });
    for element in row.elements {
        stack.push_element(element, row.paragraph_safe);
    }
    if has_simple_table {
        stack.merge_wikidot_adjacent_simple_tables();
    }
    if leaves_following_content_unwrapped {
        stack.mark_next_unwrapped();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender, text::TextRender};
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

    #[test]
    fn native_blockquote_rejects_excessive_depth() {
        enable_test_logging();

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = format!("{} too deep", ">".repeat(MAX_BLOCKQUOTE_DEPTH + 1));
        let tokenization = crate::tokenize(&input);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("quote token should follow input start");
        parser.set_rule(RULE_BLOCKQUOTE);

        let error = RULE_BLOCKQUOTE
            .try_consume(&mut parser)
            .expect_err("excessive blockquote depth should fail");
        assert_eq!(error.kind(), ParseErrorKind::BlockquoteDepthExceeded);
    }

    #[test]
    fn native_blockquote_accepts_wikidot_observed_depth_31() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = format!("{} too deep", ">".repeat(31));
        let tokenization = crate::tokenize(&input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("<blockquote>").count(), 31, "{html}");
        assert!(html.contains("<p>too deep</p>"), "{html}");
    }

    #[test]
    fn native_blockquote_rejects_non_quote_start() {
        enable_test_logging();

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("plain");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("identifier token should follow input start");
        parser.set_rule(RULE_BLOCKQUOTE);

        let error = RULE_BLOCKQUOTE
            .try_consume(&mut parser)
            .expect_err("non-quote input should not produce a blockquote");
        assert_eq!(error.kind(), ParseErrorKind::RuleFailed);
    }

    #[test]
    fn native_blockquote_prunes_empty_rows_at_every_depth() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut input = "> \n>\n>> \n".to_owned();
        crate::preprocess(&mut input);
        let tokenization = crate::tokenize(&input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.trim().is_empty(), "{html}");
    }

    #[test]
    fn native_quote_simple_tables_join_across_unspaced_empty_row() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = concat!("> ||~ H || V ||\n", ">\n", "> || A || B ||",);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html.matches("<table class=\"wiki-content-table\">").count(),
            1,
            "{html}"
        );
        assert_eq!(html.matches("<tr>").count(), 2, "{html}");
    }

    #[test]
    fn native_quote_simple_tables_stay_separate_after_spaced_empty_row() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = concat!("> ||~ H || V ||\n", "> \n", "> || A || B ||",);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html.matches("<table class=\"wiki-content-table\">").count(),
            2,
            "{html}"
        );
        assert_eq!(html.matches("<tr>").count(), 2, "{html}");
    }

    #[test]
    fn native_blockquote_prunes_rows_consumed_by_an_invisible_child() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut input = concat!(
            "> [[collapsible show=\"show\" hide=\"hide\"]]\n",
            "> [[iftags +missing]]\n",
            "> [[div]]\n",
            "> OMEGA_HIDDEN\n",
            "> [[/iftags]]\n",
            "> OMEGA_VISIBLE_INSIDE\n",
            "> [[/collapsible]]\n",
            "OMEGA_AFTER",
        )
        .to_owned();
        crate::preprocess(&mut input);
        let tokenization = crate::tokenize(&input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(!html.contains("OMEGA_HIDDEN"), "{html}");
        assert!(html.contains("OMEGA_VISIBLE_INSIDE"), "{html}");
        assert!(html.contains("OMEGA_AFTER"), "{html}");
    }

    #[test]
    fn native_blockquote_content_respects_virtual_line_start_for_headings() {
        enable_test_logging();

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = concat!(
            "> + {{WARNING}}\n",
            ">\n",
            "> {{Body}}\n",
            ">\n",
            "> ++ {{LEVEL 5 AUTHORIZATION REQUIRED}}\n",
        );
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");

        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(html.contains("<blockquote>"));
        assert!(html.contains("<h1"));
        assert!(html.contains("WARNING"));
        assert!(html.contains("<h2"));
        assert!(html.contains("LEVEL 5 AUTHORIZATION REQUIRED"));
        assert!(!html.contains("+ <tt>WARNING</tt>"));
        assert!(!html.contains("++ <tt>LEVEL 5 AUTHORIZATION REQUIRED</tt>"));
        assert_eq!(html.matches("<p>").count(), 1, "{html}");
        assert_eq!(html.matches("<br>").count(), 0, "{html}");

        let text = TextRender.render(&tree, &page_info, &settings);
        assert!(text.contains("WARNING"));
        assert!(text.contains("LEVEL 5 AUTHORIZATION REQUIRED"));
        assert!(!text.contains("+ WARNING"));
        assert!(!text.contains("++ LEVEL 5 AUTHORIZATION REQUIRED"));
    }

    #[test]
    fn native_blockquote_depth_counts_only_contiguous_markers() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = concat!(
            ">> ALPHA_CONTIGUOUS_DEPTH_TWO\n",
            "> > ALPHA_SPACED_LITERAL\n",
            "> >ALPHA_TIGHT_SPACED_LITERAL\n",
            "> ALPHA_BEFORE\n",
            "> >ALPHA_ACTIVE_LITERAL\n",
            "> ALPHA_AFTER\n",
        );
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("<blockquote>").count(), 2, "{html}");
        assert!(html.contains("ALPHA_CONTIGUOUS_DEPTH_TWO"), "{html}");
        assert!(html.contains("&gt; ALPHA_SPACED_LITERAL"), "{html}");
        assert!(html.contains("&gt;ALPHA_TIGHT_SPACED_LITERAL"), "{html}");
        assert!(html.contains("&gt;ALPHA_ACTIVE_LITERAL"), "{html}");
        assert!(html.contains("ALPHA_AFTER"), "{html}");
    }

    #[test]
    fn native_blockquote_horizontal_rule_does_not_consume_outer_block_close() {
        enable_test_logging();

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut input = concat!(
            "[[collapsible]]\n",
            "> Derivative of:\n",
            "> ------\n",
            "> Author\n",
            "[[/collapsible]]\n",
            "After\n",
        )
        .to_owned();
        crate::preprocess(&mut input);
        let tokenization = crate::tokenize(&input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");

        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(html.contains(r#"<div class="collapsible-block""#), "{html}");
        assert!(html.contains("<blockquote>"), "{html}");
        assert!(html.contains("<hr>"), "{html}");
        assert!(html.contains("Author"), "{html}");
        assert!(html.contains("After"), "{html}");
        assert!(!html.contains("[[collapsible"), "{html}");
        assert!(!html.contains("[[/collapsible]]"), "{html}");
        assert_eq!(html.matches("<p>").count(), 3, "{html}");
        assert_eq!(html.matches("<br>").count(), 0, "{html}");
        assert!(html.contains("<p>After</p>"), "{html}");
    }

    #[test]
    fn native_blockquote_line_rules_do_not_trigger_combinatorial_div_retries() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        // Reduced from EN:anomalous-entity-engagement-division-hub
        // (source SHA-256 19ceb79035996a7c70df15cbcfc49ebe4833147aef4343b43ae77786f43e5f04),
        // which repeats div-wrapped native quote lines throughout its tabview.
        let cases = [
            ("= Centered corpus quote", "text-align: center;"),
            ("+ Quoted heading", "<h1"),
            ("* Quoted list item", "<ul>"),
            ("[[toc]]", "id=\"toc\""),
            ("----", "<hr>"),
        ];

        for (quoted_line, expected) in cases {
            let mut input = String::new();
            for _ in 0..25 {
                input.push_str("[[div class=\"table1\"]]\n> ");
                input.push_str(quoted_line);
                input.push_str("\n[[/div]]\n");
            }
            input.push_str("Following sentinel\n");
            crate::preprocess(&mut input);

            let tokenization = crate::tokenize(&input);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;

            assert!(errors.is_empty(), "{quoted_line}: {errors:#?}");
            assert_eq!(
                html.matches("class=\"table1\"").count(),
                25,
                "{quoted_line}: {html}",
            );
            assert_eq!(
                html.matches("<blockquote>").count(),
                25,
                "{quoted_line}: {html}",
            );
            assert_eq!(html.matches("<br>").count(), 1, "{quoted_line}: {html}",);
            let expected_paragraphs = usize::from(quoted_line.starts_with('='));
            assert_eq!(
                html.matches("<p").count(),
                expected_paragraphs * 25,
                "{quoted_line}: {html}",
            );
            assert!(html.contains(expected), "{quoted_line}: {html}");
            assert!(html.contains("Following sentinel"), "{quoted_line}: {html}");
        }
    }

    #[test]
    fn structural_quote_line_break_policy_is_wikidot_only() {
        let page_info = PageInfo::dummy();
        let input = "> + Quoted heading\n";

        let render = |layout| {
            let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
            let tokenization = crate::tokenize(input);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            assert!(errors.is_empty(), "{layout:?}: {errors:#?}");
            HtmlRender.render(&tree, &page_info, &settings).body
        };

        let wikidot = render(Layout::Wikidot);
        let wikijump = render(Layout::Wikijump);
        assert_eq!(wikidot.matches("<br>").count(), 0, "{wikidot}");
        assert_eq!(wikijump.matches("<br>").count(), 1, "{wikijump}");
    }

    #[test]
    fn multiline_inline_pairs_stay_in_one_native_blockquote() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for (open, close) in [
            ("^^", "^^"),
            ("**", "**"),
            ("//", "//"),
            (",,", ",,"),
            ("__", "__"),
        ] {
            let mut input = String::new();
            for _ in 0..64 {
                input.push_str("> ");
                input.push_str(open);
                input.push_str("\n> quoted text");
                input.push_str(close);
                input.push('\n');
            }

            let tokenization = crate::tokenize(&input);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;

            assert!(errors.is_empty(), "{open}: {errors:#?}");
            assert_eq!(html.matches("<blockquote>").count(), 1, "{open}: {html}");
            assert_eq!(html.matches("<p>").count(), 1, "{open}: {html}");
            assert_eq!(html.matches("<br>").count(), 127, "{open}: {html}");
            assert_eq!(html.matches("quoted text").count(), 64, "{open}: {html}");
        }
    }
}
