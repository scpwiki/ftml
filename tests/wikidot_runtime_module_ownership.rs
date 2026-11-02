use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::{Element, Module, SyntaxTree};
use std::borrow::Cow;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("runtime-module-ownership"),
        category: None,
        site: Cow::Borrowed("sandbox-for-codex"),
        title: Cow::Borrowed("Runtime module ownership"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn parse_wikidot(source: &str) -> SyntaxTree<'static> {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();

    assert!(errors.is_empty(), "{errors:#?}");
    tree.to_owned()
}

fn runtime_module(tree: &SyntaxTree<'_>) -> (String, Vec<(String, String)>, String) {
    let [
        Element::Module(Module::Runtime {
            name,
            arguments,
            body,
        }),
    ] = tree.elements.as_slice()
    else {
        panic!(
            "expected one typed runtime module, got {:#?}",
            tree.elements
        );
    };
    let arguments = arguments
        .iter()
        .map(|argument| {
            (
                argument.name.as_ref().to_owned(),
                argument.value.as_ref().to_owned(),
            )
        })
        .collect::<Vec<_>>();

    (name.to_string(), arguments, body.to_string())
}

#[test]
fn sitegrid_body_is_preserved_as_a_typed_runtime_module() {
    let tree = parse_wikidot(concat!(
        "[[module SiteGrid limit=\"20\"]]\n",
        "wikipiano\n",
        "www.digistan.org\n",
        "science.wikidot.com\n",
        "[[/module]]",
    ));
    let (name, arguments, body) = runtime_module(&tree);

    assert_eq!(name, "SiteGrid");
    assert_eq!(arguments, [("limit".to_owned(), "20".to_owned())]);
    assert_eq!(body, "wikipiano\nwww.digistan.org\nscience.wikidot.com",);
}

#[test]
fn delayed_body_ownership_is_generic_and_survives_to_owned() {
    for (name, body) in [
        ("SiteGrid", "alpha.example\nbeta.example"),
        ("RuntimeOwnershipProbe", "different payload\nwith two lines"),
    ] {
        let source = format!("[[module {name} mode=\"safe\"]]\n{body}\n[[/module]]");
        let tree = parse_wikidot(&source);
        let (actual_name, arguments, actual_body) = runtime_module(&tree);

        assert_eq!(actual_name, name);
        assert_eq!(arguments, [("mode".to_owned(), "safe".to_owned())]);
        assert_eq!(actual_body, body);
    }
}
