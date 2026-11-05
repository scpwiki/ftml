/*
 * parsing/rule/impls/block/blocks/embed.rs
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
use crate::tree::{AttributeMap, Embed};
use std::borrow::Cow;

type EmbedBuilderFn = for<'p, 't> fn(
    &'p Parser<'_, 't>,
    &'p mut Arguments<'t>,
) -> Result<Embed<'t>, ParseError>;

pub const BLOCK_EMBED: BlockRule = BlockRule {
    name: "block-embed",
    accepts_names: &["embed"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn,
};

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing embed block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Embed doesn't allow star flag");
    assert!(!flag_score, "Embed doesn't allow star flag");
    assert_block_name(&BLOCK_EMBED, name);

    if parser.settings().layout.legacy() {
        return parse_wikidot_embed_body(
            parser,
            &BLOCK_EMBED,
            name,
            opener_start,
            in_head,
        );
    }

    let (name, mut arguments) = parser.get_head_name_map(&BLOCK_EMBED, in_head)?;
    let embed = build_embed(parser, &name, &mut arguments)?;

    ok!(Element::Embed(embed))
}

pub(crate) fn parse_wikidot_embed_body<'r, 't>(
    parser: &mut Parser<'r, 't>,
    rule: &BlockRule,
    name: &str,
    opener_start: usize,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    let source = parser.full_text().inner();
    let exact_opener = source.get(opener_start..).is_some_and(|rest| {
        rest.get(..2 + name.len() + 2).is_some_and(|opener| {
            opener.get(..2) == Some("[[")
                && opener
                    .get(2..2 + name.len())
                    .is_some_and(|word| word.eq_ignore_ascii_case(name))
                && opener.get(2 + name.len()..) == Some("]]")
        })
    });
    if !exact_opener {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    parser.get_head_map_with_body_start_wikidot(rule, in_head)?;

    let body = parser.get_body_text(rule)?;
    match wikidot_embed_body(&body) {
        WikidotEmbedBody::Iframe(embed) => ok!(true; Element::Embed(embed)),
        WikidotEmbedBody::Suppressed => ok!(false; Elements::None),
        WikidotEmbedBody::NoMatch => {
            let mut attributes = AttributeMap::new();
            attributes.insert("class", cow!("error-block"));
            let element = Element::Container(Container::new(
                ContainerType::Div,
                vec![text!("Sorry, no match for the embedded content.")],
                attributes,
            ));

            ok!(false; element)
        }
    }
}

enum WikidotEmbedBody {
    Iframe(Embed<'static>),
    Suppressed,
    NoMatch,
}

const WIKIDOT_EMBED_BOOLEAN_ATTRIBUTES: &[&str] =
    &["allowfullscreen", "async", "disabled", "itemscope"];

const HEX: &[u8; 16] = b"0123456789ABCDEF";

fn wikidot_embed_body(body: &str) -> WikidotEmbedBody {
    let text = body.trim();
    let bytes = text.as_bytes();

    if !text
        .get(..7)
        .is_some_and(|head| head.eq_ignore_ascii_case("<iframe"))
    {
        return WikidotEmbedBody::NoMatch;
    }

    if !bytes.get(7).is_some_and(u8::is_ascii_whitespace) {
        return WikidotEmbedBody::NoMatch;
    }

    let mut attributes: Vec<(String, Option<(char, String)>)> = Vec::new();
    let mut pos = 7;

    loop {
        while bytes.get(pos).is_some_and(u8::is_ascii_whitespace) {
            pos += 1;
        }
        match bytes.get(pos) {
            Some(b'>') => {
                pos += 1;
                break;
            }
            None => return WikidotEmbedBody::NoMatch,
            Some(_) => {}
        }

        let name_start = pos;
        while bytes
            .get(pos)
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        {
            pos += 1;
        }
        if pos == name_start {
            return WikidotEmbedBody::NoMatch;
        }
        let name = text[name_start..pos].to_ascii_lowercase();

        let value = if bytes.get(pos) == Some(&b'=') {
            pos += 1;
            let quote = match bytes.get(pos) {
                Some(&quote @ (b'"' | b'\'')) => quote,
                _ => return WikidotEmbedBody::NoMatch,
            };
            let value_start = pos + 1;
            pos = value_start;
            while let Some(&byte) = bytes.get(pos) {
                if byte == quote || matches!(byte, b'<' | b'>') {
                    break;
                }
                pos += 1;
            }
            if bytes.get(pos) != Some(&quote) {
                return WikidotEmbedBody::NoMatch;
            }
            let value = text[value_start..pos].trim().to_owned();
            pos += 1;
            Some((char::from(quote), value))
        } else {
            None
        };

        match bytes.get(pos) {
            Some(byte) if byte.is_ascii_whitespace() => {
                let mut lookahead = pos;
                while bytes.get(lookahead).is_some_and(u8::is_ascii_whitespace) {
                    lookahead += 1;
                }
                if bytes.get(lookahead) == Some(&b'>') {
                    return WikidotEmbedBody::NoMatch;
                }
            }
            Some(b'>') => {}
            _ => return WikidotEmbedBody::NoMatch,
        }

        match attributes
            .iter_mut()
            .find(|(existing, _)| *existing == name)
        {
            Some(existing) => existing.1 = value,
            None => attributes.push((name, value)),
        }
    }

    while bytes.get(pos).is_some_and(u8::is_ascii_whitespace) {
        pos += 1;
    }
    let Some(closer) = text.get(pos..) else {
        return WikidotEmbedBody::NoMatch;
    };
    if !closer
        .get(..9)
        .is_some_and(|head| head.eq_ignore_ascii_case("</iframe>"))
    {
        return WikidotEmbedBody::NoMatch;
    }
    if !closer[9..].bytes().all(|byte| byte.is_ascii_whitespace()) {
        return WikidotEmbedBody::NoMatch;
    }
    if attributes.is_empty() {
        return WikidotEmbedBody::NoMatch;
    }

    let source = attributes
        .iter()
        .find(|(name, _)| name == "src")
        .and_then(|(_, value)| value.as_ref())
        .map_or("", |(_, value)| value.as_str());
    let source = wikidot_embed_source(source);
    if is_wikidot_embed_suppressed(&source) {
        return WikidotEmbedBody::Suppressed;
    }

    let mut html = String::from("<iframe");
    for (name, value) in &attributes {
        html.push(' ');
        html.push_str(name);
        match value {
            Some((quote, value)) => {
                html.push('=');
                html.push(*quote);
                if name == "src" {
                    html.push_str(&source);
                } else {
                    html.push_str(value);
                }
                html.push(*quote);
            }
            None if WIKIDOT_EMBED_BOOLEAN_ATTRIBUTES.contains(&name.as_str()) => {
                html.push_str("=\"");
                html.push_str(name);
                html.push('"');
            }
            None => html.push_str("=\"\""),
        }
    }
    html.push_str("></iframe>");

    WikidotEmbedBody::Iframe(Embed::Iframe {
        url: Cow::Owned(source),
        html: Cow::Owned(html),
    })
}

fn wikidot_embed_source(source: &str) -> String {
    let mut encoded = String::with_capacity(source.len());
    for character in source.trim().chars() {
        if character.is_whitespace() {
            encoded.push_str("%20");
        } else if character.is_ascii() {
            encoded.push(character);
        } else {
            let mut buffer = [0; 4];
            for byte in character.encode_utf8(&mut buffer).bytes() {
                encoded.push('%');
                encoded.push(char::from(HEX[usize::from(byte >> 4)]));
                encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        }
    }
    encoded
}

fn is_wikidot_embed_suppressed(source: &str) -> bool {
    ["javascript:", "vbscript:"].iter().any(|scheme| {
        source
            .get(..scheme.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
    })
}

fn build_embed<'r, 't>(
    parser: &Parser<'r, 't>,
    name: &str,
    arguments: &mut Arguments<'t>,
) -> Result<Embed<'t>, ParseError>
where
    'r: 't,
{
    const EMBED_BUILDERS: &[(&str, EmbedBuilderFn)] =
        &[("youtube", build_youtube), ("vimeo", build_vimeo)];

    for &(embed_name, builder) in EMBED_BUILDERS {
        if embed_name.eq_ignore_ascii_case(name) {
            return builder(parser, arguments);
        }
    }

    Err(parser.make_err(ParseErrorKind::NoSuchEmbed))
}

fn build_youtube<'p, 't>(
    parser: &'p Parser<'_, 't>,
    arguments: &'p mut Arguments<'t>,
) -> Result<Embed<'t>, ParseError> {
    let video_id = arguments
        .get("video")
        .ok_or_else(|| parser.make_err(ParseErrorKind::BlockMissingArguments))?;

    Ok(Embed::Youtube { video_id })
}

fn build_vimeo<'p, 't>(
    parser: &'p Parser<'_, 't>,
    arguments: &'p mut Arguments<'t>,
) -> Result<Embed<'t>, ParseError> {
    let video_id = arguments
        .get("video")
        .ok_or_else(|| parser.make_err(ParseErrorKind::BlockMissingArguments))?;

    Ok(Embed::Vimeo { video_id })
}

#[test]
fn embed_builder_types() {
    let _: EmbedBuilderFn = build_youtube;
    let _: EmbedBuilderFn = build_vimeo;
}

#[test]
fn embed_rejects_unknown_provider() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let tokenization = crate::tokenize("[[embed unknown video=\"abc123\"]]");
    let (_tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

    assert!(
        errors
            .iter()
            .any(|error| error.kind() == ParseErrorKind::NoSuchEmbed),
        "unknown embed provider should report NoSuchEmbed: {errors:?}",
    );
}

#[test]
fn wikidot_embed_iframe_body_renders_iframe() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::settings::{WikitextMode, WikitextSettings};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize(concat!(
        "[[embed]]\n",
        "<iframe width=\"1208\" height=\"680\" ",
        "src=\"https://www.youtube.com/embed/dQw4w9WgXcQ\" ",
        "title=\"Rick Astley - Never Gonna Give You Up (Official Video) (4K Remaster)\" ",
        "frameborder=\"0\" ",
        "allow=\"accelerometer; autoplay; clipboard-write; encrypted-media; ",
        "gyroscope; picture-in-picture; web-share\" ",
        "referrerpolicy=\"strict-origin-when-cross-origin\" allowfullscreen></iframe>\n",
        "[[/embed]]",
    ));
    let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
    let html = crate::render::html::HtmlRender
        .render(&tree, &page_info, &settings)
        .body;

    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        html,
        concat!(
            "<p><iframe width=\"1208\" height=\"680\" ",
            "src=\"https://www.youtube.com/embed/dQw4w9WgXcQ\" ",
            "title=\"Rick Astley - Never Gonna Give You Up (Official Video) (4K Remaster)\" ",
            "frameborder=\"0\" ",
            "allow=\"accelerometer; autoplay; clipboard-write; encrypted-media; ",
            "gyroscope; picture-in-picture; web-share\" ",
            "referrerpolicy=\"strict-origin-when-cross-origin\" ",
            "allowfullscreen=\"allowfullscreen\"></iframe></p>",
        ),
    );
}

#[test]
fn wikidot_embed_unmatched_body_renders_error_block() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::settings::{WikitextMode, WikitextSettings};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize(concat!(
        "[[embed]]x[[/embed]]\n",
        "\n",
        "text\n",
        "[[embed]]x[[/embed]]\n",
        "\n",
        "x [[embed]]z[[/embed]] y",
    ));
    let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
    let html = crate::render::html::HtmlRender
        .render(&tree, &page_info, &settings)
        .body;

    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        html,
        concat!(
            "<div class=\"error-block\">Sorry, no match for the embedded content.</div>",
            "<p>text<br>\n</p>",
            "<div class=\"error-block\">Sorry, no match for the embedded content.</div>",
            "<p>x</p>",
            "<div class=\"error-block\">Sorry, no match for the embedded content.</div>",
            "\ny",
        ),
    );
}

#[test]
fn wikidot_embed_http_iframe_renders_iframe() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::settings::{WikitextMode, WikitextSettings};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize(
        r#"[[embed]]<iframe src="http://example.com/x"></iframe>[[/embed]]"#,
    );
    let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
    let html = crate::render::html::HtmlRender
        .render(&tree, &page_info, &settings)
        .body;

    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        html,
        r#"<p><iframe src="http://example.com/x"></iframe></p>"#,
    );
}

#[test]
fn wikidot_embed_error_block_keeps_the_preceding_line_break() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::settings::{WikitextMode, WikitextSettings};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize("text\n[[embed]]x[[/embed]]");
    let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
    let html = crate::render::html::HtmlRender
        .render(&tree, &page_info, &settings)
        .body;

    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        html,
        concat!(
            "<p>text<br>\n</p>",
            "<div class=\"error-block\">Sorry, no match for the embedded content.</div>",
        ),
    );
}

#[test]
fn wikidot_embed_error_block_leaves_same_line_text_unwrapped() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::settings::{WikitextMode, WikitextSettings};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize("x [[embed]]z[[/embed]] y");
    let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
    let html = crate::render::html::HtmlRender
        .render(&tree, &page_info, &settings)
        .body;

    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        html,
        concat!(
            "<p>x</p>",
            "<div class=\"error-block\">Sorry, no match for the embedded content.</div>",
            "\ny",
        ),
    );
}

#[test]
fn wikidot_embed_error_block_unwrapped_run_continues_over_lines() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::settings::{WikitextMode, WikitextSettings};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize("x [[embed]]z[[/embed]] y\nmore");
    let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
    let html = crate::render::html::HtmlRender
        .render(&tree, &page_info, &settings)
        .body;

    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        html,
        concat!(
            "<p>x</p>",
            "<div class=\"error-block\">Sorry, no match for the embedded content.</div>",
            "\ny<br>\nmore",
        ),
    );
}

#[test]
fn wikidot_embed_error_blocks_separate_bare_text_with_newlines() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::settings::{WikitextMode, WikitextSettings};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization =
        crate::tokenize("[[embed]]a[[/embed]] mid [[embed]]b[[/embed]] end");
    let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
    let html = crate::render::html::HtmlRender
        .render(&tree, &page_info, &settings)
        .body;

    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        html,
        concat!(
            "<div class=\"error-block\">Sorry, no match for the embedded content.</div>",
            "\nmid\n",
            "<div class=\"error-block\">Sorry, no match for the embedded content.</div>",
            "\nend",
        ),
    );
}

#[test]
fn wikidot_embed_iframe_keeps_the_preceding_line_break() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::settings::{WikitextMode, WikitextSettings};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize(
        "text\n[[embed]]<iframe src=\"https://example.com\"></iframe>[[/embed]]",
    );
    let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
    let html = crate::render::html::HtmlRender
        .render(&tree, &page_info, &settings)
        .body;

    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        html,
        "<p>text<br>\n<iframe src=\"https://example.com\"></iframe></p>",
    );
}

#[test]
fn wikidot_embed_error_block_inside_a_div_keeps_the_line_break() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::settings::{WikitextMode, WikitextSettings};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize("[[div]]\ntext\n[[embed]]x[[/embed]]\n[[/div]]");
    let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
    let html = crate::render::html::HtmlRender
        .render(&tree, &page_info, &settings)
        .body;

    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        html,
        concat!(
            "<div><p>text<br>\n</p>",
            "<div class=\"error-block\">Sorry, no match for the embedded content.</div>",
            "</div>",
        ),
    );
}
