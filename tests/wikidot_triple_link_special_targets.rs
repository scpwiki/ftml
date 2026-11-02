use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::parsing::{ParseError, ParseErrorKind};
use ftml::render::{Render, html::HtmlRender, text::TextRender};
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

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("syntax-differential"),
        category: None,
        site: Cow::Borrowed("syntax-differential"),
        title: Cow::Borrowed("Triple-link special targets"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn render(source: &str, layout: Layout) -> (String, String, Vec<ParseError>) {
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

fn render_wikidot(source: &str) -> (String, String, Vec<ParseError>) {
    render(source, Layout::Wikidot)
}

fn decode_html(value: &str) -> String {
    value
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
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
fn live_backed_triple_target_matrix_matches_class_excluded_semantics() {
    let cases = vec![
        (
            "canonical-page",
            "[[[start]]]",
            vec![anchor("/start", None, "start")],
            "start",
        ),
        (
            "canonical-label",
            "[[[start|Label]]]",
            vec![anchor("/start", None, "Label")],
            "Label",
        ),
        (
            "canonical-star",
            "[[[*start|Label]]]",
            vec![anchor("/start", None, "Label")],
            "Label",
        ),
        (
            "canonical-star-nolabel",
            "[[[*start]]]",
            vec![anchor("/start", None, "*start")],
            "*start",
        ),
        ("empty-target", "[[[|Label]]]", vec![], "[[[|Label]]]"),
        (
            "star-empty-target",
            "[[[*|Label]]]",
            vec![anchor("/", None, "Label")],
            "Label",
        ),
        (
            "empty-label",
            "[[[start|]]]",
            vec![anchor("/start", None, "start")],
            "start",
        ),
        (
            "space-label",
            "[[[start| ]]]",
            vec![anchor("/start", None, "start")],
            "start",
        ),
        (
            "leading-space-target",
            "[[[ start|Label]]]",
            vec![anchor("/start", None, "Label")],
            "Label",
        ),
        (
            "trailing-space-target",
            "[[[start |Label]]]",
            vec![anchor("/start", None, "Label")],
            "Label",
        ),
        (
            "both-space-target",
            "[[[ start |Label]]]",
            vec![anchor("/start", None, "Label")],
            "Label",
        ),
        (
            "space-around-pipe",
            "[[[start | Label]]]",
            vec![anchor("/start", None, "Label")],
            "Label",
        ),
        (
            "tabs-around-pipe",
            "[[[start\t|\tLabel]]]",
            vec![anchor("/start", None, "Label")],
            "Label",
        ),
        (
            "newline-before-pipe",
            "[[[start\n|Label]]]",
            vec![anchor("/start", None, "Label")],
            "Label",
        ),
        (
            "newline-after-pipe",
            "[[[start|\nLabel]]]",
            vec![anchor("/start", None, "Label")],
            "Label",
        ),
        (
            "newline-before-close",
            "[[[start|Label\n]]]",
            vec![anchor("/start", None, "Label")],
            "Label",
        ),
        (
            "target-newline-close",
            "[[[start\n]]]",
            vec![anchor("/start", None, "start")],
            "start",
        ),
        (
            "upper-target",
            "[[[START|Label]]]",
            vec![anchor("/start", None, "Label")],
            "Label",
        ),
        (
            "mixed-target",
            "[[[StArT|Label]]]",
            vec![anchor("/start", None, "Label")],
            "Label",
        ),
        (
            "category",
            "[[[system:join|Label]]]",
            vec![anchor("/system:join", None, "Label")],
            "Label",
        ),
        (
            "category-space",
            "[[[system: join|Label]]]",
            vec![anchor("/system:join", None, "Label")],
            "Label",
        ),
        (
            "cross-site",
            "[[[:sandbox-for-codex:start|Label]]]",
            vec![anchor("/sandbox-for-codex:start", None, "Label")],
            "Label",
        ),
        (
            "cross-site-spaces",
            "[[[: sandbox-for-codex : start|Label]]]",
            vec![anchor("/sandbox-for-codex:start", None, "Label")],
            "Label",
        ),
        (
            "slash",
            "[[[foo/bar|Label]]]",
            vec![anchor("/foo-bar", None, "Label")],
            "Label",
        ),
        (
            "leading-slash",
            "[[[/foo/bar|Label]]]",
            vec![anchor("/foo/bar", None, "Label")],
            "Label",
        ),
        (
            "trailing-slash",
            "[[[start/|Label]]]",
            vec![anchor("/start", None, "Label")],
            "Label",
        ),
        (
            "double-slash",
            "[[[foo//bar|Label]]]",
            vec![anchor("/foo-bar", None, "Label")],
            "Label",
        ),
        (
            "anchor-hash",
            "[[[start#toc1|Label]]]",
            vec![anchor("/start#toc1", None, "Label")],
            "Label",
        ),
        (
            "hash-route",
            "[[[start/#/page|Label]]]",
            vec![anchor("/start#/page", None, "Label")],
            "Label",
        ),
        (
            "bad-hash3",
            "[[[start###|Label]]]",
            vec![],
            "[[[start###|Label]]]",
        ),
        (
            "bad-hash-route",
            "[[[start/##/page|Label]]]",
            vec![],
            "[[[start/##/page|Label]]]",
        ),
        (
            "query",
            "[[[start?x=1|Label]]]",
            vec![anchor("/start-x-1", None, "Label")],
            "Label",
        ),
        (
            "query-hash",
            "[[[start?x=1#toc1|Label]]]",
            vec![anchor("/start-x-1#toc1", None, "Label")],
            "Label",
        ),
        (
            "dot",
            "[[[.|Label]]]",
            vec![anchor("/", None, "Label")],
            "Label",
        ),
        (
            "dotdot",
            "[[[..|Label]]]",
            vec![anchor("/", None, "Label")],
            "Label",
        ),
        (
            "colon-only",
            "[[[:|Label]]]",
            vec![anchor("/", None, "Label")],
            "Label",
        ),
        (
            "bang",
            "[[[!start|Label]]]",
            vec![anchor("/start", None, "Label")],
            "Label",
        ),
        (
            "bang-cross",
            "[[[!:sandbox-for-codex:start|Label]]]",
            vec![anchor("/sandbox-for-codex:start", None, "Label")],
            "Label",
        ),
        (
            "http",
            "[[[https://example.com|Label]]]",
            vec![anchor("https://example.com", None, "Label")],
            "Label",
        ),
        (
            "http-leading-space",
            "[[[ https://example.com|Label]]]",
            vec![anchor("/https:example-com", None, "Label")],
            "Label",
        ),
        (
            "http-trailing-space",
            "[[[https://example.com |Label]]]",
            vec![anchor("https://example.com", None, "Label")],
            "Label",
        ),
        (
            "mailto",
            "[[[mailto:user@example.com|Label]]]",
            vec![anchor("mailto:user@example.com", None, "Label")],
            "Label",
        ),
        (
            "ftp",
            "[[[ftp://example.com/a|Label]]]",
            vec![anchor("ftp://example.com/a", None, "Label")],
            "Label",
        ),
        (
            "unicode-target",
            "[[[日本語|Label]]]",
            vec![anchor("/", None, "Label")],
            "Label",
        ),
        (
            "percent-target",
            "[[[a%20b|Label]]]",
            vec![anchor("/a-20b", None, "Label")],
            "Label",
        ),
        (
            "plus-target",
            "[[[a+b|Label]]]",
            vec![anchor("/a-b", None, "Label")],
            "Label",
        ),
        (
            "dot-target",
            "[[[a.b|Label]]]",
            vec![anchor("/a-b", None, "Label")],
            "Label",
        ),
        (
            "semicolon-target",
            "[[[a;b|Label]]]",
            vec![anchor("/a-b", None, "Label")],
            "Label",
        ),
        (
            "comma-target",
            "[[[a,b|Label]]]",
            vec![anchor("/a-b", None, "Label")],
            "Label",
        ),
        (
            "apostrophe-target",
            "[[[a'b|Label]]]",
            vec![anchor("/a-b", None, "Label")],
            "Label",
        ),
        (
            "quote-target",
            "[[[a\"b|Label]]]",
            vec![anchor("/a-b", None, "Label")],
            "Label",
        ),
    ];

    let mut failures = Vec::new();
    for (case_id, source, expected_anchors, expected_text) in cases {
        let (html, text, _) = render_wikidot(source);
        let actual_anchors = anchors(&html);
        if actual_anchors != expected_anchors {
            failures.push(format!(
                "{case_id} anchors:\n  actual: {actual_anchors:?}\nexpected: {expected_anchors:?}\nhtml: {html}",
            ));
        }
        if text != expected_text {
            failures.push(format!(
                "{case_id} text:\n  actual: {text:?}\nexpected: {expected_text:?}\nhtml: {html}",
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn live_backed_mailto_variants_split_the_label_at_the_pipe() {
    let cases = [
        (
            "lower",
            "[[[mailto:user@example.com|Label]]]",
            "mailto:user@example.com",
            None,
            "Label",
        ),
        (
            "upper-local",
            "[[[MAILTO:User@Example.COM|Label]]]",
            "/mailto:user-example-com",
            None,
            "Label",
        ),
        (
            "mixed-local",
            "[[[MailTo:User@Example.COM|Label]]]",
            "/mailto:user-example-com",
            None,
            "Label",
        ),
        (
            "punctuation",
            "[[[mailto:first.last+tag_name-test@example-domain.com|Label]]]",
            "mailto:first.last+tag_name-test@example-domain.com",
            None,
            "Label",
        ),
        (
            "query",
            "[[[mailto:user@example.com?subject=Hello%20World&cc=two@example.com|Label]]]",
            "mailto:user@example.com?subject=Hello%20World&cc=two@example.com",
            None,
            "Label",
        ),
        (
            "empty-label",
            "[[[mailto:user@example.com|]]]",
            "mailto:user@example.com",
            None,
            "mailto:user-example-com",
        ),
        (
            "star",
            "[[[*mailto:user@example.com|Label]]]",
            "mailto:user@example.com",
            Some("_blank"),
            "Label",
        ),
        (
            "apostrophe",
            "[[[mailto:o'hara@example.com|Label]]]",
            "mailto:o'hara@example.com",
            None,
            "Label",
        ),
    ];

    for (case_id, source, href, target, label) in cases {
        let (html, text, errors) = render_wikidot(source);
        assert!(errors.is_empty(), "{case_id}: {errors:#?}");
        assert_eq!(
            anchors(&html),
            vec![anchor(href, target, label)],
            "{case_id}: {html}"
        );
        assert_eq!(text, label, "{case_id}: {html}");
        assert!(!html.contains(" rel="), "{case_id}: {html}");
    }
}

#[test]
fn special_targets_preserve_exact_dom_text_and_error_boundaries() {
    for (source, expected_html) in [
        (
            "[[[.|Label]]]",
            r#"<p><a class="newpage" href="/">Label</a></p>"#,
        ),
        (
            "[[[..|Label]]]",
            r#"<p><a class="newpage" href="/">Label</a></p>"#,
        ),
        (
            "[[[:|Label]]]",
            r#"<p><a class="newpage" href="/">Label</a></p>"#,
        ),
        (
            "[[[日本語|Label]]]",
            r#"<p><a class="newpage" href="/">Label</a></p>"#,
        ),
    ] {
        let (html, text, errors) = render_wikidot(source);
        assert_eq!(html, expected_html, "{source:?}");
        assert_eq!(text, "Label", "{source:?}");
        assert!(errors.is_empty(), "{source:?}: {errors:#?}");
    }

    for (source, expected_html, expected_text) in [
        ("A[[[.|LabelB", "<p>A[[[.|LabelB</p>", "A[[[.|LabelB"),
        (
            "A[[[mailto:user@example.com|Label]]B",
            "<p>A[[[<a href=\"mailto:user@example.com|Label\">mailto:user@example.com|Label</a>]]B</p>",
            "A[[[mailto:user@example.com|Label]]B",
        ),
        ("A[[[:|Label]B", "<p>A[[[:|Label]B</p>", "A[[[:|Label]B"),
    ] {
        let (html, text, errors) = render_wikidot(source);
        assert_eq!(html, expected_html, "{source:?}");
        assert_eq!(text, expected_text, "{source:?}");
        assert!(
            errors.iter().any(|error| matches!(
                error.kind(),
                ParseErrorKind::EndOfInput | ParseErrorKind::NoRulesMatch
            )),
            "{source:?}: {errors:#?}",
        );
    }

    let source = "A\r\n[[[日本語|Label]]]\r\nB";
    let (html, text, errors) = render_wikidot(source);
    assert_eq!(
        html,
        "<p>A<br>\n<a class=\"newpage\" href=\"/\">Label</a><br>\nB</p>"
    );
    assert_eq!(text, "A\nLabel\nB");
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn literal_owners_do_not_activate_special_targets() {
    let source = concat!(
        "[[code]]\n[[[.|Code]]] [[[mailto:user@example.com|Code]]]\n[[/code]]\n",
        "@@[[[..|Raw]]]@@\n",
        "A[!--[[[:|Comment]]]--]B\n",
        "[[html]]<a href=\"javascript:alert(1)\">[[[日本語|HTML]]]</a>[[/html]]",
    );
    let (html, text, errors) = render_wikidot(source);

    assert!(errors.is_empty(), "{errors:#?}");
    assert!(
        html.contains("[[[.|Code]]] [[[mailto:user@example.com|Code]]]"),
        "{html}"
    );
    assert!(html.contains("[[[..|Raw]]]"), "{html}");
    assert!(html.contains("AB"), "{html}");
    assert!(!html.contains("javascript:alert(1)"), "{html}");
    assert_eq!(anchors(&html), Vec::<Anchor>::new(), "{html}");
    assert!(
        text.contains("[[[.|Code]]] [[[mailto:user@example.com|Code]]]"),
        "{text}"
    );
}

#[test]
fn unsafe_schemes_stay_local_or_sanitized_in_both_layouts() {
    for layout in [Layout::Wikidot, Layout::Wikijump] {
        let source = concat!(
            "[[[javascript:alert(1)|JS]]] ",
            "[[[data:text/html,<script>|Data]]] ",
            "[[[*javascript:alert(1)|Star]]]",
        );
        let (html, _, _) = render(source, layout);
        assert!(
            !html.contains(r#"href="javascript:""#),
            "{layout:?}: {html}"
        );
        assert!(!html.contains(r#"href="data:""#), "{layout:?}: {html}");
        assert!(!html.contains("<script>"), "{layout:?}: {html}");
    }
}

#[test]
fn wikijump_layout_keeps_its_existing_special_target_behavior() {
    let source = "[[[.|Dot]]] [[[..|Dotdot]]] [[[:|Colon]]] [[[日本語|Unicode]]]";
    let (html, text, _) = render(source, Layout::Wikijump);
    assert!(html.contains(r#"href="/.""#), "{html}");
    assert!(html.contains(r#"href="/..""#), "{html}");
    assert!(
        html.contains(r##"href="#invalid-url">Colon</a>"##),
        "{html}"
    );
    assert!(
        html.contains(r#"href="/%E6%97%A5%E6%9C%AC%E8%AA%9E""#),
        "{html}"
    );
    assert_eq!(text, "Dot Dotdot Colon Unicode");
}

#[test]
fn live_backed_triple_link_transliteration_targets() {
    for (case_id, source, expected_anchors, expected_text) in [
        (
            "cyrillic-same-label",
            "[[[документ]]]",
            vec![anchor("/dokument", None, "документ")],
            "документ",
        ),
        (
            "diacritic-same-label",
            "[[[café]]]",
            vec![anchor("/cafe", None, "café")],
            "café",
        ),
        (
            "cyrillic-separate-label",
            "[[[документ|Label]]]",
            vec![anchor("/dokument", None, "Label")],
            "Label",
        ),
        (
            "diacritic-separate-label",
            "[[[café|Label]]]",
            vec![anchor("/cafe", None, "Label")],
            "Label",
        ),
        (
            "untransliteratable-same-label",
            "[[[日本語]]]",
            vec![anchor("/", None, "日本語")],
            "日本語",
        ),
    ] {
        let (html, text, errors) = render_wikidot(source);
        assert_eq!(anchors(&html), expected_anchors, "{case_id}: {html}");
        assert_eq!(text, expected_text, "{case_id}: {html}");
        assert!(errors.is_empty(), "{case_id}: {errors:#?}");
    }
}

#[test]
fn long_unicode_colon_slash_and_mailto_targets_stay_bounded() {
    let unicode = "日本語🙂".repeat(4_096);
    let colon = format!(":site:{}", "page:".repeat(4_096));
    let slash = format!("root/{}tail", "segment/".repeat(4_096));
    let mailto = format!("mailto:user@example.com?subject={}", "x".repeat(32 * 1024));
    let source = format!(
        "[[[{unicode}|Unicode]]] [[[{colon}|Colon]]] [[[{slash}|Slash]]] [[[{mailto}|Mail]]]",
    );
    let started = Instant::now();
    let (html, text, errors) = render_wikidot(&source);
    let elapsed = started.elapsed();

    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(anchors(&html).len(), 4, "{html:.400}");
    assert_eq!(text, "Unicode Colon Slash Mail");
    assert!(
        html.len() <= source.len() * 2,
        "{} > {}",
        html.len(),
        source.len() * 2
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "special-target parsing took {elapsed:?}"
    );
}
