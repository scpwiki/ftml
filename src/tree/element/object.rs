/*
 * tree/element/object.rs
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

use crate::data::PageRef;
use crate::delayed::DelayedElement;
use crate::tree::clone::*;
use crate::tree::{
    Alignment, AnchorTarget, AttributeMap, ClearFloat, CodeBlock, Container, DateItem,
    DefinitionListItem, Embed, EmbedVideo, FileSource, FloatAlignment, Gallery,
    ImageSource, LinkLabel, LinkLocation, LinkType, ListItem, ListType, Module,
    PartialElement, SocialButtons, StandaloneButton, TabViewItem, Table, VariableMap,
};
use crate::tree::{
    element_requires_bounded_tree_stack, on_bounded_tree_stack, run_on_bounded_tree_stack,
};
use ref_map::*;
use std::borrow::Cow;
use std::num::NonZeroU32;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case", tag = "element", content = "data")]
pub enum Element<'t> {
    Container(Container<'t>),

    /// A Wikidot module being invoked, along with its arguments.
    ///
    /// These modules require some kind of processing by backend software,
    /// so are represented in module forum rather than as elements to be
    /// directly rendered.
    Module(Module<'t>),

    Text(Cow<'t, str>),

    /// Raw text.
    ///
    /// This should be formatted exactly as listed.
    /// For instance, spaces being rendered to HTML should
    /// produce a `&nbsp;`.
    Raw(Cow<'t, str>),

    /// A wikitext variable.
    ///
    /// During rendering, this will be replaced with its actual value,
    /// as appropriate to the context.
    Variable(Cow<'t, str>),

    /// An element indicating an email.
    ///
    /// Whether this should become a clickable href link or just text
    /// is up to the render implementation.
    Email(Cow<'t, str>),

    #[serde(skip)]
    Delayed(DelayedElement<'t>),

    /// An authored physical-line content-section separator.
    ///
    /// This remains typed in every layout so runtime consumers can split
    /// sections without reparsing source. Only the Wikidot HTML renderer emits
    /// the legacy hidden div.
    ContentSeparator,

    Table(Table<'t>),

    TabView(Vec<TabViewItem<'t>>),

    StandaloneButton(StandaloneButton<'t>),

    SocialButtons(SocialButtons),

    EmbedVideo(EmbedVideo<'t>),

    Gallery(Gallery<'t>),

    /// An element representing an arbitrary anchor.
    ///
    /// This is distinct from link in that it maps to HTML `<a>`,
    /// and does not necessarily mean a link to some other URL.
    Anchor {
        target: Option<AnchorTarget>,
        attributes: AttributeMap<'t>,
        elements: Vec<Element<'t>>,
    },

    AnchorName(Cow<'t, str>),

    /// An element linking to a different page.
    ///
    /// The "label" field is an optional field denoting what the link should
    /// display.
    ///
    /// The "link" field is either a page reference (relative URL) or full URL.
    ///
    /// The "ltype" field tells what kind of link produced this element.
    Link {
        #[serde(rename = "type")]
        ltype: LinkType,
        link: LinkLocation<'t>,
        label: LinkLabel<'t>,
        target: Option<AnchorTarget>,
    },

    FileLink {
        file: Cow<'t, str>,
        label: Cow<'t, str>,
    },

    /// An element representing an image and its associated metadata.
    ///
    /// The "source" field is the link to the image itself.
    ///
    /// The "link" field is what the `<a>` points to, when the user clicks on the image.
    Image {
        source: ImageSource<'t>,
        link: Option<LinkLocation<'t>>,
        alignment: Option<FloatAlignment>,
        attributes: AttributeMap<'t>,
        target: Option<AnchorTarget>,
    },

    Audio {
        source: FileSource<'t>,
        alignment: Option<FloatAlignment>,
        attributes: AttributeMap<'t>,
    },

    Video {
        source: FileSource<'t>,
        alignment: Option<FloatAlignment>,
        attributes: AttributeMap<'t>,
    },

    List {
        #[serde(rename = "type")]
        ltype: ListType,
        attributes: AttributeMap<'t>,
        items: Vec<ListItem<'t>>,
    },

    DefinitionList(Vec<DefinitionListItem<'t>>),

    RadioButton {
        name: Cow<'t, str>,
        checked: bool,
        attributes: AttributeMap<'t>,
    },

    /// A checkbox.
    ///
    /// The "checked" field determines if the checkbox starts checked or not.
    CheckBox {
        checked: bool,
        attributes: AttributeMap<'t>,
    },

    #[serde(rename_all = "kebab-case")]
    Collapsible {
        elements: Vec<Element<'t>>,
        /// First element rendered outside Wikidot's collapsible content div.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unfolded_tail_start: Option<usize>,
        /// Inline span/size scopes that cross this Wikidot collapsible also
        /// wrap its folded and unfolded handles. These are renderer-owned
        /// compatibility wrappers, not authored collapsible attributes.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        wikidot_label_spans: Vec<AttributeMap<'t>>,
        attributes: AttributeMap<'t>,
        start_open: bool,
        show_text: Option<Cow<'t, str>>,
        hide_text: Option<Cow<'t, str>>,
        show_top: bool,
        show_bottom: bool,
    },

    TableOfContents {
        attributes: AttributeMap<'t>,
        align: Option<Alignment>,
    },

    /// A footnote reference.
    ///
    /// This specifies that a `[[footnote]]` was here, and that a clickable
    /// link to the footnote block should be added.
    ///
    /// The one-based index of the corresponding entry in the syntax tree's
    /// `footnotes` list. Keeping the index on the reference preserves global
    /// numbering when Wikidot suppresses an earlier body reference.
    Footnote(usize),

    /// A footnote block, containing all the footnotes from throughout the page.
    ///
    /// If a `[[footnoteblock]]` is not added somewhere in the content of the page,
    /// then it is automatically appended to the end of the syntax tree.
    FootnoteBlock {
        title: Option<Cow<'t, str>>,
        hide: bool,
    },

    /// A citation of a bibliography element, invoked via `((bibcite ...))`.
    ///
    /// The `brackets` field tells whether the resultant HTML should be surrounded
    /// in `[..]`, which is not very easily possible when using `[[bibcite ...]]`.
    BibliographyCite {
        label: Cow<'t, str>,
        brackets: bool,
        inline: bool,
    },

    /// A bibliography block, containing all the cited items from throughout the page.
    ///
    /// The `index` field is the zero-indexed value of which bibliography block this is.
    BibliographyBlock {
        index: usize,
        title: Option<Cow<'t, str>>,
        hide: bool,
    },

    #[serde(rename_all = "kebab-case")]
    User {
        name: Cow<'t, str>,
        show_avatar: bool,
    },

    Date {
        value: DateItem,
        format: Option<Cow<'t, str>>,
        hover: bool,
    },

    Color {
        color: Cow<'t, str>,
        background: bool,
        elements: Vec<Element<'t>>,
    },

    Code(CodeBlock<'t>),

    #[serde(rename_all = "kebab-case")]
    Math {
        name: Option<Cow<'t, str>>,
        latex_source: Cow<'t, str>,
    },

    #[serde(rename_all = "kebab-case")]
    MathInline {
        latex_source: Cow<'t, str>,
    },

    EquationReference(Cow<'t, str>),

    Embed(Embed<'t>),

    Html {
        contents: Cow<'t, str>,
        attributes: AttributeMap<'t>,
    },

    Iframe {
        url: Cow<'t, str>,
        attributes: AttributeMap<'t>,
    },

    #[serde(rename_all = "kebab-case")]
    Include {
        paragraph_safe: bool,
        variables: VariableMap<'t>,
        location: PageRef,
        elements: Vec<Element<'t>>,
    },

    Style(Cow<'t, str>),

    LineBreak,

    LineBreaks(NonZeroU32),

    ClearFloat(ClearFloat),

    HorizontalRule,

    /// A partial element.
    ///
    /// This will not appear in final syntax trees, but exists to
    /// facilitate parsing of complicated structures.
    ///
    /// See [`WJ-816`](https://scuttle.atlassian.net/browse/WJ-816).
    Partial(PartialElement<'t>),
}

impl Element<'_> {
    #[inline]
    pub const fn is_content_separator(&self) -> bool {
        matches!(self, Self::ContentSeparator)
    }

    /// Determines if the element is "unintentional whitespace".
    ///
    /// Specifically, it returns true if the element is:
    /// * `Element::LineBreak`
    /// * `Element::Text` where the contents all have the Unicode property `White_Space`.
    ///
    /// This does not count `Element::LineBreaks` because it is produced intentionally
    /// via `[[lines]]` rather than extra whitespace in between syntactical elements.
    pub fn is_whitespace(&self) -> bool {
        match self {
            Element::LineBreak => true,
            Element::Text(string) if string.chars().all(|c| c.is_whitespace()) => true,
            _ => false,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Element::Container(container) => container.ctype().name(),
            Element::Module(module) => module.name(),
            Element::Text(_) => "Text",
            Element::Raw(_) => "Raw",
            Element::Variable(_) => "Variable",
            Element::Email(_) => "Email",
            Element::Delayed(_) => "Delayed",
            Element::ContentSeparator => "ContentSeparator",
            Element::Table(_) => "Table",
            Element::TabView(_) => "TabView",
            Element::StandaloneButton(_) => "StandaloneButton",
            Element::SocialButtons(_) => "SocialButtons",
            Element::EmbedVideo(_) => "EmbedVideo",
            Element::Gallery(_) => "Gallery",
            Element::Anchor { .. } => "Anchor",
            Element::AnchorName(_) => "AnchorName",
            Element::Link { .. } => "Link",
            Element::FileLink { .. } => "FileLink",
            Element::Image { .. } => "Image",
            Element::Audio { .. } => "Audio",
            Element::Video { .. } => "Video",
            Element::List { .. } => "List",
            Element::DefinitionList(_) => "DefinitionList",
            Element::RadioButton { .. } => "RadioButton",
            Element::CheckBox { .. } => "CheckBox",
            Element::Collapsible { .. } => "Collapsible",
            Element::TableOfContents { .. } => "TableOfContents",
            Element::Footnote(_) => "Footnote",
            Element::FootnoteBlock { .. } => "FootnoteBlock",
            Element::BibliographyCite { .. } => "BibliographyCite",
            Element::BibliographyBlock { .. } => "BibliographyBlock",
            Element::User { .. } => "User",
            Element::Date { .. } => "Date",
            Element::Color { .. } => "Color",
            Element::Code { .. } => "Code",
            Element::Math { .. } => "Math",
            Element::MathInline { .. } => "MathInline",
            Element::EquationReference(_) => "EquationReference",
            Element::Embed(_) => "Embed",
            Element::Html { .. } => "HTML",
            Element::Iframe { .. } => "Iframe",
            Element::Include { .. } => "Include",
            Element::Style(_) => "Style",
            Element::LineBreak => "LineBreak",
            Element::LineBreaks { .. } => "LineBreaks",
            Element::ClearFloat(_) => "ClearFloat",
            Element::HorizontalRule => "HorizontalRule",
            Element::Partial(partial) => partial.name(),
        }
    }

    pub(crate) fn offset_footnote_indices(&mut self, offset: usize) {
        if offset == 0 {
            return;
        }

        let offset_elements = |elements: &mut [Element<'_>]| {
            for element in elements {
                element.offset_footnote_indices(offset);
            }
        };

        match self {
            Element::Container(container) => offset_elements(container.elements_mut()),
            Element::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        offset_elements(&mut cell.elements);
                    }
                }
            }
            Element::TabView(tabs) => {
                for item in tabs {
                    offset_elements(item.elements_mut());
                }
            }
            Element::StandaloneButton(_) => {}
            Element::SocialButtons(_) => {}
            Element::EmbedVideo(_) => {}
            Element::Gallery(_) => {}
            Element::Anchor { elements, .. }
            | Element::Collapsible { elements, .. }
            | Element::Color { elements, .. }
            | Element::Include { elements, .. } => offset_elements(elements),
            Element::List { items, .. } => {
                for item in items {
                    match item {
                        ListItem::Elements { elements, .. } => offset_elements(elements),
                        ListItem::SubList { element } => {
                            element.offset_footnote_indices(offset);
                        }
                    }
                }
            }
            Element::DefinitionList(items) => {
                for item in items {
                    offset_elements(&mut item.key_elements);
                    offset_elements(&mut item.value_elements);
                }
            }
            Element::Footnote(index) => *index += offset,
            Element::Partial(partial) => match partial {
                PartialElement::ListItem(ListItem::Elements { elements, .. }) => {
                    offset_elements(elements);
                }
                PartialElement::ListItem(ListItem::SubList { element }) => {
                    element.offset_footnote_indices(offset);
                }
                PartialElement::TableRow(row) => {
                    for cell in &mut row.cells {
                        offset_elements(&mut cell.elements);
                    }
                }
                PartialElement::TableCell(cell) => offset_elements(&mut cell.elements),
                PartialElement::Tab(tab) => offset_elements(&mut tab.elements),
                PartialElement::RubyText(ruby_text) => {
                    offset_elements(&mut ruby_text.elements);
                }
                PartialElement::WikidotEmptyInlineOwner
                | PartialElement::InlineSizeOpen(_)
                | PartialElement::InlineSizeClose(_)
                | PartialElement::InlineSpanOpen(_)
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

    /// Determines if this element type is able to be embedded in a paragraph.
    ///
    /// It does *not* look into the interiors of the element, it only does a
    /// surface-level check.
    ///
    /// This is to avoid making the call very expensive, but for a complete
    /// understanding of the paragraph requirements, see the `Elements` return.
    ///
    /// See <https://developer.mozilla.org/en-US/docs/Web/Guide/HTML/Content_categories#phrasing_content>
    pub fn paragraph_safe(&self) -> bool {
        match self {
            Element::Container(container) => container.ctype().paragraph_safe(),
            Element::Module(_) => false,
            Element::Text(_)
            | Element::Raw(_)
            | Element::Variable(_)
            | Element::Email(_) => true,
            Element::Delayed(_) => true,
            Element::ContentSeparator => false,
            Element::Table(_) => false,
            Element::TabView(_) => false,
            Element::StandaloneButton(_) => true,
            Element::SocialButtons(_) => true,
            Element::EmbedVideo(_) => false,
            Element::Gallery(_) => false,
            Element::Anchor { .. }
            | Element::AnchorName(_)
            | Element::Link { .. }
            | Element::FileLink { .. } => true,
            Element::Image { .. } => true,
            Element::Audio { .. } => true,
            Element::Video { .. } => true,
            Element::List { .. } => false,
            Element::DefinitionList(_) => false,
            Element::RadioButton { .. } | Element::CheckBox { .. } => true,
            Element::Collapsible { .. } => false,
            Element::TableOfContents { .. } => false,
            Element::Footnote(_) => true,
            Element::FootnoteBlock { .. } => false,
            Element::BibliographyCite { .. } => true,
            Element::BibliographyBlock { .. } => false,
            Element::User { .. } => true,
            Element::Date { .. } => true,
            Element::Color { .. } => true,
            Element::Code { .. } => false,
            Element::Math { .. } => false,
            Element::MathInline { .. } => true,
            Element::EquationReference(_) => true,
            Element::Embed(_) => false,
            Element::Html { .. } | Element::Iframe { .. } => false,
            Element::Include { paragraph_safe, .. } => *paragraph_safe,
            Element::Style(_) => false,
            Element::LineBreak | Element::LineBreaks { .. } => true,
            Element::ClearFloat(_) => false,
            Element::HorizontalRule => false,
            Element::Partial(partial) => partial.paragraph_safe(),
        }
    }

    /// Deep-clones the object, making it an owned version.
    ///
    /// Note that `.to_owned()` on `Cow` just copies the pointer,
    /// it doesn't make an `Cow::Owned(_)` version like its name
    /// suggests.
    pub fn to_owned(&self) -> Element<'static> {
        if on_bounded_tree_stack() {
            return self.to_owned_on_current_stack();
        }
        if element_requires_bounded_tree_stack(self) {
            run_on_bounded_tree_stack("ftml-element-clone", || {
                self.to_owned_on_current_stack()
            })
        } else {
            self.to_owned_on_current_stack()
        }
    }

    pub(crate) fn to_owned_on_current_stack(&self) -> Element<'static> {
        match self {
            Element::Container(container) => Element::Container(container.to_owned()),
            Element::Module(module) => Element::Module(module.to_owned()),
            Element::Text(text) => Element::Text(string_to_owned(text)),
            Element::Raw(text) => Element::Raw(string_to_owned(text)),
            Element::Variable(name) => Element::Variable(string_to_owned(name)),
            Element::Email(email) => Element::Email(string_to_owned(email)),
            Element::Delayed(delayed) => Element::Delayed(delayed.to_owned()),
            Element::ContentSeparator => Element::ContentSeparator,
            Element::Table(table) => Element::Table(table.to_owned()),
            Element::TabView(tabs) => {
                Element::TabView(tabs.iter().map(|item| item.to_owned()).collect())
            }
            Element::StandaloneButton(button) => {
                Element::StandaloneButton(button.to_owned())
            }
            Element::SocialButtons(social) => Element::SocialButtons(social.clone()),
            Element::EmbedVideo(embed_video) => {
                Element::EmbedVideo(embed_video.to_owned())
            }
            Element::Gallery(gallery) => Element::Gallery(gallery.to_owned()),
            Element::Anchor {
                target,
                attributes,
                elements,
            } => Element::Anchor {
                target: *target,
                attributes: attributes.to_owned(),
                elements: elements_to_owned(elements),
            },
            Element::AnchorName(name) => Element::AnchorName(string_to_owned(name)),
            Element::Link {
                ltype,
                link,
                label,
                target,
            } => Element::Link {
                ltype: *ltype,
                link: link.to_owned(),
                label: label.to_owned(),
                target: *target,
            },
            Element::FileLink { file, label } => Element::FileLink {
                file: string_to_owned(file),
                label: string_to_owned(label),
            },
            Element::List {
                ltype,
                attributes,
                items,
            } => Element::List {
                ltype: *ltype,
                attributes: attributes.to_owned(),
                items: list_items_to_owned(items),
            },
            Element::Image {
                source,
                link,
                alignment,
                attributes,
                target,
            } => Element::Image {
                source: source.to_owned(),
                link: link.ref_map(|link| link.to_owned()),
                alignment: *alignment,
                attributes: attributes.to_owned(),
                target: *target,
            },
            Element::Audio {
                source,
                alignment,
                attributes,
            } => Element::Audio {
                source: source.to_owned(),
                alignment: *alignment,
                attributes: attributes.to_owned(),
            },
            Element::Video {
                source,
                alignment,
                attributes,
            } => Element::Video {
                source: source.to_owned(),
                alignment: *alignment,
                attributes: attributes.to_owned(),
            },
            Element::DefinitionList(items) => Element::DefinitionList(
                items.iter().map(|item| item.to_owned()).collect(),
            ),
            Element::RadioButton {
                name,
                checked,
                attributes,
            } => Element::RadioButton {
                name: string_to_owned(name),
                checked: *checked,
                attributes: attributes.to_owned(),
            },
            Element::CheckBox {
                checked,
                attributes,
            } => Element::CheckBox {
                checked: *checked,
                attributes: attributes.to_owned(),
            },
            Element::Collapsible {
                elements,
                unfolded_tail_start,
                wikidot_label_spans,
                attributes,
                start_open,
                show_text,
                hide_text,
                show_top,
                show_bottom,
            } => Element::Collapsible {
                elements: elements_to_owned(elements),
                unfolded_tail_start: *unfolded_tail_start,
                wikidot_label_spans: wikidot_label_spans
                    .iter()
                    .map(AttributeMap::to_owned)
                    .collect(),
                attributes: attributes.to_owned(),
                start_open: *start_open,
                show_text: option_string_to_owned(show_text),
                hide_text: option_string_to_owned(hide_text),
                show_top: *show_top,
                show_bottom: *show_bottom,
            },
            Element::TableOfContents { align, attributes } => Element::TableOfContents {
                align: *align,
                attributes: attributes.to_owned(),
            },
            Element::Footnote(index) => Element::Footnote(*index),
            Element::FootnoteBlock { title, hide } => Element::FootnoteBlock {
                title: option_string_to_owned(title),
                hide: *hide,
            },
            Element::BibliographyCite {
                label,
                brackets,
                inline,
            } => Element::BibliographyCite {
                label: string_to_owned(label),
                brackets: *brackets,
                inline: *inline,
            },
            Element::BibliographyBlock { index, title, hide } => {
                Element::BibliographyBlock {
                    index: *index,
                    title: option_string_to_owned(title),
                    hide: *hide,
                }
            }
            Element::User { name, show_avatar } => Element::User {
                name: string_to_owned(name),
                show_avatar: *show_avatar,
            },
            Element::Date {
                value,
                format,
                hover,
            } => Element::Date {
                value: *value,
                format: option_string_to_owned(format),
                hover: *hover,
            },
            Element::Color {
                color,
                background,
                elements,
            } => Element::Color {
                color: string_to_owned(color),
                background: *background,
                elements: elements_to_owned(elements),
            },
            Element::Code(code_block) => Element::Code(code_block.to_owned()),
            Element::Math { name, latex_source } => Element::Math {
                name: option_string_to_owned(name),
                latex_source: string_to_owned(latex_source),
            },
            Element::MathInline { latex_source } => Element::MathInline {
                latex_source: string_to_owned(latex_source),
            },
            Element::EquationReference(name) => {
                Element::EquationReference(string_to_owned(name))
            }
            Element::Embed(embed) => Element::Embed(embed.to_owned()),
            Element::Html {
                contents,
                attributes,
            } => Element::Html {
                contents: string_to_owned(contents),
                attributes: attributes.to_owned(),
            },
            Element::Iframe { url, attributes } => Element::Iframe {
                url: string_to_owned(url),
                attributes: attributes.to_owned(),
            },
            Element::Include {
                paragraph_safe,
                variables,
                location,
                elements,
            } => Element::Include {
                paragraph_safe: *paragraph_safe,
                variables: string_map_to_owned(variables),
                location: location.to_owned(),
                elements: elements_to_owned(elements),
            },
            Element::Style(css) => Element::Style(string_to_owned(css)),
            Element::LineBreak => Element::LineBreak,
            Element::LineBreaks(amount) => Element::LineBreaks(*amount),
            Element::ClearFloat(clear_float) => Element::ClearFloat(*clear_float),
            Element::HorizontalRule => Element::HorizontalRule,
            Element::Partial(partial) => Element::Partial(partial.to_owned()),
        }
    }
}
