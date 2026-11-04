/*
 * parsing/rule/impls/block/blocks/image.rs
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
use crate::delayed::{DelayedElement, GeneratedImageAttribute, GeneratedKind};
use crate::parsing::discard_wikidot_controls;
use crate::tree::{
    AnchorTarget, FileSource, FloatAlignment, ImageSize, ImageSource, LinkLocation,
};
use crate::url::is_url;
use std::borrow::Cow;

pub const BLOCK_IMAGE: BlockRule = BlockRule {
    name: "block-image",
    accepts_names: &[
        "image", "=image", "<image", ">image", "f<image", "f=image", "f>image",
    ],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: false,
    parse_fn,
};

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing image block (name {name}, in-head {in_head})");
    assert!(!flag_star, "Image doesn't allow star flag");
    assert!(!flag_score, "Image doesn't allow score flag");
    assert_block_name(&BLOCK_IMAGE, name);

    let generated = parser.generated_until_right_block();
    if generated
        .iter()
        .any(|slot| slot.kind == GeneratedKind::PageLink)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let generated_image = match generated.as_slice() {
        [] => None,
        [slot] => {
            let (key, suffix) = delayed_image_attribute(parser.full_text().inner(), slot)
                .ok_or_else(|| parser.make_err(ParseErrorKind::RuleFailed))?;
            let attribute = match key {
                "alt" => GeneratedImageAttribute::Alt,
                "link" => GeneratedImageAttribute::Link,
                _ => return Err(parser.make_err(ParseErrorKind::RuleFailed)),
            };
            Some((slot.clone(), attribute, suffix))
        }
        _ => return Err(parser.make_err(ParseErrorKind::RuleFailed)),
    };

    let (source, source_prefix_is_url, mut arguments) =
        if parser.settings().layout.legacy() {
            let (source, arguments) =
                parser.get_head_field_map_wikidot(&BLOCK_IMAGE, in_head)?;
            let source_prefix_is_url = is_url(source.prefix_before_first_comment());
            (
                discard_wikidot_controls(source.into_cow()),
                source_prefix_is_url,
                arguments,
            )
        } else {
            let (source, arguments) = parser.get_head_field_map(&BLOCK_IMAGE, in_head)?;
            let source_prefix_is_url = is_url(&source);
            (source, source_prefix_is_url, arguments)
        };
    let (link, target) = if parser.settings().layout.legacy() {
        match arguments.get("link") {
            None => (None, None),
            Some(value) => {
                let value = discard_wikidot_controls(value);
                // Wikidot does not resolve a link target that starts with
                // whitespace, with or without a leading `*` marker: the whole
                // block stays literal.
                let location = value.strip_prefix('*').unwrap_or(&value);
                if location.starts_with(char::is_whitespace) {
                    return Err(parser.make_err(ParseErrorKind::RuleFailed));
                }
                let (value, target) = split_image_link_target(value);
                let link = parse_wikidot_image_link_target(value)
                    .ok_or_else(|| parser.make_err(ParseErrorKind::RuleFailed))?;
                (Some(link), target)
            }
        }
    } else {
        match arguments.get_with_bare("link") {
            None => (None, None),
            Some((value, bare)) => {
                let (value, target) = split_image_link_target(value);
                let link = if bare && value == "#" {
                    None
                } else {
                    Some(LinkLocation::parse(value))
                };
                (link, target)
            }
        }
    };
    let alignment = FloatAlignment::parse(name);
    // Only a quoted, recognized size name selects a resize explicitly. An
    // absent, unquoted, empty, or unknown size (including "original") falls
    // back to the file-type default in `classify_wikidot_image_source`.
    let explicit_attachment_size = if parser.settings().layout.legacy() {
        arguments
            .get("size")
            .and_then(|value| ImageSize::parse_wikidot(&value))
    } else {
        None
    };

    if is_url(&source) && !source_prefix_is_url {
        return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
    }
    let source = match if parser.settings().layout.legacy() {
        parse_wikidot_image_source(source, explicit_attachment_size)
    } else {
        parse_direct_image_source(source)
    } {
        Some(source) => source,
        None => return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments)),
    };

    if let Some((slot, attribute, suffix)) = generated_image {
        match attribute {
            GeneratedImageAttribute::Alt => {
                let _ = arguments.get("alt");
            }
            GeneratedImageAttribute::Link => {
                let _ = arguments.get("link");
            }
        }
        return success_elements(Element::Delayed(DelayedElement::tag_image(
            source,
            link,
            target,
            alignment,
            arguments.to_attribute_map(parser.settings()),
            attribute,
            suffix,
            slot.id,
        )));
    }

    let element = Element::Image {
        source,
        link,
        alignment,
        attributes: arguments.to_attribute_map(parser.settings()),
        target,
    };

    success_elements(element)
}

fn parse_wikidot_image_source<'t>(
    source: Cow<'t, str>,
    explicit_attachment_size: Option<ImageSize>,
) -> Option<ImageSource<'t>> {
    if source.starts_with("data:") {
        let alt = source
            .rsplit('/')
            .next()
            .unwrap_or(source.as_ref())
            .to_owned();
        return Some(ImageSource::ImplicitAttachment {
            file: source,
            page: None,
            alt: Cow::Owned(alt),
            size: ImageSize::Medium,
        });
    }
    let quoted_inner = match source.as_bytes().first() {
        Some(b'\'') => source
            .strip_prefix('\'')
            .and_then(|inner| inner.strip_suffix('\'')),
        Some(b'"') => source
            .strip_prefix('"')
            .and_then(|inner| inner.strip_suffix('"')),
        _ => None,
    };
    if let Some(inner) = quoted_inner {
        if is_url(inner) {
            return Some(ImageSource::Direct(FileSource::Url(source)));
        }
        let alt = source
            .rsplit('/')
            .next()
            .unwrap_or(source.as_ref())
            .to_owned();
        return Some(ImageSource::ImplicitAttachment {
            file: source,
            page: None,
            alt: Cow::Owned(alt),
            size: ImageSize::Medium,
        });
    }
    if source.starts_with("mailto:") || source.starts_with("dns:") {
        let alt = source
            .rsplit('/')
            .next()
            .unwrap_or(source.as_ref())
            .to_owned();
        return Some(ImageSource::ImplicitAttachment {
            file: source,
            page: None,
            alt: Cow::Owned(alt),
            size: ImageSize::Medium,
        });
    }
    if source.contains("&#") {
        return Some(ImageSource::Direct(match source {
            Cow::Borrowed(source) => match source.rsplit_once('/') {
                Some((page, file)) => FileSource::File2 {
                    page: Cow::Borrowed(page),
                    file: Cow::Borrowed(file),
                },
                None => FileSource::File1 {
                    file: Cow::Borrowed(source),
                },
            },
            Cow::Owned(source) => match source.rsplit_once('/') {
                Some((page, file)) => FileSource::File2 {
                    page: Cow::Owned(page.to_owned()),
                    file: Cow::Owned(file.to_owned()),
                },
                None => FileSource::File1 {
                    file: Cow::Owned(source),
                },
            },
        }));
    }
    if source
        .chars()
        .next()
        .is_some_and(|ch| !ch.is_ascii() && ch.is_whitespace())
    {
        let trimmed =
            source.trim_start_matches(|ch: char| !ch.is_ascii() && ch.is_whitespace());
        if is_url(trimmed) {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            let mut encoded = String::with_capacity(source.len() + 8);
            for byte in source.bytes() {
                if !byte.is_ascii() {
                    encoded.push('%');
                    encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                    encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
                } else {
                    encoded.push(char::from(byte));
                }
            }
            return Some(ImageSource::Direct(FileSource::Url(Cow::Owned(encoded))));
        }
    }
    // Wikidot treats an assignment-shaped head token containing a literal URL
    // separator as a direct source. The token itself remains inert because its
    // scheme is not at the start of the rendered src value.
    if source.contains("://") {
        return Some(ImageSource::Direct(FileSource::Url(source)));
    }

    match source {
        Cow::Borrowed(source) => {
            let parsed = FileSource::parse_wikidot(source)?;
            Some(classify_wikidot_image_source(
                parsed,
                Cow::Borrowed(source),
                explicit_attachment_size,
            ))
        }
        Cow::Owned(source) => {
            let parsed = FileSource::parse_wikidot(&source)?.to_owned();
            Some(classify_wikidot_image_source(
                parsed,
                Cow::Owned(source),
                explicit_attachment_size,
            ))
        }
    }
}

fn classify_wikidot_image_source<'t>(
    source: FileSource<'t>,
    alt: Cow<'t, str>,
    explicit_attachment_size: Option<ImageSize>,
) -> ImageSource<'t> {
    match source {
        FileSource::File1 { file } if !file.starts_with('/') => {
            let size = explicit_attachment_size
                .or_else(|| default_wikidot_attachment_size(&file));
            match size {
                Some(size) => ImageSource::ImplicitAttachment {
                    file,
                    page: None,
                    alt,
                    size,
                },
                None => ImageSource::Direct(FileSource::File1 { file }),
            }
        }
        // A page-qualified attachment follows the same display rule as a
        // current-page one; Wikidot only distinguishes the owning page.
        // The `alt` for this arm is the filename alone, not the whole
        // page-qualified source the caller passes in.
        FileSource::File2 { page, file } => {
            let size = explicit_attachment_size
                .or_else(|| default_wikidot_attachment_size(&file));
            match size {
                Some(size) => ImageSource::ImplicitAttachment {
                    alt: Cow::Owned(file.as_ref().to_owned()),
                    file,
                    page: Some(page),
                    size,
                },
                None => ImageSource::Direct(FileSource::File2 { page, file }),
            }
        }
        source => ImageSource::Direct(source),
    }
}

/// Wikidot shows a current-page attachment without an explicit size directly
/// only when its name ends in `.png`, `.jpg`, or `.gif` (ASCII
/// case-insensitive). Every other name renders the medium resize linked to the
/// original attachment. This holds for previews and saved pages alike, whether
/// or not the attachment exists.
fn default_wikidot_attachment_size(file: &str) -> Option<ImageSize> {
    const ORIGINAL_DISPLAY_EXTENSIONS: [&str; 3] = ["png", "jpg", "gif"];

    let displays_original = file.rsplit_once('.').is_some_and(|(_, extension)| {
        ORIGINAL_DISPLAY_EXTENSIONS
            .iter()
            .any(|original| extension.eq_ignore_ascii_case(original))
    });
    (!displays_original).then_some(ImageSize::Medium)
}

fn parse_direct_image_source<'t>(source: Cow<'t, str>) -> Option<ImageSource<'t>> {
    match source {
        Cow::Borrowed(source) => FileSource::parse(source).map(ImageSource::Direct),
        Cow::Owned(source) => FileSource::parse(&source)
            .map(|source| source.to_owned())
            .map(ImageSource::Direct),
    }
}

/// Splits a leading `*` off an image `link` argument, returning the
/// remaining location text and the anchor target it implies.
///
/// In Wikidot syntax, `link="*https://example.com/"` makes the image link
/// open in a new tab; the `*` is not part of the location.
fn split_image_link_target(value: Cow<'_, str>) -> (Cow<'_, str>, Option<AnchorTarget>) {
    match value.strip_prefix('*') {
        Some(rest) => (
            Cow::Owned(rest.trim_start().to_owned()),
            Some(AnchorTarget::NewTab),
        ),
        None => {
            // Leading whitespace in the argument is not part of the
            // location either (e.g. `link=" https://example.com/"`).
            let trimmed = value.trim_start();
            if trimmed.len() == value.len() {
                (value, None)
            } else {
                (Cow::Owned(trimmed.to_owned()), None)
            }
        }
    }
}

fn parse_wikidot_image_link_target<'t>(target: Cow<'t, str>) -> Option<LinkLocation<'t>> {
    if target.starts_with("http://") || target.starts_with("https://") {
        let authority = target
            .split_once("://")
            .expect("qualified HTTP URL has a separator")
            .1
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default();
        let host = authority.rsplit('@').next().unwrap_or_default();
        let host = host
            .strip_prefix('[')
            .and_then(|host| host.split_once(']').map(|(host, _)| host))
            .unwrap_or_else(|| host.split(':').next().unwrap_or_default());
        if host.is_empty() {
            return None;
        }
        return Some(LinkLocation::Url(target));
    }

    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let anchor = target.starts_with('#');
    let target = if anchor {
        target.as_ref()
    } else {
        target.trim_start_matches('/')
    };
    let mut encoded = String::with_capacity(target.len() + usize::from(!anchor));
    if !anchor {
        encoded.push('/');
    }
    for byte in target.bytes() {
        // Browser special-scheme parsing treats an authored backslash after
        // the leading slash like an authority separator. Percent-encode it so
        // a legacy internal image-link target cannot navigate off-origin.
        if matches!(byte, b' ' | b'\\') || !byte.is_ascii() {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        } else {
            encoded.push(char::from(byte));
        }
    }

    Some(LinkLocation::Url(Cow::Owned(encoded)))
}

fn delayed_image_attribute<'a>(
    source: &'a str,
    slot: &crate::delayed::GeneratedInput,
) -> Option<(&'a str, &'a str)> {
    let prefix = &source[..slot.source_range.start];
    let equals = prefix.rfind('=')?;
    if !prefix[equals + 1..]
        .chars()
        .all(|character| matches!(character, '"' | '\''))
    {
        return None;
    }
    let key_start = prefix[..equals]
        .rfind(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        })
        .map_or(0, |position| position + 1);
    let key = &prefix[key_start..equals];
    let quote = *source
        .as_bytes()
        .get(slot.source_range.start.checked_sub(1)?)?;
    if !matches!(quote, b'"' | b'\'') {
        return None;
    }
    let suffix_end = source[slot.source_range.end..]
        .bytes()
        .position(|byte| byte == quote)?;
    let suffix = &source[slot.source_range.end..slot.source_range.end + suffix_end];
    (!key.is_empty()).then_some((key, suffix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::settings::{WikitextMode, WikitextSettings};
    use std::time::{Duration, Instant};

    fn render_image(
        source: &str,
        layout: Layout,
    ) -> (String, Vec<crate::parsing::ParseError>) {
        render_image_with_page(source, layout, PageInfo::dummy())
    }

    fn render_image_with_page(
        source: &str,
        layout: Layout,
        page_info: PageInfo,
    ) -> (String, Vec<crate::parsing::ParseError>) {
        let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
        let mut source = source.to_owned();
        crate::preprocess_for_layout(&mut source, layout);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        (html, errors)
    }

    fn render_wikidot_preview(source: &str) -> (String, Vec<crate::parsing::ParseError>) {
        let mut page_info = PageInfo::dummy();
        page_info.site = cow!("scp-wiki");
        page_info.page = cow!("");
        page_info.category = None;
        render_image_with_page(source, Layout::Wikidot, page_info)
    }

    fn escape_html_attribute(value: &str) -> String {
        let mut escaped = String::with_capacity(value.len());
        for character in value.chars() {
            match character {
                '&' => escaped.push_str("&amp;"),
                '<' => escaped.push_str("&lt;"),
                '>' => escaped.push_str("&gt;"),
                '\'' => escaped.push_str("&#39;"),
                '"' => escaped.push_str("&quot;"),
                _ => escaped.push(character),
            }
        }
        escaped
    }

    fn escape_html_text(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    fn wrap_v7_image(name: &str, image: String) -> String {
        let class = match name {
            "image" => return image,
            "=image" => "image-container aligncenter",
            "<image" => "image-container alignleft",
            ">image" => "image-container alignright",
            "f<image" => "image-container floatleft",
            "f>image" => "image-container floatright",
            _ => panic!("unexpected image spelling {name:?}"),
        };
        format!(r#"<div class="{class}">{image}</div>"#)
    }

    fn expected_v7_attachment(name: &str, file: &str, alt: &str, class: &str) -> String {
        let file = escape_html_attribute(file);
        let alt = escape_html_attribute(alt);
        wrap_v7_image(
            name,
            format!(
                concat!(
                    r#"<a href="https://scp-wiki.wjfiles.com/local--files//{file}">"#,
                    r#"<img src="https://scp-wiki.wjfiles.com/local--resized-images//{file}/medium.jpg" class="{class}" alt="{alt}">"#,
                    "</a>",
                ),
                file = file,
                class = class,
                alt = alt,
            ),
        )
    }

    #[test]
    fn wikidot_v7_image_family_matches_all_84_evidence_rows() {
        let families = [
            ("image", "IMAGE"),
            ("=image", "=IMAGE"),
            ("<image", "<IMAGE"),
            (">image", ">IMAGE"),
            ("f<image", "F<IMAGE"),
            ("f>image", "F>IMAGE"),
        ];
        let mut row_count = 0;

        for (name, uppercase_name) in families {
            let literal_cases = [
                (format!("[[{name}]]"), format!("[[{name}]]")),
                (format!("[[{name}"), format!("[[{name}")),
                (
                    format!("[[{uppercase_name}]]"),
                    format!("[[{uppercase_name}]]"),
                ),
            ];
            for (source, literal) in literal_cases {
                row_count += 1;
                let (html, _) = render_wikidot_preview(&source);
                assert_eq!(
                    html,
                    format!("<p>{}</p>", escape_html_text(&literal)),
                    "{source:?}",
                );
            }

            let literal = format!("[[{name}]]");
            let boundary = format!("start-{literal}-middle\n\n{literal}");
            row_count += 1;
            let (html, _) = render_wikidot_preview(&boundary);
            let literal = escape_html_text(&literal);
            assert_eq!(
                html,
                format!("<p>start-{literal}-middle</p><p>{literal}</p>"),
                "{boundary:?}",
            );

            let attachment_cases = [
                (
                    "v7ws=\"alpha\tbeta\u{a0}gamma\"",
                    "v7ws=\"alpha",
                    "v7ws=\"alpha",
                    "image",
                ),
                (
                    "v7ser=\"serialized body\"",
                    "v7ser=\"serialized",
                    "v7ser=\"serialized",
                    "image",
                ),
                (
                    "v7text=\"visible text\"",
                    "v7text=\"visible",
                    "v7text=\"visible",
                    "image",
                ),
                (
                    "class=\"one\" class=\"two\"",
                    "class=\"one\"",
                    "class=\"one\"",
                    "two",
                ),
                ("class=\"\"", "class=\"\"", "class=\"\"", "image"),
                (
                    "v7UnknownArgument=\"x\"",
                    "v7UnknownArgument=\"x\"",
                    "v7UnknownArgument=\"x\"",
                    "image",
                ),
                (
                    "class='single quoted' data-v7=unquoted",
                    "class='single",
                    "class='single",
                    "image",
                ),
                (
                    "srcset=\"javascript:alert(1) 1x\"",
                    "srcset=\"javascript:alert(1)",
                    "srcset=\"javascript:alert(1)",
                    "image",
                ),
                (
                    r#"srcset="https:\\example.test\path""#,
                    r#"srcset="https://example.test/path""#,
                    r#"srcset="https:\\example.test\path""#,
                    "image",
                ),
            ];
            for (tail, file, alt, class) in attachment_cases {
                row_count += 1;
                let source = format!("[[{name} {tail}]]");
                let (html, errors) = render_wikidot_preview(&source);
                assert!(errors.is_empty(), "{source:?}: {errors:#?}");
                assert_eq!(
                    html,
                    expected_v7_attachment(name, file, alt, class),
                    "{source:?}",
                );
                assert!(!html.contains(r#"href="javascript:"#), "{source:?}: {html}");
                assert!(!html.contains(r#"src="javascript:"#), "{source:?}: {html}");
            }

            let percent_encoded = format!(
                r#"[[{name} srcset="https://example.test/%6a%61vascript%3aalert(1)"]]"#
            );
            row_count += 1;
            let (html, errors) = render_wikidot_preview(&percent_encoded);
            assert!(errors.is_empty(), "{percent_encoded:?}: {errors:#?}");
            assert_eq!(
                html,
                wrap_v7_image(
                    name,
                    r##"<img src="#invalid-url" class="image" alt>"##.to_owned(),
                ),
                "{percent_encoded:?}",
            );
            assert!(!html.contains("<a "), "{percent_encoded:?}: {html}");
            assert!(!html.contains("javascript:"), "{percent_encoded:?}: {html}");
        }
        assert_eq!(row_count, 84);
    }

    #[test]
    fn wikidot_double_quoted_image_links_preserve_legacy_internal_targets() {
        for (target, expected_href) in [
            ("SCP-002", "/SCP-002"),
            ("scp-002", "/scp-002"),
            ("ScP-002", "/ScP-002"),
            ("Component:Image-Block", "/Component:Image-Block"),
            ("component:image-block", "/component:image-block"),
            ("Foo Bar", "/Foo%20Bar"),
            ("foo_bar", "/foo_bar"),
            ("日本語", "/%E6%97%A5%E6%9C%AC%E8%AA%9E"),
            ("SCP-002/noredirect/true", "/SCP-002/noredirect/true"),
            ("/SCP-002", "/SCP-002"),
            ("#Section", "#Section"),
            (".", "/."),
            ("..", "/.."),
            ("//Example.COM/Path", "/Example.COM/Path"),
            ("mailto:User@Example.COM", "/mailto:User@Example.COM"),
            ("", "/"),
        ] {
            let source =
                format!(r#"[[image https://example.com/x.png link="{target}"]]"#,);
            let (html, errors) = render_image(&source, Layout::Wikidot);

            assert!(errors.is_empty(), "{target:?}: {errors:#?}");
            assert_eq!(
                html,
                format!(
                    r#"<a href="{expected_href}"><img src="https://example.com/x.png" class="image" alt="x.png"></a>"#,
                ),
                "{target:?}",
            );
        }
    }

    #[test]
    fn wikidot_qualified_https_image_link_accepts_uppercase_host_labels() {
        let source = concat!(
            "BEGIN|[[image https://example.com/x.png ",
            r#"link="https://Example.COM/Path?Q=X#Frag"]]|END"#,
        );
        let (html, errors) = render_image(source, Layout::Wikidot);

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(
            html.contains(
                r#"<a href="https://Example.COM/Path?Q=X#Frag"><img src="https://example.com/x.png""#,
            ),
            "{html}",
        );
    }

    #[test]
    fn wikidot_qualified_https_image_links_accept_host_case_variants() {
        for target in [
            "http://example.com/Path?Q=X#Frag",
            "https://example.com/Path?Q=X#Frag",
            "https://Example.com/path",
            "http://example.COM/path",
            "https://example.COM/path",
            "https://EXAMPLE.COM/path",
        ] {
            let source =
                format!(r#"[[image https://example.com/x.png link="{target}"]]"#);
            let (html, errors) = render_image(&source, Layout::Wikidot);

            assert!(errors.is_empty(), "{target}: {errors:#?}");
            assert!(
                html.contains(&format!(r#"<a href="{target}"><img "#)),
                "{target}: {html}",
            );
            assert!(!html.contains("[[image "), "{target}: {html}");
        }
    }

    #[test]
    fn wikidot_image_link_with_leading_whitespace_stays_literal() {
        for target in [" https://example.com/", "* https://example.com/"] {
            let (html, _) =
                render_wikidot_preview(&format!("[[image foo.gif link=\"{target}\"]]"));
            assert!(html.contains("[[image foo.gif"), "{html}");
            assert!(!html.contains("<img"), "{html}");
        }
        let (html, _) =
            render_wikidot_preview("[[image foo.gif link=\"*https://example.com/\"]]");
        assert!(html.contains("target=\"_blank\""), "{html}");
    }

    #[test]
    fn wikidot_single_quoted_and_bare_image_links_remain_inert() {
        for target in [
            "SCP-002",
            "scp-002",
            "ScP-002",
            "Component:Image-Block",
            "component:image-block",
            "Foo Bar",
            "foo_bar",
            "日本語",
            "/SCP-002",
            "SCP-002/noredirect/true",
            "#Section",
            ".",
            "..",
            "https://Example.COM/Path?Q=X#Frag",
            "//Example.COM/Path",
            "mailto:User@Example.COM",
            "",
        ] {
            for argument in [format!("link='{target}'"), format!("link={target}")] {
                let source = format!("[[image https://example.com/x.png {argument}]]");
                let (html, errors) = render_image(&source, Layout::Wikidot);

                assert!(errors.is_empty(), "{argument}: {errors:#?}");
                assert_eq!(
                    html,
                    r#"<img src="https://example.com/x.png" class="image" alt="x.png">"#,
                    "{argument}",
                );
            }
        }
    }

    #[test]
    fn malformed_qualified_image_link_target_stays_literal_and_inert() {
        let source = concat!(
            "[[image https://example.com/x.png ",
            r#"link="https://"]]tail"#,
        );
        let (html, errors) = render_image(source, Layout::Wikidot);

        assert!(!errors.is_empty());
        assert!(!html.contains("<img"), "{html}");
        assert!(!html.contains("<a href=\"https://\"><img"));
        assert!(html.contains("[[image "), "{html}");
        assert!(html.contains("]]tail"), "{html}");
    }

    #[test]
    fn repeated_malformed_qualified_image_links_stay_bounded_and_inert() {
        const CANDIDATE_COUNT: usize = 128;
        let candidate = concat!(
            "[[image https://example.com/x.png ",
            r#"link="https://"]]"#,
            "\n",
        );
        let source = candidate.repeat(CANDIDATE_COUNT);
        let started = Instant::now();
        let (html, errors) = render_image(&source, Layout::Wikidot);

        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(!errors.is_empty());
        assert_eq!(html.matches("[[image ").count(), CANDIDATE_COUNT);
        assert_eq!(html.matches("<img").count(), 0);
    }

    #[test]
    fn wikijump_image_link_behavior_is_unchanged() {
        let source = concat!(
            "[[image https://example.com/x.png ",
            r#"link="https://Example.COM/Path?Q=X#Frag"]]"#,
        );
        let (html, errors) = render_image(source, Layout::Wikijump);

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains(r#"href="https://Example.COM/Path?Q=X#Frag""#));
        assert!(html.contains("<img"));
    }

    #[test]
    fn wikijump_relative_image_behavior_is_unchanged() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tokenization = crate::tokenize("[[=image photo.png]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:#?}");
        let [Element::Image { source, .. }] = tree.elements.as_slice() else {
            panic!("expected direct image element, got {:?}", tree.elements);
        };
        assert_eq!(
            source,
            &ImageSource::Direct(FileSource::File1 {
                file: cow!("photo.png"),
            }),
        );

        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;
        assert_eq!(
            html,
            concat!(
                r#"<div class="wj-image-container wj-align-center">"#,
                r#"<img class="wj-image" src="https://sandbox.wjfiles.com/local--files/some-page/photo.png">"#,
                "</div>",
            ),
        );
        assert!(!html.contains("local--resized-images"));
        assert!(!html.contains("<a "));
    }

    #[test]
    fn wikidot_image_accepts_multi_segment_local_source() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[image a/b/c/d.png]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        let [Element::Image { source, .. }] = tree.elements.as_slice() else {
            panic!("expected direct image element, got {:?}", tree.elements);
        };
        assert_eq!(
            source,
            &ImageSource::Direct(FileSource::File2 {
                page: cow!("a/b/c"),
                file: cow!("d.png"),
            }),
        );
    }

    #[test]
    fn wikidot_root_file_source_has_one_path_separator() {
        let mut page_info = PageInfo::dummy();
        page_info.site = cow!("sandbox");
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[image /my-picture.png]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            "<img src=\"https://sandbox.wjfiles.com/local--files/my-picture.png\" class=\"image\" alt=\"my-picture.png\">",
        );
    }

    #[test]
    fn wikidot_unsaved_preview_links_relative_images_to_medium_resizes() {
        let mut page_info = PageInfo::dummy();
        page_info.site = cow!("sandbox-for-codex");
        page_info.page = cow!("");
        page_info.category = None;
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[image fog-green.svg]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            concat!(
                "<a href=\"https://sandbox-for-codex.wjfiles.com/local--files//fog-green.svg\">",
                "<img src=\"https://sandbox-for-codex.wjfiles.com/local--resized-images//fog-green.svg/medium.jpg\" class=\"image\" alt=\"fog-green.svg\">",
                "</a>",
            ),
        );
    }

    #[test]
    fn wikidot_local_image_size_variants_match_live_resize_boundary() {
        // Anonymous PagePreview evidence captured 2026-09-13 proves that a
        // quoted local-image size selects Wikidot's named resize while the
        // original attachment remains the implicit link target.
        let mut page_info = PageInfo::dummy();
        page_info.site = cow!("sandbox-for-codex");
        page_info.page = cow!("");
        page_info.category = None;
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for (size, suffix) in [
            ("square", "square.jpg"),
            ("thumbnail", "thumbnail.jpg"),
            ("small", "small.jpg"),
            ("medium", "medium.jpg"),
        ] {
            for name in ["f=image", "=image"] {
                let source = format!(r#"[[{name} photo.png size="{size}"]]"#);
                let tokenization = crate::tokenize(&source);
                let (tree, errors) =
                    crate::parse(&tokenization, &page_info, &settings).into();
                let html = crate::render::html::HtmlRender
                    .render(&tree, &page_info, &settings)
                    .body;

                assert!(errors.is_empty(), "{source}: {errors:#?}");
                assert!(
                    html.contains(&format!("/local--resized-images//photo.png/{suffix}")),
                    "{source}: {html}",
                );
                assert!(
                    html.contains("href=\"https://sandbox-for-codex.wjfiles.com/local--files//photo.png\""),
                    "{source}: {html}",
                );
            }
        }
    }

    #[test]
    fn wikidot_invalid_empty_and_unquoted_local_image_size_stay_unresized() {
        // The matching live controls render the current-page file directly:
        // no resized URL and no implicit original-file link are synthesized.
        let mut page_info = PageInfo::dummy();
        page_info.site = cow!("sandbox-for-codex");
        page_info.page = cow!("");
        page_info.category = None;
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for size_argument in [r#"size="bogus""#, r#"size="""#, "size=small"] {
            for name in ["f=image", "=image"] {
                let source = format!("[[{name} photo.png {size_argument}]]");
                let tokenization = crate::tokenize(&source);
                let (tree, errors) =
                    crate::parse(&tokenization, &page_info, &settings).into();
                let html = crate::render::html::HtmlRender
                    .render(&tree, &page_info, &settings)
                    .body;

                assert!(errors.is_empty(), "{source}: {errors:#?}");
                assert!(!html.contains("local--resized-images"), "{source}: {html}");
                assert!(!html.contains("<a "), "{source}: {html}");
                assert!(
                    html.contains("src=\"https://sandbox-for-codex.wjfiles.com/local--files//photo.png\""),
                    "{source}: {html}",
                );
            }
        }
    }

    #[test]
    fn wikidot_size_lookalikes_outside_bare_pairs_keep_file_type_default() {
        // Anonymous Wikidot PagePreview (2026-10-11): `size=` text inside a
        // quoted value or a comment is not a size argument, so the file-type
        // default applies. For `.png` that is the original attachment.
        let mut page_info = PageInfo::dummy();
        page_info.site = cow!("sandbox-for-codex");
        page_info.page = cow!("");
        page_info.category = None;
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for source in [
            r#"[[f=image photo.png alt="a size=b"]]"#,
            "[[f=image photo.png [!-- size=small --]]]",
        ] {
            let tokenization = crate::tokenize(source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = crate::render::html::HtmlRender
                .render(&tree, &page_info, &settings)
                .body;

            assert!(errors.is_empty(), "{source}: {errors:#?}");
            assert!(!html.contains("local--resized-images"), "{source}: {html}");
            assert!(
                html.contains(
                    "src=\"https://sandbox-for-codex.wjfiles.com/local--files//photo.png\""
                ),
                "{source}: {html}",
            );
        }
    }

    #[test]
    fn wikidot_20260913_image_argument_boundaries_match_live() {
        for (source, expected_fragment, forbidden_fragment, expect_errors) in [
            (
                r#"[[f=image https://example.com/a.png width="200px"]]"#,
                r#"width="200px""#,
                "__never__",
                false,
            ),
            (
                r#"[[f=image https://example.com/a.png width="200"]]"#,
                r#"width="200""#,
                "__never__",
                false,
            ),
            (
                r#"[[f=image https://example.com/a.png width="50%"]]"#,
                r#"width="50%""#,
                "__never__",
                false,
            ),
            (
                r#"[[f=image https://example.com/a.png width=""]]"#,
                "<img",
                "width=\"200",
                false,
            ),
            (
                "[[f=image https://example.com/a.png width=200px]]",
                "<img",
                " width=",
                false,
            ),
            (
                "[[f=image https://example.com/a.png width=200]]",
                "<img",
                " width=",
                false,
            ),
            (
                "[[f<image https://example.com/a.png]]",
                "image-container floatleft",
                "__never__",
                false,
            ),
            (
                "[[f>image https://example.com/a.png]]",
                "image-container floatright",
                "__never__",
                false,
            ),
            (
                "[[ff<image https://example.com/a.png]]",
                "[[ff&lt;image ",
                "<img",
                true,
            ),
            (
                "[[f >image https://example.com/a.png]]",
                "[[f &gt;image ",
                "<img",
                true,
            ),
            (
                "[[= image https://example.com/a.png]]",
                "[[= image ",
                "<img",
                true,
            ),
            (
                "[[== image https://example.com/a.png]]",
                "[[== image ",
                "<img",
                true,
            ),
        ] {
            let (html, errors) = render_image(source, Layout::Wikidot);
            assert_eq!(!errors.is_empty(), expect_errors, "{source}: {errors:#?}");
            assert!(html.contains(expected_fragment), "{source}: {html}");
            assert!(!html.contains(forbidden_fragment), "{source}: {html}");
        }
    }

    #[test]
    fn wikidot_implicit_attachment_preserves_typed_current_page_provenance() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[=image fog-green.svg class=\"hero\"]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:#?}");
        let [Element::Image { source, .. }] = tree.elements.as_slice() else {
            panic!("expected direct image element, got {:?}", tree.elements);
        };
        assert_eq!(
            source,
            &ImageSource::ImplicitAttachment {
                file: cow!("fog-green.svg"),
                page: None,
                alt: cow!("fog-green.svg"),
                size: ImageSize::Medium,
            },
        );

        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;
        assert_eq!(
            html,
            concat!(
                "<div class=\"image-container aligncenter\">",
                "<a href=\"https://sandbox.wjfiles.com/local--files/some-page/fog-green.svg\">",
                "<img src=\"https://sandbox.wjfiles.com/local--resized-images/some-page/fog-green.svg/medium.jpg\" class=\"hero\" alt=\"fog-green.svg\">",
                "</a></div>",
            ),
        );
    }

    #[test]
    fn wikidot_page_qualified_attachment_uses_owning_page_for_resize() {
        // Issue #734: a page-qualified attachment follows the same file-type
        // default as a current-page one. Anonymous Wikidot PagePreview
        // (2026-10-11) renders `scp-9506/x.jpeg` as the linked medium resize
        // on the owning page while `scp-9506/x.png` shows the original.
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[image other-page/photo.jpeg]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:#?}");
        let [Element::Image { source, .. }] = tree.elements.as_slice() else {
            panic!("expected direct image element, got {:?}", tree.elements);
        };
        assert_eq!(
            source,
            &ImageSource::ImplicitAttachment {
                file: cow!("photo.jpeg"),
                page: Some(cow!("other-page")),
                alt: cow!("photo.jpeg"),
                size: ImageSize::Medium,
            },
        );

        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;
        assert_eq!(
            html,
            concat!(
                "<a href=\"https://sandbox.wjfiles.com/local--files/other-page/photo.jpeg\">",
                "<img src=\"https://sandbox.wjfiles.com/local--resized-images/other-page/photo.jpeg/medium.jpg\" class=\"image\" alt=\"photo.jpeg\">",
                "</a>",
            ),
        );

        // An original-display extension stays a plain, unlinked original on
        // the owning page, exactly as before.
        let (html, errors) =
            render_image("[[image other-page/photo.png]]", Layout::Wikidot);
        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            r#"<img src="https://sandbox.wjfiles.com/local--files/other-page/photo.png" class="image" alt="photo.png">"#,
        );
    }

    #[test]
    fn wikidot_non_current_page_image_sources_remain_static_controls() {
        for (source, expected) in [
            (
                "[[image other-page/photo.png]]",
                r#"<img src="https://sandbox.wjfiles.com/local--files/other-page/photo.png" class="image" alt="photo.png">"#,
            ),
            (
                "[[image /photo.png]]",
                r#"<img src="https://sandbox.wjfiles.com/local--files/photo.png" class="image" alt="photo.png">"#,
            ),
            (
                "[[image /local--files/source-page/photo.png]]",
                r#"<img src="/local--files/source-page/photo.png" class="image" alt="photo.png">"#,
            ),
            (
                "[[image https://example.com/photo.png]]",
                r#"<img src="https://example.com/photo.png" class="image" alt="photo.png">"#,
            ),
        ] {
            let (html, errors) = render_image(source, Layout::Wikidot);
            assert!(errors.is_empty(), "{source:?}: {errors:#?}");
            assert_eq!(html, expected, "{source:?}");
            assert!(
                !html.contains("local--resized-images"),
                "{source:?}: {html}"
            );
            assert!(!html.contains("<a "), "{source:?}: {html}");
        }
    }

    #[test]
    fn wikidot_explicit_image_link_overrides_implicit_attachment_link() {
        // Anonymous Wikidot PagePreview (2026-10-11): an explicit link wraps
        // the file-type default display. `.jpeg` keeps the medium resize but
        // loses the implicit original-file link; `.png` shows the original.
        let (html, errors) = render_image(
            r#"[[image photo.jpeg link="Target Page"]]"#,
            Layout::Wikidot,
        );

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            concat!(
                r#"<a href="/Target%20Page">"#,
                r#"<img src="https://sandbox.wjfiles.com/local--resized-images/some-page/photo.jpeg/medium.jpg" class="image" alt="photo.jpeg">"#,
                "</a>",
            ),
        );
        assert_eq!(html.matches("<a ").count(), 1);
        assert!(!html.contains("local--files/some-page/photo.jpeg"));

        let (html, errors) =
            render_image(r#"[[image photo.png link="Target Page"]]"#, Layout::Wikidot);

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            concat!(
                r#"<a href="/Target%20Page">"#,
                r#"<img src="https://sandbox.wjfiles.com/local--files/some-page/photo.png" class="image" alt="photo.png">"#,
                "</a>",
            ),
        );
    }

    #[test]
    fn wikidot_implicit_attachment_obeys_disabled_local_path_boundary() {
        let page_info = PageInfo::dummy();
        let mut settings =
            WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        settings.allow_local_paths = false;
        let tokenization = crate::tokenize("[[image private.png]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            r#"<div class="wj-error-block">No images in this context</div>"#,
        );
        assert!(!html.contains("private.png"));
    }

    #[test]
    fn malformed_implicit_attachment_candidates_stay_literal_and_inert() {
        for source in ["[[image]]", "[[=image]]", "[[<image page/]]"] {
            let (html, errors) = render_image(source, Layout::Wikidot);
            assert!(!errors.is_empty(), "{source:?}");
            assert!(!html.contains("<img"), "{source:?}: {html}");
            assert!(!html.contains("<a "), "{source:?}: {html}");
            assert!(html.contains("[["), "{source:?}: {html}");
        }

        let source = r#"[[f>image "../private/image.png"]]"#;
        let (html, errors) = render_image(source, Layout::Wikidot);
        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("local--resized-images"), "{html}");
        assert!(html.contains("&quot;../private/image.png&quot;"), "{html}");
    }

    #[test]
    fn repeated_implicit_attachment_candidates_stay_bounded_and_inert() {
        const CANDIDATE_COUNT: usize = 512;
        let source =
            "[[=image srcset=\"javascript:alert(1) 1x\"]]\n".repeat(CANDIDATE_COUNT);
        let started = Instant::now();
        let (html, errors) = render_wikidot_preview(&source);

        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html.matches("local--resized-images").count(),
            CANDIDATE_COUNT
        );
        assert_eq!(html.matches("local--files").count(), CANDIDATE_COUNT);
        assert_eq!(html.matches("<img").count(), CANDIDATE_COUNT);
        assert!(!html.contains(r#"href="javascript:"#));
        assert!(!html.contains(r#"src="javascript:"#));
        assert!(!html.contains(" onerror="));
    }

    #[test]
    fn wikidot_quoted_absolute_image_sources_keep_their_quotes() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        for (source, expected) in [
            (
                r#"[[image "https://example.com/a.png"]]"#,
                r#"<img src="&quot;https://example.com/a.png&quot;" class="image" alt="a.png&quot;">"#,
            ),
            (
                "[[image 'https://example.com/a.png']]",
                r#"<img src="&#39;https://example.com/a.png&#39;" class="image" alt="a.png&#39;">"#,
            ),
        ] {
            let tokenization = crate::tokenize(source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = crate::render::html::HtmlRender
                .render(&tree, &page_info, &settings)
                .body;

            assert!(errors.is_empty(), "{source}: {errors:#?}");
            assert_eq!(html, expected, "{source}");
        }
    }

    #[test]
    fn wikidot_structural_quoted_local_paths_remain_typed_attachment_data() {
        let mut page_info = PageInfo::dummy();
        page_info.site = cow!("sandbox-for-codex");
        page_info.page = cow!("");
        page_info.category = None;
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = r#"[[image "/local--files/source-page/image.png"]]"#;
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(format!("{tree:?}").contains("Image {"), "{tree:#?}");
        assert!(html.contains("local--resized-images"), "{html}");
        assert!(
            html.contains("&quot;/local--files/source-page/image.png&quot;"),
            "{html}",
        );
    }

    #[test]
    fn image_block_preserves_canonical_wikidot_local_files_path() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(
            "[[image /local--files/source-page/assets/charts/image.png]]",
        );
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        let [Element::Image { source, .. }] = tree.elements.as_slice() else {
            panic!("expected direct image element, got {:?}", tree.elements);
        };

        assert_eq!(
            source,
            &ImageSource::Direct(FileSource::Url(cow!(
                "/local--files/source-page/assets/charts/image.png"
            ))),
        );
    }

    #[test]
    fn unaligned_image_suppresses_the_contiguous_paragraph_wrapper() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization =
            crate::tokenize("BASIC [[image /local--files/source-page/filename.png]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            "\n\nBASIC <img src=\"/local--files/source-page/filename.png\" class=\"image\" alt=\"filename.png\">",
        );
    }

    #[test]
    fn blank_line_after_unaligned_image_preserves_wikidot_separator() {
        let mut page_info = PageInfo::dummy();
        page_info.site = cow!("sandbox");
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("BASIC [[image filename.png]]\n\nLEFT");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            concat!(
                "\n\nBASIC <img src=\"https://sandbox.wjfiles.com/local--files/some-page/filename.png\" class=\"image\" alt=\"filename.png\">",
                " <p>LEFT</p>",
            ),
        );
    }

    #[test]
    fn image_breaks_a_contiguous_div_paragraph_like_wikidot() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = concat!(
            "[[div class=\"name\"]]\n",
            "NFSI\n",
            "[[image /local--files/scp-9506/NFSI.png]]\n",
            "[[span style=\"font-size:2rem\"]]National Fog Safety Initiative[[/span]]\n",
            "[[/div]]\n",
        );
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("<div class=\"name\">\n\nNFSI<br>"), "{html}");
        assert!(
            html.contains("NFSI.png\" class=\"image\" alt=\"NFSI.png\"><br>\n<span"),
            "{html}",
        );
        assert!(!html.contains("<div class=\"name\"><p>"), "{html}");
    }

    #[test]
    fn blank_line_keeps_text_paragraph_separate_from_naked_image() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = concat!(
            "[[div class=\"picture\"]]\n",
            "[[span class=\"heading2\"]]BREAKING[[/span]]\n",
            "\n",
            "[[image /local--files/scp-9506/fog.jpg]]\n",
            "[[/div]]\n",
        );
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            "<div class=\"picture\"><p><span class=\"heading2\">BREAKING</span></p><img src=\"/local--files/scp-9506/fog.jpg\" class=\"image\" alt=\"fog.jpg\"></div>",
        );
    }

    #[test]
    fn wikidot_float_center_image_uses_plain_image_container() {
        let mut page_info = PageInfo::dummy();
        page_info.site = cow!("sandbox-for-codex");
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("FLOAT CENTER [[f=image landscape.jpg]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            concat!(
                "<p>FLOAT CENTER</p>",
                "<div class=\"image-container\">",
                "<img src=\"https://sandbox-for-codex.wjfiles.com/local--files/some-page/landscape.jpg\" class=\"image\" alt=\"landscape.jpg\">",
                "</div>",
            ),
        );
    }
}
