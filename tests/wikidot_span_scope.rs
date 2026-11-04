use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::{Element, PartialElement};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn render(source: &str) -> (String, Vec<ftml::parsing::ParseError>) {
    let page_info = PageInfo {
        page: Cow::Borrowed("span-scope"),
        category: Some(Cow::Borrowed("test")),
        site: Cow::Borrowed("coverage"),
        title: Cow::Borrowed("Span scope"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    };
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokens = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokens, &page_info, &settings).into();
    assert!(!contains_inline_control(&tree.elements));
    (HtmlRender.render(&tree, &page_info, &settings).body, errors)
}

fn contains_inline_control(elements: &[Element<'_>]) -> bool {
    elements.iter().any(|element| match element {
        Element::Partial(
            PartialElement::InlineSizeOpen(_)
            | PartialElement::InlineSizeClose(_)
            | PartialElement::InlineSpanOpen(_)
            | PartialElement::InlineSpanClose(_),
        ) => true,
        Element::Container(container) => contains_inline_control(container.elements()),
        _ => false,
    })
}

#[test]
fn span_formats_headings_inside_centered_div_like_wikidot() {
    let source = concat!(
        "[[div class=\"span-scope-outer\"]]\n[[=]]\n",
        "[[span style=\"color: rgb(191, 0, 0);\"]]\n+ SPAN_SCOPE_YEAR[[/span]]\n\n",
        "[[span style=\"color: rgb(191, 0, 0);\"]]\n++ SPAN_SCOPE_DATE\n[[/span]]\n\n",
        "[[span style=\"color: rgb(191, 0, 0); font-size: 120%;\"]] SPAN_SCOPE_TIME [[/span]]\n",
        "[[/=]]\n[[/div]]",
    );
    let (html, errors) = render(source);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        html,
        concat!(
            "<div class=\"span-scope-outer\"><div style=\"text-align: center;\">",
            "<h1 id=\"toc0\"><span style=\"color: rgb(191, 0, 0);\"><span>SPAN_SCOPE_YEAR</span></span></h1>",
            "<h2 id=\"toc1\"><span style=\"color: rgb(191, 0, 0);\"><span>SPAN_SCOPE_DATE</span></span></h2>",
            "<p><span style=\"color: rgb(191, 0, 0); font-size: 120%;\">SPAN_SCOPE_TIME</span></p>",
            "</div></div>",
        ),
    );
}

#[test]
fn span_scope_crosses_div_and_is_reopened_for_each_structural_run() {
    let (html, errors) = render(concat!(
        "[[span style=\"color: rgb(1, 2, 3);\"]]SPAN_BEFORE\n",
        "[[div class=\"span-scope-block\"]]\nSPAN_INSIDE\n[[/div]]\n",
        "SPAN_AFTER[[/span]]",
    ));
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(html.matches("color: rgb(1, 2, 3);").count(), 3, "{html}");
    assert!(html.contains("<div class=\"span-scope-block\">"), "{html}");
    assert!(
        !html.contains("<span style=\"color: rgb(1, 2, 3);\"><div"),
        "{html}"
    );
}

#[test]
fn unmatched_span_opener_stays_literal() {
    let source = "[[span style=\"color: rgb(4, 5, 6);\"]]UNMATCHED_SPAN_TEXT";
    let (html, _errors) = render(source);
    assert!(
        html.contains(
            "[[span style=&quot;color: rgb(4, 5, 6);&quot;]]UNMATCHED_SPAN_TEXT"
        ),
        "{html}"
    );
    assert!(!html.contains("style=\"color: rgb(4, 5, 6);\""), "{html}");
}

#[test]
fn repeated_span_heading_sections_stay_within_interaction_budget() {
    let section = concat!(
        "[[div class=\"one column\"]]\n[[=]]\n",
        "[[span style=\"color:#bf0000\"]]\n+ 2002[[/span]]\n\n",
        "[[span style=\"color:#bf0000\"]]\n++ 5 December\n[[/span]]\n\n",
        "[[span style=\"color:#bf0000; font-size:120%;\"]] **5:53 PM** [[/span]]\n",
        "[[/=]]\n[[/div]]\n\n",
    );
    let source = section.repeat(128);
    let started = Instant::now();
    let (html, errors) = render(&source);
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(
        html.matches("<span style=\"color:#bf0000\"><span>2002</span></span>")
            .count(),
        128,
        "{html}",
    );
    assert_eq!(
        html.matches("<span style=\"color:#bf0000; font-size:120%;\"><strong>5:53 PM</strong></span>")
            .count(),
        128,
        "{html}",
    );
}

#[test]
fn adversarial_interleaved_size_and_span_scopes_stay_within_budget() {
    let repeats = 512;
    let mut source = String::new();
    for _ in 0..repeats {
        source.push_str("[[size 100%]]");
    }
    for _ in 0..repeats {
        source.push_str("[[span class=\"x\"]]");
    }
    for _ in 0..repeats {
        source.push_str("[[/size]]");
    }
    for _ in 0..repeats {
        source.push_str("[[/span]]");
    }

    let started = Instant::now();
    let (_html, errors) = render(&source);
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed(),
    );
}
