/*
 * render/html/mod.rs
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

#[macro_use]
mod attributes;
mod builder;
mod context;
mod element;
mod escape;
mod meta;
mod output;
mod random;
mod render;

pub use self::meta::{HtmlMeta, HtmlMetaType};
pub use self::output::HtmlOutput;

use self::attributes::AddedAttributes;
use self::context::HtmlContext;
use self::element::{render_element, render_elements};
use crate::data::PageInfo;
use crate::layout::Layout;
use crate::render::{Handle, Render};
use crate::settings::WikitextSettings;
use crate::tree::{Element, SyntaxTree};

#[derive(Debug)]
pub struct HtmlRender;

impl Render for HtmlRender {
    type Output = HtmlOutput;

    fn render(
        &self,
        tree: &SyntaxTree,
        page_info: &PageInfo,
        settings: &WikitextSettings,
    ) -> HtmlOutput {
        info!(
            "Rendering HTML (site {}, page {}, category {})",
            page_info.site.as_ref(),
            page_info.page.as_ref(),
            match &page_info.category {
                Some(category) => category.as_ref(),
                None => "_default",
            },
        );

        let mut ctx = HtmlContext::new(
            page_info,
            &Handle,
            settings,
            &tree.table_of_contents,
            &tree.footnotes,
            &tree.bibliographies,
            tree.wikitext_len,
        );

        // Crawl through elements and generate HTML
        match settings.layout {
            Layout::Wikidot => {
                ctx.html()
                    .div()
                    .attr(attr!("id" => "main-content"; if settings.use_true_ids))
                    .inner(|ctx| render_contents(ctx, tree));
            }
            Layout::Wikijump => {
                ctx.html()
                    .article()
                    .attr(attr!(
                        "id" => "main-content"; if settings.use_true_ids,
                        "class" => "wj-body",
                    ))
                    .inner(|ctx| render_contents(ctx, tree));
            }
        }

        // Build and return HtmlOutput
        ctx.into()
    }
}

fn render_contents(ctx: &mut HtmlContext, tree: &SyntaxTree) {
    render_elements(ctx, &tree.elements);

    if tree.needs_footnote_block {
        info!("Page needs footnote but one was not manually included, adding");
        render_element(
            ctx,
            &Element::FootnoteBlock {
                title: None,
                hide: false,
            },
        );
    }
}

/// Tests that the IDs are present when use_true_ids = true and absent otherwise.
#[test]
fn html_id_wrap() {
    use crate::settings::{EMPTY_INTERWIKI, WikitextMode};

    let page_info = PageInfo::dummy();
    let tokens = crate::tokenize("CONTENT HERE");

    macro_rules! settings {
        ($layout:ident, $use_true_ids:expr) => {
            WikitextSettings {
                mode: WikitextMode::Page,
                layout: Layout::$layout,
                enable_page_syntax: true,
                use_true_ids: $use_true_ids,
                isolate_user_ids: false,
                minify_css: false,
                allow_local_paths: true,
                interwiki: EMPTY_INTERWIKI.clone(),
            }
        };
    }

    macro_rules! test {
        ($layout:ident, $use_true_ids:expr, $starts_with:expr $(,)?) => {{
            let settings = settings!($layout, $use_true_ids);
            let (tree, errors) = crate::parse(&tokens, &page_info, &settings).into();
            assert!(errors.is_empty(), "Found unexpected parse error in test");

            let HtmlOutput { body, .. } = HtmlRender.render(&tree, &page_info, &settings);
            assert!(
                body.starts_with($starts_with),
                "Generated HTML doesn't begin as expected\ncontent: {}\ntested start: {}",
                body,
                $starts_with,
            );
        }};
    }

    test!(Wikidot, true, r#"<div id="main-content">"#);
    test!(Wikidot, false, r#"<div>"#);
    test!(
        Wikijump,
        true,
        r#"<article id="main-content" class="wj-body">"#,
    );
    test!(Wikijump, false, r#"<article class="wj-body">"#);
}
