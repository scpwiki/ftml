/*
 * render/html/element/image.rs
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
use crate::tree::{
    AnchorTarget, AttributeMap, FloatAlignment, ImageSource, LinkLocation,
};
use crate::url::{dangerous_scheme, normalize_link};

pub fn render_image(
    ctx: &mut HtmlContext,
    source: &ImageSource,
    link: &Option<LinkLocation>,
    alignment: Option<FloatAlignment>,
    attributes: &AttributeMap,
    target: Option<AnchorTarget>,
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

    match source {
        ImageSource::Direct(source) => {
            match ctx
                .handle()
                .get_file_link(source, ctx.info(), ctx.settings())
            {
                Some(url) => render_image_element(
                    ctx, &url, link, None, None, alignment, attributes, target,
                ),
                None => render_image_missing(ctx),
            }
        }
        ImageSource::ImplicitAttachment {
            file,
            page,
            alt,
            size,
        } => {
            match ctx.handle().get_implicit_attachment_image_links(
                file,
                page.as_deref(),
                *size,
                ctx.info(),
                ctx.settings(),
            ) {
                Some((display_url, original_url)) => render_image_element(
                    ctx,
                    &display_url,
                    link,
                    Some(&original_url),
                    Some(alt),
                    alignment,
                    attributes,
                    target,
                ),
                None => render_image_missing(ctx),
            }
        }
    }
}

fn render_image_element(
    ctx: &mut HtmlContext,
    image_url: &str,
    link: &Option<LinkLocation>,
    attachment_link: Option<&str>,
    attachment_alt: Option<&str>,
    alignment: Option<FloatAlignment>,
    attributes: &AttributeMap,
    target: Option<AnchorTarget>,
) {
    trace!("Found URL, rendering image (value '{image_url}')");

    match ctx.layout() {
        Layout::Wikidot => {
            render_image_element_wikidot(
                ctx,
                image_url,
                link,
                attachment_link,
                attachment_alt,
                alignment,
                attributes,
                target,
            );
        }
        Layout::Wikijump => {
            render_image_element_wikijump(
                ctx, image_url, link, alignment, attributes, target,
            );
        }
    }
}

fn render_image_element_wikidot(
    ctx: &mut HtmlContext,
    image_url: &str,
    link: &Option<LinkLocation>,
    attachment_link: Option<&str>,
    attachment_alt: Option<&str>,
    alignment: Option<FloatAlignment>,
    attributes: &AttributeMap,
    target: Option<AnchorTarget>,
) {
    let target_value = match target {
        Some(target) => target.html_attr(),
        None => "",
    };
    let encoded_wikidot_prefix = image_url.starts_with('%')
        && (image_url.contains("http://") || image_url.contains("https://"));
    let safety_url = image_url
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            image_url
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(image_url);
    let invalid_url = dangerous_scheme(safety_url) && !encoded_wikidot_prefix;
    let image_url = if invalid_url {
        "#invalid-url"
    } else {
        image_url
    };
    let get = |name| attributes.get().get(name).map(|value| value.as_ref());
    // Live Wikidot derives the default alt text from everything after the
    // last slash, keeping any query string or fragment. A rejected source
    // has no usable name, so its alt stays empty.
    let default_alt = if invalid_url {
        ""
    } else {
        image_url
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("")
    };
    let alt = get("alt").or(attachment_alt).unwrap_or(default_alt);
    let class = get("class").unwrap_or("image");
    let width = get("width");
    let height = get("height");
    let title = get("title");
    let style = get("style");

    let build_image = |ctx: &mut HtmlContext| {
        let mut image = ctx.html().img();
        image.attr(attr!(
            "src" => image_url,
            "width" => width.unwrap_or(""); if width.is_some(),
            "height" => height.unwrap_or(""); if height.is_some(),
            "title" => title.unwrap_or(""); if title.is_some(),
            "style" => style.unwrap_or(""); if style.is_some(),
            "class" => class,
        ));
        image.attr_quoted("alt", alt);
    };

    let build_link = |ctx: &mut HtmlContext| match link {
        Some(link) => {
            let url = normalize_link(link, ctx.handle());
            ctx.html()
                .a()
                .attr(
                    attr!("href" => &url, "target" => target_value; if target.is_some()),
                )
                .inner(build_image);
        }
        None => match attachment_link {
            Some(url) => {
                ctx.html().a().attr(attr!("href" => url)).inner(build_image);
            }
            None => build_image(ctx),
        },
    };

    match alignment {
        None => build_link(ctx),
        Some(align) => {
            let class = if align.float && align.align == crate::tree::Alignment::Center {
                str!("image-container")
            } else {
                format!("image-container {}", align.wd_html_class())
            };
            ctx.html()
                .div()
                .attr(attr!("class" => &class))
                .inner(build_link);
        }
    }
}

fn render_image_element_wikijump(
    ctx: &mut HtmlContext,
    image_url: &str,
    link: &Option<LinkLocation>,
    alignment: Option<FloatAlignment>,
    attributes: &AttributeMap,
    target: Option<AnchorTarget>,
) {
    let target_value = match target {
        Some(target) => target.html_attr(),
        None => "",
    };
    let opens_new_tab = matches!(target, Some(AnchorTarget::NewTab));

    let (space, align_class) = match alignment {
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
                    "src" => image_url;;
                    attributes
                ));
            };

            match link {
                Some(link) => {
                    let url = normalize_link(link, ctx.handle());
                    ctx.html()
                        .a()
                        .attr(attr!(
                            "href" => &url,
                            "target" => target_value; if target.is_some(),
                            "rel" => "noopener noreferrer"; if opens_new_tab,
                        ))
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
        .inner(|ctx| ctx.push_escaped(message));
}

#[test]
fn image_renders_missing_for_canonical_local_files_when_local_paths_are_disabled() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::{Element, SyntaxTree};

    let page_info = PageInfo::dummy();
    let mut settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    settings.allow_local_paths = false;
    let tree = SyntaxTree {
        elements: vec![Element::Image {
            source: ImageSource::Direct(crate::tree::FileSource::Url(cow!(
                "/local--files/private-page/secret.png"
            ))),
            link: None,
            alignment: None,
            attributes: AttributeMap::new(),
            target: None,
        }],
        ..SyntaxTree::default()
    };

    let output = HtmlRender.render(&tree, &page_info, &settings);
    assert_eq!(
        output.body,
        r#"<div class="wj-error-block">No images in this context</div>"#,
    );
}
