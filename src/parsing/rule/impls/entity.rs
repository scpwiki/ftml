/*
 * parsing/rule/impls/entity.rs
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
use entities::ENTITIES;
use std::borrow::Cow;
use std::char;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Decodes a complete `&name;`, `&#ddd;`, or `&#xhhh;` entity in ordinary text.
///
/// Only `Layout::Wikijump` decodes these. Live Wikidot escapes entity
/// spellings in prose (see the `entity-*` and `recorder-unit-entity-omega`
/// parity fixtures), so `Layout::Wikidot` falls through to plain text.
///
/// The result is an ordinary text element, which the renderer escapes like
/// any other text. An entity which decodes to `<`, `&`, or a quote therefore
/// produces that character as text, never as markup.
pub const RULE_ENTITY: Rule = Rule {
    name: "entity",
    position: LineRequirement::Any,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Trying to parse an HTML entity in text");

    let ampersand = parser.current();
    if parser.settings().layout.legacy() || ampersand.slice != "&" {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let remaining = parser.remaining();
    let body_len = match remaining.first() {
        Some(token) if token.token == Token::NumberedItem => 2,
        _ => 1,
    };
    let Some(tokens) = remaining.get(..=body_len) else {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    };
    let (body, semicolon) = (&tokens[..body_len], &tokens[body_len]);
    let adjacent = std::iter::once(ampersand)
        .chain(tokens)
        .zip(tokens)
        .all(|(left, right)| left.span.end == right.span.start);

    if !adjacent
        || body[body_len - 1].token != Token::Identifier
        || semicolon.token != Token::Other
        || semicolon.slice != ";"
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let entity = parser.full_text().slice_partial(&body[0], semicolon);
    let Some(decoded) = find_entity(entity).filter(|value| is_text_safe(value)) else {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    };

    parser.step_n(body_len + 2)?;
    ok!(Element::Text(decoded))
}

/// Whether a decoded entity may be emitted as ordinary text.
///
/// Control characters (including NUL and C1, which browsers remap rather than
/// emit) and Unicode noncharacters stay as their literal, escaped spelling.
fn is_text_safe(decoded: &str) -> bool {
    decoded.chars().all(|ch| {
        let codepoint = u32::from(ch);
        !ch.is_control()
            && !(0xFDD0..=0xFDEF).contains(&codepoint)
            && codepoint & 0xFFFE != 0xFFFE
    })
}

static ENTITY_MAPPING: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| {
        let mut mapping = HashMap::new();

        for entity in &ENTITIES {
            let key = strip_entity(entity.entity);
            let value = entity.characters;

            mapping.insert(key, value);
        }

        mapping
    });

pub(crate) fn decode_semicolon_entities(s: &str) -> Cow<'_, str> {
    let mut output = None;
    let mut start = 0;
    let mut offset = 0;

    while let Some(relative_entity_start) = s[offset..].find('&') {
        let entity_start = offset + relative_entity_start;
        let entity_body_start = entity_start + 1;
        let Some(relative_entity_end) = s[entity_body_start..].find(';') else {
            break;
        };

        let entity_end = entity_body_start + relative_entity_end;
        let entity = &s[entity_body_start..entity_end];

        if let Some(decoded) = find_entity(entity) {
            let output = output.get_or_insert_with(|| String::with_capacity(s.len()));
            output.push_str(&s[start..entity_start]);
            output.push_str(&decoded);
            start = entity_end + 1;
        }

        offset = entity_end + 1;
    }

    match output {
        Some(mut output) => {
            output.push_str(&s[start..]);
            Cow::Owned(output)
        }
        None => Cow::Borrowed(s),
    }
}

pub(crate) fn find_entity(entity: &str) -> Option<Cow<'_, str>> {
    if let Some(result) = ENTITY_MAPPING.get(entity) {
        return Some(cow!(result));
    }

    if let Some(value) = entity.strip_prefix("#x")
        && let Some(result) = get_char(value, 16)
    {
        return Some(result);
    }

    if let Some(value) = entity.strip_prefix('#')
        && let Some(result) = get_char(value, 10)
    {
        return Some(result);
    }

    None
}

fn get_char(value: &str, radix: u32) -> Option<Cow<'_, str>> {
    let codepoint = match u32::from_str_radix(value, radix) {
        Ok(codepoint) => codepoint,
        Err(_) => return None,
    };

    let ch = char::from_u32(codepoint)?;
    Some(Cow::Owned(ch.to_string()))
}

pub(crate) fn strip_entity(mut s: &str) -> &str {
    s = s.trim();

    if let Some(stripped) = s.strip_prefix('&') {
        s = stripped;
    }

    if let Some(stripped) = s.strip_suffix(';') {
        s = stripped;
    }

    s
}

#[test]
fn test_get_entity() {
    macro_rules! test {
        ($input:expr, $expected:expr $(,)?) => {{
            let actual = find_entity($input);
            let expected = $expected;

            assert_eq!(
                actual, expected,
                "Actual entity string doesn't match expected",
            );
        }};
    }

    test!("", None);

    test!("amp", Some(cow!("&")));
    test!("lt", Some(cow!("<")));
    test!("gt", Some(cow!(">")));
    test!("copy", Some(cow!("\u{a9}")));
    test!("xxxzzz", None);

    test!("#32", Some(cow!(" ")));
    test!("#255", Some(cow!("\u{ff}")));
    test!("#128175", Some(cow!("\u{1f4af}")));
    test!("#2097151", None);

    test!("#x20", Some(cow!(" ")));
    test!("#xff", Some(cow!("\u{ff}")));
    test!("#x1f4af", Some(cow!("\u{1f4af}")));
    test!("#x1fffff", None);
}

#[test]
fn test_get_char() {
    macro_rules! test {
        ($value:expr, $radix:expr, $expected:expr $(,)?) => {{
            let actual = get_char($value, $radix);
            let expected = $expected;

            assert_eq!(
                actual, expected,
                "Actual character value doesn't match expected",
            );
        }};
    }

    test!("32", 10, Some(Cow::Owned(str!(' '))));
    test!("255", 10, Some(Cow::Owned(str!('\u{ff}'))));
    test!("128175", 10, Some(Cow::Owned(str!('\u{1f4af}'))));
    test!("2097151", 10, None);

    test!("20", 16, Some(Cow::Owned(str!(' '))));
    test!("ff", 16, Some(Cow::Owned(str!('\u{ff}'))));
    test!("1f4af", 16, Some(Cow::Owned(str!('\u{1f4af}'))));
    test!("1fffff", 16, None);
}

#[test]
fn test_strip_entity() {
    macro_rules! test {
        ($input:expr, $expected:expr $(,)?) => {{
            let actual = strip_entity($input);
            let expected = $expected;

            assert_eq!(
                actual, expected,
                "Actual stripped entity value didn't match expected",
            );
        }};
    }

    test!("", "");
    test!("abc", "abc");
    test!("legumes1", "legumes1");
    test!("&amp;", "amp");
    test!("&#100;", "#100");
    test!("&xdeadbeef;", "xdeadbeef");

    test!("&amp", "amp");
    test!("amp;", "amp");
    test!("&#100", "#100");
    test!("#100;", "#100");

    test!(" &amp; ", "amp");
}

#[test]
fn decode_semicolon_entities_decodes_complete_valid_entities() {
    assert_eq!(
        decode_semicolon_entities("&copy; &#252; &#8212; &#x2014;"),
        "\u{a9} \u{fc} \u{2014} \u{2014}",
    );
}

#[test]
fn decode_semicolon_entities_preserves_unknown_and_incomplete_entities() {
    assert_eq!(
        decode_semicolon_entities("&copy &not-an-entity; &copy"),
        "&copy &not-an-entity; &copy",
    );
}

#[cfg(test)]
fn render_entity_html(input: &str, layout: crate::layout::Layout) -> String {
    use crate::data::PageInfo;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let tokenization = crate::tokenize(input);
    let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
    assert!(errors.is_empty(), "{input:?}: {errors:?}");
    HtmlRender.render(&tree, &page_info, &settings).body
}

#[test]
fn wikijump_text_decodes_complete_entities() {
    use crate::layout::Layout;

    assert_eq!(
        render_entity_html(
            "a&mdash;b &darr; c&nbsp;d &#8212; &#x2014; {{e &copy;}}",
            Layout::Wikijump,
        ),
        "<p>a\u{2014}b \u{2193} c\u{a0}d \u{2014} \u{2014} \
         <code class=\"wj-monospace\">e \u{a9}</code></p>",
    );
}

#[test]
fn entity_spellings_stay_escaped_unless_decoded_by_wikijump_text() {
    use crate::layout::Layout;

    assert_eq!(
        render_entity_html(
            "&mdash &bogus; &#X41; &#0; &#x80; &#xFFFE; &Tab;",
            Layout::Wikijump,
        ),
        "<p>&amp;mdash &amp;bogus; &amp;#X41; &amp;#0; &amp;#x80; &amp;#xFFFE; &amp;Tab;</p>",
    );

    // Raw text is not decoded, and live Wikidot escapes entities in prose.
    assert_eq!(
        render_entity_html("@@&mdash;@@", Layout::Wikijump),
        "<p><span class=\"wj-raw\">&amp;mdash;</span></p>",
    );
    assert_eq!(
        render_entity_html("a&mdash;b {{&copy;}}", Layout::Wikidot),
        "<p>a&amp;mdash;b <tt>&amp;copy;</tt></p>",
    );
}
