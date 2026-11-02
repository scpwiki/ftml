use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::parsing::{ParseError, ParseErrorKind};
use ftml::render::Render;
use ftml::render::html::HtmlRender;
use ftml::render::text::TextRender;
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

struct Case {
    name: &'static str,
    source: &'static str,
    html: &'static str,
    text: &'static str,
    expect_errors: bool,
}

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("explicit-list-grammar"),
        category: None,
        site: Cow::Borrowed("scp-wiki"),
        title: Cow::Borrowed("Explicit list grammar"),
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

fn assert_case(case: &Case, source: &str, html: &str, text: &str) {
    let (actual_html, actual_text, errors) = render(source);
    assert_eq!(actual_html, html, "{} HTML", case.name);
    assert_eq!(actual_text, text, "{} text", case.name);
    assert_eq!(
        !errors.is_empty(),
        case.expect_errors,
        "{} errors: {errors:#?}",
        case.name,
    );
}

const ORPHAN_LIST_ITEM_CASES: &[Case] = &[
    Case {
        name: "li-canonical-valid",
        source: "[[li]]\nv7 body\n[[/li]]",
        html: "<p>[[li]]<br>\nv7 body<br>\n[[/li]]</p>",
        text: "[[li]]\nv7 body\n[[/li]]",
        expect_errors: true,
    },
    Case {
        name: "li-incomplete-opening",
        source: "[[li",
        html: "<p>[[li</p>",
        text: "[[li",
        expect_errors: true,
    },
    Case {
        name: "li-case-variation-name",
        source: "[[LI]]\nv7 body\n[[/LI]]",
        html: "<p>[[LI]]<br>\nv7 body<br>\n[[/LI]]</p>",
        text: "[[LI]]\nv7 body\n[[/LI]]",
        expect_errors: true,
    },
    Case {
        name: "li-whitespace-control",
        source: "[[li]]\nalpha\tbeta gamma\n[[/li]]",
        html: "<p>[[li]]<br>\nalpha beta gamma<br>\n[[/li]]</p>",
        text: "[[li]]\nalpha beta gamma\n[[/li]]",
        expect_errors: true,
    },
    Case {
        name: "li-boundary",
        source: "start-[[li]]\nv7 body\n[[/li]]-middle\n\n[[li]]\nend\n[[/li]]",
        html: concat!(
            "<p>start-[[li]]<br>\nv7 body<br>\n[[/li]]-middle</p>",
            "<p>[[li]]<br>\nend<br>\n[[/li]]</p>",
        ),
        text: "start-[[li]]\nv7 body\n[[/li]]-middle\n\n[[li]]\nend\n[[/li]]",
        expect_errors: true,
    },
    Case {
        name: "li-serialization-source-preservation",
        source: "[[li]]\nserialized body\n[[/li]]",
        html: "<p>[[li]]<br>\nserialized body<br>\n[[/li]]</p>",
        text: "[[li]]\nserialized body\n[[/li]]",
        expect_errors: true,
    },
    Case {
        name: "li-text-renderer-relevance",
        source: "[[li]]\nvisible text\n[[/li]]",
        html: "<p>[[li]]<br>\nvisible text<br>\n[[/li]]</p>",
        text: "[[li]]\nvisible text\n[[/li]]",
        expect_errors: true,
    },
    Case {
        name: "li-missing-close",
        source: "[[li]]\nunterminated body",
        html: "<p>[[li]]<br>\nunterminated body</p>",
        text: "[[li]]\nunterminated body",
        expect_errors: true,
    },
    Case {
        name: "li-nesting-same-feature",
        source: "[[li]]\n[[li]]\nnested\n[[/li]]\n[[/li]]",
        html: "<p>[[li]]<br>\n[[li]]<br>\nnested<br>\n[[/li]]<br>\n[[/li]]</p>",
        text: "[[li]]\n[[li]]\nnested\n[[/li]]\n[[/li]]",
        expect_errors: true,
    },
    Case {
        name: "li-nesting-different-feature",
        source: "[[li]]\n[[bold]]nested[[/bold]]\n[[/li]]",
        html: "<p>[[li]]<br>\n[[bold]]nested[[/bold]]<br>\n[[/li]]</p>",
        text: "[[li]]\n[[bold]]nested[[/bold]]\n[[/li]]",
        expect_errors: true,
    },
    Case {
        name: "li-invalid-overlap",
        source: "[[li]]\nouter [[bold]]inner\n[[/li]][[/bold]]",
        html: "<p>[[li]]<br>\nouter [[bold]]inner<br>\n[[/li]][[/bold]]</p>",
        text: "[[li]]\nouter [[bold]]inner\n[[/li]][[/bold]]",
        expect_errors: true,
    },
    Case {
        name: "li-duplicate-arguments",
        source: r#"[[li class="one" class="two"]]
v7 body
[[/li]]"#,
        html: "<p>[[li class=&quot;one&quot; class=&quot;two&quot;]]<br>\nv7 body<br>\n[[/li]]</p>",
        text: "[[li class=\"one\" class=\"two\"]]\nv7 body\n[[/li]]",
        expect_errors: true,
    },
    Case {
        name: "li-empty-arguments",
        source: r#"[[li class=""]]
v7 body
[[/li]]"#,
        html: "<p>[[li class=&quot;&quot;]]<br>\nv7 body<br>\n[[/li]]</p>",
        text: "[[li class=\"\"]]\nv7 body\n[[/li]]",
        expect_errors: true,
    },
    Case {
        name: "li-unknown-argument",
        source: r#"[[li v7UnknownArgument="x"]]
v7 body
[[/li]]"#,
        html: "<p>[[li v7UnknownArgument=&quot;x&quot;]]<br>\nv7 body<br>\n[[/li]]</p>",
        text: "[[li v7UnknownArgument=\"x\"]]\nv7 body\n[[/li]]",
        expect_errors: true,
    },
    Case {
        name: "li-quote-variation",
        source: "[[li class='single quoted' data-v7=unquoted]]\nv7 body\n[[/li]]",
        html: "<p>[[li class=&#39;single quoted&#39; data-v7=unquoted]]<br>\nv7 body<br>\n[[/li]]</p>",
        text: "[[li class='single quoted' data-v7=unquoted]]\nv7 body\n[[/li]]",
        expect_errors: true,
    },
    Case {
        name: "li-unsafe-url",
        source: r#"[[li style="background:url(javascript:alert(1))"]]
v7 body
[[/li]]"#,
        html: "<p>[[li style=&quot;background:url(javascript:alert(1))&quot;]]<br>\nv7 body<br>\n[[/li]]</p>",
        text: "[[li style=\"background:url(javascript:alert(1))\"]]\nv7 body\n[[/li]]",
        expect_errors: true,
    },
    Case {
        name: "li-percent-encoded",
        source: r#"[[li style="https://example.test/%6a%61vascript%3aalert(1)"]]
v7 body
[[/li]]"#,
        html: concat!(
            "<p>[[li style=&quot;<a href=\"https://example.test/%6a%61vascript%3aalert(1)\">",
            "https://example.test/%6a%61vascript%3aalert(1)</a>&quot;]]<br>\n",
            "v7 body<br>\n[[/li]]</p>",
        ),
        text: "[[li style=\"https://example.test/%6a%61vascript%3aalert(1)\"]]\nv7 body\n[[/li]]",
        expect_errors: true,
    },
    Case {
        name: "li-backslash",
        source: r#"[[li style="https:\\example.test\path"]]
v7 body
[[/li]]"#,
        html: r#"<p>[[li style=&quot;https:\\example.test\path&quot;]]<br>
v7 body<br>
[[/li]]</p>"#,
        text: r#"[[li style="https:\\example.test\path"]]
v7 body
[[/li]]"#,
        expect_errors: true,
    },
];

const LIST_CASES: &[Case] = &[
    Case {
        name: "canonical-valid",
        source: "[[%t%]]\nv7 body\n[[/%t%]]",
        html: "<%t%>\n<li style=\"list-style: none\">v7 body</li>\n</%t%><br>\n",
        text: "v7 body",
        expect_errors: false,
    },
    Case {
        name: "incomplete-opening",
        source: "[[%t%",
        html: "<p>[[%t%</p>",
        text: "[[%t%",
        expect_errors: true,
    },
    Case {
        name: "case-variation-name",
        source: "[[%T%]]\nv7 body\n[[/%T%]]",
        html: "v7 body<br>\n",
        text: "v7 body",
        expect_errors: false,
    },
    Case {
        name: "whitespace-control",
        source: "[[%t%]]\nalpha\tbeta gamma\n[[/%t%]]",
        html: "<%t%>\n<li style=\"list-style: none\">alpha beta gamma</li>\n</%t%><br>\n",
        text: "alpha beta gamma",
        expect_errors: false,
    },
    Case {
        name: "boundary",
        source: "start-[[%t%]]\nv7 body\n[[/%t%]]-middle\n\n[[%t%]]\nend\n[[/%t%]]",
        html: concat!(
            "<p>start-</p><%t%>\n<li style=\"list-style: none\">v7 body</li>\n</%t%>",
            "-middle\n<%t%>\n<li style=\"list-style: none\">end</li>\n</%t%><br>\n",
        ),
        text: "start-\nv7 body\n-middle\nend",
        expect_errors: false,
    },
    Case {
        name: "serialization-source-preservation",
        source: "[[%t%]]\nserialized body\n[[/%t%]]",
        html: "<%t%>\n<li style=\"list-style: none\">serialized body</li>\n</%t%><br>\n",
        text: "serialized body",
        expect_errors: false,
    },
    Case {
        name: "text-renderer-relevance",
        source: "[[%t%]]\nvisible text\n[[/%t%]]",
        html: "<%t%>\n<li style=\"list-style: none\">visible text</li>\n</%t%><br>\n",
        text: "visible text",
        expect_errors: false,
    },
    Case {
        name: "missing-close",
        source: "[[%t%]]\nunterminated body",
        html: "<p>[[%t%]]<br>\nunterminated body</p>",
        text: "[[%t%]]\nunterminated body",
        expect_errors: true,
    },
    Case {
        name: "nesting-same-feature",
        source: "[[%t%]]\n[[%t%]]\nnested\n[[/%t%]]\n[[/%t%]]",
        html: concat!(
            "<%t%>\n<li style=\"list-style: none; display: inline\"><%t%>\n",
            "<li style=\"list-style: none\">nested</li>\n</%t%></li></%t%><br>\n",
        ),
        text: "nested",
        expect_errors: false,
    },
    Case {
        name: "nesting-different-feature",
        source: "[[%t%]]\n[[bold]]nested[[/bold]]\n[[/%t%]]",
        html: "<%t%>\n<li style=\"list-style: none\">[[bold]]nested[[/bold]]</li>\n</%t%><br>\n",
        text: "[[bold]]nested[[/bold]]",
        expect_errors: true,
    },
    Case {
        name: "invalid-overlap",
        source: "[[%t%]]\nouter [[bold]]inner\n[[/%t%]][[/bold]]",
        html: "<%t%>\n<li style=\"list-style: none\">outer [[bold]]inner</li>\n</%t%>[[/bold]]",
        text: "outer [[bold]]inner\n[[/bold]]",
        expect_errors: true,
    },
    Case {
        name: "duplicate-arguments",
        source: "[[%t% class=\"one\" class=\"two\"]]\nv7 body\n[[/%t%]]",
        html: "<%t% class=\"two\">\n<li style=\"list-style: none\">v7 body</li>\n</%t%><br>\n",
        text: "v7 body",
        expect_errors: false,
    },
    Case {
        name: "empty-arguments",
        source: "[[%t% class=\"\"]]\nv7 body\n[[/%t%]]",
        html: "<%t%>\n<li style=\"list-style: none\">v7 body</li>\n</%t%><br>\n",
        text: "v7 body",
        expect_errors: false,
    },
    Case {
        name: "unknown-argument",
        source: "[[%t% v7UnknownArgument=\"x\"]]\nv7 body\n[[/%t%]]",
        html: "<%t%>\n<li style=\"list-style: none\">v7 body</li>\n</%t%><br>\n",
        text: "v7 body",
        expect_errors: false,
    },
    Case {
        name: "quote-variation",
        source: "[[%t% class='single quoted' data-v7=unquoted]]\nv7 body\n[[/%t%]]",
        html: "<%t%>\n<li style=\"list-style: none\">v7 body</li>\n</%t%><br>\n",
        text: "v7 body",
        expect_errors: false,
    },
    Case {
        name: "unsafe-url",
        source: "[[%t% style=\"background:url(javascript:alert(1))\"]]\nv7 body\n[[/%t%]]",
        html: "<%t% style=\"background:url(javascript:alert(1))\">\n<li style=\"list-style: none\">v7 body</li>\n</%t%><br>\n",
        text: "v7 body",
        expect_errors: false,
    },
    Case {
        name: "percent-encoded",
        source: "[[%t% style=\"https://example.test/%6a%61vascript%3aalert(1)\"]]\nv7 body\n[[/%t%]]",
        html: "<%t% style=\"https://example.test/%6a%61vascript%3aalert(1)\">\n<li style=\"list-style: none\">v7 body</li>\n</%t%><br>\n",
        text: "v7 body",
        expect_errors: false,
    },
    Case {
        name: "backslash",
        source: r#"[[%t% style="https:\\example.test\path"]]
v7 body
[[/%t%]]"#,
        html: r#"<%t% style="https:\example.testpath">
<li style="list-style: none">v7 body</li>
</%t%><br>
"#,
        text: "v7 body",
        expect_errors: false,
    },
];

#[test]
fn complete_v7_explicit_list_matrix_matches_live_html_and_text() {
    assert_eq!(ORPHAN_LIST_ITEM_CASES.len() + LIST_CASES.len() * 2, 54);

    for case in ORPHAN_LIST_ITEM_CASES {
        assert_case(case, case.source, case.html, case.text);
    }
    for tag in ["ol", "ul"] {
        let upper = tag.to_ascii_uppercase();
        for case in LIST_CASES {
            let expand = |value: &str| value.replace("%t%", tag).replace("%T%", &upper);
            assert_case(
                case,
                &expand(case.source),
                &expand(case.html),
                &expand(case.text),
            );
        }
    }
}

#[test]
fn explicit_list_items_keep_last_attributes_and_empty_values_are_omitted() {
    let (html, text, errors) = render(concat!(
        "[[ul class=\"first\" class=\"second\"]]\n",
        "[[li class=\"first\" class=\"second\"]]Alpha[[/li]]\n",
        "[[li class=\"\"]]Beta[[/li]]\n",
        "[[/ul]]",
    ));

    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(text, "Alpha\nBeta");
    assert_eq!(
        html,
        concat!(
            "<ul class=\"second\">\n",
            "<li class=\"second\">Alpha</li>\n",
            "<li>Beta</li>\n",
            "</ul><br>\n",
        ),
    );
}

#[test]
fn explicit_list_attributes_still_use_the_safe_html_attribute_boundary() {
    let (html, _, errors) = render(concat!(
        "[[ul onclick=\"alert(1)\" data-safe=\"kept\"]]\n",
        "[[li onmouseover=\"alert(2)\"]]Body[[/li]]\n",
        "[[/ul]]",
    ));

    assert!(errors.is_empty(), "{errors:#?}");
    assert!(html.contains("data-safe=\"kept\""), "{html}");
    assert!(!html.contains("onclick"), "{html}");
    assert!(!html.contains("onmouseover"), "{html}");
}

#[test]
fn native_and_explicit_lists_remain_independent() {
    let (html, text, errors) = render("* Native\n\n[[ul]]\nExplicit\n[[/ul]]");

    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(text, "Native\nExplicit");
    assert_eq!(html.matches("<ul>").count(), 2, "{html}");
    assert!(html.contains("<li>Native</li>"), "{html}");
    assert!(
        html.contains("<li style=\"list-style: none\">Explicit</li>"),
        "{html}",
    );

    let (native_residual, _, errors) = render("--\n* item\n\n--");
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        native_residual,
        "<p>—</p><ul>\n<li>item</li>\n</ul><p>—</p>",
    );
}

#[test]
fn repeated_malformed_explicit_list_recovery_stays_bounded() {
    let source = (0..512)
        .map(|index| format!("[[ul]]\nrow-{index} [[bold]]inner\n[[/ul]][[/bold]]\n"))
        .collect::<String>();
    let started = Instant::now();
    let (html, text, errors) = render(&source);

    assert_eq!(html.matches("<ul>").count(), 512);
    assert_eq!(html.matches("[[/bold]]").count(), 512);
    assert!(text.contains("row-511"));
    assert!(!errors.is_empty());
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn issue_484_empty_lists_keep_breaks_and_orphan_recovery_unwrapped() {
    let source = include_str!("../test/list/block-fail/input.ftml");
    let (html, text, errors) = render(source);

    assert_eq!(
        html,
        concat!(
            "<br>\n<br>\n<br>\n<br>\n<br>\n<br>\n",
            "<ul>\n<li style=\"list-style: none\">Foo</li>\n</ul><br>\n",
            "<ol>\n<li style=\"list-style: none\">Bar</li>\n</ol><br>\n",
            "[[li]]<br>\nBaz<br>\n[[/li]]",
        ),
    );
    assert_eq!(text, "Foo\n\nBar\n\n[[li]]\nBaz\n[[/li]]");
    assert_eq!(
        errors
            .iter()
            .filter(|error| error.kind() == ParseErrorKind::ListItemOutsideList)
            .count(),
        1,
    );
}

#[test]
fn issue_484_isolated_orphan_list_item_stays_in_a_paragraph() {
    let (html, text, errors) = render("[[li]]\nBaz\n[[/li]]");

    assert_eq!(html, "<p>[[li]]<br>\nBaz<br>\n[[/li]]</p>");
    assert_eq!(text, "[[li]]\nBaz\n[[/li]]");
    assert!(!errors.is_empty(), "orphan li must remain diagnosed");
}

#[test]
fn issue_485_scored_list_recovery_keeps_literal_markers_in_one_item() {
    let source = include_str!("../test/list/block-score/input.ftml");
    let (html, text, errors) = render(source);

    assert_eq!(
        html,
        concat!(
            "<ul>\n",
            "<li style=\"list-style: none\">[[li_]]<br>\n",
            "Alpha<br>\n[[/li]]<br>\n",
            "[[li_]]<br>\nBeta<br>\n[[/li]]</li>\n",
            "</ul>",
            "<ol>\n",
            "<li style=\"list-style: none\">[[li_]]<br>\n",
            "One<br>\n[[/li]]<br>\n",
            "[[li_]]<br>\nTwo<br>\n[[/li]]</li>\n",
            "</ol>",
        ),
    );
    assert_eq!(
        text,
        "[[li_]]\nAlpha\n[[/li]]\n[[li_]]\nBeta\n[[/li]]\n[[li_]]\nOne\n[[/li]]\n[[li_]]\nTwo\n[[/li]]",
    );
    assert!(
        !errors.is_empty(),
        "malformed scored li must remain diagnosed"
    );
}

#[test]
fn issue_803_stray_li_closer_in_scored_recovery_stays_literal() {
    let (html, text, errors) = render(concat!(
        "[[ul]]\n\n",
        "[[li_]]\nALPHA\n[[/li]]\n\n",
        "[[li_]]\n\nBETA\n\n[[/li]]\n",
        "[[li]]GAMMA[[/li]]\n\n",
        "[[li_]] DELTA [[/li]]\n\n",
        "[[/ul]]",
    ));

    assert_eq!(
        html,
        concat!(
            "<ul>\n",
            "<li style=\"list-style: none\">[[li_]]<br>\n",
            "ALPHA<br>\n[[/li]]\n",
            "<p>[[li_]]</p>\n\n<p>BETA</p>\n[[/li]]</li>\n",
            "<li>GAMMA[[li_]] DELTA [[/li]]</li>\n",
            "</ul><br>\n",
        ),
    );
    assert_eq!(
        text,
        "[[li_]]\nALPHA\n[[/li]]\n[[li_]]\n\nBETA\n[[/li]]\nGAMMA[[li_]] DELTA [[/li]]",
    );
    assert!(
        !errors.is_empty(),
        "malformed scored li must remain diagnosed"
    );
}

#[test]
fn issue_803_wdpr_block_fail_input_matches_live_structure() {
    let (html, text, errors) = render(concat!(
        "[[ul]]\n[[/ul]]\n\n",
        "[[ol]]\n[[/ol]]\n\n",
        "[[ul]]\n[[li]]\n[[/li]]\n[[/ul]]\n\n",
        "[[ul]] [[/ul]]\n\n",
        "[[ol]] [[/ol]]\n\n",
        "[[ul]] [[li]] [[/li]] [[/ul]]\n\n",
        "[[ul]]\nFoo\n[[/ul]]\n\n",
        "[[ol]]\nBar\n[[/ol]]\n\n",
        "[[li]]\nBaz\n[[/li]]\n\n",
        "[[ul]]\n\n",
        "[[li_]]\nALPHA\n[[/li]]\n\n",
        "[[li_]]\n\nBETA\n\n[[/li]]\n",
        "[[li]]GAMMA[[/li]]\n\n",
        "[[li_]] DELTA [[/li]]\n\n",
        "[[/ul]]",
    ));

    assert_eq!(
        html,
        concat!(
            "<br>\n<br>\n<br>\n<br>\n<br>\n<br>\n",
            "<ul>\n<li style=\"list-style: none\">Foo</li>\n</ul><br>\n",
            "<ol>\n<li style=\"list-style: none\">Bar</li>\n</ol><br>\n",
            "[[li]]<br>\nBaz<br>\n[[/li]] <ul>\n",
            "<li style=\"list-style: none\">[[li_]]<br>\n",
            "ALPHA<br>\n[[/li]]\n",
            "<p>[[li_]]</p>\n\n<p>BETA</p>\n[[/li]]</li>\n",
            "<li>GAMMA[[li_]] DELTA [[/li]]</li>\n",
            "</ul><br>\n",
        ),
    );
    assert_eq!(
        text,
        concat!(
            "Foo\n\nBar\n\n[[li]]\nBaz\n[[/li]] \n",
            "[[li_]]\nALPHA\n[[/li]]\n[[li_]]\n\nBETA\n[[/li]]\n",
            "GAMMA[[li_]] DELTA [[/li]]",
        ),
    );
    assert!(
        !errors.is_empty(),
        "malformed list shapes must remain diagnosed"
    );
}
