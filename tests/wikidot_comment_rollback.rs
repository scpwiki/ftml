use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::SyntaxTree;
use std::borrow::Cow;
use std::sync::mpsc;
use std::time::Duration;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("comment-rollback"),
        category: Some(Cow::Borrowed("test")),
        site: Cow::Borrowed("coverage"),
        title: Cow::Borrowed("Comment rollback"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![Cow::Borrowed("test")],
        language: Cow::Borrowed("default"),
    }
}

fn parse(source: &str) -> SyntaxTree<'static> {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut text = source.to_owned();
    ftml::preprocess_for_layout(&mut text, Layout::Wikidot);
    let tokenization = ftml::tokenize(&text);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();

    assert!(errors.is_empty(), "{source:?}: {errors:?}");
    tree.to_owned()
}

fn render_html(source: &str) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    HtmlRender
        .render(&parse(source), &page_info, &settings)
        .body
}

fn strike(body: &str) -> String {
    format!(r#"<span style="text-decoration: line-through;">{body}</span>"#)
}

#[test]
fn malformed_comment_candidate_rolls_back_to_strikethrough() {
    assert_eq!(
        render_html("A[!--hidden-- ]B"),
        "<p>A[!<span style=\"text-decoration: line-through;\">hidden</span> ]B</p>",
    );
}

#[test]
fn non_adjacent_closer_matrix_matches_live_dom_and_residual_text() {
    let residuals = ["", "-", "—", "—-", "——"];

    for (hyphens, residual) in (2..=6).zip(residuals) {
        let closing_run = "-".repeat(hyphens);
        let strike = strike("hidden");
        let spaced = format!("A[!--hidden{closing_run} ]B");
        assert_eq!(
            render_html(&spaced),
            format!("<p>A[!{strike}{residual} ]B</p>"),
            "space before right bracket with {hyphens} hyphens",
        );

        let newline = format!("A[!--hidden{closing_run}\n]B");
        assert_eq!(
            render_html(&newline),
            format!("<p>A[!{strike}{residual}<br>\n]B</p>"),
            "newline before right bracket with {hyphens} hyphens",
        );
    }
}

#[test]
fn exact_and_extended_comment_closers_remain_contextual() {
    for source in [
        "A[!--hidden--]B",
        "A[!--hidden---]B",
        "A[!--hidden----]B",
        "A[!--hidden-----]B",
        "A[!--hidden------]B",
        "A[!--日本語😀--]B",
        "A[!--x\u{a0}--]B",
        "A[!--x\\--]B",
        "A[!--http://example.com/path--]B",
        "A[!--name@example-site.com--]B",
    ] {
        assert_eq!(render_html(source), "<p>AB</p>", "{source:?}");
    }

    assert_eq!(render_html("[!----]"), "");
    assert_eq!(render_html("A\n[!--hidden--]\nB"), "<p>A<br>\nB</p>",);
    assert_eq!(render_html("A[!--x--][!--y--]B"), "<p>AB</p>",);
}

#[test]
fn invalid_opener_lookalikes_use_ordinary_strikethrough_and_punctuation() {
    for (source, prefix, body) in [
        ("A[! --hidden--]B", "A[! ", "hidden"),
        ("A[!\t--hidden--]B", "A[! ", "hidden"),
        ("A[！--hidden--]B", "A[！", "hidden"),
        ("A[---hidden--]B", "A[", "-hidden"),
        ("A[&#33;--x--]B", "A[&amp;#33;", "x"),
    ] {
        assert_eq!(
            render_html(source),
            format!("<p>{prefix}{}]B</p>", strike(body)),
            "{source:?}",
        );
    }
}

#[test]
fn malformed_and_unicode_controls_remain_ordinary_text() {
    for (source, expected) in [
        ("A[!--hiddenB", "<p>A[!—hiddenB</p>"),
        ("A\n[!--hidden\nB", "<p>A<br>\n[!—hidden<br>\nB</p>"),
        ("A[!-hidden--]B", "<p>A[!-hidden—]B</p>"),
        ("A[!—hidden--]B", "<p>A[!—hidden—]B</p>"),
        ("A[!–-hidden--]B", "<p>A[!–-hidden—]B</p>"),
        ("A［！－－x－－］B", "<p>A［！－－x－－］B</p>"),
    ] {
        assert_eq!(render_html(source), expected, "{source:?}");
    }
}

#[test]
fn unmatched_closers_keep_existing_typography() {
    for (source, expected) in [
        ("A--]B", "<p>A—]B</p>"),
        ("A---]B", "<p>A—-]B</p>"),
        ("A----]B", "<p>A——]B</p>"),
    ] {
        assert_eq!(render_html(source), expected, "{source:?}");
    }
}

#[test]
fn nested_comments_close_once_and_residual_closers_reenter_ordinary_grammar() {
    for (source, expected) in [
        (
            "A[!--one [!--two--] three--]B",
            "<p>A three—]B</p>".to_owned(),
        ),
        (
            "A[!--1 [!--2 [!--3--] 2--] 1--]B",
            format!("<p>A 2{}]B</p>", strike("] 1")),
        ),
        ("A[!--one [!--two three--]B", "<p>AB</p>".to_owned()),
        ("A[!--one [!--two--] threeB", "<p>A threeB</p>".to_owned()),
    ] {
        assert_eq!(render_html(source), expected, "{source:?}");
    }
}

#[test]
fn comment_and_link_boundaries_keep_their_existing_owners() {
    assert_eq!(
        render_html("[https://example.com label--]"),
        r#"<p><a href="https://example.com">label--</a></p>"#,
    );
    assert_eq!(
        render_html("[[[page|label--]]]"),
        r#"<p><a href="/page">label--</a></p>"#,
    );
}

#[test]
fn valid_comments_stay_inert_and_rollback_stays_sanitized() {
    let tree = parse(concat!(
        "A[!--",
        "[[module CSS]]body{}[[/module]]",
        "[[html]]<script>alert(1)</script>[[/html]]",
        "[[include secret]]",
        "--]B",
    ));
    assert!(tree.html_blocks.is_empty());
    assert!(tree.code_blocks.is_empty());

    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    assert_eq!(
        HtmlRender.render(&tree, &page_info, &settings).body,
        "<p>AB</p>",
    );

    let malformed = render_html("A[!--<script>alert(1)</script>-- ]B");
    assert!(!malformed.contains("<script>"), "{malformed}");
    assert!(malformed.contains("&lt;script&gt;"), "{malformed}");
    assert!(
        malformed.contains("text-decoration: line-through"),
        "{malformed}"
    );
}

#[test]
fn long_malformed_comment_candidates_parse_with_bounded_work_and_output() {
    const CANDIDATE_COUNT: usize = 4_096;
    const LONG_RUN: usize = 32_768;
    let repeated = "A[!--x-- ]B\n".repeat(CANDIDATE_COUNT);
    let long_run = format!("A[!--x{} ]B", "-".repeat(LONG_RUN));
    let input_bytes = repeated.len() + long_run.len();
    let (sender, receiver) = mpsc::channel();

    std::thread::spawn(move || {
        let repeated_html = render_html(&repeated);
        let long_run_html = render_html(&long_run);
        let _ = sender.send((repeated_html, long_run_html));
    });

    let (repeated_html, long_run_html) = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("malformed comment rollback should remain bounded");
    assert_eq!(
        repeated_html
            .matches("text-decoration: line-through")
            .count(),
        CANDIDATE_COUNT,
    );
    assert!(long_run_html.contains("text-decoration: line-through"));
    assert!(repeated_html.len() + long_run_html.len() < input_bytes * 64);
}
