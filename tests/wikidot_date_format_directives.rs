use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::Render;
use ftml::render::html::HtmlRender;
use ftml::render::text::TextRender;
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("date-format-directives"),
        category: None,
        site: Cow::Borrowed("test"),
        title: Cow::Borrowed("Date Format Directives"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![],
        language: Cow::Borrowed("default"),
    }
}

fn render_text(source: &str, layout: Layout) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut text = source.to_owned();
    ftml::preprocess_for_layout(&mut text, settings.layout);
    let tokens = ftml::tokenize(&text);
    let (tree, errors) = ftml::parse(&tokens, &page_info, &settings).into();
    assert!(errors.is_empty(), "parse errors for {source}: {errors:?}");
    TextRender.render(&tree, &page_info, &settings)
}

fn render_html(source: &str, layout: Layout) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut text = source.to_owned();
    ftml::preprocess_for_layout(&mut text, settings.layout);
    let tokens = ftml::tokenize(&text);
    let (tree, errors) = ftml::parse(&tokens, &page_info, &settings).into();
    assert!(errors.is_empty(), "parse errors for {source}: {errors:?}");
    HtmlRender.render(&tree, &page_info, &settings).body
}

fn date_source(format: &str) -> String {
    format!("[[date 1767225600 format=\"{format}\"]]")
}

#[test]
fn wikidot_date_format_unsupported_directives_stay_literal() {
    for format in [
        "%u", "%U", "%s", "%C", "%F", "%g", "%G", "%h", "%j", "%n", "%P", "%t", "%V",
        "%w", "%W", "%x", "%X", "%Z", "%Q", "%%", "%", "%_d", "%-d", "%0d",
    ] {
        assert_eq!(
            render_text(&date_source(format), Layout::Wikidot),
            format,
            "format {format:?} must pass through literally in the Wikidot layout",
        );
    }
}

#[test]
fn wikidot_date_format_supported_directives_expand() {
    let cases = [
        ("%a", "Thu"),
        ("%A", "Thursday"),
        ("%b", "Jan"),
        ("%B", "January"),
        ("%d", "01"),
        ("%D", "01/01/26"),
        ("%e", " 1"),
        ("%H", "00"),
        ("%I", "12"),
        ("%m", "01"),
        ("%M", "00"),
        ("%p", "AM"),
        ("%r", "12:00:00 AM"),
        ("%R", "00:00"),
        ("%S", "00"),
        ("%T", "00:00:00"),
        ("%y", "26"),
        ("%Y", "2026"),
        ("%z", "GMT+00"),
        ("%d %b %Y %H:%M", "01 Jan 2026 00:00"),
        ("%Y-%m-%d", "2026-01-01"),
    ];

    for (format, expected) in cases {
        assert_eq!(
            render_text(&date_source(format), Layout::Wikidot),
            expected,
            "format {format:?} must expand in the Wikidot layout",
        );
    }
}

#[test]
fn wikidot_date_format_mixed_supported_and_unsupported() {
    assert_eq!(
        render_text(&date_source("%u %U %s %a %Y"), Layout::Wikidot),
        "%u %U %s Thu 2026",
    );
    assert_eq!(
        render_text(&date_source("[%s] %Y"), Layout::Wikidot),
        "[%s] 2026",
    );
}

#[test]
fn wikidot_date_html_keeps_default_text_and_format_class() {
    assert_eq!(
        render_html(&date_source("%u"), Layout::Wikidot),
        "<p><span class=\"odate time_1767225600 format_%25u\">01 Jan 2026 00:00</span></p>",
    );
    assert_eq!(
        render_html(&date_source("%a"), Layout::Wikidot),
        "<p><span class=\"odate time_1767225600 format_%25a\">01 Jan 2026 00:00</span></p>",
    );
}

#[test]
fn wikijump_date_format_still_expands_strftime_directives() {
    let cases = [
        ("%u", "4"),
        ("%U", "00"),
        ("%s", "1767225600"),
        ("%a", "Thu"),
        ("%P", "am"),
        ("%Z", "GMT+00"),
        ("%%", "%"),
    ];

    for (format, expected) in cases {
        assert_eq!(
            render_text(&date_source(format), Layout::Wikijump),
            expected,
            "format {format:?} must keep its native-layout expansion",
        );
    }
}
