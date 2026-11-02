use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::{Element, Module, SyntaxTree};
use std::borrow::Cow;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("scp-8980"),
        category: Some(Cow::Borrowed("_default")),
        site: Cow::Borrowed("scp-wiki"),
        title: Cow::Borrowed("SCP-8980"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![Cow::Borrowed("scp")],
        language: Cow::Borrowed("en"),
    }
}

fn page_settings() -> WikitextSettings {
    WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump)
}

fn with_parsed_tree(input: &str, visit: impl FnOnce(&SyntaxTree<'_>)) {
    let page_info = page_info();
    let settings = page_settings();
    let mut text = input.to_owned();

    ftml::preprocess(&mut text);
    let tokens = ftml::tokenize(&text);
    let result = ftml::parse(&tokens, &page_info, &settings);
    let (tree, errors) = result.into();

    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
    visit(&tree);
}

#[test]
fn scp8980_listpages_shape_is_preserved_as_delayed_node() {
    let source = include_str!("fixtures/scp8980/listpages.ftml");

    with_parsed_tree(source, |tree| {
        let [Element::Module(Module::ListPages { arguments, body })] =
            tree.elements.as_slice()
        else {
            panic!("expected exactly one top-level ListPages delayed module node");
        };

        let argument_pairs: Vec<(&str, &str)> = arguments
            .iter()
            .map(|argument| (argument.name.as_ref(), argument.value.as_ref()))
            .collect();
        assert_eq!(
            argument_pairs,
            vec![
                ("parent", "."),
                ("category", "fragment"),
                ("order", "created_at"),
                ("limit", "1"),
                ("offset", "@URL|0"),
            ],
        );
        assert_eq!(body, "%%content%%");
    });
}
