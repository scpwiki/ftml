use ftml::data::{PageInfo, PageRef, ScoreValue};
use ftml::includes::DebugIncluder;
use ftml::layout::Layout;
use ftml::parsing::{ParseError, ParseErrorKind};
use ftml::render::Render;
use ftml::render::html::HtmlRender;
use ftml::render::text::TextRender;
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::convert::Infallible;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("theme:yossistyle"),
        category: Some(Cow::Borrowed("theme")),
        site: Cow::Borrowed("scp-wiki"),
        title: Cow::Borrowed("YOSSISTYLE CSS THEME"),
        alt_title: None,
        score: ScoreValue::Integer(145),
        tags: vec![Cow::Borrowed("joke"), Cow::Borrowed("theme")],
        language: Cow::Borrowed("en"),
    }
}

fn page_settings(layout: Layout) -> WikitextSettings {
    WikitextSettings::from_mode(WikitextMode::Page, layout)
}

fn expand_parse_render_with_layout(
    input: &str,
    layout: Layout,
) -> (String, String, Vec<ParseError>, Vec<PageRef>) {
    let settings = page_settings(layout);
    let (mut text, pages) = ftml::include(
        input,
        &settings,
        DebugIncluder::first_missing(),
        || -> Infallible {
            unreachable!("DebugIncluder returns one page for every include request")
        },
    )
    .expect("DebugIncluder cannot fail");
    ftml::preprocess_for_layout(&mut text, settings.layout);

    let tokenization = ftml::tokenize(&text);
    let page_info = page_info();
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let text = TextRender.render(&tree, &page_info, &settings);
    let html = HtmlRender.render(&tree, &page_info, &settings).body;
    (text, html, errors, pages)
}

fn expand_parse_render(input: &str) -> (String, String, Vec<ParseError>, Vec<PageRef>) {
    expand_parse_render_with_layout(input, Layout::Wikidot)
}

#[test]
fn yossistyle_targetless_include_is_literal_and_error_free() {
    let source = include_str!("fixtures/yossistyle/targetless_include.ftml");
    for layout in [Layout::Wikidot, Layout::Wikijump] {
        let (text, html, errors, pages) = expand_parse_render_with_layout(source, layout);

        assert!(errors.is_empty(), "{layout:?}: {errors:?}");
        assert!(
            pages.is_empty(),
            "{layout:?}: targetless marker must not issue a fetch: {pages:?}"
        );
        assert!(
            text.contains("amounts of [[include]]s surely"),
            "{layout:?}: {text}"
        );
        assert!(
            html.contains("amounts of [[include]]s surely"),
            "{layout:?}: {html}"
        );
    }
}

#[test]
fn only_the_exact_targetless_marker_bypasses_invalid_include() {
    for marker in ["[[include]]", "[[INCLUDE]]"] {
        let (text, html, errors, pages) = expand_parse_render(marker);
        assert!(errors.is_empty(), "{marker:?}: {errors:?}");
        assert!(pages.is_empty(), "{marker:?}: {pages:?}");
        assert_eq!(text, marker);
        assert!(html.contains(marker), "{marker:?}: {html}");
    }

    for malformed in [
        "[[include ]]",
        "[[ include]]",
        "[[include\t]]",
        "[[include | key=value]]",
        "[[include :scp-wiki:]]",
    ] {
        let (text, _html, errors, pages) = expand_parse_render(malformed);
        assert!(pages.is_empty(), "{malformed:?}: {pages:?}");
        assert!(
            text.starts_with("[[") && text.ends_with("]]"),
            "malformed include must remain visible: {malformed:?}: {text:?}",
        );
        assert!(
            errors
                .iter()
                .any(|error| error.kind() == ParseErrorKind::InvalidInclude),
            "{malformed:?}: {errors:?}",
        );
    }
}

#[test]
fn targetless_include_does_not_relax_closers_or_targeted_expansion() {
    let closer = "[[/include]]";
    let (text, _html, errors, pages) = expand_parse_render(closer);
    assert_eq!(text, closer);
    assert!(pages.is_empty());
    assert!(
        !errors.is_empty(),
        "closer must retain its fallback contract"
    );
    assert!(
        errors
            .iter()
            .all(|error| error.kind() != ParseErrorKind::InvalidInclude),
        "closer is not an include opener: {errors:?}",
    );

    let (text, _html, errors, pages) = expand_parse_render("[[include component:ok]]");
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(pages, vec![PageRef::page_only("component:ok")]);
    assert_eq!(text, "<INCLUDED-PAGE component:ok {}>");

    let malformed_executable = "[[include component:ok]]trailing";
    let (text, _html, errors, pages) = expand_parse_render(malformed_executable);
    assert!(pages.is_empty(), "non-terminal close must not be expanded");
    assert_eq!(text, malformed_executable);
    assert!(
        errors
            .iter()
            .any(|error| error.kind() == ParseErrorKind::InvalidInclude),
        "{errors:?}",
    );
}

#[test]
fn targetless_include_stays_literal_in_quoted_and_literal_regions() {
    for input in [
        "@@[[include]]@@",
        "[!--[[include]]--]visible",
        "[[code]][[include]][[/code]]",
        "> [[include]]",
        ">> [[include]]",
    ] {
        let (text, html, errors, pages) = expand_parse_render(input);
        assert!(errors.is_empty(), "{input:?}: {errors:?}");
        assert!(pages.is_empty(), "{input:?}: {pages:?}");
        if input.starts_with("[!--") {
            assert_eq!(text, "visible", "{input:?}");
            assert!(!html.contains("[[include]]"), "{input:?}: {html}");
        } else {
            assert!(text.contains("[[include]]"), "{input:?}: {text}");
            assert!(html.contains("[[include]]"), "{input:?}: {html}");
        }
    }
}

#[test]
fn repeated_targetless_includes_parse_within_a_bounded_budget() {
    const COUNT: usize = 4_096;
    let source = "[[include]]".repeat(COUNT);
    let started = Instant::now();
    let (text, _html, errors, pages) = expand_parse_render(&source);
    let elapsed = started.elapsed();

    assert!(errors.is_empty(), "{errors:?}");
    assert!(pages.is_empty());
    assert_eq!(text.matches("[[include]]").count(), COUNT);
    assert!(elapsed < Duration::from_secs(3), "parse took {elapsed:?}");
}
