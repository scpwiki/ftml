/*
 * render/html/element/container.rs
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

use super::prelude::*;
use crate::tree::{
    Container, ContainerType, HtmlTag, WIKIDOT_EMPTY_QUOTE_MARKER,
    WIKIDOT_GENERATED_EMPTY_CLASS_MARKER,
};

pub fn render_container(ctx: &mut HtmlContext, container: &Container) {
    debug!("Rendering container '{}'", container.ctype().name());

    if render_plain_div_chain(ctx, container) {
        return;
    }

    match container.ctype() {
        ContainerType::RubyText => {
            ctx.html().rp().contents("(");
            render_container_internal(ctx, container);
            ctx.html().rp().contents(")");
        }

        _ => render_container_internal(ctx, container),
    }
}

fn render_plain_div_chain(ctx: &mut HtmlContext, container: &Container) -> bool {
    if container.ctype() != ContainerType::Div || !container.attributes().get().is_empty()
    {
        return false;
    }

    let mut current = container;
    let mut depth = 0usize;
    loop {
        ctx.push_raw_str("<div>");
        depth += 1;

        match current.elements() {
            [Element::Container(child)]
                if child.ctype() == ContainerType::Div
                    && child.attributes().get().is_empty() =>
            {
                current = child;
            }
            elements => {
                render_elements(ctx, elements);
                break;
            }
        }
    }

    for _ in 0..depth {
        ctx.push_raw_str("</div>");
    }
    true
}

pub fn render_container_internal(ctx: &mut HtmlContext, container: &Container) {
    let layout = ctx.layout();
    let tag_spec = container.ctype().html_tag(layout, ctx);

    let random_id = choose_id(ctx, &tag_spec);

    let mut tag = ctx.html().tag(tag_spec.tag());

    let mut generated_class_attributes = None;
    let mut empty_quote_attributes = None;
    let preserve_generated_empty_class = layout.legacy()
        && container.ctype() == ContainerType::Span
        && container
            .attributes()
            .get()
            .contains_key(WIKIDOT_GENERATED_EMPTY_CLASS_MARKER);
    // An ejected tabview leaves an empty blockquote run in live; the marker
    // keeps it through tree cleanup and never reaches the output.
    let strip_empty_quote_marker = container
        .attributes()
        .get()
        .contains_key(WIKIDOT_EMPTY_QUOTE_MARKER);
    let attributes = if preserve_generated_empty_class {
        let mut attributes = container.attributes().clone();
        attributes.remove(WIKIDOT_GENERATED_EMPTY_CLASS_MARKER);
        generated_class_attributes.insert(attributes)
    } else if strip_empty_quote_marker {
        let mut attributes = container.attributes().clone();
        attributes.remove(WIKIDOT_EMPTY_QUOTE_MARKER);
        empty_quote_attributes.insert(attributes)
    } else {
        container.attributes()
    };

    match tag_spec {
        HtmlTag::Tag(_) => tag.attr(attr!(
            "class" => ""; if preserve_generated_empty_class;;
            attributes,
        )),
        HtmlTag::TagAndClass { class, .. } => tag.attr(attr!(
            "class" => class;;
            attributes,
        )),
        HtmlTag::TagAndStyle { style, .. } => tag.attr(attr!(
            "style" => style;;
            attributes,
        )),
        HtmlTag::TagAndId { id, .. } => {
            let id = random_id.as_deref().unwrap_or(&id);
            tag.attr(attr!("id" => id;; attributes))
        }
    };

    if layout.legacy() && matches!(container.ctype(), ContainerType::Header(_)) {
        if container.elements().is_empty()
            || has_wikidot_heading_label_span(container.elements())
        {
            tag.contents(container.elements());
        } else {
            tag.inner(|ctx| {
                ctx.html().span().contents(container.elements());
            });
        }
    } else if layout.legacy()
        && container.ctype() == ContainerType::Blockquote
        && let Some(elements) = wikidot_break_only_paragraph(container.elements())
    {
        tag.contents(elements);
    } else if layout.legacy() && container_trailing_break_drops_newline(container.ctype())
    {
        match container.elements().split_last() {
            Some((Element::LineBreak, rest)) => {
                tag.inner(|ctx| {
                    render_elements(ctx, rest);
                    ctx.html().br();
                });
            }
            _ => {
                tag.contents(container.elements());
            }
        }
    } else {
        tag.contents(container.elements());
    }
}

/// A Wikidot blockquote whose whole body is one paragraph of line breaks is
/// emitted without the paragraph wrapper: live renders the breaks directly
/// inside the quote, not a paragraph shell.
fn wikidot_break_only_paragraph<'a, 't>(
    elements: &'a [Element<'t>],
) -> Option<&'a [Element<'t>]> {
    let [Element::Container(paragraph)] = elements else {
        return None;
    };
    if paragraph.ctype() != ContainerType::Paragraph {
        return None;
    }
    let elements = paragraph.elements();
    (!elements.is_empty()
        && elements.iter().all(|element| {
            matches!(element, Element::LineBreak | Element::LineBreaks(_))
        }))
    .then_some(elements)
}

fn container_trailing_break_drops_newline(ctype: ContainerType) -> bool {
    matches!(
        ctype,
        ContainerType::Bold
            | ContainerType::Italics
            | ContainerType::Monospace
            | ContainerType::Span
            | ContainerType::Strikethrough
            | ContainerType::Superscript
            | ContainerType::Subscript
            | ContainerType::Underline
    )
}

fn has_wikidot_heading_label_span(elements: &[Element]) -> bool {
    if let [element] = elements {
        match element {
            Element::Container(container)
                if matches!(
                    container.ctype(),
                    ContainerType::Span
                        | ContainerType::Size
                        | ContainerType::HeadingLabel
                ) =>
            {
                if container.ctype() == ContainerType::HeadingLabel {
                    true
                } else {
                    has_wikidot_heading_label_span(container.elements())
                }
            }
            Element::Anchor { elements, .. } | Element::Color { elements, .. } => {
                has_wikidot_heading_label_span(elements)
            }
            _ => false,
        }
    } else {
        false
    }
}

pub fn render_color(
    ctx: &mut HtmlContext,
    color: &str,
    background: bool,
    elements: &[Element],
) {
    debug!("Rendering color container (color '{color}')");

    let property = if background {
        "background-color"
    } else {
        "color"
    };
    let style = if ctx.layout().legacy() {
        if color.is_empty() {
            format!("{property}:")
        } else {
            format!("{property}: {color}")
        }
    } else {
        format!("{property}: {color};")
    };
    ctx.html()
        .span()
        .attr(attr!("style" => &style))
        .inner(|ctx| match elements.split_last() {
            Some((Element::LineBreak, rest)) if ctx.layout().legacy() => {
                render_elements(ctx, rest);
                ctx.html().br();
            }
            _ => render_elements(ctx, elements),
        });
}

fn choose_id(ctx: &mut HtmlContext, tag_spec: &HtmlTag) -> Option<String> {
    if matches!(tag_spec, HtmlTag::TagAndId { .. }) && !ctx.settings().use_true_ids {
        Some(ctx.random().generate_html_id())
    } else {
        None
    }
}

#[test]
fn container_rendering_covers_special_tag_variants() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::{
        Alignment, AttributeMap, Container, Heading, HeadingLevel, SyntaxTree,
    };

    let page_info = PageInfo::dummy();
    let wikijump = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);

    let wikijump_tree = SyntaxTree {
        elements: vec![
            Element::Container(Container::new(
                ContainerType::RubyText,
                vec![Element::Text(cow!("go"))],
                AttributeMap::new(),
            )),
            Element::Container(Container::new(
                ContainerType::Monospace,
                vec![Element::Text(cow!("mono"))],
                AttributeMap::new(),
            )),
            Element::Container(Container::new(
                ContainerType::Header(Heading {
                    level: HeadingLevel::Two,
                    has_toc: true,
                }),
                vec![Element::Text(cow!("Heading"))],
                AttributeMap::new(),
            )),
        ],
        ..SyntaxTree::default()
    };
    let wikijump_output = HtmlRender.render(&wikijump_tree, &page_info, &wikijump);

    assert!(
        wikijump_output
            .body
            .contains("<rp>(</rp><rt>go</rt><rp>)</rp>")
    );
    assert!(
        wikijump_output
            .body
            .contains(r#"<code class="wj-monospace">mono</code>"#)
    );
    assert!(
        wikijump_output
            .body
            .contains(r#"<h2 id="toc0">Heading</h2>"#)
    );

    let wikidot = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let wikidot_tree = SyntaxTree {
        elements: vec![
            Element::Container(Container::new(
                ContainerType::Align(Alignment::Right),
                vec![Element::Text(cow!("right"))],
                AttributeMap::new(),
            )),
            Element::Container(Container::new(
                ContainerType::Header(Heading {
                    level: HeadingLevel::Two,
                    has_toc: true,
                }),
                vec![Element::Text(cow!("Heading"))],
                AttributeMap::new(),
            )),
        ],
        ..SyntaxTree::default()
    };
    let wikidot_output = HtmlRender.render(&wikidot_tree, &page_info, &wikidot);

    assert_eq!(
        wikidot_output.body,
        r#"<div style="text-align: right;">right</div><h2 id="toc0"><span>Heading</span></h2>"#
    );
}

#[test]
fn wikidot_underline_uses_the_live_span_style_dom() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::{AttributeMap, Container, ContainerType, SyntaxTree};

    let page_info = PageInfo::dummy();
    let tree = SyntaxTree {
        elements: vec![Element::Container(Container::new(
            ContainerType::Underline,
            vec![Element::Text(cow!("underlined"))],
            AttributeMap::new(),
        ))],
        ..SyntaxTree::default()
    };

    let wikidot = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let wikijump = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);

    assert_eq!(
        HtmlRender.render(&tree, &page_info, &wikidot).body,
        r#"<span style="text-decoration: underline;">underlined</span>"#,
    );
    assert_eq!(
        HtmlRender.render(&tree, &page_info, &wikijump).body,
        "<u>underlined</u>",
    );
}

#[test]
fn choose_id_generates_random_ids_only_when_true_ids_are_disabled() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Handle;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::BibliographyList;

    let page_info = PageInfo::dummy();
    let handle = Handle;
    let bibliographies = BibliographyList::new();
    let tag_with_id = HtmlTag::with_id("h2", str!("toc0"));

    let mut random_id_settings =
        WikitextSettings::from_mode(WikitextMode::Draft, Layout::Wikijump);
    random_id_settings.use_true_ids = false;
    let mut random_id_ctx = HtmlContext::new(
        &page_info,
        &handle,
        &random_id_settings,
        &[],
        &[],
        &bibliographies,
        0,
    );
    assert_eq!(
        choose_id(&mut random_id_ctx, &tag_with_id).as_deref(),
        Some("wj-id-zvGvLlhGI6VEZFKj"),
    );

    let true_id_settings =
        WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let mut true_id_ctx = HtmlContext::new(
        &page_info,
        &handle,
        &true_id_settings,
        &[],
        &[],
        &bibliographies,
        0,
    );
    assert_eq!(choose_id(&mut true_id_ctx, &tag_with_id), None);
    assert_eq!(choose_id(&mut true_id_ctx, &HtmlTag::new("h2")), None);
}
