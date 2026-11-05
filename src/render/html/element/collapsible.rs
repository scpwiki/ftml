/*
 * render/html/element/collapsible.rs
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
use crate::tree::{AttributeMap, Element};

#[derive(Debug, Copy, Clone)]
pub struct Collapsible<'a> {
    pub(super) elements: &'a [Element<'a>],
    pub(super) unfolded_tail_start: Option<usize>,
    pub(super) wikidot_label_spans: &'a [AttributeMap<'a>],
    pub(super) wikidot_block_open_elements: &'a [Element<'a>],
    pub(super) attributes: &'a AttributeMap<'a>,
    pub(super) start_open: bool,
    pub(super) show_text: Option<&'a str>,
    pub(super) hide_text: Option<&'a str>,
    pub(super) show_top: bool,
    pub(super) show_bottom: bool,
}

#[derive(Debug, Copy, Clone)]
struct CollapsibleLabels<'a> {
    start_open: bool,
    show_text: &'a str,
    hide_text: &'a str,
    show_top: bool,
    show_bottom: bool,
}

pub fn render_collapsible(ctx: &mut HtmlContext, collapsible: Collapsible) {
    let Collapsible {
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
    } = collapsible;

    debug!(
        "Rendering collapsible (elements length {}, start-open {}, show-text {}, hide-text {}, show-top {}, show-bottom {})",
        elements.len(),
        start_open,
        show_text.unwrap_or("<default>"),
        hide_text.unwrap_or("<default>"),
        show_top,
        show_bottom,
    );

    match ctx.layout() {
        Layout::Wikidot => {
            let (elements, unfolded_tail) = unfolded_tail_start
                .map(|start| elements.split_at(start.min(elements.len())))
                .unwrap_or((elements, &[]));
            let show_text = wikidot_link_label(show_text, true);
            let hide_text = wikidot_link_label(hide_text, false);
            render_collapsible_wikidot(
                ctx,
                elements,
                unfolded_tail,
                wikidot_label_spans,
                wikidot_block_open_elements,
                CollapsibleLabels {
                    start_open,
                    show_text,
                    hide_text,
                    show_top,
                    show_bottom,
                },
            );
        }
        Layout::Wikijump => {
            let show_text = show_text.unwrap_or_else(|| {
                ctx.handle().get_message(ctx.language(), "collapsible-open")
            });
            let hide_text = hide_text.unwrap_or_else(|| {
                ctx.handle().get_message(ctx.language(), "collapsible-hide")
            });
            render_collapsible_wikijump(
                ctx,
                elements,
                attributes,
                CollapsibleLabels {
                    start_open,
                    show_text,
                    hide_text,
                    show_top,
                    show_bottom,
                },
            );
        }
    }
}

fn render_collapsible_wikidot(
    ctx: &mut HtmlContext,
    elements: &[Element],
    unfolded_tail: &[Element],
    label_spans: &[AttributeMap],
    block_open_elements: &[Element],
    labels: CollapsibleLabels,
) {
    let CollapsibleLabels {
        start_open,
        show_text,
        hide_text,
        show_top,
        show_bottom,
    } = labels;

    ctx.html()
        .div()
        .attr(attr!("class" => "collapsible-block"))
        .inner(|ctx| {
            render_elements(ctx, block_open_elements);

            ctx.html()
                .div()
                .attr(attr!(
                    "class" => "collapsible-block-folded",
                    "style" => "display:none"; if start_open,
                ))
                .inner(|ctx| {
                    render_wikidot_collapsible_link_with_spans(
                        ctx,
                        show_text,
                        label_spans,
                    )
                });

            ctx.html()
                .div()
                .attr(attr!(
                    "class" => "collapsible-block-unfolded",
                    "style" => "display:none"; if !start_open,
                ))
                .inner(|ctx| {
                    if show_top {
                        render_wikidot_hide_link(ctx, hide_text, label_spans);
                    }

                    ctx.html()
                        .div()
                        .attr(attr!("class" => "collapsible-block-content"))
                        .contents(elements);

                    render_elements(ctx, unfolded_tail);

                    if show_bottom {
                        render_wikidot_hide_link(ctx, hide_text, label_spans);
                    }
                });
        });
}

fn render_wikidot_hide_link(
    ctx: &mut HtmlContext,
    hide_text: &str,
    label_spans: &[AttributeMap],
) {
    ctx.html()
        .div()
        .attr(attr!("class" => "collapsible-block-unfolded-link"))
        .inner(|ctx| {
            render_wikidot_collapsible_link_with_spans(ctx, hide_text, label_spans)
        });
}

fn wikidot_link_label(text: Option<&str>, show: bool) -> &str {
    let default = if show {
        "+ show block"
    } else {
        "– hide block"
    };
    text.filter(|text| !text.is_empty() && *text != "0")
        .unwrap_or(default)
}

pub(super) fn render_wikidot_frame_link(
    ctx: &mut HtmlContext,
    text: Option<&str>,
    show: bool,
    label_spans: &[AttributeMap],
) {
    render_wikidot_collapsible_link_with_spans(
        ctx,
        wikidot_link_label(text, show),
        label_spans,
    );
}

fn render_wikidot_collapsible_link_with_spans(
    ctx: &mut HtmlContext,
    label: &str,
    label_spans: &[AttributeMap],
) {
    fn render_wrapped(ctx: &mut HtmlContext, label: &str, label_spans: &[AttributeMap]) {
        let Some((attributes, remaining)) = label_spans.split_first() else {
            render_wikidot_collapsible_link(ctx, label);
            return;
        };
        ctx.html()
            .span()
            .attr(attr!(;; attributes))
            .inner(|ctx| render_wrapped(ctx, label, remaining));
    }
    render_wrapped(ctx, label, label_spans);
}

fn render_wikidot_collapsible_link(ctx: &mut HtmlContext, label: &str) {
    ctx.html()
        .a()
        .attr(attr!(
            "class" => "collapsible-block-link",
            "href" => "javascript:;",
        ))
        .inner(|ctx| {
            for ch in label.chars() {
                match ch {
                    ' ' | '\u{00A0}' => ctx.push_raw_str("&nbsp;"),
                    '\t' => ctx.push_raw_str("&nbsp;&nbsp;&nbsp;&nbsp;"),
                    '\n' | '\r' => ctx.push_raw_str(" "),
                    '\0'..='\u{001F}' | '\u{007F}' => {}
                    _ => {
                        let mut buffer = [0; 4];
                        ctx.push_escaped(ch.encode_utf8(&mut buffer));
                    }
                }
            }
        });
}

fn render_collapsible_wikijump(
    ctx: &mut HtmlContext,
    elements: &[Element],
    attributes: &AttributeMap,
    labels: CollapsibleLabels,
) {
    let CollapsibleLabels {
        start_open,
        show_text,
        hide_text,
        show_top,
        show_bottom,
    } = labels;

    ctx.html()
        .details()
        .attr(attr!(
            "class" => "wj-collapsible",
            "open"; if start_open,
            "data-show-top"; if show_top,
            "data-show-bottom"; if show_bottom;;
            attributes,
        ))
        .inner(|ctx| {
            ctx.html()
                .summary()
                .attr(attr!(
                    "class" => "wj-collapsible-button wj-collapsible-button-top",
                ))
                .inner(|ctx| {
                    ctx.html()
                        .span()
                        .attr(attr!("class" => "wj-collapsible-show-text"))
                        .contents(show_text);

                    ctx.html()
                        .span()
                        .attr(attr!("class" => "wj-collapsible-hide-text"))
                        .contents(hide_text);
                });

            ctx.html()
                .div()
                .attr(attr!("class" => "wj-collapsible-content"))
                .contents(elements);

            if show_bottom {
                ctx.html()
                    .element("wj-collapsible-button-bottom")
                    .attr(attr!(
                        "class" => "wj-collapsible-button wj-collapsible-button-bottom",
                    ))
                    .inner(|ctx| {
                        ctx.html()
                            .span()
                            .attr(attr!("class" => "wj-collapsible-hide-text"))
                            .contents(hide_text);
                    });
            }
        });
}
