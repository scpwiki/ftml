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
#[cfg(test)]
mod test_utils;

pub use self::meta::{HtmlMeta, HtmlMetaType};
pub use self::output::{
    EmbedVideoRequirement, GalleryRequirement, HtmlOutput, HtmlResourceRequirement,
    SocialRequirement, StandaloneButtonRequirement, WikidotTabViewRequirement,
};

use self::context::HtmlContext;
use self::element::{preregister_wikidot_equations, render_element, render_elements};
use crate::data::PageInfo;
use crate::render::{Handle, PageExistenceResolver, Render, UserInfoResolver};
use crate::settings::WikitextSettings;
use crate::tree::{
    Element, SyntaxTree, run_on_bounded_tree_stack, tree_requires_bounded_tree_stack,
};

#[derive(Debug)]
pub struct HtmlRender;

impl HtmlRender {
    pub fn render_with_page_existence(
        &self,
        tree: &SyntaxTree,
        page_info: &PageInfo,
        settings: &WikitextSettings,
        page_existence: &dyn PageExistenceResolver,
    ) -> HtmlOutput {
        self.render_with_resolvers(tree, page_info, settings, page_existence, &Handle)
    }

    pub fn render_with_user_info(
        &self,
        tree: &SyntaxTree,
        page_info: &PageInfo,
        settings: &WikitextSettings,
        user_info: &dyn UserInfoResolver,
    ) -> HtmlOutput {
        self.render_with_resolvers(tree, page_info, settings, &Handle, user_info)
    }

    pub fn render_with_resolvers(
        &self,
        tree: &SyntaxTree,
        page_info: &PageInfo,
        settings: &WikitextSettings,
        page_existence: &dyn PageExistenceResolver,
        user_info: &dyn UserInfoResolver,
    ) -> HtmlOutput {
        if tree_requires_bounded_tree_stack(tree) {
            run_on_bounded_tree_stack("ftml-html-render", || {
                self.render_with_resolvers_on_current_stack(
                    tree,
                    page_info,
                    settings,
                    page_existence,
                    user_info,
                )
            })
        } else {
            self.render_with_resolvers_on_current_stack(
                tree,
                page_info,
                settings,
                page_existence,
                user_info,
            )
        }
    }

    fn render_with_resolvers_on_current_stack(
        &self,
        tree: &SyntaxTree,
        page_info: &PageInfo,
        settings: &WikitextSettings,
        page_existence: &dyn PageExistenceResolver,
        user_info: &dyn UserInfoResolver,
    ) -> HtmlOutput {
        debug!(
            "Rendering HTML (site {}, page {}, category {})",
            page_info.site.as_ref(),
            page_info.page.as_ref(),
            match &page_info.category {
                Some(category) => category.as_ref(),
                None => "_default",
            },
        );

        let mut ctx = HtmlContext::with_resolvers(
            page_info,
            (page_existence, user_info),
            settings,
            &tree.table_of_contents,
            &tree.footnotes,
            &tree.bibliographies,
            tree.wikitext_len,
        );

        if settings.layout.legacy() {
            preregister_wikidot_equations(&mut ctx, tree);
        }
        render_contents(&mut ctx, tree);
        ctx.into()
    }
}

impl Render for HtmlRender {
    type Output = HtmlOutput;

    fn render(
        &self,
        tree: &SyntaxTree,
        page_info: &PageInfo,
        settings: &WikitextSettings,
    ) -> HtmlOutput {
        self.render_with_page_existence(tree, page_info, settings, &Handle)
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
                wikidot_wrappers: Vec::new(),
                wikidot_inline_suffix: false,
                wikidot_inline_prefix: false,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::UserInfo;
    use crate::layout::Layout;
    use crate::render::UserInfoResolver;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::Element;

    struct CanonicalUser;

    impl UserInfoResolver for CanonicalUser {
        fn user_info(&self, name: &str) -> Option<UserInfo<'static>> {
            (name == "SYSTEM").then(|| {
                let mut info = UserInfo::dummy();
                info.user_id = 42;
                info.user_slug = cow!("system");
                info.user_name = cow!("system");
                info.user_profile_url = cow!("http://www.wikidot.com/user:info/system");
                info
            })
        }
    }

    #[test]
    fn html_render_collects_style_elements_without_emitting_body_style_tags() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tree = SyntaxTree {
            elements: vec![
                Element::Style(cow!(".first { color: red; }")),
                Element::Text(cow!("body")),
                Element::Style(cow!(".second { color: blue; }")),
            ],
            ..SyntaxTree::default()
        };

        let output = HtmlRender.render(&tree, &page_info, &settings);

        assert_eq!(
            output.styles,
            vec![
                ".first{color:red}".to_owned(),
                ".second{color:#00f}".to_owned(),
            ],
        );
        assert_eq!(output.body, "body");
    }

    #[test]
    fn wikidot_user_render_uses_resolved_canonical_identity() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tree = SyntaxTree {
            elements: vec![Element::User {
                name: cow!("SYSTEM"),
                show_avatar: false,
            }],
            ..SyntaxTree::default()
        };

        let output = HtmlRender.render_with_resolvers(
            &tree,
            &page_info,
            &settings,
            &Handle,
            &CanonicalUser,
        );

        assert_eq!(
            output.body,
            r#"<span class="printuser"><a href="http://www.wikidot.com/user:info/system" onclick="WIKIDOT.page.listeners.userInfo(42); return false;">system</a></span>"#,
        );
    }

    #[test]
    fn wikidot_avatar_karma_image_preserves_live_http_url() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tree = SyntaxTree {
            elements: vec![Element::User {
                name: cow!("SYSTEM"),
                show_avatar: true,
            }],
            ..SyntaxTree::default()
        };

        let output = HtmlRender.render_with_resolvers(
            &tree,
            &page_info,
            &settings,
            &Handle,
            &CanonicalUser,
        );

        assert!(
            output.body.contains(
                "background-image:url(http://www.wikidot.com/userkarma.php?u=42)"
            ),
            "{}",
            output.body,
        );
    }

    struct HttpsAvatarUser;

    impl UserInfoResolver for HttpsAvatarUser {
        fn user_info(&self, name: &str) -> Option<UserInfo<'static>> {
            CanonicalUser.user_info(name).map(|mut info| {
                info.user_avatar_data = cow!(
                    "https://www.wikidot.com/avatar.php?userid=42&amp;size=small&amp;timestamp=1791147832"
                );
                info
            })
        }
    }

    #[test]
    fn wikidot_avatar_karma_image_follows_the_avatar_base() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tree = SyntaxTree {
            elements: vec![Element::User {
                name: cow!("SYSTEM"),
                show_avatar: true,
            }],
            ..SyntaxTree::default()
        };

        let output = HtmlRender.render_with_resolvers(
            &tree,
            &page_info,
            &settings,
            &Handle,
            &HttpsAvatarUser,
        );

        assert!(
            output.body.contains(concat!(
                r#"src="https://www.wikidot.com/avatar.php?userid=42&amp;amp;size=small&amp;amp;timestamp=1791147832" "#,
                r#"alt="system" style="background-image:url(https://www.wikidot.com/userkarma.php?u=42)""#,
            )),
            "{}",
            output.body,
        );
    }

    #[test]
    fn page_language_localizes_default_footnote_heading() {
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tree = SyntaxTree {
            elements: vec![Element::Footnote(1.into())],
            footnotes: vec![vec![Element::Text(cow!("note body"))]],
            needs_footnote_block: true,
            ..SyntaxTree::default()
        };

        for language in ["ja", "ja-JP"] {
            let mut page_info = PageInfo::dummy();
            page_info.language = cow!(language);
            let output = HtmlRender.render(&tree, &page_info, &settings);

            assert!(output.body.contains("<div class=\"wj-title\">脚注</div>"));
            assert!(output.body.contains("aria-label=\"脚注 1.\""));
            assert!(!output.body.contains(">Footnotes<"));
        }
    }

    #[test]
    fn english_footnote_output_is_unchanged() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tree = SyntaxTree {
            elements: vec![Element::Footnote(1.into())],
            footnotes: vec![vec![Element::Text(cow!("note body"))]],
            needs_footnote_block: true,
            ..SyntaxTree::default()
        };

        let output = HtmlRender.render(&tree, &page_info, &settings);

        assert!(
            output
                .body
                .contains("<div class=\"wj-title\">Footnotes</div>")
        );
        assert!(output.body.contains("aria-label=\"Footnote 1.\""));
    }

    #[test]
    fn wikidot_footnotes_use_the_live_legacy_dom() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tree = SyntaxTree {
            elements: vec![Element::Footnote(1.into())],
            footnotes: vec![vec![Element::Text(cow!("note body"))]],
            needs_footnote_block: true,
            ..SyntaxTree::default()
        };

        let output = HtmlRender.render(&tree, &page_info, &settings);

        assert_eq!(
            output.body,
            concat!(
                "<sup class=\"footnoteref\"><a id=\"footnoteref-1\" href=\"javascript:;\" class=\"footnoteref\" onclick=\"WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)\">1</a></sup>",
                "<div class=\"footnotes-footer\"><div class=\"title\">Footnotes</div>",
                "<div class=\"footnote-footer\" id=\"footnote-1\"><a href=\"javascript:;\" onclick=\"WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)\">1</a>. note body</div></div>",
            ),
        );
    }
}
