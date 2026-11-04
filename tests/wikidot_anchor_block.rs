use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender, text::TextRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn render(source: &str) -> (String, String) {
    let page_info = PageInfo {
        page: Cow::Borrowed("anchor-block"),
        category: None,
        site: Cow::Borrowed("syntax-differential"),
        title: Cow::Borrowed("Anchor block differential"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    };
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, _errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    (
        TextRender.render(&tree, &page_info, &settings),
        HtmlRender.render(&tree, &page_info, &settings).body,
    )
}

#[test]
fn wikidot_anchor_block_family_matches_live_boundaries() {
    let cases = [
        (
            "anchor-canonical-valid",
            "[[anchor]]v7 body[[/anchor]]",
            "<p>[[anchor]]v7 body[[/anchor]]</p>",
        ),
        ("anchor-incomplete-opening", "[[anchor", "<p>[[anchor</p>"),
        (
            "anchor-case-variation-name",
            "[[ANCHOR]]v7 body[[/ANCHOR]]",
            "<p>[[ANCHOR]]v7 body[[/ANCHOR]]</p>",
        ),
        (
            "anchor-whitespace-control",
            "[[anchor]]alpha\tbeta\u{a0}gamma[[/anchor]]",
            "<p>[[anchor]]alpha beta\u{a0}gamma[[/anchor]]</p>",
        ),
        (
            "anchor-boundary",
            "start-[[anchor]]v7 body[[/anchor]]-middle\n\n[[anchor]]end[[/anchor]]",
            "<p>start-[[anchor]]v7 body[[/anchor]]-middle</p><p>[[anchor]]end[[/anchor]]</p>",
        ),
        (
            "anchor-serialization-source-preservation",
            "[[anchor]]serialized body[[/anchor]]",
            "<p>[[anchor]]serialized body[[/anchor]]</p>",
        ),
        (
            "anchor-text-renderer-relevance",
            "[[anchor]]visible text[[/anchor]]",
            "<p>[[anchor]]visible text[[/anchor]]</p>",
        ),
        (
            "anchor-missing-close",
            "[[anchor]]unterminated body",
            "<p>[[anchor]]unterminated body</p>",
        ),
        (
            "anchor-nesting-same-feature",
            "[[anchor]][[anchor]]nested[[/anchor]][[/anchor]]",
            "<p>[[anchor]][[anchor]]nested[[/anchor]][[/anchor]]</p>",
        ),
        (
            "anchor-nesting-different-feature",
            "[[anchor]][[bold]]nested[[/bold]][[/anchor]]",
            "<p>[[anchor]][[bold]]nested[[/bold]][[/anchor]]</p>",
        ),
        (
            "anchor-invalid-overlap",
            "[[anchor]]outer [[bold]]inner[[/anchor]][[/bold]]",
            "<p>[[anchor]]outer [[bold]]inner[[/anchor]][[/bold]]</p>",
        ),
        (
            "anchor-duplicate-arguments",
            r#"[[anchor class="one" class="two"]]v7 body[[/anchor]]"#,
            "<p>[[anchor class=&quot;one&quot; class=&quot;two&quot;]]v7 body[[/anchor]]</p>",
        ),
        (
            "anchor-empty-arguments",
            r#"[[anchor class=""]]v7 body[[/anchor]]"#,
            "<p>[[anchor class=&quot;&quot;]]v7 body[[/anchor]]</p>",
        ),
        (
            "anchor-unknown-argument",
            r#"[[anchor v7UnknownArgument="x"]]v7 body[[/anchor]]"#,
            "<p>[[anchor v7UnknownArgument=&quot;x&quot;]]v7 body[[/anchor]]</p>",
        ),
        (
            "anchor-quote-variation",
            "[[anchor class='single quoted' data-v7=unquoted]]v7 body[[/anchor]]",
            "<p>[[anchor class=&#39;single quoted&#39; data-v7=unquoted]]v7 body[[/anchor]]</p>",
        ),
        (
            "anchor-unsafe-url",
            r#"[[anchor href="javascript:alert(1)"]]v7 body[[/anchor]]"#,
            "<p>[[anchor href=&quot;javascript:alert(1)&quot;]]v7 body[[/anchor]]</p>",
        ),
        (
            "anchor-percent-encoded",
            r#"[[anchor href="https://example.test/%6a%61vascript%3aalert(1)"]]v7 body[[/anchor]]"#,
            concat!(
                "<p>[[anchor href=&quot;<a href=\"https://example.test/%6a%61vascript%3aalert(1)\">",
                "https://example.test/%6a%61vascript%3aalert(1)</a>&quot;]]v7 body[[/anchor]]</p>",
            ),
        ),
        (
            "anchor-backslash",
            r#"[[anchor href="https:\\example.test\path"]]v7 body[[/anchor]]"#,
            r#"<p>[[anchor href=&quot;https:\\example.test\path&quot;]]v7 body[[/anchor]]</p>"#,
        ),
    ];
    assert_eq!(cases.len(), 18);
    for (case_id, source, expected) in cases {
        assert_eq!(render(source).1, expected, "{case_id}: {source:?}");
    }
}

#[test]
fn wikidot_a_block_family_matches_live_grammar_with_safe_urls() {
    let cases = [
        (
            "a-canonical-valid",
            "[[a]]v7 body[[/a]]",
            "<p><a href>v7 body</a></p>",
        ),
        ("a-incomplete-opening", "[[a", "<p>[[a</p>"),
        (
            "a-case-variation-name",
            "[[A]]v7 body[[/A]]",
            "<p><a href>v7 body</a></p>",
        ),
        (
            "a-whitespace-control",
            "[[a]]alpha\tbeta\u{a0}gamma[[/a]]",
            "<p><a href>alpha beta\u{a0}gamma</a></p>",
        ),
        (
            "a-boundary",
            "start-[[a]]v7 body[[/a]]-middle\n\n[[a]]end[[/a]]",
            "<p>start-<a href>v7 body</a>-middle</p><p><a href>end</a></p>",
        ),
        (
            "a-serialization-source-preservation",
            "[[a]]serialized body[[/a]]",
            "<p><a href>serialized body</a></p>",
        ),
        (
            "a-text-renderer-relevance",
            "[[a]]visible text[[/a]]",
            "<p><a href>visible text</a></p>",
        ),
        (
            "a-missing-close",
            "[[a]]unterminated body",
            "<p>[[a]]unterminated body</p>",
        ),
        (
            "a-nesting-same-feature",
            "[[a]][[a]]nested[[/a]][[/a]]",
            "<p><a href></a><a href>nested</a></p>",
        ),
        (
            "a-nesting-different-feature",
            "[[a]][[bold]]nested[[/bold]][[/a]]",
            "<p><a href>[[bold]]nested[[/bold]]</a></p>",
        ),
        (
            "a-invalid-overlap",
            "[[a]]outer [[bold]]inner[[/a]][[/bold]]",
            "<p><a href>outer [[bold]]inner</a>[[/bold]]</p>",
        ),
        (
            "a-duplicate-arguments",
            r#"[[a class="one" class="two"]]v7 body[[/a]]"#,
            "<p><a href class=\"two\">v7 body</a></p>",
        ),
        (
            "a-empty-arguments",
            r#"[[a class=""]]v7 body[[/a]]"#,
            "<p><a href>v7 body</a></p>",
        ),
        (
            "a-unknown-argument",
            r#"[[a v7UnknownArgument="x"]]v7 body[[/a]]"#,
            "<p><a href>v7 body</a></p>",
        ),
        (
            "a-quote-variation",
            "[[a class='single quoted' data-v7=unquoted]]v7 body[[/a]]",
            "<p><a href>v7 body</a></p>",
        ),
        (
            "a-unsafe-url",
            r#"[[a href="javascript:alert(1)"]]v7 body[[/a]]"#,
            "<p><a href=\"#invalid-url\">v7 body</a></p>",
        ),
        (
            "a-percent-encoded",
            r#"[[a href="https://example.test/%6a%61vascript%3aalert(1)"]]v7 body[[/a]]"#,
            "<p><a href=\"https://example.test/%6a%61vascript%3aalert(1)\">v7 body</a></p>",
        ),
        (
            "a-backslash",
            r#"[[a href="https:\\example.test\path"]]v7 body[[/a]]"#,
            "<p><a href=\"https:example-testpath\">v7 body</a></p>",
        ),
    ];
    assert_eq!(cases.len(), 18);
    for (case_id, source, expected) in cases {
        assert_eq!(render(source).1, expected, "{case_id}: {source:?}");
    }
}

#[test]
fn wikidot_literal_anchor_preserves_text_renderer_source() {
    let source = "[[anchor]]visible [[span]]text[[/span]][[/anchor]]";
    assert_eq!(render(source).0, "[[anchor]]visible text[[/anchor]]");
}

#[test]
fn wikidot_rejected_anchor_candidates_create_no_attributes() {
    let source = concat!(
        r#"[[anchor href="javascript:alert(1)" onclick="alert(2)"]]body[[/anchor]]"#,
        r#"[[a onclick="alert(3)"]]active[[/a]]"#,
    );
    let (_, html) = render(source);
    assert!(html.contains("[[anchor href=&quot;javascript:alert(1)&quot; onclick="));
    assert!(html.contains("<a href>active</a>"));
    assert!(!html.contains(r#" onclick=""#));
    assert!(!html.contains("href=\"javascript:"));
}

#[test]
fn wikidot_literal_anchor_rollback_is_bounded() {
    let source = "[[anchor class=\"x\"]]body[[/anchor]]".repeat(2_000);
    let started = Instant::now();
    let (_, html) = render(&source);
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(html.matches("[[anchor class=&quot;x&quot;]]").count(), 1);
    assert_eq!(
        html.matches("<a href=\"/anchor]][[anchor\">").count(),
        1_999
    );
}
