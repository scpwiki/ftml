/*
 * parsing/rule/impls/link_single.rs
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

use super::RULE_COMMENT;
use super::math::wikidot_math_candidate_is_complete;
use super::prelude::*;
use crate::delayed::{DelayedElement, GeneratedKind};
use crate::parsing::collect::{
    collect_comment_elided_keep, collect_comment_elided_keep_from,
    collect_text_resolved_fused, consume_iftags_segment_fused, consume_valid_comment,
    iftags_segment_fused,
};
use crate::parsing::unmatched_close_scan::{LinkLabelScanMemo, LinkTargetScanMemo};
use crate::parsing::{ParseSuccess, discard_wikidot_controls, trim_wikidot_ascii_cow};
use crate::tree::{AnchorTarget, LinkLabel, LinkLocation, LinkType};
use crate::url::is_url;
use std::borrow::Cow;

const MAX_WIKIDOT_SINGLE_LINK_TARGET_BYTES: usize = 64 * 1024;

fn trim_cow(value: Cow<'_, str>) -> Cow<'_, str> {
    match value {
        Cow::Borrowed(value) => Cow::Borrowed(value.trim()),
        Cow::Owned(value) => Cow::Owned(value.trim().to_owned()),
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum WikidotLinkDisposition {
    Link,
    LabelOnly,
    Literal,
}

#[derive(Debug)]
struct WikidotLabel<'t> {
    text: Cow<'t, str>,
    residual_closer: &'t str,
}

#[derive(Debug, Copy, Clone, Default)]
enum WikidotBibciteOwnerScan {
    #[default]
    Search,
    Name,
    Separator,
    Label {
        nonempty: bool,
    },
}

#[derive(Debug, Copy, Clone, Default)]
enum WikidotNamedAnchorOwnerScan {
    #[default]
    Search,
    Separator,
    Name {
        nonempty: bool,
        valid: bool,
    },
}

pub const RULE_LINK_SINGLE: Rule = Rule {
    name: "link-single",
    position: LineRequirement::Any,
    try_consume_fn: link,
};

pub const RULE_LINK_SINGLE_NEW_TAB: Rule = Rule {
    name: "link-single-new-tab",
    position: LineRequirement::Any,
    try_consume_fn: link_new_tab,
};

fn link<'r, 't>(parser: &mut Parser<'r, 't>) -> ParseResult<'r, 't, Elements<'t>> {
    assert_step(parser, Token::LeftBracket)?;
    try_consume_link(parser, RULE_LINK_SINGLE, None)
}

fn link_new_tab<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    assert_step(parser, Token::LeftBracketStar)?;
    if parser.settings().layout.legacy() && parser.current().slice == "#" {
        return super::link_anchor::try_consume_wikidot_star_anchor(parser);
    }
    if parser.settings().layout.legacy() {
        let mut attempt = parser.clone();
        return match try_consume_wikidot_link(&mut attempt, Some(AnchorTarget::NewTab)) {
            Ok(success) => {
                parser.update(&attempt);
                Ok(success)
            }
            Err(error) => match super::url::wikidot_star_link_fallback(parser) {
                Some(result) => result,
                None => Err(error),
            },
        };
    }
    try_consume_link(parser, RULE_LINK_SINGLE_NEW_TAB, Some(AnchorTarget::NewTab))
}

fn try_consume_link<'r, 't>(
    parser: &mut Parser<'r, 't>,
    rule: Rule,
    target: Option<AnchorTarget>,
) -> ParseResult<'r, 't, Elements<'t>> {
    if parser.settings().layout.legacy() {
        return try_consume_wikidot_link(parser, target);
    }

    if single_link_raw_forbids_link(parser)? {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let url_close = [ParseCondition::current(Token::Whitespace)];
    let url_invalid = [
        ParseCondition::current(Token::RightBracket),
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::current(Token::LineBreak),
    ];
    let (url, _) =
        collect_text_resolved_fused(parser, rule, &url_close, &url_invalid, None, false)?;

    if !url_valid(&url)
        && let Some(expanded) = parser.settings().interwiki.build(&url)
    {
        let label_close = [ParseCondition::current(Token::RightBracket)];
        let label_invalid = [
            ParseCondition::current(Token::ParagraphBreak),
            ParseCondition::current(Token::LineBreak),
        ];
        let (label, _) = collect_text_resolved_fused(
            parser,
            rule,
            &label_close,
            &label_invalid,
            None,
            false,
        )?;
        return success_elements(Element::Link {
            ltype: LinkType::Interwiki,
            link: LinkLocation::Url(Cow::Owned(expanded)),
            label: LinkLabel::Text(trim_cow(label)),
            target,
        });
    }

    if !url_valid(&url) {
        return Err(parser.make_err(ParseErrorKind::InvalidUrl));
    }

    if target.is_none()
        && let Some(generated) = parser.current_generated().cloned()
        && generated.kind == GeneratedKind::TagLinks
    {
        parser.step()?;
        if parser.current().token != Token::RightBracket {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
        parser.step()?;
        return success_elements(Element::Delayed(DelayedElement::tag_external_label(
            generated.id,
            url,
        )));
    }

    let label_close = [ParseCondition::current(Token::RightBracket)];
    let label_invalid = [
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::current(Token::LineBreak),
    ];
    let (label, _) = collect_text_resolved_fused(
        parser,
        rule,
        &label_close,
        &label_invalid,
        None,
        false,
    )?;

    let element = Element::Link {
        ltype: LinkType::Direct,
        link: LinkLocation::Url(url),
        label: LinkLabel::Text(trim_cow(label)),
        target,
    };

    success_elements(element)
}

fn try_consume_wikidot_link<'r, 't>(
    parser: &mut Parser<'r, 't>,
    target: Option<AnchorTarget>,
) -> ParseResult<'r, 't, Elements<'t>>
where
    'r: 't,
{
    if single_link_raw_forbids_link(parser)? {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if let Some(elements) = wikidot_unlabeled_interwiki(parser, target) {
        return success_elements(elements);
    }

    let (url, authored_prefix_has_link_authority, target_end) =
        collect_wikidot_target(parser)?;
    wikidot_link_from_target(
        parser,
        target,
        url,
        target_end,
        authored_prefix_has_link_authority,
        false,
    )
}

// Live's unlabeled form ("[wikipedia:Albert_Einstein]") links the article and
// uses the path as the label. Its target walk ends at the closing bracket
// instead of at a whitespace, so it is consumed before the labeled walk below
// (probe on a clone: a whitespace first means the labeled form).
fn wikidot_unlabeled_interwiki<'r, 't>(
    parser: &mut Parser<'r, 't>,
    target: Option<AnchorTarget>,
) -> Option<Elements<'t>>
where
    'r: 't,
{
    if target.is_some() {
        return None;
    }

    // A cheap source check keeps the extra walk off every other single link;
    // the case-sensitive table lookup below is what leaves "[WIKIPEDIA:X]"
    // literal while the labeled form resolves it.
    let source = parser.full_text().inner();
    let rest = source.get(parser.current().span.start..)?;
    let mut prefix_len = 0;
    for byte in rest.bytes().take(64) {
        if byte == b':' {
            break;
        }
        if !byte.is_ascii_alphabetic() {
            return None;
        }
        prefix_len += 1;
    }
    if prefix_len == 0 || rest.as_bytes().get(prefix_len) != Some(&b':') {
        return None;
    }
    if parser.link_target_reaches_input_end(parser.current().span.start, false) {
        return None;
    }

    let mut attempt = parser.clone();
    let closes = [ParseCondition::current(Token::RightBracket)];
    let invalids = [
        ParseCondition::current(Token::Whitespace),
        ParseCondition::current(Token::LineBreak),
        ParseCondition::current(Token::ParagraphBreak),
    ];
    let (field, last) =
        collect_comment_elided_keep(&mut attempt, &closes, &invalids, None, None).ok()?;
    if last.token != Token::RightBracket {
        return None;
    }

    let url = discard_wikidot_controls(field.into_cow());
    if url.is_empty()
        || url.len() > MAX_WIKIDOT_SINGLE_LINK_TARGET_BYTES
        || wikidot_link_disposition(&url) != WikidotLinkDisposition::LabelOnly
    {
        return None;
    }

    let expanded = attempt.settings().interwiki.build(&url)?;
    let label = wikidot_interwiki_path_label(&url);
    parser.update(&attempt);
    Some(Elements::Single(Element::Link {
        ltype: LinkType::Interwiki,
        link: LinkLocation::Url(Cow::Owned(expanded)),
        label: LinkLabel::Text(label),
        target: None,
    }))
}

pub fn try_consume_wikidot_end_block_link<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<Elements<'t>>, ParseError>
where
    'r: 't,
{
    if parser.current().token != Token::LeftBlockEnd || parser.at_body_end_boundary() {
        return Ok(None);
    }
    if end_block_name_at(parser.remaining(), 0)
        .is_some_and(|name| end_block_scope_blocked(parser, &name))
    {
        return Ok(None);
    }
    let start = parser.current().span.start;
    let tokens = parser.remaining();
    let region_end =
        parser.cached_end_block_link_region(parser.full_text().inner(), start);
    // The head probe speculates later blocks on a clone that shares the
    // footnote list; a footnote registered by a probe block that does not own
    // this closer must not survive into the real parse.
    let footnote_count = parser.footnote_count();
    let context = parser.end_block_link_head_context();
    let head_claimed = if let Some(outcome) =
        parser.end_block_link_head_outcome(tokens, region_end, context)
    {
        outcome
    } else {
        let mut saw_left_block = false;
        let mut claimed_index = None;
        for (index, token) in tokens
            .iter()
            .take_while(|token| token.span.start < region_end)
            .enumerate()
        {
            if token.token != Token::LeftBlock || token.span.start <= start {
                continue;
            }
            saw_left_block = true;
            let mut probe = parser.clone();
            probe.step_to_remaining(index);
            if let Ok(success) = super::RULE_BLOCK.try_consume(&mut probe) {
                let (elements, _, _) = success.into();
                if elements
                    .into_iter()
                    .any(|element| !matches!(element, crate::tree::Element::Text(_)))
                {
                    claimed_index = Some(index);
                    break;
                }
            }
        }
        #[cfg(test)]
        parser.add_unmatched_close_scan_visits(
            tokens
                .iter()
                .take_while(|token| token.span.start < region_end)
                .count(),
        );
        parser.cache_end_block_link_head_outcome(
            tokens,
            region_end,
            saw_left_block,
            context,
            claimed_index,
        );
        claimed_index.is_some()
    };
    if parser.footnote_count() != footnote_count {
        parser.truncate_footnotes(footnote_count);
    }
    if head_claimed {
        return Ok(None);
    }
    let mut attempt = parser.clone();
    let Ok((success, _target_end)) = link_end_block(&mut attempt) else {
        return Ok(None);
    };
    let (elements, _, _) = success.into();
    let end = attempt.current().span.start;
    let crossed = tokens
        .iter()
        .take_while(|token| token.span.start < end)
        .enumerate()
        .any(|(index, token)| {
            token.token == Token::LeftBlockEnd
                && token.span.start > start
                && end_block_name_at(tokens, index)
                    .is_some_and(|name| end_block_scope_blocked(parser, &name))
        });
    if crossed {
        return Ok(None);
    }
    parser.update(&attempt);
    Ok(Some(elements))
}

fn end_block_scope_blocked(parser: &Parser<'_, '_>, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    parser.body_end_boundary_accepts(&name)
        || (name == "span"
            && (parser.in_wikidot_span_body() || parser.in_wikidot_literal_alias_body()))
        || (name == "a"
            && (parser.in_wikidot_anchor_body()
                || parser.in_wikidot_literal_alias_body()))
        || (name == "div"
            && (parser.in_wikidot_div_body() || parser.in_wikidot_literal_alias_body()))
        || (name == "center" && parser.in_wikidot_center_body())
        || (name == "bibliography" && parser.in_wikidot_bibliography_body())
}

fn end_block_name_at(tokens: &[ExtractedToken<'_>], index: usize) -> Option<String> {
    let mut name = String::new();
    for token in &tokens[index + 1..] {
        match token.token {
            Token::RightBlock => {
                return if name.is_empty() { None } else { Some(name) };
            }
            Token::Whitespace | Token::LineBreak | Token::ParagraphBreak => {
                return None;
            }
            _ => name.push_str(token.slice),
        }
    }
    None
}

fn link_end_block<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<(ParseSuccess<'r, 't, Elements<'t>>, usize), ParseError> {
    if !parser.settings().layout.legacy() || parser.discarding_hidden_body() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.current().token != Token::LeftBlockEnd {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if single_link_raw_forbids_link(parser)? {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let target_start = parser.current().span.start + 2;
    let (url, authored_prefix_has_link_authority, target_end) =
        collect_wikidot_target_from(parser, target_start)?;
    let wide_separator = parser
        .full_text()
        .inner()
        .get(target_end..)
        .is_some_and(|rest| rest.chars().take(2).filter(|c| *c == ' ').count() == 2);
    let parsed = wikidot_link_from_target(
        parser,
        None,
        url,
        target_end,
        authored_prefix_has_link_authority,
        true,
    )?;

    Ok((
        parsed.map(move |elements| {
            let mut elements = match elements {
                Elements::Multiple(elements) => elements,
                Elements::Single(element) => vec![element],
                Elements::None => Vec::new(),
            };
            elements.insert(0, text!("["));
            if wide_separator {
                elements.insert(1, text!(" "));
            }
            Elements::Multiple(elements)
        }),
        target_end,
    ))
}

fn wikidot_link_from_target<'r, 't>(
    parser: &mut Parser<'r, 't>,
    target: Option<AnchorTarget>,
    url: Cow<'t, str>,
    target_end: usize,
    authored_prefix_has_link_authority: bool,
    link_memo: bool,
) -> ParseResult<'r, 't, Elements<'t>>
where
    'r: 't,
{
    let disposition = wikidot_link_disposition(&url);
    let disposition = if disposition == WikidotLinkDisposition::Link
        && !authored_prefix_has_link_authority
    {
        WikidotLinkDisposition::Literal
    } else {
        disposition
    };
    if disposition == WikidotLinkDisposition::Literal {
        return Err(parser.make_err(ParseErrorKind::InvalidUrl));
    }

    // A starred single link only takes a destination live renders as a link
    // ("[*/path label]", "[*https://example.com/ label]"). Live keeps the whole
    // bracket literal for a scheme-shaped target ("[*wikipedia:X label]",
    // "[*foo:bar label]"), where the unstarred form still keeps the label.
    if disposition == WikidotLinkDisposition::LabelOnly && target.is_some() {
        return Err(parser.make_err(ParseErrorKind::InvalidUrl));
    }

    if target.is_none()
        && let Cow::Borrowed(url) = &url
        && let Some(generated) = parser.current_generated().cloned()
        && generated.kind == GeneratedKind::TagLinks
    {
        parser.step()?;
        if parser.current().token != Token::RightBracket {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
        parser.step()?;
        return success_elements(Element::Delayed(DelayedElement::tag_external_label(
            generated.id,
            Cow::Borrowed(url),
        )));
    }

    // This walk reaches a label: live resolves a scheme-shaped target against
    // its interwiki table without regard to ASCII case when a label is
    // authored ("[WIKIPEDIA:Albert_Einstein Upper]" links). The unlabeled form
    // is consumed by wikidot_unlabeled_interwiki before this walk.
    let interwiki_url = match disposition {
        WikidotLinkDisposition::LabelOnly => wikidot_interwiki_url(parser, &url),
        WikidotLinkDisposition::Link | WikidotLinkDisposition::Literal => None,
    };

    let empty_label_run_forms = match parser.full_text().inner().get(target_end..) {
        Some(rest) => {
            let run_len = rest
                .bytes()
                .take_while(|byte| matches!(byte, b' ' | b'\t'))
                .count();
            run_len > 1 || rest[..run_len].contains('\t')
        }
        None => false,
    };

    let WikidotLabel {
        text: label,
        residual_closer,
    } = collect_wikidot_label(parser, link_memo)?;
    let label = trim_wikidot_ascii_cow(discard_wikidot_controls(label));

    let element = match (disposition, interwiki_url) {
        (WikidotLinkDisposition::LabelOnly, Some(expanded)) => {
            if label.is_empty() && !empty_label_run_forms {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            Element::Link {
                ltype: LinkType::Interwiki,
                link: LinkLocation::Url(Cow::Owned(expanded)),
                label: LinkLabel::Text(label),
                target,
            }
        }
        (WikidotLinkDisposition::LabelOnly, None) => {
            if label.is_empty() && !empty_label_run_forms {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            Element::Text(label)
        }
        (WikidotLinkDisposition::Link, _) => {
            if label.is_empty() && !empty_label_run_forms {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            Element::Link {
                ltype: LinkType::Direct,
                link: LinkLocation::Url(url),
                label: LinkLabel::Text(label),
                target,
            }
        }
        (WikidotLinkDisposition::Literal, _) => unreachable!(),
    };

    if residual_closer.is_empty() {
        success_elements(element)
    } else {
        success_elements(vec![element, text!(residual_closer)])
    }
}

fn wikidot_interwiki_url(parser: &Parser<'_, '_>, url: &str) -> Option<String> {
    parser.settings().interwiki.build_ignore_ascii_case(url)
}

fn wikidot_interwiki_path_label<'t>(url: &Cow<'t, str>) -> Cow<'t, str> {
    match url {
        Cow::Borrowed(url) => match url.split_once(':') {
            Some((_, path)) => Cow::Borrowed(path),
            None => Cow::Borrowed(""),
        },
        Cow::Owned(url) => match url.split_once(':') {
            Some((_, path)) => Cow::Owned(path.to_owned()),
            None => Cow::Owned(String::new()),
        },
    }
}

fn collect_wikidot_target<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<(Cow<'t, str>, bool, usize), ParseError>
where
    'r: 't,
{
    collect_wikidot_target_inner(parser, None, false, false)
}

fn collect_wikidot_target_from<'r, 't>(
    parser: &mut Parser<'r, 't>,
    start: usize,
) -> Result<(Cow<'t, str>, bool, usize), ParseError>
where
    'r: 't,
{
    let line_break_separator = parser.in_wikidot_bibliography_body();
    collect_wikidot_target_inner(parser, Some(start), line_break_separator, true)
}

fn collect_wikidot_target_inner<'r, 't>(
    parser: &mut Parser<'r, 't>,
    start: Option<usize>,
    line_break_separator: bool,
    link_memo: bool,
) -> Result<(Cow<'t, str>, bool, usize), ParseError>
where
    'r: 't,
{
    // The target walk only stops early at whitespace, a line break, a
    // paragraph break, a valid comment, or a fused segment; the last two
    // need a later `]`. With none of those left, the walk is certain to
    // reach `InputEnd` and fail with `EndOfInput`, so reproduce that
    // error instead of walking (a long unclosed `[` run used to walk the
    // rest of the document once per opener).
    if !link_memo
        && parser.link_target_reaches_input_end(parser.current().span.start, false)
    {
        return Err(parser.make_end_of_input_err());
    }
    let closes: &[ParseCondition] = if line_break_separator {
        &[
            ParseCondition::current(Token::Whitespace),
            ParseCondition::current(Token::LineBreak),
        ]
    } else {
        &[ParseCondition::current(Token::Whitespace)]
    };
    let invalids: &[ParseCondition] = if line_break_separator {
        &[ParseCondition::current(Token::ParagraphBreak)]
    } else {
        &[
            ParseCondition::current(Token::LineBreak),
            ParseCondition::current(Token::ParagraphBreak),
        ]
    };
    let memo = link_memo.then(|| LinkTargetScanMemo {
        line_break_separator,
        in_iftags_span: parser.in_wikidot_iftags_span(),
    });
    let (target, _) = match start {
        Some(start) => {
            collect_comment_elided_keep_from(parser, start, closes, invalids, None, memo)?
        }
        None => collect_comment_elided_keep(parser, closes, invalids, None, memo)?,
    };
    if target.span().len() > MAX_WIKIDOT_SINGLE_LINK_TARGET_BYTES {
        return Err(parser.make_err(ParseErrorKind::InvalidUrl));
    }
    let target_end = target.span().end;
    let authored_prefix_has_link_authority = target.comment_ranges().is_empty()
        || wikidot_link_disposition(target.prefix_before_first_comment())
            == WikidotLinkDisposition::Link;
    let target = discard_wikidot_controls(target.into_cow());
    if target.len() > MAX_WIKIDOT_SINGLE_LINK_TARGET_BYTES {
        return Err(parser.make_err(ParseErrorKind::InvalidUrl));
    }
    Ok((target, authored_prefix_has_link_authority, target_end))
}

fn collect_wikidot_label<'r, 't>(
    parser: &mut Parser<'r, 't>,
    link_memo: bool,
) -> Result<WikidotLabel<'t>, ParseError>
where
    'r: 't,
{
    let memo = if link_memo && parser.body_end_boundaries_is_empty() {
        Some(LinkLabelScanMemo {
            in_table_cell: parser.in_wikidot_simple_table_cell(),
            in_triple_link: parser.in_wikidot_literal_triple_link(),
            in_iftags_span: parser.in_wikidot_iftags_span(),
        })
    } else {
        None
    };
    let mut traversed = Vec::new();
    let result = collect_wikidot_label_inner(parser, memo, &mut traversed);
    if let (Some(memo), Err(_)) = (memo, &result) {
        parser.cache_link_label_scan_failed(
            &traversed,
            memo.in_table_cell,
            memo.in_triple_link,
            memo.in_iftags_span,
        );
    }
    result
}

fn collect_wikidot_label_inner<'r, 't>(
    parser: &mut Parser<'r, 't>,
    link_memo: Option<LinkLabelScanMemo>,
    traversed: &mut Vec<usize>,
) -> Result<WikidotLabel<'t>, ParseError>
where
    'r: 't,
{
    if wikidot_label_has_complete_owner(parser) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let mut copied_until = start;
    let mut without_comments = None::<String>;

    loop {
        if let Some(memo) = link_memo {
            let addr = parser.current() as *const ExtractedToken as usize;
            traversed.push(addr);
            #[cfg(test)]
            parser.add_unmatched_close_scan_visits(1);
            if parser.link_label_scan_failed(
                addr,
                memo.in_table_cell,
                memo.in_triple_link,
                memo.in_iftags_span,
            ) {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
        }

        match parser.current().token {
            token
                if parser.in_wikidot_simple_table_cell()
                    && is_table_column_token(token) =>
            {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            Token::RightLink if parser.in_wikidot_literal_triple_link() => {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            Token::RightBracket | Token::RightBlock | Token::RightLink => {
                let end = parser.current().span.start;
                if let Some(output) = without_comments.as_mut() {
                    output.push_str(&source[copied_until..end]);
                }
                let residual_closer = &parser.current().slice[1..];
                parser.step()?;
                return Ok(WikidotLabel {
                    text: match without_comments {
                        Some(output) => Cow::Owned(output),
                        None => Cow::Borrowed(&source[start..end]),
                    },
                    residual_closer,
                });
            }
            Token::LeftComment => {
                let comment_start = parser.current().span.start;
                let output = without_comments
                    .get_or_insert_with(|| String::with_capacity(comment_start - start));
                output.push_str(&source[copied_until..comment_start]);

                let mut comment = parser.clone();
                consume_valid_comment(&mut comment)?;
                parser.update(&comment);
                copied_until = parser.current().span.start;
            }
            Token::LeftBlock => match consume_iftags_segment_fused(parser)? {
                Some(segment) => {
                    {
                        let output = without_comments.get_or_insert_with(|| {
                            String::with_capacity(segment.range.start - start)
                        });
                        output.push_str(&source[copied_until..segment.range.start]);
                        if segment.include_body {
                            output.push_str(&source[segment.body.clone()]);
                        }
                    }
                    copied_until = segment.range.end;

                    if segment.fused_closer {
                        return Ok(WikidotLabel {
                            text: Cow::Owned(without_comments.take().unwrap()),
                            residual_closer: "",
                        });
                    }
                }
                None => {
                    parser.step()?;
                }
            },
            Token::LineBreak | Token::ParagraphBreak => {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            Token::LeftBlockEnd if parser.at_body_end_boundary() => {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            Token::InputEnd => return Err(parser.make_err(ParseErrorKind::EndOfInput)),
            Token::GeneratedPageLink | Token::GeneratedTagLinks | Token::RuntimeText => {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            _ => {
                parser.step()?;
            }
        }
    }
}

fn is_table_column_token(token: Token) -> bool {
    matches!(
        token,
        Token::TableColumn
            | Token::TableColumnTitle
            | Token::TableColumnCenter
            | Token::TableColumnRight
    )
}

fn wikidot_label_has_complete_owner<'r, 't>(parser: &Parser<'r, 't>) -> bool
where
    'r: 't,
{
    let start = parser.current().span.start;
    if let Some(outcome) = parser.single_link_label_owner_scan_outcome(start) {
        return outcome;
    }

    if wikidot_label_has_complete_named_anchor_owner(parser)
        || wikidot_label_has_complete_bibcite_owner(parser)
        || wikidot_label_has_complete_math_owner(parser)
    {
        parser.cache_single_link_label_owner_scan_outcomes(&[start], true);
        return true;
    }

    let mut scan = parser.clone();
    let mut traversed = Vec::new();
    let mut raw_open = false;
    let mut alternate_raw_open = false;
    let mut triple_link_open = false;
    let mut span_open = false;
    let mut footnote_open = false;
    let footnote_may_own = !parser.in_footnote();
    let mut size_open = false;
    let mut a_open = false;
    loop {
        traversed.push(scan.current().span.start);
        match scan.current().token {
            Token::Raw if raw_open => {
                parser.cache_single_link_label_owner_scan_outcomes(&traversed, true);
                return true;
            }
            Token::Raw => raw_open = true,
            Token::LeftRaw => alternate_raw_open = true,
            Token::RightRaw if alternate_raw_open => {
                parser.cache_single_link_label_owner_scan_outcomes(&traversed, true);
                return true;
            }
            Token::LeftLink | Token::LeftLinkStar => triple_link_open = true,
            Token::RightLink if triple_link_open => {
                parser.cache_single_link_label_owner_scan_outcomes(&traversed, true);
                return true;
            }
            Token::LeftBlock => {
                let mut opener = scan.clone();
                if let Ok((name, in_head, segment)) = opener.get_block_name(false) {
                    let mut head_complete = !in_head;
                    if in_head {
                        loop {
                            match opener.current().token {
                                Token::RightBlock => {
                                    let _ = opener.step();
                                    head_complete = true;
                                    break;
                                }
                                Token::RightLink if opener.current().slice == "]]]" => {
                                    let _ = opener.step();
                                    head_complete = true;
                                    break;
                                }
                                Token::RightBracket
                                | Token::RightLink
                                | Token::LineBreak
                                | Token::ParagraphBreak
                                | Token::InputEnd => break,
                                _ => {
                                    if opener.step().is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    if head_complete {
                        let actionable = opener.head_word_is_actionable(
                            &name,
                            matches!(name, Cow::Borrowed(_)),
                            segment,
                        );
                        if actionable
                            && (name.eq_ignore_ascii_case("image")
                                || name.eq_ignore_ascii_case("user"))
                        {
                            parser.cache_single_link_label_owner_scan_outcomes(
                                &traversed, true,
                            );
                            return true;
                        }
                        if actionable {
                            span_open |= name.eq_ignore_ascii_case("span");
                            footnote_open |=
                                footnote_may_own && name.eq_ignore_ascii_case("footnote");
                            size_open |= name.eq_ignore_ascii_case("size");
                            a_open |= name.eq_ignore_ascii_case("a");
                        }
                        scan.update(&opener);
                        continue;
                    }
                }
            }
            Token::LeftBlockEnd => {
                let mut close = scan.clone();
                if let Ok((name, _residual)) = close.get_wikidot_end_block_with_residual()
                {
                    if span_open && name.eq_ignore_ascii_case("span") {
                        parser.cache_single_link_label_owner_scan_outcomes(
                            &traversed, true,
                        );
                        return true;
                    }
                    if size_open && name.eq_ignore_ascii_case("size") {
                        parser.cache_single_link_label_owner_scan_outcomes(
                            &traversed, true,
                        );
                        return true;
                    }
                    if a_open && name.eq_ignore_ascii_case("a") {
                        parser.cache_single_link_label_owner_scan_outcomes(
                            &traversed, true,
                        );
                        return true;
                    }
                    if footnote_open && name.eq_ignore_ascii_case("footnote") {
                        parser.cache_single_link_label_owner_scan_outcomes(
                            &traversed, true,
                        );
                        return true;
                    }
                    scan.update(&close);
                    continue;
                }
            }
            Token::RightBracket
            | Token::RightBlock
            | Token::RightLink
            | Token::LineBreak
            | Token::ParagraphBreak
            | Token::InputEnd => {
                parser.cache_single_link_label_owner_scan_outcomes(&traversed, false);
                return false;
            }
            _ => {}
        }
        if scan.step().is_err() {
            parser.cache_single_link_label_owner_scan_outcomes(&traversed, false);
            return false;
        }
    }
}

fn wikidot_label_has_complete_named_anchor_owner<'r, 't>(parser: &Parser<'r, 't>) -> bool
where
    'r: 't,
{
    let mut scan = parser.clone();
    let mut anchor = WikidotNamedAnchorOwnerScan::default();
    loop {
        if scan.current().token == Token::LeftComment {
            anchor = WikidotNamedAnchorOwnerScan::Search;
            let mut comment = scan.clone_with_rule(RULE_COMMENT);
            if RULE_COMMENT.try_consume(&mut comment).is_ok() {
                scan.update(&comment);
                continue;
            }
        }
        if anchor.advance(scan.current()) {
            return true;
        }

        match scan.current().token {
            Token::RightBracket
            | Token::RightBlock
            | Token::RightLink
            | Token::LineBreak
            | Token::ParagraphBreak
            | Token::InputEnd
            | Token::RuntimeText
            | Token::GeneratedPageLink
            | Token::GeneratedTagLinks => return false,
            _ => {}
        }
        if scan.step().is_err() {
            return false;
        }
    }
}

fn wikidot_label_has_complete_math_owner<'r, 't>(parser: &Parser<'r, 't>) -> bool
where
    'r: 't,
{
    let mut scan = parser.clone();
    loop {
        if scan.current().token == Token::LeftComment {
            let mut comment = scan.clone();
            if consume_valid_comment(&mut comment).is_ok() {
                scan.update(&comment);
                continue;
            }
        }

        match scan.current().token {
            Token::LeftMath => return wikidot_math_candidate_is_complete(&scan),
            Token::RightBracket
            | Token::RightBlock
            | Token::RightLink
            | Token::LineBreak
            | Token::ParagraphBreak
            | Token::InputEnd
            | Token::RuntimeText
            | Token::GeneratedPageLink
            | Token::GeneratedTagLinks => return false,
            _ => {}
        }
        if scan.step().is_err() {
            return false;
        }
    }
}

fn wikidot_label_has_complete_bibcite_owner<'r, 't>(parser: &Parser<'r, 't>) -> bool
where
    'r: 't,
{
    let mut scan = parser.clone();
    let mut bibcite = WikidotBibciteOwnerScan::default();
    loop {
        if scan.current().token == Token::LeftComment {
            bibcite = WikidotBibciteOwnerScan::Search;
            let mut comment = scan.clone_with_rule(RULE_COMMENT);
            if RULE_COMMENT.try_consume(&mut comment).is_ok() {
                scan.update(&comment);
                continue;
            }
        }
        if bibcite.advance(scan.current()) {
            return true;
        }

        match scan.current().token {
            Token::RightBracket
            | Token::RightBlock
            | Token::RightLink
            | Token::LineBreak
            | Token::ParagraphBreak
            | Token::InputEnd
            | Token::GeneratedPageLink
            | Token::GeneratedTagLinks => return false,
            _ => {}
        }
        if scan.step().is_err() {
            return false;
        }
    }
}

impl WikidotBibciteOwnerScan {
    fn advance(&mut self, token: &ExtractedToken<'_>) -> bool {
        let restart = || {
            if token.token == Token::LeftParentheses {
                Self::Name
            } else {
                Self::Search
            }
        };

        *self = match *self {
            Self::Search if token.token == Token::LeftParentheses => Self::Name,
            Self::Name
                if token.token == Token::Identifier
                    && token.slice.eq_ignore_ascii_case("bibcite") =>
            {
                Self::Separator
            }
            Self::Separator if token.token == Token::Whitespace => {
                Self::Label { nonempty: false }
            }
            Self::Label { nonempty }
                if token.token == Token::RightParentheses && nonempty =>
            {
                return true;
            }
            Self::Label { nonempty }
                if token.slice.chars().all(|character| {
                    character.is_ascii_alphanumeric() || character == '_'
                }) =>
            {
                Self::Label {
                    nonempty: nonempty || !token.slice.is_empty(),
                }
            }
            _ => restart(),
        };
        false
    }
}

impl WikidotNamedAnchorOwnerScan {
    fn advance(&mut self, token: &ExtractedToken<'_>) -> bool {
        let restart = || {
            if token.token == Token::LeftBlockAnchor {
                Self::Separator
            } else {
                Self::Search
            }
        };

        *self = match *self {
            Self::Search if token.token == Token::LeftBlockAnchor => Self::Separator,
            Self::Separator if token.token == Token::Whitespace => Self::Name {
                nonempty: false,
                valid: true,
            },
            Self::Name {
                nonempty: true,
                valid: true,
            } if matches!(token.token, Token::RightBlock | Token::RightLink) => {
                return true;
            }
            Self::Name { nonempty, valid }
                if !matches!(
                    token.token,
                    Token::RightBlock
                        | Token::RightLink
                        | Token::LineBreak
                        | Token::ParagraphBreak
                        | Token::InputEnd
                        | Token::RuntimeText
                        | Token::GeneratedPageLink
                        | Token::GeneratedTagLinks
                ) =>
            {
                Self::Name {
                    nonempty: nonempty || !token.slice.is_empty(),
                    valid: valid
                        && token.slice.chars().all(|character| {
                            character.is_ascii_alphanumeric()
                                || matches!(character, '-' | '_')
                        }),
                }
            }
            _ => restart(),
        };
        false
    }
}

fn single_link_raw_forbids_link<'r, 't>(
    parser: &Parser<'r, 't>,
) -> Result<bool, ParseError>
where
    'r: 't,
{
    use super::raw::RULE_RAW;

    // The probe walk only ever returns `true` through a successful
    // `RULE_RAW` attempt at a `Raw`/`LeftRaw` token, and the tokenizer
    // only produces those from `@@`/`@<` bytes. With no `@` left the walk
    // can only step to `InputEnd` (or to a token that already returns
    // `false`), so skip it: for a long run of `[` that walk used to scan
    // the rest of the document once per opener.
    if parser.raw_forbids_link_reaches_input_end(parser.current().span.start) {
        return Ok(false);
    }

    let mut probe = parser.clone();
    let mut no_raw_close = false;
    let mut no_alt_raw_close = false;

    loop {
        match probe.current().token {
            Token::RightBracket
            | Token::LineBreak
            | Token::ParagraphBreak
            | Token::InputEnd => return Ok(false),

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
                        Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                            break;
                        }
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

fn wikidot_link_disposition(url: &str) -> WikidotLinkDisposition {
    if url.is_empty() || url.len() > MAX_WIKIDOT_SINGLE_LINK_TARGET_BYTES {
        return WikidotLinkDisposition::Literal;
    }
    if url.starts_with('/')
        || url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("ftp://")
        || url.starts_with("gopher://")
        || url.starts_with("news://")
        || url.starts_with("mms://")
        || url.starts_with("ssh://")
        || url.starts_with("git://")
        || url.starts_with("sftp://")
        || url.starts_with("svn://")
        || url.starts_with("telnet://")
        || url.starts_with("cvs://")
        || url.starts_with("mailto:")
    {
        return WikidotLinkDisposition::Link;
    }

    let Some((scheme, rest)) = url.split_once(':') else {
        return WikidotLinkDisposition::Literal;
    };
    if !valid_bounded_scheme(scheme) || scheme.eq_ignore_ascii_case("data") {
        return WikidotLinkDisposition::Literal;
    }
    if rest.is_empty() || !wikidot_scheme_target_rest_is_visible(rest) {
        return WikidotLinkDisposition::Literal;
    }

    WikidotLinkDisposition::LabelOnly
}

// Live keeps a scheme-shaped single-link target as text (and resolves it as an
// interwiki when the prefix is known) only when every character after the
// colon is one it accepts there. Saved-page probes (2026-11-06) show that a
// target containing "%", "?", "'", a quote, "*", ",", "@", "!", "^", "\", a
// backtick, braces, a pipe, "[", "<", ">" or a non-ASCII character renders the
// whole bracket literally instead, for both known and unknown prefixes.
fn wikidot_scheme_target_rest_is_visible(rest: &str) -> bool {
    rest.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'#' | b'&'
                    | b'('
                    | b')'
                    | b'+'
                    | b'-'
                    | b'.'
                    | b'/'
                    | b':'
                    | b';'
                    | b'='
                    | b'_'
                    | b'~'
            )
    })
}

fn valid_bounded_scheme(scheme: &str) -> bool {
    if scheme.is_empty() {
        return false;
    }
    let mut bytes = scheme.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && bytes.all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.')
        })
}

fn url_valid(url: &str) -> bool {
    if url.is_empty() {
        return false;
    }

    if url.starts_with('/') {
        return true;
    }

    if is_url(url) {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::{WikidotLinkDisposition, valid_bounded_scheme, wikidot_link_disposition};
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn wikijump_single_bracket_interwiki_resolves_known_prefixes() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let render = |source: &str| {
            let tokenization = crate::tokenize(source);
            let (tree, _) = crate::parse(&tokenization, &page_info, &settings).into();
            HtmlRender.render(&tree, &page_info, &settings).body
        };

        let html = render("[wikipedia:Mallard The duck] [https://example.com/ Plain]");
        assert!(
            html.contains(r#"href="https://wikipedia.org/wiki/Mallard">The duck</a>"#),
            "{html}"
        );
        assert!(
            html.contains(r#"href="https://example.com/">Plain</a>"#),
            "{html}"
        );

        let html = render("[nosuchwiki:Mallard The duck]");
        assert!(!html.contains("<a "), "{html}");
    }

    #[test]
    fn wikidot_single_link_disposition_separates_compatibility_and_security() {
        for url in [
            "http://example.com",
            "https://example.com",
            "ftp://example.com/a",
            "gopher://example.com/g",
            "news://example.com/n",
            "mms://example.com/m",
            "mailto:user@example.com",
            "/start",
            "//example.com/a",
        ] {
            assert_eq!(
                wikidot_link_disposition(url),
                WikidotLinkDisposition::Link,
                "{url}",
            );
        }
        for url in [
            "HTTPS://example.com",
            "HtTpS://example.com",
            "https:example.com",
            "javascript:void(0)",
            "foo:bar",
        ] {
            assert_eq!(
                wikidot_link_disposition(url),
                WikidotLinkDisposition::LabelOnly,
                "{url}",
            );
        }
        for url in ["", "example.com", "data:text/plain,x", "DATA:text/html,x"] {
            assert_eq!(
                wikidot_link_disposition(url),
                WikidotLinkDisposition::Literal,
                "{url}",
            );
        }
    }

    #[test]
    fn wikidot_single_link_scheme_targets_require_live_visible_characters() {
        for url in [
            "foo:bar",
            "foo:Val/ue",
            "foo.bar:Value",
            "wikipedia:Albert_Einstein#Sec",
            "wikipedia:it:Albert_Einstein",
            "google:free+wiki",
            "dictionary:wiki",
        ] {
            assert_eq!(
                wikidot_link_disposition(url),
                WikidotLinkDisposition::LabelOnly,
                "{url}",
            );
        }

        for url in [
            "foo:",
            "foo:bar%20baz",
            "foo:Bar?x=1",
            "foo:A!B",
            "wikipedia:Albert,E",
            "wikipedia:A|B",
            "wikipedia:A<B",
            "wikipedia:日本",
        ] {
            assert_eq!(
                wikidot_link_disposition(url),
                WikidotLinkDisposition::Literal,
                "{url}",
            );
        }
    }

    #[test]
    fn wikidot_single_link_scheme_scan_accepts_long_valid_schemes() {
        assert!(valid_bounded_scheme("web+demo"));
        assert!(valid_bounded_scheme(&"a".repeat(65)));
        assert!(valid_bounded_scheme(&"a".repeat(1_024)));
        assert!(!valid_bounded_scheme("1http"));
        assert!(!valid_bounded_scheme("java_script"));
    }

    #[test]
    fn wikidot_layout_does_not_weaken_dangerous_single_link_sanitization() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization =
            crate::tokenize("[javascript:alert(1) XSS] [data:text/html,alert(2) XSS]");
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(!html.contains("[javascript:alert(1) XSS]"), "{html}");
        assert!(html.contains("XSS"), "{html}");
        assert!(html.contains("[data:text/html,alert(2) XSS]"), "{html}");
        assert!(!html.contains("href=\"javascript:"), "{html}");
        assert!(!html.contains("href=\"data:"), "{html}");
    }

    #[test]
    fn wikidot_single_link_labels_with_complete_blocks_do_not_form() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let render = |source: &str| {
            let tokenization = crate::tokenize(source);
            let (tree, _) = crate::parse(&tokenization, &page_info, &settings).into();
            HtmlRender.render(&tree, &page_info, &settings).body
        };

        let html = render("[https://example.com/ [[span class=\"c\"]]Y[[/span]] end]");
        assert_eq!(
            html,
            "<p>[<a href=\"https://example.com/\">https://example.com/</a> <span class=\"c\">Y</span> end]</p>",
        );

        for (source, corrupt) in [
            (
                "[https://example.com [[image foo.gif]]]",
                "[[image foo.gif</a>",
            ),
            (
                "[https://example.com/ [[size 120%]]big[[/size]]]",
                "[[size 120%</a>",
            ),
            (
                "[https://example.com/ [[user SomeUser]]]",
                "[[user SomeUser</a>",
            ),
        ] {
            let html = render(source);
            assert!(!html.contains(corrupt), "{source} => {html}");
            assert!(
                html.contains("[<a href=\"https://example.com"),
                "{source} => {html}"
            );
        }

        let html = render("[https://example.com [[div]]A[[/div]]]");
        assert_eq!(
            html,
            "<p><a href=\"https://example.com\">[[div</a>]A[[/div]]]</p>",
        );

        let html = render("[https://example.com/ [[span class=\"c\"]]Y]");
        assert_eq!(
            html,
            "<p><a href=\"https://example.com/\">[[span class=&quot;c&quot;</a>]Y]</p>",
        );
    }
}
