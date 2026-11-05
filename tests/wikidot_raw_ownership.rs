use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::parsing::{ParseError, ParseErrorKind};
use ftml::render::{Render, html::HtmlRender, text::TextRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("raw-ownership"),
        category: None,
        site: Cow::Borrowed("syntax-differential"),
        title: Cow::Borrowed("Raw ownership"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn render(source: &str) -> (String, String, Vec<ParseError>) {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;
    let text = TextRender.render(&tree, &page_info, &settings);
    (html, text, errors)
}

fn render_clean(source: &str) -> (String, String) {
    let (html, text, errors) = render(source);
    assert!(errors.is_empty(), "{source:?}: {errors:#?}");
    (html, text)
}

fn assert_owner_rollback(errors: &[ParseError], source: &str) {
    assert!(
        errors
            .iter()
            .any(|error| error.kind() == ParseErrorKind::RuleFailed),
        "{source:?}: {errors:#?}",
    );
    assert!(
        errors
            .iter()
            .any(|error| error.kind() == ParseErrorKind::NoRulesMatch),
        "{source:?}: {errors:#?}",
    );
}

#[test]
fn evidenced_raw_delimiter_and_owner_matrix_matches_wikidot() {
    for (source, expected_html, expected_text, rolls_back) in [
        (
            "@@A@@@@B@@",
            r#"<p><span style="white-space: pre-wrap;">A</span>@@B@@</p>"#,
            "A@@B@@",
            false,
        ),
        (
            "@@<b>A&B</b>@@",
            r#"<p>@<span style="white-space: pre-wrap;">b&gt;A&amp;B&lt;/b</span>@</p>"#,
            "@b>A&B</b@",
            false,
        ),
        (
            "[[[scp-002|@@A@@]]]",
            r#"<p>[[[scp-002|<span style="white-space: pre-wrap;">A</span>]]]</p>"#,
            "[[[scp-002|A]]]",
            true,
        ),
        (
            r#"[[span title="@@A@@"]]X[[/span]]"#,
            "<p><span>X</span></p>",
            "X",
            false,
        ),
    ] {
        let (html, text, errors) = render(source);
        assert_eq!(
            (html, text),
            (expected_html.to_owned(), expected_text.to_owned())
        );
        if rolls_back {
            assert_owner_rollback(&errors, source);
        } else {
            assert!(errors.is_empty(), "{source:?}: {errors:#?}");
        }
    }
}

#[test]
fn raw_precedence_controls_keep_their_existing_owners() {
    for (source, expected_html) in [
        (
            "@@A@@ @@B@@",
            concat!(
                r#"<p><span style="white-space: pre-wrap;">A</span> "#,
                r#"<span style="white-space: pre-wrap;">B</span></p>"#,
            ),
        ),
        (
            "[https://example.com @@A@@]",
            concat!(
                r#"<p>[<a href="https://example.com">https://example.com</a> "#,
                r#"<span style="white-space: pre-wrap;">A</span>]</p>"#,
            ),
        ),
        (
            "+ @@A@@",
            concat!(
                r#"<h1 id="toc0"><span>"#,
                r#"<span style="white-space: pre-wrap;">A</span></span></h1>"#,
            ),
        ),
        (
            "||@@A@@||",
            concat!(
                "<table class=\"wiki-content-table\">\n<tr>\n<td>",
                r#"<span style="white-space: pre-wrap;">A</span>"#,
                "</td>\n</tr>\n</table>",
            ),
        ),
        (
            "@@[[#if 1 | A | B ]]@@",
            r#"<p><span style="white-space: pre-wrap;">[[#if&#32;1&#32;|&#32;A&#32;|&#32;B&#32;]]</span></p>"#,
        ),
        (
            "[[code]]\n@@A@@\n[[/code]]",
            "<div class=\"code\"><pre><code>@@A@@</code></pre></div>",
        ),
    ] {
        assert_eq!(render_clean(source).0, expected_html, "{source:?}");
    }
}

#[test]
fn malformed_and_angle_boundary_controls_preserve_exact_residual_text() {
    for (source, expected_html, expected_text) in [
        ("@@A", "<p>@@A</p>", "@@A"),
        ("@@A\nB@@", "<p>@@A<br>\nB@@</p>", "@@A\nB@@"),
        (
            "@<&amp;>@",
            r#"<p><span style="white-space: pre-wrap;">&amp;</span></p>"#,
            "&",
        ),
        (
            "@@&amp;@@",
            r#"<p><span style="white-space: pre-wrap;">&amp;amp;</span></p>"#,
            "&amp;",
        ),
    ] {
        let (html, text, errors) = render(source);
        assert_eq!(html, expected_html, "{source:?}");
        assert_eq!(text, expected_text, "{source:?}");
        if source.starts_with("@@A") {
            assert!(
                errors.iter().any(|error| matches!(
                    error.kind(),
                    ParseErrorKind::EndOfInput | ParseErrorKind::RuleFailed
                )),
                "{source:?}: {errors:#?}",
            );
        } else {
            assert!(errors.is_empty(), "{source:?}: {errors:#?}");
        }
    }
}

#[test]
fn inline_math_and_raw_composition_matches_live_owner_transfer() {
    for (source, expected_html, expected_text, rolls_back) in [
        (
            "[[$OUTER @@INNER@@ TAIL$]]",
            r#"<p><span class="math-inline">$$</span></p>"#,
            "",
            false,
        ),
        (
            "[[$@@INNER@@$]]",
            r#"<p><span class="math-inline">$$</span></p>"#,
            "",
            false,
        ),
        (
            "[[$OUTER @@INNER$]] TAIL@@",
            concat!(
                r#"<p>[[$OUTER <span style="white-space: pre-wrap;">"#,
                "INNER$]]&#32;TAIL</span></p>",
            ),
            "[[$OUTER INNER$]] TAIL",
            true,
        ),
    ] {
        let (html, text, errors) = render(source);
        assert_eq!(html, expected_html, "{source:?}");
        assert_eq!(text, expected_text, "{source:?}");
        if rolls_back {
            assert_owner_rollback(&errors, source);
        } else {
            assert!(errors.is_empty(), "{source:?}: {errors:#?}");
        }
    }
}

#[test]
fn incomplete_raw_marker_inside_math_does_not_gain_owner_authority() {
    assert_eq!(
        render_clean("[[$A @@ B$]]"),
        (
            r#"<p><span class="math-inline">$A @@ B$</span></p>"#.to_owned(),
            String::new(),
        ),
    );
}

#[test]
fn long_closed_runs_and_unclosed_candidates_remain_bounded() {
    let closed_run = "@".repeat(65_536);
    let unclosed = format!("@@{}", "A@".repeat(32_768));
    let started = Instant::now();

    let (closed_html, _, _) = render(&closed_run);
    let (unclosed_html, unclosed_text, errors) = render(&unclosed);

    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(closed_html.len() <= closed_run.len() + 16);
    assert_eq!(unclosed_text, unclosed);
    assert!(unclosed_html.len() <= unclosed.len() * 2 + 16);
    assert!(
        errors
            .iter()
            .any(|error| error.kind() == ParseErrorKind::EndOfInput)
    );
}
