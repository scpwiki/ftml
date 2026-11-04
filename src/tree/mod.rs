/*
 * tree/mod.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2026 Wikijump Team
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program. If not, see <http://www.gnu.org/licenses/>.
 */

pub mod attribute;

mod align;
mod anchor;
mod bibliography;
mod button;
mod clear_float;
mod clone;
mod code;
mod container;
mod content;
mod date;
mod definition_list;
mod element;
mod embed;
mod embed_video;
mod file_source;
mod gallery;
mod heading;
mod image_source;
mod link;
mod list;
mod module;
mod partial;
mod ruby;
mod social;
mod tab;
mod table;
mod tag;
mod traversal;
mod variables;

pub use self::align::*;
pub use self::anchor::*;
pub use self::attribute::AttributeMap;
pub use self::bibliography::*;
pub use self::button::*;
pub use self::clear_float::*;
pub use self::code::CodeBlock;
pub use self::container::*;
pub use self::content::*;
pub use self::date::DateItem;
pub(crate) use self::date::date_format_within_limits;
pub use self::definition_list::*;
pub use self::element::*;
pub use self::embed::*;
pub use self::embed_video::*;
pub use self::file_source::*;
pub use self::gallery::*;
pub use self::heading::*;
pub use self::image_source::*;
pub use self::link::*;
pub use self::list::*;
pub use self::module::*;
pub use self::partial::*;
pub use self::ruby::*;
pub use self::social::*;
pub use self::tab::*;
pub use self::table::*;
pub use self::tag::*;
pub use self::variables::*;

use self::clone::{elements_lists_to_owned, elements_to_owned, string_to_owned};
pub(crate) use self::traversal::{
    element_requires_bounded_tree_stack, elements_require_bounded_tree_stack,
    on_bounded_tree_stack, run_on_bounded_tree_stack, tree_requires_bounded_tree_stack,
};
use crate::data::PageRef;
use crate::parsing::{ParseError, ParseOutcome};

pub(crate) const WIKIDOT_GENERATED_EMPTY_CLASS_MARKER: &str =
    "data-ftml-generated-empty-class";
use std::borrow::Cow;
use std::collections::HashSet;

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "kebab-case")]
pub struct SyntaxTree<'t> {
    pub elements: Vec<Element<'t>>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub table_of_contents: Vec<Element<'t>>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub html_blocks: Vec<Cow<'t, str>>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub code_blocks: Vec<CodeBlock<'t>>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub footnotes: Vec<Vec<Element<'t>>>,

    #[serde(default, skip_serializing_if = "Not::not")]
    pub needs_footnote_block: bool,

    #[serde(default, skip_serializing_if = "BibliographyList::is_empty")]
    pub bibliographies: BibliographyList<'t>,

    #[serde(skip)]
    pub wikitext_len: usize,
}

impl PartialEq for SyntaxTree<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.elements == other.elements
            && self.table_of_contents == other.table_of_contents
            && self.html_blocks == other.html_blocks
            && self.code_blocks == other.code_blocks
            && self.footnotes == other.footnotes
            && self.needs_footnote_block == other.needs_footnote_block
            && self.bibliographies == other.bibliographies
    }
}

impl Eq for SyntaxTree<'_> {}

impl serde::Serialize for SyntaxTree<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::{Serializer as _, ser::SerializeStruct};

        let mut field_count = 1;
        field_count += usize::from(!self.table_of_contents.is_empty());
        field_count += usize::from(!self.html_blocks.is_empty());
        field_count += usize::from(!self.code_blocks.is_empty());
        field_count += usize::from(!self.footnotes.is_empty());
        field_count += usize::from(self.needs_footnote_block);
        field_count += usize::from(!self.bibliographies.is_empty());

        let serializer = serde_stacker::Serializer::new(serializer);
        let mut state = serializer.serialize_struct("SyntaxTree", field_count)?;
        state.serialize_field("elements", &self.elements)?;
        if !self.table_of_contents.is_empty() {
            state.serialize_field("table-of-contents", &self.table_of_contents)?;
        }
        if !self.html_blocks.is_empty() {
            state.serialize_field("html-blocks", &self.html_blocks)?;
        }
        if !self.code_blocks.is_empty() {
            state.serialize_field("code-blocks", &self.code_blocks)?;
        }
        if !self.footnotes.is_empty() {
            state.serialize_field("footnotes", &self.footnotes)?;
        }
        if self.needs_footnote_block {
            state.serialize_field("needs-footnote-block", &self.needs_footnote_block)?;
        }
        if !self.bibliographies.is_empty() {
            state.serialize_field("bibliographies", &self.bibliographies)?;
        }
        state.end()
    }
}

impl<'t> Clone for SyntaxTree<'t> {
    fn clone(&self) -> Self {
        if tree_requires_bounded_tree_stack(self) {
            run_on_bounded_tree_stack("ftml-tree-clone", || self.clone_on_current_stack())
        } else {
            self.clone_on_current_stack()
        }
    }
}

impl<'t> SyntaxTree<'t> {
    fn clone_on_current_stack(&self) -> Self {
        Self {
            elements: self.elements.clone(),
            table_of_contents: self.table_of_contents.clone(),
            html_blocks: self.html_blocks.clone(),
            code_blocks: self.code_blocks.clone(),
            footnotes: self.footnotes.clone(),
            needs_footnote_block: self.needs_footnote_block,
            bibliographies: self.bibliographies.clone(),
            wikitext_len: self.wikitext_len,
        }
    }

    pub(crate) fn from_element_result(
        elements: Vec<Element<'t>>,
        errors: Vec<ParseError>,
        (html_blocks, code_blocks): (Vec<Cow<'t, str>>, Vec<CodeBlock<'t>>),
        table_of_contents: Vec<Element<'t>>,
        (footnotes, needs_footnote_block): (Vec<Vec<Element<'t>>>, bool),
        bibliographies: BibliographyList<'t>,
        wikitext_len: usize,
    ) -> ParseOutcome<Self> {
        let tree = SyntaxTree {
            elements,
            table_of_contents,
            html_blocks,
            code_blocks,
            footnotes,
            needs_footnote_block,
            bibliographies,
            wikitext_len,
        };
        ParseOutcome::new(tree, errors)
    }

    pub fn to_owned(&self) -> SyntaxTree<'static> {
        if tree_requires_bounded_tree_stack(self) {
            run_on_bounded_tree_stack("ftml-tree-clone", || {
                self.to_owned_on_current_stack()
            })
        } else {
            self.to_owned_on_current_stack()
        }
    }

    fn to_owned_on_current_stack(&self) -> SyntaxTree<'static> {
        SyntaxTree {
            elements: elements_to_owned(&self.elements),
            table_of_contents: elements_to_owned(&self.table_of_contents),
            html_blocks: self
                .html_blocks
                .iter()
                .map(|html| string_to_owned(html))
                .collect(),
            code_blocks: self
                .code_blocks
                .iter()
                .map(|code| code.to_owned())
                .collect(),
            footnotes: elements_lists_to_owned(&self.footnotes),
            needs_footnote_block: self.needs_footnote_block,
            bibliographies: self.bibliographies.to_owned(),
            wikitext_len: self.wikitext_len,
        }
    }

    /// Collects the distinct internal page references rendered by this tree.
    ///
    /// The returned references retain their site and extra URL portions. Their
    /// order is the order of first appearance in the tree.
    pub fn page_references(&self) -> Vec<PageRef> {
        if tree_requires_bounded_tree_stack(self) {
            return run_on_bounded_tree_stack("ftml-page-references", || {
                self.page_references_on_current_stack()
            });
        }
        self.page_references_on_current_stack()
    }

    fn page_references_on_current_stack(&self) -> Vec<PageRef> {
        let mut references = Vec::new();
        let mut seen = HashSet::new();

        collect_page_references(&self.elements, &mut references, &mut seen);
        collect_page_references(&self.table_of_contents, &mut references, &mut seen);
        for footnote in &self.footnotes {
            collect_page_references(footnote, &mut references, &mut seen);
        }
        for bibliography in self.bibliographies.slice() {
            for (_, elements) in bibliography.slice() {
                collect_page_references(elements, &mut references, &mut seen);
            }
        }

        references
    }

    /// Collects the distinct user names rendered by this tree.
    ///
    /// Their order is the order of first appearance in the tree.
    pub fn user_references(&self) -> Vec<String> {
        if tree_requires_bounded_tree_stack(self) {
            return run_on_bounded_tree_stack("ftml-user-references", || {
                self.user_references_on_current_stack()
            });
        }
        self.user_references_on_current_stack()
    }

    fn user_references_on_current_stack(&self) -> Vec<String> {
        let mut references = Vec::new();
        let mut seen = HashSet::new();

        collect_user_references(&self.elements, &mut references, &mut seen);
        collect_user_references(&self.table_of_contents, &mut references, &mut seen);
        for footnote in &self.footnotes {
            collect_user_references(footnote, &mut references, &mut seen);
        }
        for bibliography in self.bibliographies.slice() {
            for (_, elements) in bibliography.slice() {
                collect_user_references(elements, &mut references, &mut seen);
            }
        }

        references
    }
}

fn collect_page_references(
    elements: &[Element<'_>],
    references: &mut Vec<PageRef>,
    seen: &mut HashSet<PageRef>,
) {
    for element in elements {
        if let Element::Link {
            link: LinkLocation::Page(page),
            ..
        } = element
            && seen.insert(page.clone())
        {
            references.push(page.clone());
        }

        match element {
            Element::Container(container) => {
                collect_page_references(container.elements(), references, seen);
            }
            Element::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        collect_page_references(&cell.elements, references, seen);
                    }
                }
            }
            Element::TabView(tabs) => {
                for item in tabs {
                    collect_page_references(item.elements(), references, seen);
                }
            }
            Element::Anchor { elements, .. }
            | Element::Collapsible { elements, .. }
            | Element::Color { elements, .. }
            | Element::Include { elements, .. } => {
                collect_page_references(elements, references, seen);
            }
            Element::List { items, .. } => {
                for item in items {
                    match item {
                        ListItem::Elements { elements, .. } => {
                            collect_page_references(elements, references, seen);
                        }
                        ListItem::SubList { element } => {
                            collect_page_references(
                                std::slice::from_ref(element),
                                references,
                                seen,
                            );
                        }
                    }
                }
            }
            Element::DefinitionList(items) => {
                for item in items {
                    collect_page_references(&item.key_elements, references, seen);
                    collect_page_references(&item.value_elements, references, seen);
                }
            }
            Element::Partial(partial) => match partial {
                PartialElement::ListItem(ListItem::Elements { elements, .. }) => {
                    collect_page_references(elements, references, seen);
                }
                PartialElement::ListItem(ListItem::SubList { element }) => {
                    collect_page_references(
                        std::slice::from_ref(element),
                        references,
                        seen,
                    );
                }
                PartialElement::TableRow(row) => {
                    for cell in &row.cells {
                        collect_page_references(&cell.elements, references, seen);
                    }
                }
                PartialElement::TableCell(cell) => {
                    collect_page_references(&cell.elements, references, seen);
                }
                PartialElement::Tab(tab) => {
                    collect_page_references(&tab.elements, references, seen);
                }
                PartialElement::RubyText(ruby_text) => {
                    collect_page_references(&ruby_text.elements, references, seen);
                }
                PartialElement::WikidotEmptyInlineOwner
                | PartialElement::InlineSizeOpen(..)
                | PartialElement::InlineSizeClose(_)
                | PartialElement::InlineSpanOpen(..)
                | PartialElement::InlineSpanClose(_)
                | PartialElement::InlineFormatOpen(_)
                | PartialElement::InlineFormatClose(_)
                | PartialElement::InlineColorOpen(..)
                | PartialElement::InlineColorClose
                | PartialElement::InlineAnchorOpen(..)
                | PartialElement::InlineAnchorClose(_) => {}
            },
            _ => {}
        }
    }
}

fn collect_user_references(
    elements: &[Element<'_>],
    references: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    for element in elements {
        if let Element::User { name, .. } = element {
            let name = name.to_string();
            if seen.insert(name.clone()) {
                references.push(name);
            }
        }

        match element {
            Element::Container(container) => {
                collect_user_references(container.elements(), references, seen);
            }
            Element::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        collect_user_references(&cell.elements, references, seen);
                    }
                }
            }
            Element::TabView(tabs) => {
                for item in tabs {
                    collect_user_references(item.elements(), references, seen);
                }
            }
            Element::Anchor { elements, .. }
            | Element::Collapsible { elements, .. }
            | Element::Color { elements, .. }
            | Element::Include { elements, .. } => {
                collect_user_references(elements, references, seen);
            }
            Element::List { items, .. } => {
                for item in items {
                    match item {
                        ListItem::Elements { elements, .. } => {
                            collect_user_references(elements, references, seen);
                        }
                        ListItem::SubList { element } => {
                            collect_user_references(
                                std::slice::from_ref(element),
                                references,
                                seen,
                            );
                        }
                    }
                }
            }
            Element::DefinitionList(items) => {
                for item in items {
                    collect_user_references(&item.key_elements, references, seen);
                    collect_user_references(&item.value_elements, references, seen);
                }
            }
            Element::Partial(partial) => match partial {
                PartialElement::ListItem(ListItem::Elements { elements, .. }) => {
                    collect_user_references(elements, references, seen);
                }
                PartialElement::ListItem(ListItem::SubList { element }) => {
                    collect_user_references(
                        std::slice::from_ref(element),
                        references,
                        seen,
                    );
                }
                PartialElement::TableRow(row) => {
                    for cell in &row.cells {
                        collect_user_references(&cell.elements, references, seen);
                    }
                }
                PartialElement::TableCell(cell) => {
                    collect_user_references(&cell.elements, references, seen);
                }
                PartialElement::Tab(tab) => {
                    collect_user_references(&tab.elements, references, seen);
                }
                PartialElement::RubyText(ruby_text) => {
                    collect_user_references(&ruby_text.elements, references, seen);
                }
                PartialElement::WikidotEmptyInlineOwner
                | PartialElement::InlineSizeOpen(..)
                | PartialElement::InlineSizeClose(_)
                | PartialElement::InlineSpanOpen(..)
                | PartialElement::InlineSpanClose(_)
                | PartialElement::InlineFormatOpen(_)
                | PartialElement::InlineFormatClose(_)
                | PartialElement::InlineColorOpen(..)
                | PartialElement::InlineColorClose
                | PartialElement::InlineAnchorOpen(..)
                | PartialElement::InlineAnchorClose(_) => {}
            },
            _ => {}
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod deep_stack_tests {
    use super::*;
    use crate::tree::{LinkLabel, LinkLocation, LinkType};
    use std::borrow::Cow;

    fn deep_container_tree(depth: usize) -> SyntaxTree<'static> {
        let leaf = Element::Container(Container::new(
            ContainerType::Span,
            vec![
                Element::Link {
                    ltype: LinkType::Page,
                    link: LinkLocation::Page(PageRef::page_only("deep-page")),
                    label: LinkLabel::Slug(Cow::Borrowed("deep-page")),
                    target: None,
                },
                Element::User {
                    name: Cow::Borrowed("deep-user"),
                    show_avatar: false,
                },
            ],
            AttributeMap::new(),
        ));
        let mut element = leaf;
        for _ in 0..depth {
            element = Element::Container(Container::new(
                ContainerType::Div,
                vec![element],
                AttributeMap::new(),
            ));
        }
        SyntaxTree {
            elements: vec![element],
            wikitext_len: depth * 7 + 1,
            ..SyntaxTree::default()
        }
    }

    fn deep_list_tree(depth: usize) -> SyntaxTree<'static> {
        let mut element = Element::Container(Container::new(
            ContainerType::Span,
            vec![
                Element::Link {
                    ltype: LinkType::Page,
                    link: LinkLocation::Page(PageRef::page_only("deep-page")),
                    label: LinkLabel::Slug(Cow::Borrowed("deep-page")),
                    target: None,
                },
                Element::User {
                    name: Cow::Borrowed("deep-user"),
                    show_avatar: false,
                },
            ],
            AttributeMap::new(),
        ));
        for _ in 0..depth {
            element = Element::List {
                ltype: ListType::Bullet,
                attributes: AttributeMap::new(),
                items: vec![ListItem::SubList {
                    element: Box::new(element),
                }],
            };
        }
        SyntaxTree {
            elements: vec![element],
            wikitext_len: depth * 8 + 1,
            ..SyntaxTree::default()
        }
    }

    #[test]
    fn syntax_tree_serialization_preserves_the_existing_json_shape() {
        let mut tree = SyntaxTree {
            wikitext_len: 999,
            ..SyntaxTree::default()
        };
        assert_eq!(
            serde_json::to_string(&tree).expect("serialize empty syntax tree"),
            r#"{"elements":[]}"#,
        );

        tree.needs_footnote_block = true;
        tree.html_blocks.push(Cow::Borrowed("<b>x</b>"));
        assert_eq!(
            serde_json::to_value(&tree).expect("serialize syntax tree fields"),
            serde_json::json!({
                "elements": [],
                "html-blocks": ["<b>x</b>"],
                "needs-footnote-block": true,
            }),
        );
    }

    #[test]
    fn deep_owned_clone_and_reference_scans_do_not_inherit_a_small_caller_stack() {
        const DEPTH: usize = 768;
        for tree in [deep_container_tree(DEPTH), deep_list_tree(DEPTH)] {
            std::thread::scope(|scope| {
                std::thread::Builder::new()
                    .name("ftml-small-tree-caller".to_owned())
                    .stack_size(256 * 1024)
                    .spawn_scoped(scope, || {
                        let serialized = serde_json::to_string(&tree)
                            .expect("deep tree must serialize on the small caller stack");
                        assert!(serialized.contains("deep-page"));
                        let cloned_tree = tree.clone();
                        assert_eq!(cloned_tree.elements.len(), 1);
                        let owned_tree = tree.to_owned();
                        assert_eq!(owned_tree.elements.len(), 1);
                        let owned_element = tree.elements[0].to_owned();
                        assert_eq!(
                            tree.page_references(),
                            [PageRef::page_only("deep-page")]
                        );
                        assert_eq!(tree.user_references(), ["deep-user"]);
                        drop(owned_element);
                        drop(owned_tree);
                        drop(cloned_tree);
                    })
                    .expect("start small tree caller")
                    .join()
                    .expect("deep tree operations must not overflow the caller stack");
            });
        }
    }
}

#[test]
fn borrowed_to_owned() {
    use std::mem;

    let tree_1: SyntaxTree<'_> = SyntaxTree::default();
    let tree_2: SyntaxTree<'static> = tree_1.to_owned();

    mem::drop(tree_1);

    let tree_3: SyntaxTree<'static> = tree_2.clone();

    mem::drop(tree_3);
}

#[test]
fn page_references_collect_nested_rendered_locations_once() {
    fn page_link(page: PageRef) -> Element<'static> {
        Element::Link {
            ltype: LinkType::Page,
            link: LinkLocation::Page(page),
            label: LinkLabel::Slug(cow!("label")),
            target: None,
        }
    }

    let nested = PageRef::page_only("nested#section");
    let footnote = PageRef::page_and_site("other-site", "footnote");
    let bibliography = PageRef::page_only("bibliography");
    let mut bibliography_list = BibliographyList::new();
    let mut bibliography_items = Bibliography::new();
    bibliography_items.add(cow!("source"), vec![page_link(bibliography.clone())]);
    bibliography_list.push(bibliography_items);

    let tree = SyntaxTree {
        elements: vec![
            Element::Container(Container::new(
                ContainerType::Div,
                vec![page_link(nested.clone())],
                AttributeMap::new(),
            )),
            page_link(nested.clone()),
            Element::Link {
                ltype: LinkType::Direct,
                link: LinkLocation::Url(cow!("https://example.com")),
                label: LinkLabel::Text(cow!("external")),
                target: None,
            },
        ],
        footnotes: vec![vec![page_link(footnote.clone())]],
        bibliographies: bibliography_list,
        ..SyntaxTree::default()
    };

    assert_eq!(tree.page_references(), vec![nested, footnote, bibliography],);
}

#[test]
fn user_references_collect_nested_names_once() {
    fn user(name: &'static str) -> Element<'static> {
        Element::User {
            name: cow!(name),
            show_avatar: false,
        }
    }

    let tree = SyntaxTree {
        elements: vec![
            Element::Container(Container::new(
                ContainerType::Div,
                vec![user("SYSTEM")],
                AttributeMap::new(),
            )),
            user("SYSTEM"),
        ],
        footnotes: vec![vec![user("account-name")]],
        ..SyntaxTree::default()
    };

    assert_eq!(tree.user_references(), ["SYSTEM", "account-name"]);
}
