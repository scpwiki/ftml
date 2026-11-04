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

pub const RULE_WIKIDOT_SCHEME_URL: Rule = Rule {
    name: "wikidot-scheme-url",
    position: LineRequirement::Any,
    try_consume_fn: wikidot_scheme_url,
};

const WIKIDOT_ONLY_AUTOLINK_SCHEMES: [&str; 9] = [
    "gopher://",
    "news://",
    "mms://",
    "ssh://",
    "git://",
    "sftp://",
    "svn://",
    "telnet://",
    "cvs://",
];

pub(crate) fn is_wikidot_only_autolink_url(url: &str) -> bool {
    WIKIDOT_ONLY_AUTOLINK_SCHEMES
        .iter()
        .any(|scheme| url.starts_with(scheme))
}

pub(crate) fn starts_wikidot_only_scheme_url(source: &str, start: usize) -> bool {
    WIKIDOT_ONLY_AUTOLINK_SCHEMES.iter().any(|scheme| {
        source[start..].starts_with(scheme)
            && wikidot_automatic_url_end(source, start, source.len())
                > start + scheme.len()
    })
}

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
    let url_start = start + usize::from(starred);
    let mut advanced = false;
    let url = if parser.look_ahead(0).is_some_and(|next| {
        next.token == Token::LeftComment && next.span.start == token.span.end
    }) {
        match strict_url_past_comments(parser, source, url_start, token.span.end) {
            Some((url, probe)) => {
                parser.update(&probe);
                advanced = true;
                url
            }
            None => Cow::Borrowed(&source[url_start..token.span.end]),
        }
    } else {
        Cow::Borrowed(&source[url_start..token.span.end])
    };

    let link = Element::Link {
        ltype: LinkType::Direct,
        link: LinkLocation::Url(url),
        label: LinkLabel::Url,
        target: None,
    };

    if !advanced {
        parser.step()?;
    }
    if starred {
        return Ok(Elements::Multiple(vec![text!("*"), link]));
    }
    Ok(Elements::Single(link))
}

fn strict_url_past_comments<'r, 't>(
    parser: &Parser<'r, 't>,
    source: &'t str,
    url_start: usize,
    first_end: usize,
) -> Option<(Cow<'t, str>, Parser<'r, 't>)>
where
    'r: 't,
{
    let mut probe = parser.clone();
    probe.step().ok()?;

    let mut comments = Vec::new();
    let mut end = first_end;
    let mut scan_pos = first_end;
    let mut last_char = source[..first_end].chars().next_back();

    loop {
        let current = probe.current();
        if probe.current_generated().is_some() {
            break;
        }
        if current.token == Token::LeftComment && current.span.start == scan_pos {
            match consume_valid_comment(&mut probe) {
                Ok(range) => {
                    scan_pos = range.end;
                    comments.push(range);
                    continue;
                }
                Err(_) => break,
            }
        }
        if current.span.start != scan_pos
            || current.token == Token::InputEnd
            || !strict_url_token_continues(source, current.span.clone(), last_char)
        {
            break;
        }
        end = current.span.end;
        scan_pos = end;
        last_char = current.slice.chars().next_back();
        probe.step().ok()?;
    }

    if comments.is_empty() || end == first_end {
        return None;
    }
    let field = CommentElidedText::new(source, url_start..end, comments);
    Some((field.into_cow(), probe))
}

fn strict_url_token_continues(
    source: &str,
    span: std::ops::Range<usize>,
    last_char: Option<char>,
) -> bool {
    let first = source[span.clone()].chars().next();
    if matches!(
        (last_char, first),
        (Some('>'), Some('@')) | (Some('@'), Some('@'))
    ) {
        return false;
    }
    let bytes = source.as_bytes();
    let mut pos = span.start;
    while pos < span.end {
        let byte = bytes[pos];
        let character = source[pos..]
            .chars()
            .next()
            .expect("token slice is valid UTF-8");
        if matches!(
            byte,
            b'\n' | b'\r' | b' ' | b'\t' | b'"' | b'\'' | b'|' | b'[' | b']'
        ) || character.is_whitespace()
            || matches!(byte, 0x00..=0x08 | 0x0b..=0x0c | 0x0e..=0x1a | 0x1c..=0x1f)
            || source[pos..].starts_with(">@")
            || source[pos..].starts_with("@@")
        {
            return false;
        }
        pos += character.len_utf8();
    }
    true
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
    let end = parser.current().span.end;
    let comments = Vec::new();
    parser.step()?;

    wikidot_url_elements_rest(parser, source, start, url_start, end, starred, comments)
}

pub(super) fn wikidot_star_link_fallback<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Option<ParseResult<'r, 't, Elements<'t>>> {
    if parser.current().token != Token::Url {
        return None;
    }
    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let end = parser.current().span.end;
    parser.step().ok()?;
    let rest =
        wikidot_url_elements_rest(parser, source, start, start, end, true, Vec::new())
            .ok()?;
    let mut elements = vec![text!("[")];
    match rest {
        Elements::None => {}
        Elements::Single(element) => elements.push(element),
        Elements::Multiple(mut rest) => elements.append(&mut rest),
    }
    Some(ok!(Elements::Multiple(elements)))
}

fn wikidot_scheme_url<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Consuming identifier as a Wikidot-only scheme URL");
    if !parser.settings().layout.legacy() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let end = parser.current().span.end;
    if starts_wikidot_only_scheme_url(source, start) {
        let comments = Vec::new();
        parser.step()?;

        return ok!(wikidot_url_elements_rest(
            parser, source, start, start, end, false, comments
        )?);
    }

    let Some(url_start) = glued_scheme_url_start(source, start, end) else {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    };
    let comments = Vec::new();
    parser.step()?;

    let rest = wikidot_url_elements_rest(
        parser, source, start, url_start, end, false, comments,
    )?;
    let mut elements = vec![text!(&source[start..url_start])];
    match rest {
        Elements::None => {}
        Elements::Single(element) => elements.push(element),
        Elements::Multiple(mut rest) => elements.append(&mut rest),
    }
    ok!(Elements::Multiple(elements))
}

const GLUED_SCHEME_NAMES: [(&str, &str); 13] = [
    ("https", "https://"),
    ("http", "http://"),
    ("ftp", "ftp://"),
    ("mailto", "mailto:"),
    ("gopher", "gopher://"),
    ("news", "news://"),
    ("mms", "mms://"),
    ("ssh", "ssh://"),
    ("git", "git://"),
    ("sftp", "sftp://"),
    ("svn", "svn://"),
    ("telnet", "telnet://"),
    ("cvs", "cvs://"),
];

pub(crate) fn identifier_may_start_wikidot_url(slice: &str) -> bool {
    GLUED_SCHEME_NAMES.iter().any(|(name, _)| {
        slice == *name
            || slice.strip_suffix(name).is_some_and(|prefix| {
                prefix
                    .bytes()
                    .last()
                    .is_some_and(|byte| byte.is_ascii_digit())
            })
    })
}

pub(crate) fn glued_scheme_url_start(
    source: &str,
    start: usize,
    end: usize,
) -> Option<usize> {
    let text = &source[start..end];
    let mut best = None;
    for (name, prefix) in GLUED_SCHEME_NAMES {
        let Some(head) = text.strip_suffix(name) else {
            continue;
        };
        if !head
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_digit())
        {
            continue;
        }
        let url_start = end - name.len();
        if !source[url_start..].starts_with(prefix) {
            continue;
        }
        if best.is_none_or(|current| url_start < current) {
            best = Some(url_start);
        }
    }
    best
}

fn wikidot_url_elements_rest<'r, 't>(
    parser: &mut Parser<'r, 't>,
    source: &'t str,
    start: usize,
    url_start: usize,
    mut end: usize,
    starred: bool,
    mut comments: Vec<std::ops::Range<usize>>,
) -> Result<Elements<'t>, ParseError>
where
    'r: 't,
{
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
        if matches!(parser.current().token, Token::Raw | Token::LeftRaw) {
            let mut raw_probe = parser.clone();
            if super::raw::RULE_RAW.try_consume(&mut raw_probe).is_ok() {
                break;
            }
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

    let at_sentence_boundary =
        matches!(parser.current().token, Token::Whitespace | Token::InputEnd)
            || suffix.is_some();
    let mut trailing = Vec::new();
    if at_sentence_boundary {
        while let Some(last) = url.chars().next_back() {
            if !matches!(
                last,
                ',' | '*'
                    | '-'
                    | '}'
                    | '{'
                    | '.'
                    | ';'
                    | ':'
                    | '!'
                    | ')'
                    | ']'
                    | '>'
                    | '^'
                    | '`'
                    | '|'
                    | '#'
            ) && (last.is_ascii() || last.is_whitespace())
            {
                break;
            }
            trailing.push(last);
            url = without_last_character(url);
        }
    }
    let trailing: String = trailing.iter().rev().collect();

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
        if matches!(bytes[end], b'\n' | b'\r' | b' ' | b'\t' | b'"' | b'\'')
            || matches!(bytes[end], 0x00..=0x08 | 0x0b..=0x0c | 0x0e..=0x1a | 0x1c..=0x1f)
            || source[end..].starts_with("[[")
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

    #[test]
    fn wikidot_url_rule_splits_trailing_dash_and_brace_runs() {
        assert_url_rule(
            "http://example.com/a-- next",
            Layout::Wikidot,
            Elements::Multiple(vec![link("http://example.com/a"), text!("--")]),
        );
        assert_url_rule(
            "http://example.com/a}}",
            Layout::Wikidot,
            Elements::Multiple(vec![link("http://example.com/a"), text!("}}")]),
        );
        assert_url_rule(
            "http://example.com/a-",
            Layout::Wikidot,
            Elements::Multiple(vec![link("http://example.com/a"), text!("-")]),
        );
        assert_url_rule(
            "http://example.com/a--b",
            Layout::Wikidot,
            link("http://example.com/a--b").into(),
        );
        assert_url_rule(
            "http://example.com/a-- next",
            Layout::Wikijump,
            link("http://example.com/a--").into(),
        );
        assert_url_rule(
            "http://example.com/a}}",
            Layout::Wikijump,
            link("http://example.com/a}}").into(),
        );
    }

    #[test]
    fn wikidot_autolink_recognizes_live_schemes() {
        use crate::render::Render;

        let source = "more-schemes gopher://example.com/g news://example.com/n mms://example.com/m end\n\ndescribed [gopher://example.com/g G] [news://example.com/n N] [mms://example.com/m M] end\n\nstar *gopher://example.com/g mid **news://example.com/n end";
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = source.to_owned();
        crate::preprocess_for_layout(&mut source, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let page_info = PageInfo::dummy();
        let (tree, _) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        for expected in [
            "<a href=\"gopher://example.com/g\">gopher://example.com/g</a>",
            "<a href=\"news://example.com/n\">news://example.com/n</a>",
            "<a href=\"mms://example.com/m\">mms://example.com/m</a>",
            "<a href=\"gopher://example.com/g\">G</a>",
            "<a href=\"news://example.com/n\">N</a>",
            "<a href=\"mms://example.com/m\">M</a>",
            "<a href=\"gopher://example.com/g\" target=\"_blank\">gopher://example.com/g</a>",
            "*<a href=\"news://example.com/n\" target=\"_blank\">news://example.com/n</a>",
        ] {
            assert!(html.contains(expected), "{expected} missing from {html}");
        }
    }

    #[test]
    fn wikidot_autolink_schemes_stay_literal_in_wikijump_layout() {
        use crate::render::Render;

        let source = "more-schemes gopher://example.com/g news://example.com/n mms://example.com/m end";
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tokenization = crate::tokenize(source);
        let page_info = PageInfo::dummy();
        let (tree, _) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        assert_eq!(
            html,
            "<p>more-schemes gopher:<em>example.com/g news:</em>example.com/n mms://example.com/m end</p>",
        );
    }

    #[test]
    fn wikidot_containers_close_over_urls_like_live() {
        use crate::render::Render;

        let source = "mono {{http://example.com/m}} end\n\nstrike --http://example.com/s-- end\n\nmono-text {{see http://example.com/m}} end\n\nstrike-text --see http://example.com/s-- end\n\nrun {{http://example.com/m}}}} end\n\ninternal --http://example.com/a--b-- end\n\nnoclose --http://example.com/a--b end\n\nbare http://example.com/a-- http://example.com/a}} http://example.com/a- end\n\nglued --http://example.com/s--tail {{http://example.com/m}}tail end";
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = source.to_owned();
        crate::preprocess_for_layout(&mut source, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let page_info = PageInfo::dummy();
        let (tree, _) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        for expected in [
            "<tt><a href=\"http://example.com/m\">http://example.com/m</a></tt>",
            "<span style=\"text-decoration: line-through;\"><a href=\"http://example.com/s\">http://example.com/s</a></span>",
            "<tt>see <a href=\"http://example.com/m\">http://example.com/m</a></tt>",
            "<span style=\"text-decoration: line-through;\">see <a href=\"http://example.com/s\">http://example.com/s</a></span>",
            "<tt><a href=\"http://example.com/m\">http://example.com/m</a>}}</tt>",
            "<span style=\"text-decoration: line-through;\"><a href=\"http://example.com/a--b\">http://example.com/a--b</a></span>",
            "\u{2014}<a href=\"http://example.com/a--b\">http://example.com/a--b</a>",
            "<a href=\"http://example.com/a\">http://example.com/a</a>\u{2014}",
            "<a href=\"http://example.com/a\">http://example.com/a</a>}}",
            "<a href=\"http://example.com/a\">http://example.com/a</a>-",
            "\u{2014}<a href=\"http://example.com/s--tail\">http://example.com/s--tail</a>",
            "{{<a href=\"http://example.com/m}}tail\">http://example.com/m}}tail</a>",
        ] {
            assert!(html.contains(expected), "{expected} missing from {html}");
        }
    }

    #[test]
    fn wikidot_superscript_subscript_close_over_urls_like_live() {
        use crate::render::Render;

        let source = "sup ^^http://example.com/s^^ end\n\nsub ,,http://example.com/s,, end\n\ninternal ,,http://example.com/a,,b,, end\n\nrun ^^http://example.com/s^^^ end\n\nsplit ^^note http://example.com/s^^ tail^^ end\n\nperiod ^^http://example.com/s^^. end\n\nbefore ^^http://example.com/s.^^ end\n\nnoclose ^^http://example.com/s^^tail end\n\nmismatch ^^http://example.com/s,, end";
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = source.to_owned();
        crate::preprocess_for_layout(&mut source, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let page_info = PageInfo::dummy();
        let (tree, _) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        for expected in [
            "<sup><a href=\"http://example.com/s\">http://example.com/s</a></sup>",
            "<sub><a href=\"http://example.com/s\">http://example.com/s</a></sub>",
            "<sub><a href=\"http://example.com/a,,b\">http://example.com/a,,b</a></sub>",
            "<sup><a href=\"http://example.com/s\">http://example.com/s</a></sup>^",
            "<sup>note <a href=\"http://example.com/s\">http://example.com/s</a></sup> tail^^",
            "<sup><a href=\"http://example.com/s\">http://example.com/s</a></sup>.",
            "<sup><a href=\"http://example.com/s\">http://example.com/s</a>.</sup>",
            "^^<a href=\"http://example.com/s^^tail\">http://example.com/s^^tail</a>",
            "^^<a href=\"http://example.com/s\">http://example.com/s</a>,,",
        ] {
            assert!(html.contains(expected), "{expected} missing from {html}");
        }
    }

    #[test]
    fn wikidot_monospace_close_over_url_sheds_like_live() {
        use crate::render::Render;

        let source = "shed {{https://example.com/a}}. end\n\nsplit {{note https://example.com/a}} tail}} end\n\nbefore {{https://example.com/a.}} end\n\nrun {{https://example.com/a}}}} end\n\ndouble {{https://example.com/a}}!}} end\n\nspaced {{note https://example.com/a}} }} end\n\nnoclose {{https://example.com/a}. end";
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = source.to_owned();
        crate::preprocess_for_layout(&mut source, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let page_info = PageInfo::dummy();
        let (tree, _) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        for expected in [
            "<tt><a href=\"https://example.com/a\">https://example.com/a</a></tt>.",
            "<tt>note <a href=\"https://example.com/a\">https://example.com/a</a></tt> tail}}",
            "<tt><a href=\"https://example.com/a\">https://example.com/a</a>.</tt>",
            "<tt><a href=\"https://example.com/a\">https://example.com/a</a>}}</tt>",
            "<tt><a href=\"https://example.com/a\">https://example.com/a</a></tt>!}}",
            "<tt>note <a href=\"https://example.com/a\">https://example.com/a</a></tt> }}",
            "{{<a href=\"https://example.com/a\">https://example.com/a</a>}.",
        ] {
            assert!(html.contains(expected), "{expected} missing from {html}");
        }
    }

    #[test]
    fn wikidot_strikethrough_close_over_url_sheds_like_live() {
        use crate::render::Render;

        let source = "shed --https://example.com/a--. end\n\nbefore --https://example.com/a.-- end\n\nrun --https://example.com/a---. end\n\ndouble --https://example.com/a--.-- end\n\nopen ---https://example.com/a--. end\n\nsplit --note https://example.com/a-- tail-- end\n\nsplit-suffix --note https://example.com/a--. tail-- end\n\nsecond --https://example.com/a--. and --b-- end\n\nnoclose --https://example.com/a--tail end\n\npadded -- https://example.com/a--. end";
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = source.to_owned();
        crate::preprocess_for_layout(&mut source, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let page_info = PageInfo::dummy();
        let (tree, _) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        for expected in [
            "<span style=\"text-decoration: line-through;\"><a href=\"https://example.com/a\">https://example.com/a</a></span>.",
            "<span style=\"text-decoration: line-through;\"><a href=\"https://example.com/a\">https://example.com/a</a>.</span>",
            "<span style=\"text-decoration: line-through;\">note <a href=\"https://example.com/a\">https://example.com/a</a></span> tail\u{2014}",
            "<span style=\"text-decoration: line-through;\">note <a href=\"https://example.com/a\">https://example.com/a</a></span>. tail\u{2014}",
            "<span style=\"text-decoration: line-through;\">b</span>",
            "\u{2014}<a href=\"https://example.com/a--tail\">https://example.com/a--tail</a>",
            "\u{2014} <a href=\"https://example.com/a\">https://example.com/a</a>\u{2014}.",
        ] {
            assert!(html.contains(expected), "{expected} missing from {html}");
        }

        assert!(
            html.contains(
                "<span style=\"text-decoration: line-through;\"><a href=\"https://example.com/a\">https://example.com/a</a></span>.\u{2014}"
            ),
            "double-shed paragraph missing from {html}"
        );
        assert!(
            html.contains(
                "<span style=\"text-decoration: line-through;\">-<a href=\"https://example.com/a\">https://example.com/a</a></span>."
            ),
            "open-run paragraph missing from {html}"
        );
        assert!(
            html.contains(
                "<span style=\"text-decoration: line-through;\"><a href=\"https://example.com/a\">https://example.com/a</a></span>-."
            ),
            "run-suffix paragraph missing from {html}"
        );
    }
}
