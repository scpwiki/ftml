use ftml::data::{PageInfo, PageRef, ScoreValue};
use ftml::delayed::{
    DelayedInput, GeneratedInput, GeneratedKind, GeneratedValue, InputSegment,
    SlotBindings, SlotId, TextOrigin, parse_delayed_list,
};
use ftml::layout::Layout;
use ftml::parsing::ParseError;
use ftml::render::{Render, html::HtmlRender, text::TextRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::SyntaxTree;
use regex::Regex;
use serde_json::Value;
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

const SIGNED_LIVE_MATRIX: &str =
    include_str!("fixtures/wikidot-alias-closers-live-20260730.json");
const FOLLOWUP_LIVE_MATRIX: &str =
    include_str!("fixtures/wikidot-alias-closers-followup-live-20260807.json");

static INTERTAG_WHITESPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r">\s+<").expect("valid normalization regex"));

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("issue-337-alias-closers"),
        category: None,
        site: Cow::Borrowed("scp-wiki"),
        title: Cow::Borrowed("Issue 337 alias closers"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn settings(mode: WikitextMode) -> WikitextSettings {
    let mut settings = WikitextSettings::from_mode(mode, Layout::Wikidot);
    settings.enable_html_blocks = false;
    settings
}

fn parse_render(source: &str) -> (SyntaxTree<'static>, String, String, Vec<ParseError>) {
    let page_info = page_info();
    let settings = settings(WikitextMode::Page);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokens = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokens, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;
    let text = TextRender.render(&tree, &page_info, &settings);
    (tree.to_owned(), html, text, errors)
}

fn normalize_live_html(html: &str) -> String {
    let html = html.strip_prefix("\n\n").unwrap_or(html);
    let html = html
        .strip_suffix('\n')
        .unwrap_or(html)
        .replace("<br />", "<br>")
        .replace("&#171;", "«")
        .replace("&#187;", "»")
        .replace("&#32;", " ")
        .replace("&nbsp;", "\u{a0}")
        .replace("</table>\r\n", "</table>")
        .replace("</table>\n", "</table>");
    INTERTAG_WHITESPACE.replace_all(&html, "><").into_owned()
}

fn assert_live_fixture(fixture: &str, expected_schema: &str, expected_rows: usize) {
    let document: Value = serde_json::from_str(fixture).expect("valid live fixture JSON");
    assert_eq!(document["schema"], expected_schema);
    assert_eq!(document["provenance"]["authenticated"], false);
    assert_eq!(document["provenance"]["mutated"], false);
    assert_eq!(document["provenance"]["module"], "edit/PagePreviewModule",);
    assert_eq!(document["provenance"]["site"], "scp-wiki");

    let rows = document["rows"].as_array().expect("fixture rows");
    assert_eq!(rows.len(), expected_rows);
    let mut case_ids = BTreeSet::new();
    let mut mismatches = Vec::new();
    for row in rows {
        let case_id = row["case_id"].as_str().expect("case id");
        assert!(case_ids.insert(case_id), "duplicate case id {case_id}");
        let source = row["source"].as_str().expect("source");
        let expected = row["raw_html"].as_str().expect("live HTML");
        for field in ["source_sha256", "raw_html_sha256"] {
            let hash = row[field].as_str().expect("SHA-256 field");
            assert_eq!(hash.len(), 64, "{case_id}: malformed {field}");
            assert!(
                hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "{case_id}: malformed {field}",
            );
        }

        let (_, actual, _, _) = parse_render(source);
        let actual = normalize_live_html(&actual);
        let expected = normalize_live_html(expected);
        if actual != expected {
            mismatches.push(format!(
                "{case_id}\nsource: {source:?}\nactual: {actual:?}\nexpected: {expected:?}",
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "live PagePreview mismatches:\n\n{}",
        mismatches.join("\n\n"),
    );
}

#[test]
fn signed_live_alias_matrix_matches_all_fifty_observations() {
    assert_live_fixture(
        SIGNED_LIVE_MATRIX,
        "ftml.wikidot_alias_closers_live_observation.v1",
        50,
    );
}

#[test]
fn followup_live_alias_matrix_matches_edge_and_control_observations() {
    assert_live_fixture(
        FOLLOWUP_LIVE_MATRIX,
        "ftml.wikidot_alias_closers_followup_live_observation.v1",
        41,
    );
}

#[test]
fn rejected_alias_closers_preserve_exact_literal_text() {
    for name in ["div", "span"] {
        let source = format!("[[{name}_]]\n+ HEADING\n* LIST\n|| TABLE ||\n[[/{name}_]]");
        let (_, html, text, errors) = parse_render(&source);
        assert!(!errors.is_empty(), "rejected {name}_ alias needs recovery");
        assert!(html.contains(&format!("[[{name}_]]")), "{html}");
        assert!(html.contains(&format!("[[/{name}_]]")), "{html}");
        assert!(text.starts_with(&format!("[[{name}_]]\n")), "{text}");
        assert!(text.ends_with(&format!("\n[[/{name}_]]")), "{text}");
    }
}

#[test]
fn active_alias_tree_is_owned_serializable_and_has_no_parser_markers() {
    let source = concat!(
        "[[div_ class=\"outer\"]]\n",
        "[[span_ class=\"inner\"]]\n雪 Ω\n[[/span]]\n",
        "[[/div]]",
    );
    let (owned, html, _, errors) = parse_render(source);
    // Live PagePreview keeps the scored div head literal when its body opens
    // with a paired scored span alias, and renders the alias inline inside the
    // recovered paragraph (captured 2026-11-05).
    assert!(
        !errors.is_empty(),
        "literal recovery records fallback errors"
    );
    assert_eq!(
        html,
        "<p>[[div_ class=&quot;outer&quot;]]<span class=\"inner\">雪 Ω</span>[[/div]]</p>",
    );

    let serialized = serde_json::to_string(&owned).expect("serializable owned tree");
    assert!(!serialized.contains("data-ftml"), "{serialized}");
    assert!(!serialized.contains("scored-span"), "{serialized}");
    let restored: SyntaxTree<'_> =
        serde_json::from_str(&serialized).expect("deserializable owned tree");
    assert_eq!(
        HtmlRender
            .render(&restored, &page_info(), &settings(WikitextMode::Page))
            .body,
        html,
    );
}

#[test]
fn alias_attributes_keep_the_existing_security_boundary() {
    for name in ["div", "span"] {
        let source = format!(
            "[[{name}_ onclick=\"alert(1)\" style=\"background:url(javascript:alert(2))\"]]\nBODY\n[[/{name}]]",
        );
        let (_, html, _, errors) = parse_render(&source);
        assert!(errors.is_empty(), "{errors:#?}");
        assert!(!html.contains("onclick"), "{html}");
        assert!(!html.contains("<script"), "{html}");
        assert!(html.starts_with(&format!("<{name} style=")), "{html}");
    }
}

fn render_delayed(input: &DelayedInput<'_>, bindings: &SlotBindings<'_>) -> String {
    let page_info = page_info();
    let settings = settings(WikitextMode::List);
    let delayed = parse_delayed_list(input, &page_info, &settings)
        .expect("supported delayed alias input");
    let bound = delayed.bind(bindings).expect("matching delayed bindings");
    bound.render_html(&page_info, &settings).body().to_owned()
}

#[test]
fn runtime_scalar_alias_markers_have_no_syntax_authority() {
    let source = "[[span_]]\nBODY\n[[/span]]";
    let entirely_runtime = DelayedInput::new(
        source,
        vec![InputSegment::text(
            0..source.len(),
            TextOrigin::RuntimeScalar,
        )],
    )
    .expect("valid runtime input");
    assert_eq!(
        render_delayed(&entirely_runtime, &SlotBindings::empty()),
        "<p>[[span_]]\nBODY\n[[/span]]</p>",
    );

    let close_start = source.find("[[/span]]").expect("closer");
    let runtime_close = DelayedInput::new(
        source,
        vec![
            InputSegment::text(0..close_start, TextOrigin::Authored),
            InputSegment::text(close_start..source.len(), TextOrigin::RuntimeScalar),
        ],
    )
    .expect("valid split runtime input");
    let html = render_delayed(&runtime_close, &SlotBindings::empty());
    assert!(!html.contains("<span"), "{html}");
    assert!(html.contains("[[span_]]"), "{html}");
    assert!(html.contains("[[/span]]"), "{html}");
}

#[test]
fn generated_alias_markers_have_no_syntax_authority() {
    let marker = "%%title_linked%%";
    let source = format!("[[span_]]\n{marker}");
    let start = source.find(marker).expect("generated marker");
    let input = DelayedInput::new(
        &source,
        vec![
            InputSegment::text(0..start, TextOrigin::Authored),
            InputSegment::generated(GeneratedInput {
                source_range: start..source.len(),
                id: SlotId::new(337),
                kind: GeneratedKind::PageLink,
                occurrence: 0,
            }),
        ],
    )
    .expect("valid generated input");
    let bindings = SlotBindings::new(vec![(
        SlotId::new(337),
        GeneratedValue::PageLink {
            page: PageRef::page_only("alias-closer-control"),
            label: Cow::Borrowed("[[/span]]"),
        },
    )])
    .expect("unique binding");
    let html = render_delayed(&input, &bindings);
    assert!(!html.contains("<span"), "{html}");
    assert!(html.contains("[[span_]]"), "{html}");
    assert!(html.contains("href=\"/alias-closer-control\""), "{html}");
    assert!(html.contains("[[/span]]"), "{html}");
}

#[test]
fn rejected_deep_alias_nesting_is_bounded_and_non_recursive() {
    const DEPTH: usize = 4096;
    let mut source = String::with_capacity(DEPTH * 22);
    for _ in 0..DEPTH {
        source.push_str("[[span_]]\n");
    }
    source.push_str("BODY\n");
    for _ in 0..DEPTH {
        source.push_str("[[/span_]]\n");
    }

    let started = Instant::now();
    let (_, html, _, errors) = parse_render(&source);
    assert!(!errors.is_empty());
    assert!(!html.contains("<span"), "alias unexpectedly activated");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "iterative alias recovery exceeded its bounded budget: {:?}",
        started.elapsed(),
    );
}
