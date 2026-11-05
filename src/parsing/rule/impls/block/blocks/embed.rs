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

use super::iframe::wikidot_iframe_url;
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
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing embed block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Embed doesn't allow star flag");
    assert!(!flag_score, "Embed doesn't allow star flag");
    assert_block_name(&BLOCK_EMBED, name);

    if parser.settings().layout.legacy() {
        return parse_wikidot_embed_body(parser, &BLOCK_EMBED, in_head);
    }

    let (name, mut arguments) = parser.get_head_name_map(&BLOCK_EMBED, in_head)?;
    let embed = build_embed(parser, &name, &mut arguments)?;

    ok!(Element::Embed(embed))
}

pub(crate) fn parse_wikidot_embed_body<'r, 't>(
    parser: &mut Parser<'r, 't>,
    rule: &BlockRule,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    let (arguments, _) = parser.get_head_map_with_body_start_wikidot(rule, in_head)?;
    if !arguments.is_empty() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let body = parser.get_body_text(rule)?;
    match wikidot_embed_iframe(&body) {
        Some(embed) => ok!(true; Element::Embed(embed)),
        None => {
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

fn wikidot_embed_iframe(body: &str) -> Option<Embed<'static>> {
    let text = body.trim();
    let bytes = text.as_bytes();
    if !text
        .get(..7)
        .is_some_and(|head| head.eq_ignore_ascii_case("<iframe"))
    {
        return None;
    }
    if !matches!(
        bytes.get(7),
        Some(b' ' | b'\t' | b'\r' | b'\n' | b'>' | b'/')
    ) {
        return None;
    }

    let mut parsed: Vec<(String, Cow<'_, str>)> = Vec::new();
    let mut pos = 7;
    let self_closing = loop {
        while bytes
            .get(pos)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            pos += 1;
        }
        match bytes.get(pos) {
            Some(b'>') => {
                pos += 1;
                break false;
            }
            Some(b'/') if bytes.get(pos + 1) == Some(&b'>') => {
                pos += 2;
                break true;
            }
            Some(_) => {
                let name_start = pos;
                while let Some(&byte) = bytes.get(pos) {
                    if byte.is_ascii_whitespace() || matches!(byte, b'=' | b'>' | b'/') {
                        break;
                    }
                    pos += 1;
                }
                if pos == name_start {
                    return None;
                }
                let name = text[name_start..pos].to_ascii_lowercase();

                while bytes
                    .get(pos)
                    .is_some_and(|byte| byte.is_ascii_whitespace())
                {
                    pos += 1;
                }
                let value = if bytes.get(pos) == Some(&b'=') {
                    pos += 1;
                    while bytes
                        .get(pos)
                        .is_some_and(|byte| byte.is_ascii_whitespace())
                    {
                        pos += 1;
                    }
                    match bytes.get(pos) {
                        Some(quote @ (b'"' | b'\'')) => {
                            let value_start = pos + 1;
                            let mut value_end = value_start;
                            while bytes.get(value_end).is_some_and(|byte| byte != quote) {
                                value_end += 1;
                            }
                            if bytes.get(value_end) != Some(quote) {
                                return None;
                            }
                            pos = value_end + 1;
                            Cow::Borrowed(&text[value_start..value_end])
                        }
                        Some(_) => {
                            let value_start = pos;
                            while let Some(&byte) = bytes.get(pos) {
                                if byte.is_ascii_whitespace() || byte == b'>' {
                                    break;
                                }
                                pos += 1;
                            }
                            Cow::Borrowed(&text[value_start..pos])
                        }
                        None => return None,
                    }
                } else {
                    Cow::Owned(name.clone())
                };
                parsed.push((name, value));
            }
            None => return None,
        }
    };

    let rest = &text[pos..];
    let contents = if self_closing {
        if !rest.is_empty() {
            return None;
        }
        ""
    } else {
        let close = rest.to_ascii_lowercase().find("</iframe>")?;
        if !rest[close + 9..].is_empty() {
            return None;
        }
        rest[..close].trim()
    };

    let src = parsed
        .iter()
        .find(|(name, _)| name == "src")
        .map(|(_, value)| value.trim())?;
    if !wikidot_iframe_url(src) {
        return None;
    }

    let mut html = String::from("<iframe");
    for (name, value) in &parsed {
        html.push(' ');
        html.push_str(name);
        html.push_str("=\"");
        for ch in value.chars() {
            match ch {
                '&' => html.push_str("&amp;"),
                '"' => html.push_str("&quot;"),
                '<' => html.push_str("&lt;"),
                _ => html.push(ch),
            }
        }
        html.push('\"');
    }
    html.push('>');
    for ch in contents.chars() {
        match ch {
            '&' => html.push_str("&amp;"),
            '<' => html.push_str("&lt;"),
            '>' => html.push_str("&gt;"),
            _ => html.push(ch),
        }
    }
    html.push_str("</iframe>");

    Some(Embed::Iframe {
        url: Cow::Owned(src.to_owned()),
        html: Cow::Owned(html),
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
    use crate::render::Render;
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
fn wikidot_embed_iframe_allow_list_matches_live() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::settings::{WikitextMode, WikitextSettings};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

    for (source, expected) in [
        (
            "[[embed]]<iframe src=\"http://example.com/x\"></iframe>[[/embed]]",
            "<p><iframe src=\"http://example.com/x\"></iframe></p>",
        ),
        (
            "[[embed]]<iframe src=\"HTTP://example.com/x\"></iframe>[[/embed]]",
            "<p><iframe src=\"HTTP://example.com/x\"></iframe></p>",
        ),
        (
            "[[embed]]<iframe src=\"ftp://example.com/x\"></iframe>[[/embed]]",
            "<p><iframe src=\"ftp://example.com/x\"></iframe></p>",
        ),
        (
            "[[embed]]<iframe src=\"gopher://example.com/x\"></iframe>[[/embed]]",
            "<p><iframe src=\"gopher://example.com/x\"></iframe></p>",
        ),
        (
            "[[embed]]<iframe src=\"mailto:a@example.com\"></iframe>[[/embed]]",
            "<p><iframe src=\"mailto:a@example.com\"></iframe></p>",
        ),
        (
            "[[embed]]<iframe src=\"https://example.com/x\"></iframe>[[/embed]]",
            "<p><iframe src=\"https://example.com/x\"></iframe></p>",
        ),
        (
            "[[embed]]<script src=\"https://example.com/a.js\"></script>[[/embed]]",
            "<div class=\"error-block\">Sorry, no match for the embedded content.</div>",
        ),
        (
            "[[embed]]<object data=\"x\"></object>[[/embed]]",
            "<div class=\"error-block\">Sorry, no match for the embedded content.</div>",
        ),
    ] {
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        assert!(errors.is_empty(), "{source:?}: {errors:#?}");
        assert_eq!(html, expected, "{source:?}");
    }
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
