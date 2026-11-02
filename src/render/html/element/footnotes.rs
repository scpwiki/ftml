/*
 * render/html/element/footnotes.rs
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
use crate::render::text::TextRender;
use crate::tree::ContainerType;
use std::num::NonZeroUsize;

pub fn render_footnote(ctx: &mut HtmlContext, index: usize) {
    debug!("Rendering footnote reference");

    let Some(index) = NonZeroUsize::new(index) else {
        render_missing_footnote(ctx, index);
        return;
    };
    let id = str!(index);

    let footnote_string = ctx.handle().get_message(ctx.language(), "footnote");
    let label = format!("{footnote_string} {index}.");

    let Some(contents) = ctx.get_footnote(index) else {
        render_missing_footnote(ctx, index.get());
        return;
    };

    if ctx.layout() == Layout::Wikidot {
        let reference_id = format!("footnoteref-{index}");
        let footnote_id = format!("footnote-{index}");
        let onclick = format!("WIKIDOT.page.utils.scrollToReference('{footnote_id}')");
        ctx.html()
            .sup()
            .attr(attr!("class" => "footnoteref"))
            .inner(|ctx| {
                ctx.html()
                    .a()
                    .attr(attr!(
                        "id" => &reference_id,
                        "href" => "javascript:;",
                        "class" => "footnoteref",
                        "onclick" => &onclick,
                    ))
                    .contents(&id);
            });
        return;
    }

    // A footnote reference is phrasing content and is commonly rendered inside a
    // paragraph. Preserve rich inline markup when possible, but flatten block
    // footnotes to escaped text for the hover preview. The full footnote block
    // still renders the original structure below the article.
    let tooltip_text = (!contents.iter().all(|element| {
        !matches!(element, Element::Partial(_)) && element.paragraph_safe()
    }))
    .then(|| TextRender.render_partial(contents, ctx.info(), ctx.settings(), 0));

    if tooltip_text.is_none() && !ctx.enter_footnote_ref(index.get()) {
        render_missing_footnote(ctx, index.get());
        return;
    }

    ctx.html()
        .span()
        .attr(attr!("class" => "wj-footnote-ref"))
        .inner(|ctx| {
            ctx.html()
                .element("wj-footnote-ref-marker")
                .attr(attr!(
                    "class" => "wj-footnote-ref-marker",
                    "role" => "link",
                    "aria-label" => &label,
                    "data-id" => &id,
                ))
                .contents(&id);

            // Tooltip shown on hover.
            // Is aria-hidden due to difficulty in getting a simultaneous
            // tooltip and link to work. A screen reader can still navigate
            // through to the link and read the footnote directly.
            ctx.html()
                .span()
                .attr(attr!(
                    "class" => "wj-footnote-ref-tooltip",
                    "aria-hidden" => "true",
                ))
                .inner(|ctx| {
                    ctx.html()
                        .span()
                        .attr(attr!("class" => "wj-footnote-ref-tooltip-label"))
                        .contents(&label);

                    ctx.html()
                        .span()
                        .attr(attr!("class" => "wj-footnote-ref-contents"))
                        .inner(|ctx| match tooltip_text.as_deref() {
                            Some(text) => render_tooltip_text(ctx, text),
                            None => render_elements(ctx, contents),
                        });
                });
        });
    if tooltip_text.is_none() {
        ctx.exit_footnote_ref(index.get());
    }
}

fn render_missing_footnote(ctx: &mut HtmlContext, index: usize) {
    warn!("Footnote index out of bounds from gathered footnote list: {index}");
    let message = ctx
        .handle()
        .get_message(ctx.language(), "footnote-cite-not-found");

    ctx.html()
        .span()
        .attr(attr!("class" => "wj-error-inline"))
        .inner(|ctx| ctx.push_escaped(message));
}

fn render_tooltip_text(ctx: &mut HtmlContext, text: &str) {
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            ctx.html().br();
        }
        ctx.push_escaped(line);
    }
}

pub fn render_footnote_block(ctx: &mut HtmlContext, title: Option<&str>) {
    debug!(
        "Rendering footnote block (title {})",
        title.unwrap_or("<default>"),
    );

    let title_default;
    let title: &str = match title {
        Some(title) => title,
        None => {
            title_default = ctx
                .handle()
                .get_message(ctx.language(), "footnote-block-title");

            title_default
        }
    };

    if ctx.layout() == Layout::Wikidot {
        ctx.html()
            .div()
            .attr(attr!("class" => "footnotes-footer"))
            .inner(|ctx| {
                ctx.html()
                    .div()
                    .attr(attr!("class" => "title"))
                    .contents(title);
                for (index, contents) in ctx.footnotes().iter().enumerate() {
                    let index = index + 1;
                    let id = index.to_string();
                    let footnote_id = format!("footnote-{index}");
                    let onclick = format!(
                        "WIKIDOT.page.utils.scrollToReference('footnoteref-{index}')"
                    );
                    ctx.html()
                        .div()
                        .attr(attr!(
                            "class" => "footnote-footer",
                            "id" => &footnote_id,
                        ))
                        .inner(|ctx| {
                            ctx.html()
                                .a()
                                .attr(attr!(
                                    "href" => "javascript:;",
                                    "onclick" => &onclick,
                                ))
                                .contents(&id);
                            ctx.push_raw('.');
                            ctx.push_raw(' ');
                            render_wikidot_footnote_contents(ctx, contents);
                        });
                }
            });
        return;
    }

    ctx.html()
        .div()
        .attr(attr!("class" => "wj-footnote-list"))
        .inner(|ctx| {
            ctx.html()
                .div()
                .attr(attr!("class" => "wj-title"))
                .contents(title);

            ctx.html().ol().inner(|ctx| {
                for (index, contents) in ctx.footnotes().iter().enumerate() {
                    let index = index + 1;
                    let id = &format!("{index}");

                    ctx.html()
                        .li()
                        .attr(attr!(
                            "class" => "wj-footnote-list-item",
                            "data-id" => id,
                        ))
                        .inner(|ctx| {
                            ctx.html()
                                .element("wj-footnote-list-item-marker")
                                .attr(attr!(
                                    "class" => "wj-footnote-list-item-marker",
                                    "type" => "button",
                                    "role" => "link",
                                ))
                                .inner(|ctx| {
                                    str_write!(ctx, "{index}");

                                    ctx.html()
                                        .span()
                                        .attr(attr!("class" => "wj-footnote-sep"))
                                        .inner(|ctx| ctx.push_raw('.'));
                                });

                            ctx.html()
                                .div()
                                .attr(attr!("class" => "wj-footnote-list-item-contents"))
                                .contents(contents);
                        });
                }
            });
        });
}

fn render_wikidot_footnote_contents(ctx: &mut HtmlContext, contents: &[Element]) {
    for element in contents {
        if matches!(
            element,
            Element::Container(container) if container.ctype() == ContainerType::Paragraph
        ) {
            ctx.push_raw('\n');
        }
        render_element(ctx, element);
    }
}
