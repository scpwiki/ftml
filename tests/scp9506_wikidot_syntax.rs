use ftml::data::{PageInfo, PageRef, ScoreValue};
use ftml::includes::{FetchedPage, IncludeRef, Includer};
use ftml::layout::Layout;
use ftml::render::Render;
use ftml::render::text::TextRender;
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::{Element, Module, SyntaxTree};
use std::borrow::Cow;
use std::collections::HashMap;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("scp-9506"),
        category: Some(Cow::Borrowed("_default")),
        site: Cow::Borrowed("scp-wiki"),
        title: Cow::Borrowed("National Fog Safety Initiative"),
        alt_title: None,
        score: ScoreValue::Integer(388),
        tags: vec![Cow::Borrowed("scp"), Cow::Borrowed("theme")],
        language: Cow::Borrowed("en"),
    }
}

fn page_settings() -> WikitextSettings {
    WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump)
}

fn assert_parses_without_errors(input: &str) {
    let page_info = page_info();
    let settings = page_settings();
    let mut text = input.to_owned();

    ftml::preprocess(&mut text);
    let tokens = ftml::tokenize(&text);
    let result = ftml::parse(&tokens, &page_info, &settings);
    let (_tree, errors) = result.into();

    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
}

fn render_text(input: &str) -> String {
    let page_info = page_info();
    let settings = page_settings();
    let mut text = input.to_owned();

    ftml::preprocess(&mut text);
    let tokens = ftml::tokenize(&text);
    let result = ftml::parse(&tokens, &page_info, &settings);
    let (tree, errors) = result.into();

    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
    TextRender.render(&tree, &page_info, &settings)
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

struct StaticIncluder {
    pages: HashMap<String, &'static str>,
}

impl StaticIncluder {
    fn new(pages: impl IntoIterator<Item = (&'static str, &'static str)>) -> Self {
        Self {
            pages: pages
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        }
    }
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
                let content = self.pages.get(&key).map(|content| Cow::Borrowed(*content));

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

#[test]
fn scp9506_theme_and_component_includes_expand_with_variables() {
    let source = r#"[[include :scp-wiki:theme:basalt | hidetitle=true]]
[[include :scp-wiki:component:acs-animation]]
[[include :scp-wiki:component:mega-cool-author-page-tool
 |inc-custom-list= --]
 |category=SCPs
 |tags=+scp -co-authored
 |order=
 |perpage=250
 |shadow=no
]]
"#;

    let includer = StaticIncluder::new([
        (":scp-wiki:theme:basalt", "BASALT THEME {$hidetitle}"),
        (":scp-wiki:component:acs-animation", "ACS ANIMATION CSS"),
        (
            ":scp-wiki:component:mega-cool-author-page-tool",
            r#"AUTHOR TOOL {$category}
{$inc-custom-list}
[[module ListPages created_by="=" order="{$order}" category="-fragment" tag="{$tags}" perPage="{$perpage}"]]
[[div class="content-box {$shadow}"]]
%%title_linked%%
%%content%%
%%content{1}%%
[[/div]]
[[/module]]
"#,
        ),
    ]);

    let (expanded, pages) = ftml::include(source, &page_settings(), includer, || {
        "invalid include result".to_owned()
    })
    .expect("SCP-9506 include dependencies should parse and expand");

    let page_refs: Vec<String> = pages.into_iter().map(|page| page.to_string()).collect();
    assert_eq!(
        page_refs,
        vec![
            ":scp-wiki:theme:basalt",
            ":scp-wiki:component:acs-animation",
            ":scp-wiki:component:mega-cool-author-page-tool",
        ],
    );

    assert!(expanded.contains("BASALT THEME true"));
    assert!(expanded.contains("ACS ANIMATION CSS"));
    assert!(expanded.contains("AUTHOR TOOL SCPs"));
    assert!(expanded.contains("[[module ListPages"));
    assert!(expanded.contains("order=\"{$order}\""));
    assert!(expanded.contains("+scp -co-authored"));
    assert!(expanded.contains("perPage=\"250"));
    assert!(expanded.contains("content-box no"));
    assert!(expanded.contains("%%content%%"));
    assert!(expanded.contains("%%content{1}%%"));
}

#[test]
fn scp9506_listpages_syntax_is_preserved_as_delayed_node() {
    let expanded_source = r#"[[module CSS]]
@import url(https://scp-wiki.wdfiles.com/local--code/theme%3Abasalt/3);
.nfsi-grid { display: grid; }
[[/module]]

[[div class="nfsi-alert"]]
National Fog Safety Initiative local compatibility page.
[[/div]]

[[image fog-green.svg]]
[[image fog-map.svg]]
[[image alert-card.svg]]

[[module ListPages created_by="=" tag="+scp" order="" category="-fragment" tag="-co-authored" perPage="250" custom="@URL" unknown="kept"]]
[[div class="content-box no"]]
%%title_linked%%
%%content%%
%%content{1}%%
[[/div]]
[[/module]]

====
Section one
====
Section two

[[iframe https://scp-wiki.wikidot.com/common--theme/base/css/style.css]]
"#;

    assert_parses_without_errors(expanded_source);
    with_parsed_tree(expanded_source, |tree| {
        let (arguments, body) = tree
            .elements
            .iter()
            .find_map(|element| match element {
                Element::Module(Module::ListPages { arguments, body }) => {
                    Some((arguments, body))
                }
                _ => None,
            })
            .expect("ListPages should be preserved as a delayed module node");

        let argument_pairs: Vec<(&str, &str)> = arguments
            .iter()
            .map(|argument| (argument.name.as_ref(), argument.value.as_ref()))
            .collect();
        assert_eq!(
            argument_pairs,
            vec![
                ("created_by", "="),
                ("tag", "+scp"),
                ("order", ""),
                ("category", "-fragment"),
                ("tag", "-co-authored"),
                ("perPage", "250"),
                ("custom", "@URL"),
                ("unknown", "kept"),
            ],
        );
        assert!(body.contains(r#"[[div class="content-box no"]]"#));
        assert!(body.contains("%%content%%"));
        assert!(body.contains("%%content{1}%%"));
        assert!(body.contains("[[/div]]"));

        let owned_tree = tree.to_owned();
        let owned_module = owned_tree
            .elements
            .iter()
            .find_map(|element| match element {
                Element::Module(module @ Module::ListPages { .. }) => Some(module),
                _ => None,
            })
            .expect("owned tree should preserve the ListPages module");
        assert_eq!(owned_module.name(), "ListPages");
        match owned_module {
            Module::ListPages { arguments, body } => {
                assert_eq!(arguments[1].name.as_ref(), "tag");
                assert_eq!(arguments[1].value.as_ref(), "+scp");
                assert!(body.contains("%%title_linked%%"));
            }
            _ => unreachable!("matched ListPages above"),
        }
    });

    let rendered = render_text(expanded_source);
    assert!(
        rendered.contains("National Fog Safety Initiative local compatibility page.")
    );
    assert!(rendered.contains("Section one"));
    assert!(rendered.contains("Section two"));
}
