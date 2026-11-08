/*
 * parsing/rule/impls/bold.rs
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

use super::inline_delimiter::{
    ReachableCloseMode, append_split_close_leftover, append_wikidot_line_close_space,
    assert_unpadded_open, inline_body_padded_through_seams, prepend_split_close_leftover,
    validate_reachable_close,
};
use super::prelude::*;
use crate::parsing::ParseSuccess;
use crate::parsing::collect::{SegmentCtx, TokenWindow, split_delimiter_end};
use crate::tree::{Container, PartialElement, WikidotBlockCloseKind, WikidotBlockShell};
use std::borrow::Cow;

pub const RULE_BOLD: Rule = Rule {
    name: "bold",
    position: LineRequirement::Any,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Trying to create bold (strong) container");
    if parser.current().token == Token::BulletItem {
        return try_consume_split_delimiter(parser);
    }
    if let Some(after) = empty_list_bold_after(parser) {
        parser.step_to_remaining(after);
        return ok!(Elements::None);
    }
    validate_reachable_close(
        parser,
        Token::Bold,
        Some(SplitDelimiter::Bold),
        ReachableCloseMode::Standard,
    )?;
    assert_unpadded_open(parser, Token::Bold)?;
    let close = [
        ParseCondition::current(Token::Bold),
        ParseCondition::SplitClose(SplitDelimiter::Bold),
    ];
    let invalid = [
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::token_pair(Token::Bold, Token::Whitespace),
        ParseCondition::token_pair(Token::Whitespace, Token::Bold),
        ParseCondition::PaddedSplitClose(SplitDelimiter::Bold),
    ];
    let ctype = ContainerType::Bold;
    let collected = collect_container(parser, RULE_BOLD, ctype, &close, &invalid, None)?;
    finish_collected(parser, collected)
}

fn is_break_token(token: Token) -> bool {
    matches!(
        token,
        Token::LineBreak | Token::ParagraphBreak | Token::Whitespace
    )
}

fn block_head_is(
    tokens: &[ExtractedToken<'_>],
    index: usize,
    name: &str,
    closing: bool,
) -> bool {
    let open = if closing {
        Token::LeftBlockEnd
    } else {
        Token::LeftBlock
    };
    tokens.get(index).is_some_and(|token| token.token == open)
        && tokens.get(index + 1).is_some_and(|token| {
            token.token == Token::Identifier && token.slice.eq_ignore_ascii_case(name)
        })
        && tokens
            .get(index + 2)
            .is_some_and(|token| token.token == Token::RightBlock)
}

fn skip_breaks(tokens: &[ExtractedToken<'_>], mut index: usize) -> usize {
    while tokens
        .get(index)
        .is_some_and(|token| is_break_token(token.token))
    {
        index += 1;
    }
    index
}

/// Classify the gap after a stopped pair's closer for the understrike tail
/// (see `mark_format_stop_at_list`). Closers of enclosing crossing pairs
/// (`**__` after the list, at any nesting depth) sit between the stopped
/// closer and the enclosing scope's close, so they are transparent to the
/// scan. `LineBreak` then the closer's scope block yields the break tail;
/// adjacent content yields the bare tail; anything else yields no tail.
fn stopped_tail_marker(
    current: &ExtractedToken<'_>,
    rest: &[ExtractedToken<'_>],
) -> Option<&'static str> {
    let mut cursor = 0usize;
    loop {
        let token: &ExtractedToken<'_> = if cursor == 0 {
            current
        } else {
            rest.get(cursor - 1)?
        };
        cursor += 1;
        match token.token {
            Token::LineBreak => {
                if block_head_is(rest, cursor - 1, "span", true)
                    || block_head_is(rest, cursor - 1, "size", true)
                    || block_head_is(rest, cursor - 1, "a", true)
                {
                    return Some(
                        crate::tree::WIKIDOT_UNDERSTRIKE_LIST_CLOSE_TAIL_BREAK_MARKER,
                    );
                }
                return None;
            }
            Token::RuntimeText | Token::Identifier if !token.slice.trim().is_empty() => {
                return Some(
                    crate::tree::WIKIDOT_UNDERSTRIKE_LIST_CLOSE_TAIL_BARE_MARKER,
                );
            }
            Token::Bold
            | Token::Italics
            | Token::Underline
            | Token::DoubleDash
            | Token::Superscript
            | Token::Subscript
            | Token::LeftMonospace
            | Token::RightMonospace => continue,
            _ => return None,
        }
    }
}

fn empty_list_bold_after(parser: &Parser<'_, '_>) -> Option<usize> {
    if !parser.settings().layout.legacy() {
        return None;
    }
    if parser.current().token != Token::Bold {
        return None;
    }
    let tokens = parser.remaining();
    let list_name = if block_head_is(tokens, 0, "ul", false) {
        "ul"
    } else if block_head_is(tokens, 0, "ol", false) {
        "ol"
    } else {
        return None;
    };
    if parser.current().span.end != tokens[0].span.start {
        return None;
    }
    let mut index = skip_breaks(tokens, 3);
    let mut items = 0;
    loop {
        if !block_head_is(tokens, index, "li", false) {
            break;
        }
        items += 1;
        index += 3;
        while !block_head_is(tokens, index, "li", true) {
            let token = tokens.get(index)?;
            if !is_break_token(token.token) {
                return None;
            }
            index += 1;
        }
        index += 3;
        index = skip_breaks(tokens, index);
    }
    if items == 0 {
        return None;
    }
    if !block_head_is(tokens, index, list_name, true) {
        return None;
    }
    let close_right = index + 2;
    index += 3;
    tokens.get(index)?;
    if tokens[index].token != Token::Bold {
        return None;
    }
    if tokens[close_right].span.end != tokens[index].span.start {
        return None;
    }
    Some(index + 1)
}

fn try_consume_split_delimiter<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    let Some(split_match) = split_delimiter_end(
        TokenWindow::at(parser),
        SegmentCtx::from_parser(parser),
        SplitDelimiter::Bold,
    ) else {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    };
    validate_reachable_close(
        parser,
        Token::Bold,
        Some(SplitDelimiter::Bold),
        ReachableCloseMode::Standard,
    )?;
    while parser.current().span.start < split_match.consume_end
        && parser.current().token != Token::InputEnd
    {
        parser.step()?;
    }
    if parser.current().token == Token::Whitespace {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let close = [
        ParseCondition::current(Token::Bold),
        ParseCondition::SplitClose(SplitDelimiter::Bold),
    ];
    let invalid = [
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::token_pair(Token::Bold, Token::Whitespace),
        ParseCondition::token_pair(Token::Whitespace, Token::Bold),
        ParseCondition::PaddedSplitClose(SplitDelimiter::Bold),
    ];
    let collected = collect_container(
        parser,
        RULE_BOLD,
        ContainerType::Bold,
        &close,
        &invalid,
        None,
    )?;
    let collected = match &split_match.leftover {
        Some(range) => {
            let text = parser.full_text().inner()[range.clone()].to_owned();
            prepend_split_close_leftover(collected, &text)
        }
        None => collected,
    };
    finish_collected(parser, collected)
}

fn bold_body_crosses_flow_block(elements: &Elements<'_>) -> bool {
    let Elements::Single(Element::Container(container)) = elements else {
        return false;
    };
    container.elements().iter().any(|element| match element {
        Element::Container(inner) => {
            matches!(inner.ctype(), ContainerType::Div | ContainerType::Note)
        }
        Element::Table(_) | Element::Math { .. } | Element::Code(_) => true,
        Element::Partial(
            PartialElement::WikidotBlockOpen(WikidotBlockShell::Div { .. })
            | PartialElement::WikidotBlockClose(WikidotBlockCloseKind::Div { .. }),
        ) => true,
        _ => false,
    })
}

fn finish_collected<'r, 't>(
    parser: &mut Parser<'r, 't>,
    collected: ParseSuccess<Elements<'t>>,
) -> ParseResult<'r, 't, Elements<'t>> {
    let leftover = collected.split_close_leftover.clone();
    let (elements, errors, paragraph_safe) = collected.into();
    if parser.settings().layout.legacy()
        && matches!(
            &elements,
            Elements::Single(Element::Container(container))
                if inline_body_padded_through_seams(container.elements())
        )
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.settings().layout.legacy() && bold_body_crosses_flow_block(&elements) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.settings().layout.legacy()
        && super::inline_delimiter::legacy_format_body_code_preceded_by_content(&elements)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.settings().layout.legacy()
        && matches!(
            &elements,
            Elements::Single(Element::Container(container)) if container.elements().is_empty()
        )
    {
        let base = if super::footnote_first::starts_footnote(parser) {
            Elements::Single(Element::Partial(PartialElement::WikidotEmptyInlineOwner))
        } else {
            Elements::None
        };
        let elements = match leftover {
            Some(leftover) => append_split_close_leftover(base, leftover),
            None => base,
        };
        return ok!(paragraph_safe; elements, errors);
    }
    let elements =
        append_wikidot_line_close_space(elements, parser.settings().layout.legacy());
    let elements = match leftover {
        Some(leftover) => append_split_close_leftover(elements, leftover),
        None => elements,
    };
    let elements = super::bold::mark_format_stop_at_list(parser, elements);
    let elements = mark_span_list_close_tail(parser, elements);
    ok!(paragraph_safe; elements, errors)
}

/// A bold, italics, superscript or subscript pair that closed beyond an
/// explicit list inside a span or size body leaves two live shapes on the
/// following scope-content paragraph (spanin family). The syntax tree loses
/// the newline-count distinction
/// there, so the gap after the close marker is classified here, from the
/// token stream, and stamped on the format container as an internal marker
/// for inline-scope lowering:
/// a single newline then `[[/span]]`/`[[/size]]` (break-only tail segment
/// materialises
/// after the list) or adjacent content text (the after-list paragraph is
/// bare with no inserted leading break). At this point the parser sits on
/// the first token after the pair close.
///
/// The inverse shape (strong-x family) stamps the same markers: a span or
/// size scope inside the format pair closes beyond the explicit list (its
/// close sits in the pair's tail segment), so the paragraph after the list
/// is likewise bare — with a leading break when the close is followed by a
/// newline (`<br>[[/hidden]]`), and no leading break when content follows
/// the pair close directly (`tail<br>...`).
pub(super) fn mark_span_list_close_tail<'r, 't>(
    parser: &Parser<'r, 't>,
    mut elements: Elements<'t>,
) -> Elements<'t> {
    if !parser.settings().layout.legacy() {
        return elements;
    }
    let in_span_body = parser.in_wikidot_span_body();
    let in_size_body = parser.in_wikidot_size_body();
    let in_inline_scope_body = in_span_body || in_size_body;
    let Elements::Single(Element::Container(container)) = &elements else {
        return elements;
    };
    if !matches!(
        container.ctype(),
        ContainerType::Bold
            | ContainerType::Italics
            | ContainerType::Superscript
            | ContainerType::Subscript
    ) {
        return elements;
    }
    let crosses_explicit_list = container
        .elements()
        .iter()
        .any(crate::parsing::inline_scope::subtree_contains_explicit_list);
    if !crosses_explicit_list {
        return elements;
    }
    if container
        .elements()
        .iter()
        .any(|child| crate::parsing::inline_scope::subtree_contains_flow_block(child))
    {
        return elements;
    }
    let current = parser.current();
    let rest = parser.remaining();
    let marker = if in_inline_scope_body {
        if current.token == Token::LineBreak
            && (block_head_is(rest, 0, "span", true)
                || block_head_is(rest, 0, "size", true))
        {
            crate::tree::WIKIDOT_SPAN_LIST_CLOSE_TAIL_BREAK_MARKER
        } else if matches!(current.token, Token::RuntimeText | Token::Identifier)
            && !current.slice.trim().is_empty()
        {
            crate::tree::WIKIDOT_SPAN_LIST_CLOSE_TAIL_BARE_MARKER
        } else {
            return elements;
        }
    } else {
        if !crate::parsing::inline_scope::label_scope_straddles_list(container.elements())
        {
            return elements;
        }
        if current.token == Token::LineBreak {
            crate::tree::WIKIDOT_FORMAT_LABEL_LIST_CLOSE_TAIL_BREAK_MARKER
        } else if matches!(current.token, Token::RuntimeText | Token::Identifier)
            && !current.slice.trim().is_empty()
        {
            crate::tree::WIKIDOT_FORMAT_LABEL_LIST_CLOSE_TAIL_BARE_MARKER
        } else if current.token == Token::Whitespace
            && rest.first().is_some_and(|next| {
                matches!(next.token, Token::RuntimeText | Token::Identifier)
                    && !next.slice.trim().is_empty()
            })
        {
            crate::tree::WIKIDOT_FORMAT_LABEL_LIST_CLOSE_TAIL_BARE_MARKER
        } else {
            return elements;
        }
    };
    let Elements::Single(Element::Container(container)) = &mut elements else {
        unreachable!("single bold element checked above")
    };
    container.attributes_mut().insert(marker, Cow::Borrowed(""));
    elements
}

/// An underline or strikethrough pair that closed beyond an explicit list
/// inside a Wikidot inline scope (`[[span]]`, `[[size]]`, `[[a]]`) does not
/// carry its styling across the list on live: only the segment before the
/// list keeps its styling, the list and the post-list segment are unstyled,
/// and both markers are consumed. (Bold, italics, superscript and subscript
/// pair across the list there, and underline/strike pair across it outside
/// an inline scope and inside `[[div]]`.) Stamp the collected container so
/// inline-scope lowering segments it that way, plus its post-list tail
/// shape. The tail classification is the same token-after-close rule for
/// post-list segments with and without content (a content-bearing segment
/// is the fold shape: live closes the enclosing scope right after the
/// post-list segment); the FOLD arm is the fallback for a close marker
/// followed by something other than a newline+scope-close or adjacent text
/// (e.g. `b__[[/span]]` on one line). At this point the parser sits on
/// the first token after the close.
pub(super) fn mark_format_stop_at_list<'r, 't>(
    parser: &Parser<'r, 't>,
    mut elements: Elements<'t>,
) -> Elements<'t> {
    if !parser.settings().layout.legacy()
        || !(parser.in_wikidot_span_body()
            || parser.in_wikidot_size_body()
            || parser.in_wikidot_anchor_body())
    {
        return elements;
    }
    let Some(container) = find_stop_at_list_container(&elements) else {
        return elements;
    };
    // Bold/italics pairs do not carry their styling across an explicit
    // list inside an anchor body on live (li items and the post segment
    // render unstyled; the pre segment keeps styling): the pair forms a
    // stopped pair there, unlike in span/size/free scopes where it crosses
    // normally. Probed live (frozen ci2-anchor-pre-b; fresh
    // muse-probe-anchorpre-boldanchorattr-v2/italicsspan-verified twins).
    // Superscript/subscript/monospace remain unprobed for anchor bodies
    // and keep the crossing behaviour.
    let in_anchor = parser.in_wikidot_anchor_body();
    if !match container.ctype() {
        ContainerType::Underline | ContainerType::Strikethrough => true,
        ContainerType::Bold | ContainerType::Italics => in_anchor,
        _ => false,
    } {
        return elements;
    }
    // A list nested inside an inner format pair (the nested stopped-pair
    // shapes, e.g. `__**[[ul]]...**__`) crosses the outer pair too; the
    // child segmentation pass at lowering turns it into a direct child
    // before the stopped-pair path runs. The rule-exit check must see past
    // direct children.
    let crosses_list = container
        .elements()
        .iter()
        .any(crate::parsing::inline_scope::subtree_contains_explicit_list);
    if !crosses_list
        || container.elements().iter().any(|child| {
            crate::parsing::inline_scope::subtree_contains_flow_block(child)
                || crate::parsing::inline_scope::subtree_contains_native_list(child)
        })
    {
        return elements;
    }
    // Content between the last explicit list (at any depth) and the pair's
    // closer; a nested inner pair's list is out of direct-children reach,
    // so the post-list check looks past direct children.
    let post_has_content =
        crate::parsing::inline_scope::post_list_subtree_has_content(container.elements());
    let current = parser.current();
    let rest = parser.remaining();
    // The gap after the stopped pair's closer: a line break then the
    // enclosing scope's close block yields a bare break tail; adjacent
    // content yields a bare content tail. Markers of enclosing crossing
    // formats (e.g. `**__` after the list in a nested stopped pair) sit
    // between this closer and the gap, so they are transparent to the
    // classification. A content-bearing post-list segment (the fold shape)
    // stamps the same classification; the FOLD marker is the fallback for a
    // close marker followed by something other than a newline+scope-close
    // or adjacent text (e.g. `b__[[/span]]` on one line): live closes the
    // enclosing scope right after the post-list segment.
    let tail_marker = if post_has_content {
        match stopped_tail_marker(current, rest) {
            Some(marker) => Some(marker),
            None => Some(crate::tree::WIKIDOT_UNDERSTRIKE_LIST_CLOSE_TAIL_FOLD_MARKER),
        }
    } else {
        stopped_tail_marker(current, rest)
    };
    let Some(container) = find_stop_at_list_container_mut(&mut elements) else {
        unreachable!("stop-at-list candidate checked above")
    };
    container.attributes_mut().insert(
        crate::tree::WIKIDOT_FORMAT_STOPS_AT_LIST_MARKER,
        Cow::Borrowed(""),
    );
    if let Some(marker) = tail_marker {
        container.attributes_mut().insert(marker, Cow::Borrowed(""));
    }
    elements
}

/// The format container a stop-at-list verdict applies to, whether it came
/// back from the collector as a single element or in a Multiple alongside
/// line-close artefacts (line-start `**` split-close leftovers). Only the
/// format kinds eligible for the stop are considered.
fn find_stop_at_list_container<'e, 't>(
    elements: &'e Elements<'t>,
) -> Option<&'e Container<'t>> {
    fn candidate<'c, 't>(element: &'c Element<'t>) -> Option<&'c Container<'t>> {
        let Element::Container(container) = element else {
            return None;
        };
        matches!(
            container.ctype(),
            ContainerType::Underline
                | ContainerType::Strikethrough
                | ContainerType::Bold
                | ContainerType::Italics
        )
        .then_some(container)
    }
    match elements {
        Elements::Single(element) => candidate(element),
        Elements::Multiple(vec) => vec.iter().find_map(candidate),
        Elements::None => None,
    }
}

fn find_stop_at_list_container_mut<'e, 't>(
    elements: &'e mut Elements<'t>,
) -> Option<&'e mut Container<'t>> {
    fn candidate_mut<'c, 't>(
        element: &'c mut Element<'t>,
    ) -> Option<&'c mut Container<'t>> {
        let Element::Container(container) = element else {
            return None;
        };
        matches!(
            container.ctype(),
            ContainerType::Underline
                | ContainerType::Strikethrough
                | ContainerType::Bold
                | ContainerType::Italics
        )
        .then_some(container)
    }
    match elements {
        Elements::Single(element) => candidate_mut(element),
        Elements::Multiple(vec) => vec.iter_mut().find_map(candidate_mut),
        Elements::None => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn unmatched_bold_does_not_swallow_the_next_block() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(concat!(
            "[[collapsible show=\"A\" hide=\"A\"]]
",
            "**C6：**嗯，好的***的不得了。
",
            "[[/collapsible]]
",
            "[[collapsible show=\"B\" hide=\"B\"]]
",
            "**C6：**又是你，我真***的开心。
",
            "[[/collapsible]]
",
        ));
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(
            html.matches("collapsible-block-folded").count(),
            2,
            "{html}"
        );
        assert_eq!(html.matches("<strong>").count(), 2, "{html}");
    }

    #[test]
    fn split_bold_opener_with_kept_segment_body_stays_literal() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("*[[iftags +tale]]YES[[/iftags]]*word**");
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(html, "<p>*YES*word**</p>");
    }

    #[test]
    fn wikidot_bold_comment_only_line_keeps_one_line_break() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("**a\n[!-- c --]\nb**");
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(html, "<p><strong>a<br>\nb</strong></p>");
    }

    #[test]
    fn wikidot_multiline_inline_formatting_matches_live_structure() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(concat!(
            "**a\nb**\n\n",
            "//a\nb//\n\n",
            "--a\nb--\n\n",
            "^^a\nb^^\n\n",
            ",,a\nb,,\n\n",
            "{{a\nb}}\n\n",
            "##c00|a\nb##\n\n",
            "__a\nb__\n\n",
            "--a\n--\n\n",
            "--a \nb--\n\n",
            "--a\nb --\n\n",
            "**a\n[!-- c --]\nb**\n",
        ));
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(
            html,
            concat!(
                "<p><strong>a<br>\nb</strong></p>",
                "<p><em>a<br>\nb</em></p>",
                "<p><span style=\"text-decoration: line-through;\">a<br>\nb</span></p>",
                "<p><sup>a<br>\nb</sup></p>",
                "<p><sub>a<br>\nb</sub></p>",
                "<p><tt>a<br>\nb</tt></p>",
                "<p><span style=\"color: #c00\">a<br>\nb</span></p>",
                "<p><span style=\"text-decoration: underline;\">a<br>\nb</span></p>",
                "<p><span style=\"text-decoration: line-through;\">a<br></span></p>",
                "<p><span style=\"text-decoration: line-through;\">a<br>\nb</span></p>",
                "<p>\u{2014}a<br>\nb \u{2014}</p>",
                "<p><strong>a<br>\nb</strong></p>",
            ),
        );
    }

    #[test]
    fn bold_across_a_note_stays_a_paragraph_of_literals() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(concat!(
            "**||[[=]]\n",
            "[[note]]\n",
            "----\n",
            "[[/note]]\n",
            "[[/=]]||**",
        ));
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(
            html.starts_with("<p>**||[[=]]</p><div class=\"wiki-note\">"),
            "{html}",
        );
        assert!(html.contains("</div><br>\n[[/=]]||**"), "{html}");
        assert!(!html.contains("<strong>"), "{html}");
    }
}
