use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use regex::Regex;
use std::borrow::Cow;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
struct Anchor {
    href: String,
    target: Option<String>,
    text: String,
}

fn anchor(href: &str, target: Option<&str>, text: &str) -> Anchor {
    Anchor {
        href: href.to_owned(),
        target: target.map(str::to_owned),
        text: text.to_owned(),
    }
}

fn decode_html(value: &str) -> String {
    value
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn visible_text(html: &str) -> String {
    static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?s)<[^>]+>"#).unwrap());
    let boundaries = html
        .replace("<br>\n", "\n")
        .replace("<br>", "\n")
        .replace("</p><div class=\"footnotes-footer\">", "\n")
        .replace(
            "</div><div class=\"footnote-footer\"",
            "\n<div class=\"footnote-footer\"",
        );
    decode_html(&TAG.replace_all(&boundaries, ""))
}

fn render(source: &str) -> (String, String) {
    let page_info = PageInfo {
        page: Cow::Borrowed("syntax-differential"),
        category: None,
        site: Cow::Borrowed("syntax-differential"),
        title: Cow::Borrowed("Single-link recovery"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    };
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, _) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;
    let text = visible_text(&html);
    (html, text)
}

fn anchors(html: &str) -> Vec<Anchor> {
    static ANCHOR: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?s)<a\b(?P<attrs>[^>]*)>(?P<text>.*?)</a>"#).unwrap()
    });
    static HREF: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"\bhref="(?P<value>[^"]*)""#).unwrap());
    static TARGET: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"\btarget="(?P<value>[^"]*)""#).unwrap());
    static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?s)<[^>]+>"#).unwrap());

    ANCHOR
        .captures_iter(html)
        .filter_map(|capture| {
            let attrs = capture.name("attrs")?.as_str();
            let href = HREF.captures(attrs)?.name("value")?.as_str();
            let target = TARGET
                .captures(attrs)
                .and_then(|capture| capture.name("value"))
                .map(|value| value.as_str().to_owned());
            let text = TAG.replace_all(capture.name("text")?.as_str(), "");
            Some(Anchor {
                href: decode_html(href),
                target,
                text: decode_html(&text),
            })
        })
        .collect()
}

#[test]
fn live_backed_single_link_matrix_matches_href_target_and_text() {
    let dense_source = "[https://example.com A]".repeat(20);
    let dense_anchors = (0..20)
        .map(|_| anchor("https://example.com", None, "A"))
        .collect::<Vec<_>>();
    let cases = vec![
        (
            "https",
            "[https://example.com Label]",
            vec![anchor("https://example.com", None, "Label")],
            "Label".to_owned(),
        ),
        (
            "http",
            "[http://example.com Label]",
            vec![anchor("http://example.com", None, "Label")],
            "Label".to_owned(),
        ),
        (
            "star",
            "[*https://example.com Label]",
            vec![anchor("https://example.com", Some("_blank"), "Label")],
            "Label".to_owned(),
        ),
        (
            "relative",
            "[/start Label]",
            vec![anchor("/start", None, "Label")],
            "Label".to_owned(),
        ),
        (
            "relative-star",
            "[*/start Label]",
            vec![anchor("/start", Some("_blank"), "Label")],
            "Label".to_owned(),
        ),
        (
            "mailto",
            "[mailto:user@example.com Mail]",
            vec![anchor("mailto:user@example.com", None, "Mail")],
            "Mail".to_owned(),
        ),
        (
            "ftp",
            "[ftp://example.com/a FTP]",
            vec![anchor("ftp://example.com/a", None, "FTP")],
            "FTP".to_owned(),
        ),
        (
            "upper-scheme",
            "[HTTPS://example.com Label]",
            vec![],
            "Label".to_owned(),
        ),
        (
            "mixed-scheme",
            "[HtTpS://example.com Label]",
            vec![],
            "Label".to_owned(),
        ),
        (
            "scheme-no-slashes",
            "[https:example.com Label]",
            vec![],
            "Label".to_owned(),
        ),
        (
            "protocol-relative",
            "[//example.com/a Label]",
            vec![anchor("//example.com/a", None, "Label")],
            "Label".to_owned(),
        ),
        (
            "bare-domain",
            "[example.com Label]",
            vec![],
            "[example.com Label]".to_owned(),
        ),
        ("empty-target", "[ Label]", vec![], "[ Label]".to_owned()),
        (
            "empty-label",
            "[https://example.com ]",
            vec![anchor("https://example.com", None, "https://example.com")],
            "[https://example.com ]".to_owned(),
        ),
        (
            "no-label",
            "[https://example.com]",
            vec![anchor("https://example.com", None, "https://example.com")],
            "[https://example.com]".to_owned(),
        ),
        (
            "space-target",
            "[ https://example.com Label]",
            vec![anchor("https://example.com", None, "https://example.com")],
            "[ https://example.com Label]".to_owned(),
        ),
        (
            "two-spaces",
            "[https://example.com  Label]",
            vec![anchor("https://example.com", None, "Label")],
            "Label".to_owned(),
        ),
        (
            "tab-separator",
            "[https://example.com\tLabel]",
            vec![anchor("https://example.com", None, "Label")],
            "Label".to_owned(),
        ),
        (
            "newline-separator",
            "[https://example.com\nLabel]",
            vec![anchor("https://example.com", None, "https://example.com")],
            "[https://example.com\nLabel]".to_owned(),
        ),
        (
            "newline-label",
            "[https://example.com A\nB]",
            vec![anchor("https://example.com", None, "https://example.com")],
            "[https://example.com A\nB]".to_owned(),
        ),
        (
            "target-pipe",
            "[https://example.com/a|b Label]",
            vec![anchor("https://example.com/a|b", None, "Label")],
            "Label".to_owned(),
        ),
        (
            "target-right-bracket",
            "[https://example.com/a]b Label]",
            vec![anchor("https://example.com/a]b", None, "Label")],
            "Label".to_owned(),
        ),
        (
            "target-left-bracket",
            "[https://example.com/a[b Label]",
            vec![anchor("https://example.com/a[b", None, "Label")],
            "Label".to_owned(),
        ),
        (
            "target-paren",
            "[https://example.com/a(b) Label]",
            vec![anchor("https://example.com/a(b)", None, "Label")],
            "Label".to_owned(),
        ),
        (
            "target-apostrophe",
            "[https://example.com/a'b Label]",
            vec![anchor("https://example.com/a'b", None, "Label")],
            "Label".to_owned(),
        ),
        (
            "target-unicode",
            "[https://example.com/日本語 Label]",
            vec![anchor(
                "https://example.com/%E6%97%A5%E6%9C%AC%E8%AA%9E",
                None,
                "Label",
            )],
            "Label".to_owned(),
        ),
        (
            "target-space-encoded",
            "[https://example.com/a%20b Label]",
            vec![anchor("https://example.com/a%20b", None, "Label")],
            "Label".to_owned(),
        ),
        (
            "label-bold",
            "[https://example.com **A**]",
            vec![anchor("https://example.com", None, "**A**")],
            "**A**".to_owned(),
        ),
        (
            "label-italic",
            "[https://example.com //A//]",
            vec![anchor("https://example.com", None, "//A//")],
            "//A//".to_owned(),
        ),
        (
            "label-mono",
            "[https://example.com {{A}}]",
            vec![anchor("https://example.com", None, "{{A}}")],
            "{{A}}".to_owned(),
        ),
        (
            "label-raw",
            "[https://example.com @@A@@]",
            vec![anchor("https://example.com", None, "https://example.com")],
            "[https://example.com A]".to_owned(),
        ),
        (
            "label-span",
            "[https://example.com [[span]]A[[/span]]]",
            vec![anchor("https://example.com", None, "https://example.com")],
            "[https://example.com A]".to_owned(),
        ),
        (
            "label-comment",
            "[https://example.com A[!--x--]B]",
            vec![anchor("https://example.com", None, "AB")],
            "AB".to_owned(),
        ),
        (
            "label-pipe",
            "[https://example.com A|B]",
            vec![anchor("https://example.com", None, "A|B")],
            "A|B".to_owned(),
        ),
        (
            "label-bracket",
            "[https://example.com A[B]",
            vec![anchor("https://example.com", None, "A[B")],
            "A[B".to_owned(),
        ),
        (
            "label-triple",
            "[https://example.com A[[[start|B]]]C]",
            vec![
                anchor("https://example.com", None, "https://example.com"),
                anchor("/start", None, "B"),
            ],
            "[https://example.com ABC]".to_owned(),
        ),
        (
            "label-footnote",
            "[https://example.com A[[footnote]]N[[/footnote]]B]",
            vec![
                anchor("https://example.com", None, "https://example.com"),
                anchor("javascript:;", None, "1"),
                anchor("javascript:;", None, "1"),
            ],
            "[https://example.com A1B]\nFootnotes\n1. N".to_owned(),
        ),
        (
            "javascript-safe",
            "[javascript:; Label]",
            vec![],
            "Label".to_owned(),
        ),
        (
            "javascript-code",
            "[javascript:void(0) Label]",
            vec![],
            "Label".to_owned(),
        ),
        (
            "data",
            "[data:text/plain,x Label]",
            vec![],
            "[data:text/plain,x Label]".to_owned(),
        ),
        (
            "unknown-scheme",
            "[foo:bar Label]",
            vec![],
            "Label".to_owned(),
        ),
        (
            "unclosed",
            "A[https://example.com LabelB",
            vec![anchor("https://example.com", None, "https://example.com")],
            "A[https://example.com LabelB".to_owned(),
        ),
        (
            "extra-close",
            "A[https://example.com Label]]B",
            vec![anchor("https://example.com", None, "Label")],
            "ALabel]B".to_owned(),
        ),
        (
            "adjacent",
            "[https://example.com A][https://example.org B]",
            vec![
                anchor("https://example.com", None, "A"),
                anchor("https://example.org", None, "B"),
            ],
            "AB".to_owned(),
        ),
        (
            "dense",
            dense_source.as_str(),
            dense_anchors,
            "A".repeat(20),
        ),
    ];

    assert_eq!(cases.len(), 45);
    for (case_id, source, expected_anchors, expected_text) in cases {
        let (html, text) = render(source);
        assert_eq!(anchors(&html), expected_anchors, "{case_id}: {html}");
        assert_eq!(text, expected_text, "{case_id}: {html}");
    }
}

#[test]
fn unsafe_and_noncanonical_schemes_never_become_hrefs() {
    for source in [
        "[JaVaScRiPt:alert(1) XSS]",
        "[DATA:text/html,x XSS]",
        "[vbscript:msgbox(1) XSS]",
    ] {
        let (html, _) = render(source);
        assert!(anchors(&html).is_empty(), "{source:?}: {html}");
        assert!(!html.contains("href=\"javascript:"), "{source:?}: {html}");
        assert!(!html.contains("href=\"data:"), "{source:?}: {html}");
        assert!(!html.contains("href=\"vbscript:"), "{source:?}: {html}");
    }
}

#[test]
fn valid_comments_are_elided_from_single_link_targets_and_labels() {
    for (source, href, label) in [
        (
            "[https://exam[!--x--]ple.com AB]",
            "https://example.com",
            "AB",
        ),
        (
            "[https://example.com A[!--x--]B]",
            "https://example.com",
            "AB",
        ),
        (
            "[https://exam[!--target--]ple.com A[!--label--]B]",
            "https://example.com",
            "AB",
        ),
    ] {
        let (html, text) = render(source);
        assert_eq!(
            anchors(&html),
            vec![anchor(href, None, label)],
            "{source:?}: {html}",
        );
        assert_eq!(text, label, "{source:?}: {html}");
    }
}

#[test]
fn malformed_nested_owners_remain_literal_link_labels() {
    for (source, label) in [
        ("[https://example.com @@A]", "@@A"),
        ("[https://example.com @<A]", "@<A"),
    ] {
        let (html, _) = render(source);
        assert_eq!(
            anchors(&html),
            vec![anchor("https://example.com", None, label)],
            "{source:?}: {html}",
        );
    }
}

#[test]
fn complete_inline_bibcite_owns_single_link_label_recovery() {
    let source = "[https://example.com ((bibcite alpha))]";
    let (html, text) = render(source);

    assert_eq!(
        anchors(&html),
        vec![anchor("https://example.com", None, "https://example.com")],
        "{html}",
    );
    assert_eq!(
        text, "[https://example.com Bibliography item alpha not found.]",
        "{html}",
    );
    assert_eq!(html.matches("error-inline").count(), 1, "{html}");
}

#[test]
fn bibcite_owner_transfer_is_complete_nested_and_comment_aware() {
    for source in [
        "[https://example.com **((bibcite alpha))**]",
        "[https://example.com [!--before--]((bibcite alpha))[!--after--]]",
    ] {
        let (html, text) = render(source);
        assert_eq!(
            anchors(&html),
            vec![anchor("https://example.com", None, "https://example.com")],
            "{source:?}: {html}",
        );
        assert_eq!(
            html.matches("error-inline").count(),
            1,
            "{source:?}: {html}"
        );
        assert!(
            text.starts_with("[https://example.com "),
            "{source:?}: {html}"
        );
        assert!(text.ends_with(']'), "{source:?}: {html}");
    }

    let source = concat!(
        "[https://example.com ",
        "((bibcite alpha))((bibcite beta))",
        "]]",
    );
    let (html, text) = render(source);
    assert_eq!(
        anchors(&html),
        vec![anchor("https://example.com", None, "https://example.com")],
        "{html}",
    );
    assert_eq!(html.matches("error-inline").count(), 2, "{html}");
    assert!(text.ends_with("]]"), "{html}");
}

#[test]
fn empty_invalid_malformed_and_commented_bibcites_do_not_take_link_ownership() {
    for (source, label) in [
        ("[https://example.com ((bibcite alpha)]", "((bibcite alpha)"),
        ("[https://example.com ((bibcite ))]", "((bibcite ))"),
        (
            "[https://example.com ((bibcite alpha|beta))]",
            "((bibcite alpha|beta))",
        ),
        (
            "[https://example.com [!--((bibcite alpha))--]Label]",
            "Label",
        ),
    ] {
        let (html, _) = render(source);
        assert_eq!(
            anchors(&html),
            vec![anchor("https://example.com", None, label)],
            "{source:?}: {html}",
        );
        assert!(!html.contains("error-inline"), "{source:?}: {html}");
    }
}

#[test]
fn bibcite_link_recovery_escapes_invalid_html_labels() {
    let source = "[https://example.com ((bibcite <script>alert_1</script>))]";
    let (html, _) = render(source);

    assert_eq!(
        anchors(&html),
        vec![anchor(
            "https://example.com",
            None,
            "((bibcite <script>alert_1</script>))",
        )],
        "{html}",
    );
    assert!(!html.contains("<script>"), "{html}");
    assert!(html.contains("&lt;script&gt;"), "{html}");
}

#[test]
fn malformed_bibcite_owner_candidates_stay_bounded() {
    let malformed = "((bibcite a(".repeat(8 * 1024);
    let source = format!("[https://example.com {malformed}]");
    let started = Instant::now();
    let (html, _) = render(&source);
    let elapsed = started.elapsed();

    assert_eq!(html.matches("href=\"https://example.com\"").count(), 1);
    assert!(!html.contains("error-inline"), "{html}");
    assert!(
        elapsed < Duration::from_secs(5),
        "bibcite owner scan took {elapsed:?}",
    );
}

#[test]
fn closer_runs_leave_every_extra_bracket_as_residual_text() {
    for (source, expected_text) in [
        ("A[https://example.com Label]]B", "ALabel]B"),
        ("A[https://example.com Label]]]B", "ALabel]]B"),
        ("A[https://example.com Label]]]]B", "ALabel]]]B"),
    ] {
        let (html, text) = render(source);
        assert_eq!(
            anchors(&html),
            vec![anchor("https://example.com", None, "Label")],
            "{source:?}: {html}",
        );
        assert_eq!(text, expected_text, "{source:?}: {html}");
    }
}

#[test]
fn long_and_dense_single_link_scans_stay_bounded() {
    let long_target = format!("https://example.com/{}", "a".repeat(16 * 1024));
    let malformed_owners = "@<".repeat(8 * 1024);
    let unit = concat!(
        "[https://example.com/a]b Label] ",
        "[HTTPS://example.com Label] ",
        "[https://example.com @@A@@] ",
        "[https://example.com Label]] ",
    );
    let source = format!(
        "[{long_target} Label] {} [https://example.com {malformed_owners}]",
        unit.repeat(2_048),
    );
    let started = Instant::now();
    let (html, _) = render(&source);
    let elapsed = started.elapsed();

    assert!(html.contains("https://example.com/aaaa"), "{html}");
    assert_eq!(html.matches(">Label</a>").count(), 4_097, "{html}");
    assert!(html.contains("@&lt;@&lt;"), "{html}");
    assert!(
        elapsed < Duration::from_secs(5),
        "single-link recovery took {elapsed:?}",
    );
}
