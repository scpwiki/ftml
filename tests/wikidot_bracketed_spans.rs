use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};

fn render(source: &str, layout: Layout) -> String {
    let info = PageInfo {
        page: "bracketed-spans".into(),
        category: None,
        site: "test".into(),
        title: "Bracketed spans".into(),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: "en".into(),
    };
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, layout);
    let tokens = ftml::tokenize(&source);
    let (tree, _) = ftml::parse(&tokens, &info, &settings).into();
    HtmlRender.render(&tree, &info, &settings).body
}

#[test]
fn wikidot_spans_inside_literal_brackets_keep_their_inline_owner() {
    for (source, expected) in [
        (
            "[[[span class=\"blinkbar\"]]1[[/span]]]",
            "<p>[<span class=\"blinkbar\">1</span>]</p>",
        ),
        (
            "//[[[span class=\"invis\"]]VISUAL ERROr[[/span]] walks.]//",
            "<p><em>[<span class=\"invis\">VISUAL ERROr</span> walks.]</em></p>",
        ),
        (
            "[[[span style=\"font-family: 'Atkinson Hyperlegible'\"]]##422B24|sample##[[/span]]]",
            "<p>[<span style=\"font-family: &#39;Atkinson Hyperlegible&#39;\"><span style=\"color: #422b24\">sample</span></span>]</p>",
        ),
    ] {
        assert_eq!(render(source, Layout::Wikidot), expected, "{source}");
    }
}

#[test]
fn ordinary_triple_links_and_native_layout_keep_their_ownership() {
    for source in [
        "[[[span]]]",
        "[[[span|label]]]",
        "[[[spanning-space|Spanning Space]]]",
    ] {
        let html = render(source, Layout::Wikidot);
        assert!(html.contains("<a "), "{source}: {html}");
        assert!(!html.contains("<span"), "{source}: {html}");
    }
    assert!(render("[[[span]]text[[/span]]]", Layout::Wikijump).contains("<a "));
}

#[test]
fn retained_saved_page_excerpts_keep_the_bracketed_span_shape() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/wikidot-bracketed-spans-evidence.json"
    ))
    .unwrap();
    for observation in fixture["observations"].as_array().unwrap() {
        let source = observation["source_excerpt"].as_str().unwrap();
        let observed = observation["html_excerpt"].as_str().unwrap();
        assert!(observed.contains("[<span"), "{observation}");
        let rendered = render(source, Layout::Wikidot);
        assert!(rendered.contains("[<span"), "{rendered}");
        assert!(!rendered.contains("href=\"/span"), "{rendered}");
    }
}
