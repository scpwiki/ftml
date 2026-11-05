/*
 * parsing/rule/impls/block/blocks/iframe.rs
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
use crate::parsing::discard_wikidot_controls;
use crate::tree::AttributeMap;
use std::borrow::Cow;

pub const BLOCK_IFRAME: BlockRule = BlockRule {
    name: "block-iframe",
    accepts_names: &["iframe"],
    accepts_star: false,
    accepts_score: false,
    // Live Wikidot renders the iframe inline: a newline next to the head
    // becomes a line break instead of being swallowed as block glue.
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
    debug!("Parsing iframe block (in-head {in_head})");
    assert!(!flag_star, "iframe doesn't allow star flag");
    assert!(!flag_score, "iframe doesn't allow score flag");
    assert_block_name(&BLOCK_IFRAME, name);

    let (url, arguments) = if parser.settings().layout.legacy() {
        parse_wikidot_head(parser, in_head)?
    } else {
        parser.get_head_name_map(&BLOCK_IFRAME, in_head)?
    };
    let url = if parser.settings().layout.legacy() {
        discard_wikidot_controls(url)
    } else {
        url
    };
    if !(if parser.settings().layout.legacy() {
        wikidot_iframe_url(&url)
    } else {
        crate::url::is_url(&url)
    }) {
        warn!("Iframe block references non-URL ({} bytes)", url.len());
        return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
    }

    let attributes = if parser.settings().layout.legacy() && arguments.has_spaced_equals()
    {
        AttributeMap::default()
    } else {
        arguments.to_attribute_map(parser.settings())
    };
    let element = Element::Iframe { url, attributes };

    ok!(parser.settings().layout.legacy(); element)
}

fn parse_wikidot_head<'r, 't>(
    parser: &mut Parser<'r, 't>,
    in_head: bool,
) -> Result<(Cow<'t, str>, Arguments<'t>), ParseError>
where
    'r: 't,
{
    if !in_head {
        return Err(parser.make_err(ParseErrorKind::BlockMissingName));
    }
    let (url, url_in_head) = parser.get_head_word(ParseErrorKind::ModuleMissingName)?;
    if !url_in_head && let Some((tail, match_end)) = greedy_url_extension(parser, &url) {
        consume_greedy_tail(parser, match_end)?;
        let mut extended = url.into_owned();
        extended.push_str(&tail);
        return Ok((Cow::Owned(extended), Arguments::new_case_sensitive()));
    }
    let arguments = parser.get_head_map_wikidot(&BLOCK_IFRAME, url_in_head)?;
    Ok((url, arguments))
}

fn consume_greedy_tail(
    parser: &mut Parser<'_, '_>,
    match_end: usize,
) -> Result<(), ParseError> {
    loop {
        let current = parser.current();
        if current.token == Token::InputEnd || current.span.start >= match_end {
            break;
        }
        let position = current.span.start;
        parser.step()?;
        if parser.current().span.start <= position {
            break;
        }
    }
    Ok(())
}

fn greedy_url_extension(parser: &Parser<'_, '_>, url: &str) -> Option<(String, usize)> {
    if !wikidot_iframe_url(&discard_wikidot_controls(Cow::Borrowed(url))) {
        return None;
    }
    let source = parser.full_text().inner();
    let after_head = parser.current().span.start;
    let head_end = after_head.checked_sub("]]".len())?;
    if source.get(head_end..after_head) != Some("]]") {
        return None;
    }
    let rest = source.get(head_end..)?;
    let (tail, match_end) = greedy_tail(rest)?;
    Some((tail, head_end + match_end))
}

/// Extend a bare iframe URL the way live Wikidot does.
///
/// When the head is exactly `[[iframe URL]]`, live keeps scanning past the
/// head close while the following text (up to the last `[[/name]]` closer)
/// contains no spaces or tabs. Newlines in the absorbed span are dropped.
/// Closed `[[code]]` spans are opaque to the scan, matching live extracting
/// code blocks before iframes. Returns the absorbed tail starting at the
/// head close (newlines removed) and the match end, both relative to the
/// head close.
fn greedy_tail(rest: &str) -> Option<(String, usize)> {
    debug_assert!(rest.starts_with("]]"));
    let tail = rest.get(2..)?;
    let spans = code_spans(tail);
    let bytes = tail.as_bytes();
    let mut candidates = Vec::new();
    let mut index = 0;
    while index + 3 <= bytes.len() {
        if bytes[index] == b'[' && bytes[index + 1] == b'[' && bytes[index + 2] == b'/' {
            let mut name_end = index + 3;
            while name_end < bytes.len()
                && !matches!(bytes[name_end], b' ' | b'\t' | b'\r' | b'\n' | b']' | b'[')
            {
                name_end += 1;
            }
            if !spans
                .iter()
                .any(|&(start, end)| index >= start && index < end)
            {
                candidates.push((index, name_end));
            }
        }
        index += 1;
    }
    for (_, name_end) in candidates.iter().rev() {
        let name_end = *name_end;
        let segment = rest.get(..2 + name_end)?;
        if segment.bytes().any(|byte| byte == b' ' || byte == b'\t') {
            continue;
        }
        if spans.iter().any(|&(start, _)| start + 2 < 2 + name_end) {
            continue;
        }
        let after = rest.get(2 + name_end..)?;
        if after.starts_with("]]") {
            let src_end = 2 + name_end;
            return Some((filter_tail(&rest[..src_end]), src_end + "]]".len()));
        }
        if let Some(close) = after.find("]]") {
            let src_end = 2 + name_end;
            return Some((filter_tail(&rest[..src_end]), src_end + close + "]]".len()));
        }
    }
    None
}

fn code_spans(tail: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let lower = tail.to_ascii_lowercase();
    let mut from = 0;
    while let Some(offset) = lower[from..].find("[[code") {
        let open = from + offset;
        let after_name = open + "[[code".len();
        if !matches!(
            tail.as_bytes().get(after_name).copied(),
            Some(b' ' | b'\t' | b'\r' | b'\n' | b']')
        ) {
            from = after_name;
            continue;
        }
        let Some(open_end) = tail[after_name..].find("]]") else {
            break;
        };
        let open_end = after_name + open_end + "]]".len();
        let Some(close) = lower[open_end..].find("[[/code]]") else {
            from = open_end;
            continue;
        };
        let close_end = open_end + close + "[[/code]]".len();
        spans.push((open, close_end));
        from = close_end;
    }
    spans
}

fn filter_tail(tail: &str) -> String {
    if !tail.bytes().any(|byte| byte == b'\r' || byte == b'\n') {
        return tail.to_owned();
    }
    tail.chars()
        .filter(|character| !matches!(character, '\r' | '\n'))
        .collect()
}

fn wikidot_iframe_url(url: &str) -> bool {
    [
        "http://",
        "https://",
        "ftp://",
        "gopher://",
        "mailto:",
        "news://",
        "mms://",
    ]
    .iter()
    .any(|prefix| {
        url.get(..prefix.len())
            .is_some_and(|value| value.eq_ignore_ascii_case(prefix))
    })
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn wikidot_iframe_discards_attributes_when_equals_is_spaced() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(
            r#"[[iframe https://example.com/ id = "my-example" class="iframe" width = "90%"]]"#,
        );
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            concat!(
                "<p><iframe src=\"https://example.com/\" align frameborder ",
                "height scrolling width class style></iframe></p>",
            ),
        );
    }

    #[test]
    fn wikidot_iframe_keeps_compact_attributes() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization =
            crate::tokenize(r#"[[iframe https://example.com/ frameborder="0"]]"#);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("frameborder=\"0\""), "{html}");
    }
}
