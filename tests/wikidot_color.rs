use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::Element;
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("wikidot-color"),
        category: None,
        site: Cow::Borrowed("syntax-differential"),
        title: Cow::Borrowed("Wikidot color"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn parse(source: &str) -> ftml::tree::SyntaxTree<'static> {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokens = ftml::tokenize(&source);
    let (tree, _errors) = ftml::parse(&tokens, &page_info, &settings).into();
    tree.to_owned()
}

fn render(source: &str) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tree = parse(source);
    HtmlRender.render(&tree, &page_info, &settings).body
}

#[test]
fn residual_color_matrix_matches_live_wikidot_dom() {
    for (source, expected) in [
        (
            r###"[[span title="##red|A##"]]X[[/span]]"###,
            "<p><span>X</span></p>",
        ),
        (
            "##orange|**ORANGE##-PRIME:**",
            "<p><span style=\"color: orange\"><strong>ORANGE</strong></span><strong>-PRIME:</strong></p>",
        ),
        ("##red||A##", "<p><span style=\"color: red\">|A</span></p>"),
        ("##red|##", "<p>##red|##</p>"),
        ("##|A##", "<p>##|A##</p>"),
        (
            "##red|A####",
            "<p><span style=\"color: red\">A</span>##</p>",
        ),
        (
            "##red|A ##blue|B## C##",
            "<p><span style=\"color: red\">A</span> blue|B## C##</p>",
        ),
        (
            "## red | A ##",
            "<p><span style=\"color: red\">A</span></p>",
        ),
        ("##red|A", "<p>##red|A</p>"),
        (
            "##url(javascript:alert(1))|A##",
            "<p>##url(javascript:alert(1))|A##</p>",
        ),
    ] {
        assert_eq!(render(source), expected, "{source:?}");
    }

    assert_eq!(
        render("|| ##red|A||B## ||"),
        concat!(
            "<table class=\"wiki-content-table\">\n",
            "<tr>\n",
            "<td><span style=\"color: red\">A</span></td>\n",
            "<td>B</td>\n",
            "</tr>\n",
            "</table>",
        ),
    );
}

#[test]
fn foreground_and_background_admit_only_static_color_values() {
    for (source, expected_style) in [
        ("##red|A##", "color: red"),
        ("###B01|A##", "color: #b01"),
        ("##rgb(10, 12, 14)|A##", "color: rgb(10, 12, 14)"),
        ("##rgba(10,20,30,.5)|A##", "color: rgba(10,20,30,.5)"),
        ("##hsl(120, 50%, 25%)|A##", "color: hsl(120, 50%, 25%)"),
        (
            "##hsla(120deg,50%,25%,75%)|A##",
            "color: hsla(120deg,50%,25%,75%)",
        ),
        ("##|gold|A##", "background-color: gold"),
    ] {
        assert_eq!(
            render(source),
            format!("<p><span style=\"{expected_style}\">A</span></p>"),
            "{source:?}",
        );
    }

    let tree = parse("##|gold|A##");
    let [Element::Container(paragraph)] = tree.elements.as_slice() else {
        panic!("expected a paragraph: {:#?}", tree.elements);
    };
    let [Element::Color { background, .. }] = paragraph.elements() else {
        panic!("expected a color node: {:#?}", paragraph.elements());
    };
    assert!(*background);
}

#[test]
fn css_var_color_values_render_as_spans() {
    for (source, expected) in [
        (
            "##var(--c-keyword)|A##",
            "<p><span style=\"color: var(--c-keyword)\">A</span></p>",
        ),
        (
            "##|var(--c-accent)|A##",
            "<p><span style=\"background-color: var(--c-accent)\">A</span></p>",
        ),
        (
            "##var(--missing, red)|A##",
            "<p><span style=\"color: var(--missing, red)\">A</span></p>",
        ),
        (
            "##var(--a, var(--b, blue))|A##",
            "<p><span style=\"color: var(--a, var(--b, blue))\">A</span></p>",
        ),
        (
            "**##var(--c-keyword)|[[/####var(--c-builtin)|div####var(--c-keyword)|]]##**",
            concat!(
                "<p><strong>",
                "<span style=\"color: var(--c-keyword)\">[[/</span>",
                "<span style=\"color: var(--c-builtin)\">div</span>",
                "<span style=\"color: var(--c-keyword)\">]]</span>",
                "</strong></p>",
            ),
        ),
    ] {
        assert_eq!(render(source), expected, "{source:?}");
    }
}

#[test]
fn unsafe_and_malformed_color_values_remain_literal() {
    for source in [
        "##|red|##",
        "##||A##",
        "##expression(alert(1))|A##",
        "##rgb(1, url(x), 3)|A##",
        "##rgb(1, 2)|A##",
        "##hsl(1, 2, 3)|A##",
        "##not-a-color|A##",
        "##red;background:blue|A##",
        "##var(x)|A##",
        "##var()|A##",
    ] {
        assert_eq!(render(source), format!("<p>{source}</p>"), "{source:?}");
    }
}

#[test]
fn dense_valid_and_malformed_color_runs_remain_bounded() {
    const RUNS: usize = 2_048;
    let valid = std::iter::repeat_n("##rgb(1, 2, 3)|A##", RUNS)
        .collect::<Vec<_>>()
        .join(" ");
    let started = Instant::now();
    let html = render(&valid);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(
        html.matches("<span style=\"color: rgb(1, 2, 3)\">").count(),
        RUNS
    );

    let malformed = format!("##rgb({}|A##", "1,".repeat(8_192));
    let started = Instant::now();
    let tree = parse(&malformed);
    assert!(started.elapsed() < Duration::from_secs(5));
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let html = HtmlRender.render(&tree, &page_info, &settings).body;
    assert!(html.starts_with("<p>##rgb("));
    assert!(html.ends_with("|A##</p>"));
}

#[test]
fn color_close_after_double_bracket_is_not_eaten_by_anchor_token() {
    for (source, expected) in [
        (
            "##darkred|[[##",
            "<p><span style=\"color: darkred\">[[</span></p>",
        ),
        (
            "**##darkred|[[##**",
            "<p><strong><span style=\"color: darkred\">[[</span></strong></p>",
        ),
        (
            "**##var(--c-keyword)|[[##**",
            "<p><strong><span style=\"color: var(--c-keyword)\">[[</span></strong></p>",
        ),
        (
            "##darkred|[[####blue|div##",
            "<p><span style=\"color: darkred\">[[</span><span style=\"color: blue\">div</span></p>",
        ),
        (
            "**##darkred|[[##**##blue|include##",
            "<p><strong><span style=\"color: darkred\">[[</span></strong><span style=\"color: blue\">include</span></p>",
        ),
        (
            "##darkred|[[# anchor]]##",
            "<p><span style=\"color: darkred\"><a name=\"anchor\"></a></span></p>",
        ),
    ] {
        assert_eq!(render(source), expected, "{source:?}");
    }
}
