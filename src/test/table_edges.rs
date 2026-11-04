/*
 * test/table_edges.rs
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

use crate::data::PageInfo;
use crate::layout::Layout;
use crate::settings::{WikitextMode, WikitextSettings};
use crate::tree::{Alignment, ContainerType, Element, TableType};

#[derive(Debug)]
struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, _record: &log::Record<'_>) {}

    fn flush(&self) {}
}

static TEST_LOGGER: TestLogger = TestLogger;
static TEST_LOGGER_INIT: std::sync::Once = std::sync::Once::new();

fn enable_test_logging() {
    TEST_LOGGER_INIT.call_once(|| {
        let _ = log::set_logger(&TEST_LOGGER);
        log::set_max_level(log::LevelFilter::Trace);
    });
}

fn assert_single_text(elements: &[Element<'_>], expected: &str) {
    match elements {
        [Element::Text(actual)] => assert_eq!(actual, expected),
        _ => panic!("expected one text element, got {elements:?}"),
    }
}

fn assert_text_content(elements: &[Element<'_>], expected: &str) {
    let mut actual = String::new();

    for element in elements {
        match element {
            Element::Text(text) => actual.push_str(text),
            _ => panic!("expected text-only elements, got {elements:?}"),
        }
    }

    assert_eq!(actual, expected);
}

#[test]
fn simple_table_parser_edges_preserve_rows_and_headers() {
    enable_test_logging();

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

    let tokenization = crate::tokenize("|| A  || text ||\n|| B ||   \n|| C ||\n");
    let result = crate::parse(&tokenization, &page_info, &settings);
    let (tree, errors) = result.into();

    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");

    let [Element::Table(table)] = tree.elements.as_slice() else {
        panic!("expected one table, got {:?}", tree.elements);
    };

    assert_eq!(table.table_type, TableType::Simple);
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[0].cells.len(), 2);
    assert_eq!(table.rows[1].cells.len(), 1);
    assert_eq!(table.rows[2].cells.len(), 1);

    assert_single_text(&table.rows[0].cells[0].elements, "A");
    assert_single_text(&table.rows[0].cells[1].elements, "text");
    assert_single_text(&table.rows[1].cells[0].elements, "B");
    assert_single_text(&table.rows[2].cells[0].elements, "C");

    let tokenization = crate::tokenize("|| Name ||~ Value ||\n");
    let result = crate::parse(&tokenization, &page_info, &settings);
    let (tree, errors) = result.into();

    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");

    let [Element::Table(table)] = tree.elements.as_slice() else {
        panic!("expected one table, got {:?}", tree.elements);
    };

    assert_eq!(table.table_type, TableType::Simple);
    assert_eq!(table.rows.len(), 1);
    assert_eq!(table.rows[0].cells.len(), 2);
    assert!(!table.rows[0].cells[0].header);
    assert!(table.rows[0].cells[1].header);
    assert_single_text(&table.rows[0].cells[0].elements, "Name");
    assert_single_text(&table.rows[0].cells[1].elements, "Value");
}

#[test]
fn simple_table_missing_end_renders_an_empty_cell_like_wikidot() {
    enable_test_logging();

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize("|| Missing end");
    let result = crate::parse(&tokenization, &page_info, &settings);
    let (tree, errors) = result.into();

    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
    let [Element::Table(table)] = tree.elements.as_slice() else {
        panic!("expected one table, got {:?}", tree.elements);
    };
    assert_eq!(table.rows.len(), 1);
    assert_eq!(table.rows[0].cells.len(), 1);
    assert!(table.rows[0].cells[0].elements.is_empty());
}

#[test]
fn simple_table_finishes_before_following_paragraph() {
    enable_test_logging();

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize("|| A ||\nfollowing");
    let result = crate::parse(&tokenization, &page_info, &settings);
    let (tree, errors) = result.into();

    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
    let [Element::Table(table), Element::Container(paragraph)] = tree.elements.as_slice()
    else {
        panic!(
            "expected table followed by paragraph, got {:?}",
            tree.elements
        );
    };

    assert_eq!(table.table_type, TableType::Simple);
    assert_eq!(table.rows.len(), 1);
    assert_eq!(table.rows[0].cells.len(), 1);
    assert_single_text(&table.rows[0].cells[0].elements, "A");
    assert_eq!(paragraph.ctype(), ContainerType::Paragraph);
    assert_single_text(paragraph.elements(), "following");
}

#[test]
fn simple_table_consumes_rich_cell_contents() {
    enable_test_logging();

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize("|| **bold** ||");
    let result = crate::parse(&tokenization, &page_info, &settings);
    let (tree, errors) = result.into();

    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
    let [Element::Table(table)] = tree.elements.as_slice() else {
        panic!("expected one table, got {:?}", tree.elements);
    };

    assert_eq!(table.rows.len(), 1);
    assert_eq!(table.rows[0].cells.len(), 1);
    assert_eq!(table.rows[0].cells[0].elements.len(), 1);
    assert!(
        !matches!(table.rows[0].cells[0].elements[0], Element::Text(_)),
        "rich table cell content should not flatten into plain text",
    );
}

#[test]
fn simple_table_left_marker_sets_cell_alignment() {
    enable_test_logging();

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize("||< left ||");
    let result = crate::parse(&tokenization, &page_info, &settings);
    let (tree, errors) = result.into();

    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
    let [Element::Table(table)] = tree.elements.as_slice() else {
        panic!("expected one table, got {:?}", tree.elements);
    };

    assert_eq!(table.table_type, TableType::Simple);
    assert_eq!(table.rows.len(), 1);
    assert_eq!(table.rows[0].cells.len(), 1);

    let cell = &table.rows[0].cells[0];
    assert!(!cell.header);
    assert_eq!(cell.align, Some(Alignment::Left));
    assert_eq!(cell.column_span.get(), 1);
    assert_text_content(&cell.elements, "left");
}

#[test]
fn simple_table_combined_header_alignment_markers_remain_literal() {
    enable_test_logging();

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize(
        "||~> header right ||\n||~= header center ||\n||>~ right header ||\n||=~ center header ||",
    );
    let result = crate::parse(&tokenization, &page_info, &settings);
    let (tree, errors) = result.into();

    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
    let [Element::Table(table)] = tree.elements.as_slice() else {
        panic!("expected one table, got {:?}", tree.elements);
    };

    assert_eq!(table.table_type, TableType::Simple);
    assert_eq!(table.rows.len(), 4);

    let cell = &table.rows[0].cells[0];
    assert!(!cell.header);
    assert_eq!(cell.align, None);
    assert_text_content(&cell.elements, "~> header right");

    let cell = &table.rows[1].cells[0];
    assert!(!cell.header);
    assert_eq!(cell.align, None);
    assert_text_content(&cell.elements, "~= header center");

    let cell = &table.rows[2].cells[0];
    assert!(!cell.header);
    assert_eq!(cell.align, None);
    assert_text_content(&cell.elements, ">~ right header");

    let cell = &table.rows[3].cells[0];
    assert!(!cell.header);
    assert_eq!(cell.align, None);
    assert_text_content(&cell.elements, "=~ center header");
}

fn with_wikidot_table<R>(
    input: &str,
    check: impl FnOnce(&crate::tree::Table<'_>) -> R,
) -> R {
    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize(input);
    let result = crate::parse(&tokenization, &page_info, &settings);
    let (tree, errors) = result.into();

    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");

    let [Element::Table(table)] = tree.elements.as_slice() else {
        panic!("expected one table, got {:?}", tree.elements);
    };

    assert_eq!(table.table_type, TableType::Simple);
    check(table)
}

fn with_wikijump_table<R>(
    input: &str,
    check: impl FnOnce(&crate::tree::Table<'_>) -> R,
) -> R {
    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let tokenization = crate::tokenize(input);
    let result = crate::parse(&tokenization, &page_info, &settings);
    let (tree, errors) = result.into();

    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");

    let [Element::Table(table)] = tree.elements.as_slice() else {
        panic!("expected one table, got {:?}", tree.elements);
    };

    assert_eq!(table.table_type, TableType::Simple);
    check(table)
}

fn assert_bold_text(elements: &[Element<'_>], expected: &str) {
    let [Element::Container(container)] = elements else {
        panic!("expected one container, got {elements:?}");
    };

    assert_eq!(container.ctype(), ContainerType::Bold);
    assert_single_text(container.elements(), expected);
}

#[test]
fn simple_table_bold_closes_at_boundary_with_closer_in_next_row() {
    enable_test_logging();

    with_wikidot_table("||**a||\n||b**||", |table| {
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0].cells.len(), 1);
        assert_eq!(table.rows[1].cells.len(), 1);
        assert_bold_text(&table.rows[0].cells[0].elements, "a");
        assert_single_text(&table.rows[1].cells[0].elements, "b");
    });
}

#[test]
fn simple_table_bold_closes_at_boundary_with_closer_two_rows_down() {
    enable_test_logging();

    with_wikidot_table("||**a||\n||middle||\n||b**||", |table| {
        assert_eq!(table.rows.len(), 3);
        assert_bold_text(&table.rows[0].cells[0].elements, "a");
        assert_single_text(&table.rows[1].cells[0].elements, "middle");
        assert_single_text(&table.rows[2].cells[0].elements, "b");
    });
}

#[test]
fn simple_table_color_closes_at_boundary_with_closer_in_next_row() {
    enable_test_logging();

    with_wikidot_table("||##red|a||\n||b##||", |table| {
        assert_eq!(table.rows.len(), 2);
        let [Element::Color { elements, .. }] =
            table.rows[0].cells[0].elements.as_slice()
        else {
            panic!(
                "expected one color element, got {:?}",
                table.rows[0].cells[0].elements
            );
        };
        assert_single_text(elements, "a");
        assert_single_text(&table.rows[1].cells[0].elements, "b");
    });
}

#[test]
fn simple_table_unterminated_row_discards_contents_before_paragraph() {
    enable_test_logging();

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize("||**a\nb**||");
    let result = crate::parse(&tokenization, &page_info, &settings);
    let (tree, _errors) = result.into();

    let [Element::Table(table), Element::Container(paragraph)] = tree.elements.as_slice()
    else {
        panic!("expected table then paragraph, got {:?}", tree.elements);
    };

    assert_eq!(table.rows.len(), 1);
    assert_eq!(table.rows[0].cells.len(), 1);
    assert!(table.rows[0].cells[0].elements.is_empty());
    assert_eq!(paragraph.ctype(), ContainerType::Paragraph);
    assert_text_content(paragraph.elements(), "b**||");
}

#[test]
fn simple_table_unterminated_row_before_next_row_stays_one_table() {
    enable_test_logging();

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize("||**a\n||b**||");
    let result = crate::parse(&tokenization, &page_info, &settings);
    let (tree, _errors) = result.into();

    let [Element::Table(table)] = tree.elements.as_slice() else {
        panic!("expected one table, got {:?}", tree.elements);
    };

    assert_eq!(table.rows.len(), 2);
    assert!(table.rows[0].cells[0].elements.is_empty());
    assert_text_content(&table.rows[1].cells[0].elements, "b**");
}

#[test]
fn simple_table_unterminated_row_keeps_completed_cells_in_wikijump() {
    enable_test_logging();

    with_wikijump_table("|| A || B ||\n|| C || D", |table| {
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0].cells.len(), 2);
        assert_single_text(&table.rows[0].cells[0].elements, "A");
        assert_single_text(&table.rows[0].cells[1].elements, "B");
        assert_eq!(table.rows[1].cells.len(), 1);
        assert_single_text(&table.rows[1].cells[0].elements, "C");
    });
}

#[test]
fn simple_table_junk_after_last_cell_keeps_completed_cells_in_wikijump() {
    enable_test_logging();

    with_wikijump_table("|| A || B ||\n|| C || D || junk", |table| {
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[1].cells.len(), 2);
        assert_single_text(&table.rows[1].cells[0].elements, "C");
        assert_single_text(&table.rows[1].cells[1].elements, "D");
    });
}

#[test]
fn simple_table_span_inside_bold_closes_at_cell_boundary() {
    enable_test_logging();

    with_wikidot_table("||**a [[span]]b||c[[/span]] d**||", |table| {
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0].cells.len(), 2);
        let [Element::Container(bold)] = table.rows[0].cells[0].elements.as_slice()
        else {
            panic!(
                "expected one container, got {:?}",
                table.rows[0].cells[0].elements
            );
        };
        assert_eq!(bold.ctype(), ContainerType::Bold);
        let span = bold
            .elements()
            .iter()
            .find_map(|element| match element {
                Element::Container(container)
                    if container.ctype() == ContainerType::Span =>
                {
                    Some(container)
                }
                _ => None,
            })
            .expect("expected a span container inside bold");
        assert_single_text(span.elements(), "b");
        assert_text_content(&table.rows[0].cells[1].elements, "c d");
    });
}

#[test]
fn simple_table_crossed_closer_ends_span_opened_in_later_cell() {
    enable_test_logging();

    with_wikidot_table("||**a||[[span]]b** c[[/span]]||", |table| {
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0].cells.len(), 2);
        assert_bold_text(&table.rows[0].cells[0].elements, "a");
        let cell = &table.rows[0].cells[1];
        let [Element::Container(span), rest @ ..] = cell.elements.as_slice() else {
            panic!("expected span then text, got {:?}", cell.elements);
        };
        assert_eq!(span.ctype(), ContainerType::Span);
        assert_single_text(span.elements(), "b");
        assert_text_content(rest, " c");
    });
}
