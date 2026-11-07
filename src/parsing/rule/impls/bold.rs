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
use crate::tree::{PartialElement, WikidotBlockCloseKind, WikidotBlockShell};

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
    if let Some(after) = empty_ul_bold_after(parser) {
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

fn empty_ul_bold_after(parser: &Parser<'_, '_>) -> Option<usize> {
    if !parser.settings().layout.legacy() {
        return None;
    }
    if parser.current().token != Token::Bold {
        return None;
    }
    let tokens = parser.remaining();
    if !block_head_is(tokens, 0, "ul", false) {
        return None;
    }
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
    if !block_head_is(tokens, index, "ul", true) {
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
        Element::Table(_) => true,
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
    ok!(paragraph_safe; elements, errors)
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
