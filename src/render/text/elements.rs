/*
 * render/text/elements.rs
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

use super::TextContext;
use crate::tree::{
    CodeBlock, ContainerType, DefinitionListItem, Element, ListItem, Module,
    PartialElement, TabViewItem,
};

pub fn render_elements(ctx: &mut TextContext, elements: &[Element]) {
    debug!("Rendering elements (length {})", elements.len());

    for element in elements {
        render_element(ctx, element);
    }
}

pub fn render_element(ctx: &mut TextContext, element: &Element) {
    debug!("Rendering element {}", element.name());

    match element {
        Element::Container(container) => {
            let mut invisible = false;
            let add_newlines = match container.ctype() {
                ContainerType::Hidden => return,

                ContainerType::Invisible => {
                    ctx.enable_invisible();
                    invisible = true;

                    false
                }

                ContainerType::Div
                | ContainerType::Paragraph
                | ContainerType::Blockquote
                | ContainerType::Header(_) => true,

                ContainerType::RubyText => {
                    ctx.push('(');
                    false
                }

                _ => false,
            };

            if add_newlines {
                ctx.add_newline();
            }

            render_elements(ctx, container.elements());

            if container.ctype() == ContainerType::RubyText {
                ctx.push(')');
            }

            if add_newlines {
                ctx.add_newline();
            }

            if invisible {
                ctx.disable_invisible();
            }
        }
        Element::Module(Module::Runtime { name, .. })
            if ctx.settings().layout.legacy() =>
        {
            ctx.add_newline();
            ctx.push_str("[[module ");
            ctx.push_str(name);
            ctx.push_str(
                "]] No such module, please check available modules and fix this page.",
            );
            ctx.add_newline();
        }
        Element::Module(_) => {}
        Element::Text(text) | Element::Raw(text) | Element::Email(text) => {
            ctx.push_str(text);
        }
        Element::Delayed(_) => {
            panic!("unbound delayed element reached the text renderer")
        }
        Element::ContentSeparator => {}
        Element::Variable(name) => {
            let value = match ctx.variables().get(name) {
                Some(value) => str!(value),
                None => format!("{{${name}}}"),
            };

            debug!(
                "Rendering variable (name '{}', value {})",
                name.as_ref(),
                value,
            );
            ctx.push_str(&value);
        }
        Element::Table(table) => {
            if !ctx.ends_with_newline() {
                ctx.add_newline();
            }

            for row in &table.rows {
                for cell in &row.cells {
                    render_elements(ctx, &cell.elements);
                }

                ctx.add_newline();
            }

            ctx.add_newline();
        }
        Element::TabView(items) => {
            for item in items {
                match item {
                    TabViewItem::Tab(tab) => {
                        ctx.push_str(&tab.label);
                        ctx.add_newline();

                        render_elements(ctx, &tab.elements);
                        ctx.add_newline();
                    }
                    TabViewItem::Content(elements) => {
                        render_elements(ctx, elements);
                        ctx.add_newline();
                    }
                }
            }
        }
        Element::StandaloneButton(button) => ctx.push_str(&button.label),
        Element::SocialButtons(_) => {}
        Element::EmbedVideo(_) => {}
        Element::Gallery(_) => {}
        Element::Anchor { elements, .. } => render_elements(ctx, elements),
        Element::AnchorName(_) => {}
        Element::Link { link, label, .. } => {
            let site = ctx.info().site.as_ref();

            ctx.handle().get_link_label(site, link, label, |label| {
                ctx.push_str(label);
            });
        }
        Element::FileLink { label, .. } => ctx.push_str(label),
        Element::Image { .. } => {}
        Element::Audio { .. } => {}
        Element::Video { .. } => {}
        Element::List { items, .. } => {
            if !ctx.ends_with_newline() {
                ctx.add_newline();
            }

            for item in items {
                match item {
                    ListItem::SubList { element } => render_element(ctx, element),
                    ListItem::Elements { elements, .. } => {
                        if elements.is_empty() {
                            continue;
                        }

                        render_elements(ctx, elements);
                        if !ctx.ends_with_newline() {
                            ctx.add_newline();
                        }
                    }
                }
            }
        }
        Element::DefinitionList(items) => {
            for DefinitionListItem {
                key_elements,
                value_elements,
                ..
            } in items
            {
                render_elements(ctx, key_elements);
                ctx.push(' ');
                render_elements(ctx, value_elements);
                ctx.add_newline();
            }

            ctx.add_newline();
        }
        Element::RadioButton { .. } | Element::CheckBox { .. } => {}
        Element::Collapsible { elements, .. } => {
            render_elements(ctx, elements);
        }
        Element::TableOfContents { .. } => {}
        Element::Footnote(index) => {
            if ctx.title_mode() {
                ctx.push_str(&index.to_string());
            }
        }
        Element::FootnoteBlock { .. }
        | Element::BibliographyCite { .. }
        | Element::BibliographyBlock { .. } => {}
        Element::User { name, .. } => ctx.push_str(name),
        Element::Date { value, format, .. } => {
            let formatted = if ctx.settings().layout.legacy() {
                value.format_or_default_wikidot(format.as_deref(), ctx.language())
            } else {
                value.format_or_default(format.as_deref(), ctx.language())
            };
            ctx.push_str(&formatted);
        }
        Element::Color { elements, .. } => render_elements(ctx, elements),
        Element::Code(CodeBlock { contents, .. }) => {
            ctx.add_newline();
            ctx.push_str(contents);
            ctx.add_newline();
        }
        Element::Math { .. } | Element::MathInline { .. } => {}
        Element::EquationReference(name) => {
            str_write!(ctx, "[{name}]");
        }
        Element::Embed(_) | Element::Html { .. } | Element::Iframe { .. } => {}
        Element::Include {
            variables,
            elements,
            ..
        } => {
            debug!(
                "Rendering include (variables length {}, elements length {})",
                variables.len(),
                elements.len(),
            );

            ctx.variables_mut().push_scope(variables);
            render_elements(ctx, elements);
            ctx.variables_mut().pop_scope();
        }
        Element::Style(_) | Element::ClearFloat(_) => {}
        Element::LineBreak => ctx.add_newline(),
        Element::LineBreaks(amount) => {
            for _ in 0..amount.get() {
                ctx.add_newline();
            }
        }
        Element::HorizontalRule => {}
        Element::Partial(partial) => render_partial(ctx, partial),
    }
}

fn render_partial(ctx: &mut TextContext, partial: &PartialElement) {
    warn!(
        "Encountered partial element during text rendering: {}",
        partial.name()
    );

    match partial {
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
        PartialElement::ListItem(ListItem::Elements { elements, .. }) => {
            if elements.is_empty() {
                return;
            }

            if !ctx.ends_with_newline() {
                ctx.add_newline();
            }
            render_elements(ctx, elements);
            ctx.add_newline();
        }
        PartialElement::ListItem(ListItem::SubList { element }) => {
            render_element(ctx, element)
        }
        PartialElement::TableRow(row) => {
            if !ctx.ends_with_newline() {
                ctx.add_newline();
            }
            for cell in &row.cells {
                render_elements(ctx, &cell.elements);
            }
            ctx.add_newline();
        }
        PartialElement::TableCell(cell) => render_elements(ctx, &cell.elements),
        PartialElement::Tab(tab) => {
            if !ctx.ends_with_newline() {
                ctx.add_newline();
            }
            ctx.push_str(&tab.label);
            ctx.add_newline();
            render_elements(ctx, &tab.elements);
            ctx.add_newline();
        }
        PartialElement::RubyText(ruby_text) => {
            ctx.push('(');
            render_elements(ctx, &ruby_text.elements);
            ctx.push(')');
        }
    }
}

#[test]
fn text_render_skips_non_textual_elements_and_expands_include_variables() {
    use super::TextRender;
    use crate::data::{PageInfo, PageRef};
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::{
        Alignment, AttributeMap, Container, ContainerType, ListType, Module, SyntaxTree,
        VariableMap,
    };

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let mut variables = VariableMap::new();
    variables.insert(cow!("name"), cow!("included"));

    let tree = SyntaxTree {
        elements: vec![
            Element::Text(cow!("start ")),
            Element::Container(Container::new(
                ContainerType::Hidden,
                vec![Element::Text(cow!("hidden"))],
                AttributeMap::new(),
            )),
            Element::Module(Module::Rate),
            Element::Include {
                paragraph_safe: true,
                variables,
                location: PageRef::page_only(cow!("component:text")),
                elements: vec![Element::Variable(cow!("name"))],
            },
            Element::List {
                ltype: ListType::Bullet,
                attributes: AttributeMap::new(),
                items: vec![
                    ListItem::Elements {
                        attributes: AttributeMap::new(),
                        elements: vec![],
                    },
                    ListItem::Elements {
                        attributes: AttributeMap::new(),
                        elements: vec![Element::Text(cow!("list item"))],
                    },
                ],
            },
            Element::RadioButton {
                name: cow!("choice"),
                checked: true,
                attributes: AttributeMap::new(),
            },
            Element::CheckBox {
                checked: true,
                attributes: AttributeMap::new(),
            },
            Element::TableOfContents {
                align: Some(Alignment::Left),
                attributes: AttributeMap::new(),
            },
            Element::Footnote(1),
            Element::FootnoteBlock {
                title: Some(cow!("Notes")),
                hide: false,
            },
            Element::BibliographyCite {
                label: cow!("ref"),
                brackets: true,
                inline: false,
            },
            Element::BibliographyBlock {
                index: 0,
                title: Some(cow!("References")),
                hide: false,
            },
        ],
        ..SyntaxTree::default()
    };

    let output = TextRender.render(&tree, &page_info, &settings);

    assert_eq!(output, "start included\nlist item");
}

#[test]
fn text_render_tolerates_partial_elements() {
    use super::TextRender;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::{AttributeMap, PartialElement, SyntaxTree};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let tree = SyntaxTree {
        elements: vec![
            Element::Partial(PartialElement::ListItem(ListItem::Elements {
                attributes: AttributeMap::new(),
                elements: vec![Element::Text(cow!("partial"))],
            })),
            Element::Text(cow!("after")),
        ],
        ..SyntaxTree::default()
    };

    assert_eq!(
        TextRender.render(&tree, &page_info, &settings),
        "partial\nafter",
    );
}
