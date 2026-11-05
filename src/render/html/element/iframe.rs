/*
 * render/html/element/iframe.rs
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
use crate::tree::AttributeMap;
use crate::url::normalize_href;

pub fn render_iframe(ctx: &mut HtmlContext, url: &str, attributes: &AttributeMap) {
    debug!("Rendering iframe block (url '{url}')");
    let src = if ctx.layout().legacy() {
        crate::url::percent_encode_non_ascii(url)
    } else {
        normalize_href(url, None)
    };

    if ctx.layout().legacy() {
        let get = |name| {
            attributes
                .get()
                .get(name)
                .map_or("", std::convert::AsRef::as_ref)
        };
        ctx.html().iframe().attr(attr!(
            "src" => &src,
            "align" => get("align"),
            "frameborder" => get("frameborder"),
            "height" => get("height"),
            "scrolling" => get("scrolling"),
            "width" => get("width"),
            "class" => get("class"),
            "style" => get("style"),
        ));
    } else {
        ctx.html().iframe().attr(attr!(
            "src" => &src,
            "crossorigin";;
            attributes
        ));
    }
}

pub fn render_html(ctx: &mut HtmlContext, contents: &str, attributes: &AttributeMap) {
    debug!("Rendering html block (submitting to remote for iframe)");

    let iframe_url = ctx.handle().post_html(ctx.info(), contents);
    let src = normalize_href(&iframe_url, None);

    if ctx.layout().legacy() {
        ctx.html().iframe().attr(attr!(
            "src" => &src,
            "allowtransparency" => "true",
            "frameborder" => "0",
            "class" => "html-block-iframe",
        ));
    } else {
        ctx.html().iframe().attr(attr!(
            "src" => &src,
            "crossorigin";;
            attributes
        ));
    }
}
