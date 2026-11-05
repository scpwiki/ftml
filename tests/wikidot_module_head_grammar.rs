use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::Render;
use ftml::render::html::HtmlRender;
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::{Element, Module, SyntaxTree};
use std::borrow::Cow;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("module-head-grammar"),
        category: None,
        site: Cow::Borrowed("sandbox-for-codex"),
        title: Cow::Borrowed("Module head grammar"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn settings() -> WikitextSettings {
    WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot)
}

fn parse_wikidot(source: &str) -> SyntaxTree<'static> {
    let page_info = page_info();
    let settings = settings();
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, _errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    tree.to_owned()
}

fn html(tree: &SyntaxTree<'_>) -> String {
    HtmlRender.render(tree, &page_info(), &settings()).body
}

fn list_pages(tree: &SyntaxTree<'_>) -> (Vec<(String, String)>, String) {
    let Element::Module(Module::ListPages { arguments, body }) = &tree.elements[0] else {
        panic!("expected ListPages module: {tree:#?}");
    };

    let arguments = arguments
        .iter()
        .map(|argument| (argument.name.to_string(), argument.value.to_string()))
        .collect();
    (arguments, body.to_string())
}

fn backlinks(tree: &SyntaxTree<'_>) -> Option<String> {
    let Element::Module(Module::Backlinks { page }) = &tree.elements[0] else {
        panic!("expected Backlinks module: {tree:#?}");
    };
    page.as_ref().map(|page| page.to_string())
}

fn page_tree_root(tree: &SyntaxTree<'_>) -> Option<String> {
    let Element::Module(Module::PageTree { root, .. }) = &tree.elements[0] else {
        panic!("expected PageTree module: {tree:#?}");
    };
    root.as_ref().map(|root| root.to_string())
}

fn module_count(tree: &SyntaxTree<'_>) -> usize {
    tree.elements
        .iter()
        .filter(|element| matches!(element, Element::Module(_)))
        .count()
}

#[test]
fn self_closing_module_head_parses_with_an_empty_body() {
    let tree = parse_wikidot("[[module ListPages limit=\"1\"]]\nAFTER");
    assert_eq!(
        list_pages(&tree),
        (vec![("limit".to_owned(), "1".to_owned())], String::new()),
    );
    assert_eq!(html(&tree), "<p>TODO: module ListPages</p><p>AFTER</p>");

    let tree = parse_wikidot("[[module ListPages]]");
    assert_eq!(list_pages(&tree), (Vec::new(), String::new()));
    assert_eq!(html(&tree), "<p>TODO: module ListPages</p>");

    let tree = parse_wikidot("[[module Rate]]   \nAFTER");
    assert_eq!(module_count(&tree), 1);
    assert_eq!(html(&tree), "<p>TODO: module Rate</p><p>AFTER</p>");
}

#[test]
fn same_line_closer_is_consumed_when_the_head_has_a_separator() {
    let tree = parse_wikidot("[[module Backlinks page=\"scp-001\"]][[/module]]");
    assert_eq!(backlinks(&tree), Some("scp-001".to_owned()));
    assert_eq!(html(&tree), "<p>TODO: module Backlinks</p>");

    let tree = parse_wikidot("[[module Backlinks ]][[/module]]");
    assert_eq!(backlinks(&tree), None);
    assert_eq!(html(&tree), "<p>TODO: module Backlinks</p>");

    let tree = parse_wikidot("[[module Backlinks page=\"scp-001\"]] [[/module]]");
    assert_eq!(backlinks(&tree), Some("scp-001".to_owned()));

    let tree = parse_wikidot("[[module TagCloud limit=\"1\"]][[/module]]");
    assert_eq!(module_count(&tree), 1);
}

#[test]
fn same_line_closer_is_literal_when_the_head_has_no_separator() {
    for source in [
        "[[module Backlinks]][[/module]]",
        "[[module Rate]][[/module]]",
        "[[module ListPages]][[/module]]",
        "[[module PageTree]][[/module]]",
    ] {
        let tree = parse_wikidot(source);
        assert_eq!(module_count(&tree), 0, "{source}: {tree:#?}");
        assert_eq!(html(&tree), format!("<p>{source}</p>"), "{source}");
    }
}

#[test]
fn same_line_content_after_the_head_makes_the_line_literal() {
    for source in [
        "[[module Rate]]BODY[[/module]]",
        "[[module ListPages limit=\"1\"]]TAIL",
        "[[module Backlinks page=\"scp-001\"]]TAIL",
        "[[module ListPages limit=\"1\"]][[/module]]TAIL",
        "[[module Bogus x=\"1\"]]TAIL",
    ] {
        let tree = parse_wikidot(source);
        assert_eq!(module_count(&tree), 0, "{source}: {tree:#?}");
        let expected = source.replace('"', "&quot;");
        assert_eq!(html(&tree), format!("<p>{expected}</p>"), "{source}");
    }
}

#[test]
fn empty_body_closed_on_the_next_line_keeps_the_closer_literal() {
    let tree = parse_wikidot("[[module ListPages limit=\"1\"]]\n[[/module]]\nAFTER");
    assert_eq!(
        list_pages(&tree),
        (vec![("limit".to_owned(), "1".to_owned())], String::new()),
    );
    assert_eq!(
        html(&tree),
        "<p>TODO: module ListPages</p><p>[[/module]]<br>\nAFTER</p>",
    );

    let tree = parse_wikidot("[[module Backlinks]]\n\n[[/module]]");
    assert_eq!(backlinks(&tree), None);
    assert_eq!(
        html(&tree),
        "<p>TODO: module Backlinks</p><p>[[/module]]</p>",
    );
}

#[test]
fn bodyless_modules_consume_a_closed_body() {
    let tree = parse_wikidot("[[module Backlinks]]\nBODY\n[[/module]]\nAFTER");
    assert_eq!(backlinks(&tree), None);
    assert_eq!(html(&tree), "<p>TODO: module Backlinks</p><p>AFTER</p>");

    let tree = parse_wikidot("[[module Rate]]\nBODY\n[[/module]]");
    assert_eq!(module_count(&tree), 1);
    assert_eq!(html(&tree), "<p>TODO: module Rate</p>");
}

#[test]
fn closed_body_does_not_swallow_a_nested_module() {
    let tree = parse_wikidot("[[module Backlinks]]\n[[module Rate]]\nBODY\n[[/module]]");
    assert_eq!(module_count(&tree), 2);
    assert_eq!(
        html(&tree),
        "<p>TODO: module Backlinks</p><p>TODO: module Rate</p>",
    );
}

#[test]
fn css_module_owns_its_inline_head_body() {
    let tree = parse_wikidot("[[module CSS show=\"head\"]]x{}[[/module]]");
    assert_eq!(module_count(&tree), 0);
    assert_eq!(html(&tree), "");

    let tree = parse_wikidot("[[module CSS show=\"head\"]]x{}");
    assert_eq!(
        html(&tree),
        "<p>[[module CSS show=&quot;head&quot;]]x{}</p>"
    );
}

#[test]
fn module_arguments_are_double_quoted_key_value_pairs() {
    for source in [
        "[[module ListPages limit='1' category='*']]",
        "[[module ListPages limit=1 category=*]]",
        "[[module ListPages limit = \"1\"]]",
        "[[module ListPages limit= \"1\"]]",
        "[[module ListPages limit =\"1\"]]",
    ] {
        let tree = parse_wikidot(source);
        assert_eq!(
            list_pages(&tree).0,
            Vec::<(String, String)>::new(),
            "{source}"
        );
    }

    let tree = parse_wikidot("[[module ListPages limit=\"1\" \"junk\" category=\"*\"]]");
    assert_eq!(
        list_pages(&tree).0,
        vec![
            ("limit".to_owned(), "1".to_owned()),
            ("category".to_owned(), "*".to_owned()),
        ],
    );

    let tree = parse_wikidot("[[module ListPages order=\"created_at desc\"]]");
    assert_eq!(
        list_pages(&tree).0,
        vec![("order".to_owned(), "created_at desc".to_owned())],
    );

    let tree = parse_wikidot("[[module PageTree junk root=\"scp-001\"]]");
    assert_eq!(page_tree_root(&tree), Some("scp-001".to_owned()));

    let tree = parse_wikidot("[[module PageTree root='scp-001']]");
    assert_eq!(page_tree_root(&tree), None);
}
