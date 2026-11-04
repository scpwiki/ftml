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
    ReachableCloseMode, append_split_close_leftover, assert_unpadded_open,
    prepend_split_close_leftover, validate_reachable_close,
};
use super::prelude::*;
use crate::parsing::ParseSuccess;
use crate::parsing::collect::{SegmentCtx, TokenWindow, split_delimiter_end};
use crate::tree::PartialElement;

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

/// The two `*`s of a bold opener can be split by elided source text
/// (`*[[iftags -zzz]][[/iftags]]*word**`): live Wikidot resolves the
/// text before the delimiter forms, so the pair still opens bold.
/// The shared split scan decides the match on the resolved text: a
/// kept segment body contributes its text, so a body holding exactly
/// the second `*` completes the opener, and a body with other text
/// between the markers is not a bold opener on live either.
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

fn finish_collected<'r, 't>(
    parser: &mut Parser<'r, 't>,
    collected: ParseSuccess<Elements<'t>>,
) -> ParseResult<'r, 't, Elements<'t>> {
    let leftover = collected.split_close_leftover.clone();
    let (elements, errors, paragraph_safe) = collected.into();
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
        // Real shape from cn scp-cn-852-3: dialogue lines pair `**`
        // around speaker names, and a censorship `***` leaves one
        // `**` unmatched. It must not pair across the collapsible
        // boundary; Wikidot renders both collapsibles.
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
        // The segment gate passes and its body is kept, so real text
        // sits between the two s: not a bold opener, matching the
        // text-level resolution live Wikidot performs.
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
                "<p><span style=\"text-decoration: line-through;\">a<br>\n</span></p>",
                "<p><span style=\"text-decoration: line-through;\">a<br>\nb</span></p>",
                "<p>\u{2014}a<br>\nb \u{2014}</p>",
                "<p><strong>a<br>\nb</strong></p>",
            ),
        );
    }
}
