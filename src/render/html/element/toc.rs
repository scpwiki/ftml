/*
 * render/html/element/toc.rs
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
use crate::tree::{Alignment, AttributeMap, FloatAlignment};

pub fn render_table_of_contents(
    ctx: &mut HtmlContext,
    align: Option<Alignment>,
    attributes: &AttributeMap,
) {
    debug!("Creating table of contents");
    if ctx.layout().legacy() {
        render_wikidot_table_of_contents(ctx, align);
        return;
    }
    let use_true_ids = ctx.settings().use_true_ids;

    let class_value = match align {
        None => "",
        Some(align) => FloatAlignment { align, float: true }.wj_html_class(),
    };

    ctx.html()
        .div()
        .attr(attr!(
            "id" => "wj-toc"; if use_true_ids,
            "class" => class_value; if align.is_some();;
            attributes
        ))
        .inner(|ctx| {
            ctx.html()
                .div()
                .attr(attr!("id" => "wj-toc-action-bar"; if use_true_ids))
                .inner(|ctx| {
                    ctx.html().a().attr(attr!(
                        "href" => "javascript:;",
                        "onclick" => "WIKIJUMP.page.listeners.foldToc(event)",
                    ));
                });

            let table_of_contents_title = ctx
                .handle()
                .get_message(ctx.language(), "table-of-contents");

            ctx.html()
                .div()
                .attr(attr!("class" => "title"))
                .inner(|ctx| ctx.push_escaped(table_of_contents_title));

            ctx.html()
                .div()
                .attr(attr!("id" => "wj-toc-list"; if use_true_ids))
                .inner(|ctx| {
                    ctx.push_cached_table_of_contents(|ctx| {
                        let table_of_contents = ctx.table_of_contents();
                        render_elements(ctx, table_of_contents);
                    });
                });
        });
}

fn render_wikidot_table_of_contents(ctx: &mut HtmlContext, align: Option<Alignment>) {
    if align.is_none() {
        ctx.html()
            .table()
            .attr(attr!("style" => "margin:0; padding:0"))
            .inner(|ctx| {
                ctx.html().tr().inner(|ctx| {
                    ctx.html()
                        .table_cell(false)
                        .attr(attr!("style" => "margin:0; padding:0"))
                        .inner(|ctx| render_wikidot_toc_box(ctx, align));
                });
            });
    } else {
        render_wikidot_toc_box(ctx, align);
    }
}

fn render_wikidot_toc_box(ctx: &mut HtmlContext, align: Option<Alignment>) {
    let class_value =
        align.map(|align| FloatAlignment { align, float: true }.wd_html_class());
    ctx.html()
        .div()
        .attr(attr!(
            "id" => "toc",
            "class" => class_value.unwrap_or_default(); if class_value.is_some(),
        ))
        .inner(|ctx| {
            ctx.html()
                .div()
                .attr(attr!("id" => "toc-action-bar"))
                .inner(|ctx| {
                    ctx.html()
                        .a()
                        .attr(attr!(
                            "href" => "javascript:;",
                            "onclick" => "WIKIDOT.page.listeners.foldToc(event)",
                        ))
                        .contents("Fold");
                    ctx.html()
                        .a()
                        .attr(attr!(
                            "style" => "display: none",
                            "href" => "javascript:;",
                            "onclick" => "WIKIDOT.page.listeners.unfoldToc(event)",
                        ))
                        .contents("Unfold");
                });
            let title = ctx
                .handle()
                .get_message(ctx.language(), "table-of-contents");
            ctx.html()
                .div()
                .attr(attr!("class" => "title"))
                .contents(title);
            ctx.html()
                .div()
                .attr(attr!("id" => "toc-list"))
                .inner(|ctx| {
                    ctx.push_cached_table_of_contents(|ctx| {
                        let table_of_contents = ctx.table_of_contents();
                        render_wikidot_toc_entries(ctx, table_of_contents, 1);
                    });
                });
        });
}

fn render_wikidot_toc_entries(ctx: &mut HtmlContext, elements: &[Element], depth: usize) {
    for element in elements {
        let Element::List { items, .. } = element else {
            render_element(ctx, element);
            continue;
        };
        for item in items {
            match item {
                crate::tree::ListItem::Elements { elements, .. } => {
                    let style = format!("margin-left: {depth}em;");
                    ctx.html()
                        .div()
                        .attr(attr!("style" => &style))
                        .inner(|ctx| render_elements(ctx, elements));
                }
                crate::tree::ListItem::SubList { element } => {
                    render_wikidot_toc_entries(
                        ctx,
                        std::slice::from_ref(element.as_ref()),
                        depth + 1,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::output::HtmlOutput;
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Handle, Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::BibliographyList;

    #[test]
    fn table_of_contents_renders_aligned_class_ids_and_entries() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let toc_entries = [text!("Section & details")];
        let footnotes: [Vec<Element<'static>>; 0] = [];
        let bibliographies = BibliographyList::new();
        let mut ctx = HtmlContext::new(
            &page_info,
            &Handle,
            &settings,
            &toc_entries,
            &footnotes,
            &bibliographies,
            0,
        );
        let mut attributes = AttributeMap::new();
        assert!(attributes.insert("class", cow!("custom-toc")));

        render_table_of_contents(&mut ctx, Some(Alignment::Left), &attributes);

        let output = HtmlOutput::from(ctx);
        assert!(output.body.contains(r#"id="wj-toc""#));
        assert!(output.body.contains(r#"class="wj-float-left custom-toc""#));
        assert!(output.body.contains(r#"<div id="wj-toc-action-bar">"#));
        assert!(
            output
                .body
                .contains(r#"<div class="title">Table of Contents</div>"#)
        );
        assert!(
            output
                .body
                .contains(r#"<div id="wj-toc-list">Section &amp; details</div>"#)
        );
    }

    #[test]
    fn wikidot_table_of_contents_uses_legacy_controls_and_ignores_attributes() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let toc_entries: [Element<'static>; 0] = [];
        let footnotes: [Vec<Element<'static>>; 0] = [];
        let bibliographies = BibliographyList::new();
        let mut ctx = HtmlContext::new(
            &page_info,
            &Handle,
            &settings,
            &toc_entries,
            &footnotes,
            &bibliographies,
            0,
        );
        let mut attributes = AttributeMap::new();
        assert!(attributes.insert("id", cow!("u-contents")));

        render_table_of_contents(&mut ctx, Some(Alignment::Right), &attributes);

        let output = HtmlOutput::from(ctx);
        assert_eq!(
            output.body,
            concat!(
                r#"<div id="toc" class="floatright"><div id="toc-action-bar">"#,
                r#"<a href="javascript:;" onclick="WIKIDOT.page.listeners.foldToc(event)">Fold</a>"#,
                r#"<a style="display: none" href="javascript:;" onclick="WIKIDOT.page.listeners.unfoldToc(event)">Unfold</a>"#,
                r#"</div><div class="title">Table of Contents</div><div id="toc-list"></div></div>"#,
            ),
        );
    }

    #[test]
    fn wikidot_nonfloating_toc_uses_table_wrapper_and_flat_indented_entries() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = "[[toc]]\n+ One\n++ Two".to_owned();
        crate::preprocess(&mut source);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(
            html.starts_with(
                r#"<table style="margin:0; padding:0"><tr><td style="margin:0; padding:0"><div id="toc">"#,
            ),
            "{html}",
        );
        assert!(
            html.contains(
                r##"<div id="toc-list"><div style="margin-left: 1em;"><a href="#toc0">One</a></div><div style="margin-left: 2em;"><a href="#toc1">Two</a></div></div>"##,
            ),
            "{html}",
        );
    }

    #[test]
    fn repeated_table_of_contents_blocks_render_identical_inner_lists() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let toc_entries = [text!("Repeated & entry")];
        let footnotes: [Vec<Element<'static>>; 0] = [];
        let bibliographies = BibliographyList::new();
        let mut ctx = HtmlContext::new(
            &page_info,
            &Handle,
            &settings,
            &toc_entries,
            &footnotes,
            &bibliographies,
            0,
        );
        let attributes = AttributeMap::new();

        render_table_of_contents(&mut ctx, None, &attributes);
        render_table_of_contents(&mut ctx, None, &attributes);
        render_table_of_contents(&mut ctx, None, &attributes);

        let output = HtmlOutput::from(ctx);
        let toc_lists = output
            .body
            .match_indices(r#"<div id="wj-toc-list">"#)
            .map(|(start, marker)| {
                let list_start = start + marker.len();
                let list_end = output.body[list_start..]
                    .find("</div>")
                    .map(|offset| list_start + offset)
                    .expect("TOC list div should close");
                &output.body[list_start..list_end]
            })
            .collect::<Vec<_>>();

        assert_eq!(toc_lists.len(), 3);
        assert!(toc_lists.iter().all(|toc_list| *toc_list == toc_lists[0]));
    }
}
