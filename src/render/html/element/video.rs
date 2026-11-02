/*
 * render/html/element/video.rs
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
use crate::tree::{AttributeMap, FileSource, FloatAlignment};

pub fn render_video(
    ctx: &mut HtmlContext,
    source: &FileSource,
    alignment: Option<FloatAlignment>,
    attributes: &AttributeMap,
) {
    debug!(
        "Rendering video element (source '{}', alignment {}, float {})",
        source.name(),
        match alignment {
            Some(video) => video.align.name(),
            None => "<default>",
        },
        match alignment {
            Some(video) => video.float,
            None => false,
        },
    );

    let source_url = ctx
        .handle()
        .get_file_link(source, ctx.info(), ctx.settings());

    match source_url {
        Some(url) => render_video_element(ctx, &url, alignment, attributes),

        None => render_video_missing(ctx),
    }
}

fn render_video_element(
    ctx: &mut HtmlContext,
    video_url: &str,
    alignment: Option<FloatAlignment>,
    attributes: &AttributeMap,
) {
    trace!("Found URL, rendering video (value '{video_url}')");

    match ctx.layout() {
        Layout::Wikidot => {
            render_video_element_wikidot(ctx, video_url, alignment, attributes)
        }
        Layout::Wikijump => {
            render_video_element_wikijump(ctx, video_url, alignment, attributes)
        }
    }
}

fn render_video_element_wikidot(
    ctx: &mut HtmlContext,
    video_url: &str,
    alignment: Option<FloatAlignment>,
    attributes: &AttributeMap,
) {
    let align_class = alignment.map(FloatAlignment::wd_html_class).unwrap_or("");
    let align_space = if alignment.is_some() { " " } else { "" };

    ctx.html()
        .tag("video")
        .attr(attr!(
            "class" => "video" align_space align_class,
            "controls",
            "preload" => "metadata",
            "crossorigin";;
            attributes,
        ))
        .inner(|ctx| {
            ctx.html().tag("source").attr(attr!("src" => video_url));
        });
}

fn render_video_element_wikijump(
    ctx: &mut HtmlContext,
    video_url: &str,
    alignment: Option<FloatAlignment>,
    attributes: &AttributeMap,
) {
    let align_class = alignment.map(FloatAlignment::wj_html_class).unwrap_or("");
    let align_space = if alignment.is_some() { " " } else { "" };

    ctx.html()
        .tag("video")
        .attr(attr!(
            "class" => "wj-video" align_space align_class,
            "controls",
            "preload" => "metadata",
            "crossorigin";;
            attributes
        ))
        .inner(|ctx| {
            ctx.html().tag("source").attr(attr!("src" => video_url));
        });
}

fn render_video_missing(ctx: &mut HtmlContext) {
    trace!("Video URL unresolved, missing or error");

    let message = ctx
        .handle()
        .get_message(ctx.language(), "video-context-bad");

    ctx.html()
        .div()
        .attr(attr!("class" => "wj-error-block"))
        .inner(|ctx| ctx.push_escaped(message));
}

#[test]
fn video_renders_missing_when_local_paths_are_disabled() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::{Element, SyntaxTree};

    let page_info = PageInfo::dummy();
    let mut settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    settings.allow_local_paths = false;

    for source in [
        FileSource::File1 {
            file: cow!("local.mp4"),
        },
        FileSource::Url(cow!("/local--files/private-page/secret.mp4")),
    ] {
        let tree = SyntaxTree {
            elements: vec![Element::Video {
                source,
                alignment: None,
                attributes: AttributeMap::new(),
            }],
            ..SyntaxTree::default()
        };
        let output = HtmlRender.render(&tree, &page_info, &settings);
        assert_eq!(
            output.body,
            r#"<div class="wj-error-block">No videos in this context</div>"#,
        );
    }
}
