use ftml::data::{PageInfo, PageRef, ScoreValue};
use ftml::includes::{FetchedPage, IncludeRef, Includer};
use ftml::layout::Layout;
use ftml::render::Render;
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::{Element, Module};
use std::borrow::Cow;
use std::collections::HashMap;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("param-bracket-values"),
        category: None,
        site: Cow::Borrowed("test"),
        title: Cow::Borrowed("Param Bracket Values"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![],
        language: Cow::Borrowed("default"),
    }
}

fn settings(layout: Layout) -> WikitextSettings {
    WikitextSettings::from_mode(WikitextMode::Page, layout)
}

#[derive(Debug)]
struct StaticIncluder {
    pages: HashMap<&'static str, &'static str>,
}

impl<'t> Includer<'t> for StaticIncluder {
    type Error = String;

    fn include_pages(
        &mut self,
        includes: &[IncludeRef<'t>],
    ) -> Result<Vec<FetchedPage<'t>>, Self::Error> {
        Ok(includes
            .iter()
            .map(|include| {
                let page_ref = include.page_ref().clone();
                let key = page_ref.to_string();
                let content = self
                    .pages
                    .get(key.as_str())
                    .map(|content| Cow::Borrowed(*content));

                FetchedPage { page_ref, content }
            })
            .collect())
    }

    fn no_such_include(
        &mut self,
        page_ref: &PageRef,
    ) -> Result<Cow<'t, str>, Self::Error> {
        Err(format!("missing include fixture for {page_ref}"))
    }
}

fn parse_tree(
    source: &str,
    layout: Layout,
) -> (
    ftml::tree::SyntaxTree<'static>,
    Vec<ftml::parsing::ParseError>,
) {
    let mut text = source.to_owned();
    let page_info = page_info();
    let settings = settings(layout);
    ftml::preprocess_for_layout(&mut text, settings.layout);
    let tokens = ftml::tokenize(&text);
    let result = ftml::parse(&tokens, &page_info, &settings);
    let (tree, errors) = result.into();
    (tree.to_owned(), errors)
}

fn render_text(source: &str, layout: Layout) -> String {
    let mut text = source.to_owned();
    let page_info = page_info();
    let settings = settings(layout);
    ftml::preprocess_for_layout(&mut text, settings.layout);
    let tokens = ftml::tokenize(&text);
    let result = ftml::parse(&tokens, &page_info, &settings);
    let (tree, _errors) = result.into();

    ftml::render::text::TextRender.render(&tree, &page_info, &settings)
}

fn render_expanded(source: &str, layout: Layout) -> String {
    let includer = StaticIncluder {
        pages: HashMap::from([(
            "probe-f66:component-echo",
            "Note: {$note}\nOther: {$other}",
        )]),
    };
    let (expanded, _pages) = ftml::include(source, &settings(layout), includer, || {
        "invalid include result".to_owned()
    })
    .expect("include expansion should succeed");

    render_text(&expanded, layout)
}

const INCLUDE_PROBE: &str = concat!(
    "Control include:\n",
    "[[include probe-f66:component-echo |note=plain |other=x]]\n",
    "\n",
    "Bracket case:\n",
    "[[include probe-f66:component-echo |note=a]]b]]\n",
    "\n",
    "After marker line.\n",
);

#[test]
fn include_value_containing_brackets_survives_wikidot_layout() {
    let text = render_expanded(INCLUDE_PROBE, Layout::Wikidot);

    assert!(text.contains("Note: plain"), "{text}");
    assert!(text.contains("Other: x"), "{text}");
    assert!(text.contains("Note: a]]b"), "{text}");
    assert!(text.contains("After marker line."), "{text}");
}

#[test]
fn include_value_containing_brackets_survives_wikijump_layout() {
    let text = render_expanded(INCLUDE_PROBE, Layout::Wikijump);

    assert!(text.contains("Note: plain"), "{text}");
    assert!(text.contains("Other: x"), "{text}");
    assert!(text.contains("Note: a]]b"), "{text}");
    assert!(text.contains("After marker line."), "{text}");
}

#[test]
fn module_head_value_containing_brackets_parses() {
    for layout in [Layout::Wikidot, Layout::Wikijump] {
        let source = concat!(
            "[[module ListPages limit=\"1\" separate=\"no\" ",
            "fullname=\"probe-f66:bl-target]]z\"]]\n",
            "BODY\n",
            "[[/module]]\n",
        );
        let (tree, errors) = parse_tree(source, layout);

        assert!(errors.is_empty(), "{layout:?}: {errors:?}");
        let [Element::Module(Module::ListPages { arguments, .. })] = &tree.elements[..]
        else {
            panic!(
                "{layout:?}: expected a ListPages module, got {:?}",
                tree.elements
            );
        };
        let fullname = arguments
            .iter()
            .find(|argument| argument.name == "fullname")
            .expect("fullname argument should be present");
        assert_eq!(fullname.value, "probe-f66:bl-target]]z", "{layout:?}");
    }
}
