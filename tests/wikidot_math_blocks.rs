use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::parsing::ParseError;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::Element;
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("wikidot-math-blocks"),
        category: None,
        site: Cow::Borrowed("compatibility"),
        title: Cow::Borrowed("Wikidot math blocks"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![],
        language: Cow::Borrowed("en"),
    }
}

fn render(source: &str) -> (String, Vec<ParseError>) {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;
    (html, errors)
}

#[test]
fn complete_v7_math_family_matches_live_dom_and_recovery() {
    let cases = [
        (
            "math-canonical-valid",
            "[[math]]\nv7 body\n[[/math]]",
            concat!(
                "<span class=\"equation-number\">(1)</span>\n",
                "<div class=\"math-equation\" id=\"equation-1\">",
                "\\begin{equation} v7 body \\end{equation}</div>",
            ),
        ),
        ("math-incomplete-opening", "[[math", "<p>[[math</p>"),
        (
            "math-case-variation-name",
            "[[MATH]]\nv7 body\n[[/MATH]]",
            concat!(
                "<span class=\"equation-number\">(1)</span>\n",
                "<div class=\"math-equation\" id=\"equation-1\">",
                "\\begin{equation} v7 body \\end{equation}</div>",
            ),
        ),
        (
            "math-whitespace-control",
            "[[math]]\nalpha\tbeta\u{00A0}gamma\n[[/math]]",
            concat!(
                "<span class=\"equation-number\">(1)</span>\n",
                "<div class=\"math-equation\" id=\"equation-1\">",
                "\\begin{equation} alpha beta\u{00A0}gamma \\end{equation}</div>",
            ),
        ),
        (
            "math-boundary",
            "start-[[math]]\nv7 body\n[[/math]]-middle\n\n[[math]]\nend\n[[/math]]",
            concat!(
                "<p>start-[[math]]<br>\nv7 body<br>\n[[/math]]-middle</p>",
                "<span class=\"equation-number\">(1)</span>\n",
                "<div class=\"math-equation\" id=\"equation-1\">",
                "\\begin{equation} end \\end{equation}</div>",
            ),
        ),
        (
            "math-serialization-source-preservation",
            "[[math]]\nserialized body\n[[/math]]",
            concat!(
                "<span class=\"equation-number\">(1)</span>\n",
                "<div class=\"math-equation\" id=\"equation-1\">",
                "\\begin{equation} serialized body \\end{equation}</div>",
            ),
        ),
        (
            "math-text-renderer-relevance",
            "[[math]]\nvisible text\n[[/math]]",
            concat!(
                "<span class=\"equation-number\">(1)</span>\n",
                "<div class=\"math-equation\" id=\"equation-1\">",
                "\\begin{equation} visible text \\end{equation}</div>",
            ),
        ),
        (
            "math-missing-close",
            "[[math]]\nunterminated body",
            "<p>[[math]]<br>\nunterminated body</p>",
        ),
        (
            "math-nesting-same-feature",
            "[[math]]\n[[math]]\nnested\n[[/math]]\n[[/math]]",
            concat!(
                "<span class=\"equation-number\">(1)</span>\n",
                "<div class=\"math-equation\" id=\"equation-1\">",
                "\\begin{equation} [[math]] nested \\end{equation}</div>",
                "<p>[[/math]]</p>",
            ),
        ),
        (
            "math-nesting-different-feature",
            "[[math]]\n[[bold]]nested[[/bold]]\n[[/math]]",
            concat!(
                "<span class=\"equation-number\">(1)</span>\n",
                "<div class=\"math-equation\" id=\"equation-1\">",
                "\\begin{equation} [[bold]]nested[[/bold]] \\end{equation}</div>",
            ),
        ),
        (
            "math-invalid-overlap",
            "[[math]]\nouter [[bold]]inner\n[[/math]][[/bold]]",
            "<p>[[math]]<br>\nouter [[bold]]inner<br>\n[[/math]][[/bold]]</p>",
        ),
        (
            "math-duplicate-arguments",
            "[[math v7arg=\"one\" v7arg=\"two\"]]\nv7 body\n[[/math]]",
            concat!(
                "<span class=\"equation-number\">(1)</span>\n",
                "<div class=\"math-equation\" id=\"equation-1\">",
                "\\begin{equation} v7 body \\end{equation}</div>",
            ),
        ),
        (
            "math-empty-arguments",
            "[[math v7arg=\"\"]]\nv7 body\n[[/math]]",
            concat!(
                "<span class=\"equation-number\">(1)</span>\n",
                "<div class=\"math-equation\" id=\"equation-1\">",
                "\\begin{equation} v7 body \\end{equation}</div>",
            ),
        ),
        (
            "math-unknown-argument",
            "[[math v7UnknownArgument=\"x\"]]\nv7 body\n[[/math]]",
            concat!(
                "<span class=\"equation-number\">(1)</span>\n",
                "<div class=\"math-equation\" id=\"equation-1\">",
                "\\begin{equation} v7 body \\end{equation}</div>",
            ),
        ),
        (
            "math-quote-variation",
            "[[math v7arg='single quoted' data-v7=unquoted]]\nv7 body\n[[/math]]",
            concat!(
                "<p>[[math v7arg=&#39;single quoted&#39; data-v7=unquoted]]<br>\n",
                "v7 body<br>\n[[/math]]</p>",
            ),
        ),
    ];

    for (property, source, expected) in cases {
        let (html, _) = render(source);
        assert_eq!(html, expected, "{property}");
    }
}

#[test]
fn malformed_and_bare_math_heads_remain_literal() {
    for source in [
        "[[math invalid-name]]\nbody\n[[/math]]",
        "[[math name=bare]]\nbody\n[[/math]]",
        "[[math name=bare name=\"quoted\"]]\nbody\n[[/math]]",
        "[[math name=\"unterminated]]\nbody\n[[/math]]",
        "[[math name='single quoted']]\nbody\n[[/math]]",
    ] {
        let (html, _) = render(source);
        assert!(!html.contains("math-equation"), "{source:?}: {html}");
        assert!(html.contains("[[math"), "{source:?}: {html}");
        assert!(html.contains("[[/math]]"), "{source:?}: {html}");
    }
}

#[test]
fn formula_wikitext_stays_inert_and_html_escaped() {
    let source = concat!(
        "[[math probe=\"ignored\"]]\n",
        "[[include :system:danger]] [[html]]<script>alert(1)</script>[[/html]]\n",
        "[[/math]]",
    );
    let (html, errors) = render(source);

    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(html.matches("math-equation").count(), 1, "{html}");
    assert!(html.contains("[[include :system:danger]]"), "{html}");
    assert!(
        html.contains("[[html]]&lt;script&gt;alert(1)&lt;/script&gt;[[/html]]"),
        "{html}"
    );
    assert!(!html.contains("<script>"), "{html}");
}

#[test]
fn formula_body_preserves_literal_bytes_in_the_math_element() {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let source = "[[math]]\nA  [[bold]]B[[/bold]]\tC\n[[/math]]";
    let tokenization = ftml::tokenize(source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();

    assert!(errors.is_empty(), "{errors:#?}");
    let [Element::Math {
        name,
        latex_source,
        wikidot_block_open_elements,
    }] = tree.elements.as_slice()
    else {
        panic!("expected one math element, got {:#?}", tree.elements);
    };
    assert!(wikidot_block_open_elements.is_empty());
    assert!(name.is_none());
    assert_eq!(latex_source, "A  [[bold]]B[[/bold]]\tC");
}

#[test]
fn crossed_owner_waits_for_later_standalone_closer() {
    let source = concat!(
        "[[math]]\nouter [[bold]]inner\n[[/math]][[/bold]]\n\n",
        "AFTER\n\n",
        "[[math duplicate=\"one\" duplicate=\"two\"]]\nlater\n[[/math]]",
    );
    let (html, _) = render(source);

    assert_eq!(html.matches("math-equation").count(), 1, "{html}");
    assert!(html.contains("id=\"equation-1\""), "{html}");
    assert!(!html.contains("id=\"equation-2\""), "{html}");
    assert!(
        html.contains("outer [[bold]]inner [[/math]][[/bold]]"),
        "{html}"
    );
    assert!(
        html.contains(
            "AFTER [[math duplicate=&quot;one&quot; duplicate=&quot;two&quot;]] later"
        ),
        "{html}"
    );
}

#[test]
fn dense_valid_and_crossed_math_candidates_stay_bounded() {
    const ROW_COUNT: usize = 1_024;
    let mut source = String::new();
    for _ in 0..ROW_COUNT {
        source.push_str("[[math probe=\"x\"]]\nactive\n[[/math]]\n");
        source.push_str("[[math]]\n[[bold]]crossed\n[[/math]][[/bold]]\n");
        source.push_str("[[math probe='malformed value']]\nliteral\n[[/math]]\n");
    }
    let started = Instant::now();

    let (html, _) = render(&source);

    assert!(started.elapsed() < Duration::from_secs(5));
    let expected = ROW_COUNT * 2;
    assert_eq!(html.matches("math-equation").count(), expected, "{html}");
    assert!(
        html.contains(&format!("id=\"equation-{expected}\"")),
        "{html}"
    );
    assert!(
        !html.contains(&format!("id=\"equation-{}\"", expected + 1)),
        "{html}"
    );
}
