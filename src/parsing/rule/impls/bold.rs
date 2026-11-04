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

use super::inline_delimiter::{assert_unpadded_open, validate_reachable_close};
use super::prelude::*;
use crate::parsing::ParseSuccess;
use crate::parsing::collect::{consume_iftags_segment, consume_valid_comment};
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
    validate_reachable_close(parser, Token::Bold)?;
    assert_unpadded_open(parser, Token::Bold)?;
    let close = [ParseCondition::current(Token::Bold)];
    let invalid = [
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::token_pair(Token::Bold, Token::Whitespace),
        ParseCondition::token_pair(Token::Whitespace, Token::Bold),
    ];
    let ctype = ContainerType::Bold;
    let collected = collect_container(parser, RULE_BOLD, ctype, &close, &invalid, None)?;
    finish_collected(parser, collected)
}

/// The two `*`s of a bold opener can be split by an iftags segment
/// (`*[[iftags -zzz]][[/iftags]]*word**`): live Wikidot resolves the
/// segment before the delimiter forms, so the pair still opens bold.
/// The split opener only forms when every segment between the markers
/// resolves to no text; a segment whose body would be kept puts real
/// text between the markers, which is not a bold opener on live either.
fn try_consume_split_delimiter<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    let mut probe = parser.clone();
    probe.step()?; // first `*`
    let mut segments = 0usize;
    loop {
        if probe.current().token == Token::LeftBlock {
            let Some(segment) = consume_iftags_segment(&mut probe)? else {
                break;
            };
            if segment.include_body
                && !probe.full_text().inner()[segment.body.clone()].is_empty()
            {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            segments += 1;
        } else if probe.current().token == Token::LeftComment {
            let mut comment = probe.clone();
            if consume_valid_comment(&mut comment).is_err() {
                break;
            }
            probe.update(&comment);
            segments += 1;
        } else {
            break;
        }
    }
    if segments == 0 || probe.current().token != Token::BulletItem {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    validate_reachable_close(parser, Token::Bold)?;
    parser.step()?; // first `*`
    loop {
        if parser.current().token == Token::LeftBlock
            && consume_iftags_segment(parser)?.is_some()
        {
            continue;
        }
        if parser.current().token == Token::LeftComment {
            let mut comment = parser.clone();
            if consume_valid_comment(&mut comment).is_ok() {
                parser.update(&comment);
                continue;
            }
        }
        break;
    }
    assert_step(parser, Token::BulletItem)?;
    if parser.current().token == Token::Whitespace {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let close = [ParseCondition::current(Token::Bold)];
    let invalid = [
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::token_pair(Token::Bold, Token::Whitespace),
        ParseCondition::token_pair(Token::Whitespace, Token::Bold),
    ];
    let collected = collect_container(
        parser,
        RULE_BOLD,
        ContainerType::Bold,
        &close,
        &invalid,
        None,
    )?;
    finish_collected(parser, collected)
}

fn finish_collected<'r, 't>(
    parser: &mut Parser<'r, 't>,
    collected: ParseSuccess<Elements<'t>>,
) -> ParseResult<'r, 't, Elements<'t>> {
    let (elements, errors, paragraph_safe) = collected.into();
    if parser.settings().layout.legacy()
        && matches!(
            &elements,
            Elements::Single(Element::Container(container)) if container.elements().is_empty()
        )
    {
        return if super::footnote_first::starts_footnote(parser) {
            ok!(
                paragraph_safe;
                Element::Partial(PartialElement::WikidotEmptyInlineOwner),
                errors,
            )
        } else {
            ok!(paragraph_safe; Elements::None, errors)
        };
    }
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
