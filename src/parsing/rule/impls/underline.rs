/*
 * parsing/rule/impls/underline.rs
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
    ReachableCloseMode, append_wikidot_line_close_space, assert_unpadded_open,
    inline_body_padded_through_seams, legacy_format_body_contains_table,
    validate_reachable_close,
};
use super::prelude::*;

pub const RULE_UNDERLINE: Rule = Rule {
    name: "underline",
    position: LineRequirement::Any,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Trying to create underline container");
    validate_reachable_close(
        parser,
        Token::Underline,
        None,
        ReachableCloseMode::Standard,
    )?;
    assert_unpadded_open(parser, Token::Underline)?;
    let close = [ParseCondition::current(Token::Underline)];
    let invalid = [
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::token_pair(Token::Underline, Token::Whitespace),
        ParseCondition::token_pair(Token::Whitespace, Token::Underline),
    ];
    let ctype = ContainerType::Underline;
    let collected =
        collect_container(parser, RULE_UNDERLINE, ctype, &close, &invalid, None)?;
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
    if parser.settings().layout.legacy() && legacy_format_body_contains_table(&elements) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.settings().layout.legacy()
        && matches!(
            &elements,
            Elements::Single(Element::Container(container)) if container.elements().is_empty()
        )
    {
        return ok!(paragraph_safe; Elements::None, errors);
    }
    let elements = super::bold::mark_format_stop_at_list(parser, elements);
    let elements =
        append_wikidot_line_close_space(elements, parser.settings().layout.legacy());
    ok!(paragraph_safe; elements, errors)
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn wikidot_discards_complete_empty_underline_pairs_in_runs() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        for (input, expected) in [
            ("______[[/span]]", "<p>__[[/span]]</p>"),
            ("______x", "<p>__x</p>"),
            ("x________y", "<p>xy</p>"),
            ("______ ______", "<p>__ __</p>"),
        ] {
            let mut source = input.to_owned();
            crate::preprocess(&mut source);
            let tokenization = crate::tokenize(&source);
            let (tree, _errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;
            assert_eq!(html, expected, "{input:?}");
        }
    }

    #[test]
    fn wikidot_discards_long_underline_run_before_text() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = "__".repeat(4096);
        source.push('x');
        let tokenization = crate::tokenize(&source);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(html, "<p>x</p>");
    }

    #[test]
    fn wikidot_short_underline_runs_keep_odd_even_semantics() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for marker_count in 1..=12 {
            let source = format!("{}x", "__".repeat(marker_count));
            let tokenization = crate::tokenize(&source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;
            let expected = if marker_count % 2 == 0 {
                "<p>x</p>"
            } else {
                "<p>__x</p>"
            };

            assert_eq!(html, expected, "{marker_count} underline tokens");
            if marker_count % 2 == 0 {
                assert!(errors.is_empty(), "{marker_count}: {errors:#?}");
            }
        }
    }

    #[test]
    fn unmatched_underline_does_not_swallow_the_next_block() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(concat!(
            "[[collapsible show=\"A\" hide=\"A\"]]
",
            "Some text __unmatched opener\n",
            "[[/collapsible]]
",
            "[[collapsible show=\"B\" hide=\"B\"]]
",
            "__paired__ text\n",
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
        assert_eq!(
            html.matches("text-decoration: underline").count(),
            1,
            "{html}"
        );
    }

    #[test]
    fn wikidot_formatting_does_not_cross_block_boundaries() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(concat!(
            "**a\n+++ heading\nb**\n\n",
            "+++ __a\nb__\n\n",
            "__a\n+++ heading\nb__\n\n",
            "* __a\nb__\n",
        ));
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(
            html,
            concat!(
                "<p>**a</p>",
                "<h3 id=\"toc0\"><span>heading</span></h3>",
                "<p>b**</p>",
                "<h3 id=\"toc1\"><span>__a</span></h3>",
                "<p>b__</p>",
                "<p>__a</p>",
                "<h3 id=\"toc2\"><span>heading</span></h3>",
                "<p>b__</p>",
                "<ul>\n<li>__a</li>\n</ul>",
                "<p>b__</p>",
            ),
        );
    }

    #[test]
    fn wikidot_underline_still_applies_within_a_heading_or_list_line() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("+++ __a b__\n\n* __c d__\n");
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(
            html,
            concat!(
                "<h3 id=\"toc0\"><span>",
                "<span style=\"text-decoration: underline;\">a b</span>",
                "</span></h3>",
                "<ul>\n<li>",
                "<span style=\"text-decoration: underline;\">c d</span>",
                "</li>\n</ul>",
            ),
        );
    }
}
