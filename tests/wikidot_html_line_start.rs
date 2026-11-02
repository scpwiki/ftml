use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;

fn render(source: &str) -> String {
    let page_info = PageInfo {
        page: Cow::Borrowed("html-line-start"),
        category: Some(Cow::Borrowed("test")),
        site: Cow::Borrowed("coverage"),
        title: Cow::Borrowed("Html line start"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    };
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokens = ftml::tokenize(&source);
    let (tree, _errors) = ftml::parse(&tokens, &page_info, &settings).into();
    HtmlRender.render(&tree, &page_info, &settings).body
}

#[test]
fn midline_html_markers_stay_literal() {
    for (source, expected) in [
        (
            "prefix [[span]] [[html]] [[/span]] [[/html]]",
            "<p>prefix <span>[[html]]</span> [[/html]]</p>",
        ),
        (
            "prefix [[html]] [[/html]]",
            "<p>prefix [[html]] [[/html]]</p>",
        ),
        (
            "prefix [[html]]x[[/html]] tail",
            "<p>prefix [[html]]x[[/html]] tail</p>",
        ),
    ] {
        assert_eq!(render(source), expected, "{source:?}");
    }
}

#[test]
fn html_block_at_line_start_still_renders() {
    let html = render("prefix\n[[html]]\nx\n[[/html]]");
    assert!(html.contains("html-block-iframe"), "{html}");
}
