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

use super::link_triple_transliteration::transliterate_character;
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
        // The URL walk only stops early at `|`, `]]]`, a paragraph
        // break, a valid comment, or a fused segment; the last two need a
        // later `]`. With none of those left, the walk is certain to reach
        // `InputEnd` and fail with `EndOfInput`, so reproduce that error
        // instead of walking (a long unclosed `[` run used to walk the
        // rest of the document once per triple-link opener).
        if parser.link_target_reaches_input_end(parser.current().span.start, true) {
            return Err(parser.make_end_of_input_err());
        }
        let (url, last) =
            collect_comment_elided_keep(parser, &url_close, url_invalid, None, None)?;
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
    let label = if parser.settings().layout.legacy() && target.is_some() && !is_url(&url)
    {
        match strip_category(&url) {
            Some(stripped) => Cow::Owned(stripped.to_owned()),
            None => Cow::Owned(format!("*{label}")),
        }
    } else {
        label
    };

    let parsed_link =
        parse_link_location(parser, url, leading_space, authored_url_prefix_is_url);
    let Some((link, ltype)) = parsed_link else {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    };

    let label =
        if parser.settings().layout.legacy() && matches!(link, LinkLocation::Page(_)) {
            Cow::Owned(
                collapse_whitespace_runs(&label)
                    .trim_matches(' ')
                    .to_owned(),
            )
        } else {
            label
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
    if !parser.settings().layout.legacy() {
        return match strip_category_native(url) {
            Some(stripped) => stripped,
            None => url,
        };
    }

    let base = if url.starts_with(':') || url.starts_with('/') {
        url
    } else {
        match strip_category(url) {
            Some(stripped) => stripped,
            None => url,
        }
    };
    strip_fragment(base)
}

fn strip_fragment(target: &str) -> &str {
    match target.split_once('#') {
        Some((page, _)) => page,
        None => target,
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
        let page = strip_fragment(url.as_ref());
        let transliterated = transliterate_triple_target(page);
        let normalized = normalize_triple_slug(&transliterated);
        match link {
            LinkLocation::Page(_) => LinkLabel::Title(Cow::Owned(normalized)),
            LinkLocation::Url(_) => LinkLabel::Text(Cow::Owned(normalized)),
        }
    } else if label.is_empty() {
        LinkLabel::Page
    } else if legacy && matches!(link, LinkLocation::Page(_)) {
        LinkLabel::Text(Cow::Owned(
            collapse_whitespace_runs(&label)
                .trim_matches(' ')
                .to_owned(),
        ))
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

fn transliterate_triple_target(url: &str) -> String {
    let mut transliterated = String::with_capacity(url.len());

    for character in url.chars() {
        if character.is_ascii() {
            transliterated.push(character);
        } else if let Some(latin) = transliterate_character(character) {
            transliterated.push_str(latin);
        } else {
            transliterated.push(' ');
        }
    }

    transliterated
}

/// Normalizes a transliterated triple-link slug the way live does. With at
/// most one category separator the full `wikidot_normalize::normalize`
/// pipeline already matches live; multi-category targets follow their own
/// derived rule (`normalize_triple_slug_multi`).
fn normalize_triple_slug(text: &str) -> String {
    if text.matches(':').count() > 1 {
        return normalize_triple_slug_multi(text);
    }

    // Live keeps a trailing `:_` (an underscore directly after a category
    // colon, as in `fragment:_template`) and drops every other trailing run
    // of `:`/`_` in a colon-bearing target.
    let mut text = if text.contains(':') {
        let keep = match text
            .char_indices()
            .rfind(|&(_, character)| character != ':' && character != '_')
        {
            Some((index, character)) => index + character.len_utf8(),
            None => 0,
        };
        let tail = &text[keep..];
        if tail.is_empty() || tail == ":_" {
            text.to_owned()
        } else {
            text[..keep].to_owned()
        }
    } else {
        text.to_owned()
    };
    text = collapse_colon_runs(&text);
    strip_default_category(&mut text);
    normalize(&mut text);
    text
}

fn normalize_triple_slug_multi(text: &str) -> String {
    let mut folded = slug_fold(slug_trailing_trim(text));
    if folded.len() >= 9 && folded[..9].eq_ignore_ascii_case("_default:") {
        folded = folded[9..].to_owned();
    }
    let mut folded = slug_collapse_start(&folded);
    if let Some((category, name)) = folded.split_once(':') {
        folded = format!("{category}:{}", slug_collapse_start(name));
    }
    if !folded.bytes().any(|byte| byte.is_ascii_alphanumeric()) {
        return String::new();
    }
    folded
}

fn slug_trailing_trim(raw: &str) -> &str {
    let mut end = raw.len();
    for (index, character) in raw.char_indices().rev() {
        if matches!(character, ':' | '_' | '-') {
            end = index;
        } else {
            break;
        }
    }
    let run = &raw[end..];
    if run.is_empty() || raw == "_" {
        return raw;
    }
    if run == ":_" {
        if let Some(first) = raw.find(":_") {
            if first == end {
                return raw;
            }
            let after = raw[first + 2..].chars().next();
            if after.is_some_and(|character| character.is_alphanumeric()) {
                return raw;
            }
        }
        return &raw[..end];
    }
    &raw[..end]
}

fn slug_fold(text: &str) -> String {
    let lowered = text.trim().to_lowercase();
    let mut folded: String = lowered
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | ':' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect();
    folded = collapse_char_runs(&folded, ':');
    folded = collapse_char_runs(&folded, '-');
    folded = replace_slug_pairs(&folded, [(':', '-'), ('-', ':')], ':');
    folded = replace_slug_pairs(&folded, [('_', '-'), ('-', '_')], '_');
    if let Some(rest) = folded.strip_prefix('_') {
        folded = format!(":_{rest}");
    }
    let mut underscored = String::with_capacity(folded.len());
    let mut previous = None;
    for character in folded.chars() {
        if character == '_' && previous != Some(':') {
            underscored.push('-');
        } else {
            underscored.push(character);
        }
        previous = Some(character);
    }
    folded = underscored.trim_matches('-').to_owned();
    folded = collapse_char_runs(&folded, '-');
    folded = collapse_char_runs(&folded, ':');
    folded = replace_slug_pairs(&folded, [(':', '-'), ('-', ':')], ':');
    folded = replace_slug_pairs(&folded, [('_', '-'), ('-', '_')], '_');
    folded.trim_matches(':').to_owned()
}

fn slug_collapse_start(text: &str) -> String {
    let run_len: usize = text
        .chars()
        .take_while(|character| matches!(character, ':' | '_' | '-'))
        .map(char::len_utf8)
        .sum();
    if run_len == 0 {
        return text.to_owned();
    }
    let (run, rest) = text.split_at(run_len);
    if run.contains('_') {
        format!("_{rest}")
    } else {
        text.to_owned()
    }
}

fn collapse_char_runs(text: &str, needle: char) -> String {
    let mut collapsed = String::with_capacity(text.len());
    let mut previous = None;
    for character in text.chars() {
        if character == needle && previous == Some(needle) {
            continue;
        }
        previous = Some(character);
        collapsed.push(character);
    }
    collapsed
}

fn replace_slug_pairs(text: &str, pairs: [(char, char); 2], replacement: char) -> String {
    let mut text = text.to_owned();
    loop {
        let chars: Vec<char> = text.chars().collect();
        let hit = chars
            .windows(2)
            .position(|window| pairs.contains(&(window[0], window[1])));
        match hit {
            Some(index) => {
                let mut next: String = chars[..index].iter().collect();
                next.push(replacement);
                next.extend(chars[index + 2..].iter());
                text = next;
            }
            None => return text,
        }
    }
}

fn collapse_whitespace_runs(text: &str) -> String {
    let mut collapsed = String::with_capacity(text.len());
    let mut in_whitespace = false;
    for character in text.chars() {
        if matches!(character, ' ' | '\t') {
            if !in_whitespace {
                collapsed.push(' ');
                in_whitespace = true;
            }
        } else {
            collapsed.push(character);
            in_whitespace = false;
        }
    }
    collapsed
}

/// Builds the local page reference for a transliterated triple-link target.
/// Leading colons are dropped (`::a` becomes `a`) and the `#` fragment stays
/// raw for later percent-encoding; it is never transliterated. Spaces in the
/// fragment are percent-encoded up front, matching live triple-link hrefs.
fn triple_page_ref(target: &str) -> PageRef {
    let target = target.trim_start_matches(':');
    let (page, extra) = match target.split_once('#') {
        Some((page, anchor)) => (
            page,
            Some(format!(
                "#{}",
                collapse_whitespace_runs(anchor).replace(' ', "%20")
            )),
        ),
        None => (target, None),
    };
    PageRef {
        site: None,
        page: normalize_triple_slug(page),
        extra,
    }
}

/// Turns `/` in the page portion into `-`, leaving the `#` fragment raw:
/// live triple-link fragments keep every character and are percent-encoded
/// later, so `a#b/c` is `/a#b/c` while `a/b#c` is `/a-b#c`.
fn replace_page_slashes(target: &str) -> String {
    match target.split_once('#') {
        Some((page, anchor)) => format!("{}#{anchor}", page.replace('/', "-")),
        None => target.replace('/', "-"),
    }
}

fn collapse_colon_runs(text: &str) -> String {
    if !text.contains("::") {
        return str!(text);
    }

    let mut collapsed = String::with_capacity(text.len());
    let mut prev_colon = false;
    for character in text.chars() {
        if character == ':' {
            if prev_colon {
                continue;
            }
            prev_colon = true;
            collapsed.push(':');
        } else {
            prev_colon = false;
            collapsed.push(character);
        }
    }
    collapsed
}

fn strip_default_category(text: &mut String) {
    const PREFIX: &str = "_default:";
    if text
        .get(..PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(PREFIX))
    {
        text.replace_range(..PREFIX.len(), "");
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

    if url.matches('#').count() >= 2 {
        return None;
    }

    if url.is_empty() {
        return Some((LinkLocation::Page(PageRef::page_only("")), LinkType::Page));
    }

    if matches!(url.as_ref(), "." | "..") {
        return Some((LinkLocation::Page(PageRef::page_only("")), LinkType::Page));
    }

    if let Some(interwiki) = url.strip_prefix('!') {
        let local = interwiki.strip_prefix(':').unwrap_or(interwiki);
        let (page_part, anchor_part) = match local.split_once('#') {
            Some((page, anchor)) => (page, Some(anchor)),
            None => (local, None),
        };
        let mut target = transliterate_triple_target(page_part);
        if let Some(anchor) = anchor_part {
            target.push('#');
            target.push_str(anchor);
        }
        return Some((
            LinkLocation::Page(triple_page_ref(&replace_page_slashes(&target))),
            LinkType::Page,
        ));
    }

    if leading_space && is_url(&url) && authored_url_prefix_is_url {
        let page = url.replace("://", ":");
        let page = page.trim_end_matches('/');
        return Some((LinkLocation::Page(PageRef::page_only(page)), LinkType::Page));
    }

    if !url.is_ascii() {
        let trimmed = url.trim_start();
        if !is_url(trimmed) && !trimmed.starts_with('/') {
            let (page_part, anchor_part) = match url.split_once('#') {
                Some((page, anchor)) => (page.to_owned(), Some(anchor.to_owned())),
                None => (url.as_ref().to_owned(), None),
            };
            let mut transliterated = transliterate_triple_target(&page_part);
            if let Some(anchor) = anchor_part {
                transliterated.push('#');
                transliterated.push_str(&anchor);
            }
            url = Cow::Owned(transliterated);
        }
    }

    if url.starts_with(':') {
        return Some((
            LinkLocation::Page(triple_page_ref(url.as_ref())),
            LinkType::Page,
        ));
    }

    if let Some((page, extra)) = url.split_once("/#/") {
        let mut page_ref = triple_page_ref(page);
        page_ref.extra = Some(format!("#/{extra}"));
        return Some((LinkLocation::Page(page_ref), LinkType::Page));
    }

    if url.contains('/')
        && !url.starts_with('/')
        && (!is_url(&url) || !authored_url_prefix_is_url)
    {
        return Some((
            LinkLocation::Page(triple_page_ref(&replace_page_slashes(&url))),
            LinkType::Page,
        ));
    }

    if is_url(&url) && !authored_url_prefix_is_url {
        return Some((LinkLocation::Page(PageRef::page_only(&url)), LinkType::Page));
    }

    if url.starts_with('#') {
        return Some((
            LinkLocation::Page(triple_page_ref(url.as_ref())),
            LinkType::Page,
        ));
    }

    if url.starts_with('/') || is_url(&url) {
        if url.contains(' ') {
            url = Cow::Owned(url.replace(' ', "%20"));
        }
        return LinkLocation::parse_with_interwiki(url, parser.settings());
    }

    Some((
        LinkLocation::Page(triple_page_ref(url.as_ref())),
        LinkType::Page,
    ))
}

fn strip_category(url: &str) -> Option<&str> {
    if is_url(url) {
        return None;
    }
    let page = strip_fragment(url);
    page.find(':').map(|idx| page[idx + 1..].trim_start())
}

fn strip_category_native(url: &str) -> Option<&str> {
    if is_url(url) {
        return None;
    }
    match url.find(':') {
        Some(0) => {
            let url = &url[1..];

            url.find(':').map(|idx| {
                let url = url[idx + 1..].trim_start();

                strip_category_native(url).unwrap_or(url)
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
    test!(":scp-wiki:scp-001", Some("scp-wiki:scp-001"));
    test!(":scp-wiki : SCP-001", Some("scp-wiki : SCP-001"));
    test!(
        ":scp-wiki:system:recent-changes",
        Some("scp-wiki:system:recent-changes"),
    );
    test!(
        ":scp-wiki : system : Recent Changes",
        Some("scp-wiki : system : Recent Changes"),
    );
    test!(": snippets : redirect", Some("snippets : redirect"));
    test!(":", Some(""));
    test!("a:b#c", Some("b"));
    test!("a#b:c", None);
}

#[test]
fn test_transliterate_triple_target() {
    macro_rules! test {
        ($input:expr, $expected:expr $(,)?) => {{
            let actual = transliterate_triple_target($input);

            assert_eq!(
                actual, $expected,
                "Transliterated triple-link target doesn't match expected",
            );
        }};
    }

    test!("документ", "dokument");
    test!("café", "cafe");

    test!("ДОКУМЕНТ", "dokument");
    test!("CAFÉ", "CAFe");

    test!("й", "j");
    test!("ё", "e");
    test!("Ä", "ae");
    test!("ß", "ss");
    test!("Σ", "s");
    test!("я", "a");

    test!("日本語", "   ");
    test!("документ日本語", "dokument   ");
    test!("ａ", " ");
    test!("ﬁ", " ");

    test!("pa\u{200b}ge", "pa ge");
    test!("\u{200b}page", " page");
    test!("page\u{200b}", "page ");
    test!("pa\u{200c}ge", "pa ge");
    test!("pa\u{feff}ge", "pa ge");
    test!("pa\u{ad}ge", "pa ge");
    test!("pa\u{2060}ge", "pa ge");
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
