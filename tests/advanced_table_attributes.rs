use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender, text::TextRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::{Element, SyntaxTree};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("advanced-table-attributes"),
        category: None,
        site: Cow::Borrowed("compatibility"),
        title: Cow::Borrowed("Advanced table attributes"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![],
        language: Cow::Borrowed("en"),
    }
}

fn parse(source: &str, layout: Layout) -> SyntaxTree<'static> {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
    tree.to_owned()
}

fn render_tree(tree: &SyntaxTree, layout: Layout) -> (String, String) {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    (
        HtmlRender.render(tree, &page_info, &settings).body,
        TextRender.render(tree, &page_info, &settings),
    )
}

fn advanced_cell(arguments: &str) -> String {
    format!("[[table]]\n[[row]]\n[[cell{arguments}]]\nA\n[[/cell]]\n[[/row]]\n[[/table]]")
}

#[derive(Clone, Copy, Debug)]
enum AttributeOwner {
    Table,
    Row,
    Cell,
}

fn advanced_table_with_attribute(owner: AttributeOwner, arguments: &str) -> String {
    match owner {
        AttributeOwner::Table => format!(
            "[[table{arguments}]]\n[[row]]\n[[cell]]A[[/cell]]\n[[/row]]\n[[/table]]"
        ),
        AttributeOwner::Row => format!(
            "[[table]]\n[[row{arguments}]]\n[[cell]]A[[/cell]]\n[[/row]]\n[[/table]]"
        ),
        AttributeOwner::Cell => advanced_cell(arguments),
    }
}

fn expected_table_with_attribute(owner: AttributeOwner, attribute: &str) -> String {
    match owner {
        AttributeOwner::Table => {
            format!("<table{attribute}>\n<tr>\n<td>A</td>\n</tr>\n</table>")
        }
        AttributeOwner::Row => {
            format!("<table>\n<tr{attribute}>\n<td>A</td>\n</tr>\n</table>")
        }
        AttributeOwner::Cell => {
            format!("<table>\n<tr>\n<td{attribute}>A</td>\n</tr>\n</table>")
        }
    }
}

fn only_cell<'a, 't>(tree: &'a SyntaxTree<'t>) -> &'a ftml::tree::TableCell<'t> {
    let [Element::Table(table)] = tree.elements.as_slice() else {
        panic!("expected one advanced table: {:?}", tree.elements);
    };
    let [row] = table.rows.as_slice() else {
        panic!("expected one row: {:?}", table.rows);
    };
    let [cell] = row.cells.as_slice() else {
        panic!("expected one cell: {:?}", row.cells);
    };
    cell
}

#[test]
fn advanced_table_header_case_matrix_matches_live_wikidot() {
    for (table, row, cell, expected_tag) in [
        ("table", "row", "cell", "td"),
        ("TABLE", "ROW", "CELL", "td"),
        ("Table", "Row", "Cell", "td"),
        ("tAbLe", "rOw", "cElL", "td"),
        ("table", "row", "hcell", "th"),
        ("table", "row", "HCELL", "td"),
        ("table", "row", "Hcell", "td"),
    ] {
        let source = format!(
            "[[{table}]]\n[[{row}]]\n[[{cell}]]A[[/{cell}]]\n[[/{row}]]\n[[/{table}]]"
        );
        let tree = parse(&source, Layout::Wikidot);
        let (html, text) = render_tree(&tree, Layout::Wikidot);
        assert_eq!(
            html,
            format!("<table>\n<tr>\n<{expected_tag}>A</{expected_tag}>\n</tr>\n</table>"),
            "{source:?}",
        );
        assert_eq!(text, "A", "{source:?}");
    }
}

#[test]
fn advanced_table_element_attribute_matrix_matches_live_wikidot() {
    let cases = [
        (r#" class="grid""#, r#" class="grid""#),
        (
            r#" style="border-collapse:collapse""#,
            r#" style="border-collapse:collapse""#,
        ),
        (
            r#" class="grid" style="width:100%""#,
            r#" class="grid" style="width:100%""#,
        ),
        (r#" title="T""#, ""),
        (r#" id="x""#, r#" id="u-x""#),
        (r#" data-x="1""#, r#" data-x="1""#),
        (r#" onclick="x""#, ""),
        (" class=grid", ""),
        (" class='grid'", ""),
        (r#" CLASS="grid""#, ""),
        (r#" Class="grid""#, ""),
        (r#" class="""#, ""),
        // Live serializes a whitespace-normalized empty value as `class=""`.
        (r#" class=" ""#, r#" class="""#),
        (r#" class="a" class="b""#, r#" class="b""#),
        (r#" class="A[!--x--]B""#, r#" class="AB""#),
        (r#" class="日本語""#, r#" class="日本語""#),
        (
            r#" style="color:red;expression(x)""#,
            r#" style="color:red;expression(x)""#,
        ),
    ];

    for owner in [
        AttributeOwner::Table,
        AttributeOwner::Row,
        AttributeOwner::Cell,
    ] {
        for (arguments, expected_attribute) in cases {
            let source = advanced_table_with_attribute(owner, arguments);
            let tree = parse(&source, Layout::Wikidot);
            let (html, text) = render_tree(&tree, Layout::Wikidot);
            assert_eq!(
                html,
                expected_table_with_attribute(owner, expected_attribute),
                "{owner:?}: {arguments:?}",
            );
            assert_eq!(text, "A", "{owner:?}: {arguments:?}");
        }
    }
}

#[test]
fn advanced_table_span_lexeme_matrix_matches_live_wikidot() {
    let cases = [
        ("0", None, 1),
        ("1", Some("1"), 1),
        ("2", Some("2"), 2),
        ("00", Some("00"), 1),
        ("01", Some("01"), 1),
        ("-1", Some("-1"), 1),
        ("+1", Some("+1"), 1),
        ("1.0", Some("1.0"), 1),
        ("1e2", Some("1e2"), 1),
        ("4294967295", Some("4294967295"), u32::MAX),
        ("4294967296", Some("4294967296"), 1),
        ("", None, 1),
        (" ", Some(""), 1),
        ("abc", Some("abc"), 1),
        (" 2 ", Some("2"), 2),
        ("2x", Some("2x"), 1),
    ];

    for attribute in ["colspan", "rowspan"] {
        for (value, expected_value, expected_column_span) in cases {
            let arguments = format!(r#" {attribute}="{value}""#);
            let source = advanced_cell(&arguments);
            let tree = parse(&source, Layout::Wikidot);
            let cell = only_cell(&tree);
            let expected_attribute = expected_value.map_or_else(String::new, |value| {
                if value.is_empty() {
                    format!(r#" {attribute}="""#)
                } else {
                    format!(r#" {attribute}="{value}""#)
                }
            });
            let expected_semantic = if attribute == "colspan" {
                expected_column_span
            } else {
                1
            };

            assert_eq!(
                cell.column_span.get(),
                expected_semantic,
                "{attribute}={value:?}",
            );
            assert_eq!(
                cell.attributes.get().get(attribute).map(Cow::as_ref),
                expected_value,
                "{attribute}={value:?}",
            );
            assert_eq!(
                render_tree(&tree, Layout::Wikidot),
                (
                    expected_table_with_attribute(
                        AttributeOwner::Cell,
                        &expected_attribute
                    ),
                    "A".to_owned(),
                ),
                "{attribute}={value:?}",
            );
        }
    }

    for (arguments, expected) in [
        (" colspan=2", ""),
        (" colspan='2'", ""),
        (r#" COLSPAN="2""#, ""),
        (r#" colspan="2" colspan="3""#, r#" colspan="3""#),
        (r#" rowspan="2" rowspan="3""#, r#" rowspan="3""#),
        (r#" colspan="2" rowspan="3""#, r#" colspan="2" rowspan="3""#),
    ] {
        let tree = parse(&advanced_cell(arguments), Layout::Wikidot);
        assert_eq!(
            render_tree(&tree, Layout::Wikidot),
            (
                expected_table_with_attribute(AttributeOwner::Cell, expected),
                "A".to_owned(),
            ),
            "{arguments:?}",
        );
    }
}

#[test]
fn advanced_table_attributes_are_element_specific_and_security_filtered() {
    let source = concat!(
        "[[table colspan=\"2\" rowspan=\"3\" onclick=\"alert(1)\"]]\n",
        "[[row colspan=\"2\" rowspan=\"3\" onmouseover=\"alert(1)\"]]\n",
        "[[cell background=\"javascript:alert(1)\" onclick=\"alert(1)\" ",
        "colspan=\"2\" rowspan=\"3\"]]A[[/cell]]\n",
        "[[/row]]\n[[/table]]",
    );
    let tree = parse(source, Layout::Wikidot);
    let (html, text) = render_tree(&tree, Layout::Wikidot);

    assert_eq!(
        html,
        concat!(
            "<table>\n<tr>\n",
            "<td background=\"#invalid-url\" colspan=\"2\" rowspan=\"3\">A</td>\n",
            "</tr>\n</table>",
        ),
    );
    assert_eq!(text, "A");
    assert!(!html.contains("javascript:"), "{html}");
    assert!(!html.contains("onclick"), "{html}");
    assert!(!html.contains("onmouseover"), "{html}");
}

#[test]
fn advanced_table_colspan_round_trips_lexeme_and_semantic_span() {
    let tree = parse(&advanced_cell(r#" colspan="0002""#), Layout::Wikidot);
    let json = serde_json::to_string(&tree).expect("serialize advanced table tree");
    let round_trip: SyntaxTree<'static> =
        serde_json::from_str(&json).expect("deserialize advanced table tree");

    assert_eq!(only_cell(&round_trip).column_span.get(), 2);
    assert_eq!(
        only_cell(&round_trip)
            .attributes
            .get()
            .get("colspan")
            .map(Cow::as_ref),
        Some("0002"),
    );
    assert_eq!(
        render_tree(&round_trip, Layout::Wikidot),
        (
            expected_table_with_attribute(AttributeOwner::Cell, r#" colspan="0002""#),
            "A".to_owned(),
        ),
    );
}

#[test]
fn advanced_table_long_colspan_lexemes_have_bounded_numeric_work() {
    const DIGITS: usize = 128 * 1024;
    let value = "9".repeat(DIGITS);
    let source = advanced_cell(&format!(r#" colspan="{value}""#));
    let started = Instant::now();
    let tree = parse(&source, Layout::Wikidot);
    let elapsed = started.elapsed();
    let cell = only_cell(&tree);

    assert_eq!(cell.column_span.get(), 1);
    assert_eq!(
        cell.attributes
            .get()
            .get("colspan")
            .map(|lexeme| lexeme.len()),
        Some(DIGITS),
    );
    assert!(elapsed < Duration::from_secs(5), "elapsed: {elapsed:?}");

    let (html, text) = render_tree(&tree, Layout::Wikidot);
    assert_eq!(text, "A");
    assert!(html.starts_with("<table>\n<tr>\n<td colspan=\"9999"));
    assert!(html.ends_with("\">A</td>\n</tr>\n</table>"));
}

#[test]
fn wikijump_advanced_table_attribute_behavior_stays_unchanged() {
    let source = concat!(
        "[[table title=\"T\"]]\n",
        "[[row title=\"R\"]]\n",
        "[[cell title=\"C\" colspan=\"01\"]]A[[/cell]]\n",
        "[[/row]]\n[[/table]]",
    );
    let tree = parse(source, Layout::Wikijump);

    assert_eq!(
        render_tree(&tree, Layout::Wikijump),
        (
            concat!(
                "<table class=\"wj-table wj-table-advanced\" title=\"T\"><tbody>",
                "<tr title=\"R\"><td title=\"C\">A</td></tr></tbody></table>",
            )
            .to_owned(),
            "A".to_owned(),
        ),
    );
    assert_eq!(only_cell(&tree).column_span.get(), 1);
}
