use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn render(source: &str, layout: Layout) -> String {
    let page_info = PageInfo {
        page: Cow::Borrowed("anchor-link-grammar"),
        category: None,
        site: Cow::Borrowed("syntax-differential"),
        title: Cow::Borrowed("Anchor-link grammar"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    };
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, _) = ftml::parse(&tokenization, &page_info, &settings).into();
    HtmlRender.render(&tree, &page_info, &settings).body
}

fn render_wikidot(source: &str) -> String {
    render(source, Layout::Wikidot)
}

#[test]
fn live_backed_anchor_link_matrix_matches_exact_html_and_source_recovery() {
    let cases = [
        (
            "canonical",
            "[#toc1 Label]",
            r##"<p><a href="#toc1">Label</a></p>"##,
        ),
        (
            "star",
            "[*#toc1 Label]",
            r##"<p><a href="#toc1" target="_blank">Label</a></p>"##,
        ),
        (
            "empty-anchor",
            "[# Label]",
            r#"<p><a href="javascript:;">Label</a></p>"#,
        ),
        ("empty-label", "[#toc1 ]", "<p>[#toc1 ]</p>"),
        ("no-label", "[#toc1]", "<p>[#toc1]</p>"),
        ("space-before", "[ #toc1 Label]", "<p>[ #toc1 Label]</p>"),
        (
            "upper",
            "[#TOC1 Label]",
            r##"<p><a href="#TOC1">Label</a></p>"##,
        ),
        ("unicode", "[#日本語 Label]", "<p>[#日本語 Label]</p>"),
        ("punctuation", "[#a-b_c.1 Label]", "<p>[#a-b_c.1 Label]</p>"),
        ("newline", "[#toc1\nLabel]", "<p>[#toc1<br>\nLabel]</p>"),
        (
            "nested-label-delimiter",
            "[#toc1 **A**]",
            r##"<p><a href="#toc1">**A**</a></p>"##,
        ),
        (
            "comment-label",
            "[#toc1 A[!--x--]B]",
            r##"<p><a href="#toc1">AB</a></p>"##,
        ),
        ("unclosed", "[#toc1 Label", "<p>[#toc1 Label</p>"),
    ];

    for (case_id, source, expected) in cases {
        assert_eq!(render_wikidot(source), expected, "{case_id}: {source:?}");
    }
}

#[test]
fn anchor_identity_and_escaping_remain_fragment_safe() {
    assert_eq!(
        render_wikidot("[#TOC1 One][#TOC1 Two]"),
        concat!(
            r##"<p><a href="#TOC1">One</a>"##,
            r##"<a href="#TOC1">Two</a></p>"##,
        ),
    );
    assert_eq!(
        render_wikidot("[#toc1 <tag>&\"']"),
        r##"<p><a href="#toc1">&lt;tag&gt;&amp;&quot;&#39;</a></p>"##,
    );

    for source in [
        "[#toc1\"onclick Label]",
        "[*#toc1\"onclick Label]",
        "[#日本語<script Label]",
    ] {
        let html = render_wikidot(source);
        assert!(!html.contains("<a "), "{source:?}: {html}");
        assert!(!html.contains("onclick="), "{source:?}: {html}");
        assert!(!html.contains("<script"), "{source:?}: {html}");
    }
}

#[test]
fn wikijump_anchor_grammar_remains_unchanged() {
    assert_eq!(
        render("[#A_B.C Label]", Layout::Wikijump),
        concat!(
            r#"<p><a class="wj-link wj-link-anchor" data-link-type="anchor" "#,
            r##"href="#a-b-c">Label</a></p>"##,
        ),
    );
    assert_eq!(
        render("[#toc1 ]", Layout::Wikijump),
        concat!(
            r#"<p><a class="wj-link wj-link-anchor" data-link-type="anchor" "#,
            r##"href="#toc1"></a></p>"##,
        ),
    );
    assert_eq!(
        render("[*#toc1 Label]", Layout::Wikijump),
        "<p>[*#toc1 Label]</p>",
    );
}

#[test]
fn long_anchors_and_malformed_bracket_runs_stay_bounded() {
    let long_anchor = "A".repeat(16 * 1024);
    let bracket_run = "]".repeat(8 * 1024);
    let invalid_dense = "[#a-b_c.1 Label]".repeat(2_048);
    let source =
        format!("[#{long_anchor} Label] [#toc1 Label{bracket_run}] {invalid_dense}",);

    let started = Instant::now();
    let html = render_wikidot(&source);
    let elapsed = started.elapsed();

    assert!(html.starts_with(r##"<p><a href="#AAAA"##), "{html:.200}");
    assert!(html.contains(">Label</a>"), "{html:.200}");
    assert!(html.len() <= source.len() * 2, "{}", html.len());
    assert!(
        elapsed < Duration::from_secs(3),
        "anchor-link parsing took {elapsed:?}",
    );
}
