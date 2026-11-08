use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("crossed-closers-2568"),
        category: None,
        site: Cow::Borrowed("scp-wiki"),
        title: Cow::Borrowed("Crossed Closers 2568"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn render(source: &str) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, Layout::Wikidot);
    let tokenization = ftml::tokenize(&source);
    let (tree, _errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    HtmlRender.render(&tree, &page_info, &settings).body
}

fn assert_no_literals(html: &str, markers: &[&str]) {
    for marker in markers {
        assert!(!html.contains(marker), "literal {marker} in html: {html}");
    }
}

const A1_MARKERS: &[&str] = &["[[=]]", "[[/=]]", "[[/div]]", "[[/collapsible]]"];

#[test]
fn a1_center_close_inside_collapsible_pairs_in_stream_order() {
    let html = render(concat!(
        "[[=]]\n",
        "[[div class=\"a\"]]\n",
        "x\n",
        "[[collapsible show=\"s\" hide=\"h\"]]\n",
        "[[/=]]\n",
        "[[div class=\"b\"]]\n",
        "y\n",
        "[[/div]]\n",
        "[[/collapsible]]\n",
        "[[/div]]\n",
    ));
    assert_no_literals(&html, A1_MARKERS);
    assert!(html.contains("class=\"collapsible-block\""), "html: {html}");
    assert!(
        html.contains("<div class=\"collapsible-block-content\"></div>"),
        "html: {html}"
    );
    assert!(html.contains("class=\"b\""), "html: {html}");
    assert!(html.contains("<p>y</p>"), "html: {html}");
}

#[test]
fn a1_content_between_collapsible_and_wrapper_closes() {
    let html = render(concat!(
        "[[=]]\n",
        "[[div class=\"a\"]]\n",
        "x\n",
        "[[collapsible show=\"s\" hide=\"h\"]]\n",
        "[[/=]]\n",
        "[[div class=\"b\"]]\n",
        "y\n",
        "[[/div]]\n",
        "[[/collapsible]]\n",
        "loose\n",
        "[[/div]]\n",
    ));
    assert_no_literals(&html, A1_MARKERS);
    assert!(html.contains("class=\"b\""), "html: {html}");
    assert!(html.contains("loose"), "html: {html}");
    let loose_at = html.find("loose").expect("loose present");
    let collapsible_at = html
        .find("collapsible-block-content")
        .expect("collapsible present");
    assert!(loose_at > collapsible_at, "html: {html}");
}

#[test]
fn a1_nested_collapsible_in_crossed_tail() {
    let html = render(concat!(
        "[[=]]\n",
        "[[div class=\"a\"]]\n",
        "x\n",
        "[[collapsible show=\"s\" hide=\"h\"]]\n",
        "[[/=]]\n",
        "[[div class=\"b\"]]\n",
        "[[collapsible show=\"s2\"]]\n",
        "z\n",
        "[[/collapsible]]\n",
        "[[/div]]\n",
        "[[/collapsible]]\n",
        "[[/div]]\n",
    ));
    assert!(!html.contains("[[=]]"), "html: {html}");
    assert!(!html.contains("[[/=]]"), "html: {html}");
    assert_eq!(
        html.matches("class=\"collapsible-block\"").count(),
        1,
        "html: {html}"
    );
    assert!(
        html.contains("<div class=\"collapsible-block-content\"></div>"),
        "html: {html}"
    );
    assert!(html.contains("z"), "html: {html}");
}

const A2_MARKERS: &[&str] = &[
    "[[/li]]",
    "[[/ul]]",
    "[[/ol]]",
    "[[/div]]",
    "[[/collapsible]]",
];

fn assert_list_closed_before(html: &str, list_close: &str, tail: &str) {
    let list_end = html.find(list_close).expect("list close tag present");
    let tail_at = html.find(tail).expect("tail text present");
    assert!(
        list_end < tail_at,
        "{list_close} should precede {tail:?} in html: {html}"
    );
}

#[test]
fn a2_div_crossing_list_closers() {
    let html = render(concat!(
        "[[ul]]\n",
        "[[li]]\n",
        "start\n",
        "[[div]]\n",
        "inner\n",
        "[[/li]]\n",
        "[[/ul]]\n",
        "[[/div]]\n",
        "after\n",
    ));
    assert_no_literals(&html, A2_MARKERS);
    // Item 78 / probe muse-probe-adj1108-a2-div-cross
    // (closeout-verify/adjudge-1108/probe.json): div body cross-terminated by
    // [[/li]] keeps PR #1242's trailing break (`inner<br`), the list closes
    // before the follower, and the immediate text follower is bare with a
    // leading `<br />` (not wrapped in `<p>`). Internal whitespace/`<br />`
    // style may differ from live's raw formatting.
    assert!(html.contains("<div><p>inner<br"), "html: {html}");
    assert_list_closed_before(&html, "</ul>", "after");
    let ul_end = html.find("</ul>").expect("ul close");
    let after_at = html.find("after").expect("after");
    let between = &html[ul_end..after_at];
    assert!(between.contains("<br"), "expected <br before bare after: {html}");
    assert!(!html.contains("<p>after</p>"), "follower must stay bare: {html}");
}

#[test]
fn a2_collapsible_crossing_list_closers() {
    let html = render(concat!(
        "[[ul]]\n",
        "[[li]]\n",
        "start\n",
        "[[collapsible show=\"s\"]]\n",
        "inner\n",
        "[[/li]]\n",
        "[[/ul]]\n",
        "[[/collapsible]]\n",
        "after\n",
    ));
    assert_no_literals(&html, A2_MARKERS);
    assert!(html.contains("collapsible-block-content"), "html: {html}");
    assert!(html.contains("inner"), "html: {html}");
    assert_list_closed_before(&html, "</ul>", "after");
    let ul_end = html.find("</ul>").expect("ul close");
    let after_at = html.find("after").expect("after");
    assert!(
        html[ul_end..after_at].contains("<br"),
        "expected <br before bare after: {html}"
    );
    assert!(!html.contains("<p>after</p>"), "follower must stay bare: {html}");
}

#[test]
fn a2_div_crossing_ordered_list_closers() {
    let html = render(concat!(
        "[[ol]]\n",
        "[[li]]\n",
        "start\n",
        "[[div]]\n",
        "inner\n",
        "[[/li]]\n",
        "[[/ol]]\n",
        "[[/div]]\n",
        "after\n",
    ));
    assert_no_literals(&html, A2_MARKERS);
    assert_list_closed_before(&html, "</ol>", "after");
    let ol_end = html.find("</ol>").expect("ol close");
    let after_at = html.find("after").expect("after");
    assert!(
        html[ol_end..after_at].contains("<br"),
        "expected <br before bare after: {html}"
    );
    assert!(!html.contains("<p>after</p>"), "follower must stay bare: {html}");
}

#[test]
fn a2_second_list_item_before_list_close() {
    let html = render(concat!(
        "[[ul]]\n",
        "[[li]]\n",
        "first\n",
        "[[li]]\n",
        "second\n",
        "[[/li]]\n",
        "[[/ul]]\n",
        "after\n",
    ));
    assert_no_literals(&html, A2_MARKERS);
    assert!(html.contains("first"), "html: {html}");
    assert!(html.contains("second"), "html: {html}");
    assert_list_closed_before(&html, "</ul>", "after");
}

#[test]
fn a2_list_close_terminates_open_item() {
    let html = render(concat!(
        "[[ul]]\n",
        "[[li]]\n",
        "foo\n",
        "[[/ul]]\n",
        "after\n",
    ));
    assert!(html.contains("[[li]]"), "html: {html}");
    assert!(html.contains("list-style: none"), "html: {html}");
    assert!(!html.contains("[[ul]]"), "html: {html}");
    assert!(html.contains("<li"), "html: {html}");
    assert!(html.contains("foo"), "html: {html}");
    assert_list_closed_before(&html, "</ul>", "after");
}

#[test]
fn a2_normal_nesting_unchanged() {
    let html = render(concat!(
        "[[ul]]\n",
        "[[li]]\n",
        "start\n",
        "[[div]]\n",
        "inner\n",
        "[[/div]]\n",
        "[[/li]]\n",
        "[[/ul]]\n",
        "after\n",
    ));
    assert_no_literals(&html, A2_MARKERS);
    assert!(html.contains("<div><p>inner</p></div>"), "html: {html}");
    assert_list_closed_before(&html, "</ul>", "after");
}
