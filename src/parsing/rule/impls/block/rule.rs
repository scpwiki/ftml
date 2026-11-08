/*
 * parsing/rule/impls/block/rule.rs
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

use super::super::prelude::*;
use super::mapping::{get_block_rule_with_name, get_block_rule_with_name_for_layout};
use super::parser::{
    WikidotHeadSpaceRegion, head_seam_forms_block, head_word_never_resolves,
};
use super::{
    BlockRule,
    blocks::{
        BLOCK_COLLAPSIBLE, BLOCK_EMBED, BLOCK_EMBED_AUDIO, BLOCK_EMBED_VIDEO, BLOCK_HTML,
        BLOCK_IFTAGS, BLOCK_NOTE, BLOCK_SPAN, BLOCK_TABVIEW,
        iftags_head_keeps_inline_body,
    },
};
use crate::parsing::inline_scope::WIKIDOT_SCORE_SPAN_UNWRAPPED_ATTRIBUTE;
use crate::parsing::parser::QuoteBodyLineStatus;
use crate::parsing::rule::bibliography_order;
use crate::settings::WikitextMode;
use crate::tree::{ContainerType, PartialElement};
use std::borrow::Cow;

pub const RULE_BLOCK: Rule = Rule {
    name: "block",
    position: LineRequirement::Any,
    try_consume_fn: block_regular,
};

pub const RULE_BLOCK_STAR: Rule = Rule {
    name: "block-star",
    position: LineRequirement::Any,
    try_consume_fn: block_star,
};

pub const RULE_BLOCK_SKIP_NEWLINE: Rule = Rule {
    name: "block-skip",
    position: LineRequirement::Any,
    try_consume_fn: block_skip,
};

fn block_regular<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    if wikidot_scored_span_residual_opener(parser) {
        parse_wikidot_scored_span_residual(parser)
    } else {
        parse_block(parser, false)
    }
}

fn block_star<'r, 't>(parser: &mut Parser<'r, 't>) -> ParseResult<'r, 't, Elements<'t>> {
    parse_block(parser, true)
}

fn block_for_head_word<'r, 't>(
    parser: &Parser<'r, 't>,
    name: &str,
    borrowed: bool,
    segment: bool,
) -> Option<&'static BlockRule> {
    let block = if parser.discarding_hidden_body() {
        get_block_rule_with_name(name)
    } else {
        get_block_rule_with_name_for_layout(name, parser.settings().layout)
    }?;
    let actionable = borrowed
        || if segment {
            head_seam_forms_block(block, parser.settings().layout)
        } else {
            !head_word_never_resolves(block)
        };
    actionable.then_some(block)
}

pub(crate) fn head_word_names_known_block<'r, 't>(parser: &mut Parser<'r, 't>) -> bool
where
    'r: 't,
{
    let Ok((name, _, segment)) = parser.get_block_name(false) else {
        return false;
    };
    let lookup_name: Cow<'t, str> = if name.ends_with('_') {
        match name {
            Cow::Borrowed(name) => Cow::Borrowed(name.strip_suffix('_').unwrap_or(name)),
            Cow::Owned(mut name) => {
                name.pop();
                Cow::Owned(name)
            }
        }
    } else {
        name
    };
    if lookup_name.ends_with('_') {
        return false;
    }
    block_for_head_word(
        parser,
        &lookup_name,
        matches!(lookup_name, Cow::Borrowed(_)),
        segment,
    )
    .is_some()
}

fn preserves_preceding_line_break<'r, 't>(
    parser: &Parser<'r, 't>,
    block_rule: &BlockRule,
) -> bool
where
    'r: 't,
{
    block_rule.name == BLOCK_IFTAGS.name && iftags_head_keeps_inline_body(parser)
}

fn block_skip<'r, 't>(parser: &mut Parser<'r, 't>) -> ParseResult<'r, 't, Elements<'t>> {
    let span_doubles_leading_break = parser.settings().layout.legacy()
        && (parser.in_wikidot_span_body() || parser.in_wikidot_size_body())
        && parser.current().token == Token::LineBreak;
    parser.step()?;
    let quote_body = parser.quote_body_cursor().is_some();

    let result = parser.evaluate_fn(|parser| {
        if quote_body {
            match parser.prepare_quote_body_line()? {
                QuoteBodyLineStatus::Prepared => {}
                QuoteBodyLineStatus::Inactive | QuoteBodyLineStatus::Boundary => {
                    return Ok(false);
                }
            }
        }

        if ![Token::LeftBlock, Token::LeftBlockStar].contains(&parser.current().token) {
            return Ok(false);
        }

        let (name, _, segment) = parser.get_block_name(false)?;

        let block =
            block_for_head_word(parser, &name, matches!(name, Cow::Borrowed(_)), segment);
        match block {
            Some(block_rule) => Ok(block_rule.accepts_newlines_outside_body
                && block_rule_enabled(parser, block_rule)
                && !(parser.settings().layout.legacy()
                    && (block_rule.name == BLOCK_EMBED.name
                        || block_rule.name == BLOCK_EMBED_AUDIO.name))
                // L85: a break before an explicit list survives (live keeps
                // it: R1, l-ul-li-closein); native lists still skip.
                && !(parser.settings().layout.legacy()
                    && matches!(
                        block_rule.name,
                        "block-list-unordered" | "block-list-ordered"
                    ))
                // L85 scope-5: live keeps the break before an align block in
                // item bodies (li-align-contained; family-uniform per ruling
                // treatment, only [[=]] directly probed).
                && !(parser.settings().layout.legacy()
                    && parser.wikidot_li_inside_item_body()
                    && matches!(
                        block_rule.name,
                        "block-align-center"
                            | "block-align-justify"
                            | "block-align-left"
                            | "block-align-right"
                    ))
                // Live keeps the single separator newline before a tabview
                // when a bare block precedes it (the break renders as a
                // bare `<br>`; gather publishes the block-adjacency as
                // `wikidot_line_follows_block`). Saved-page probes
                // muse-probe-tvp-o3-ad, muse-probe-tfp-ad7/ad9/w14.
                && !(block_rule.name == BLOCK_TABVIEW.name && parser.wikidot_line_follows_block())
                && !preserves_preceding_line_break(parser, block_rule)
                && !(quote_body && block_rule.name != BLOCK_NOTE.name)),
            None => Ok(false),
        }
    });

    if result {
        if quote_body {
            match parser.prepare_quote_body_line()? {
                QuoteBodyLineStatus::Prepared => {}
                QuoteBodyLineStatus::Inactive | QuoteBodyLineStatus::Boundary => {
                    return Err(parser.make_err(ParseErrorKind::RuleFailed));
                }
            }
        }
        let flag_star = parser.current().token == Token::LeftBlockStar;
        let mut success = parse_block(parser, flag_star)?;
        if success.paragraph_safe && wikidot_literal_css_module(&success.item) {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
        // An open span in a list item keeps the newline this skip swallowed.
        // A collapsible doubles it. A tabview keeps one. d4-46 (a): a div
        // doubles it too (a10/a20 saved-page probes). d446-neighbors: note
        // and code double it too (r1/r2/q4/p5 saved-page probes). A tabview
        // in an item body keeps one even without an open span (w7).
        if parser.wikidot_li_inside_item_body()
            && (span_doubles_leading_break || elements_open_tabview(&success.item))
        {
            let leading_breaks = if elements_open_collapsible(&success.item)
                || elements_open_div(&success.item)
                || elements_open_note(&success.item)
                || elements_open_code(&success.item)
            {
                2
            } else if elements_open_tabview(&success.item) {
                1
            } else {
                0
            };
            if leading_breaks > 0 {
                let mut elements = match success.item {
                    Elements::Multiple(elements) => elements,
                    Elements::Single(element) => vec![element],
                    Elements::None => Vec::new(),
                };
                for _ in 0..leading_breaks {
                    elements.insert(0, Element::LineBreak);
                }
                success.item = Elements::Multiple(elements);
            }
        }
        Ok(success)
    } else {
        Err(parser.make_err(ParseErrorKind::RuleFailed))
    }
}

fn elements_open_tabview(elements: &Elements<'_>) -> bool {
    let is_tabview = |element: &Element<'_>| matches!(element, Element::TabView(_));
    match elements {
        Elements::Single(element) => is_tabview(element),
        Elements::Multiple(elements) => elements.iter().any(is_tabview),
        Elements::None => false,
    }
}

fn elements_open_collapsible(elements: &Elements<'_>) -> bool {
    let is_collapsible = |element: &Element<'_>| match element {
        Element::Collapsible { .. } => true,
        Element::Partial(PartialElement::WikidotBlockOpen(shell)) => {
            matches!(shell, crate::tree::WikidotBlockShell::Collapsible { .. })
        }
        _ => false,
    };
    match elements {
        Elements::Single(element) => is_collapsible(element),
        Elements::Multiple(elements) => elements.iter().any(is_collapsible),
        Elements::None => false,
    }
}

fn elements_open_div(elements: &Elements<'_>) -> bool {
    let is_div = |element: &Element<'_>| match element {
        Element::Container(container) => matches!(container.ctype(), ContainerType::Div),
        Element::Partial(PartialElement::WikidotBlockOpen(shell)) => {
            matches!(shell, crate::tree::WikidotBlockShell::Div { .. })
        }
        _ => false,
    };
    match elements {
        Elements::Single(element) => is_div(element),
        Elements::Multiple(elements) => elements.iter().any(is_div),
        Elements::None => false,
    }
}

fn elements_open_note(elements: &Elements<'_>) -> bool {
    let is_note = |element: &Element<'_>| match element {
        Element::Container(container) => matches!(container.ctype(), ContainerType::Note),
        Element::Partial(PartialElement::WikidotBlockOpen(shell)) => {
            matches!(shell, crate::tree::WikidotBlockShell::Note { .. })
        }
        _ => false,
    };
    match elements {
        Elements::Single(element) => is_note(element),
        Elements::Multiple(elements) => elements.iter().any(is_note),
        Elements::None => false,
    }
}

fn elements_open_code(elements: &Elements<'_>) -> bool {
    let is_code = |element: &Element<'_>| matches!(element, Element::Code { .. });
    match elements {
        Elements::Single(element) => is_code(element),
        Elements::Multiple(elements) => elements.iter().any(is_code),
        Elements::None => false,
    }
}

fn wikidot_scored_span_residual_opener(parser: &Parser<'_, '_>) -> bool {
    if !parser.settings().layout.legacy() || parser.current().token != Token::LeftBlock {
        return false;
    }
    let start = parser.current().span.start;
    parser
        .full_text()
        .inner()
        .get(start..)
        .and_then(|source| source.get(.."[[span_]]]".len()))
        .is_some_and(|source| source.eq_ignore_ascii_case("[[span_]]]"))
}

#[inline(never)]
fn parse_wikidot_scored_span_residual<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>>
where
    'r: 't,
{
    let opener_start = parser.current().span.start;
    parser.set_rule(RULE_BLOCK);
    parser.get_optional_space()?;
    let (name, in_head) = parser.get_wikidot_block_name_with_residual_opener()?;
    let Some(name) = name.strip_suffix('_') else {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    };
    if !name.eq_ignore_ascii_case("span") || in_head {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    parser.set_block(&BLOCK_SPAN);
    let ate_separator = parser.current().token == Token::Whitespace;
    parser.get_optional_space()?;

    (BLOCK_SPAN.parse_fn)(parser, name, opener_start, false, true, false).map(|parsed| {
        parsed.map(|elements| {
            let mut elements = match elements {
                Elements::Multiple(elements) => elements,
                Elements::Single(element) => vec![element],
                Elements::None => Vec::new(),
            };
            if let Some(Element::Partial(PartialElement::InlineSpanOpen(
                attributes,
                ..,
            ))) = elements.first_mut()
            {
                attributes.insert(WIKIDOT_SCORE_SPAN_UNWRAPPED_ATTRIBUTE, cow!(""));
            }
            let bracket = 1.min(elements.len());
            elements.insert(bracket, text!("]"));
            if ate_separator {
                elements.insert(bracket + 1, text!(" "));
            }
            Elements::Multiple(elements)
        })
    })
}

fn wikidot_literal_css_module(elements: &Elements<'_>) -> bool {
    let Elements::Single(Element::Text(text)) = elements else {
        return false;
    };
    let Some(prefix) = text.get(.."[[module CSS".len()) else {
        return false;
    };
    prefix.eq_ignore_ascii_case("[[module CSS")
}

fn block_rule_enabled(parser: &Parser<'_, '_>, block_rule: &BlockRule) -> bool {
    if block_rule.name == BLOCK_EMBED_VIDEO.name
        || block_rule.name == BLOCK_EMBED_AUDIO.name
    {
        return parser.settings().layout.legacy();
    }
    block_rule.name != BLOCK_HTML.name
        || parser.settings().enable_html_blocks
        || parser.settings().layout.legacy()
}

fn wikidot_space_only_head_fails<'r, 't>(
    parser: &Parser<'r, 't>,
    block_name: &str,
    opener_start: usize,
    flag_star: bool,
) -> bool
where
    'r: 't,
{
    let single_separator = matches!(
        block_name,
        "block-span"
            | "block-anchor"
            | "block-list-unordered"
            | "block-list-ordered"
            | "block-list-item"
            | "block-bibliography"
            | "block-user"
            | "block-footnote-block"
            | "block-ruby"
    );
    let any_whitespace = matches!(
        block_name,
        "block-footnote"
            | "block-note"
            | "block-align-left"
            | "block-align-right"
            | "block-align-center"
            | "block-align-justify"
    );
    if !single_separator && !any_whitespace {
        return false;
    }
    match parser.wikidot_head_space_region(opener_start, flag_star) {
        WikidotHeadSpaceRegion::SingleSeparator => true,
        WikidotHeadSpaceRegion::WhitespaceOnly => any_whitespace,
        WikidotHeadSpaceRegion::Other => false,
    }
}

fn wikidot_note_head_with_arguments_fails<'r, 't>(
    parser: &Parser<'r, 't>,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
) -> bool
where
    'r: 't,
{
    if flag_star || flag_score {
        return false;
    }
    matches!(
        parser.wikidot_head_region_is_empty(opener_start),
        Some(false)
    )
}

fn wikidot_scored_div_body_suppresses_block(
    parser: &Parser<'_, '_>,
    block_name: &str,
    opener_start: usize,
) -> bool {
    if matches!(
        block_name,
        "block-align-left"
            | "block-align-right"
            | "block-align-center"
            | "block-align-justify"
    ) {
        return parser.wikidot_in_scored_div_direct_body();
    }
    if matches!(block_name, "block-tabview" | "block-tab" | "block-gallery") {
        return parser.wikidot_scored_div_first_line_suppresses_at(opener_start);
    }
    false
}

fn parse_block<'r, 't>(
    parser: &mut Parser<'r, 't>,
    flag_star: bool,
) -> ParseResult<'r, 't, Elements<'t>>
where
    'r: 't,
{
    let parent_rule = parser.rule();
    let opener_start = parser.current().span.start;
    let block_opener_start = match parser.current().token {
        Token::LeftBlock | Token::LeftBlockStar => opener_start,
        _ => opener_start.saturating_sub(2),
    };

    parser.set_rule(if flag_star {
        RULE_BLOCK_STAR
    } else {
        RULE_BLOCK
    });

    let spaced_name = parser
        .look_ahead(0)
        .is_some_and(|token| token.token == Token::Whitespace);
    parser.get_optional_space()?;

    let residual_block_opener = parser.settings().layout.legacy()
        && !flag_star
        && parser
            .full_text()
            .inner()
            .get(opener_start..)
            .is_some_and(|source| {
                source
                    .get(.."[[embedvideo]]]".len())
                    .is_some_and(|opener| opener.eq_ignore_ascii_case("[[embedvideo]]]"))
                    || source
                        .get(.."[[gallery]]]".len())
                        .is_some_and(|opener| opener.eq_ignore_ascii_case("[[gallery]]]"))
            });
    let (name, in_head, segment) = if residual_block_opener {
        let (name, in_head) = parser.get_wikidot_block_name_with_residual_opener()?;
        (Cow::Borrowed(name), in_head, false)
    } else {
        parser.get_block_name(flag_star)?
    };

    parser.set_head_had_segment(segment);

    let flag_score = name.ends_with('_');
    let lookup_name: Cow<'t, str> = if flag_score {
        match name {
            Cow::Borrowed(name) => Cow::Borrowed(name.strip_suffix('_').unwrap_or(name)),
            Cow::Owned(mut name) => {
                name.pop();
                Cow::Owned(name)
            }
        }
    } else {
        name
    };
    if flag_score && lookup_name.ends_with('_') {
        return Err(parser.make_err(ParseErrorKind::NoSuchBlock));
    }
    let name: &str = &lookup_name;

    let block = match block_for_head_word(
        parser,
        &lookup_name,
        matches!(lookup_name, Cow::Borrowed(_)),
        segment,
    ) {
        Some(block) => block,
        None => {
            if parser
                .fused_name_residual()
                .is_some_and(|position| position > opener_start)
            {
                parser.clear_fused_name_residual();
            }
            return Err(parser.make_err(ParseErrorKind::NoSuchBlock));
        }
    };
    if !block_rule_enabled(parser, block) {
        if parser
            .fused_name_residual()
            .is_some_and(|position| position > opener_start)
        {
            parser.clear_fused_name_residual();
        }
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.settings().layout.legacy()
        && matches!(
            block.name,
            "block-tabview"
                | "block-align-left"
                | "block-align-right"
                | "block-align-center"
                | "block-align-justify"
        )
        && parser.wikidot_scored_join_head(block_opener_start)
    {
        return Err(parser.make_err(ParseErrorKind::NotStartOfLine));
    }
    if parser.settings().layout.legacy()
        && matches!(
            block.name,
            "block-math"
                | "block-div"
                | "block-table"
                | "block-tabview"
                | "block-note"
                | "block-align-left"
                | "block-align-right"
                | "block-align-center"
                | "block-align-justify"
        )
        && parser.fused_name_residual().is_some_and(|position| {
            position > opener_start && position <= parser.current().span.start
        })
    {
        parser.clear_fused_name_residual();
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.settings().layout.legacy()
        && spaced_name
        && matches!(
            block.name,
            "block-bibliography"
                | "block-button"
                | "block-date"
                | "block-equation-ref"
                | "block-file"
                | "block-footnote"
                | "block-gallery"
                | "block-iframe"
                | "block-image"
                | "block-math"
                | "block-size"
                | "block-toc"
        )
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.settings().layout.legacy()
        && block.name == "block-embedvideo"
        && spaced_name
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.settings().layout.legacy()
        && !parser.discarding_hidden_body()
        && block.name == "block-tab"
        && !parser.accepts_partial_here(crate::tree::AcceptsPartial::Tab)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    // A view head inside a tabview body stays literal: the body only forms tab
    // structure, and a nested view would overlap the outer one
    // (tabview/tab-stray-fails keeps the inner opener as content).
    if parser.settings().layout.legacy()
        && !parser.discarding_hidden_body()
        && block.name == "block-tabview"
        && parser.accepts_partial_here(crate::tree::AcceptsPartial::Tab)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.settings().layout.legacy()
        && !parser.discarding_hidden_body()
        && matches!(block.name, "block-table-cell-regular" | "block-table-cell-header")
        && parser.in_wikidot_advanced_table_cell_body()
        && !parser.accepts_partial_here(crate::tree::AcceptsPartial::TableCell)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.settings().layout.legacy()
        && !parser.discarding_hidden_body()
        && !wikidot_block_has_physical_line_ownership(
            parser,
            parent_rule,
            block,
            opener_start,
        )
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.settings().layout.legacy()
        && !parser.discarding_hidden_body()
        && parent_rule.name() == "list"
        && matches!(block.name, "block-div" | "block-collapsible")
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.settings().layout.legacy()
        && !parser.discarding_hidden_body()
        && block.name == "block-collapsible"
        && (spaced_name
            || parent_rule.name() == "block-collapsible"
            || parser.in_wikidot_collapsible())
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.settings().layout.legacy()
        && !parser.discarding_hidden_body()
        && block.name == "block-user"
        && spaced_name
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if wikidot_scored_div_body_suppresses_block(parser, block.name, block_opener_start) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if parser.settings().layout.legacy()
        && in_head
        && wikidot_space_only_head_fails(
            parser,
            block.name,
            block_opener_start,
            flag_star,
        )
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if block.name == "block-note"
        && parser.settings().layout.legacy()
        && in_head
        && !parser.discarding_hidden_body()
        && wikidot_note_head_with_arguments_fails(
            parser,
            block_opener_start,
            flag_star,
            flag_score,
        )
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    parser.set_block(block);

    if !block.accepts_star && flag_star {
        return Err(parser.make_err(ParseErrorKind::BlockDisallowsStar));
    }

    if !block.accepts_score && flag_score {
        return Err(parser.make_err(ParseErrorKind::BlockDisallowsScore));
    }

    // The Wikidot [[html]] body is raw source captured from the byte after the
    // opener, so the space that follows a head-less opener belongs to the body
    // and must not be consumed by the generic block-head spacing rule.
    let raw_opener_body = parser.settings().layout.legacy() && block.name == "block-html";
    if !raw_opener_body {
        parser.get_optional_space()?;
    }

    if parser.settings().layout.legacy()
        && parser.in_wikidot_bibliography_body()
        && (bibliography_order::block_rule_cannot_form_in_bibliography_body(block.name)
            || block.name == "block-html")
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let memoization = BlockFailureMemoization::for_block(parser, block);
    if let Some(error) = memoization.cached_failure(parser, block, opener_start) {
        return Err(error);
    }

    let mut result = (block.parse_fn)(
        parser,
        name,
        block_opener_start,
        flag_star,
        flag_score,
        in_head,
    );
    if result.is_ok()
        && matches!(
            block.name,
            "block-math"
                | "block-div"
                | "block-table"
                | "block-tabview"
                | "block-note"
                | "block-align-left"
                | "block-align-right"
                | "block-align-center"
                | "block-align-justify"
        )
        && let Some(position) = parser.fused_name_residual()
        && position > opener_start
    {
        let source = parser.full_text().inner();
        let line_start = source[..opener_start]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        if position >= line_start && !source[line_start..position].contains('\n') {
            parser.clear_fused_name_residual();
            result = Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
    }
    if result.is_ok()
        && let Some(position) = parser.fused_name_residual()
        && position > opener_start
        && position <= parser.current().span.start
    {
        parser.clear_fused_name_residual();
        result = result.map(|parsed| {
            parsed.map(|elements| {
                let mut elements = match elements {
                    Elements::Multiple(elements) => elements,
                    Elements::Single(element) => vec![element],
                    Elements::None => Vec::new(),
                };
                elements.push(text!("]"));
                Elements::Multiple(elements)
            })
        });
    }
    if result.is_err()
        && parser
            .fused_name_residual()
            .is_some_and(|position| position > opener_start)
    {
        parser.clear_fused_name_residual();
    }
    if let Err(error) = &result {
        memoization.record_failure(parser, block, opener_start, error);
    }
    #[cfg(test)]
    if result.is_ok() {
        crate::parsing::parser::block_pairs::record_formed_block(
            block.name,
            block_opener_start,
            parser.current().span.start,
        );
    }
    result
}

#[derive(Clone, Copy)]
struct BlockFailureMemoization {
    wikijump_deterministic: bool,
    wikidot_tabview: bool,
    wikidot_collapsible: bool,
    depth: usize,
}

impl BlockFailureMemoization {
    fn for_block(parser: &Parser<'_, '_>, block: &BlockRule) -> Self {
        let page_mode = parser.settings().mode == WikitextMode::Page;
        let visible_body = !parser.discarding_hidden_body();
        let wikidot_cache_context = parser.settings().layout.legacy()
            && page_mode
            && visible_body
            && !parser.in_footnote()
            && parser.native_blockquote_depth().is_none()
            && !parser.in_wikidot_center_body();
        BlockFailureMemoization {
            wikijump_deterministic: !parser.settings().layout.legacy() && visible_body,
            wikidot_tabview: wikidot_cache_context && block.name == BLOCK_TABVIEW.name,
            wikidot_collapsible: wikidot_cache_context
                && block.name == BLOCK_COLLAPSIBLE.name
                && !parser.in_wikidot_collapsible(),
            depth: parser.depth(),
        }
    }

    fn cached_failure(
        &self,
        parser: &Parser<'_, '_>,
        block: &BlockRule,
        opener_start: usize,
    ) -> Option<ParseError> {
        if self.wikijump_deterministic
            && let Some(error) =
                parser.deterministic_block_failure(block.name, opener_start)
        {
            return Some(error);
        }
        if self.wikijump_deterministic
            && let Some(error) =
                parser.recursion_depth_block_failure(block.name, opener_start)
        {
            return Some(error);
        }
        if self.wikidot_tabview
            && let Some(error) = parser.wikidot_tabview_failure(opener_start)
        {
            return Some(error);
        }
        if self.wikidot_collapsible
            && let Some(error) = parser.wikidot_collapsible_failure(opener_start)
        {
            return Some(error);
        }
        None
    }

    fn record_failure(
        &self,
        parser: &mut Parser<'_, '_>,
        block: &BlockRule,
        opener_start: usize,
        error: &ParseError,
    ) {
        let deterministic_failure = error.kind() == ParseErrorKind::EndOfInput
            || block.name == "block-table-row"
                && error.kind() == ParseErrorKind::TableRowContainsNonCell
            || block.name == BLOCK_TABVIEW.name
                && matches!(
                    error.kind(),
                    ParseErrorKind::TabViewEmpty | ParseErrorKind::TabViewContainsNonTab
                );
        if self.wikijump_deterministic && deterministic_failure {
            parser.cache_deterministic_block_failure(block.name, opener_start, error);
        }
        if self.wikijump_deterministic
            && error.kind() == ParseErrorKind::RecursionDepthExceeded
        {
            parser.cache_recursion_depth_block_failure(
                block.name,
                opener_start,
                self.depth,
                error,
            );
        }
        if self.wikidot_tabview && deterministic_failure {
            parser.cache_wikidot_tabview_failure(opener_start, error);
        }
        if self.wikidot_collapsible && error.kind() == ParseErrorKind::EndOfInput {
            parser.cache_wikidot_collapsible_failure(opener_start, error);
        }
    }
}

fn wikidot_block_has_physical_line_ownership(
    parser: &Parser<'_, '_>,
    parent_rule: Rule,
    block: &BlockRule,
    opener_start: usize,
) -> bool {
    let in_footnote = parser.in_footnote() || parent_rule.name() == "block-footnote";
    if parser.settings().mode == WikitextMode::List
        || parser.in_native_blockquote_line()
        || (in_footnote && block.name != "block-tabview")
    {
        return true;
    }

    let needs_line_owner = matches!(
        block.name,
        "block-code"
            | "block-math"
            | "block-module"
            | "block-bibliography"
            | "block-tabview"
            | "block-toc"
    );
    if !needs_line_owner {
        return true;
    }

    let source = parser.full_text().inner().as_bytes();
    opener_start == 0 || source.get(opener_start - 1) == Some(&b'\n')
}
