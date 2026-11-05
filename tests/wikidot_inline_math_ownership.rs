use ftml::data::{PageInfo, PageRef, ScoreValue};
use ftml::delayed::{
    DelayedInput, GeneratedInput, GeneratedKind, GeneratedValue, InputSegment,
    SlotBindings, SlotId, TextOrigin, parse_delayed_list,
};
use ftml::layout::Layout;
use ftml::parsing::{ParseError, ParseErrorKind};
use ftml::render::{Render, html::HtmlRender, text::TextRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("inline-math-ownership"),
        category: None,
        site: Cow::Borrowed("syntax-differential"),
        title: Cow::Borrowed("Inline math ownership"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn render_with_layout(source: &str, layout: Layout) -> (String, String, Vec<ParseError>) {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;
    let text = TextRender.render(&tree, &page_info, &settings);
    (html, text, errors)
}

fn render(source: &str) -> (String, String, Vec<ParseError>) {
    render_with_layout(source, Layout::Wikidot)
}

fn assert_error(errors: &[ParseError], kind: ParseErrorKind, source: &str) {
    assert!(
        errors.iter().any(|error| error.kind() == kind),
        "{source:?}: missing {kind:?} in {errors:#?}",
    );
}

#[test]
fn canonical_twenty_case_inline_math_family_stays_fixed() {
    let cases = [
        (
            "adjacent",
            "[[$x$]][[$y$]]",
            r#"<p><span class="math-inline">$x$</span><span class="math-inline">$y$</span></p>"#,
            "",
        ),
        (
            "basic",
            "A[[$x+1$]]B",
            r#"<p>A<span class="math-inline">$x+1$</span>B</p>"#,
            "AB",
        ),
        ("closer-only", "x+1$]]", "<p>x+1$]]</p>", "x+1$]]"),
        (
            "code-owner",
            "[[code]]\n[[$x$]]\n[[/code]]",
            "<div class=\"code\"><pre><code>[[$x$]]</code></pre></div>",
            "[[$x$]]",
        ),
        (
            "empty",
            "[[$$]]",
            r#"<p><span class="math-inline">$$</span></p>"#,
            "",
        ),
        (
            "escaped-control",
            "\\[[$x$]]",
            r#"<p>\<span class="math-inline">$x$</span></p>"#,
            "\\",
        ),
        (
            "explicit-link-label",
            "[https://example.com [[$x$]]]",
            concat!(
                r#"<p>[<a href="https://example.com">https://example.com</a> "#,
                r#"<span class="math-inline">$x$</span>]</p>"#,
            ),
            "[https://example.com ]",
        ),
        (
            "extra-close",
            "[[$x$]]$]]",
            r#"<p><span class="math-inline">$x$</span>$]]</p>"#,
            "$]]",
        ),
        (
            "heading",
            "+ [[$x$]]",
            r#"<h1 id="toc0"><span><span class="math-inline">$x$</span></span></h1>"#,
            "",
        ),
        (
            "triple-link-label",
            "[[[scp-002|[[$x$]]]]]",
            r#"<p>[[[scp-002|<span class="math-inline">$x$</span>]]]</p>"#,
            "[[[scp-002|]]]",
        ),
        (
            "list",
            "* [[$x$]]",
            "<ul>\n<li><span class=\"math-inline\">$x$</span></li>\n</ul>",
            "",
        ),
        (
            "nested-open",
            "[[$A[[$B$]]C$]]",
            r#"<p><span class="math-inline">$A[[$B$</span>C$]]</p>"#,
            "C$]]",
        ),
        (
            "padded",
            "[[$ x + 1 $]]",
            r#"<p><span class="math-inline">$x + 1$</span></p>"#,
            "",
        ),
        (
            "paragraph-break",
            "[[$A\n\nB$]]",
            "<p>[[$A</p><p>B$]]</p>",
            "[[$A\n\nB$]]",
        ),
        (
            "raw-owner",
            "@@[[$x$]]@@",
            r#"<p><span style="white-space: pre-wrap;">[[$x$]]</span></p>"#,
            "[[$x$]]",
        ),
        (
            "single-newline",
            "[[$A\nB$]]",
            r#"<p><span class="math-inline">$$</span></p>"#,
            "",
        ),
        (
            "space-only",
            "[[$ $]]",
            r#"<p><span class="math-inline">$$</span></p>"#,
            "",
        ),
        (
            "table",
            "|| [[$x$]] ||",
            concat!(
                "<table class=\"wiki-content-table\">\n<tr>\n<td>",
                "<span class=\"math-inline\">$x$</span>",
                "</td>\n</tr>\n</table>",
            ),
            "",
        ),
        ("unclosed", "[[$x+1", "<p>[[$x+1</p>", "[[$x+1"),
        (
            "unicode",
            "[[$日本語+🙂$]]",
            r#"<p><span class="math-inline">$日本語+🙂$</span></p>"#,
            "",
        ),
    ];

    assert_eq!(cases.len(), 20);
    for (case_id, source, expected_html, expected_text) in cases {
        let (html, text, _) = render(source);
        assert_eq!(html, expected_html, "{case_id}: {source:?}");
        assert_eq!(text, expected_text, "{case_id}: {source:?}: {html}");
    }
}

#[test]
fn newline_comment_and_closer_recovery_matches_live_boundaries() {
    for (case_id, source, expected_html) in [
        (
            "two-separated-newlines",
            "[[$A\nB\nC$]]",
            r#"<p><span class="math-inline">$$</span></p>"#,
        ),
        (
            "single-crlf",
            "[[$A\r\nB$]]",
            r#"<p><span class="math-inline">$$</span></p>"#,
        ),
        (
            "paragraph-crlf",
            "[[$A\r\n\r\nB$]]",
            "<p>[[$A</p><p>B$]]</p>",
        ),
        (
            "comment-elision",
            "[[$A[!--hidden--]B$]]",
            r#"<p><span class="math-inline">$AB$</span></p>"#,
        ),
        (
            "comment-hides-close",
            "[[$A[!--$]]--]B$]]",
            r#"<p><span class="math-inline">$AB$</span></p>"#,
        ),
        (
            "multiline-comment-elision",
            "[[$A[!--hidden\ncomment--]B$]]",
            r#"<p><span class="math-inline">$AB$</span></p>"#,
        ),
        (
            "malformed-comment-is-formula-text",
            "[[$A[!--hidden$]]",
            r#"<p><span class="math-inline">$A[!--hidden$</span></p>"#,
        ),
        (
            "comment-joins-close-after-dollar",
            "[[$A$[!--hidden--]]]",
            r#"<p><span class="math-inline">$A$</span></p>"#,
        ),
        (
            "comment-joins-close-between-brackets",
            "[[$A$][!--hidden--]]",
            r#"<p><span class="math-inline">$A$</span></p>"#,
        ),
        (
            "comments-split-both-close-boundaries",
            "[[$A$[!--one--]][!--two--]]",
            r#"<p><span class="math-inline">$A$</span></p>"#,
        ),
        (
            "short-close-is-content",
            "[[$A$]B$]]",
            r#"<p><span class="math-inline">$A$]B$</span></p>"#,
        ),
        (
            "bracket-run",
            "[[$x$]]]]]]",
            r#"<p><span class="math-inline">$x$</span>]]]]</p>"#,
        ),
    ] {
        let (html, _, _) = render(source);
        assert_eq!(html, expected_html, "{case_id}: {source:?}");
    }

    let unclosed = "[[$x+1";
    let (_, _, errors) = render(unclosed);
    assert_error(&errors, ParseErrorKind::EndOfInput, unclosed);
}

#[test]
fn complete_math_owns_only_authored_link_label_candidates() {
    for (source, expected_html, reports_rollback) in [
        (
            "[https://example.com A[[$x$]]B]",
            concat!(
                r#"<p>[<a href="https://example.com">https://example.com</a> A"#,
                r#"<span class="math-inline">$x$</span>B]</p>"#,
            ),
            false,
        ),
        (
            "[https://example.com [!--hidden--][[$x$]]]",
            concat!(
                r#"<p>[<a href="https://example.com">https://example.com</a> "#,
                r#"<span class="math-inline">$x$</span>]</p>"#,
            ),
            false,
        ),
        (
            "[https://example.com [[$A[!--$]]--]B$]]]",
            concat!(
                r#"<p>[<a href="https://example.com">https://example.com</a> "#,
                r#"<span class="math-inline">$AB$</span>]</p>"#,
            ),
            false,
        ),
        (
            "[https://example.com [[$A\nB$]]]",
            concat!(
                r#"<p>[<a href="https://example.com">https://example.com</a> "#,
                r#"<span class="math-inline">$$</span>]</p>"#,
            ),
            false,
        ),
        (
            "[https://example.com [[$A$[!--hidden--]]]]",
            concat!(
                r#"<p>[<a href="https://example.com">https://example.com</a> "#,
                r#"<span class="math-inline">$A$</span>]</p>"#,
            ),
            false,
        ),
        (
            "[[[scp-002|A[[$x$]]B]]]",
            r#"<p>[[[scp-002|A<span class="math-inline">$x$</span>B]]]</p>"#,
            true,
        ),
        (
            "[[[scp-002|A[[$x$[!--hidden--]]]B]]]",
            r#"<p>[[[scp-002|A<span class="math-inline">$x$</span>B]]]</p>"#,
            true,
        ),
    ] {
        let (html, _, errors) = render(source);
        assert_eq!(html, expected_html, "{source:?}");
        if reports_rollback {
            assert_error(&errors, ParseErrorKind::RuleFailed, source);
            assert_error(&errors, ParseErrorKind::NoRulesMatch, source);
        } else {
            assert!(errors.is_empty(), "{source:?}: {errors:#?}");
        }
    }

    let commented = "[https://example.com [!--[[$x$]]--]Label]";
    let (html, _, errors) = render(commented);
    assert_eq!(html, r#"<p><a href="https://example.com">Label</a></p>"#,);
    assert!(errors.is_empty(), "{errors:#?}");

    let malformed = "[https://example.com [[$x] Label]";
    let (html, _, _) = render(malformed);
    assert_eq!(
        html,
        r#"<p><a href="https://example.com">[[$x</a> Label]</p>"#,
    );
}

#[test]
fn formula_bytes_are_inert_escaped_and_do_not_renumber_footnotes() {
    let source = concat!(
        "[[footnote]]P[[/footnote]]",
        "[https://example.com [[$<script>**X**[[html]]Y[[/html]]",
        "[[footnote]]hidden[[/footnote]]$]]]",
        "[[footnote]]Q[[/footnote]]",
    );
    let (html, _, errors) = render(source);
    assert!(errors.is_empty(), "{errors:#?}");

    assert!(!html.contains("<script>"), "{html}");
    assert!(!html.contains("<strong>"), "{html}");
    assert!(!html.contains("<iframe"), "{html}");
    assert!(html.contains("&lt;script&gt;"), "{html}");
    assert!(html.contains("**X**[[html]]Y[[/html]]"), "{html}");
    assert!(!html.contains("hidden</div>"), "{html}");
    assert_eq!(
        html.matches("class=\"footnote-footer\"").count(),
        2,
        "{html}"
    );
    assert!(html.contains("id=\"footnoteref-1\""), "{html}");
    assert!(html.contains("id=\"footnoteref-2\""), "{html}");
    assert!(!html.contains("id=\"footnoteref-3\""), "{html}");
    assert!(html.contains(">1</a>. P</div>"), "{html}");
    assert!(html.contains(">2</a>. Q</div>"), "{html}");

    for source in [
        "[javascript:alert(1) [[$x$]]]",
        "[data:text/html,x [[$x$]]]",
        "[vbscript:msgbox(1) [[$x$]]]",
    ] {
        let (html, _, _) = render(source);
        assert!(!html.contains("href=\"javascript:"), "{source:?}: {html}");
        assert!(!html.contains("href=\"data:"), "{source:?}: {html}");
        assert!(!html.contains("href=\"vbscript:"), "{source:?}: {html}");
    }
}

fn render_delayed(input: &DelayedInput<'_>, bindings: &SlotBindings<'_>) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
    let delayed = parse_delayed_list(input, &page_info, &settings)
        .expect("supported delayed inline-math fixture");
    let bound = delayed.bind(bindings).expect("matching delayed binding");
    bound.render_html(&page_info, &settings).body().to_owned()
}

#[test]
fn delayed_and_runtime_values_keep_their_provenance() {
    let authored_source = "[[$x$]]";
    let authored = DelayedInput::new(
        authored_source,
        vec![InputSegment::text(
            0..authored_source.len(),
            TextOrigin::Authored,
        )],
    )
    .expect("valid authored fixture");
    assert_eq!(
        render_delayed(&authored, &SlotBindings::empty()),
        r#"<p><span class="math-inline">$x$</span></p>"#,
    );

    let runtime = DelayedInput::new(
        authored_source,
        vec![InputSegment::text(
            0..authored_source.len(),
            TextOrigin::RuntimeScalar,
        )],
    )
    .expect("valid runtime fixture");
    assert_eq!(
        render_delayed(&runtime, &SlotBindings::empty()),
        "<p>[[$x$]]</p>",
    );

    let source = "[[$OUTER @@%%page%%@@ TAIL$]]";
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
    let html = render_delayed(&generated, &bindings);
    assert!(!html.contains("math-inline"), "{html}");
    assert!(html.contains("[[$OUTER "), "{html}");
    assert!(html.contains("Generated&#32;reference"), "{html}");
    assert!(html.contains(" TAIL$]]"), "{html}");

    let direct_source = "[[$A %%page%% B$]]";
    let direct_start = direct_source.find("%%page%%").expect("fixture marker");
    let direct_end = direct_start + "%%page%%".len();
    let direct_generated = DelayedInput::new(
        direct_source,
        vec![
            InputSegment::text(0..direct_start, TextOrigin::Authored),
            InputSegment::generated(GeneratedInput {
                source_range: direct_start..direct_end,
                id: SlotId::new(1),
                kind: GeneratedKind::PageLink,
                occurrence: 0,
            }),
            InputSegment::text(direct_end..direct_source.len(), TextOrigin::Authored),
        ],
    )
    .expect("valid direct generated fixture");
    let direct_html = render_delayed(&direct_generated, &bindings);
    assert!(!direct_html.contains("math-inline"), "{direct_html}");
    assert!(direct_html.contains("[[$A "), "{direct_html}");
    assert!(direct_html.contains("Generated reference"), "{direct_html}");
    assert!(direct_html.contains(" B$]]"), "{direct_html}");

    let runtime_source = "[[$A RUNTIME B$]]";
    let runtime_start = runtime_source.find("RUNTIME").expect("fixture scalar");
    let runtime_end = runtime_start + "RUNTIME".len();
    let mixed_runtime = DelayedInput::new(
        runtime_source,
        vec![
            InputSegment::text(0..runtime_start, TextOrigin::Authored),
            InputSegment::text(runtime_start..runtime_end, TextOrigin::RuntimeScalar),
            InputSegment::text(runtime_end..runtime_source.len(), TextOrigin::Authored),
        ],
    )
    .expect("valid mixed runtime fixture");
    assert_eq!(
        render_delayed(&mixed_runtime, &SlotBindings::empty()),
        "<p>[[$A RUNTIME B$]]</p>",
    );
}

#[test]
fn wikijump_layout_keeps_its_existing_multiline_behavior() {
    let source = "before[[$A\nB$]]after";
    let (html, text, _) = render_with_layout(source, Layout::Wikijump);
    assert!(!html.contains("math-inline"), "{html}");
    assert!(html.contains("before[[$A"), "{html}");
    assert!(html.contains("B$]]after"), "{html}");
    assert_eq!(text, source);
}

#[test]
fn long_and_dense_inline_math_stays_bounded() {
    let formula = "x".repeat(512 * 1024);
    let long = format!("[[$ {formula} $]]");
    let dense = "[[$x$]]".repeat(4_096);
    let comment_dense = format!("[[$ {} $]]", "x[!--hidden--]".repeat(4_096));
    let bracket_dense = format!("[[$A{}$]]", "]".repeat(4_096));
    let unclosed = format!("[[$A{}", "x".repeat(512 * 1024));
    let started = Instant::now();

    let (long_html, _, long_errors) = render(&long);
    let (dense_html, _, dense_errors) = render(&dense);
    let (comment_html, _, comment_errors) = render(&comment_dense);
    let (bracket_html, _, bracket_errors) = render(&bracket_dense);
    let (unclosed_html, unclosed_text, unclosed_errors) = render(&unclosed);

    assert!(long_errors.is_empty(), "{long_errors:#?}");
    assert!(dense_errors.is_empty(), "{dense_errors:#?}");
    assert!(comment_errors.is_empty(), "{comment_errors:#?}");
    assert!(bracket_errors.is_empty(), "{bracket_errors:#?}");
    assert_eq!(long_html.matches("math-inline").count(), 1);
    assert_eq!(dense_html.matches("math-inline").count(), 4_096);
    assert_eq!(comment_html.matches("math-inline").count(), 1);
    assert!(!comment_html.contains("hidden"), "{comment_html}");
    assert_eq!(bracket_html.matches("math-inline").count(), 1);
    assert_eq!(unclosed_text, unclosed);
    assert!(unclosed_html.contains("[[$A"), "{unclosed_html}");
    assert_error(&unclosed_errors, ParseErrorKind::EndOfInput, &unclosed);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "inline-math parsing took {:?}",
        started.elapsed(),
    );
}
