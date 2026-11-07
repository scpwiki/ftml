use ftml::data::{PageInfo, PageRef, ScoreValue};
use ftml::delayed::{
    DelayedInput, GeneratedInput, GeneratedKind, GeneratedValue, InputSegment,
    SlotBindings, SlotId, TextOrigin, parse_delayed_list,
};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("syntax-differential"),
        category: None,
        site: Cow::Borrowed("syntax-differential"),
        title: Cow::Borrowed("Inline bibcite"),
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
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    assert!(errors.is_empty(), "{source:?}: {errors:#?}");
    HtmlRender.render(&tree, &page_info, &settings).body
}

#[test]
fn live_backed_twenty_case_inline_bibcite_family_stays_fixed() {
    let cases = [
        (
            "adjacent",
            "((bibcite a))((bibcite b))",
            "</span><span class=\"error-inline\">",
            2,
        ),
        (
            "basic",
            "A((bibcite alpha))B",
            "<em>alpha</em> not found.</span>B",
            1,
        ),
        ("closer-only", "alpha))", "<p>alpha))</p>", 0),
        (
            "code-owner",
            "[[code]]\n((bibcite alpha))\n[[/code]]",
            "<pre><code>((bibcite alpha))</code></pre>",
            0,
        ),
        (
            "duplicate-word",
            "((bibcite bibcite alpha))",
            "((bibcite bibcite alpha))",
            0,
        ),
        ("empty-label", "((bibcite ))", "<p>((bibcite ))</p>", 0),
        (
            "escaped",
            "\\((bibcite alpha))",
            "<em>alpha</em> not found.",
            1,
        ),
        (
            "external-link-label",
            "[https://example.com ((bibcite alpha))]",
            "[<a href=\"https://example.com\">https://example.com</a> ",
            1,
        ),
        (
            "heading",
            "+ ((bibcite alpha))",
            "<h1 id=\"toc0\"><span><span class=\"error-inline\">",
            1,
        ),
        (
            "internal-link-label",
            "[[[scp-002|((bibcite alpha))]]]",
            ">((bibcite alpha))</a>",
            0,
        ),
        (
            "list",
            "* ((bibcite alpha))",
            "<li><span class=\"error-inline\">",
            1,
        ),
        (
            "missing-close",
            "((bibcite alpha",
            "<p>((bibcite alpha</p>",
            0,
        ),
        (
            "nested-format",
            "((bibcite **alpha**))",
            "((bibcite <strong>alpha</strong>))",
            0,
        ),
        ("no-space", "((bibcitealpha))", "<p>((bibcitealpha))</p>", 0),
        (
            "paragraph-break",
            "((bibcite alpha\n\nbeta))",
            "<p>beta))</p>",
            0,
        ),
        (
            "raw-owner",
            "@@((bibcite alpha))@@",
            "((bibcite&#32;alpha))",
            0,
        ),
        (
            "single-newline",
            "((bibcite alpha\nbeta))",
            "((bibcite alpha<br>",
            0,
        ),
        (
            "span-attribute",
            "[[span title=\"((bibcite alpha))\"]]X[[/span]]",
            ">X</span>",
            0,
        ),
        (
            "table",
            "|| ((bibcite alpha)) ||",
            "<td><span class=\"error-inline\">",
            1,
        ),
        (
            "unicode",
            "((bibcite 日本語🙂))",
            "<p>((bibcite 日本語🙂))</p>",
            0,
        ),
    ];

    for (case_id, source, expected_fragment, diagnostic_count) in cases {
        let html = render(source);
        assert!(
            html.contains(expected_fragment),
            "{case_id}: missing {expected_fragment:?} in {html}",
        );
        assert_eq!(
            html.matches("class=\"error-inline\"").count(),
            diagnostic_count,
            "{case_id}: {html}",
        );
    }
}

#[test]
fn valid_labels_keep_missing_item_diagnostics_and_bibliography_integration() {
    for (source, label) in [
        ("((bibcite alpha))", "alpha"),
        ("((bibcite   A9))", "A9"),
        ("((bibcite\tTabbed))", "Tabbed"),
    ] {
        let html = render(source);
        assert_eq!(html.matches("class=\"error-inline\"").count(), 1, "{html}");
        assert!(html.contains(&format!("<em>{label}</em>")), "{html}");
    }

    let html = render(concat!(
        "[[bibliography]]\n",
        ": alpha : Alpha & source\n",
        "[[/bibliography]]\n",
        "((bibcite alpha))",
    ));
    assert!(!html.contains("error-inline"), "{html}");
    assert!(html.contains("class=\"bibcite\""), "{html}");
    assert!(
        html.contains("class=\"bibitem\" id=\"bibitem-1\""),
        "{html}"
    );
    assert!(html.contains("Alpha &amp; source"), "{html}");
}

#[test]
fn invalid_labels_remain_ordinary_escaped_syntax() {
    for (source, expected_fragment) in [
        ("((bibcite alpha beta))", "((bibcite alpha beta))"),
        ("((bibcite 日本語))", "((bibcite 日本語))"),
        (
            "((bibcite **alpha**))",
            "((bibcite <strong>alpha</strong>))",
        ),
        ("((bibcite alpha|beta))", "((bibcite alpha|beta))"),
        ("((BIBCITE alpha_2))", "((BIBCITE alpha_2))"),
        ("((bibcite smith-2020))", "((bibcite smith-2020))"),
        ("((bibcite alpha[beta]))", "((bibcite alpha[beta]))"),
    ] {
        let html = render(source);
        assert!(!html.contains("error-inline"), "{source:?}: {html}");
        assert!(html.contains(expected_fragment), "{source:?}: {html}");
    }

    let html = render("((bibcite alpha[!--hidden--]))");
    assert!(html.contains("error-inline"), "{html}");
    assert!(html.contains("<em>alpha</em>"), "{html}");

    let html = render("((bibcite <script>alert_1</script>))");
    assert!(!html.contains("<script>"), "{html}");
    assert!(html.contains("&lt;script&gt;"), "{html}");
    assert!(!html.contains("error-inline"), "{html}");
}

fn render_delayed(input: &DelayedInput<'_>, bindings: &SlotBindings<'_>) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
    let delayed = parse_delayed_list(input, &page_info, &settings)
        .expect("supported delayed bibcite fixture");
    let bound = delayed.bind(bindings).expect("matching delayed binding");
    bound.render_html(&page_info, &settings).body().to_owned()
}

#[test]
fn delayed_provenance_keeps_authored_owners_active_and_runtime_data_inert() {
    let source = "[https://example.com ((bibcite alpha))]";
    let authored = DelayedInput::new(
        source,
        vec![InputSegment::text(0..source.len(), TextOrigin::Authored)],
    )
    .expect("valid authored fixture");
    let authored_html = render_delayed(&authored, &SlotBindings::empty());
    assert!(authored_html.starts_with("<p>[<a href=\"https://example.com\">"));
    assert_eq!(authored_html.matches("error-inline").count(), 1);

    let runtime = DelayedInput::new(
        source,
        vec![InputSegment::text(
            0..source.len(),
            TextOrigin::RuntimeScalar,
        )],
    )
    .expect("valid runtime fixture");
    assert_eq!(
        render_delayed(&runtime, &SlotBindings::empty()),
        "<p>[https://example.com ((bibcite alpha))]</p>",
    );

    let source = "[https://example.com ((bibcite %%page%%))]";
    let marker_start = source.find("%%page%%").expect("fixture marker");
    let marker_end = marker_start + "%%page%%".len();
    let generated = DelayedInput::new(
        source,
        vec![
            InputSegment::text(0..marker_start, TextOrigin::Authored),
            InputSegment::generated(GeneratedInput {
                source_range: marker_start..marker_end,
                id: SlotId::new(1),
                kind: GeneratedKind::PageLink,
                occurrence: 0,
            }),
            InputSegment::text(marker_end..source.len(), TextOrigin::Authored),
        ],
    )
    .expect("valid generated fixture");
    let bindings = SlotBindings::new(vec![(
        SlotId::new(1),
        GeneratedValue::PageLink {
            page: PageRef::page_only("component:reference"),
            label: Cow::Borrowed("Generated reference"),
        },
    )])
    .expect("unique generated binding");
    let generated_html = render_delayed(&generated, &bindings);
    assert!(!generated_html.contains("error-inline"), "{generated_html}");
    assert!(generated_html.contains("((bibcite "), "{generated_html}");
    assert!(
        generated_html.contains("Generated reference"),
        "{generated_html}"
    );
}

#[test]
fn bibcite_anchor_id_is_stable_across_renders() {
    let source = "((bibcite b1))\n\n[[bibliography]]\n: b1 : Title\n[[/bibliography]]\n";
    let first = render(source);
    let second = render(source);
    assert_eq!(first, second, "bibcite id changed between renders");
    assert!(first.contains("id=\"bibcite-1-"), "{first}");
}

#[test]
fn wikidot_anchor_closes_before_bibcite() {
    let source = "[[a href=\"/A\"]]x((bibcite b1))y[[/a]]\n\n[[bibliography]]\n: b1 : Title\n[[/bibliography]]\n";
    let html = render(source);
    assert!(
        html.contains("</a><a href=\"javascript:;\" class=\"bibcite\"")
            || html.contains("</a><a class=\"bibcite\" href=\"javascript:;\""),
        "outer anchor still wraps the cite: {html}"
    );
    assert!(
        html.contains(">1</a>y</p>"),
        "text after the cite should sit outside the outer anchor: {html}"
    );

    let literal = render(
        "[[a href=\"/A\"]]x[[bibcite b1]]y[[/a]]\n\n[[bibliography]]\n: b1 : Title\n[[/bibliography]]\n",
    );
    assert!(
        literal.contains(">x[[bibcite b1]]y</a>"),
        "block bibcite is not an anchor producer: {literal}"
    );
}
