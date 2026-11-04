use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::Render;
use ftml::render::text::TextRender;
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::{ContainerType, Element, ListItem, SyntaxTree};
use std::borrow::Cow;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("strikethrough-inline-regression"),
        category: Some(Cow::Borrowed("test")),
        site: Cow::Borrowed("coverage"),
        title: Cow::Borrowed("Strikethrough Inline Regression"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![Cow::Borrowed("test")],
        language: Cow::Borrowed("default"),
    }
}

fn parse(input: &str) -> SyntaxTree<'static> {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let mut text = input.to_owned();
    ftml::preprocess(&mut text);
    let tokens = ftml::tokenize(&text);
    let result = ftml::parse(&tokens, &page_info, &settings);
    let (tree, errors) = result.into();

    assert!(errors.is_empty(), "{errors:?}");
    tree.to_owned()
}

fn render_text(input: &str) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    TextRender.render(&parse(input), &page_info, &settings)
}

fn count_container(tree: &SyntaxTree<'_>, needle: ContainerType) -> usize {
    fn count_elements(elements: &[Element<'_>], needle: ContainerType) -> usize {
        elements
            .iter()
            .map(|element| match element {
                Element::Container(container) => {
                    usize::from(container.ctype() == needle)
                        + count_elements(container.elements(), needle)
                }
                Element::Table(table) => table
                    .rows
                    .iter()
                    .flat_map(|row| row.cells.iter())
                    .map(|cell| count_elements(&cell.elements, needle))
                    .sum(),
                Element::TabView(tabs) => tabs
                    .iter()
                    .map(|item| count_elements(item.elements(), needle))
                    .sum(),
                Element::Anchor { elements, .. }
                | Element::Collapsible { elements, .. }
                | Element::Color { elements, .. } => count_elements(elements, needle),
                Element::List { items, .. } => items
                    .iter()
                    .map(|item| match item {
                        ListItem::Elements { elements, .. } => {
                            count_elements(elements, needle)
                        }
                        ListItem::SubList { element } => {
                            count_elements(std::slice::from_ref(element), needle)
                        }
                    })
                    .sum(),
                Element::DefinitionList(items) => items
                    .iter()
                    .map(|item| {
                        count_elements(&item.key_elements, needle)
                            + count_elements(&item.value_elements, needle)
                    })
                    .sum(),
                _ => 0,
            })
            .sum()
    }

    count_elements(&tree.elements, needle)
}

#[test]
fn unclosed_dash_prose_remains_em_dash_text() {
    let tree = parse("a -- b");

    assert_eq!(count_container(&tree, ContainerType::Strikethrough), 0);
    assert_eq!(render_text("a -- b"), "a \u{2014} b");
}

#[test]
fn compact_dash_strikethrough_still_parses() {
    let tree = parse("--strike--");

    assert_eq!(count_container(&tree, ContainerType::Strikethrough), 1);
    assert_eq!(render_text("--strike--"), "strike");
}

#[test]
fn whitespace_around_dash_delimiters_still_does_not_parse() {
    let tree = parse("-- strike --");

    assert_eq!(count_container(&tree, ContainerType::Strikethrough), 0);
    assert_eq!(render_text("-- strike --"), "\u{2014} strike \u{2014}");
}

#[test]
fn dash_strikethrough_still_contains_nested_bold_and_italics() {
    let tree = parse("--nested **bold** //italic//--");

    assert_eq!(count_container(&tree, ContainerType::Strikethrough), 1);
    assert_eq!(count_container(&tree, ContainerType::Bold), 1);
    assert_eq!(count_container(&tree, ContainerType::Italics), 1);
    assert_eq!(
        render_text("--nested **bold** //italic//--"),
        "nested bold italic"
    );
}

#[test]
fn dash_strikethrough_opener_does_not_match_across_paragraph_break() {
    let input = "--first\n\n--second--";
    let tree = parse(input);

    assert_eq!(count_container(&tree, ContainerType::Strikethrough), 1);
    assert_eq!(render_text(input), "\u{2014}first\n\nsecond");
}

#[test]
fn dash_strikethrough_does_not_cross_a_list_and_paragraph_break() {
    let tree = parse("--\n* item\n\n--");

    assert_eq!(count_container(&tree, ContainerType::Strikethrough), 0);
}

#[test]
fn multiple_dash_strikethroughs_in_one_paragraph_still_parse() {
    let tree = parse("--one-- and --two--");

    assert_eq!(count_container(&tree, ContainerType::Strikethrough), 2);
    assert_eq!(render_text("--one-- and --two--"), "one and two");
}

#[test]
fn prose_with_two_dash_pairs_still_uses_em_dashes() {
    let tree = parse("a -- b -- c");

    assert_eq!(count_container(&tree, ContainerType::Strikethrough), 0);
    assert_eq!(render_text("a -- b -- c"), "a \u{2014} b \u{2014} c");
}
