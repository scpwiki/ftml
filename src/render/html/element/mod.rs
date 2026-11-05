/*
 * render/html/element/mod.rs
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

mod audio;
mod bibliography;
mod block_frame;
mod button;
mod clear_float;
mod collapsible;
mod container;
mod date;
mod definition_list;
mod embed;
mod embed_video;
mod file;
mod footnotes;
mod gallery;
mod iframe;
mod image;
mod include;
mod input;
mod link;
mod list;
mod math;
mod module;
mod social;
mod style;
mod table;
mod tabs;
mod text;
mod toc;
mod user;
mod video;

mod prelude {
    pub use super::super::attributes::AddedAttributes;
    pub use super::super::context::HtmlContext;
    pub use super::super::random::Random;
    pub use super::{render_element, render_elements};
    pub use crate::layout::Layout;
    pub use crate::tree::Element;
}

use self::audio::render_audio;
use self::bibliography::{render_bibcite, render_bibliography};
use self::block_frame::{BlockFrame, render_block_frame};
use self::button::render_standalone_button;
use self::clear_float::render_clear_float;
use self::collapsible::{Collapsible, render_collapsible};
use self::container::{render_color, render_container};
use self::date::render_date;
use self::definition_list::render_definition_list;
use self::embed::render_embed;
use self::embed_video::render_embed_video;
use self::file::render_file_link;
use self::footnotes::{render_footnote, render_footnote_block};
use self::gallery::render_gallery;
use self::iframe::{render_html, render_iframe};
use self::image::render_image;
use self::include::{render_include, render_variable};
use self::input::{render_checkbox, render_radio_button};
use self::link::{render_anchor, render_anchor_target, render_link};
use self::list::render_list;
use self::math::{render_equation_reference, render_math_block, render_math_inline};
use self::module::render_module;
use self::social::render_social_buttons;
use self::style::render_style;
use self::table::render_table;
use self::tabs::render_tabview;
use self::text::{render_code, render_email, render_wikitext_raw};
use self::toc::render_table_of_contents;
use self::user::render_user;
use self::video::render_video;
use super::HtmlContext;
use super::attributes::AddedAttributes;
use crate::tree::{CodeBlock, Element, ListItem, PartialElement};
use ref_map::*;

pub fn render_elements(ctx: &mut HtmlContext, elements: &[Element]) {
    debug!("Rendering elements (length {})", elements.len());

    let mut index = 0;
    while let Some(element) = elements.get(index) {
        match element {
            Element::Text(text) => {
                ctx.push_escaped(text);
                index += 1;

                while let Some(Element::Text(text)) = elements.get(index) {
                    ctx.push_escaped(text);
                    index += 1;
                }
            }
            element => {
                render_element(ctx, element);
                index += 1;
                if ctx.layout().legacy()
                    && matches!(element, Element::HorizontalRule)
                    && matches!(elements.get(index), Some(Element::HorizontalRule))
                {
                    ctx.push_raw('\n');
                }
            }
        }
    }
}

#[test]
fn wikidot_separates_adjacent_horizontal_rules_with_a_text_newline() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize("----\n----");
    let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        HtmlRender.render(&tree, &page_info, &settings).body,
        "<hr>\n<hr>",
    );
}

pub fn render_element(ctx: &mut HtmlContext, element: &Element) {
    macro_rules! ref_cow {
        ($input:expr) => {
            $input.ref_map(|s| s.as_ref())
        };
    }

    debug!("Rendering element '{}'", element.name());

    match element {
        Element::Container(container) => render_container(ctx, container),
        Element::Module(module) => render_module(ctx, module),
        Element::Text(text) => ctx.push_escaped(text),
        Element::Raw(text) => render_wikitext_raw(ctx, text),
        Element::Variable(name) => render_variable(ctx, name),
        Element::Email(email) => render_email(ctx, email),
        Element::Delayed(_) => {
            panic!("unbound delayed element reached the HTML renderer")
        }
        Element::ContentSeparator => {
            if ctx.layout().legacy() {
                ctx.push_raw_str(
                    r#"<div class="content-separator" style="display: none:"></div>"#,
                );
            }
        }
        Element::Table(table) => render_table(ctx, table),
        Element::TabView(tabs) => render_tabview(ctx, tabs),
        Element::StandaloneButton(button) => render_standalone_button(ctx, button),
        Element::SocialButtons(social) => render_social_buttons(ctx, social),
        Element::EmbedVideo(embed_video) => render_embed_video(ctx, embed_video),
        Element::Gallery(gallery) => render_gallery(ctx, gallery),
        Element::Anchor {
            elements,
            attributes,
            target,
        } => render_anchor(ctx, elements, attributes, *target),
        Element::AnchorName(name) => render_anchor_target(ctx, name),
        Element::Link {
            ltype,
            link,
            label,
            target,
        } => render_link(ctx, link, label, *target, *ltype),
        Element::FileLink { file, label } => render_file_link(ctx, file, label),
        Element::Image {
            source,
            link,
            alignment,
            attributes,
            target,
        } => render_image(ctx, source, link, *alignment, attributes, *target),
        Element::Audio {
            source,
            alignment,
            attributes,
        } => render_audio(ctx, source, *alignment, attributes),
        Element::Video {
            source,
            alignment,
            attributes,
        } => render_video(ctx, source, *alignment, attributes),
        Element::List {
            ltype,
            items,
            attributes,
            ..
        } => render_list(ctx, *ltype, items, attributes),
        Element::DefinitionList(items) => render_definition_list(ctx, items),
        Element::RadioButton {
            name,
            checked,
            attributes,
        } => render_radio_button(ctx, name, *checked, attributes),
        Element::CheckBox {
            checked,
            attributes,
        } => render_checkbox(ctx, *checked, attributes),
        Element::Collapsible {
            elements,
            unfolded_tail_start,
            wikidot_label_spans,
            wikidot_block_open_elements,
            attributes,
            start_open,
            show_text,
            hide_text,
            show_top,
            show_bottom,
        } => render_collapsible(
            ctx,
            Collapsible {
                elements,
                unfolded_tail_start: *unfolded_tail_start,
                wikidot_label_spans,
                wikidot_block_open_elements,
                attributes,
                start_open: *start_open,
                show_text: ref_cow!(show_text),
                hide_text: ref_cow!(hide_text),
                show_top: *show_top,
                show_bottom: *show_bottom,
            },
        ),
        Element::WikidotBlockFrame {
            attributes,
            children,
            collapsible_link,
            omit_close,
        } => render_block_frame(
            ctx,
            BlockFrame {
                attributes,
                children,
                collapsible_link: collapsible_link.as_ref(),
                omit_close: *omit_close,
            },
        ),
        Element::TableOfContents { align, attributes } => {
            render_table_of_contents(ctx, *align, attributes)
        }
        Element::Footnote(index) => render_footnote(ctx, *index),
        Element::FootnoteBlock {
            title,
            hide,
            wikidot_wrappers,
            ..
        } => {
            if (ctx.layout().legacy() || !*hide) && !ctx.footnotes().is_empty() {
                render_footnote_block(ctx, ref_cow!(title), wikidot_wrappers);
            }
        }
        Element::BibliographyCite {
            label,
            brackets,
            inline,
        } => render_bibcite(ctx, label, *brackets, *inline),
        Element::BibliographyBlock { index, title, hide } => {
            if ctx.layout().legacy() || !*hide {
                if let Some(bibliography) = ctx.get_bibliography(*index) {
                    let title = title.ref_map(|s| s.as_ref());
                    render_bibliography(ctx, title, *index, bibliography);
                } else {
                    warn!("Missing bibliography for bibliography block index {index}");
                    let message = ctx
                        .handle()
                        .get_message(ctx.language(), "bibliography-block-not-found");
                    ctx.html()
                        .span()
                        .attr(attr!("class" => "wj-error-inline"))
                        .inner(|ctx| ctx.push_escaped(message));
                }
            }
        }
        Element::User { name, show_avatar } => render_user(ctx, name, *show_avatar),
        Element::Date {
            value,
            format,
            hover,
        } => render_date(ctx, *value, ref_cow!(format), *hover),
        Element::Color {
            color,
            background,
            elements,
        } => render_color(ctx, color, *background, elements),
        Element::Code(CodeBlock {
            contents,
            language,
            name: _,
            wikidot_block_open_elements,
        }) => render_code(
            ctx,
            ref_cow!(language),
            contents,
            wikidot_block_open_elements,
        ),
        Element::Math {
            name,
            latex_source,
            wikidot_block_open_elements,
        } => render_math_block(
            ctx,
            ref_cow!(name),
            latex_source,
            wikidot_block_open_elements,
        ),
        Element::MathInline { latex_source } => render_math_inline(ctx, latex_source),
        Element::EquationReference(name) => render_equation_reference(ctx, name),
        Element::Embed(embed) => render_embed(ctx, embed),
        Element::Html {
            contents,
            attributes,
        } => render_html(ctx, contents, attributes),
        Element::Iframe { url, attributes } => render_iframe(ctx, url, attributes),
        Element::Include {
            variables,
            location,
            elements,
            ..
        } => render_include(ctx, location, variables, elements),
        Element::Style(css) => render_style(ctx, css),
        Element::LineBreak => render_line_break(ctx),
        Element::LineBreaks(amount) => {
            let amount = amount.get();
            for _ in 0..amount {
                render_line_break(ctx);
            }
        }
        Element::ClearFloat(clear_float) => render_clear_float(ctx, *clear_float),
        Element::HorizontalRule => {
            ctx.html().hr();
        }
        Element::Partial(partial) => render_partial(ctx, partial),
    }
}

fn render_line_break(ctx: &mut HtmlContext) {
    ctx.html().br();
    if ctx.layout().legacy() {
        ctx.push_raw('\n');
    }
}

#[test]
fn html_render_tolerates_partial_elements() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::{AttributeMap, ListItem, PartialElement, SyntaxTree};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let tree = SyntaxTree {
        elements: vec![Element::Partial(PartialElement::ListItem(
            ListItem::Elements {
                attributes: AttributeMap::new(),
                elements: vec![Element::Text(cow!("partial"))],
            },
        ))],
        ..SyntaxTree::default()
    };

    let output = HtmlRender.render(&tree, &page_info, &settings);
    assert!(output.body.contains("partial"));
}

#[test]
fn wikidot_layout_preserves_the_text_node_after_a_line_break() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize("A\nB");
    let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
    assert!(errors.is_empty(), "{errors:?}");

    let output = HtmlRender.render(&tree, &page_info, &settings);
    assert_eq!(output.body, "<p>A<br>\nB</p>");
}

fn render_partial(ctx: &mut HtmlContext, partial: &PartialElement) {
    warn!(
        "Encountered partial element during rendering: {}",
        partial.name()
    );

    match partial {
        PartialElement::WikidotEmptyInlineOwner
        | PartialElement::WikidotBlockOpen(_)
        | PartialElement::WikidotBlockClose(_)
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
            render_elements(ctx, elements)
        }
        PartialElement::ListItem(ListItem::SubList { element }) => {
            render_element(ctx, element)
        }
        PartialElement::TableRow(row) => {
            for cell in &row.cells {
                render_elements(ctx, &cell.elements);
            }
        }
        PartialElement::TableCell(cell) => render_elements(ctx, &cell.elements),
        PartialElement::Tab(tab) => {
            if !tab.label.is_empty() {
                ctx.html().span().contents(&tab.label);
                if !tab.elements.is_empty() {
                    ctx.push_escaped(" ");
                }
            }
            render_elements(ctx, &tab.elements);
        }
        PartialElement::RubyText(ruby_text) => render_elements(ctx, &ruby_text.elements),
    }
}
