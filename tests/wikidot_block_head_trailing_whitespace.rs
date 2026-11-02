use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("block-head-trailing-whitespace"),
        category: None,
        site: Cow::Borrowed("scp-wiki"),
        title: Cow::Borrowed("Block head trailing whitespace"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn render(source: &str) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, Layout::Wikidot);
    let tokenization = ftml::tokenize(&source);
    let (tree, _errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    HtmlRender.render(&tree, &page_info, &settings).body
}

fn assert_renders(source: &str, expected: &str) {
    assert_eq!(render(source), expected, "source: {source:?}");
}

#[test]
fn div_head_with_arguments_ignores_trailing_spaces_before_the_line_break() {
    let expected = "<div class=\"a\"><p>A</p></div>";
    assert_renders("[[div class=\"a\"]]\nA\n[[/div]]", expected);
    assert_renders("[[div class=\"a\"]] \nA\n[[/div]]", expected);
    assert_renders("[[div class=\"a\"]]  \nA\n[[/div]]", expected);
    assert_renders("[[div class=\"a\"]]\t\nA\n[[/div]]", expected);
}

#[test]
fn div_head_with_style_ignores_trailing_space() {
    assert_renders(
        "[[div style=\"border: solid 2px #000000; padding: 1px 15px;\"]] \nA\n[[/div]]",
        "<div style=\"border: solid 2px #000000; padding: 1px 15px;\"><p>A</p></div>",
    );
}

#[test]
fn div_head_trailing_space_before_a_blank_line_opens_the_block() {
    assert_renders(
        "[[div class=\"a\"]] \n\nA\n\n[[/div]]",
        "<div class=\"a\"><p>A</p></div>",
    );
}

#[test]
fn consecutive_divs_with_trailing_head_space_both_open() {
    let html = render(
        "[[div class=\"a\"]] \n**A**\n[[/div]]\n\n[[div class=\"b\"]] \n**B**\n[[/div]]",
    );
    assert!(!html.contains("[["), "stray markup in {html:?}");
    assert_eq!(html.matches("<div").count(), 2, "{html:?}");
}

#[test]
fn div_head_trailing_space_before_inline_text_is_not_a_line_break() {
    let html = render("[[div class=\"a\"]] A[[/div]]");
    assert!(html.contains("A"), "{html:?}");
}

#[test]
fn controls_without_arguments_and_other_blocks_are_unchanged() {
    assert_renders("[[div]] \nA\n[[/div]]", "<div><p>A</p></div>");
    assert_renders("[[div]]\t\nA\n[[/div]]", "<div><p>A</p></div>");
    assert_renders("[[div]]\nA\n[[/div]] ", "<div><p>A</p></div>\n");
    let html = render("[[collapsible show=\"a\" hide=\"b\"]] \ninner\n[[/collapsible]]");
    assert!(html.contains("collapsible-block-content"), "{html:?}");
    assert!(!html.contains("[["), "{html:?}");
    assert_renders(
        "x [[span class=\"q\"]] \nin span\n[[/span]]",
        "<p>x <span class=\"q\"><br>\nin span<br>\n</span></p>",
    );
}
