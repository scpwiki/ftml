/*
 * tree/container.rs
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

//! Representation of generic syntax elements which wrap other elements.

use super::clone::elements_to_owned;
use super::{Alignment, AttributeMap, Element, Heading, HtmlTag};
use crate::layout::Layout;
use crate::next_index::{NextIndex, TableOfContentsIndex};
use strum_macros::IntoStaticStr;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct Container<'t> {
    #[serde(rename = "type")]
    ctype: ContainerType,
    attributes: AttributeMap<'t>,
    elements: Vec<Element<'t>>,
}

impl<'t> Container<'t> {
    #[inline]
    pub fn new(
        ctype: ContainerType,
        elements: Vec<Element<'t>>,
        attributes: AttributeMap<'t>,
    ) -> Self {
        Container {
            ctype,
            attributes,
            elements,
        }
    }

    #[inline]
    pub fn ctype(&self) -> ContainerType {
        self.ctype
    }

    #[inline]
    pub fn elements(&self) -> &[Element<'t>] {
        &self.elements
    }

    #[inline]
    pub(crate) fn elements_mut(&mut self) -> &mut Vec<Element<'t>> {
        &mut self.elements
    }

    #[inline]
    pub fn attributes(&self) -> &AttributeMap<'t> {
        &self.attributes
    }

    #[inline]
    pub fn attributes_mut(&mut self) -> &mut AttributeMap<'t> {
        &mut self.attributes
    }

    pub fn to_owned(&self) -> Container<'static> {
        Container {
            ctype: self.ctype,
            attributes: self.attributes.to_owned(),
            elements: elements_to_owned(&self.elements),
        }
    }
}

impl<'t> From<Container<'t>> for Vec<Element<'t>> {
    fn from(container: Container<'t>) -> Vec<Element<'t>> {
        container.elements
    }
}

#[derive(
    Serialize, Deserialize, IntoStaticStr, Debug, Copy, Clone, Hash, PartialEq, Eq,
)]
#[serde(rename_all = "kebab-case")]
pub enum ContainerType {
    Bold,
    Italics,
    Underline,
    Superscript,
    Subscript,
    Strikethrough,
    Monospace,
    Span,
    HeadingLabel,
    Div,
    Mark,
    Blockquote,
    Note,
    Insertion,
    Deletion,
    Hidden,
    Invisible,
    Size,
    Ruby,
    RubyText,
    Paragraph,
    Align(Alignment),
    Header(Heading),
}

impl ContainerType {
    #[inline]
    pub fn name(self) -> &'static str {
        self.into()
    }

    #[inline]
    pub fn html_tag(
        self,
        layout: Layout,
        indexer: &mut dyn NextIndex<TableOfContentsIndex>,
    ) -> HtmlTag {
        match self {
            ContainerType::Bold => HtmlTag::new("strong"),
            ContainerType::Italics => HtmlTag::new("em"),
            ContainerType::Underline => match layout {
                Layout::Wikidot => {
                    HtmlTag::with_style("span", "text-decoration: underline;")
                }
                Layout::Wikijump => HtmlTag::new("u"),
            },
            ContainerType::Superscript => HtmlTag::new("sup"),
            ContainerType::Subscript => HtmlTag::new("sub"),
            ContainerType::Strikethrough => match layout {
                Layout::Wikidot => {
                    HtmlTag::with_style("span", "text-decoration: line-through;")
                }
                Layout::Wikijump => HtmlTag::new("s"),
            },
            ContainerType::Monospace => match layout {
                Layout::Wikidot => HtmlTag::new("tt"),
                Layout::Wikijump => HtmlTag::with_class("code", "wj-monospace"),
            },
            ContainerType::Span => HtmlTag::new("span"),
            ContainerType::HeadingLabel => HtmlTag::new("span"),
            ContainerType::Div => HtmlTag::new("div"),
            ContainerType::Mark => HtmlTag::new("mark"),
            ContainerType::Blockquote => HtmlTag::new("blockquote"),
            ContainerType::Note => match layout {
                Layout::Wikidot => HtmlTag::with_class("div", "wiki-note"),
                Layout::Wikijump => HtmlTag::with_class("div", "wj-note"),
            },
            ContainerType::Insertion => HtmlTag::new("ins"),
            ContainerType::Deletion => HtmlTag::new("del"),
            ContainerType::Hidden => HtmlTag::with_class("span", "wj-hidden"),
            ContainerType::Invisible => HtmlTag::with_class("span", "wj-invisible"),
            ContainerType::Size => HtmlTag::new("span"),
            ContainerType::Ruby => HtmlTag::new("ruby"),
            ContainerType::RubyText => HtmlTag::new("rt"),
            ContainerType::Paragraph => HtmlTag::new("p"),
            ContainerType::Align(alignment) => match layout {
                Layout::Wikidot => HtmlTag::with_style("div", alignment.wd_html_style()),
                Layout::Wikijump => HtmlTag::with_class("div", alignment.wj_html_class()),
            },
            ContainerType::Header(heading) => heading.html_tag(indexer),
        }
    }

    /// Determines if this container type is able to be embedded in a paragraph.
    ///
    /// See `Element::paragraph_safe()`, as the same caveats apply.
    #[inline]
    pub fn paragraph_safe(self) -> bool {
        match self {
            ContainerType::Bold => true,
            ContainerType::Italics => true,
            ContainerType::Underline => true,
            ContainerType::Superscript => true,
            ContainerType::Subscript => true,
            ContainerType::Strikethrough => true,
            ContainerType::Monospace => true,
            ContainerType::Span => true,
            ContainerType::HeadingLabel => true,
            ContainerType::Div => false,
            ContainerType::Mark => true,
            ContainerType::Blockquote => false,
            ContainerType::Note => false,
            ContainerType::Insertion => true,
            ContainerType::Deletion => true,
            ContainerType::Hidden => true,
            ContainerType::Invisible => true,
            ContainerType::Size => true,
            ContainerType::Ruby => true,
            ContainerType::RubyText => true,
            ContainerType::Paragraph => false,
            ContainerType::Align(_) => false,
            ContainerType::Header(_) => false,
        }
    }
}

#[test]
fn container_helpers_preserve_attributes_and_elements() {
    let mut attributes = AttributeMap::new();
    assert!(attributes.insert("class", cow!("marker")));

    let element = Element::Text(cow!("contents"));
    let mut container =
        Container::new(ContainerType::Span, vec![element.clone()], attributes);

    assert_eq!(container.ctype(), ContainerType::Span);
    assert_eq!(container.elements(), std::slice::from_ref(&element));
    assert!(container.attributes().get().contains_key("class"));
    assert!(container.attributes_mut().insert("id", cow!("sample")));

    let owned = container.to_owned();
    assert_eq!(owned.ctype(), ContainerType::Span);
    assert_eq!(owned.elements(), std::slice::from_ref(&element));
    assert!(owned.attributes().get().contains_key("id"));

    let elements: Vec<Element<'_>> = container.into();
    assert_eq!(elements, vec![element]);
}

#[test]
fn container_type_html_tags_and_paragraph_safety() {
    use crate::layout::Layout;
    use crate::next_index::Incrementer;

    let cases = [
        (ContainerType::Bold, "strong", true),
        (ContainerType::Italics, "em", true),
        (ContainerType::Underline, "u", true),
        (ContainerType::Superscript, "sup", true),
        (ContainerType::Subscript, "sub", true),
        (ContainerType::Strikethrough, "s", true),
        (ContainerType::Monospace, "code", true),
        (ContainerType::Span, "span", true),
        (ContainerType::HeadingLabel, "span", true),
        (ContainerType::Div, "div", false),
        (ContainerType::Mark, "mark", true),
        (ContainerType::Blockquote, "blockquote", false),
        (ContainerType::Insertion, "ins", true),
        (ContainerType::Deletion, "del", true),
        (ContainerType::Hidden, "span", true),
        (ContainerType::Invisible, "span", true),
        (ContainerType::Size, "span", true),
        (ContainerType::Ruby, "ruby", true),
        (ContainerType::RubyText, "rt", true),
        (ContainerType::Paragraph, "p", false),
        (ContainerType::Align(Alignment::Left), "div", false),
        (
            ContainerType::Header(Heading {
                level: crate::tree::HeadingLevel::Two,
                has_toc: false,
            }),
            "h2",
            false,
        ),
    ];

    for (ctype, expected_tag, expected_paragraph_safe) in cases {
        let mut indexer = Incrementer::disabled();
        assert_eq!(
            ctype.html_tag(Layout::Wikijump, &mut indexer).tag(),
            expected_tag
        );
        assert_eq!(ctype.paragraph_safe(), expected_paragraph_safe);
    }

    let mut indexer = Incrementer::default();
    assert_eq!(
        ContainerType::Align(Alignment::Right).html_tag(Layout::Wikidot, &mut indexer),
        HtmlTag::with_style("div", "text-align: right;"),
    );

    let mut indexer = Incrementer::default();
    assert_eq!(
        ContainerType::Monospace.html_tag(Layout::Wikidot, &mut indexer),
        HtmlTag::new("tt"),
    );

    let mut indexer = Incrementer::default();
    assert_eq!(
        ContainerType::Strikethrough.html_tag(Layout::Wikidot, &mut indexer),
        HtmlTag::with_style("span", "text-decoration: line-through;"),
    );

    let mut indexer = Incrementer::default();
    assert_eq!(
        ContainerType::Align(Alignment::Right).html_tag(Layout::Wikijump, &mut indexer),
        HtmlTag::with_class("div", "wj-align-right"),
    );
}
