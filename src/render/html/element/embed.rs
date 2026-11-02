/*
 * render/html/element/embed.rs
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
use crate::tree::Embed;

pub fn render_embed(ctx: &mut HtmlContext, embed: &Embed) {
    debug!(
        "Rendering embed (variant '{}', url '{}')",
        embed.name(),
        embed.direct_url(),
    );

    if let Embed::Iframe { html, .. } = embed {
        ctx.push_raw_str(html);
        return;
    }

    ctx.html()
        .div()
        .attr(attr!(
            "class" => "wj-embed",
        ))
        .inner(|ctx| match embed {
            Embed::Youtube { video_id } => {
                let url = format!("https://www.youtube.com/embed/{video_id}");

                ctx.html().iframe().attr(attr!(
                    "src" => &url,
                    "frameborder" => "0",
                    "allow" => "accelerometer; autoplay; "
                               "clipboard-write; encrypted-media; "
                               "gyroscope; picture-in-picture",
                    "allowfullscreen",
                ));
            }

            Embed::Vimeo { video_id } => {
                let url = format!("https://player.vimeo.com/video/{video_id}");

                ctx.html().iframe().attr(attr!(
                    "src" => &url,
                    "frameborder" => "0",
                    "allow" => "autoplay; fullscreen; picture-in-picture",
                    "allowfullscreen",
                ));
            }

            Embed::GithubGist { username, hash } => {
                let url = format!("https://gist.github.com/{username}/{hash}.js");

                ctx.html().script().attr(attr!("src" => &url));
            }

            Embed::Iframe { .. } => unreachable!(),

            Embed::GitlabSnippet { snippet_id } => {
                let url = format!("https://gitlab.com/-/snippets/{snippet_id}.js");

                ctx.html().script().attr(attr!("src" => &url));
            }
        });
}

#[test]
fn embed_renders_script_variants() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::{Element, SyntaxTree};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let tree = SyntaxTree {
        elements: vec![
            Element::Embed(Embed::GithubGist {
                username: cow!("octocat"),
                hash: cow!("abc123"),
            }),
            Element::Embed(Embed::GitlabSnippet {
                snippet_id: cow!("98765"),
            }),
        ],
        ..SyntaxTree::default()
    };

    let output = HtmlRender.render(&tree, &page_info, &settings);

    let expected = concat!(
        r#"<div class="wj-embed"><script src="https://gist.github.com/octocat/abc123.js"></script></div>"#,
        r#"<div class="wj-embed"><script src="https://gitlab.com/-/snippets/98765.js"></script></div>"#,
    );

    assert_eq!(output.body, expected);
}
