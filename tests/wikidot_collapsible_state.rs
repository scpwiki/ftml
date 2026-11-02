use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::parsing::{ParseError, ParseErrorKind};
use ftml::render::{Render, html::HtmlRender, text::TextRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

const V7_RENDERER_SHA256: &str =
    "c2b4ee78c5b37e7c8eb1c5800426ed0249b3346347c0d05007b6ecff0df6ee11";

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("issue-320-collapsible-state"),
        category: None,
        site: Cow::Borrowed("scp-wiki"),
        title: Cow::Borrowed("Issue 320 collapsible state"),
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
    ftml::preprocess_for_layout(&mut source, layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;
    let text = TextRender.render(&tree, &page_info, &settings);
    (html, text, errors)
}

fn collapsible_html(
    body: &str,
    start_open: bool,
    show: &str,
    hide: &str,
    show_top: bool,
    show_bottom: bool,
) -> String {
    let folded_style = if start_open {
        r#" style="display:none""#
    } else {
        ""
    };
    let unfolded_style = if start_open {
        ""
    } else {
        r#" style="display:none""#
    };
    let hide_link = format!(
        concat!(
            r#"<div class="collapsible-block-unfolded-link">"#,
            r#"<a class="collapsible-block-link" href="javascript:;">{hide}</a></div>"#,
        ),
        hide = hide,
    );

    format!(
        concat!(
            r#"<div class="collapsible-block">"#,
            r#"<div class="collapsible-block-folded"{folded_style}>"#,
            r#"<a class="collapsible-block-link" href="javascript:;">{show}</a></div>"#,
            r#"<div class="collapsible-block-unfolded"{unfolded_style}>"#,
            "{top}",
            r#"<div class="collapsible-block-content">{body}</div>"#,
            "{bottom}",
            "</div></div>",
        ),
        folded_style = folded_style,
        show = show,
        unfolded_style = unfolded_style,
        body = body,
        top = if show_top { &hide_link } else { "" },
        bottom = if show_bottom { &hide_link } else { "" },
    )
}

fn default_collapsible(body: &str) -> String {
    collapsible_html(
        body,
        false,
        "+&nbsp;show&nbsp;block",
        "–&nbsp;hide&nbsp;block",
        true,
        false,
    )
}

fn error_kinds(errors: &[ParseError]) -> Vec<ParseErrorKind> {
    errors.iter().map(ParseError::kind).collect()
}

#[test]
fn v7_duplicate_and_empty_folded_values_keep_the_collapsible() {
    let expected = concat!(
        r#"<div class="collapsible-block"><div class="collapsible-block-folded">"#,
        r#"<a class="collapsible-block-link" href="javascript:;">+&nbsp;show&nbsp;block</a>"#,
        r#"</div><div class="collapsible-block-unfolded" style="display:none">"#,
        r#"<div class="collapsible-block-unfolded-link"><a class="collapsible-block-link" "#,
        r#"href="javascript:;">–&nbsp;hide&nbsp;block</a></div>"#,
        r#"<div class="collapsible-block-content"><p>v7 body</p></div></div></div>"#,
    );

    for source in [
        "[[collapsible folded=\"one\" folded=\"two\"]]\nv7 body\n[[/collapsible]]",
        "[[collapsible folded=\"\"]]\nv7 body\n[[/collapsible]]",
    ] {
        let (html, text, errors) = render(source, Layout::Wikidot);
        assert!(errors.is_empty(), "{source:?}: {errors:#?}");
        assert_eq!(html, expected);
        assert_eq!(text, "v7 body");
    }
}

#[test]
fn full_v7_collapsible_family_matches_saved_page_preview() {
    let ordinary = [
        ("[[collapsible]]\nv7 body\n[[/collapsible]]", "v7 body"),
        ("[[COLLAPSIBLE]]\nv7 body\n[[/COLLAPSIBLE]]", "v7 body"),
        (
            "[[collapsible]]\nserialized body\n[[/collapsible]]",
            "serialized body",
        ),
        (
            "[[collapsible]]\nvisible text\n[[/collapsible]]",
            "visible text",
        ),
        (
            "[[collapsible folded=\"one\" folded=\"two\"]]\nv7 body\n[[/collapsible]]",
            "v7 body",
        ),
        (
            "[[collapsible folded=\"\"]]\nv7 body\n[[/collapsible]]",
            "v7 body",
        ),
        (
            "[[collapsible v7UnknownArgument=\"x\"]]\nv7 body\n[[/collapsible]]",
            "v7 body",
        ),
        (
            "[[collapsible folded='single quoted' data-v7=unquoted]]\nv7 body\n[[/collapsible]]",
            "v7 body",
        ),
        (
            "[[collapsible style=\"background:url(javascript:alert(1))\"]]\nv7 body\n[[/collapsible]]",
            "v7 body",
        ),
        (
            "[[collapsible style=\"https://example.test/%6a%61vascript%3aalert(1)\"]]\nv7 body\n[[/collapsible]]",
            "v7 body",
        ),
        (
            "[[collapsible style=\"https:\\\\example.test\\path\"]]\nv7 body\n[[/collapsible]]",
            "v7 body",
        ),
    ];
    for (source, body) in ordinary {
        let (html, text, errors) = render(source, Layout::Wikidot);
        assert!(
            errors.is_empty(),
            "renderer {V7_RENDERER_SHA256}, {source:?}: {errors:#?}",
        );
        assert_eq!(html, default_collapsible(&format!("<p>{body}</p>")));
        assert_eq!(text, body);
    }

    let whitespace = "[[collapsible]]\nalpha\tbeta gamma\n[[/collapsible]]";
    let (html, text, errors) = render(whitespace, Layout::Wikidot);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(html, default_collapsible("<p>alpha beta gamma</p>"));
    assert_eq!(text, "alpha beta gamma");

    let boundary = concat!(
        "start-[[collapsible]]\nv7 body\n[[/collapsible]]-middle\n\n",
        "[[collapsible]]\nend\n[[/collapsible]]",
    );
    let (html, text, errors) = render(boundary, Layout::Wikidot);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        html,
        format!(
            "start-{}-middle\n{}",
            default_collapsible("<p>v7 body</p>"),
            default_collapsible("<p>end</p>"),
        ),
    );
    assert_eq!(text, "start-\nv7 body\n-middle\n\nend");

    let malformed = [
        (
            "[[collapsible",
            "<p>[[collapsible</p>",
            "[[collapsible",
            vec![ParseErrorKind::EndOfInput, ParseErrorKind::NoRulesMatch],
        ),
        (
            "[[collapsible]]\nunterminated body",
            "<p>[[collapsible]]<br>\nunterminated body</p>",
            "[[collapsible]]\nunterminated body",
            vec![
                ParseErrorKind::EndOfInput,
                ParseErrorKind::NoRulesMatch,
                ParseErrorKind::NoRulesMatch,
            ],
        ),
    ];
    for (source, expected_html, expected_text, expected_errors) in malformed {
        let (html, text, errors) = render(source, Layout::Wikidot);
        assert_eq!(html, expected_html);
        assert_eq!(text, expected_text);
        assert_eq!(error_kinds(&errors), expected_errors);
    }

    let nested =
        "[[collapsible]]\n[[collapsible]]\nnested\n[[/collapsible]]\n[[/collapsible]]";
    let (html, text, errors) = render(nested, Layout::Wikidot);
    assert_eq!(
        html,
        format!(
            "{}<br>\n[[/collapsible]]",
            default_collapsible("<p>[[collapsible]]<br>\nnested</p>"),
        ),
    );
    assert_eq!(text, "[[collapsible]]\nnested\n\n[[/collapsible]]");
    assert_eq!(
        error_kinds(&errors),
        vec![
            ParseErrorKind::RuleFailed,
            ParseErrorKind::NoRulesMatch,
            ParseErrorKind::NoRulesMatch,
            ParseErrorKind::NoRulesMatch,
            ParseErrorKind::NoRulesMatch,
        ],
    );

    let different = "[[collapsible]]\n[[bold]]nested[[/bold]]\n[[/collapsible]]";
    let (html, text, errors) = render(different, Layout::Wikidot);
    assert_eq!(html, default_collapsible("<p>[[bold]]nested[[/bold]]</p>"),);
    assert_eq!(text, "[[bold]]nested[[/bold]]");
    assert_eq!(errors.len(), 5);

    let crossed = "[[collapsible]]\nouter [[bold]]inner\n[[/collapsible]][[/bold]]";
    let (html, text, errors) = render(crossed, Layout::Wikidot);
    assert_eq!(
        html,
        format!(
            "{}[[/bold]]",
            default_collapsible("<p>outer [[bold]]inner</p>"),
        ),
    );
    assert_eq!(text, "outer [[bold]]inner\n[[/bold]]");
    assert_eq!(errors.len(), 5);
}

#[test]
fn wikidot_collapsible_arguments_use_last_exact_value_and_permissive_defaults() {
    let cases = [
        (r#"folded="""#, false, true, false),
        (r#"folded="no""#, true, true, false),
        (r#"folded="false""#, true, true, false),
        (r#"folded="False""#, false, true, false),
        (r#"folded="0""#, false, true, false),
        (r#"folded="garbage""#, false, true, false),
        (r#"folded="no" folded="yes""#, false, true, false),
        (r#"folded="yes" folded="no""#, true, true, false),
        (r#"hideLocation="""#, false, true, false),
        (r#"hideLocation="side""#, false, true, false),
        (r#"hideLocation="bottom""#, false, false, true),
        (r#"hideLocation="both""#, false, true, true),
        (r#"hideLocation="BOTH""#, false, true, false),
        (
            r#"hideLocation="bottom" hideLocation="both""#,
            false,
            true,
            true,
        ),
        (
            r#"Folded="no" Show="UP" Hide="DOWN" HIDELOCATION="both""#,
            false,
            true,
            false,
        ),
        (
            "folded='no' show='S' hide='H' hideLocation='both'",
            false,
            true,
            false,
        ),
        (
            "folded=no show=S hide=H hideLocation=both",
            false,
            true,
            false,
        ),
        (r#"folded="no show="S""#, false, true, false),
        (r#"show="OPEN"#, false, true, false),
        (r#"hide="CLOSE"#, false, true, false),
        (r#"hideLocation="both"#, false, true, false),
        (r#"show="OPEN hide="CLOSE""#, false, true, false),
        (r#"hide="CLOSE folded="no""#, false, true, false),
        (r#"hideLocation="both show="OPEN""#, false, true, false),
        (r#"unknown="value""#, false, true, false),
    ];

    for (arguments, start_open, show_top, show_bottom) in cases {
        let source = format!("[[collapsible {arguments}]]B[[/collapsible]]");
        let (html, text, errors) = render(&source, Layout::Wikidot);
        assert!(errors.is_empty(), "{source:?}: {errors:#?}");
        assert_eq!(
            html,
            collapsible_html(
                "<p>B</p>",
                start_open,
                "+&nbsp;show&nbsp;block",
                "–&nbsp;hide&nbsp;block",
                show_top,
                show_bottom,
            ),
            "{source:?}",
        );
        assert_eq!(text, "B");
    }

    for arguments in [r#"show="" hide="""#, r#"show="0" hide="0""#] {
        let source = format!("[[collapsible {arguments}]]B[[/collapsible]]");
        let (html, _, errors) = render(&source, Layout::Wikidot);
        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html, default_collapsible("<p>B</p>"));
    }

    let source = concat!(
        "[[collapsible show=\"FIRST\" show=\"LAST\" ",
        "hide=\"FIRST-H\" hide=\"LAST-H\"]]B[[/collapsible]]",
    );
    let (html, _, errors) = render(source, Layout::Wikidot);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        html,
        collapsible_html("<p>B</p>", false, "LAST", "LAST-H", true, false),
    );
}

#[test]
fn wikijump_collapsible_argument_behavior_is_unchanged() {
    let malformed = "[[collapsible folded=\"garbage\"]]B[[/collapsible]]";
    let (html, text, errors) = render(malformed, Layout::Wikijump);
    assert_eq!(
        html,
        "<p>[[collapsible folded=&quot;garbage&quot;]]B[[/collapsible]]</p>"
    );
    assert_eq!(text, malformed);
    assert!(
        errors
            .iter()
            .any(|error| error.kind() == ParseErrorKind::BlockMalformedArguments),
        "{errors:#?}",
    );

    let source =
        "[[collapsible folded=\"NO\" show=\"0\" hideLocation=\"BOTH\"]]B[[/collapsible]]";
    let (html, text, errors) = render(source, Layout::Wikijump);
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(
        html.starts_with("<details class=\"wj-collapsible\" open"),
        "{html}"
    );
    assert!(html.contains("data-show-top data-show-bottom"), "{html}");
    assert!(html.contains(">0</span>"), "{html}");
    assert_eq!(text, "B");
}

#[test]
fn wikidot_quoted_crossed_center_close_leaves_following_text_outside_content() {
    let source = concat!(
        "> [[=]]\n",
        "> [[collapsible show=\"poetry\" hide=\"poetry\"]]\n",
        "> [[/=]]\n",
        "> originally written here\n",
        "> [[/collapsible]]\n",
        "outside\n",
    );
    let (html, _, errors) = render(source, Layout::Wikidot);
    assert!(errors.is_empty(), "{errors:#?}\nHTML: {html}");
    assert_eq!(
        html,
        concat!(
            r#"<blockquote><div style="text-align: center;"><div class="collapsible-block">"#,
            r#"<div class="collapsible-block-folded"><a class="collapsible-block-link" href="javascript:;">poetry</a></div>"#,
            r#"<div class="collapsible-block-unfolded" style="display:none"><div class="collapsible-block-unfolded-link">"#,
            r#"<a class="collapsible-block-link" href="javascript:;">poetry</a></div>"#,
            r#"<div class="collapsible-block-content"></div><br>"#,
            "\noriginally written here</div></div></div></blockquote><p>outside</p>",
        ),
    );

    let ordinary = concat!(
        "> [[=]]\n",
        "> [[collapsible show=\"poetry\" hide=\"poetry\"]]\n",
        "> ordinary body\n",
        "> [[/collapsible]]\n",
        "> [[/=]]\n",
        "outside\n",
    );
    let (ordinary_html, _, ordinary_errors) = render(ordinary, Layout::Wikidot);
    assert!(ordinary_errors.is_empty(), "{ordinary_errors:#?}");
    assert!(
        ordinary_html.contains(
            r#"<div class="collapsible-block-content"><p>ordinary body</p></div>"#,
        ),
        "{ordinary_html}",
    );

    let (wikijump_html, _, wikijump_errors) = render(source, Layout::Wikijump);
    assert!(wikijump_errors.is_empty(), "{wikijump_errors:#?}");
    assert!(
        wikijump_html.contains(r#"<details class="wj-collapsible""#),
        "{wikijump_html}",
    );
    assert!(
        wikijump_html.contains(
            r#"<div class="wj-collapsible-content"><p>originally written here</p></div>"#,
        ),
        "{wikijump_html}",
    );
}

#[test]
fn wikidot_collapsible_paragraph_boundaries_match_inline_and_empty_bodies() {
    let a = default_collapsible("<p>A</p>");
    let b = default_collapsible("<p>B</p>");
    let empty = default_collapsible("");
    let cases = [
        (
            "prefix[[collapsible]]\nB\n[[/collapsible]]suffix",
            format!("prefix{}suffix", default_collapsible("<p>B</p>")),
            "prefix\nB\nsuffix",
        ),
        (
            "[[collapsible]]A[[/collapsible]][[collapsible]]B[[/collapsible]]",
            format!("{a}{b}"),
            "A\n\nB",
        ),
        (
            "a[[collapsible]][[/collapsible]]b",
            format!("a{empty}b"),
            "ab",
        ),
        (
            "a\n[[collapsible]]\n[[/collapsible]]\nb",
            format!("<p>a</p>{empty}<br>\nb"),
            "a\n\nb",
        ),
    ];

    for (source, expected_html, expected_text) in cases {
        let (html, text, errors) = render(source, Layout::Wikidot);
        assert!(errors.is_empty(), "{source:?}: {errors:#?}");
        assert_eq!(html, expected_html, "{source:?}");
        assert_eq!(text, expected_text, "{source:?}");
    }

    let source =
        "雪[[collapsible show=\"開く\" hide=\"閉じる\"]]\r\n本文🙂\r\n[[/collapsible]]ω";
    let (html, text, errors) = render(source, Layout::Wikidot);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        html,
        format!(
            "雪{}ω",
            collapsible_html("<p>本文🙂</p>", false, "開く", "閉じる", true, false),
        ),
    );
    assert_eq!(text, "雪\n本文🙂\nω");
}

#[test]
fn wikidot_collapsible_labels_remain_escaped_text() {
    let source = concat!(
        r#"[[collapsible show="<img src=x onerror=alert(1)>" "#,
        r#"hide="<svg/onload=alert(2)> & ' "]]B[[/collapsible]]"#,
    );
    let (html, text, errors) = render(source, Layout::Wikidot);
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(!html.contains("<img"), "{html}");
    assert!(!html.contains("<svg"), "{html}");
    assert!(
        html.contains("&lt;img&nbsp;src=x&nbsp;onerror=alert(1)&gt;"),
        "{html}",
    );
    assert!(
        html.contains("&lt;svg/onload=alert(2)&gt;&nbsp;&amp;&nbsp;&#39;&nbsp;"),
        "{html}",
    );
    assert_eq!(html.matches("href=\"javascript:;\"").count(), 2);
    assert!(!html.contains(" onclick="), "{html}");
    assert_eq!(text, "B");

    let (html, _, errors) = render(
        r#"[[collapsible show="**bold**" hide="//italic//"]]B[[/collapsible]]"#,
        Layout::Wikidot,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(html.contains(">**bold**</a>"), "{html}");
    assert!(html.contains(">//italic//</a>"), "{html}");
    assert!(!html.contains("<strong>"), "{html}");
    assert!(!html.contains("<em>"), "{html}");
}

#[test]
fn collapsible_markers_stay_literal_inside_literal_owners() {
    let sources = [
        "[[code]]\n[[collapsible folded=\"no\"]]C[[/collapsible]]\n[[/code]]",
        "@@[[collapsible folded=\"no\"]]R[[/collapsible]]@@",
        "[!-- [[collapsible folded=\"no\"]]M[[/collapsible]] --]",
        "[[html]]\n[[collapsible folded=\"no\"]]H[[/collapsible]]\n[[/html]]",
    ];
    for source in sources {
        let (html, _, errors) = render(source, Layout::Wikidot);
        assert!(errors.is_empty(), "{source:?}: {errors:#?}");
        assert!(!html.contains("class=\"collapsible-block\""), "{html}");
    }
}

#[test]
fn adversarial_collapsible_argument_scans_are_bounded() {
    let mut complete = String::from("[[collapsible");
    for _ in 0..8_192 {
        complete.push_str(" folded=\"garbage\"");
    }
    complete.push_str(" folded=\"no\"]]B[[/collapsible]]");

    let started = Instant::now();
    let (html, text, errors) = render(&complete, Layout::Wikidot);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        html,
        collapsible_html(
            "<p>B</p>",
            true,
            "+&nbsp;show&nbsp;block",
            "–&nbsp;hide&nbsp;block",
            true,
            false,
        ),
    );
    assert_eq!(text, "B");

    let mut unclosed = String::from("[[collapsible");
    for _ in 0..8_192 {
        unclosed.push_str(" folded=\"");
    }
    unclosed.push_str("tail");
    let started = Instant::now();
    let (html, text, errors) = render(&unclosed, Layout::Wikidot);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(!errors.is_empty());
    assert!(html.starts_with("<p>[[collapsible"), "{html}");
    assert_eq!(text, unclosed);
}
