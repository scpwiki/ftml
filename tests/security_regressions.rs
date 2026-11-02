use ftml::data::{PageInfo, PageRef, ScoreValue};
use ftml::includes::DebugIncluder;
use ftml::layout::Layout;
use ftml::parsing::ParseErrorKind;
use ftml::render::Render;
use ftml::render::html::HtmlOutput;
use ftml::render::html::HtmlRender;
use ftml::render::text::TextRender;
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::{
    AttributeMap, ContainerType, Element, LinkLabel, LinkLocation, LinkType, ListItem,
    PartialElement, RubyText, SyntaxTree, Tab, TableCell, TableRow,
};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::num::NonZeroU32;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("security-regression"),
        category: None,
        site: Cow::Borrowed("security"),
        title: Cow::Borrowed("Security Regression"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![],
        language: Cow::Borrowed("default"),
    }
}

fn parse(input: &str, layout: Layout) -> SyntaxTree<'static> {
    let (tree, errors) = parse_with_errors(input, layout);

    assert!(errors.is_empty(), "{errors:?}");
    tree
}

fn parse_with_errors(
    input: &str,
    layout: Layout,
) -> (SyntaxTree<'static>, Vec<ftml::parsing::ParseError>) {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut text = input.to_owned();
    ftml::preprocess_for_layout(&mut text, layout);
    let tokens = ftml::tokenize(&text);
    let result = ftml::parse(&tokens, &page_info, &settings);
    let (tree, errors) = result.into();

    (tree.to_owned(), errors)
}

fn render_html(tree: &SyntaxTree, layout: Layout) -> String {
    render_html_output(tree, layout).body
}

fn render_html_output(tree: &SyntaxTree, layout: Layout) -> HtmlOutput {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    HtmlRender.render(tree, &page_info, &settings)
}

fn render_text(tree: &SyntaxTree, layout: Layout) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    TextRender.render(tree, &page_info, &settings)
}

#[test]
fn doubled_block_score_marker_fails_closed_without_panicking() {
    for source in [
        "[[div__]]",
        "[[span__]]",
        "[[ul__]]",
        "[[size__ 100%]]",
        "[[*div__]]",
    ] {
        let result = std::panic::catch_unwind(|| {
            let (tree, _errors) = parse_with_errors(source, Layout::Wikidot);
            render_html(&tree, Layout::Wikidot)
        });

        assert!(
            result.is_ok(),
            "double-score block syntax must not panic: {source}"
        );
        assert_eq!(result.unwrap(), format!("<p>{source}</p>"));
    }
}

fn assert_false_iftags_guarded(hidden: &str, layout: Layout) {
    let input = format!(
        "[[iftags +missing]]\n{hidden}\n[[html]]\n<b>guarded</b>\n[[/html]]\n[[/iftags]]\nvisible",
    );
    let tree = parse(&input, layout);

    assert_eq!(
        render_text(&tree, layout),
        "visible",
        "{layout:?}: {hidden}"
    );
    assert!(tree.html_blocks.is_empty(), "{layout:?}: {hidden}");
    assert!(tree.code_blocks.is_empty(), "{layout:?}: {hidden}");
}

fn assert_false_iftags_fails_closed(hidden: &str, layout: Layout) {
    let tree = parse(&format!("[[iftags +missing]]\n{hidden}"), layout);

    assert!(tree.elements.is_empty(), "{layout:?}: {hidden}");
    assert!(tree.html_blocks.is_empty(), "{layout:?}: {hidden}");
    assert!(tree.code_blocks.is_empty(), "{layout:?}: {hidden}");
}

fn render_iframe_with_url_and_attributes(
    url: &'static str,
    attributes: AttributeMap,
) -> String {
    let tree = SyntaxTree {
        elements: vec![Element::Iframe {
            url: Cow::Borrowed(url),
            attributes,
        }],
        ..SyntaxTree::default()
    };

    render_html(&tree, Layout::Wikijump)
}

fn render_math_inline_source(latex_source: &'static str) -> String {
    let tree = SyntaxTree {
        elements: vec![Element::MathInline {
            latex_source: Cow::Borrowed(latex_source),
        }],
        ..SyntaxTree::default()
    };

    render_html(&tree, Layout::Wikijump)
}

fn render_iframe_with_attributes(attributes: AttributeMap) -> String {
    render_iframe_with_url_and_attributes("https://example.com/embed", attributes)
}

fn table_cell(text: &'static str) -> TableCell<'static> {
    TableCell {
        header: false,
        column_span: NonZeroU32::new(1).expect("one is non-zero"),
        align: None,
        attributes: AttributeMap::new(),
        elements: vec![Element::Text(Cow::Borrowed(text))],
    }
}

#[test]
fn color_markup_cannot_add_css_declarations() {
    for payload in [
        "red;background-image:url(https://attacker.invalid/pixel)",
        "red\";background-image:url(https://attacker.invalid/pixel)",
        "red';background-image:url(https://attacker.invalid/pixel)",
        "red&#59;background-image:url(https://attacker.invalid/pixel)",
    ] {
        let tree = parse(&format!("##{payload}|text##"), Layout::Wikijump);
        let output = render_html(&tree, Layout::Wikijump);

        assert!(output.contains(r#"style="color: inherit;""#));
        assert!(!output.contains("background-image"));
    }
}

#[test]
fn size_block_cannot_add_css_declarations() {
    for payload in [
        "80%;background-image:url(https://attacker.invalid/pixel)",
        "80%\";background-image:url(https://attacker.invalid/pixel)",
        "80%';background-image:url(https://attacker.invalid/pixel)",
        "80%&#59;background-image:url(https://attacker.invalid/pixel)",
    ] {
        let tree = parse(
            &format!("[[size {payload}]]text[[/size]]"),
            Layout::Wikijump,
        );
        let output = render_html(&tree, Layout::Wikijump);

        assert!(output.contains(r#"style="font-size: inherit;""#));
        assert!(!output.contains("background-image"));
    }

    let tree = parse("[[size 2vh]]text[[/size]]", Layout::Wikijump);
    let output = render_html(&tree, Layout::Wikijump);

    assert!(output.contains(r#"style="font-size: 2vh;""#));
}

#[test]
fn empty_label_triple_url_link_does_not_panic() {
    let tree = parse("[[[https://example.com|]]]", Layout::Wikidot);
    let output = render_html(&tree, Layout::Wikidot);

    assert!(output.contains(r#"href="https://example.com""#));
    assert!(output.contains(">https:example-com</a>"));
}

#[test]
fn protocol_relative_links_are_classified_as_external() {
    let tree = parse("[//attacker.invalid/path click]", Layout::Wikijump);
    let output = render_html_output(&tree, Layout::Wikijump);

    assert!(output.body.contains(r#"href="//attacker.invalid/path""#));
    assert!(output.body.contains("wj-link-external"));
    assert!(output.backlinks.internal_links.is_empty());
    assert_eq!(
        output
            .backlinks
            .external_links
            .iter()
            .map(Cow::as_ref)
            .collect::<Vec<_>>(),
        vec!["//attacker.invalid/path"],
    );

    let tree = parse("[/local-page local]", Layout::Wikijump);
    let output = render_html_output(&tree, Layout::Wikijump);

    assert!(output.body.contains("wj-link-internal"));
    assert!(output.backlinks.external_links.is_empty());
    assert_eq!(
        output.backlinks.internal_links,
        vec![PageRef::page_only("local-page")],
    );
}

#[test]
fn unsafe_triple_url_scheme_is_not_rendered_into_href() {
    for url in [
        "javascript:alert(1)",
        " JaVaScRiPt:alert(1)",
        "\tjavascript:alert(1)",
    ] {
        let tree = SyntaxTree {
            elements: vec![Element::Link {
                ltype: LinkType::Direct,
                link: LinkLocation::Url(Cow::Borrowed(url)),
                label: LinkLabel::Text(Cow::Borrowed("click")),
                target: None,
            }],
            ..SyntaxTree::default()
        };
        let output = render_html_output(&tree, Layout::Wikidot);

        assert!(!output.body.to_ascii_lowercase().contains("javascript:"));
        assert!(output.body.contains(r##"href="#invalid-url""##));
        assert!(output.backlinks.internal_links.is_empty());
        assert!(output.backlinks.external_links.is_empty());
    }

    {
        let url = "#frag";
        let tree = SyntaxTree {
            elements: vec![Element::Link {
                ltype: LinkType::Direct,
                link: LinkLocation::Url(Cow::Borrowed(url)),
                label: LinkLabel::Text(Cow::Borrowed("click")),
                target: None,
            }],
            ..SyntaxTree::default()
        };
        let output = render_html_output(&tree, Layout::Wikidot);

        assert!(output.body.contains(r##"href="#frag""##));
        assert!(output.backlinks.internal_links.is_empty());
        assert!(output.backlinks.external_links.is_empty());
    }
}

#[test]
fn mathml_text_payloads_are_escaped_without_disabling_mathml() {
    let output = render_math_inline_source(r"\text{<script>alert(1)</script> & <>}");
    let lower_output = output.to_ascii_lowercase();

    assert!(!lower_output.contains("<script"));
    assert!(output.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(output.contains("&amp;"));
    assert!(output.contains("&lt;&gt;"));
    assert!(output.contains("<wj-math-ml"));
    assert!(output.contains("<math "));
    assert!(output.contains("<mtext>"));
}

#[test]
fn mathml_operators_and_parse_error_text_are_escaped() {
    let output = render_math_inline_source(r"x < y & z");
    let lower_output = output.to_ascii_lowercase();

    assert!(!lower_output.contains("<script"));
    assert!(output.contains("<math "));
    assert!(output.contains("<mo>&lt;</mo>"));
    assert!(output.contains("&amp;"));

    let output = render_math_inline_source(r"2^{\pi - 1");

    assert!(!output.contains("<script"));
    assert!(output.contains("wj-error-inline") || output.contains("[PARSE ERROR:"));
}

#[test]
fn benign_mathml_rendering_still_outputs_math_elements() {
    let output = render_math_inline_source("x^2");

    assert!(output.contains("<wj-math-ml"));
    assert!(output.contains(
        r#"<math xmlns="http://www.w3.org/1998/Math/MathML" display="inline">"#
    ));
    assert!(output.contains("<msup><mi>x</mi><mn>2</mn></msup>"));
}

#[test]
fn unsafe_iframe_src_scheme_is_not_rendered() {
    for url in [
        "javascript:alert(1)",
        " JaVaScRiPt:alert(1)",
        "\tjavascript:alert(1)",
    ] {
        let output = render_iframe_with_url_and_attributes(url, AttributeMap::new());

        assert!(!output.to_ascii_lowercase().contains("javascript:"));
        assert!(output.contains(r##"src="#invalid-url""##));
    }
}

#[test]
fn raw_attribute_maps_are_sanitized_before_rendering() {
    let mut raw = BTreeMap::new();
    raw.insert(Cow::Borrowed("onclick"), Cow::Borrowed("alert(1)"));
    raw.insert(Cow::Borrowed("href"), Cow::Borrowed(" JaVaScRiPt:alert(1)"));
    let from_raw_attributes = AttributeMap::from(raw);

    assert!(!from_raw_attributes.get().contains_key("onclick"));
    assert_eq!(
        from_raw_attributes.get().get("href").map(Cow::as_ref),
        Some("#invalid-url"),
    );

    let from_raw_map = render_iframe_with_attributes(from_raw_attributes);

    assert!(!from_raw_map.contains("onclick"));
    assert!(!from_raw_map.contains("javascript:"));
    assert!(from_raw_map.contains(r##"href="#invalid-url""##));

    let mut inserted = AttributeMap::new();
    assert!(!inserted.insert("onclick", Cow::Borrowed("alert(1)")));
    assert!(inserted.insert("href", Cow::Borrowed(" JaVaScRiPt:alert(1)")));
    assert!(!inserted.get().contains_key("onclick"));
    assert_eq!(
        inserted.get().get("href").map(Cow::as_ref),
        Some("#invalid-url")
    );

    let from_insert = render_iframe_with_attributes(inserted);

    assert!(!from_insert.contains("onclick"));
    assert!(!from_insert.contains("javascript:"));
    assert!(from_insert.contains(r##"href="#invalid-url""##));

    let deserialized =
        serde_json::from_str(r#"{"onclick":"alert(1)","href":"\tjavascript:alert(1)"}"#)
            .expect("attribute map should deserialize");
    let deserialized: AttributeMap = deserialized;

    assert!(!deserialized.get().contains_key("onclick"));
    assert_eq!(
        deserialized.get().get("href").map(Cow::as_ref),
        Some("#invalid-url"),
    );

    let from_deserialize = render_iframe_with_attributes(deserialized);

    assert!(!from_deserialize.contains("onclick"));
    assert!(!from_deserialize.contains("javascript:"));
    assert!(from_deserialize.contains(r##"href="#invalid-url""##));

    let mut local_anchor = AttributeMap::new();
    assert!(local_anchor.insert("usemap", Cow::Borrowed("not-an-anchor")));
    assert_eq!(
        local_anchor.get().get("usemap").map(Cow::as_ref),
        Some("#invalid-url"),
    );
}

#[test]
fn mismatched_link_labels_render_fallback_text() {
    let tree = SyntaxTree {
        elements: vec![
            Element::Link {
                ltype: LinkType::Direct,
                link: LinkLocation::Page(PageRef::page_only("target-page")),
                label: LinkLabel::Url,
                target: None,
            },
            Element::Text(Cow::Borrowed(" ")),
            Element::Link {
                ltype: LinkType::Direct,
                link: LinkLocation::Url(Cow::Borrowed("https://example.com")),
                label: LinkLabel::Page,
                target: None,
            },
        ],
        ..SyntaxTree::default()
    };
    let html = render_html(&tree, Layout::Wikijump);
    let text = render_text(&tree, Layout::Wikijump);

    assert!(html.contains(">target-page</a>"));
    assert!(html.contains(">https://example.com</a>"));
    assert_eq!(text, "target-page https://example.com");
}

#[test]
fn malformed_ast_footnote_reference_renders_error() {
    let tree = SyntaxTree {
        elements: vec![Element::Footnote(1)],
        footnotes: vec![],
        ..SyntaxTree::default()
    };
    let output = render_html(&tree, Layout::Wikijump);

    assert!(output.contains("wj-error-inline"));
    assert!(output.contains("Footnote item not found"));
}

#[test]
fn recursive_ast_footnote_reference_fails_closed() {
    let tree = SyntaxTree {
        elements: vec![Element::Footnote(1)],
        footnotes: vec![vec![Element::Footnote(1)]],
        ..SyntaxTree::default()
    };
    let output = render_html(&tree, Layout::Wikijump);

    assert!(output.contains("wj-error-inline"), "{output}");
    assert!(output.contains("Footnote item not found"), "{output}");
    assert!(output.len() < 20_000, "recursive output grew unexpectedly");
}

#[test]
fn residual_gallery_inside_no_paragraph_body_does_not_leak_paragraph_state() {
    let input = concat!(
        "[[bibliography]]\n",
        ": item : before\n",
        "[[gallery]]]\n",
        "[[/bibliography]]\n",
        "after",
    );
    let result = std::panic::catch_unwind(|| parse_with_errors(input, Layout::Wikidot));

    assert!(
        result.is_ok(),
        "residual gallery must not panic outer paragraph gathering"
    );
}

#[test]
fn spaced_css_module_closer_does_not_panic() {
    let result = std::panic::catch_unwind(|| {
        parse_with_errors("[[module CSS]]\n[[/ module]]", Layout::Wikidot)
    });

    assert!(
        result.is_ok(),
        "spaced CSS closer must fail/recover without panic"
    );
}

#[test]
fn standalone_buttons_respect_page_syntax_mode_gate() {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::ForumPost, Layout::Wikidot);
    let mut source = "[[button edit text=\"Edit\"]]".to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokens = ftml::tokenize(&source);
    let (tree, _errors) = ftml::parse(&tokens, &page_info, &settings).into();
    let output = HtmlRender.render(&tree, &page_info, &settings);

    assert!(
        !output.body.contains("wiki-standalone-button"),
        "{}",
        output.body
    );
    assert!(
        output.resource_requirements.is_empty(),
        "{:#?}",
        output.resource_requirements
    );
}

#[test]
fn plain_failed_inline_div_keeps_its_body_literal() {
    let input = "[[div]][[footnote]]secret[[/footnote]][[/div]]";
    let (tree, _errors) = parse_with_errors(input, Layout::Wikidot);
    let html = render_html(&tree, Layout::Wikidot);

    assert!(tree.footnotes.is_empty(), "{tree:#?}");
    assert!(!html.contains("footnotes-footer"), "{html}");
    assert!(html.contains("[[div]]"), "{html}");
    assert!(html.contains("[[footnote]]secret[[/footnote]]"), "{html}");
    assert!(html.contains("[[/div]]"), "{html}");
}

#[test]
fn unsafe_wikidot_image_source_never_reaches_src_attribute() {
    let tree = parse("[[image javascript://alert(1)]]", Layout::Wikidot);
    let html = render_html(&tree, Layout::Wikidot);

    assert!(!html.contains("src=\"javascript:"), "{html}");
    assert!(html.contains("src=\"#invalid-url\""), "{html}");
}

#[test]
fn backslash_wikidot_image_link_stays_internal() {
    let input = r#"[[image https://example.com/a.png link="\\evil.example/path"]]"#;
    let tree = parse(input, Layout::Wikidot);
    let html = render_html(&tree, Layout::Wikidot);

    assert!(!html.contains("href=\"/\\evil.example"), "{html}");
    assert!(html.contains("%5Cevil.example"), "{html}");
}

#[test]
fn false_iftags_two_sibling_closes_do_not_poison_close_cache() {
    let input = concat!(
        "[[iftags +missing]]\n",
        "[[iftags -missing]]\nfirst\n[[/iftags]]\n",
        "[[iftags -missing]]\nsecond\n[[/iftags]]\n",
        "[[html]]\n<b>secret</b>\n[[/html]]\n",
        "[[/iftags]]\n",
        "visible",
    );
    let tree = parse(input, Layout::Wikidot);

    assert_eq!(render_text(&tree, Layout::Wikidot), "visible");
    assert!(tree.html_blocks.is_empty(), "{tree:#?}");
}

#[test]
fn scored_inline_div_at_document_start_remains_literal() {
    let (tree, _errors) = parse_with_errors("[[div_]]inline[[/div]]", Layout::Wikidot);
    let html = render_html(&tree, Layout::Wikidot);

    assert!(html.contains("[[div_]]"), "{html}");
    assert!(html.contains("[[/div]]"), "{html}");
    assert!(!html.contains("<div"), "{html}");
}

#[test]
fn malformed_ast_bibliography_block_renders_error() {
    let tree = SyntaxTree {
        elements: vec![Element::BibliographyBlock {
            index: 0,
            title: None,
            hide: false,
        }],
        ..SyntaxTree::default()
    };
    let output = render_html(&tree, Layout::Wikijump);

    assert!(output.contains("wj-error-inline"));
    assert!(output.contains("Bibliography block not found"));
}

#[test]
fn recursive_bibcite_tooltips_render_error_instead_of_recursing() {
    for input in [
        "[[bibliography]]\n: a : ((bibcite a))\n[[/bibliography]]\n((bibcite a))",
        "[[bibliography]]\n: a : ((bibcite b))\n: b : ((bibcite a))\n[[/bibliography]]\n((bibcite a))",
    ] {
        let tree = parse(input, Layout::Wikijump);
        let output = render_html(&tree, Layout::Wikijump);

        assert!(output.contains("wj-error-inline"));
        assert!(output.contains("Bibliography item not found"));
        assert!(output.len() < 20_000);
    }
}

#[test]
fn non_bibcite_parentheses_render_as_text() {
    let tree = parse("before ((notbibcite label)) after", Layout::Wikijump);
    let output = render_html(&tree, Layout::Wikijump);

    assert!(output.contains("before"));
    assert!(output.contains("notbibcite"));
    assert!(output.contains("after"));
}

#[test]
fn malformed_partial_elements_render_fallback_html() {
    let tree = SyntaxTree {
        elements: vec![
            Element::Partial(PartialElement::ListItem(ListItem::Elements {
                attributes: AttributeMap::new(),
                elements: vec![Element::Text(Cow::Borrowed("partial"))],
            })),
            Element::Partial(PartialElement::ListItem(ListItem::SubList {
                element: Box::new(Element::Text(Cow::Borrowed("sublist"))),
            })),
            Element::Partial(PartialElement::TableRow(TableRow {
                attributes: AttributeMap::new(),
                cells: vec![table_cell("row-cell")],
            })),
            Element::Partial(PartialElement::TableCell(table_cell("cell"))),
            Element::Partial(PartialElement::Tab(Tab {
                label: Cow::Borrowed("tab-label"),
                elements: vec![Element::Text(Cow::Borrowed("tab-body"))],
            })),
            Element::Partial(PartialElement::RubyText(RubyText {
                attributes: AttributeMap::new(),
                elements: vec![Element::Text(Cow::Borrowed("ruby"))],
            })),
        ],
        ..SyntaxTree::default()
    };
    let output = render_html(&tree, Layout::Wikijump);

    for expected in [
        "partial",
        "sublist",
        "row-cell",
        "cell",
        "tab-label",
        "tab-body",
        "ruby",
    ] {
        assert!(
            output.contains(expected),
            "{expected} missing from {output}"
        );
    }
    assert!(output.contains("tab-label</span> tab-body"));
}

#[test]
fn malformed_partial_elements_render_fallback_text() {
    let cases = [
        (
            Element::Partial(PartialElement::ListItem(ListItem::Elements {
                attributes: AttributeMap::new(),
                elements: vec![],
            })),
            "",
        ),
        (
            Element::Partial(PartialElement::ListItem(ListItem::SubList {
                element: Box::new(Element::Text(Cow::Borrowed("sublist"))),
            })),
            "sublist",
        ),
        (
            Element::Partial(PartialElement::TableRow(TableRow {
                attributes: AttributeMap::new(),
                cells: vec![table_cell("row-cell")],
            })),
            "row-cell",
        ),
        (
            Element::Partial(PartialElement::TableCell(table_cell("cell"))),
            "cell",
        ),
        (
            Element::Partial(PartialElement::Tab(Tab {
                label: Cow::Borrowed("tab-label"),
                elements: vec![Element::Text(Cow::Borrowed("tab-body"))],
            })),
            "tab-label\ntab-body",
        ),
        (
            Element::Partial(PartialElement::RubyText(RubyText {
                attributes: AttributeMap::new(),
                elements: vec![Element::Text(Cow::Borrowed("ruby"))],
            })),
            "(ruby)",
        ),
    ];

    for (element, expected) in cases {
        let tree = SyntaxTree {
            elements: vec![element],
            ..SyntaxTree::default()
        };

        assert_eq!(render_text(&tree, Layout::Wikijump), expected);
    }

    for (element, expected) in [
        (
            Element::Partial(PartialElement::ListItem(ListItem::Elements {
                attributes: AttributeMap::new(),
                elements: vec![Element::Text(Cow::Borrowed("partial"))],
            })),
            "prefix\npartial",
        ),
        (
            Element::Partial(PartialElement::TableRow(TableRow {
                attributes: AttributeMap::new(),
                cells: vec![table_cell("row-cell")],
            })),
            "prefix\nrow-cell",
        ),
        (
            Element::Partial(PartialElement::Tab(Tab {
                label: Cow::Borrowed("tab-label"),
                elements: vec![Element::Text(Cow::Borrowed("tab-body"))],
            })),
            "prefix\ntab-label\ntab-body",
        ),
    ] {
        let tree = SyntaxTree {
            elements: vec![Element::Text(Cow::Borrowed("prefix")), element],
            ..SyntaxTree::default()
        };

        assert_eq!(render_text(&tree, Layout::Wikijump), expected);
    }
}

#[test]
fn empty_page_includes_are_rejected() {
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let (output, pages) = ftml::include(
        "[[include :scp-wiki:]]",
        &settings,
        ftml::includes::NullIncluder,
        || -> Infallible { unreachable!("no include request should be emitted") },
    )
    .expect("malformed include should be ignored, not treated as an include request");

    assert_eq!(output, "[[include :scp-wiki:]]");
    assert!(pages.is_empty());

    let (_tree, errors) =
        parse_with_errors("[[include-elements :scp-wiki:]]", Layout::Wikijump);
    assert!(
        errors
            .iter()
            .any(|error| error.kind() == ParseErrorKind::BlockMalformedArguments),
        "{errors:?}",
    );
}

#[test]
fn malformed_include_prefixes_are_skipped_once() {
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let input = "[[include :scp-wiki:]]\n[[include component:ok]]";
    let (output, pages) =
        ftml::include(input, &settings, DebugIncluder, || -> Infallible {
            unreachable!("debug includer should match include requests")
        })
        .expect("include preprocessing should not fail");

    assert!(output.contains("[[include :scp-wiki:]]"));
    assert!(output.contains("<INCLUDED-PAGE component:ok {}>"));
    assert_eq!(pages, vec![PageRef::page_only("component:ok")]);

    let repeated = "[[include page\n".repeat(6_400);
    let started = Instant::now();
    let (output, pages) = ftml::include(
        &repeated,
        &settings,
        ftml::includes::NullIncluder,
        || -> Infallible { unreachable!("null includer should not fail") },
    )
    .expect("malformed includes should be ignored");

    assert_eq!(output, repeated);
    assert!(pages.is_empty());
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn empty_list_lines_do_not_fallback_quadratically() {
    let input = "* \n".repeat(1_000);
    let (tree, errors) = parse_with_errors(&input, Layout::Wikijump);

    assert!(errors.is_empty(), "{errors:?}");
    assert!(tree.elements.is_empty(), "{:?}", tree.elements);
}

#[test]
fn over_limit_date_formats_fall_back_without_format_class() {
    let format = "%c".repeat(65);
    let input = format!(r#"[[date 2010-01-01 format="{format}"]]"#);
    let tree = parse(&input, Layout::Wikijump);
    let output = render_html_output(&tree, Layout::Wikijump);

    assert!(!output.body.contains("format_"));
    assert!(!output.body.contains("%25c"));
    assert!(output.body.contains("wj-date"));
}

#[test]
fn hidden_conditionals_do_not_publish_metadata_blocks() {
    for layout in [Layout::Wikijump, Layout::Wikidot] {
        let tree = parse(
            "[[iftags +missing]]\n+ Secret heading\n[[footnote]]secret[[/footnote]]\n[[bibliography]]\n: secret : reference\n[[/bibliography]]\n[[code]]\nsecret\n[[/code]]\n[[html]]\n<b>secret</b>\n[[/html]]\n[[/iftags]]",
            layout,
        );

        assert!(
            tree.elements.is_empty()
                && tree.table_of_contents.is_empty()
                && tree.code_blocks.is_empty()
                && tree.html_blocks.is_empty()
                && tree.footnotes.is_empty()
                && !tree.needs_footnote_block
                && tree.bibliographies.is_empty(),
            "{layout:?}: {tree:?}",
        );
    }

    let ifcategory = "[[ifcategory missing]]\n+ Secret heading\n[[footnote]]secret[[/footnote]]\n[[bibliography]]\n: secret : reference\n[[/bibliography]]\n[[code]]\nsecret\n[[/code]]\n[[html]]\n<b>secret</b>\n[[/html]]\n[[/ifcategory]]";
    let wikijump = parse(ifcategory, Layout::Wikijump);
    assert!(wikijump.elements.is_empty(), "{wikijump:?}");
    assert!(wikijump.code_blocks.is_empty(), "{wikijump:?}");
    assert!(wikijump.html_blocks.is_empty(), "{wikijump:?}");

    let (wikidot, literal_errors) = parse_with_errors(ifcategory, Layout::Wikidot);
    assert!(!literal_errors.is_empty());
    assert!(!wikidot.elements.is_empty(), "{wikidot:?}");
    assert_eq!(wikidot.code_blocks.len(), 1, "{wikidot:?}");
    assert_eq!(wikidot.html_blocks, ["\n<b>secret</b>\n"], "{wikidot:?}");
    assert_eq!(wikidot.footnotes.len(), 1, "{wikidot:?}");
    assert!(!wikidot.bibliographies.is_empty(), "{wikidot:?}");
    let wikidot_html = render_html(&wikidot, Layout::Wikidot);
    assert!(
        wikidot_html.contains(
            r#"<iframe src="https://example.com/" allowtransparency="true" frameborder="0" class="html-block-iframe"></iframe>"#,
        ),
        "{wikidot_html}",
    );
    assert!(
        wikidot_html.contains("Secret heading<sup class=\"footnoteref\""),
        "{wikidot_html}",
    );

    let tree = parse(
        "[[ifcategory _default]]\n[[code]]\nvisible\n[[/code]]\n[[/ifcategory]]",
        Layout::Wikijump,
    );

    assert_eq!(tree.code_blocks.len(), 1);
    assert_eq!(tree.code_blocks[0].contents, "visible");
}

#[test]
fn disabled_html_blocks_remain_opaque_inside_false_wikidot_iftags() {
    let input = concat!(
        "[[iftags +missing]]\n",
        "[[html]]\n",
        "[[/iftags]]\n",
        "SECRET\n",
        "[[/html]]\n",
        "[[/iftags]]\n",
        "public",
    );
    let page_info = page_info();
    let mut settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    settings.enable_html_blocks = false;
    let tokenization = ftml::tokenize(input);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let text = TextRender.render(&tree, &page_info, &settings);

    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(text, "public");
    assert!(tree.html_blocks.is_empty(), "{tree:#?}");
    assert!(!format!("{tree:?}").contains("SECRET"), "{tree:#?}");
}

#[test]
fn false_iftags_raw_body_end_markers_stay_hidden() {
    let raw_bodies = [
        "[[code]]\n[[/iftags]]\n[[html]]\n<b>raw-code</b>\n[[/html]]\n[[/code]]",
        "[[html]]\n[[/iftags]]\n<b>raw-html</b>\n[[/html]]",
        "[[raw]]\n[[/iftags]]\n[[html]]raw-raw[[/html]]\n[[/raw]]",
        "[[math]]\n\\text{[[/iftags]] raw-math}\n[[/math]]",
        "[[module CSS]]\n/* [[/iftags]] raw-module-css */\n[[/module]]",
        "[[module ListPages]]\n[[/iftags]] raw-module-list-pages\n[[/module]]",
        "@@[[/iftags]] raw-inline@@",
        "[!-- [[/iftags]] raw-comment --]",
        "[[math]]\n[[/math]]",
        "[[module Rate]]",
    ];

    for layout in [Layout::Wikijump, Layout::Wikidot] {
        for raw_body in raw_bodies {
            if layout == Layout::Wikidot
                && (raw_body.starts_with("[[raw]]")
                    || raw_body.starts_with("[[math]]\n\\text{")
                    || raw_body.starts_with("[!-- "))
            {
                let input = format!(
                    "[[iftags +missing]]\n{raw_body}\n[[html]]\n<b>guarded</b>\n[[/html]]\n[[/iftags]]\nvisible",
                );
                let tree = parse(&input, layout);
                assert!(tree.elements.is_empty(), "{raw_body}: {tree:#?}");
                assert!(tree.html_blocks.is_empty(), "{raw_body}: {tree:#?}");
                assert!(tree.code_blocks.is_empty(), "{raw_body}: {tree:#?}");
                continue;
            }
            assert_false_iftags_guarded(raw_body, layout);
        }
    }

    let (wikidot_math, _) = parse_with_errors(
        concat!(
            "[[iftags +missing]]\n",
            "[[math]]\n",
            "\\text{[[/iftags]] raw-math}\n",
            "[[/math]]\n",
            "[[html]]\n",
            "<b>guarded</b>\n",
            "[[/html]]\n",
            "[[/iftags]]\n",
            "visible",
        ),
        Layout::Wikidot,
    );
    assert!(wikidot_math.elements.is_empty(), "{wikidot_math:#?}");
    assert!(wikidot_math.html_blocks.is_empty(), "{wikidot_math:#?}");
    assert!(wikidot_math.code_blocks.is_empty(), "{wikidot_math:#?}");
}

#[test]
fn false_iftags_nested_conditionals_do_not_close_the_outer_body() {
    let nested = [
        "[[iftags +missing]]\ninner false\n[[/iftags]]",
        "[[iftags -missing]]\ninner true\n[[/iftags]]",
        "[[ifcategory missing]]\ninner category\n[[/ifcategory]]",
        "[[ifcategory missing]]\n[[code]]\n[[/ifcategory]]\n[[/iftags]]\nraw nested conditional\n[[/code]]\n[[/ifcategory]]",
    ];

    for layout in [Layout::Wikijump, Layout::Wikidot] {
        for conditional in nested {
            assert_false_iftags_guarded(conditional, layout);
        }
    }
}

#[test]
fn false_iftags_split_line_image_head_does_not_close_the_page() {
    for layout in [Layout::Wikijump, Layout::Wikidot] {
        assert_false_iftags_guarded(
            "[[image http://example.com/x.png\n link=#]]",
            layout,
        );
    }
}

#[test]
fn false_iftags_nameless_embed_head_does_not_close_the_page() {
    for layout in [Layout::Wikijump, Layout::Wikidot] {
        assert_false_iftags_guarded("[[embed]]\n[[/embed]]", layout);
    }
}

#[test]
fn false_iftags_malformed_hidden_structures_fail_closed() {
    let malformed = [
        "[[code]]\n[[/iftags]]\nunclosed code",
        "[[html]]\n[[/iftags]]\n<b>unclosed html</b>",
        "[[raw]]\n[[/iftags]]\nunclosed raw",
        "[[math]]\n[[/iftags]]\nunclosed math",
        "[[module CSS]]\n[[/iftags]]\n.unclosed { color: red; }",
        "[[module ListPages]]\n[[/iftags]]\nunclosed module",
        "[[code @=\"bad\"]]\n[[/iftags]]\n[[html]]\n<b>malformed head</b>\n[[/html]]",
        "[[iftags +missing]]\ninner unclosed\n[[/iftags]]\n[[html]]\n<b>unclosed nested</b>\n[[/html]]",
    ];

    for hidden in malformed {
        assert_false_iftags_fails_closed(hidden, Layout::Wikijump);
    }
}

#[test]
fn wikidot_false_iftags_unclosed_raw_child_fails_closed() {
    for hidden in [
        "[[code]]\n[[/iftags]]\nunclosed code",
        "[[html]]\n[[/iftags]]\n<b>unclosed html</b>",
        "[[raw]]\n[[/iftags]]\nunclosed raw",
    ] {
        let tree = parse(&format!("[[iftags +missing]]\n{hidden}"), Layout::Wikidot);

        assert!(tree.elements.is_empty(), "{hidden}: {tree:#?}");
        assert!(tree.html_blocks.is_empty(), "{hidden}: {tree:#?}");
        assert!(tree.code_blocks.is_empty(), "{hidden}: {tree:#?}");
    }
}

#[test]
fn wikidot_false_iftags_closed_raw_and_comment_fail_closed() {
    for hidden in [
        "[[raw]]\n[[/iftags]]\n[[html]]raw-raw[[/html]]\n[[/raw]]",
        "[!-- [[/iftags]] raw-comment --]",
    ] {
        let input = format!(
            "[[iftags +missing]]\n{hidden}\n[[html]]\n<b>guarded</b>\n[[/html]]\n[[/iftags]]\nvisible",
        );
        let (tree, _) = parse_with_errors(&input, Layout::Wikidot);
        assert!(tree.elements.is_empty(), "{hidden}: {tree:#?}");
        assert!(tree.html_blocks.is_empty(), "{hidden}: {tree:#?}");
        assert!(tree.code_blocks.is_empty(), "{hidden}: {tree:#?}");
    }
}

#[test]
fn wikidot_false_iftags_single_nested_close_ends_the_outer_gate() {
    let input = concat!(
        "[[iftags +missing]]\n",
        "[[iftags +missing]]\n",
        "inner unclosed\n",
        "[[/iftags]]\n",
        "[[html]]\n",
        "<b>unclosed nested</b>\n",
        "[[/html]]",
    );
    let (tree, _) = parse_with_errors(input, Layout::Wikidot);

    assert_eq!(render_text(&tree, Layout::Wikidot), "");
    assert_eq!(tree.html_blocks, ["\n<b>unclosed nested</b>\n"]);
}

#[test]
fn false_iftags_unclosed_parsed_child_stops_at_outer_boundary() {
    let input = "[[iftags +missing]]\n[[div]]\nhidden child\n[[/iftags_]]\nvisible";

    let tree = parse(input, Layout::Wikijump);
    let output = render_html(&tree, Layout::Wikijump);

    assert!(output.contains("visible"), "{output}");
    assert!(!output.contains("hidden child"), "{output}");
}

#[test]
fn false_iftags_malformed_paragraph_child_respects_parent_boundaries() {
    let cases = [
        concat!(
            "[[iftags +missing]]\n",
            "[[div]]\n",
            "[[collapsible]]\n",
            "hidden child\n",
            "[[/div]]\n",
            "[[/iftags]]\n",
            "visible",
        ),
        concat!(
            "[[iftags +missing]]\n",
            "[[div]]\n",
            "[[collapsible]]\n",
            "hidden child\n",
            "[[/iftags_]]\n",
            "visible",
        ),
    ];

    for input in cases {
        let tree = parse(input, Layout::Wikijump);
        let output = render_text(&tree, Layout::Wikijump);

        assert_eq!(output, "visible", "{tree:?}");
    }
}

#[test]
fn false_iftags_discard_restores_quoted_block_cursor() {
    let input = concat!(
        "> [[collapsible show=\"show\" hide=\"hide\"]]\n",
        "> [[iftags +missing]]\n",
        "> [[div]]\n",
        "> hidden child\n",
        "> [[/iftags]]\n",
        "> visible inside\n",
        "> [[/collapsible]]\n",
        "outside\n",
    );

    for layout in [Layout::Wikijump, Layout::Wikidot] {
        let tree = parse(input, layout);
        let text = render_text(&tree, layout);

        assert!(!text.contains("hidden child"), "{layout:?}: {text}");
        assert!(text.contains("visible inside"), "{layout:?}: {text}");
        assert!(text.contains("outside"), "{layout:?}: {text}");
    }
}

#[test]
fn false_iftags_closed_unknown_module_does_not_hide_following_content() {
    let input = concat!(
        "[[iftags +missing]]\n",
        "[[module Unknown]]\n",
        "hidden unsupported module\n",
        "[[/module]]\n",
        "[[/iftags]]\n",
        "visible",
    );

    for layout in [Layout::Wikijump, Layout::Wikidot] {
        let tree = parse(input, layout);

        assert_eq!(render_text(&tree, layout), "visible", "{layout:?}");
    }
}

#[test]
fn false_iftags_deep_nested_conditionals_parse_within_budget() {
    const DEPTH: usize = 64;

    let mut input = String::from("[[iftags +missing]]\n");
    input.push_str(&"[[iftags +missing]]\n".repeat(DEPTH));
    input.push_str("hidden\n");
    input.push_str(&"[[/iftags]]\n".repeat(DEPTH));
    input.push_str("[[/iftags]]\nvisible");

    let started = Instant::now();
    let tree = parse(&input, Layout::Wikidot);
    let elapsed = started.elapsed();
    let output = render_html(&tree, Layout::Wikidot);

    assert!(output.contains("visible"), "{output}");
    assert!(!output.contains("hidden"), "{output}");
    assert!(elapsed < Duration::from_secs(3), "parse took {elapsed:?}");
}

#[test]
fn wikidot_false_iftags_sparse_closer_matrix_preserves_public_behavior() {
    for closer_count in 0..=3 {
        let input = format!(
            "[[iftags +missing]]\n{}hidden\n{}visible",
            "[[iftags +missing]]\n".repeat(3),
            "[[/iftags]]\n".repeat(closer_count),
        );
        let (tree, _) = parse_with_errors(&input, Layout::Wikidot);
        let text = render_text(&tree, Layout::Wikidot);

        match closer_count {
            0 => {
                assert!(text.contains("[[iftags +missing]]"), "{text}");
                assert!(text.contains("hidden"), "{text}");
                assert!(text.contains("visible"), "{text}");
            }
            1 => assert_eq!(text, "visible"),
            2 | 3 => assert!(text.is_empty(), "{closer_count}: {text}"),
            _ => unreachable!(),
        }
    }
}

#[test]
fn wikidot_false_iftags_sparse_closer_scans_scale_linearly() {
    for nested_count in [64, 128, 256, 512] {
        let input = format!(
            "[[iftags +missing]]\n{}hidden\n[[/iftags]]\nvisible",
            "[[iftags +missing]]\n".repeat(nested_count),
        );
        let started = Instant::now();
        let (tree, _) = parse_with_errors(&input, Layout::Wikidot);
        let elapsed = started.elapsed();

        assert_eq!(render_text(&tree, Layout::Wikidot), "visible");
        assert!(
            elapsed < Duration::from_secs(3),
            "{nested_count} nested candidates took {elapsed:?}",
        );
    }
}

#[test]
fn hostile_wikidot_lookahead_families_remain_bounded() {
    let parse_cases = [
        (
            "multiline set-tags button",
            format!("[[button set-tags{}", "\n    ".repeat(4_096)),
        ),
        ("spaced unmatched closes", "[[/ a ".repeat(4_096)),
        (
            "triple-link raw probes",
            format!("[[[start|{}]]]", "@<a".repeat(4_096)),
        ),
        (
            "repeated malformed footnoteblocks",
            "[[footnoteblock x=\"bad\"]]".repeat(2_048),
        ),
        (
            "newline malformed single links",
            "[https://example.com label\n".repeat(2_048),
        ),
        (
            "single-link owner recovery",
            format!("{}@@x@@]", "[https://example.com ".repeat(1_024)),
        ),
        ("same-line line-owned blocks", "x[[toc]]".repeat(4_096)),
        ("unterminated code candidates", "[[code]]\n".repeat(2_048)),
    ];

    for (name, input) in parse_cases {
        let started = Instant::now();
        let _ = parse_with_errors(&input, Layout::Wikidot);
        let elapsed = started.elapsed();
        assert!(elapsed < Duration::from_secs(3), "{name} took {elapsed:?}");
    }
}

#[test]
fn hostile_wikidot_preprocessing_and_tokenization_remain_bounded() {
    let mut angle_run = format!("x{}", ">".repeat(100_000));
    let started = Instant::now();
    ftml::preprocess_for_layout(&mut angle_run, Layout::Wikidot);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "right-angle quote preprocessing exceeded budget"
    );

    let mut math_closers = format!("<<x>>\n[[math]]\n{}", "[[/math\n".repeat(4_096));
    let started = Instant::now();
    ftml::preprocess_for_layout(&mut math_closers, Layout::Wikidot);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "malformed math closer preprocessing exceeded budget"
    );

    let brackets = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    let started = Instant::now();
    let _ = ftml::tokenize(&brackets);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "bracket-run tokenization exceeded budget"
    );
}

#[test]
fn false_iftags_unclosed_outer_body_keeps_layout_specific_semantics() {
    let input = "[[iftags +missing]]\nfollowing body";

    let (wikidot, literal_errors) = parse_with_errors(input, Layout::Wikidot);
    assert!(!literal_errors.is_empty());
    assert!(
        render_text(&wikidot, Layout::Wikidot).contains("[[iftags +missing]]")
            && render_text(&wikidot, Layout::Wikidot).contains("following body"),
        "{:?}",
        wikidot.elements
    );

    let wikijump = parse(input, Layout::Wikijump);
    assert!(wikijump.elements.is_empty(), "{:?}", wikijump.elements);
}

#[test]
fn deeply_repeated_headings_parse_without_stack_recursion() {
    const HEADING_COUNT: usize = 4_096;

    let input = (0..HEADING_COUNT)
        .map(|index| format!("+ heading {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let tree = parse(&input, Layout::Wikijump);

    assert_eq!(tree.elements.len(), HEADING_COUNT);
    assert!(tree.elements.iter().all(|element| {
        matches!(
            element,
            Element::Container(container)
                if matches!(container.ctype(), ContainerType::Header(_))
        )
    }));
}

#[test]
fn html_render_does_not_retain_comment_sized_body_capacity() {
    let comment_body = "x".repeat(2 * 1024 * 1024);
    let input = format!("[!--{comment_body}--]");
    let tree = parse(&input, Layout::Wikijump);

    let output = render_html_output(&tree, Layout::Wikijump);

    assert!(output.body.is_empty());
    assert!(
        output.body.capacity() < input.len() / 16,
        "body capacity {} should not scale with comment-heavy input length {}",
        output.body.capacity(),
        input.len(),
    );
}

#[test]
fn decoded_text_entities_cannot_inject_markup() {
    let input = "&lt;script&gt;alert(1)&lt;/script&gt; &#60;b&#x3E; &quot;&#39;&amp;lt; \
                 &#91;&#91;div&#93;&#93; &#42;&#42;x&#42;&#42;";
    let tree = parse(input, Layout::Wikijump);

    assert_eq!(
        render_html(&tree, Layout::Wikijump),
        "<p>&lt;script&gt;alert(1)&lt;/script&gt; &lt;b&gt; &quot;&#39;&amp;lt; \
         [[div]] **x**</p>",
    );
    assert_eq!(
        render_text(&tree, Layout::Wikijump),
        "<script>alert(1)</script> <b> \"'&lt; [[div]] **x**",
    );
}
