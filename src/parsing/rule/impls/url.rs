/*
 * parsing/rule/impls/url.rs
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
use crate::parsing::collect::{CommentElidedText, consume_valid_comment};
use crate::parsing::discard_wikidot_controls;
use crate::tree::{AnchorTarget, LinkLabel, LinkLocation, LinkType};
use std::borrow::Cow;

pub const RULE_URL: Rule = Rule {
    name: "url",
    position: LineRequirement::Any,
    try_consume_fn,
};

pub(crate) fn url_elements<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Elements<'t>, ParseError>
where
    'r: 't,
{
    if parser.settings().layout.legacy() {
        return wikidot_url_elements(parser);
    }

    let token = parser.current();
    let source = parser.full_text().inner();
    let start = token.span.start;
    let starred = token.token == Token::UrlStar;
    let url = &source[start + usize::from(starred)..token.span.end];
    let link = Element::Link {
        ltype: LinkType::Direct,
        link: LinkLocation::Url(cow!(url)),
        label: LinkLabel::Url,
        target: None,
    };

    parser.step()?;
    if starred {
        return Ok(Elements::Multiple(vec![text!("*"), link]));
    }
    Ok(Elements::Single(link))
}

fn wikidot_url_elements<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Elements<'t>, ParseError>
where
    'r: 't,
{
    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let starred = parser.current().token == Token::UrlStar;
    let url_start = start + usize::from(starred);
    let mut end = parser.current().span.end;
    let mut comments = Vec::new();
    parser.step()?;

    loop {
        if parser.current_generated().is_some()
            || matches!(
                parser.current().token,
                Token::GeneratedPageLink | Token::GeneratedTagLinks | Token::RuntimeText
            )
        {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
        if parser.current().token == Token::LeftComment
            && parser.current().span.start == end
        {
            comments.push(consume_valid_comment(parser)?);
            end = parser.current().span.start;
            continue;
        }
        if parser.current().token == Token::DiscardedControl
            && parser.current().span.start == end
        {
            end = parser.current().span.end;
            parser.step()?;
            continue;
        }
        if parser.current().token == Token::InputEnd {
            break;
        }

        let token = parser.current();
        let token_text = &source[token.span.clone()];
        if token_text.chars().next().is_some_and(|character| {
            character.is_whitespace() && !character.is_ascii_whitespace()
        }) {
            let next_start = token.span.end;
            let Some(next_character) = source[next_start..].chars().next() else {
                break;
            };
            let next_end = next_start + next_character.len_utf8();
            if wikidot_automatic_url_end(source, next_start, next_end) == next_start {
                break;
            }
            end = token.span.end;
            parser.step()?;
            continue;
        }
        let fragment_end =
            wikidot_automatic_url_end(source, token.span.start, token.span.end);
        if fragment_end != token.span.end {
            break;
        }
        end = token.span.end;
        parser.step()?;
    }

    let field = CommentElidedText::new(source, url_start..end, comments);
    let mut url = discard_wikidot_controls(field.into_cow());
    let previous = start
        .checked_sub(1)
        .and_then(|index| source.as_bytes().get(index));
    let suffix = match (previous, url.as_bytes().last()) {
        (Some(b'('), Some(b')')) => Some(")"),
        (Some(b'['), Some(b']')) => Some("]"),
        _ => None,
    };
    if suffix.is_some() {
        url = without_last_character(url);
    }

    let mut trailing = Vec::new();
    while let Some(last) = url.chars().next_back() {
        if !matches!(last, ',' | '*') && (last.is_ascii() || last.is_whitespace()) {
            break;
        }
        trailing.push(last);
        url = without_last_character(url);
    }
    let trailing: String = trailing.iter().rev().collect();

    let split_terminal_period = url.ends_with('.')
        && (matches!(parser.current().token, Token::Whitespace | Token::InputEnd)
            || suffix.is_some());
    if split_terminal_period {
        url = without_last_character(url);
    }

    let link = Element::Link {
        ltype: LinkType::Direct,
        link: LinkLocation::Url(url),
        label: LinkLabel::Url,
        target: starred.then_some(AnchorTarget::NewTab),
    };
    let mut elements = vec![link];
    if !trailing.is_empty() {
        elements.push(Element::Text(Cow::Owned(trailing)));
    }
    if split_terminal_period {
        elements.push(text!("."));
    }
    if let Some(suffix) = suffix {
        elements.push(text!(suffix));
    }

    Ok(if elements.len() == 1 {
        Elements::Single(elements.pop().unwrap())
    } else {
        Elements::Multiple(elements)
    })
}

fn without_last_character(value: Cow<'_, str>) -> Cow<'_, str> {
    match value {
        Cow::Borrowed(value) => {
            let last = value
                .chars()
                .next_back()
                .expect("automatic URL suffix removal requires a non-empty value");
            Cow::Borrowed(
                value
                    .strip_suffix(last)
                    .expect("last character must be a suffix"),
            )
        }
        Cow::Owned(mut value) => {
            value.pop();
            Cow::Owned(value)
        }
    }
}

fn wikidot_automatic_url_end(source: &str, mut end: usize, limit: usize) -> usize {
    let bytes = source.as_bytes();
    while end < limit {
        let character = source[end..].chars().next().unwrap();
        if matches!(
            bytes[end],
            b'\n' | b'\r' | b' ' | b'\t' | b'"' | b'\'' | b'['
        ) || matches!(bytes[end], 0x00..=0x08 | 0x0b..=0x0c | 0x0e..=0x1a | 0x1c..=0x1f)
            || source[end..].starts_with("@@")
            || source[end..].starts_with("]]")
            || source[end..].starts_with("||")
        {
            break;
        }
        end += character.len_utf8();
    }
    end
}

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Consuming token as a URL");
    ok!(url_elements(parser)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    fn assert_url_rule(source: &str, layout: Layout, expected: Elements<'static>) {
        let tokenization = crate::tokenize(source);
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().expect("URL token should follow input start");
        let actual = RULE_URL
            .try_consume(&mut parser)
            .expect("URL rule should consume the URL")
            .item;
        assert_eq!(actual, expected);
    }

    fn link(url: &'static str) -> Element<'static> {
        Element::Link {
            ltype: LinkType::Direct,
            link: LinkLocation::Url(cow!(url)),
            label: LinkLabel::Url,
            target: None,
        }
    }

    #[test]
    fn wikidot_url_rule_splits_terminal_period_at_text_boundaries() {
        let expected =
            Elements::Multiple(vec![link("https://example.com/test"), text!(".")]);
        assert_url_rule(
            "https://example.com/test. next",
            Layout::Wikidot,
            expected.clone(),
        );
        assert_url_rule("https://example.com/test.", Layout::Wikidot, expected);
    }

    #[test]
    fn url_rule_keeps_other_periods_and_wikijump_behavior() {
        assert_url_rule(
            "https://example.com/a.b/c next",
            Layout::Wikidot,
            link("https://example.com/a.b/c").into(),
        );
        assert_url_rule(
            "https://example.com/test. next",
            Layout::Wikijump,
            link("https://example.com/test.").into(),
        );
    }

    #[test]
    fn wikidot_url_rule_extends_live_punctuation_and_stops_at_block_syntax() {
        assert_url_rule(
            "http://example.com/path|END next",
            Layout::Wikidot,
            link("http://example.com/path|END").into(),
        );
        assert_url_rule(
            "mailto:User@example.com|END next",
            Layout::Wikidot,
            link("mailto:User@example.com|END").into(),
        );

        let source = "https://example.com/video[[/embedvideo]]";
        let tokenization = crate::tokenize(source);
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().expect("URL token should follow input start");
        let actual = RULE_URL
            .try_consume(&mut parser)
            .expect("URL rule should stop before block syntax")
            .item;

        assert_eq!(actual, link("https://example.com/video").into());
        assert_eq!(parser.current().token, Token::LeftBlockEnd);
    }

    #[test]
    fn wikidot_url_rule_keeps_nbsp_inside_the_automatic_url() {
        assert_url_rule(
            "https://example.com/a.png\u{00a0}width= next",
            Layout::Wikidot,
            link("https://example.com/a.png\u{00a0}width=").into(),
        );
        assert_url_rule(
            "https://example.com/a.png\u{00a0}width= next",
            Layout::Wikijump,
            link("https://example.com/a.png").into(),
        );
        assert_url_rule(
            "https://example.com\u{00a0}]]",
            Layout::Wikidot,
            link("https://example.com").into(),
        );
    }

    #[test]
    fn wikidot_url_rule_keeps_live_unicode_spaces_inside_the_automatic_url() {
        // Anonymous PagePreview observations on 2026-08-21 show that Wikidot
        // treats these Unicode whitespace scalars as URL data rather than as
        // automatic-link terminators. ASCII space/tab/newline remain the
        // actual whitespace boundaries.
        for whitespace in [
            '\u{1680}', '\u{2000}', '\u{2001}', '\u{2002}', '\u{2003}', '\u{2004}',
            '\u{2005}', '\u{2006}', '\u{2007}', '\u{2008}', '\u{2009}', '\u{200a}',
            '\u{2028}', '\u{2029}', '\u{202f}', '\u{205f}', '\u{3000}',
        ] {
            let source = format!("https://example.com/a{whitespace}b next");
            let expected = format!("https://example.com/a{whitespace}b");
            let tokenization = crate::tokenize(&source);
            let page_info = PageInfo::dummy();
            let settings =
                WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
            let mut parser = Parser::new(&tokenization, &page_info, &settings);
            parser.step().expect("URL token should follow input start");
            let actual = RULE_URL
                .try_consume(&mut parser)
                .expect("URL rule should consume live Unicode URL whitespace")
                .item;
            assert_eq!(
                actual,
                Element::Link {
                    ltype: LinkType::Direct,
                    link: LinkLocation::Url(Cow::Owned(expected)),
                    label: LinkLabel::Url,
                    target: None,
                }
                .into(),
                "U+{:04X}",
                whitespace as u32,
            );
        }
    }

    #[test]
    fn wikidot_url_rule_stops_before_a_token_that_contains_a_url_delimiter() {
        let source = "https://example.com/\u{000b}\\\" next";
        let tokenization = crate::tokenize(source);
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().expect("URL token should follow input start");

        let actual = RULE_URL
            .try_consume(&mut parser)
            .expect("URL rule should stop before the escaped quote token")
            .item;

        assert_eq!(actual, link("https://example.com/").into());
        assert_eq!(parser.current().token, Token::EscapedDoubleQuote);
    }

    fn star_link(url: &'static str) -> Element<'static> {
        Element::Link {
            ltype: LinkType::Direct,
            link: LinkLocation::Url(cow!(url)),
            label: LinkLabel::Url,
            target: Some(AnchorTarget::NewTab),
        }
    }

    #[test]
    fn wikidot_url_rule_star_prefix_opens_in_a_new_tab() {
        assert_url_rule(
            "*https://example.com/new-tab-target",
            Layout::Wikidot,
            star_link("https://example.com/new-tab-target").into(),
        );
        assert_url_rule(
            "*https://example.com/new-tab-target",
            Layout::Wikijump,
            Elements::Multiple(vec![
                text!("*"),
                link("https://example.com/new-tab-target"),
            ]),
        );
    }

    #[test]
    fn wikidot_url_rule_splits_trailing_comma_and_stars() {
        assert_url_rule(
            "http://example.com/page, mid",
            Layout::Wikidot,
            Elements::Multiple(vec![link("http://example.com/page"), text!(",")]),
        );
        assert_url_rule(
            "http://example.com/b** end",
            Layout::Wikidot,
            Elements::Multiple(vec![link("http://example.com/b"), text!("**")]),
        );
        assert_url_rule(
            "http://example.com/a,b next",
            Layout::Wikidot,
            link("http://example.com/a,b").into(),
        );
    }

    #[test]
    fn wikidot_url_rule_stops_at_non_ascii_text() {
        assert_url_rule(
            "http://example.com/pg」 quoted",
            Layout::Wikidot,
            Elements::Multiple(vec![link("http://example.com/pg"), text!("」")]),
        );
        assert_url_rule(
            "http://example.com/pageです",
            Layout::Wikidot,
            Elements::Multiple(vec![link("http://example.com/page"), text!("です")]),
        );
    }

    #[test]
    fn wikidot_autolink_star_and_boundaries_render_like_live() {
        use crate::render::Render;

        let source = "star mid *http://example.com/star-mid x\n\nbold **http://example.com/b** end\n\nnot-a-link abchttp://example.com/pg here";
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = source.to_owned();
        crate::preprocess_for_layout(&mut source, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let page_info = PageInfo::dummy();
        let (tree, _) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        assert!(
            html.contains(
                "star mid <a href=\"http://example.com/star-mid\" target=\"_blank\">http://example.com/star-mid</a> x"
            ),
            "{html}",
        );
        assert!(
            html.contains(
                "bold *<a href=\"http://example.com/b\" target=\"_blank\">http://example.com/b</a>** end"
            ),
            "{html}",
        );
        assert!(
            html.contains("<p>not-a-link abchttp://example.com/pg here</p>"),
            "{html}",
        );
    }
}
