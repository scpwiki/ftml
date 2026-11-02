use ftml::data::{PageInfo, ScoreValue, UserInfo};
use ftml::layout::Layout;
use ftml::parsing::ParseError;
use ftml::render::html::HtmlRender;
use ftml::render::{Render, UserInfoResolver};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("lane-2-block-argument-grammar"),
        category: None,
        site: Cow::Borrowed("sandbox-for-codex"),
        title: Cow::Borrowed("Lane 2 block argument grammar"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn render(source: &str, layout: Layout) -> (String, Vec<ParseError>) {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;
    (html, errors)
}

fn render_wikidot(source: &str) -> String {
    let (html, errors) = render(source, Layout::Wikidot);
    assert!(errors.is_empty(), "{source}: {errors:#?}");
    html
}

#[test]
fn wikidot_dates_use_the_wikidot_default_format() {
    assert_eq!(
        render_wikidot("[[date 0]]"),
        r#"<p><span class="odate time_0">01 Jan 1970 00:00</span></p>"#,
    );
}

#[test]
fn wikidot_dates_apply_the_default_format_to_nonzero_timestamps() {
    assert_eq!(
        render_wikidot("[[date 1234567890]]"),
        r#"<p><span class="odate time_1234567890">13 Feb 2009 23:31</span></p>"#,
    );
}

#[test]
fn wikidot_date_format_changes_the_class_but_not_server_visible_text() {
    let source = r#"[[date 0 format="%Y"]]"#;
    assert_eq!(
        render_wikidot(source),
        r#"<p><span class="odate time_0 format_%25Y">01 Jan 1970 00:00</span></p>"#,
    );

    let (wikijump, errors) = render(source, Layout::Wikijump);
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(
        wikijump.ends_with(">1970</span></p>"),
        "Wikijump must retain explicit formatting: {wikijump}",
    );
}

struct MissingUser;

impl UserInfoResolver for MissingUser {
    fn user_info(&self, _name: &str) -> Option<UserInfo<'static>> {
        None
    }
}

fn render_wikidot_with_missing_user(source: &str) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();

    assert!(errors.is_empty(), "{errors:#?}");
    HtmlRender
        .render_with_user_info(&tree, &page_info, &settings, &MissingUser)
        .body
}

#[test]
fn wikidot_missing_user_suffix_remains_inside_the_error_span() {
    assert_eq!(
        render_wikidot_with_missing_user("[[user missing]]"),
        concat!(
            r#"<p><span class="error-inline"><em>missing</em>"#,
            " does not match any existing user name</span></p>",
        ),
    );

    assert_eq!(
        render_wikidot_with_missing_user("[[user v7ws=\"alpha\tbeta\u{00a0}gamma\"]]",),
        concat!(
            r#"<p><span class="error-inline"><em>v7ws=&quot;alpha beta"#,
            "\u{00a0}",
            r#"gamma&quot;</em>"#,
            " does not match any existing user name</span></p>",
        ),
    );
}

#[test]
fn wikidot_anonymous_user_is_literal() {
    assert_eq!(
        render_wikidot_with_missing_user("[[user anonymous]] [[USER Anonymous]]"),
        "<p>Anonymous Anonymous</p>",
    );
}

#[test]
fn wikidot_malformed_attribute_fragments_keep_blocks_and_resynchronize() {
    assert_eq!(
        render_wikidot("[[span bogus=bare class=\"middle\" id=\"last\"]]BODY[[/span]]",),
        "<p><span id=\"u-last\">BODY</span></p>",
    );

    let collapsible = render_wikidot(
        "[[collapsible bogus=bare show=\"MIDDLE\" hide=\"LAST\"]]BODY[[/collapsible]]",
    );
    assert!(
        collapsible.contains("+&nbsp;show&nbsp;block"),
        "{collapsible}",
    );
    assert!(collapsible.contains(">LAST</a>"), "{collapsible}");
    assert!(!collapsible.contains("MIDDLE"), "{collapsible}");

    assert_eq!(
        render_wikidot(
            "[[image https://example.com/omega.png bogus=bare alt=\"MIDDLE\" width=\"41px\"]]",
        ),
        concat!(
            "<img src=\"https://example.com/omega.png\" width=\"41px\" ",
            "class=\"image\" alt=\"omega.png\">",
        ),
    );

    assert_eq!(
        render_wikidot(
            "[[iframe https://example.com bogus=bare width=\"41\" height=\"42\"]]",
        ),
        concat!(
            "<p><iframe src=\"https://example.com\" align frameborder ",
            "height=\"42\" scrolling width class style></iframe></p>",
        ),
    );
}

#[test]
fn wikidot_attribute_names_are_case_sensitive() {
    assert_eq!(
        render_wikidot(
            "[[div CLASS=\"upper\" ID=\"upper-id\" data-probe=\"lower\"]]\nBODY\n[[/div]]",
        ),
        "<div data-probe=\"lower\"><p>BODY</p></div>",
    );

    let collapsible = render_wikidot(
        "[[collapsible Show=\"UPPER\" hide=\"lower\"]]BODY[[/collapsible]]",
    );
    assert!(
        collapsible.contains("+&nbsp;show&nbsp;block"),
        "{collapsible}",
    );
    assert!(collapsible.contains(">lower</a>"), "{collapsible}");
    assert!(!collapsible.contains("UPPER"), "{collapsible}");
}

#[test]
fn wikidot_equals_spacing_and_multiline_values_follow_getattrs() {
    assert_eq!(
        render_wikidot("[[div class =\"before\" id=\"kept\"]]\nBODY\n[[/div]]",),
        "<div class=\"before\" id=\"u-kept\"><p>BODY</p></div>",
    );
    assert_eq!(
        render_wikidot("[[div class= \"ignored\" id=\"also-absorbed\"]]\nBODY\n[[/div]]",),
        "<div><p>BODY</p></div>",
    );

    let collapsible = render_wikidot(
        "[[collapsible show=\"OPEN\nSECOND\" hide=\"CLOSE\"]]\nBODY\n[[/collapsible]]",
    );
    assert!(collapsible.contains(">OPEN SECOND</a>"), "{collapsible}");
    assert!(collapsible.contains(">CLOSE</a>"), "{collapsible}");

    let collapsible = render_wikidot(
        "[[collapsible show=\"OPEN\tSECOND\" hide=\"CLOSE\tSECOND\"]]\nBODY\n[[/collapsible]]",
    );
    assert!(
        collapsible.contains(">OPEN&nbsp;&nbsp;&nbsp;&nbsp;SECOND</a>"),
        "{collapsible}",
    );
    assert!(
        collapsible.contains(">CLOSE&nbsp;&nbsp;&nbsp;&nbsp;SECOND</a>"),
        "{collapsible}",
    );
}

#[test]
fn wikidot_attribute_values_apply_php_stripslashes_semantics() {
    assert_eq!(
        render_wikidot(
            "[[div data-probe=\"a\\qb\" data-backslash=\"a\\\\b\" \
             data-zero=\"a\\0b\" data-unicode=\"雪ω🙂\"]]\nBODY\n[[/div]]",
        ),
        concat!(
            "<div data-backslash=\"a\\b\" data-probe=\"aqb\" ",
            "data-unicode=\"雪ω🙂\" data-zero=\"ab\"><p>BODY</p></div>",
        ),
    );
}

#[test]
fn wikidot_attribute_values_normalize_ascii_whitespace_and_controls() {
    assert_eq!(
        render_wikidot(
            "[[div data-probe=\"  alpha\t beta\n gamma  \" \
             data-controls=\"a\u{000B}\u{000C}b\"]]\nBODY\n[[/div]]",
        ),
        concat!(
            "<div data-controls=\"ab\" data-probe=\"alpha beta gamma\">",
            "<p>BODY</p></div>",
        ),
    );
}

#[test]
fn wikijump_layout_keeps_its_strict_case_insensitive_argument_grammar() {
    let (html, errors) = render(
        "[[div CLASS= \"kept\" data-probe=\"a\\qb\"]]\nBODY\n[[/div]]",
        Layout::Wikijump,
    );
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        html,
        "<div class=\"kept\" data-probe=\"a\\qb\"><p>BODY</p></div>",
    );
}
