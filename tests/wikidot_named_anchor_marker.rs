use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::parsing::{ParseError, ParseErrorKind};
use ftml::render::Render;
use ftml::render::html::HtmlRender;
use ftml::render::text::TextRender;
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("named-anchor-marker"),
        category: None,
        site: Cow::Borrowed("syntax-differential"),
        title: Cow::Borrowed("Named anchor marker"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn render(source: &str) -> (String, String, Vec<ParseError>) {
    render_with_layout(source, Layout::Wikidot)
}

fn render_with_layout(source: &str, layout: Layout) -> (String, String, Vec<ParseError>) {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    (
        HtmlRender.render(&tree, &page_info, &settings).body,
        TextRender.render(&tree, &page_info, &settings),
        errors,
    )
}

fn error_signature(
    errors: &[ParseError],
) -> Vec<(&str, ParseErrorKind, std::ops::Range<usize>)> {
    errors
        .iter()
        .map(|error| (error.rule(), error.kind(), error.span()))
        .collect()
}

#[test]
fn live_backed_named_anchor_mismatches_match_exact_html_and_text() {
    for (source, expected_html, expected_text) in [
        ("[[# ]]", "<p>[[# ]]</p>", "[[# ]]"),
        ("[[# alpha]]]", r#"<p><a name="alpha"></a>]</p>"#, "]"),
        (
            "[[[scp-002|[[# alpha]]]]]",
            r#"<p>[[[scp-002|<a name="alpha"></a>]]]</p>"#,
            "[[[scp-002|]]]",
        ),
        (
            "[https://example.com [[# alpha]]]",
            concat!(
                r#"<p>[<a href="https://example.com">https://example.com</a> "#,
                r#"<a name="alpha"></a>]</p>"#,
            ),
            "[https://example.com ]",
        ),
    ] {
        let (html, text, errors) = render(source);
        assert_eq!(html, expected_html, "{source:?}");
        assert_eq!(text, expected_text, "{source:?}");

        let expected_errors = match source {
            "[[[scp-002|[[# alpha]]]]]" => vec![
                ("link-triple", ParseErrorKind::RuleFailed, 11..14),
                ("fallback", ParseErrorKind::NoRulesMatch, 0..3),
            ],
            _ => Vec::new(),
        };
        assert_eq!(error_signature(&errors), expected_errors, "{source:?}");
    }
}

#[test]
fn live_backed_nineteen_case_anchor_family_keeps_its_control_contracts() {
    let cases = [
        (
            "adjacent",
            "[[# a]][[# b]]",
            r#"<p><a name="a"></a><a name="b"></a></p>"#,
            "",
        ),
        (
            "basic",
            "A[[# alpha]]B",
            r#"<p>A<a name="alpha"></a>B</p>"#,
            "AB",
        ),
        (
            "code-owner",
            "[[code]]\n[[# alpha]]\n[[/code]]",
            r#"<div class="code"><pre><code>[[# alpha]]</code></pre></div>"#,
            "[[# alpha]]",
        ),
        ("empty", "[[# ]]", "<p>[[# ]]</p>", "[[# ]]"),
        (
            "escaped",
            "\\[[# alpha]]",
            "<p>\\<a name=\"alpha\"></a></p>",
            "\\",
        ),
        (
            "external-link-label",
            "[https://example.com [[# alpha]]]",
            concat!(
                r#"<p>[<a href="https://example.com">https://example.com</a> "#,
                r#"<a name="alpha"></a>]</p>"#,
            ),
            "[https://example.com ]",
        ),
        (
            "extra-close",
            "[[# alpha]]]",
            r#"<p><a name="alpha"></a>]</p>"#,
            "]",
        ),
        (
            "heading",
            "+ [[# alpha]]",
            r#"<h1 id="toc0"><span><a name="alpha"></a></span></h1>"#,
            "",
        ),
        (
            "internal-link-label",
            "[[[scp-002|[[# alpha]]]]]",
            r#"<p>[[[scp-002|<a name="alpha"></a>]]]</p>"#,
            "[[[scp-002|]]]",
        ),
        (
            "list",
            "* [[# alpha]]",
            "<ul>\n<li><a name=\"alpha\"></a></li>\n</ul>",
            "",
        ),
        (
            "missing-close",
            "[[# alpha",
            "<p>[[# alpha</p>",
            "[[# alpha",
        ),
        (
            "multiple",
            "[[# alpha beta]]",
            r#"<p>[<a href="javascript:;">alpha beta</a>]</p>"#,
            "[alpha beta]",
        ),
        ("no-space", "[[#alpha]]", "<p>[[#alpha]]</p>", "[[#alpha]]"),
        (
            "raw-owner",
            "@@[[# alpha]]@@",
            r#"<p><span style="white-space: pre-wrap;">[[# alpha]]</span></p>"#,
            "[[# alpha]]",
        ),
        (
            "single-newline",
            "[[# alpha\nbeta]]",
            "<p>[[# alpha<br>\nbeta]]</p>",
            "[[# alpha\nbeta]]",
        ),
        (
            "span-attribute-separate-owner",
            "[[span title=\"[[# alpha]]\"]]X[[/span]]",
            "<p><span>&quot;]]X</span></p>",
            "\"]]X",
        ),
        (
            "symbol",
            "[[# symbol$%_foo]]",
            r#"<p>[<a href="javascript:;">symbol$%_foo</a>]</p>"#,
            "[symbol$%_foo]",
        ),
        (
            "table",
            "|| [[# alpha]] ||",
            concat!(
                "<table class=\"wiki-content-table\">\n<tr>\n",
                "<td><a name=\"alpha\"></a></td>\n</tr>\n</table>",
            ),
            "",
        ),
        (
            "unicode",
            "[[# 日本語🙂]]",
            r#"<p>[<a href="javascript:;">日本語🙂</a>]</p>"#,
            "[日本語🙂]",
        ),
    ];

    assert_eq!(cases.len(), 19);
    for (case_id, source, expected_html, expected_text) in cases {
        let (html, text, errors) = render(source);
        assert_eq!(html, expected_html, "{case_id}: {source:?}");
        assert_eq!(text, expected_text, "{case_id}: {source:?}");

        let expected_errors = match case_id {
            "internal-link-label" => vec![
                ("link-triple", ParseErrorKind::RuleFailed, 11..14),
                ("fallback", ParseErrorKind::NoRulesMatch, 0..3),
            ],
            "missing-close" => vec![
                ("anchor", ParseErrorKind::EndOfInput, 9..9),
                ("fallback", ParseErrorKind::NoRulesMatch, 0..3),
            ],
            "no-space" => vec![
                ("anchor", ParseErrorKind::RuleFailed, 3..8),
                ("fallback", ParseErrorKind::NoRulesMatch, 0..3),
                ("fallback", ParseErrorKind::NoRulesMatch, 8..10),
            ],
            "single-newline" => vec![
                ("anchor", ParseErrorKind::RuleFailed, 9..10),
                ("fallback", ParseErrorKind::NoRulesMatch, 0..3),
                ("fallback", ParseErrorKind::NoRulesMatch, 14..16),
            ],
            "span-attribute-separate-owner" => {
                vec![("fallback", ParseErrorKind::NoRulesMatch, 26..28)]
            }
            _ => Vec::new(),
        };
        assert_eq!(error_signature(&errors), expected_errors, "{case_id}");
    }
}

#[test]
fn valid_nested_anchors_own_links_but_invalid_candidates_do_not() {
    for (source, expected_html, expected_text) in [
        (
            "[https://example.com [!--before--][[# alpha]][!--after--]]",
            concat!(
                r#"<p>[<a href="https://example.com">https://example.com</a>"#,
                r#" <a name="alpha"></a>]</p>"#,
            ),
            "[https://example.com ]",
        ),
        (
            "[[[start|A[[# alpha]]B]]]",
            r#"<p>[[[start|A<a name="alpha"></a>B]]]</p>"#,
            "[[[start|AB]]]",
        ),
    ] {
        let (html, text, _) = render(source);
        assert_eq!(html, expected_html, "{source:?}");
        assert_eq!(text, expected_text, "{source:?}");
    }

    for source in [
        "[https://example.com [[# ]]]",
        "[https://example.com [[# alpha beta]]]",
        "[[[start|A[[# ]]B]]]",
        "[[[start|A[[# alpha beta]]B]]]",
    ] {
        let (html, _, _) = render(source);
        assert!(!html.contains("<a name"), "{source:?}: {html}");
    }
}

#[test]
fn whitespace_comments_and_malformed_close_runs_stay_transactional() {
    let (html, text, errors) = render("[[#\talpha-beta_gamma]] [[# alpha\tbeta]]");
    assert_eq!(
        html,
        concat!(
            r#"<p><a name="alpha-beta_gamma"></a> [<a href="javascript:;">"#,
            "alpha beta</a>]</p>",
        ),
    );
    assert_eq!(text, " [alpha beta]");
    assert!(errors.is_empty(), "{errors:#?}");

    let (comment_html, comment_text, _) = render("A[!--[[# alpha]]--]B");
    assert_eq!(comment_html, "<p>AB</p>");
    assert_eq!(comment_text, "AB");

    let (crlf_html, crlf_text, _) = render("[[# alpha\r\nbeta]]");
    assert_eq!(crlf_html, "<p>[[# alpha<br>\nbeta]]</p>");
    assert_eq!(crlf_text, "[[# alpha\nbeta]]");

    for brackets in 1..=12 {
        let source = format!("[[# alpha{}", "]".repeat(brackets));
        let (html, text, _) = render(&source);
        if brackets == 1 {
            assert_eq!(html, format!("<p>{source}</p>"), "{brackets}");
            assert_eq!(text, source, "{brackets}");
        } else {
            let residual = "]".repeat(brackets - 2);
            assert_eq!(
                html,
                format!(r#"<p><a name="alpha"></a>{residual}</p>"#),
                "{brackets}",
            );
            assert_eq!(text, residual, "{brackets}");
        }
    }

    for brackets in 1..=12 {
        let source = format!("[[# {}", "]".repeat(brackets));
        let (html, text, _) = render(&source);
        assert_eq!(html, format!("<p>{source}</p>"), "{brackets}");
        assert_eq!(text, source, "{brackets}");
    }

    let source = "[[# alpha beta]]]";
    let (html, text, _) = render(source);
    assert_eq!(html, format!("<p>{source}</p>"));
    assert_eq!(text, source);

    for brackets in 3..=5 {
        let source = format!("[[[start|A[[# alpha B{}", "]".repeat(brackets));
        let residual = "]".repeat(brackets - 2);
        let (html, text, _) = render(&source);
        assert_eq!(
            html,
            format!(r#"<p>[[[start|A[<a href="javascript:;">alpha B</a>]{residual}</p>"#),
            "{source:?}",
        );
        assert_eq!(text, format!("[[[start|A[alpha B]{residual}"), "{source:?}");
    }
}

#[test]
fn anchor_names_cannot_create_html_or_event_attributes() {
    for source in [
        r#"[[# "><script>alert(1)</script>]]"#,
        r#"[[# alpha" onmouseover="alert(1)]]"#,
        "[[# <img/src=x/onerror=alert(1)>]]",
    ] {
        let (html, _, _) = render(source);
        assert!(!html.contains("<script"), "{source:?}: {html}");
        assert!(!html.contains("<img"), "{source:?}: {html}");
        assert!(!html.contains(" onmouseover=\""), "{source:?}: {html}");
        assert!(!html.contains(" onerror=\""), "{source:?}: {html}");
        assert!(!html.contains("<a name="), "{source:?}: {html}");
    }

    let (html, _, errors) = render("[[# onclick]]");
    assert_eq!(html, r#"<p><a name="onclick"></a></p>"#);
    assert!(!html.contains(" onclick="), "{html}");
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn wikijump_layout_keeps_its_named_anchor_dom_and_isolation() {
    let (html, text, errors) = render_with_layout("A[[# alpha]]B", Layout::Wikijump);
    assert_eq!(
        html,
        r#"<p>A<a class="wj-anchor-target" id="alpha"></a>B</p>"#,
    );
    assert_eq!(text, "AB");
    assert!(errors.is_empty(), "{errors:#?}");

    let (empty_html, empty_text, empty_errors) =
        render_with_layout("[[# ]]", Layout::Wikijump);
    assert_eq!(empty_html, "<p>[[# ]]</p>");
    assert_eq!(empty_text, "[[# ]]");
    assert!(empty_errors.is_empty(), "{empty_errors:#?}");

    let (extra_html, extra_text, _) =
        render_with_layout("[[# alpha]]]", Layout::Wikijump);
    assert_eq!(extra_html, "<p>[[# alpha]]]</p>");
    assert_eq!(extra_text, "[[# alpha]]]");

    let (unicode_html, unicode_text, unicode_errors) =
        render_with_layout("A[[# 日本語🙂]]B", Layout::Wikijump);
    assert_eq!(
        unicode_html,
        r#"<p>A<a class="wj-anchor-target" id="日本語🙂"></a>B</p>"#,
    );
    assert_eq!(unicode_text, "AB");
    assert!(unicode_errors.is_empty(), "{unicode_errors:#?}");
}

#[test]
fn complete_html_code_and_raw_owners_keep_markers_inert() {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let source = "[[html]]<span>[[# alpha]]</span>[[/html]]";
    let tokenization = ftml::tokenize(source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    assert_eq!(tree.html_blocks, ["<span>[[# alpha]]</span>"]);
    let output = HtmlRender.render(&tree, &page_info, &settings);
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(!output.body.contains("<a name"), "{}", output.body);

    for source in ["[[code]]\n[[# alpha]]\n[[/code]]", "@@[[# alpha]]@@"] {
        let (html, text, errors) = render(source);
        assert!(!html.contains("<a name"), "{source:?}: {html}");
        assert_eq!(text, "[[# alpha]]", "{source:?}");
        assert!(errors.is_empty(), "{source:?}: {errors:#?}");
    }
}

#[test]
fn dense_long_and_adversarial_anchor_inputs_stay_bounded() {
    let dense = "[[# a]]".repeat(4_096);
    let long_name = "a".repeat(128 * 1024);
    let long_brackets = "]".repeat(20_000);
    let source = format!("{dense}[[# {long_name}]]X[[# a{long_brackets}");

    let started = Instant::now();
    let (html, text, errors) = render(&source);
    let elapsed = started.elapsed();

    assert_eq!(html.matches("<a name=").count(), 4_098, "{html}");
    assert_eq!(text, format!("X{}", "]".repeat(long_brackets.len() - 2)));
    assert!(
        errors.iter().all(|error| {
            error.rule() == "fallback" && error.kind() == ParseErrorKind::NoRulesMatch
        }),
        "unexpected errors in {} recoveries",
        errors.len(),
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "named-anchor parsing took {elapsed:?}",
    );

    let malformed_owners = "[[# ".repeat(16 * 1024);
    let explicit = format!("[https://example.com {malformed_owners}Label]");
    let started = Instant::now();
    let (html, _, _) = render(&explicit);
    let elapsed = started.elapsed();
    assert_eq!(html.matches(r#"href="https://example.com""#).count(), 1);
    assert!(!html.contains("<a name"), "{html}");
    assert!(
        elapsed < Duration::from_secs(3),
        "named-anchor owner scan took {elapsed:?}",
    );
}
