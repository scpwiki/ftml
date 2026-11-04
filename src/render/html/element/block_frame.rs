/*
 * render/html/element/block_frame.rs
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

use super::collapsible::render_wikidot_frame_link;
use super::prelude::*;
use crate::tree::{AttributeMap, Element, WikidotFrameLink};

#[derive(Debug, Copy, Clone)]
pub struct BlockFrame<'a> {
    pub(super) attributes: &'a AttributeMap<'a>,
    pub(super) children: &'a [Element<'a>],
    pub(super) collapsible_link: Option<&'a WikidotFrameLink<'a>>,
    pub(super) omit_close: bool,
}

pub fn render_block_frame(ctx: &mut HtmlContext, frame: BlockFrame) {
    let BlockFrame {
        attributes,
        children,
        collapsible_link,
        omit_close,
    } = frame;

    debug!(
        "Rendering wikidot block frame (children length {}, omit-close {})",
        children.len(),
        omit_close,
    );

    let mut tag = ctx.html().div();
    tag.attr(attr!(;; attributes));
    if omit_close {
        tag.omit_close();
    }
    tag.inner(|ctx| {
        if let Some(link) = collapsible_link {
            render_wikidot_frame_link(
                ctx,
                link.text.as_deref(),
                link.show,
                &link.spans,
            );
        }
        render_elements(ctx, children);
    });
}
