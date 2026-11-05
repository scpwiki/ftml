/*
 * render/text/mod.rs
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

mod context;
mod elements;

use self::context::TextContext;
use self::elements::render_elements;
use crate::data::PageInfo;
use crate::render::{Handle, Render};
use crate::settings::WikitextSettings;
use crate::tree::{
    BibliographyList, Element, SyntaxTree, elements_require_bounded_tree_stack,
    run_on_bounded_tree_stack, tree_requires_bounded_tree_stack,
};

#[derive(Debug)]
pub struct TextRender;

impl TextRender {
    pub fn render_partial(
        &self,
        elements: &[Element],
        page_info: &PageInfo,
        settings: &WikitextSettings,
        wikitext_len: usize,
    ) -> String {
        self.render_partial_mode(elements, page_info, settings, wikitext_len, false)
    }

    /// Render elements as a heading title, as used for table-of-contents
    /// entries. Unlike a plain text render, a footnote reference reads as
    /// its number, matching the text the rendered heading shows.
    pub fn render_partial_title(
        &self,
        elements: &[Element],
        page_info: &PageInfo,
        settings: &WikitextSettings,
        wikitext_len: usize,
    ) -> String {
        self.render_partial_mode(elements, page_info, settings, wikitext_len, true)
    }

    fn render_partial_mode(
        &self,
        elements: &[Element],
        page_info: &PageInfo,
        settings: &WikitextSettings,
        wikitext_len: usize,
        title: bool,
    ) -> String {
        let bibliographies = BibliographyList::new();
        let partial = RenderPartial {
            elements,
            page_info,
            settings,
            table_of_contents: &[],
            footnotes: &[],
            bibliographies: &bibliographies,
            wikitext_len,
            title,
        };
        if elements_require_bounded_tree_stack(elements) {
            run_on_bounded_tree_stack("ftml-text-render", || {
                self.render_partial_direct(partial)
            })
        } else {
            self.render_partial_direct(partial)
        }
    }

    fn render_partial_direct(&self, partial: RenderPartial) -> String {
        debug!(
            "Rendering text (site {}, page {}, category {})",
            partial.page_info.site.as_ref(),
            partial.page_info.page.as_ref(),
            page_category(partial.page_info),
        );

        let mut ctx = TextContext::new(
            partial.page_info,
            &Handle,
            partial.settings,
            partial.table_of_contents,
            partial.footnotes,
            partial.bibliographies,
            partial.wikitext_len,
        );
        ctx.set_title_mode(partial.title);
        render_elements(&mut ctx, partial.elements);

        trim_outer_newlines(ctx.buffer());

        ctx.into()
    }
}

fn trim_outer_newlines(buffer: &mut String) {
    let leading_newlines = buffer.bytes().take_while(|byte| *byte == b'\n').count();
    if leading_newlines > 0 {
        buffer.drain(..leading_newlines);
    }

    let trimmed_len = buffer.trim_end_matches('\n').len();
    buffer.truncate(trimmed_len);
}

impl Render for TextRender {
    type Output = String;

    #[inline]
    fn render(
        &self,
        tree: &SyntaxTree,
        page_info: &PageInfo,
        settings: &WikitextSettings,
    ) -> String {
        let site = page_info.site.as_ref();
        let page = page_info.page.as_ref();
        let category = page_category(page_info);
        debug!("Rendering text (site {site}, page {page}, category {category})");

        let partial = RenderPartial::tree(tree, page_info, settings);
        if tree_requires_bounded_tree_stack(tree) {
            run_on_bounded_tree_stack("ftml-text-render", || {
                self.render_partial_direct(partial)
            })
        } else {
            self.render_partial_direct(partial)
        }
    }
}

fn page_category<'a>(page_info: &'a PageInfo) -> &'a str {
    page_info.category.as_deref().unwrap_or("_default")
}

#[derive(Debug)]
struct RenderPartial<'a> {
    elements: &'a [Element<'a>],
    page_info: &'a PageInfo<'a>,
    settings: &'a WikitextSettings,
    table_of_contents: &'a [Element<'a>],
    footnotes: &'a [Vec<Element<'a>>],
    bibliographies: &'a BibliographyList<'a>,
    wikitext_len: usize,
    title: bool,
}

impl<'a> RenderPartial<'a> {
    fn tree(
        tree: &'a SyntaxTree<'a>,
        page_info: &'a PageInfo<'a>,
        settings: &'a WikitextSettings,
    ) -> Self {
        RenderPartial {
            elements: &tree.elements,
            page_info,
            settings,
            table_of_contents: &tree.table_of_contents,
            footnotes: &tree.footnotes,
            bibliographies: &tree.bibliographies,
            wikitext_len: tree.wikitext_len,
            title: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TextRender, trim_outer_newlines};
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::{CodeBlock, Element, SyntaxTree};
    use std::borrow::Cow;

    #[test]
    fn text_render_entrypoints_trim_outer_newlines() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let elements = vec![Element::LineBreak, text!("body"), Element::LineBreak];

        assert_eq!(
            TextRender.render_partial(&elements, &page_info, &settings, 9),
            "body",
        );

        let tree = SyntaxTree {
            elements,
            wikitext_len: 9,
            ..SyntaxTree::default()
        };
        assert_eq!(TextRender.render(&tree, &page_info, &settings), "body");

        let mut categorized = PageInfo::dummy();
        categorized.category = Some(cow!("system"));
        assert_eq!(TextRender.render(&tree, &categorized, &settings), "body");
    }

    #[test]
    fn text_render_footnote_reference_is_its_number_in_titles_only() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let elements = vec![text!("Head"), Element::Footnote(1), text!(" tail")];

        assert_eq!(
            TextRender.render_partial_title(&elements, &page_info, &settings, 9),
            "Head1 tail",
        );
        assert_eq!(
            TextRender.render_partial(&elements, &page_info, &settings, 9),
            "Head tail",
        );
    }

    #[test]
    fn text_render_trims_many_leading_newlines_in_one_drain() {
        let mut text = "\n".repeat(10_000);
        text.push_str("body\n");

        trim_outer_newlines(&mut text);

        assert_eq!(text, "body");
    }

    #[test]
    fn text_render_partial_trims_empty_code_block_newlines() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let elements = vec![
            Element::Code(CodeBlock {
                contents: Cow::Borrowed(""),
                language: None,
                name: None,
                wikidot_block_open_elements: Vec::new(),
            }),
            Element::Code(CodeBlock {
                contents: Cow::Borrowed("body"),
                language: None,
                name: None,
                wikidot_block_open_elements: Vec::new(),
            }),
        ];

        assert_eq!(
            TextRender.render_partial(&elements, &page_info, &settings, 0),
            "body",
        );
    }
}
