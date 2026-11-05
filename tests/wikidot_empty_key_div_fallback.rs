use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::parsing::{ParseError, ParseErrorKind};
use ftml::render::{Render, html::HtmlRender, text::TextRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::{ContainerType, Element, SyntaxTree};
use serde_json::Value;
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("issue-316-empty-key-div-fallback"),
        category: None,
        site: Cow::Borrowed("sandbox-for-codex"),
        title: Cow::Borrowed("Issue 316 empty-key div fallback"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn parse_render(
    source: &str,
    layout: Layout,
) -> (SyntaxTree<'static>, String, String, Vec<ParseError>) {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;
    let text = TextRender.render(&tree, &page_info, &settings);
    (tree.to_owned(), html, text, errors)
}

fn render_wikidot(source: &str) -> (String, String, Vec<ParseError>) {
    let (_, html, text, errors) = parse_render(source, Layout::Wikidot);
    (html, text, errors)
}

fn malformed_errors(errors: &[ParseError]) -> Vec<(&str, std::ops::Range<usize>)> {
    errors
        .iter()
        .filter(|error| {
            matches!(
                error.kind(),
                ParseErrorKind::BlockMalformedArguments | ParseErrorKind::RuleFailed
            )
        })
        .map(|error| (error.rule(), error.span()))
        .collect()
}

fn live_fixture() -> Value {
    serde_json::from_str(include_str!(
        "fixtures/wikidot-empty-key-div-live-20260731.json"
    ))
    .expect("valid issue 316 live fixture")
}

#[test]
fn live_backed_empty_key_div_is_one_literal_paragraph_with_a_diagnostic() {
    let fixture = live_fixture();
    assert_eq!(
        fixture["schema"],
        "ftml.wikidot_empty_key_div_live_observation.v1"
    );
    assert_eq!(fixture["transport"], "anonymous edit/PagePreviewModule");
    assert_eq!(fixture["mutated_server_state"], false);
    assert_eq!(
        fixture["reference_tree"],
        "59f18f01a270782e6ac6b36c569593ecbc051758"
    );

    let row = &fixture["rows"][0];
    assert_eq!(
        row["source_sha256"],
        "70c72841e3511ad3ea1bcec8601a4e7f1abc1c9880652a14d3eeb2e315320aaa",
    );
    let source = row["source"].as_str().expect("fixture source");
    let (tree, html, text, errors) = parse_render(source, Layout::Wikidot);

    assert_eq!(html, row["html"].as_str().expect("fixture HTML"));
    assert_eq!(text, row["text"].as_str().expect("fixture text"));
    assert_eq!(malformed_errors(&errors).len(), 1, "{errors:#?}");
    assert_eq!(malformed_errors(&errors)[0].0, "block-div");
    assert_eq!(malformed_errors(&errors)[0].1, 16..20);
    assert!(matches!(
        tree.elements.as_slice(),
        [Element::Container(container)] if container.ctype() == ContainerType::Paragraph
    ));
}

#[test]
fn empty_key_div_and_span_names_roll_back_separately_from_valid_attributes() {
    for (name, close) in [("div", "div", "block-div"), ("div_", "div", "block-div")]
        .map(|(name, close, _)| (name, close))
    {
        let source = format!(r#"[[{name} ="value"]]body[[/{close}]]"#);
        let (html, text, errors) = render_wikidot(&source);
        assert_eq!(
            html,
            format!("<p>{}</p>", source.replace('"', "&quot;")),
            "{name}",
        );
        assert_eq!(text, source, "{name}");
        assert!(malformed_errors(&errors).len() <= 1, "{name}: {errors:#?}");
    }

    for argument in ["=\"value\"", "= \"value\"", "='value'", "=bare"] {
        let source = format!("[[span {argument}]]body[[/span]]");
        let (html, text, errors) = render_wikidot(&source);
        assert_eq!(html, "<p><span>body</span></p>", "{argument}");
        assert_eq!(text, "body", "{argument}");
        assert!(malformed_errors(&errors).is_empty(), "{errors:#?}");
    }

    for (source, expected) in [
        (
            "[[div class=\"value\"]]\nbody\n[[/div]]",
            "<div class=\"value\"><p>body</p></div>",
        ),
        (
            "[[div_ class=\"value\"]]\nbody\n[[/div]]",
            "<div class=\"value\">body</div>",
        ),
        (
            "[[span class=\"value\"]]body[[/span]]",
            "<p><span class=\"value\">body</span></p>",
        ),
        // Live PagePreview leaves a run whose last content is a scored span
        // unwrapped (captured 2026-11-05; same shape as
        // span/fused-opener-scored-map-residual).
        (
            "[[span_ class=\"value\"]]body[[/span]]",
            "<span class=\"value\">body</span>",
        ),
    ] {
        let (html, _, errors) = render_wikidot(source);
        assert_eq!(html, expected, "{source:?}");
        assert!(
            malformed_errors(&errors).is_empty(),
            "{source:?}: {errors:#?}"
        );
    }
}

#[test]
fn empty_key_owners_follow_live_placement_and_nested_syntax_rules() {
    let cases = [
        (
            "inline",
            "[[div =\"value\"]]body[[/div]]",
            "<p>[[div =&quot;value&quot;]]body[[/div]]</p>",
            "[[div =\"value\"]]body[[/div]]",
            true,
        ),
        (
            "own-line",
            "[[div =\"value\"]]\nbody\n[[/div]]",
            "<div><p>body</p></div>",
            "body",
            false,
        ),
        (
            "prose-adjacent",
            "before [[span =\"value\"]]body[[/span]] after",
            "<p>before <span>body</span> after</p>",
            "before body after",
            false,
        ),
        (
            "nested-container",
            "[[div]]\n[[span =\"value\"]]body[[/span]]\n[[/div]]",
            "<div><p><span>body</span></p></div>",
            "body",
            false,
        ),
        (
            "nested-syntax",
            "[[span =\"value\"]]**body**[[/span]]",
            "<p><span><strong>body</strong></span></p>",
            "body",
            false,
        ),
    ];

    for (case_id, source, expected, expected_text, diagnostic) in cases {
        let (html, text, errors) = render_wikidot(source);
        assert_eq!(html, expected, "{case_id}");
        assert_eq!(text, expected_text, "{case_id}");
        assert_eq!(
            !malformed_errors(&errors).is_empty(),
            diagnostic,
            "{case_id}: {errors:#?}"
        );
    }
}

#[test]
fn empty_key_candidates_recover_inside_quote_list_and_table_owners() {
    for (case_id, source, expected, diagnostic) in [
        (
            "quote",
            "> [[div =\"value\"]]body[[/div]]",
            "<blockquote><p>[[div =&quot;value&quot;]]body[[/div]]</p></blockquote>",
            true,
        ),
        (
            "list",
            "* [[span =\"value\"]]body[[/span]]",
            "<ul>\n<li><span>body</span></li>\n</ul>",
            false,
        ),
        (
            "table",
            "|| [[span =\"value\"]]body[[/span]] ||",
            "<table class=\"wiki-content-table\">\n<tr>\n<td><span>body</span></td>\n</tr>\n</table>",
            false,
        ),
    ] {
        let (html, _, errors) = render_wikidot(source);
        assert_eq!(html, expected, "{case_id}");
        assert_eq!(
            !malformed_errors(&errors).is_empty(),
            diagnostic,
            "{case_id}: {errors:#?}"
        );
    }
}

#[test]
fn missing_and_extra_closers_recover_for_later_syntax() {
    let missing = "[[span =\"value\"]]body\n\n**later**";
    let (html, _, errors) = render_wikidot(missing);
    assert_eq!(
        html,
        "<p>[[span =&quot;value&quot;]]body</p><p><strong>later</strong></p>",
    );
    assert_eq!(malformed_errors(&errors).len(), 1, "{errors:#?}");

    let extra = "[[span =\"value\"]]body[[/span]][[/span]]\n\n**later**";
    let (html, _, errors) = render_wikidot(extra);
    assert_eq!(
        html,
        "<p><span>body</span>[[/span]]</p><p><strong>later</strong></p>",
    );
    assert!(malformed_errors(&errors).is_empty(), "{errors:#?}");
}

#[test]
fn literal_owners_keep_empty_key_candidates_inert() {
    for source in [
        "[[code]]\n[[span =\"value\"]]body[[/span]]\n[[/code]]",
        "@@[[span =\"value\"]]body[[/span]]@@",
        "[!-- [[span =\"value\"]]body[[/span]] --]sentinel",
        "[[html]]\n[[span =\"value\"]]body[[/span]]\n[[/html]]",
    ] {
        let (_, _, errors) = render_wikidot(source);
        assert!(
            malformed_errors(&errors).is_empty(),
            "{source:?}: {errors:#?}"
        );
    }
}

#[test]
fn malformed_empty_key_div_releases_nested_safe_syntax_without_unsafe_html() {
    let source = concat!(
        "[[div =\"value\" onclick=\"alert(1)\"]]<script>alert(2)</script>",
        "[[span class=\"trusted\"]]nested[[/span]][[/div]]",
    );
    let (tree, html, text, errors) = parse_render(source, Layout::Wikidot);
    assert_eq!(malformed_errors(&errors).len(), 1, "{errors:#?}");
    assert_eq!(
        text,
        "[[div =\"value\" onclick=\"alert(1)\"]]<script>alert(2)</script>nested[[/div]]"
    );
    assert!(
        html.starts_with("<p>[[div =&quot;value&quot; onclick="),
        "{html}"
    );
    assert!(
        html.contains("&lt;script&gt;alert(2)&lt;/script&gt;"),
        "{html}"
    );
    assert!(!html.contains("<script>"), "{html}");
    assert!(!html.contains("<div "), "{html}");
    assert!(
        html.contains("<span class=\"trusted\">nested</span>"),
        "{html}"
    );
    assert!(!tree.elements.iter().any(|element| {
        matches!(element, Element::Container(container) if container.ctype() == ContainerType::Div)
    }));

    let source = "[[span class=\"safe\" =\"bad\"]]body[[/span]]";
    let (_, html, text, errors) = parse_render(source, Layout::Wikidot);
    assert_eq!(text, "body");
    assert!(malformed_errors(&errors).is_empty(), "{errors:#?}");
    assert_eq!(html, "<p><span class=\"safe\">body</span></p>");

    let source = "[[span class=\"safe\" =bare]]body[[/span]]";
    let (_, html, text, errors) = parse_render(source, Layout::Wikidot);
    assert_eq!(text, "body");
    assert!(malformed_errors(&errors).is_empty(), "{errors:#?}");
    assert_eq!(html, "<p><span class=\"safe\">body</span></p>");
}

#[test]
fn unicode_and_crlf_empty_key_source_keeps_normalized_line_boundaries() {
    let source = "[[div =\"値😀\"]]\r\n雪\r\n[[/div]]";
    let (html, text, errors) = render_wikidot(source);
    assert_eq!(html, "<div><p>雪</p></div>");
    assert_eq!(text, "雪");
    assert!(malformed_errors(&errors).is_empty(), "{errors:#?}");
}

#[test]
fn v7_active_div_paragraph_rows_match_live_dom() {
    let fixture = live_fixture();
    for row in fixture["rows"]
        .as_array()
        .expect("fixture rows")
        .iter()
        .skip(1)
    {
        let case_id = row["case_id"].as_str().expect("case id");
        let source = row["source"].as_str().expect("source");
        let expected = row["html"].as_str().expect("HTML");
        let (html, _, _) = render_wikidot(source);
        assert_eq!(html, expected, "{case_id}");
    }
}

#[test]
fn repeated_empty_key_candidates_stay_bounded() {
    const CANDIDATE_COUNT: usize = 1_024;
    let unit = "[[span =\"value\"]]**literal**[[/span]] ";
    let source = unit.repeat(CANDIDATE_COUNT);
    let started = Instant::now();
    let (html, text, errors) = render_wikidot(&source);
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(5),
        "empty-key recovery took {elapsed:?}",
    );
    assert!(malformed_errors(&errors).is_empty(), "{errors:#?}");
    assert_eq!(html.matches("<span>").count(), CANDIDATE_COUNT);
    assert_eq!(
        html.matches("<strong>literal</strong>").count(),
        CANDIDATE_COUNT
    );
    assert_eq!(text.matches("literal").count(), CANDIDATE_COUNT);

    let unclosed_unit = "[[span =\"value\"]] ";
    let unclosed_source = unclosed_unit.repeat(CANDIDATE_COUNT);
    let started = Instant::now();
    let (html, text, errors) = render_wikidot(&unclosed_source);
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(5),
        "unclosed empty-key recovery took {elapsed:?}",
    );
    assert_eq!(malformed_errors(&errors).len(), CANDIDATE_COUNT);
    assert_eq!(text, unclosed_source.trim_end());
    assert!(!html.contains("<span"), "{html}");
}

#[test]
fn wikijump_layout_keeps_strict_empty_key_rejection() {
    let source = "[[span =\"value\"]]body[[/span]]";
    let (_, html, text, errors) = parse_render(source, Layout::Wikijump);
    assert_eq!(html, "<p>[[span =&quot;value&quot;]]body[[/span]]</p>");
    assert_eq!(text, source);
    assert!(
        errors
            .iter()
            .any(|error| error.kind() == ParseErrorKind::BlockMalformedArguments),
        "{errors:#?}",
    );
}
