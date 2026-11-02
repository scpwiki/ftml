use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("advanced-table-recovery"),
        category: None,
        site: Cow::Borrowed("compatibility"),
        title: Cow::Borrowed("Advanced table recovery"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![],
        language: Cow::Borrowed("en"),
    }
}

fn render_with_layout(source: &str, layout: Layout) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, _) = ftml::parse(&tokenization, &page_info, &settings).into();
    HtmlRender.render(&tree, &page_info, &settings).body
}

fn render(source: &str) -> String {
    render_with_layout(source, Layout::Wikidot)
}

#[test]
fn advanced_table_structural_recovery_matches_live_wikidot() {
    assert_eq!(
        render(concat!(
            "[[table]]\n",
            "[[row]]\n",
            "[[cell]]\n",
            "A\n",
            "[[/hcell]]\n",
            "[[/row]]\n",
            "[[/table]]",
        )),
        "<table>\n<tr>\n<th>A</th>\n</tr>\n</table>",
    );

    assert_eq!(
        render("[[table]]\n[[row]]\n[[cell]]A[[/row]]\n[[/table]]"),
        "<p>[[table]]<br>\n[[row]]<br>\n[[cell]]A[[/row]]<br>\n[[/table]]</p>",
    );

    assert_eq!(
        render(concat!(
            "[[table]]\n",
            "[[row]]\n",
            "[[cell]]\n",
            "A\n",
            "[[/cell]]\n",
            "[[/cell]]\n",
            "[[/row]]\n",
            "[[/table]]",
        )),
        "[[/cell]]\n<table>\n<tr>\n<td>A</td>\n</tr>\n</table>",
    );
}

#[test]
fn advanced_table_invocation_scope_matches_live_wikidot() {
    const TABLE: &str = concat!(
        "[[table]]\n",
        "[[row]]\n",
        "[[cell]]\n",
        "A\n",
        "[[/cell]]\n",
        "[[/row]]\n",
        "[[/table]]",
    );
    const TABLE_HTML: &str = "<table>\n<tr>\n<td>A</td>\n</tr>\n</table>";

    assert_eq!(
        render("[[ table]]\n[[row]]\n[[cell]]A[[/cell]]\n[[/row]]\n[[/table]]"),
        "<p>[[ table]]<br>\n[[row]]<br>\n[[cell]]A[[/cell]]<br>\n[[/row]]<br>\n[[/table]]</p>",
    );
    assert_eq!(
        render("[[ table ]]\n[[ row ]]\n[[ cell ]]A[[/cell]]\n[[/row]]\n[[/table]]"),
        "<p>[[ table ]]<br>\n[[ row ]]<br>\n[[ cell ]]A[[/cell]]<br>\n[[/row]]<br>\n[[/table]]</p>",
    );

    assert_eq!(
        render(&format!("+ {TABLE}")),
        concat!(
            "<h1 id=\"toc0\"><span>[[table]]</span></h1>",
            "<p>[[row]]<br>\n[[cell]]<br>\nA<br>\n",
            "[[/cell]]<br>\n[[/row]]<br>\n[[/table]]</p>",
        ),
    );
    assert_eq!(
        render(&format!("{{{{{TABLE}}}}}")),
        format!("<p>{{{{</p>{TABLE_HTML}<p>}}}}</p>"),
    );

    assert_eq!(
        render(&format!("> {}", TABLE.replace('\n', "\n> "))),
        format!("<blockquote>{TABLE_HTML}</blockquote>"),
    );
    assert_eq!(
        render(&format!("* {}", TABLE.replace('\n', "\n* "))),
        concat!(
            "<ul>\n",
            "<li>[[table]]</li>\n<li>[[row]]</li>\n<li>[[cell]]</li>\n",
            "<li>A</li>\n<li>[[/cell]]</li>\n<li>[[/row]]</li>\n",
            "<li>[[/table]]</li>\n</ul>",
        ),
    );
    assert_eq!(
        render(&format!("@@{TABLE}@@")),
        format!("<p>@@</p>{TABLE_HTML}<p>@@</p>"),
    );
    assert_eq!(render(&format!("[!--{TABLE}--]")), "");
    assert_eq!(
        render(&format!("[[code]]\n{TABLE}\n[[/code]]")),
        format!("<div class=\"code\"><pre><code>{TABLE}</code></pre></div>"),
    );
}

#[test]
fn advanced_table_cell_closer_matrix_matches_live_wikidot() {
    for (opener, closer, tag) in [
        ("cell", "cell", "td"),
        ("cell", "hcell", "th"),
        ("hcell", "cell", "td"),
        ("hcell", "hcell", "th"),
    ] {
        let source = format!(
            "[[table]]\n[[row]]\n[[{opener}]]A[[/{closer}]]\n[[/row]]\n[[/table]]"
        );
        assert_eq!(
            render(&source),
            format!("<table>\n<tr>\n<{tag}>A</{tag}>\n</tr>\n</table>"),
            "{opener}/{closer}",
        );
    }

    for (opener, closer, residual, tag) in [
        ("cell", "cell", "cell", "td"),
        ("cell", "cell", "hcell", "td"),
        ("hcell", "hcell", "cell", "th"),
        ("hcell", "hcell", "hcell", "th"),
    ] {
        let source = format!(
            concat!(
                "[[table]]\n[[row]]\n[[{opener}]]A[[/{closer}]]\n",
                "[[/{residual}]]\n[[/row]]\n[[/table]]",
            ),
            opener = opener,
            closer = closer,
            residual = residual
        );
        assert_eq!(
            render(&source),
            format!("[[/{residual}]]\n<table>\n<tr>\n<{tag}>A</{tag}>\n</tr>\n</table>"),
        );
    }

    let duplicate = concat!(
        "[[table]]\n[[row]]\n[[cell]]A[[/cell]]\n",
        "[[/cell]]\n[[/cell]]\n[[/row]]\n[[/table]]",
    );
    assert_eq!(
        render(duplicate),
        "[[/cell]]<br>\n[[/cell]]\n<table>\n<tr>\n<td>A</td>\n</tr>\n</table>",
    );
}

#[test]
fn advanced_table_invalid_outer_graph_preserves_complete_source() {
    for (source, expected) in [
        (
            "[[table]]\n[[row]]\n[[cell]]A[[/row]]\n[[/table]]",
            "<p>[[table]]<br>\n[[row]]<br>\n[[cell]]A[[/row]]<br>\n[[/table]]</p>",
        ),
        (
            "[[table]]\n[[row]]\n[[cell]]A[[/table]]",
            "<p>[[table]]<br>\n[[row]]<br>\n[[cell]]A[[/table]]</p>",
        ),
        (
            "[[table]]\n[[row]]\n[[cell]]A[[/cell]]\n[[/table]]",
            "<p>[[table]]<br>\n[[row]]<br>\n[[cell]]A[[/cell]]<br>\n[[/table]]</p>",
        ),
        (
            "[[table]]\n[[row]]\n[[cell]]A[[/cell]]\n[[/row]]",
            "<p>[[table]]<br>\n[[row]]<br>\n[[cell]]A[[/cell]]<br>\n[[/row]]</p>",
        ),
        (
            concat!(
                "[[table]]\n[[row]]\n[[cell]]A[[/cell]]\n[[/row]]\n",
                "[[/row]]\n[[/table]]",
            ),
            concat!(
                "<p>[[table]]<br>\n[[row]]<br>\n[[cell]]A[[/cell]]<br>\n",
                "[[/row]]<br>\n[[/row]]<br>\n[[/table]]</p>",
            ),
        ),
        (
            concat!(
                "[[table]]\n[[row]]\n[[/cell]]\n",
                "[[cell]]A[[/cell]]\n[[/row]]\n[[/table]]",
            ),
            concat!(
                "<p>[[table]]<br>\n[[row]]<br>\n[[/cell]]<br>\n",
                "[[cell]]A[[/cell]]<br>\n[[/row]]<br>\n[[/table]]</p>",
            ),
        ),
    ] {
        let html = render(source);
        assert_eq!(html, expected, "{source:?}");
        assert!(!html.contains("<table>"), "{source:?}: {html}");
    }

    let valid_then_extra_table = concat!(
        "[[table]]\n[[row]]\n[[cell]]A[[/cell]]\n[[/row]]\n[[/table]]\n",
        "[[/table]]",
    );
    assert_eq!(
        render(valid_then_extra_table),
        "<table>\n<tr>\n<td>A</td>\n</tr>\n</table><p>[[/table]]</p>",
    );
}

#[test]
fn advanced_table_matching_controls_stay_unchanged() {
    for source in [
        "[[table]][[row]][[cell]]A[[/cell]][[/row]][[/table]]",
        "[[TABLE]]\n[[ROW]]\n[[CELL]]A[[/CELL]]\n[[/ROW]]\n[[/TABLE]]",
        "[[Table]]\n[[Row]]\n[[Cell]]A[[/Cell]]\n[[/Row]]\n[[/Table]]",
        "[[tAbLe]]\n[[rOw]]\n[[cElL]]A[[/cElL]]\n[[/rOw]]\n[[/tAbLe]]",
        "[[table ]]\n[[row ]]\n[[cell ]]A[[/cell]]\n[[/row]]\n[[/table]]",
        " [[table]]\n [[row]]\n [[cell]]A[[/cell]]\n [[/row]]\n [[/table]]",
        "[[table]]\n  [[row]]\n    [[cell]]A[[/cell]]\n  [[/row]]\n[[/table]]",
    ] {
        assert_eq!(
            render(source),
            "<table>\n<tr>\n<td>A</td>\n</tr>\n</table>",
            "{source:?}",
        );
    }

    for source in [
        "[[table]]\n[[row]]\n[[HCELL]]A[[/HCELL]]\n[[/row]]\n[[/table]]",
        "[[table]]\n[[row]]\n[[Hcell]]A[[/Hcell]]\n[[/row]]\n[[/table]]",
        "[[table]]\n[[row]]\n[[cell]]A[[/HCELL]]\n[[/row]]\n[[/table]]",
    ] {
        assert_eq!(
            render(source),
            "<table>\n<tr>\n<td>A</td>\n</tr>\n</table>",
            "{source:?}",
        );
    }

    assert_eq!(
        render("[[table]]\n[[row]]\n[[cell]]\n[[/cell]]\n[[/row]]\n[[/table]]"),
        "<table>\n<tr>\n<td></td>\n</tr>\n</table>",
    );
    for source in [
        "[[table]]\n[[/table]]",
        "[[table]]\n[[row]]\n[[/row]]\n[[/table]]",
    ] {
        assert!(!render(source).contains("<table>"), "{source:?}");
    }

    let nested = concat!(
        "[[table]][[row]][[cell]]O",
        "[[table]][[row]][[cell]]I[[/cell]][[/row]][[/table]]",
        "Z[[/cell]][[/row]][[/table]]",
    );
    let nested_html = render(nested);
    assert_eq!(nested_html.matches("<table>").count(), 2, "{nested_html}");
    assert!(nested_html.contains("<p>O</p><table>"), "{nested_html}");
    assert!(nested_html.contains("</table><p>Z</p>"), "{nested_html}");
}

#[test]
fn advanced_table_wikijump_layout_does_not_adopt_legacy_crossed_recovery() {
    let source = "[[table]][[row]][[cell]]A[[/hcell]][[/row]][[/table]]";
    let html = render_with_layout(source, Layout::Wikijump);
    assert_eq!(html, format!("<p>{source}</p>"));
    assert!(!html.contains("<td>"), "{html}");
    assert!(!html.contains("<th>"), "{html}");
}
