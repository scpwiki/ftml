use ftml::data::{PageInfo, PageRef, ScoreValue};
use ftml::delayed::{
    DelayedInput, GeneratedInput, GeneratedKind, GeneratedValue, InputSegment,
    SlotBindings, SlotId, TextOrigin, parse_delayed_list,
};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender, text::TextRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::{ContentSegment, Element, SyntaxTree};
use std::borrow::Cow;
use std::time::{Duration, Instant};

const SEPARATOR_HTML: &str =
    r#"<div class="content-separator" style="display: none:"></div>"#;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("issue-334"),
        category: None,
        site: Cow::Borrowed("syntax-differential"),
        title: Cow::Borrowed("Content separators"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn parse(source: &str, layout: Layout) -> SyntaxTree<'static> {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let tokenization = ftml::tokenize(source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    assert!(errors.is_empty(), "{source:?}: {errors:#?}");
    tree.to_owned()
}

fn render_with_layout(source: &str, layout: Layout) -> (String, String) {
    let tree = parse(source, layout);
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    (
        HtmlRender.render(&tree, &page_info, &settings).body,
        TextRender.render(&tree, &page_info, &settings),
    )
}

fn render(source: &str) -> (String, String) {
    render_with_layout(source, Layout::Wikidot)
}

fn visible_lines(text: &str) -> String {
    text.lines()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_delayed(source: &str, segments: Vec<InputSegment>) -> String {
    let input = DelayedInput::new(source, segments).expect("valid segmented input");
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
    let delayed = parse_delayed_list(&input, &page_info, &settings)
        .expect("content-section delayed parse");
    delayed
        .bind(&SlotBindings::empty())
        .expect("empty delayed bindings")
        .render_html(&page_info, &settings)
        .body()
        .to_owned()
}

#[test]
fn padded_authored_separator_remains_text_in_wikidot() {
    assert_eq!(render(" \t====\t ").0, "<p>====</p>");
}

#[test]
fn live_backed_twenty_seven_case_matrix_matches_the_ftml_seam() {
    let cases = [
        (
            "alignment",
            "[[=]]\n====\n[[/=]]",
            format!(r#"<div style="text-align: center;">{SEPARATOR_HTML}</div>"#),
            "",
        ),
        (
            "between",
            "A\n====\nB",
            format!("<p>A</p>{SEPARATOR_HTML}<p>B</p>"),
            "A\nB",
        ),
        (
            "blank",
            "A\n\n====\n\nB",
            format!("<p>A</p>{SEPARATOR_HTML}<p>B</p>"),
            "A\nB",
        ),
        (
            "code",
            "[[code]]\n====\n[[/code]]",
            "<div class=\"code\"><pre><code>====</code></pre></div>".to_owned(),
            "====",
        ),
        (
            "div",
            "[[div]]\nA\n====\nB\n[[/div]]",
            format!("<div><p>A</p>{SEPARATOR_HTML}<p>B</p></div>"),
            "A\nB",
        ),
        (
            "empty-sections",
            "====\n====",
            format!("{SEPARATOR_HTML}{SEPARATOR_HTML}"),
            "",
        ),
        ("escaped", "\\====", "<p>\\====</p>".to_owned(), "\\===="),
        (
            "heading",
            "+ ====",
            "<h1 id=\"toc0\"><span>====</span></h1>".to_owned(),
            "====",
        ),
        ("inline", "A====B", "<p>A====B</p>".to_owned(), "A====B"),
        ("leading-space", " ====", "<p>====</p>".to_owned(), "===="),
        (
            "list",
            "* ====",
            "<ul>\n<li>====</li>\n</ul>".to_owned(),
            "====",
        ),
        (
            "module",
            "[[module ListPages]]\nA\n====\nB\n[[/module]]",
            format!("<p>A</p>{SEPARATOR_HTML}<p>B</p>"),
            "A\nB",
        ),
        (
            "multiple",
            "A\n====\nB\n====\nC",
            format!("<p>A</p>{SEPARATOR_HTML}<p>B</p>{SEPARATOR_HTML}<p>C</p>"),
            "A\nB\nC",
        ),
        (
            "quote",
            "> ====",
            format!("<blockquote>{SEPARATOR_HTML}</blockquote>"),
            "",
        ),
        (
            "raw",
            "@@====@@",
            "<p><span style=\"white-space: pre-wrap;\">====</span></p>".to_owned(),
            "====",
        ),
        ("run-1", "=", "<p>=</p>".to_owned(), "="),
        ("run-2", "==", "<p>==</p>".to_owned(), "=="),
        ("run-3", "===", "<p>===</p>".to_owned(), "==="),
        ("run-4", "====", SEPARATOR_HTML.to_owned(), ""),
        ("run-5", "=====", SEPARATOR_HTML.to_owned(), ""),
        ("run-6", "======", SEPARATOR_HTML.to_owned(), ""),
        ("run-7", "=======", SEPARATOR_HTML.to_owned(), ""),
        ("run-8", "========", SEPARATOR_HTML.to_owned(), ""),
        ("run-9", "=========", SEPARATOR_HTML.to_owned(), ""),
        (
            "table",
            "|| ==== ||",
            "<table class=\"wiki-content-table\">\n<tr>\n<td>====</td>\n</tr>\n</table>"
                .to_owned(),
            "====",
        ),
        ("trailing-space", "==== ", "<p>====</p>".to_owned(), "===="),
        (
            "unicode",
            "＝＝＝＝",
            "<p>＝＝＝＝</p>".to_owned(),
            "＝＝＝＝",
        ),
    ];

    assert_eq!(cases.len(), 27);
    for (case_id, source, expected_html, expected_text) in cases {
        let (html, text) = if case_id == "module" {
            let body = "A\n====\nB";
            let html = render_delayed(
                body,
                vec![InputSegment::text(0..body.len(), TextOrigin::Authored)],
            );
            (html, render(body).1)
        } else {
            render(source)
        };
        assert_eq!(html, expected_html, "{case_id}: {source:?}");
        assert_eq!(visible_lines(&text), expected_text, "{case_id}: {source:?}",);
    }
}

#[test]
fn horizontal_ascii_whitespace_and_crlf_are_exact() {
    for source in [" ====", "\t====", " \t ====\t ", "A\r\n\t===== \t\r\nB"] {
        assert!(!render(source).0.contains(SEPARATOR_HTML), "{source:?}");
    }

    for source in [
        "\u{000b}====",
        "====\u{000c}",
        "\u{00a0}====",
        "====\u{2003}",
        " ==== prose",
        "prose ====",
        " === =",
    ] {
        assert!(!render(source).0.contains(SEPARATOR_HTML), "{source:?}");
    }
}

#[test]
fn literal_owners_escapes_unicode_and_malformed_lines_cannot_create_boundaries() {
    for source in [
        "[[code]]\n====\n[[/code]]",
        "@@====@@",
        "[!--\n====\n--]",
        "[[html]]\n====\n[[/html]]",
        "\\====",
        "＝＝＝＝",
        "===",
        "A====B",
        "====B",
        "A====",
        "==== ==",
    ] {
        let tree = parse(source, Layout::Wikidot);
        assert!(
            tree.content_segments()
                .all(|segment| segment != ContentSegment::Boundary),
            "{source:?}: {tree:#?}",
        );
    }
}

#[test]
fn typed_segments_preserve_empty_leading_trailing_and_adjacent_sections() {
    let tree = parse("====\nA\n====\n====", Layout::Wikijump);
    let segments = tree.content_segments().collect::<Vec<_>>();

    assert_eq!(segments.len(), 7);
    assert!(matches!(segments[0], ContentSegment::Section([])));
    assert_eq!(segments[1], ContentSegment::Boundary);
    assert!(matches!(segments[2], ContentSegment::Section([_])));
    assert_eq!(segments[3], ContentSegment::Boundary);
    assert!(matches!(segments[4], ContentSegment::Section([])));
    assert_eq!(segments[5], ContentSegment::Boundary);
    assert!(matches!(segments[6], ContentSegment::Section([])));

    assert_eq!(
        render_with_layout("A\n====\nB", Layout::Wikijump).0,
        "<p>A</p><p>B</p>",
    );
}

#[test]
fn generated_and_runtime_text_have_no_structural_authority_at_the_delayed_seam() {
    let authored = "A\n====\nB";
    assert_eq!(
        render_delayed(
            authored,
            vec![InputSegment::text(0..authored.len(), TextOrigin::Authored)],
        ),
        format!("<p>A</p>{SEPARATOR_HTML}<p>B</p>"),
    );

    for (case_id, source, segments) in [
        (
            "runtime-separator",
            "====",
            vec![InputSegment::text(0..4, TextOrigin::RuntimeScalar)],
        ),
        (
            "mixed-origin-run",
            "====",
            vec![
                InputSegment::text(0..2, TextOrigin::Authored),
                InputSegment::text(2..4, TextOrigin::RuntimeScalar),
            ],
        ),
        (
            "runtime-newline",
            "\n====",
            vec![
                InputSegment::text(0..1, TextOrigin::RuntimeScalar),
                InputSegment::text(1..5, TextOrigin::Authored),
            ],
        ),
    ] {
        let html = render_delayed(source, segments);
        assert!(!html.contains(SEPARATOR_HTML), "{case_id}: {html}");
    }

    let generated_source = "====";
    let generated = DelayedInput::new(
        generated_source,
        vec![InputSegment::generated(GeneratedInput {
            source_range: 0..generated_source.len(),
            id: SlotId::new(1),
            kind: GeneratedKind::PageLink,
            occurrence: 0,
        })],
    )
    .expect("valid generated separator control");
    let bindings = SlotBindings::new(vec![(
        SlotId::new(1),
        GeneratedValue::PageLink {
            page: PageRef::page_only("target"),
            label: Cow::Borrowed("===="),
        },
    )])
    .expect("unique binding");
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
    let html = parse_delayed_list(&generated, &page_info, &settings)
        .expect("generated control parse")
        .bind(&bindings)
        .expect("generated control binding")
        .render_html(&page_info, &settings)
        .body()
        .to_owned();
    assert!(!html.contains(SEPARATOR_HTML), "{html}");
}

#[test]
fn listpages_runtime_shaped_controls_remain_literal_at_the_ftml_seam() {
    for source in [
        "A====B",
        "[[code]]\n====\n[[/code]]",
        "@@====@@",
        "[!--\n====\n--]",
        "[[html]]\n====\n[[/html]]",
        "[[module ListPages]] ====",
    ] {
        let html = render_delayed(
            source,
            vec![InputSegment::text(0..source.len(), TextOrigin::Authored)],
        );
        assert!(!html.contains(SEPARATOR_HTML), "{source:?}: {html}");
    }

    let malformed_then_valid = "[[module ListPages]]]\n====";
    let html = render_delayed(
        malformed_then_valid,
        vec![InputSegment::text(
            0..malformed_then_valid.len(),
            TextOrigin::Authored,
        )],
    );
    assert_eq!(html.matches(SEPARATOR_HTML).count(), 1, "{html}");
}

#[test]
fn boundary_serialization_to_owned_and_spoof_resistance_are_stable() {
    let borrowed = {
        let source = "====".to_owned();
        parse(&source, Layout::Wikijump)
    };
    assert_eq!(borrowed.elements, vec![Element::ContentSeparator]);

    let value = serde_json::to_value(&borrowed).expect("serialize typed boundary");
    assert_eq!(
        value["elements"][0],
        serde_json::json!({"element": "content-separator"}),
    );
    let restored: SyntaxTree<'static> =
        serde_json::from_value(value).expect("deserialize typed boundary");
    assert_eq!(restored.elements, borrowed.to_owned().elements);
    assert_eq!(
        restored.wikitext_len, 0,
        "the size hint is intentionally skipped"
    );

    let spoof = parse(
        "[[div class=\"content-separator\" style=\"display: none:\"]]\n[[/div]]",
        Layout::Wikidot,
    );
    assert!(
        spoof
            .content_segments()
            .all(|segment| segment != ContentSegment::Boundary),
        "a user-authored class must not become a typed boundary",
    );
    let (html, _) = render("==== onmouseover=alert(1)");
    assert!(!html.contains(SEPARATOR_HTML), "{html}");
    assert!(!html.contains("<script"), "{html}");
}

#[test]
fn dense_separator_parsing_is_linear_and_bounded() {
    let mut source = " \t====\t \r\n".repeat(8_192);
    source.push_str(&"=".repeat(16_384));
    let started = Instant::now();
    let tree = parse(&source, Layout::Wikidot);
    let elapsed = started.elapsed();

    assert_eq!(
        tree.elements
            .iter()
            .filter(|element| element.is_content_separator())
            .count(),
        1,
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "dense separator parse took {elapsed:?}",
    );
}
