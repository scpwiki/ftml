/*
 * render/html/element/image.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2025 Wikijump Team
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
use crate::tree::{AttributeMap, FloatAlignment, ImageSource, LinkLocation};
use crate::url::normalize_link;

pub fn render_image(
    ctx: &mut HtmlContext,
    source: &ImageSource,
    link: &Option<LinkLocation>,
    alignment: Option<FloatAlignment>,
    attributes: &AttributeMap,
) {
    debug!(
        "Rendering image element (source '{}', link {:?}, alignment {}, float {})",
        source.name(),
        link,
        match alignment {
            Some(image) => image.align.name(),
            None => "<default>",
        },
        match alignment {
            Some(image) => image.float,
            None => false,
        },
    );

    let source_url = ctx
        .handle()
        .get_image_link(source, ctx.info(), ctx.settings());

    match source_url {
        // Found URL
        Some(url) => render_image_element(ctx, &url, link, alignment, attributes),

        // Missing or error
        None => render_image_missing(ctx),
    }
}

fn render_image_element(
    ctx: &mut HtmlContext,
    image_url: &str,
    link: &Option<LinkLocation>,
    alignment: Option<FloatAlignment>,
    attributes: &AttributeMap,
) {
    trace!("Found URL, rendering image (value '{image_url}')");

    // Wikidot
    //
    // The structure is thus:
    // 1. If alignment, wrap in <div>. Otherwise nothing.
    // 2. If link, wrap in <a>. Otherwise nothing.
    // 3. The image itself, <img>.
    //
    // We define the closures in reverse order so
    // we can properly (conditionally) nest them.

    let render_image = |ctx: &mut HtmlContext| {
        ctx.html().img().attr(attr!(
            "src" => image_url,
            "class" => "image",
            "crossorigin";;
            attributes,
        ));
    };

    let render_link = |ctx: &mut HtmlContext, link: &LinkLocation| {
        let url = normalize_link(link, ctx.handle());
        ctx.html()
            .a()
            .attr(attr!("href" => &url))
            .inner(render_image);
    };

    let build_link = |ctx: &mut HtmlContext| match link {
        Some(link) => render_link(ctx, link),
        None => render_image(ctx),
    };

    let render_alignment = |ctx: &mut HtmlContext, align: FloatAlignment| {
        ctx.html()
            .div()
            .attr(attr!("class" => "image-container " align.wd_html_class()))
            .inner(build_link);
    };

    let build_alignment = |ctx: &mut HtmlContext| match alignment {
        Some(align) => render_alignment(ctx, align),
        None => build_link(ctx),
    };

    build_alignment(ctx);

    // XXX

    let (space, align_class) = match alignment {
        // TODO add wikidot compat
        Some(align) => (" ", align.wj_html_class()),
        None => ("", ""),
    };

    ctx.html()
        .div()
        .attr(attr!(
            "class" => "wj-image-container" space align_class,
        ))
        .inner(|ctx| {
            let build_image = |ctx: &mut HtmlContext| {
                ctx.html().img().attr(attr!(
                    "class" => "wj-image",
                    "src" => image_url,
                    "crossorigin";;
                    attributes
                ));
            };

            match link {
                Some(link) => {
                    let url = normalize_link(link, ctx.handle());
                    ctx.html()
                        .a()
                        .attr(attr!("href" => &url))
                        .inner(build_image);
                }
                None => build_image(ctx),
            };
        });
}

fn render_image_missing(ctx: &mut HtmlContext) {
    trace!("Image URL unresolved, missing or error");

    let message = ctx
        .handle()
        .get_message(ctx.language(), "image-context-bad");

    ctx.html()
        .div()
        .attr(attr!("class" => "wj-error-block"))
        .contents(message);
}
