use ftml::data::{PageInfo, ScoreValue};
use ftml::includes::DebugIncluder;
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("triple-link-label-ownership"),
        category: None,
        site: Cow::Borrowed("syntax-differential"),
        title: Cow::Borrowed("Triple-link label ownership"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn settings() -> WikitextSettings {
    WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot)
}

fn render(source: &str) -> String {
    let page_info = page_info();
    let settings = settings();
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, _) = ftml::parse(&tokenization, &page_info, &settings).into();
    HtmlRender.render(&tree, &page_info, &settings).body
}

#[test]
fn literal_label_controls_remain_inside_the_outer_link() {
    for (source, label) in [
        ("[[[start|**A**]]]", "**A**"),
        ("[[[start|//A//]]]", "//A//"),
        ("[[[start|{{A}}]]]", "{{A}}"),
        ("[[[start|~~A~~]]]", "~~A~~"),
        ("[[[start|,,A,,]]]", ",,A,,"),
        ("[[[start|^^A^^]]]", "^^A^^"),
        ("[[[start|__A__]]]", "__A__"),
        ("[[[start|A|B]]]", "A|B"),
        ("[[[start|A||B]]]", "A||B"),
        ("[[[start|((bibcite alpha))]]]", "((bibcite alpha))"),
        ("[[[start|日本語🙂]]]", "日本語🙂"),
    ] {
        assert_eq!(
            render(source),
            format!(r#"<p><a href="/start">{label}</a></p>"#),
            "{source:?}",
        );
    }
}

#[test]
fn raw_span_and_brackets_roll_back_the_outer_link() {
    let cases = [
        (
            "[[[start|@@A@@]]]",
            r#"<p>[[[start|<span style="white-space: pre-wrap;">A</span>]]]</p>"#,
        ),
        (
            "[[[start|@<A>@]]]",
            r#"<p>[[[start|<span style="white-space: pre-wrap;">A</span>]]]</p>"#,
        ),
        (
            "[[[start|[[span]]A[[/span]]]]]",
            "<p>[[[start|<span>A</span>]]]</p>",
        ),
        ("[[[start|A[B]]]", "<p>[[[start|A[B]]]</p>"),
        ("[[[start|A]B]]]", "<p>[[[start|A]B]]]</p>"),
    ];

    for (source, expected) in cases {
        let html = render(source);
        assert_eq!(html, expected, "{source:?}");
        assert!(!html.contains(r#"href="/start""#), "{source:?}: {html}");
    }
}

#[test]
fn incomplete_raw_markers_remain_literal_label_text() {
    for (source, label) in [
        ("[[[start|A@@B]]]", "A@@B"),
        ("[[[start|A@<B]]]", "A@&lt;B"),
        ("[[[start|A>@B]]]", "A&gt;@B"),
    ] {
        assert_eq!(
            render(source),
            format!(r#"<p><a href="/start">{label}</a></p>"#),
            "{source:?}",
        );
    }
}

#[test]
fn nested_links_take_ownership_and_leave_outer_markers() {
    let cases = [
        (
            "[[[start|A[[[system:join|B]]]C]]]",
            r#"<p>[[[start|A<a href="/system:join">B</a>C]]]</p>"#,
        ),
        (
            "[[[start|A[https://example.com B]C]]]",
            r#"<p>[[[start|A<a href="https://example.com">B</a>C]]]</p>"#,
        ),
        (
            "[[[start|A[#toc B]C]]]",
            r##"<p>[[[start|A<a href="#toc">B</a>C]]]</p>"##,
        ),
    ];

    for (source, expected) in cases {
        let html = render(source);
        assert_eq!(html, expected, "{source:?}");
        assert!(!html.contains(r#"href="/start""#), "{source:?}: {html}");
    }
}

#[test]
fn image_and_footnote_side_effects_survive_outer_rollback() {
    assert_eq!(
        render("[[[start|A[[image http://example.com/a.png]]B]]]"),
        concat!(
            "\n\n[[[start|A",
            r#"<img src="http://example.com/a.png" class="image" alt="a.png">"#,
            "B]]]",
        ),
    );

    let html = render(concat!(
        "[[footnote]]P[[/footnote]] ",
        "[[[start|A[[footnote]]N[[/footnote]]B]]] ",
        "[[footnote]]Q[[/footnote]]",
    ));
    assert!(!html.contains(r#"href="/start""#), "{html}");
    assert!(
        html.contains("[[[start|A<sup class=\"footnoteref\">"),
        "{html}"
    );
    for number in 1..=3 {
        assert!(
            html.contains(&format!("id=\"footnoteref-{number}\"")),
            "{html}",
        );
        assert!(
            html.contains(&format!("id=\"footnote-{number}\"")),
            "{html}",
        );
    }
    assert!(html.contains("B]]]<sup class=\"footnoteref\">"), "{html}");
    assert_eq!(
        html.matches("class=\"footnote-footer\"").count(),
        3,
        "{html}"
    );
    assert!(html.contains(">1</a>. P</div>"), "{html}");
    assert!(html.contains(">2</a>. N</div>"), "{html}");
    assert!(html.contains(">3</a>. Q</div>"), "{html}");
}

#[test]
fn other_live_backed_inline_blocks_take_ownership() {
    assert_eq!(
        render("[[[start|A[[size 120%]]X[[/size]]B]]]"),
        r#"<p>[[[start|A<span style="font-size:120%;">X</span>B]]]</p>"#,
    );
    assert_eq!(
        render("[[[start|A[[iframe https://example.com/]]B]]]"),
        concat!(
            r#"<p>[[[start|A<iframe src="https://example.com/" align="" frameborder="" "#,
            r#"height="" scrolling="" width="" class="" style=""></iframe>B]]]</p>"#,
        ),
    );
    assert_eq!(
        render("[[[start|A[[# apple]]B]]]"),
        r#"<p>[[[start|A<a name="apple"></a>B]]]</p>"#,
    );
    assert_eq!(
        render("[[[start|A[[$x$]]B]]]"),
        r#"<p>[[[start|A<span class="math-inline">$x$</span>B]]]</p>"#,
    );
}

#[test]
fn valid_comments_are_transparent_but_malformed_comments_own_brackets() {
    assert_eq!(
        render("[[[start|A[!--hidden [brackets]--]B]]]"),
        r#"<p><a href="/start">AB</a></p>"#,
    );
    assert_eq!(render("[[[start|A[!--hidden--]]]"), "<p>[[[start|A]]</p>",);

    let malformed = render("[[[start|A[!--unfinished B]]]");
    assert!(!malformed.contains(r#"href="/start""#), "{malformed}");
    assert!(malformed.contains("[[[start|A[!"), "{malformed}");
    assert!(malformed.ends_with("unfinished B]]]</p>"), "{malformed}");
}

#[test]
fn block_recovery_stays_fail_closed_after_outer_rollback() {
    for source in [
        "[[[start|A[[module Rate]]B]]]",
        "[[[start|A[[module CSS]]B[[/module]]C]]]",
        "[[[start|A[[include component:does-not-exist]]B]]]",
        "[[[start|A[[unknown]]B]]]",
        "[[[start|A[[image]]B]]]",
        "[[[start|A[[footnote]]N B]]]",
        "[[[start|A[[span]]B]]]",
    ] {
        assert_eq!(render(source), format!("<p>{source}</p>"), "{source:?}");
    }

    let html_source = "[[[start|A[[html]]<script>alert(1)</script>[[/html]]B]]]";
    let html = render(html_source);
    assert_eq!(
        html,
        "<p>[[[start|A[[html]]&lt;script&gt;alert(1)&lt;/script&gt;[[/html]]B]]]</p>",
    );
    assert!(!html.contains(r#"href="/start""#), "{html}");
    assert!(!html.contains("<script>"), "{html}");
    assert!(!html.contains("<iframe"), "{html}");
}

#[test]
fn inline_include_scanning_does_not_gain_execution_authority() {
    let source = "[[[start|A[[include component:example]]B]]]";
    let (expanded, pages) =
        ftml::include(source, &settings(), DebugIncluder::first_missing(), || {
            unreachable!("inline include must not be collected")
        })
        .unwrap();

    assert_eq!(expanded, source);
    assert!(pages.is_empty());
    assert_eq!(render(&expanded), format!("<p>{source}</p>"));
}

#[test]
fn malformed_nested_closers_preserve_exact_residual_source() {
    for source in [
        "[[[start|A[[[system:join|B]C]]]",
        "[[[start|A[[[system:join|B]]C]]]",
        "[[[start|A[[$x B]]]",
    ] {
        assert_eq!(render(source), format!("<p>{source}</p>"), "{source:?}");
    }

    assert_eq!(
        render("[[[start|A[[# apple B]]]"),
        r#"<p>[[[start|A[<a href="javascript:;">apple B</a>]]</p>"#,
    );

    assert_eq!(
        render("[[[start|A[[[system:join|B]]]]C]]]"),
        r#"<p>[[[start|A<a href="/system:join">B</a>]C]]]</p>"#,
    );
    assert_eq!(
        render("[[[start|A[https://example.com B C]]]"),
        r#"<p>[[[start|A[<a href="https://example.com">https://example.com</a> B C]]]</p>"#,
    );
}

#[test]
fn bracket_runs_and_multiline_labels_keep_their_existing_boundaries() {
    for (source, expected) in [
        ("A[[[start|Label]B", "<p>A[[[start|Label]B</p>"),
        ("A[[[start|Label]]B", "<p>A[[[start|Label]]B</p>"),
        (
            "A[[[start|Label]]]B",
            r#"<p>A<a href="/start">Label</a>B</p>"#,
        ),
        (
            "A[[[start|Label]]]]B",
            r#"<p>A<a href="/start">Label</a>]B</p>"#,
        ),
        (
            "A[[[start|Label]]]]]B",
            r#"<p>A<a href="/start">Label</a>]]B</p>"#,
        ),
        ("[[[start|A\nB]]]", "<p><a href=\"/start\">A\nB</a></p>"),
    ] {
        assert_eq!(render(source), expected, "{source:?}");
    }
}

#[test]
fn parser_functions_still_substitute_before_literal_label_collection() {
    assert_eq!(
        render("[[[start|A[[#if 1 | B | C ]]D]]]"),
        r#"<p><a href="/start">ABD</a></p>"#,
    );
}

#[test]
fn deep_owners_and_long_bracket_runs_remain_bounded() {
    let depth = 2_048;
    let mut nested = "[[[page|".repeat(depth);
    nested.push('X');
    nested.push_str(&"]]]".repeat(depth));

    let start = Instant::now();
    let nested_html = render(&nested);
    let nested_elapsed = start.elapsed();
    assert_eq!(nested_html.matches("href=\"/page\"").count(), 1);
    assert!(
        nested_elapsed < Duration::from_secs(5),
        "deep owner recovery took {nested_elapsed:?}",
    );

    let run = 20_000;
    let brackets = format!("[[[start|A{}", "]".repeat(run));
    let start = Instant::now();
    let bracket_html = render(&brackets);
    let bracket_elapsed = start.elapsed();
    assert_eq!(bracket_html.matches(r#"href="/start""#).count(), 1);
    assert_eq!(bracket_html.matches(']').count(), run - 3);
    assert!(
        bracket_elapsed < Duration::from_secs(5),
        "long bracket run took {bracket_elapsed:?}",
    );
}
