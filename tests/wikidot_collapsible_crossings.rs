use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("collapsible-crossings"),
        category: None,
        site: Cow::Borrowed("scp-wiki"),
        title: Cow::Borrowed("Collapsible Crossings"),
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

fn collapsible_blocks(html: &str) -> usize {
    html.matches("class=\"collapsible-block\"").count()
}

fn assert_clean_pair(html: &str) {
    assert_eq!(collapsible_blocks(html), 1, "html: {html}");
    assert!(!html.contains("[[collapsible"), "html: {html}");
    assert!(!html.contains("[[/collapsible]]"), "html: {html}");
    assert!(!html.contains("[[/div]]"), "html: {html}");
}

#[test]
fn closer_on_list_item_line_closes_pair() {
    let html = render(concat!(
        "[[collapsible show=\"Fiction\"]]\n",
        "intro text\n",
        "* First Book, Author One\n",
        "* Second Book, Author Two [[/collapsible]]\n",
        "after\n",
    ));
    assert_clean_pair(&html);
    assert!(html.contains("Second Book"), "html: {html}");
}

#[test]
fn closer_on_blockquote_line_closes_pair() {
    let html = render(concat!(
        "**Addendum:** [[collapsible show=\"Test Log\"]]\n",
        "> first quoted line\n",
        "> second quoted line\n",
        "> [[/collapsible]]\n",
        "after\n",
    ));
    assert_clean_pair(&html);
    assert!(html.contains("second quoted line"), "html: {html}");
}

#[test]
fn crossed_div_closers_after_content_and_raw_lines() {
    let html = render(concat!(
        "[[collapsible show=\"Addenda\"]]\n",
        "intro line\n",
        " \n",
        "[[div class=\"chat\"]]\n",
        "**A** posted:\n",
        "@@ @@\n",
        "##white|OO## reply\n",
        "[[/collapsible]]\n",
        "[[/div]]\n",
    ));
    assert_clean_pair(&html);
    assert!(html.contains("intro line"), "html: {html}");
    assert!(html.contains("class=\"chat\""), "html: {html}");
}

#[test]
fn reopened_center_with_wrapper_div() {
    let html = render(concat!(
        "[[=]]\n",
        "[[div class=\"licensebox\"]]\n",
        "[[collapsible show=\"Licensing\"]]\n",
        "cite text\n",
        "[[/=]]\n",
        "[[<]]\n",
        "middle\n",
        "[[/<]]\n",
        "[[=]]\n",
        "[[/collapsible]]\n",
        "[[/div]]\n",
        "[[/=]]",
    ));
    assert_clean_pair(&html);
    assert!(html.contains("cite text"), "html: {html}");
    assert!(html.contains("middle"), "html: {html}");
    assert!(!html.contains("[[/=]]"), "html: {html}");
}

#[test]
fn align_wrapper_closing_early_with_reopen() {
    let html = render(concat!(
        "[[>]]\n",
        "[[span class=\"blur\"]][[collapsible show=\"V\" hide=\"H\"]][[/span]]\n",
        "[[/>]]\n",
        "[[<]]\n",
        "body text\n",
        "[[/<]]\n",
        "[[>]]\n",
        "[[/collapsible]]\n",
        "[[/>]]",
    ));
    assert_clean_pair(&html);
    assert!(
        html.contains("<div style=\"text-align: right;\">"),
        "html: {html}"
    );
    assert!(html.contains("class=\"blur\""), "html: {html}");
    assert!(html.contains("body text"), "html: {html}");
}

#[test]
fn div_wrapper_closing_early_with_second_div() {
    let html = render(concat!(
        "[[div style=\"font-weight: bold;\"]]\n",
        "[[collapsible show=\"Level 5\" hide=\"Granted\"]]\n",
        "[[/div]]\n",
        "[[div style=\"font-weight: normal;\"]]\n",
        "> = **Treaty Draft**\n",
        "> body line\n",
        "[[/collapsible]]\n",
        "\n",
        "[[/div]]",
    ));
    assert_clean_pair(&html);
    assert!(html.contains("Treaty Draft"), "html: {html}");
    assert!(html.contains("font-weight: bold"), "html: {html}");
}

#[test]
fn div_collapsible_and_div_close_sharing_one_line() {
    let html = render(concat!(
        "[[div style=\"font-weight: bold;\"]] [[collapsible show=\"A\" hide=\"B\"]] [[/div]]\n",
        "[[div]]\n",
        "> quoted treaty text\n",
        "[[/collapsible]]\n",
        "[[/div]]",
    ));
    assert_clean_pair(&html);
    assert!(html.contains("quoted treaty text"), "html: {html}");
}

#[test]
fn unclosed_div_at_collapsible_end_in_center_tail() {
    let html = render(concat!(
        "[[=]]\n",
        "[[size 150%]][[collapsible show=\"Breach\" hide=\"Close\"]][[/size]]\n",
        "[[/=]]\n",
        "[[div class=\"blockquote\"]]\n",
        "body text\n",
        "[[/collapsible]]",
    ));
    assert_clean_pair(&html);
    assert!(html.contains("body text"), "html: {html}");
}

#[test]
fn nested_collapsible_stays_literal_like_wikidot() {
    let html = render(concat!(
        "[[collapsible]]\n",
        "[[collapsible]]\n",
        "nested\n",
        "[[/collapsible]]\n",
        "[[/collapsible]]\n",
    ));
    assert_eq!(collapsible_blocks(&html), 1, "html: {html}");
    assert!(html.contains("[[collapsible]]"), "html: {html}");
    assert!(html.contains("[[/collapsible]]"), "html: {html}");
}

#[test]
fn quoted_pair_still_uses_native_quote_parsing() {
    let html = render(concat!(
        "> [[collapsible show=\"poetry\"]]\n",
        "> line one\n",
        "> [[/collapsible]]\n",
    ));
    assert_clean_pair(&html);
    assert!(html.contains("line one"), "html: {html}");
}
