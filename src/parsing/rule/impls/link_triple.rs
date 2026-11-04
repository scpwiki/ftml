/*
 * parsing/rule/impls/link_triple.rs
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
use crate::data::PageRef;
use crate::parsing::collect::{
    collect_comment_elided_keep, collect_text_resolved_fused,
    complete_fused_bracket_close, consume_iftags_segment_fused, consume_valid_comment,
    iftags_segment_fused,
};
use crate::parsing::{discard_wikidot_controls, trim_wikidot_ascii_cow};
use crate::tree::{AnchorTarget, LinkLabel, LinkLocation, LinkType};
use crate::url::is_url;
use std::borrow::Cow;
use unicode_normalization::UnicodeNormalization;
use wikidot_normalize::normalize;

pub const RULE_LINK_TRIPLE: Rule = Rule {
    name: "link-triple",
    position: LineRequirement::Any,
    try_consume_fn: link,
};

pub const RULE_LINK_TRIPLE_NEW_TAB: Rule = Rule {
    name: "link-triple-new-tab",
    position: LineRequirement::Any,
    try_consume_fn: link_new_tab,
};

fn link<'r, 't>(parser: &mut Parser<'r, 't>) -> ParseResult<'r, 't, Elements<'t>> {
    let source_start = parser.current().span.start;
    assert_step(parser, Token::LeftLink)?;
    if parser.settings().layout.legacy()
        && parser.current().token == Token::Identifier
        && parser.current().slice.eq_ignore_ascii_case("span")
    {
        let mut span = parser.clone();
        if let Ok(mut parsed) = super::block::RULE_BLOCK.try_consume(&mut span) {
            let mut elements = vec![text!("[")];
            elements.extend(parsed.item);
            parsed.item = Elements::Multiple(elements);
            parser.update(&span);
            return Ok(parsed);
        }
    }
    if parser.settings().layout.legacy()
        && parser.current().token == Token::NumberedItem
        && parser.current().slice == "#"
    {
        if parser
            .look_ahead(0)
            .is_some_and(|token| token.token == Token::Whitespace)
        {
            let mut anchor = parser.clone();
            if let Ok(mut parsed) = super::anchor::try_consume_wikidot_triple_anchor(
                &mut anchor,
                source_start + 1,
            ) {
                let mut elements = vec![text!("[")];
                elements.extend(parsed.item);
                parsed.item = Elements::Multiple(elements);
                parser.update(&anchor);
                return Ok(parsed);
            }
        }
        let mut anchor = parser.clone();
        if let Ok(mut parsed) =
            super::link_anchor::try_consume_wikidot_bracket_anchor(&mut anchor)
        {
            let mut elements = vec![text!("[[")];
            elements.extend(parsed.item);
            parsed.item = Elements::Multiple(elements);
            parser.update(&anchor);
            return Ok(parsed);
        }
    }
    try_consume_link(parser, RULE_LINK_TRIPLE, None, source_start)
}

fn link_new_tab<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    let source_start = parser.current().span.start;
    assert_step(parser, Token::LeftLinkStar)?;
    try_consume_link(
        parser,
        RULE_LINK_TRIPLE_NEW_TAB,
        Some(AnchorTarget::NewTab),
        source_start,
    )
}

fn try_consume_link<'r, 't>(
    parser: &mut Parser<'r, 't>,
    rule: Rule,
    target: Option<AnchorTarget>,
    source_start: usize,
) -> ParseResult<'r, 't, Elements<'t>> {
    let url_close = [
        ParseCondition::current(Token::Pipe),
        ParseCondition::current(Token::RightLink),
    ];
    let url_invalid = [ParseCondition::current(Token::ParagraphBreak)];
    let native_url_invalid = [
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::current(Token::LineBreak),
    ];
    let url_invalid = if parser.settings().layout.legacy() {
        &url_invalid[..]
    } else {
        &native_url_invalid[..]
    };
    let (url, last, authored_url_prefix_is_url) = if parser.settings().layout.legacy() {
        let (url, last) =
            collect_comment_elided_keep(parser, &url_close, url_invalid, None)?;
        let authored_url_prefix_is_url =
            is_url(url.prefix_before_first_comment().trim_start());
        (
            discard_wikidot_controls(url.into_cow()),
            last,
            authored_url_prefix_is_url,
        )
    } else {
        let (url, last) = collect_text_resolved_fused(
            parser,
            rule,
            &url_close,
            url_invalid,
            None,
            false,
        )?;
        let authored = is_url(url.trim_start());
        (url, last, authored)
    };

    let leading_space = url.trim_start_matches([' ', '\t']).len() != url.len();
    let url = trim_wikidot_ascii_cow(url);

    if url.is_empty()
        && parser.settings().layout.legacy()
        && target.is_some()
        && last.token == Token::Pipe
    {
        return build_separate(
            parser,
            rule,
            Cow::Borrowed(""),
            false,
            false,
            target,
            source_start,
        );
    }
    if url.is_empty() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if parser.settings().layout.legacy()
        && target.is_none()
        && !leading_space
        && url.starts_with('#')
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    match last.token {
        Token::RightLink => build_same(
            parser,
            url,
            leading_space,
            authored_url_prefix_is_url,
            target,
        ),

        Token::Pipe => build_separate(
            parser,
            rule,
            url,
            leading_space,
            authored_url_prefix_is_url,
            target,
            source_start,
        ),

        _ => unreachable!(),
    }
}

fn build_same<'r, 't>(
    parser: &mut Parser<'r, 't>,
    url: Cow<'t, str>,
    leading_space: bool,
    authored_url_prefix_is_url: bool,
    target: Option<AnchorTarget>,
) -> ParseResult<'r, 't, Elements<'t>> {
    let label = match url.clone() {
        Cow::Borrowed(url) => Cow::Borrowed(same_link_label(parser, url)),
        Cow::Owned(url) => Cow::Owned(same_link_label(parser, &url).to_owned()),
    };
    let label = if parser.settings().layout.legacy() && url.starts_with('#') {
        Cow::Borrowed("")
    } else {
        label
    };
    let label = if parser.settings().layout.legacy()
        && target.is_some()
        && !is_url(&url)
        && strip_category(&url).is_none()
    {
        Cow::Owned(format!("*{label}"))
    } else {
        label
    };

    let parsed_link =
        parse_link_location(parser, url, leading_space, authored_url_prefix_is_url);
    let Some((link, ltype)) = parsed_link else {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    };

    let element = Element::Link {
        ltype,
        target: wikidot_target(parser, target, &link),
        link,
        label: LinkLabel::Slug(label),
    };

    success_elements(element)
}

fn same_link_label<'t>(parser: &Parser<'_, '_>, url: &'t str) -> &'t str {
    match strip_category(url) {
        Some(_)
            if parser.settings().layout.legacy()
                && url.starts_with(':')
                && (url.contains(" :") || url.contains(": ")) =>
        {
            url
        }
        Some(stripped) => stripped,
        None => url,
    }
}

fn build_separate<'r, 't>(
    parser: &mut Parser<'r, 't>,
    rule: Rule,
    url: Cow<'t, str>,
    leading_space: bool,
    authored_url_prefix_is_url: bool,
    target: Option<AnchorTarget>,
    source_start: usize,
) -> ParseResult<'r, 't, Elements<'t>> {
    let label_close = [ParseCondition::current(Token::RightLink)];
    let legacy = parser.settings().layout.legacy();
    let label_invalid = [ParseCondition::current(Token::ParagraphBreak)];
    let native_label_invalid = [
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::current(Token::LineBreak),
    ];
    let label_invalid = if legacy {
        &label_invalid[..]
    } else {
        &native_label_invalid[..]
    };
    let label = if legacy {
        match collect_wikidot_separate_label(parser, source_start)? {
            WikidotLabelCandidate::Label(label) => label,
            WikidotLabelCandidate::PreserveSource(source) => {
                return ok!(Element::Text(source));
            }
        }
    } else {
        if native_label_raw_forbids_link(parser)? {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
        let label = collect_text_resolved_fused(
            parser,
            rule,
            &label_close,
            label_invalid,
            None,
            false,
        )?
        .0;
        if label.contains("[!--") {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
        label
    };

    let label = if legacy {
        trim_wikidot_ascii_cow(discard_wikidot_controls(label))
    } else {
        match label {
            Cow::Borrowed(label) => Cow::Borrowed(label.trim()),
            Cow::Owned(label) => Cow::Owned(label.trim().to_owned()),
        }
    };

    let parsed_link = parse_link_location(
        parser,
        url.clone(),
        leading_space,
        authored_url_prefix_is_url,
    );
    let Some((link, ltype)) = parsed_link else {
        if legacy && (url.contains("###") || url.contains("/##/")) {
            return ok!(Element::Text(Cow::Owned(format!("[[[{url}|{label}]]]"))));
        }
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    };

    let label = if label.is_empty() && legacy {
        let mut normalized = str!(url.as_ref());
        if normalized.starts_with(':') {
            normalized.remove(0);
        }
        normalize(&mut normalized);
        match link {
            LinkLocation::Page(_) => LinkLabel::Title(Cow::Owned(normalized)),
            LinkLocation::Url(_) => LinkLabel::Text(Cow::Owned(normalized)),
        }
    } else if label.is_empty() {
        LinkLabel::Page
    } else {
        LinkLabel::Text(label)
    };

    let element = Element::Link {
        ltype,
        target: wikidot_target(parser, target, &link),
        link,
        label,
    };

    success_elements(element)
}

fn native_label_raw_forbids_link<'r, 't>(
    parser: &Parser<'r, 't>,
) -> Result<bool, ParseError>
where
    'r: 't,
{
    use super::raw::RULE_RAW;

    let mut probe = parser.clone();
    let mut no_raw_close = false;
    let mut no_alt_raw_close = false;

    loop {
        match probe.current().token {
            Token::RightLink => return Ok(false),

            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                return Ok(false);
            }

            Token::LeftBlock if iftags_segment_fused(&probe).is_some() => {
                consume_iftags_segment_fused(&mut probe)?;
            }

            Token::LeftComment => {
                let mut comment_probe = probe.clone();
                if consume_valid_comment(&mut comment_probe).is_err() {
                    return Ok(false);
                }
                probe.update(&comment_probe);
            }

            Token::Raw | Token::LeftRaw => {
                let alternate = probe.current().token == Token::LeftRaw;
                let known_unclosed = if alternate {
                    no_alt_raw_close
                } else {
                    no_raw_close
                };
                if known_unclosed {
                    probe.step()?;
                    continue;
                }

                let terminator = if alternate {
                    Token::RightRaw
                } else {
                    Token::Raw
                };
                let mut close_probe = probe.clone();
                close_probe.step()?;
                let mut has_close = false;
                loop {
                    match close_probe.current().token {
                        token if token == terminator => {
                            has_close = true;
                            break;
                        }
                        Token::RightLink
                        | Token::LineBreak
                        | Token::ParagraphBreak
                        | Token::InputEnd => break,
                        _ => {
                            close_probe.step()?;
                        }
                    }
                }
                if !has_close {
                    if alternate {
                        no_alt_raw_close = true;
                    } else {
                        no_raw_close = true;
                    }
                    probe.step()?;
                    continue;
                }

                let mut raw_probe = probe.clone();
                if RULE_RAW.try_consume(&mut raw_probe).is_ok() {
                    return Ok(true);
                }
                probe.step()?;
            }

            _ => {
                probe.step()?;
            }
        }
    }
}

fn collect_wikidot_separate_label<'r, 't>(
    parser: &mut Parser<'r, 't>,
    source_start: usize,
) -> Result<WikidotLabelCandidate<'t>, ParseError>
where
    'r: 't,
{
    use super::raw::RULE_RAW;

    const BRACKET_OWNERS: &[Token] = &[
        Token::LeftBracket,
        Token::LeftBracketAnchor,
        Token::LeftBracketStar,
        Token::RightBracket,
        Token::LeftBlock,
        Token::LeftBlockEnd,
        Token::LeftBlockAnchor,
        Token::LeftBlockStar,
        Token::LeftMath,
        Token::RightBlock,
        Token::RightMath,
        Token::LeftLink,
        Token::LeftLinkStar,
    ];

    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let mut segment_start = start;
    let mut visible_label = None::<String>;
    let mut preserve_source = false;
    let mut no_raw_close = false;
    let mut no_alt_raw_close = false;

    loop {
        match parser.current().token {
            Token::RightLink => {
                let end = parser.current().span.start;
                let source_end = parser.current().span.end;
                parser.step()?;
                if preserve_source {
                    return Ok(WikidotLabelCandidate::PreserveSource(Cow::Borrowed(
                        &source[source_start..source_end],
                    )));
                }
                return match visible_label {
                    Some(mut label) => {
                        if end >= segment_start {
                            label.push_str(&source[segment_start..end]);
                        }
                        Ok(WikidotLabelCandidate::Label(Cow::Owned(label)))
                    }
                    None => Ok(WikidotLabelCandidate::Label(Cow::Borrowed(
                        &source[start..end],
                    ))),
                };
            }

            _ if preserve_source => {
                parser.step()?;
            }

            Token::LeftComment => {
                let comment_start = parser.current().span.start;
                let mut comment_parser = parser.clone();
                if consume_valid_comment(&mut comment_parser).is_err() {
                    return Err(parser.make_err(ParseErrorKind::RuleFailed));
                }

                visible_label
                    .get_or_insert_with(String::new)
                    .push_str(&source[segment_start..comment_start]);
                segment_start = comment_parser.current().span.start;
                parser.update(&comment_parser);

                if parser.current().token == Token::RightBlock {
                    parser.step()?;
                    return Ok(WikidotLabelCandidate::Label(Cow::Owned(
                        visible_label.unwrap_or_default(),
                    )));
                }
            }

            Token::Raw | Token::LeftRaw => {
                let alternate = parser.current().token == Token::LeftRaw;
                let known_unclosed = if alternate {
                    no_alt_raw_close
                } else {
                    no_raw_close
                };
                if known_unclosed {
                    parser.step()?;
                    continue;
                }

                let terminator = if alternate {
                    Token::RightRaw
                } else {
                    Token::Raw
                };
                let mut close_probe = parser.clone();
                close_probe.step()?;
                let mut has_close = false;
                loop {
                    match close_probe.current().token {
                        token if token == terminator => {
                            has_close = true;
                            break;
                        }
                        Token::RightLink
                        | Token::LineBreak
                        | Token::ParagraphBreak
                        | Token::InputEnd => break,
                        _ => {
                            close_probe.step()?;
                        }
                    }
                }
                if !has_close {
                    if alternate {
                        no_alt_raw_close = true;
                    } else {
                        no_raw_close = true;
                    }
                    parser.step()?;
                    continue;
                }

                let mut raw_parser = parser.clone();
                if RULE_RAW.try_consume(&mut raw_parser).is_ok() {
                    return Err(parser.make_err(ParseErrorKind::RuleFailed));
                }
                parser.step()?;
            }

            Token::ParagraphBreak | Token::InputEnd => {
                let kind = if parser.current().token == Token::InputEnd {
                    ParseErrorKind::EndOfInput
                } else {
                    ParseErrorKind::RuleFailed
                };
                return Err(parser.make_err(kind));
            }

            Token::GeneratedPageLink | Token::GeneratedTagLinks | Token::RuntimeText => {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }

            Token::LeftBlock if iftags_segment_fused(parser).is_some() => {
                let segment = consume_iftags_segment_fused(parser)?
                    .expect("iftags segment presence was just checked");
                let label = visible_label.get_or_insert_with(String::new);
                label.push_str(&source[segment_start..segment.range.start]);
                if segment.include_body {
                    label.push_str(&source[segment.body.clone()]);
                }
                segment_start = segment.range.end;

                if segment.fused_closer {
                    match complete_fused_bracket_close(parser, "]]]") {
                        Some(last) => {
                            if preserve_source {
                                return Ok(WikidotLabelCandidate::PreserveSource(
                                    Cow::Borrowed(&source[source_start..last.span.end]),
                                ));
                            }
                            return Ok(WikidotLabelCandidate::Label(Cow::Owned(
                                visible_label.unwrap_or_default(),
                            )));
                        }
                        None if preserve_source => {
                            return Err(parser.make_err(ParseErrorKind::RuleFailed));
                        }
                        None => {
                            let only_brackets_left = std::iter::once(parser.current())
                                .chain(parser.remaining())
                                .all(|token| {
                                    matches!(
                                        token.token,
                                        Token::RightBracket
                                            | Token::RightBlock
                                            | Token::RightLink
                                            | Token::InputEnd
                                    )
                                });
                            if only_brackets_left {
                                return Ok(WikidotLabelCandidate::Label(Cow::Owned(
                                    visible_label.unwrap_or_default(),
                                )));
                            }
                        }
                    }
                }
            }

            Token::LeftBlock => {
                if wikidot_label_block_executes(parser) {
                    return Err(parser.make_err(ParseErrorKind::RuleFailed));
                }
                preserve_source = true;
                parser.step()?;
            }

            token if BRACKET_OWNERS.contains(&token) => {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }

            _ => {
                parser.step()?;
            }
        }
    }
}

enum WikidotLabelCandidate<'t> {
    Label(Cow<'t, str>),
    PreserveSource(Cow<'t, str>),
}

fn wikidot_label_block_executes<'r, 't>(parser: &Parser<'r, 't>) -> bool
where
    'r: 't,
{
    let mut block = parser.clone();
    let Ok((name, _, segment)) = block.get_block_name(false) else {
        return false;
    };

    ["span", "size", "image", "footnote", "iframe"]
        .iter()
        .any(|owner| name.eq_ignore_ascii_case(owner))
        && block.head_word_is_actionable(&name, matches!(name, Cow::Borrowed(_)), segment)
}

fn wikidot_target(
    parser: &Parser<'_, '_>,
    target: Option<AnchorTarget>,
    link: &LinkLocation<'_>,
) -> Option<AnchorTarget> {
    match (parser.settings().layout.legacy(), link) {
        (true, LinkLocation::Url(_)) | (false, _) => target,
        (true, LinkLocation::Page(_)) => None,
    }
}

fn transliterate_triple_target(url: &str) -> Option<String> {
    let mut transliterated = String::with_capacity(url.len());

    for character in url.nfkd() {
        if is_format_character(character) {
            transliterated.push(' ');
        } else if character.is_ascii() {
            transliterated.push(character);
        } else if is_transliteration_combining_mark(character) {
            continue;
        } else {
            let latin = transliterate_cyrillic(character)?;
            transliterated.push_str(latin);
        }
    }

    Some(transliterated)
}

fn transliterate_triple_anchor(anchor: &str) -> String {
    let mut transliterated = String::with_capacity(anchor.len());

    for character in anchor.chars() {
        if is_format_character(character) {
            transliterated.push(character);
            continue;
        }

        if character.is_ascii() {
            transliterated.push(character);
            continue;
        }

        match transliterate_triple_target(character.encode_utf8(&mut [0; 4])) {
            Some(transliteration) => transliterated.push_str(&transliteration),
            None => transliterated.push(character),
        }
    }

    transliterated
}

fn is_format_character(character: char) -> bool {
    matches!(
        character,
        '\u{ad}'
            | '\u{600}'..='\u{605}'
            | '\u{61c}'
            | '\u{6dd}'
            | '\u{70f}'
            | '\u{8e2}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}'
            | '\u{fff9}'..='\u{fffb}'
            | '\u{110bd}'
            | '\u{110cd}'
            | '\u{13430}'..='\u{1343f}'
            | '\u{1bca0}'..='\u{1bca3}'
            | '\u{1d173}'..='\u{1d17a}'
            | '\u{e0001}'
            | '\u{e0020}'..='\u{e007f}'
    )
}

fn is_transliteration_combining_mark(character: char) -> bool {
    matches!(
        character,
        '\u{300}'..='\u{36f}'
            | '\u{483}'..='\u{489}'
            | '\u{1ab0}'..='\u{1aff}'
            | '\u{1dc0}'..='\u{1dff}'
            | '\u{20d0}'..='\u{20ff}'
            | '\u{fe20}'..='\u{fe2f}'
    )
}

fn transliterate_cyrillic(character: char) -> Option<&'static str> {
    match character {
        'а' | 'А' => Some("a"),
        'б' | 'Б' => Some("b"),
        'в' | 'В' => Some("v"),
        'г' | 'Г' => Some("g"),
        'д' | 'Д' => Some("d"),
        'е' | 'Е' => Some("e"),
        'ж' | 'Ж' => Some("zh"),
        'з' | 'З' => Some("z"),
        'и' | 'И' => Some("i"),
        'к' | 'К' => Some("k"),
        'л' | 'Л' => Some("l"),
        'м' | 'М' => Some("m"),
        'н' | 'Н' => Some("n"),
        'о' | 'О' => Some("o"),
        'п' | 'П' => Some("p"),
        'р' | 'Р' => Some("r"),
        'с' | 'С' => Some("s"),
        'т' | 'Т' => Some("t"),
        'у' | 'У' => Some("u"),
        'ф' | 'Ф' => Some("f"),
        'х' | 'Х' => Some("kh"),
        'ц' | 'Ц' => Some("ts"),
        'ч' | 'Ч' => Some("ch"),
        'ш' | 'Ш' => Some("sh"),
        'щ' | 'Щ' => Some("shch"),
        'ъ' | 'Ъ' | 'ь' | 'Ь' => Some(""),
        'ы' | 'Ы' => Some("y"),
        'э' | 'Э' => Some("e"),
        'ю' | 'Ю' => Some("yu"),
        'я' | 'Я' => Some("ya"),
        _ => None,
    }
}

fn parse_link_location<'r, 't>(
    parser: &Parser<'r, 't>,
    mut url: Cow<'t, str>,
    leading_space: bool,
    authored_url_prefix_is_url: bool,
) -> Option<(LinkLocation<'t>, LinkType)> {
    if !parser.settings().layout.legacy() {
        return LinkLocation::parse_with_interwiki(url, parser.settings());
    }

    if url.contains("###") || url.contains("/##/") {
        return None;
    }

    if url.is_empty() {
        return Some((LinkLocation::Page(PageRef::page_only("")), LinkType::Page));
    }

    if matches!(url.as_ref(), "." | "..") {
        return Some((LinkLocation::Page(PageRef::page_only("")), LinkType::Page));
    }

    if let Some(interwiki) = url.strip_prefix('!') {
        if let Some(expanded) = parser.settings().interwiki.build(interwiki) {
            return Some((LinkLocation::Url(Cow::Owned(expanded)), LinkType::Interwiki));
        }
        let local = interwiki.strip_prefix(':').unwrap_or(interwiki);
        return Some((
            LinkLocation::Page(PageRef::page_only(local)),
            LinkType::Page,
        ));
    }

    if let Some(cross_site) = url.strip_prefix(':') {
        let mut page = cross_site
            .split(':')
            .map(|part| {
                let mut part = part.trim().to_owned();
                normalize(&mut part);
                part
            })
            .collect::<Vec<_>>()
            .join(":");
        if page.is_empty() {
            return Some((LinkLocation::Page(PageRef::page_only("")), LinkType::Page));
        }
        page.make_ascii_lowercase();
        return Some((
            LinkLocation::Page(PageRef {
                site: None,
                page,
                extra: None,
            }),
            LinkType::Page,
        ));
    }

    if leading_space && is_url(&url) && authored_url_prefix_is_url {
        let page = url.replace("://", ":");
        let page = page.trim_end_matches('/');
        return Some((LinkLocation::Page(PageRef::page_only(page)), LinkType::Page));
    }

    if let Some((page, extra)) = url.split_once("/#/") {
        let mut page_ref = PageRef::page_only(page);
        page_ref.extra = Some(format!("#/{extra}"));
        return Some((LinkLocation::Page(page_ref), LinkType::Page));
    }

    if url.contains('/')
        && !url.starts_with('/')
        && (!is_url(&url) || !authored_url_prefix_is_url)
    {
        return Some((
            LinkLocation::Page(PageRef::page_only(url.replace('/', "-"))),
            LinkType::Page,
        ));
    }

    if !url.is_ascii() && !is_url(&url) {
        let (page_part, anchor_part) = match url.split_once('#') {
            Some((page, anchor)) => (page, Some(anchor)),
            None => (url.as_ref(), None),
        };
        match transliterate_triple_target(page_part) {
            Some(transliterated)
                if !transliterated.is_empty() || page_part.is_empty() =>
            {
                url = Cow::Owned(match anchor_part {
                    Some(anchor) => format!(
                        "{transliterated}#{}",
                        transliterate_triple_anchor(anchor),
                    ),
                    None => transliterated,
                });
            }
            _ => {
                return Some((
                    LinkLocation::Page(PageRef::page_only("")),
                    LinkType::Page,
                ));
            }
        }
    }

    if is_url(&url) && !authored_url_prefix_is_url {
        return Some((LinkLocation::Page(PageRef::page_only(&url)), LinkType::Page));
    }

    if url.starts_with('#') {
        return Some((LinkLocation::Page(PageRef::page_only(&url)), LinkType::Page));
    }

    LinkLocation::parse_with_interwiki(url, parser.settings())
}

fn strip_category(url: &str) -> Option<&str> {
    if is_url(url) {
        return None;
    }
    match url.find(':') {
        Some(0) => {
            let url = &url[1..];

            url.find(':').map(|idx| {
                let url = url[idx + 1..].trim_start();

                strip_category(url).unwrap_or(url)
            })
        }

        Some(idx) => Some(url[idx + 1..].trim_start()),

        None => None,
    }
}

#[test]
fn test_strip_category() {
    macro_rules! test {
        ($input:expr, $expected:expr $(,)?) => {{
            let actual = strip_category($input);

            assert_eq!(
                actual, $expected,
                "Actual stripped URL label doesn't match expected",
            );
        }};
    }

    test!("", None);
    test!("scp-001", None);
    test!("Guide Hub", None);
    test!("theme:just-girly-things", Some("just-girly-things"));
    test!("theme: just-girly-things", Some("just-girly-things"));
    test!("theme: Just Girly Things", Some("Just Girly Things"));
    test!("component:fancy-sidebar", Some("fancy-sidebar"));
    test!("component:Fancy Sidebar", Some("Fancy Sidebar"));
    test!("component: Fancy Sidebar", Some("Fancy Sidebar"));
    test!(
        "multiple:categories:here:test",
        Some("categories:here:test"),
    );
    test!(
        "multiple: categories: here: test",
        Some("categories: here: test"),
    );
    test!(":scp-wiki:scp-001", Some("scp-001"));
    test!(":scp-wiki : SCP-001", Some("SCP-001"));
    test!(":scp-wiki:system:recent-changes", Some("recent-changes"));
    test!(
        ":scp-wiki : system : Recent Changes",
        Some("Recent Changes"),
    );
    test!(": snippets : redirect", Some("redirect"));
    test!(":", None);
}

#[test]
fn test_transliterate_triple_target() {
    macro_rules! test {
        ($input:expr, $expected:expr $(,)?) => {{
            let actual = transliterate_triple_target($input);
            let expected: Option<String> = $expected.map(str::to_owned);

            assert_eq!(
                actual, expected,
                "Transliterated triple-link target doesn't match expected",
            );
        }};
    }

    test!("документ", Some("dokument"));
    test!("café", Some("cafe"));

    test!("ДОКУМЕНТ", Some("dokument"));
    test!("CAFÉ", Some("CAFE"));

    test!("й", Some("i"));
    test!("ё", Some("e"));

    test!("日本語", None);
    test!("документ日本語", None);
    test!("ß", None);

    test!("pa\u{200b}ge", Some("pa ge"));
    test!("\u{200b}page", Some(" page"));
    test!("page\u{200b}", Some("page "));
    test!("pa\u{200c}ge", Some("pa ge"));
    test!("pa\u{feff}ge", Some("pa ge"));
    test!("pa\u{ad}ge", Some("pa ge"));
    test!("pa\u{2060}ge", Some("pa ge"));
}

#[test]
fn test_transliterate_triple_anchor() {
    macro_rules! test {
        ($input:expr, $expected:expr $(,)?) => {
            assert_eq!(
                transliterate_triple_anchor($input),
                $expected,
                "Transliterated triple-link anchor doesn't match expected",
            )
        };
    }

    test!("café", "cafe");
    test!("документ", "dokument");
    test!("Section Title", "Section Title");

    test!("日本語セクション", "日本語セクション");
    test!("Section日本", "Section日本");
    test!("ß", "ß");
    test!("frag\u{200b}ment", "frag\u{200b}ment");
}

#[cfg(test)]
mod wikidot_tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::html::HtmlRender;
    use crate::render::{PageExistenceResolver, Render};
    use crate::settings::{WikitextMode, WikitextSettings};

    fn render(source: &str) -> String {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        HtmlRender.render(&tree, &page_info, &settings).body
    }

    #[test]
    fn star_is_label_syntax_not_new_tab_in_wikidot() {
        let html = render("[[[*SCP-001]]] [[[*some-page|Label]]]");
        assert!(html.contains(r#"href="/scp-001">*SCP-001</a>"#), "{html}");
        assert!(html.contains(r#"href="/some-page">Label</a>"#), "{html}");
        assert!(!html.contains("target="), "{html}");
    }

    #[test]
    fn wikidot_star_external_triple_links_open_a_new_tab() {
        let html = render(concat!(
            "[[[*http://ja.scp-wiki.net/scp-040-jp |ねこでした。]]] ",
            "[[[http://ja.scp-wiki.net/scp-040-jp |ordinary]]]",
        ));

        assert!(
            html.contains(
                r#"<a href="http://ja.scp-wiki.net/scp-040-jp" target="_blank">ねこでした。</a>"#,
            ),
            "{html}",
        );
        assert!(
            html.contains(r#"<a href="http://ja.scp-wiki.net/scp-040-jp">ordinary</a>"#,),
            "{html}",
        );
    }

    #[test]
    fn wikidot_unlabeled_external_triple_links_keep_the_scheme_in_the_label() {
        let html = render(concat!(
            "[[[http://sandbox-for-codex.wikidot.com/example]]] ",
            "[[[*http://sandbox-for-codex.wikidot.com/new-tab]]]",
        ));

        assert!(
            html.contains(concat!(
                r#"<a href="http://sandbox-for-codex.wikidot.com/example">"#,
                "http://sandbox-for-codex.wikidot.com/example</a>",
            )),
            "{html}",
        );
        assert!(
            html.contains(concat!(
                r#"<a href="http://sandbox-for-codex.wikidot.com/new-tab" target="_blank">"#,
                "http://sandbox-for-codex.wikidot.com/new-tab</a>",
            )),
            "{html}",
        );
    }

    #[test]
    fn empty_label_uses_normalized_slug_in_wikidot() {
        let html = render("[[[some-page|]]] [[[:scp-wiki:scp-series|]]]");
        assert!(html.contains(">some-page</a>"), "{html}");
        assert!(html.contains(">scp-wiki:scp-series</a>"), "{html}");
        assert!(!html.contains("<page-title"), "{html}");
    }

    #[test]
    fn wikidot_cross_site_and_subpath_forms_are_local_slugs() {
        let html = render(concat!(
            "[[[:scp-wiki:component:theme|Sigma-9 Theme]]] ",
            "[[[:scp-wiki : system : Recent Changes]]] ",
            "[[[MAIN/#/page|Hash routing]]] ",
            "[[[example/edit|Edit page]]]",
        ));
        assert!(
            html.contains(r#"href="/scp-wiki:component:theme">Sigma-9 Theme</a>"#),
            "{html}",
        );
        assert!(
            html.contains(
                r#"href="/scp-wiki:system:recent-changes">:scp-wiki : system : Recent Changes</a>"#
            ),
            "{html}",
        );
        assert!(
            html.contains(r#"href="/main#/page">Hash routing</a>"#),
            "{html}"
        );
        assert!(
            html.contains(r#"href="/example-edit">Edit page</a>"#),
            "{html}"
        );
    }

    #[test]
    fn wikidot_rejects_malformed_hash_routes_and_treats_spaced_url_as_slug() {
        let html = render(concat!(
            "[[[home###|Home]]] ",
            "[[[MAIN/##/page#toc1|Hash routing]]] ",
            "[[[ https://example.com/ | Example ]]]",
        ));
        assert!(html.contains("[[[home###|Home]]]"), "{html}");
        assert!(
            html.contains("[[[MAIN/##/page#toc1|Hash routing]]]"),
            "{html}"
        );
        assert!(
            html.contains(r#"href="/https:example-com">Example</a>"#),
            "{html}"
        );
    }

    #[test]
    fn wikidot_only_treats_leading_space_before_url_as_slug() {
        let html = render(concat!(
            "[[[https://example.com | Trailing]]] ",
            "[[[ https://example.com|Leading]]] ",
            "[[[ https://example.com | Both]]]",
        ));
        assert!(
            html.contains(r#"href="https://example.com">Trailing</a>"#),
            "{html}"
        );
        assert!(
            html.contains(r#"href="/https:example-com">Leading</a>"#),
            "{html}"
        );
        assert!(
            html.contains(r#"href="/https:example-com">Both</a>"#),
            "{html}"
        );
    }

    #[test]
    fn wikidot_accepts_line_break_before_triple_link_label() {
        let html = render("[[[some-page |\nLabel]]]");
        assert!(html.contains(r#"href="/some-page">Label</a>"#), "{html}");
    }

    #[test]
    fn wikidot_star_with_empty_destination_links_to_root() {
        let html = render("[[[|some-page]]] [[[*|some-page]]]");
        assert!(html.contains("[[[|some-page]]]"), "{html}");
        assert!(
            html.contains(r#"class="newpage" href="/">some-page</a>"#),
            "{html}"
        );
    }

    struct CallerExistence(bool);

    impl PageExistenceResolver for CallerExistence {
        fn page_exists(&self, _site: &str, _page: &str) -> bool {
            self.0
        }
    }

    fn render_with_existence(source: &str, exists: bool) -> String {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        HtmlRender
            .render_with_page_existence(
                &tree,
                &page_info,
                &settings,
                &CallerExistence(exists),
            )
            .body
    }

    #[test]
    fn wikidot_triple_links_carry_newpage_only_when_caller_marks_target_missing() {
        for source in [
            "[[[!banana:Missing provider]]]",
            "[[[!:missing|Label]]]",
            "[[[scp-055|object]]]",
        ] {
            let missing = render_with_existence(source, false);
            assert!(
                missing.contains(r#"class="newpage""#),
                "{source}: {missing}"
            );
            let present = render_with_existence(source, true);
            assert!(!present.contains("newpage"), "{source}: {present}");
        }

        assert_eq!(
            render_with_existence("[[[!banana:Missing provider]]]", false),
            r#"<p><a class="newpage" href="/banana:missing-provider">Missing provider</a></p>"#,
        );
        assert_eq!(
            render_with_existence("[[[!banana:Missing provider]]]", true),
            r#"<p><a href="/banana:missing-provider">Missing provider</a></p>"#,
        );
        assert_eq!(
            render_with_existence("[[[!:missing|Label]]]", false),
            r#"<p><a class="newpage" href="/missing">Label</a></p>"#,
        );
        assert_eq!(
            render_with_existence("[[[scp-055|object]]]", false),
            r#"<p><a class="newpage" href="/scp-055">object</a></p>"#,
        );
    }

    #[test]
    fn wikidot_accepts_line_break_before_triple_link_closer() {
        let html = render("[[[some-page\n]]]some-page");
        assert!(
            html.contains(r#"href="/some-page">some-page</a>some-page"#),
            "{html}"
        );
    }
}
