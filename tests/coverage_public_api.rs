use ftml::data::{PageInfo, PageRef, ScoreValue};
use ftml::includes::IncludeRef;
use ftml::layout::Layout;
use ftml::render::Render;
use ftml::render::html::HtmlRender;
use ftml::render::text::TextRender;
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::{
    Alignment, AttributeMap, Bibliography, BibliographyList, Element, FileSource,
    ImageSource, ListItem, ListType, PartialElement, RubyText, SyntaxTree, Tab, Table,
};
use std::borrow::Cow;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("coverage-page"),
        category: Some(Cow::Borrowed("test")),
        site: Cow::Borrowed("coverage"),
        title: Cow::Borrowed("Coverage Page"),
        alt_title: None,
        score: ScoreValue::Integer(7),
        tags: vec![Cow::Borrowed("test")],
        language: Cow::Borrowed("default"),
    }
}

fn parse_and_render_text(input: &str) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let mut text = input.to_owned();
    ftml::preprocess(&mut text);
    let tokens = ftml::tokenize(&text);
    let result = ftml::parse(&tokens, &page_info, &settings);
    let (tree, errors) = result.into();

    assert!(errors.is_empty(), "{errors:?}");
    TextRender.render(&tree, &page_info, &settings)
}

fn first_table<'a, 't>(elements: &'a [Element<'t>]) -> Option<&'a Table<'t>> {
    for element in elements {
        match element {
            Element::Table(table) => return Some(table),
            Element::Container(container) => {
                if let Some(table) = first_table(container.elements()) {
                    return Some(table);
                }
            }
            _ => {}
        }
    }

    None
}

#[test]
fn public_api_text_render_covers_mixed_block_elements() {
    let output = parse_and_render_text(
        r#"[[collapsible show="Show" hide="Hide"]]
Visible text
[[/collapsible]]

[[code type="rust"]]
fn main() {}
[[/code]]

[[module Rate]]

||~ Head || Cell ||
|| A || B ||

##abc|Tinted text##

= centered

* one
* two
"#,
    );

    assert!(output.contains("Visible text"));
    assert!(output.contains("fn main() {}"));
    assert!(output.contains("HeadCell"));
    assert!(output.contains("Tinted text"));
    assert!(output.contains("centered"));
    assert!(output.contains("one"));
    assert!(output.contains("two"));
}

#[test]
fn public_api_text_render_covers_anchor_link_labels() {
    let output = parse_and_render_text("[#section Jump]\n[# Fake]");

    assert!(output.contains("Jump"));
    assert!(output.contains("Fake"));
}

#[test]
fn public_api_text_render_partial_trims_outer_newlines() {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let elements = [Element::Text(Cow::Borrowed("\nPartial body\n"))];
    let output = TextRender.render_partial(&elements, &page_info, &settings, 14);

    assert_eq!(output, "Partial body");
}

#[test]
fn public_api_html_render_tracks_metadata_backlinks_and_indices() {
    let mut page_info = page_info();
    page_info.alt_title = Some(Cow::Borrowed("Alt Coverage"));
    page_info.tags.push(Cow::Borrowed("html"));
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let mut text = String::from(
        r#"[[toc]]

+ Heading

[[[target-page]]]
[/local-page Local]
[https://example.com External]
[# Fake]
Text body
"#,
    );
    ftml::preprocess(&mut text);
    let tokens = ftml::tokenize(&text);
    let result = ftml::parse(&tokens, &page_info, &settings);
    let (tree, errors) = result.into();

    assert!(errors.is_empty(), "{errors:?}");
    let output = HtmlRender.render(&tree, &page_info, &settings);

    assert!(output.body.contains("Heading"));
    assert!(output.body.contains("Text body"));
    assert!(
        output
            .meta
            .iter()
            .any(|meta| meta.value.contains("Coverage Page - Alt Coverage")),
    );
    assert!(
        output
            .backlinks
            .internal_links
            .iter()
            .any(|page| page.page() == "target-page"),
    );
    assert!(
        output
            .backlinks
            .internal_links
            .iter()
            .any(|page| page.page() == "local-page"),
    );
    assert!(
        output
            .backlinks
            .external_links
            .iter()
            .any(|link| link.as_ref() == "https://example.com"),
    );
}

#[test]
fn public_api_text_render_entrypoints_trim_outer_newlines() {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let elements = vec![
        Element::LineBreak,
        Element::Text(Cow::Borrowed("body")),
        Element::LineBreak,
    ];

    assert_eq!(
        TextRender.render_partial(&elements, &page_info, &settings, 9),
        "body",
    );

    let tree = SyntaxTree {
        elements,
        wikitext_len: 9,
        ..SyntaxTree::default()
    };
    assert_eq!(TextRender.render(&tree, &page_info, &settings), "body");
}

#[test]
fn public_api_parses_simple_table_boundary_cases() {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let mut text = String::from("|| A || B ||\n|| C || D ||");
    ftml::preprocess(&mut text);
    let tokens = ftml::tokenize(&text);
    assert!(!tokens.tokens().is_empty());

    let result = ftml::parse(&tokens, &page_info, &settings);
    assert!(result.value().wikitext_len <= text.len());
    let (tree, errors) = result.into();

    assert!(errors.is_empty(), "{errors:?}");
    let table = first_table(&tree.elements).expect("simple table should parse");
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0].cells.len(), 2);
    assert_eq!(table.rows[1].cells.len(), 2);
    assert_eq!(TextRender.render(&tree, &page_info, &settings), "AB\nCD");
}

#[test]
fn public_api_html_render_covers_fallback_branches_from_ast() {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::ForumPost, Layout::Wikijump);

    let mut bibliography = Bibliography::new();
    bibliography.add(
        Cow::Borrowed("alpha"),
        vec![Element::Text(Cow::Borrowed("Reference body"))],
    );
    let mut bibliographies = BibliographyList::new();
    bibliographies.push(bibliography);

    let mut list_attributes = AttributeMap::new();
    assert!(list_attributes.insert("class", Cow::Borrowed("nested-list")));

    let tree = SyntaxTree {
        elements: vec![
            Element::TableOfContents {
                attributes: AttributeMap::new(),
                align: Some(Alignment::Right),
            },
            Element::Math {
                name: Some(Cow::Borrowed("eq-main")),
                latex_source: Cow::Borrowed("x^2"),
                environment: None,
                wikidot_block_open_elements: Vec::new(),
                wikidot_label_spans: Vec::new(),
            },
            Element::Footnote(1),
            Element::Footnote(2),
            Element::FootnoteBlock {
                title: None,
                hide: false,
                wikidot_wrappers: Vec::new(),
                wikidot_inline_suffix: false,
                wikidot_inline_prefix: false,
            },
            Element::BibliographyCite {
                label: Cow::Borrowed("missing"),
                brackets: false,
                inline: true,
            },
            Element::BibliographyBlock {
                index: 0,
                title: None,
                hide: false,
            },
            Element::BibliographyBlock {
                index: 7,
                title: None,
                hide: false,
            },
            Element::Color {
                color: Cow::Borrowed("rebeccapurple"),
                background: false,
                elements: vec![Element::Text(Cow::Borrowed("colored"))],
            },
            Element::Image {
                source: ImageSource::Direct(FileSource::File1 {
                    file: Cow::Borrowed("local.png"),
                }),
                link: None,
                alignment: None,
                attributes: AttributeMap::new(),
                target: None,
            },
            Element::Audio {
                source: FileSource::File1 {
                    file: Cow::Borrowed("local.mp3"),
                },
                alignment: None,
                attributes: AttributeMap::new(),
            },
            Element::Video {
                source: FileSource::File1 {
                    file: Cow::Borrowed("local.mp4"),
                },
                alignment: None,
                attributes: AttributeMap::new(),
            },
            Element::List {
                ltype: ListType::Bullet,
                attributes: list_attributes,
                wikidot_explicit_block: false,
                items: vec![ListItem::SubList {
                    element: Box::new(Element::List {
                        ltype: ListType::Numbered,
                        attributes: AttributeMap::new(),
                        wikidot_explicit_block: false,
                        items: vec![ListItem::Elements {
                            attributes: AttributeMap::new(),
                            elements: vec![Element::Text(Cow::Borrowed("nested"))],
                        }],
                    }),
                }],
            },
            Element::Partial(PartialElement::Tab(Tab {
                label: Cow::Borrowed("Detached"),
                elements: vec![Element::Text(Cow::Borrowed("partial body"))],
            })),
            Element::Partial(PartialElement::RubyText(RubyText {
                attributes: AttributeMap::new(),
                elements: vec![Element::Text(Cow::Borrowed("ruby fallback"))],
            })),
        ],
        table_of_contents: vec![Element::Text(Cow::Borrowed("TOC entry"))],
        footnotes: vec![vec![Element::Text(Cow::Borrowed("Footnote body"))]],
        bibliographies,
        wikitext_len: 256,
        ..SyntaxTree::default()
    };

    let output = HtmlRender.render(&tree, &page_info, &settings);
    assert!(output.body.contains("Table of Contents"));
    assert!(output.body.contains("TOC entry"));
    assert!(output.body.contains("wj-equation-number"));
    assert!(output.body.contains("Footnote item not found"));
    assert!(output.body.contains("Footnote body"));
    assert!(output.body.contains("Bibliography item <em>"));
    assert!(output.body.contains("</em> not found."));
    assert!(output.body.contains("Reference body"));
    assert!(output.body.contains("Bibliography block not found"));
    assert!(output.body.contains("colored"));
    assert!(output.body.contains("No images in this context"));
    assert!(output.body.contains("No audio in this context"));
    assert!(output.body.contains("No videos in this context"));
    assert!(output.body.contains("nested"));
    assert!(output.body.contains("Detached"));
    assert!(output.body.contains("partial body"));
    assert!(output.body.contains("ruby fallback"));

    let include = IncludeRef::page_only(PageRef::parse("component:theme").unwrap());
    assert_eq!(include.page_ref().page(), "component:theme");
}
