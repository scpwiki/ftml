use ftml::data::{PageInfo, ScoreValue};
use ftml::delayed::{
    DelayedInput, GeneratedInput, GeneratedKind, GeneratedValue, InputSegment,
    SlotBindings, SlotId, TextOrigin, parse_delayed_list,
};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("syntax-differential"),
        category: None,
        site: Cow::Borrowed("scp-wiki"),
        title: Cow::Borrowed("Alignment ownership"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn render_wikidot(source: &str) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokens = ftml::tokenize(&source);
    let (tree, _) = ftml::parse(&tokens, &page_info, &settings).into();
    HtmlRender.render(&tree, &page_info, &settings).body
}

#[test]
fn registry_alignment_block_residuals_match_live_wikidot() {
    let cases = [
        (
            "residual-alignment-block-center",
            "[[=]]\nA\n[[/=]]",
            r#"<div style="text-align: center;"><p>A</p></div>"#,
        ),
        (
            "residual-alignment-block-code-owner",
            "[[code]]\n[[=]]\nA\n[[/=]]\n[[/code]]",
            r#"<div class="code"><pre><code>[[=]]
A
[[/=]]</code></pre></div>"#,
        ),
        (
            "residual-alignment-block-empty",
            "[[=]]\n\n[[/=]]",
            r#"<div style="text-align: center;"></div>"#,
        ),
        (
            "residual-alignment-block-escaped",
            "\\[[=]]\nA\n[[/=]]",
            "<p>\\[[=]]<br>\nA<br>\n[[/=]]</p>",
        ),
        (
            "residual-alignment-block-extra-close",
            "[[=]]\nA\n[[/=]]\n[[/=]]",
            "<div style=\"text-align: center;\"><p>A</p></div><br>\n[[/=]]",
        ),
        (
            "residual-alignment-block-heading",
            "[[=]]\n+ H\n[[/=]]",
            r#"<div style="text-align: center;"><h1 id="toc0"><span>H</span></h1></div>"#,
        ),
        (
            "residual-alignment-block-inline",
            "[[=]]A[[/=]]",
            "<p>[[=]]A[[/=]]</p>",
        ),
        (
            "residual-alignment-block-justify",
            "[[==]]\nA\n[[/==]]",
            r#"<div style="text-align: justify;"><p>A</p></div>"#,
        ),
        (
            "residual-alignment-block-left",
            "[[<]]\nA\n[[/<]]",
            r#"<div style="text-align: left;"><p>A</p></div>"#,
        ),
        (
            "residual-alignment-block-list",
            "[[=]]\n* A\n[[/=]]",
            "<div style=\"text-align: center;\"><ul>\n<li>A</li>\n</ul></div>",
        ),
        (
            "residual-alignment-block-nested-other",
            "[[=]]\nA\n[[>]]\nB\n[[/>]]\nC\n[[/=]]",
            concat!(
                "<div style=\"text-align: center;\"><p>A<br>\n</p>",
                "<div style=\"text-align: right;\"><p>B</p></div><br>\nC</div>",
            ),
        ),
        (
            "residual-alignment-block-nested-same",
            "[[=]]\nA\n[[=]]\nB\n[[/=]]\nC\n[[/=]]",
            concat!(
                "<div style=\"text-align: center;\"><p>A<br>\n</p>",
                "<div style=\"text-align: center;\"><p>B</p></div><br>\nC</div>",
            ),
        ),
        (
            "residual-alignment-block-paragraph",
            "[[=]]\nA\n\nB\n[[/=]]",
            r#"<div style="text-align: center;"><p>A</p><p>B</p></div>"#,
        ),
        (
            "residual-alignment-block-prose",
            "BEFORE\n[[=]]\nA\n[[/=]]\nAFTER",
            concat!(
                "<p>BEFORE<br>\n</p>",
                "<div style=\"text-align: center;\"><p>A</p></div><br>\nAFTER",
            ),
        ),
        (
            "residual-alignment-block-right",
            "[[>]]\nA\n[[/>]]",
            r#"<div style="text-align: right;"><p>A</p></div>"#,
        ),
        (
            "residual-alignment-block-table",
            "[[=]]\n|| A || B ||\n[[/=]]",
            concat!(
                "<div style=\"text-align: center;\">",
                "<table class=\"wiki-content-table\">\n<tr>\n<td>A</td>\n",
                "<td>B</td>\n</tr>\n</table></div>",
            ),
        ),
        (
            "residual-alignment-block-unclosed",
            "[[=]]\nA",
            "<p>[[=]]<br>\nA</p>",
        ),
        (
            "residual-alignment-block-wrong-close",
            "[[=]]\nA\n[[/<]]",
            "<p>[[=]]<br>\nA<br>\n[[/&lt;]]</p>",
        ),
    ];

    for (case_id, source, expected) in cases {
        assert_eq!(render_wikidot(source), expected, "{case_id}");
    }
}

#[test]
fn alignment_empty_bodies_use_each_exact_wikidot_style() {
    for (open, close, style) in [
        ("<", "<", "left"),
        (">", ">", "right"),
        ("=", "=", "center"),
        ("==", "==", "justify"),
    ] {
        assert_eq!(
            render_wikidot(&format!("[[{open}]]\n\n[[/{close}]]")),
            format!(r#"<div style="text-align: {style};"></div>"#),
        );
    }
}

#[test]
fn alignment_syntax_stays_inert_inside_literal_owners() {
    for source in [
        "[[code]]\n[[=]]\nCODE\n[[/=]]\n[[/code]]",
        "@@[[=]]RAW[[/=]]@@",
        "[!--\n[[=]]\nCOMMENT\n[[/=]]\n--]\nSENTINEL",
        "[[html]]\n[[=]]\nHTML\n[[/=]]\n[[/html]]",
    ] {
        let html = render_wikidot(source);
        assert!(!html.contains("text-align: center;"), "{source:?}: {html}");
    }
}

#[test]
fn alignment_recovery_preserves_generated_value_provenance() {
    let marker = "%%title_linked%%";
    for (escaped, expected_literal) in [(false, None), (true, Some("\\[[=]]"))] {
        let source =
            format!("{}[[=]]\n{marker}\n[[/=]]", if escaped { "\\" } else { "" },);
        let start = source.find(marker).expect("generated marker");
        let end = start + marker.len();
        let input = DelayedInput::new(
            &source,
            vec![
                InputSegment::text(0..start, TextOrigin::Authored),
                InputSegment::generated(GeneratedInput {
                    source_range: start..end,
                    id: SlotId::new(1),
                    kind: GeneratedKind::PageLink,
                    occurrence: 0,
                }),
                InputSegment::text(end..source.len(), TextOrigin::Authored),
            ],
        )
        .expect("valid delayed alignment input");
        let page_info = page_info();
        let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
        let delayed = parse_delayed_list(&input, &page_info, &settings)
            .expect("supported delayed input");
        let bindings = SlotBindings::new(vec![(
            SlotId::new(1),
            GeneratedValue::PageLink {
                page: ftml::data::PageRef::page_only("generated-page"),
                label: Cow::Borrowed("[[=]] generated label"),
            },
        )])
        .expect("unique generated binding");
        let bound = delayed.bind(&bindings).expect("matching generated binding");
        let html = bound.render_html(&page_info, &settings).body().to_owned();

        assert!(html.contains(r#"href="/generated-page""#), "{html}");
        assert!(html.contains("[[=]] generated label"), "{html}");
        if let Some(literal) = expected_literal {
            assert!(html.contains(literal), "{html}");
            assert!(!html.contains("text-align: center;"), "{html}");
            assert!(html.contains("[[/=]]"), "{html}");
        } else {
            assert!(html.starts_with(r#"<div style="text-align: center;">"#));
            assert!(!html.contains("[[/=]]"), "{html}");
        }
    }
}

#[test]
fn dense_nested_alignment_is_bounded_and_keeps_every_wrapper() {
    const DEPTH: usize = 64;
    let mut source = String::new();
    for depth in 0..DEPTH {
        let marker = ["=", ">", "==", "<"][depth % 4];
        source.push_str(&format!("[[{marker}]]\nlevel-{depth}\n"));
    }
    source.push_str("DEEPEST\n");
    for depth in (0..DEPTH).rev() {
        let marker = ["=", ">", "==", "<"][depth % 4];
        source.push_str(&format!("[[/{marker}]]\n"));
    }

    let started = Instant::now();
    let html = render_wikidot(&source);

    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(html.matches("text-align:").count(), DEPTH, "{html}");
    assert!(html.contains("DEEPEST"), "{html}");
    for depth in 0..DEPTH {
        assert!(html.contains(&format!("level-{depth}")), "{html}");
    }
}

#[test]
fn dense_wrong_alignment_closers_fail_closed_in_bounded_time() {
    const ROW_COUNT: usize = 512;
    let mut source = String::new();
    for row in 0..ROW_COUNT {
        source.push_str(&format!("[[=]]\nmalformed-{row}\n[[/<]]\n"));
    }
    source.push_str("OUTSIDE-SENTINEL");

    let started = Instant::now();
    let html = render_wikidot(&source);

    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(!html.contains("text-align:"), "{html}");
    assert_eq!(html.matches("[[=]]").count(), ROW_COUNT, "{html}");
    assert_eq!(html.matches("[[/&lt;]]").count(), ROW_COUNT, "{html}");
    assert!(html.contains("OUTSIDE-SENTINEL"), "{html}");
}

#[test]
fn wikijump_center_line_behavior_is_unchanged() {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let render = |source| {
        let tokens = ftml::tokenize(source);
        let (tree, _) = ftml::parse(&tokens, &page_info, &settings).into();
        HtmlRender.render(&tree, &page_info, &settings).body
    };

    assert!(render("= ").contains("wj-align-center"));
    assert!(!render(" = centered").contains("wj-align-center"));
}

#[test]
fn registry_center_line_residuals_match_live_wikidot() {
    let cases = [
        (
            "residual-center-line-basic",
            "= centered",
            r#"<p style="text-align: center;">centered</p>"#,
        ),
        ("residual-center-line-empty", "= ", "<p>=</p>"),
        (
            "residual-center-line-escaped",
            "\\= centered",
            "<p>\\= centered</p>",
        ),
        (
            "residual-center-line-heading-like",
            "+ = centered",
            r#"<h1 id="toc0"><span>= centered</span></h1>"#,
        ),
        (
            "residual-center-line-leading-space",
            " = centered",
            "<p>= centered</p>",
        ),
        (
            "residual-center-line-leading-tab",
            "\t= centered",
            "<p>= centered</p>",
        ),
        (
            "residual-center-line-blank-then-indented",
            "\n\n = centered =",
            "<p>= centered =</p>",
        ),
        (
            "residual-center-line-mid-document-indented",
            "foo\n\n = centered =",
            "<p>foo</p><p>= centered =</p>",
        ),
        (
            "residual-center-line-mid-paragraph-indented",
            "foo\n = centered =",
            "<p>foo<br>\n= centered =</p>",
        ),
        (
            "residual-center-line-after-spaced-line",
            " \n= centered",
            r#"<p style="text-align: center;">centered</p>"#,
        ),
        (
            "residual-center-line-list",
            "* = centered",
            "<ul>\n<li>= centered</li>\n</ul>",
        ),
        (
            "residual-center-line-prose",
            "A\n= centered\nB",
            concat!(
                "<p>A</p><p style=\"text-align: center;\">centered</p>",
                "<p>B</p>",
            ),
        ),
        (
            "residual-center-line-quote",
            "> = centered",
            r#"<blockquote><p style="text-align: center;">centered</p></blockquote>"#,
        ),
        (
            "residual-center-line-table",
            "|| = centered ||",
            concat!(
                "<table class=\"wiki-content-table\">\n<tr>\n",
                "<td>= centered</td>\n</tr>\n</table>",
            ),
        ),
        (
            "residual-center-line-tight",
            "=centered",
            "<p>=centered</p>",
        ),
        (
            "residual-center-line-two-equals",
            "== centered",
            "<p>== centered</p>",
        ),
        (
            "residual-center-line-unicode",
            "＝ centered",
            "<p>＝ centered</p>",
        ),
    ];

    for (case_id, source, expected) in cases {
        assert_eq!(render_wikidot(source), expected, "{case_id}");
    }
}
