/*
 * parsing/rule/mapping.rs
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

use super::{Rule, impls::*};
use crate::parsing::token::{ExtractedToken, Token};

const LINE_BREAK_RULES: &[Rule] = &[
    RULE_BLOCK_SKIP_NEWLINE,
    RULE_DEFINITION_LIST_SKIP_NEWLINE,
    RULE_LINE_BREAK,
];

#[inline]
pub fn get_rules_for_token(current: &ExtractedToken) -> &'static [Rule] {
    match current.token {
        Token::LeftBracket => &[RULE_LINK_SINGLE, RULE_TEXT],
        Token::LeftBracketAnchor => &[RULE_LINK_ANCHOR],
        Token::LeftBracketStar => &[RULE_LINK_SINGLE_NEW_TAB],
        Token::RightBracket => &[RULE_TEXT],
        Token::LeftBlock => &[RULE_BLOCK],
        Token::LeftBlockEnd => &[],
        Token::LeftBlockAnchor => &[RULE_DELAYED_CONDITIONAL, RULE_ANCHOR],
        Token::LeftBlockStar => &[RULE_BLOCK_STAR],
        Token::RightBlock => &[],
        Token::LeftParentheses => &[RULE_BIBCITE, RULE_TEXT],
        Token::RightParentheses => &[RULE_TEXT],
        Token::LeftMath => &[RULE_MATH],
        Token::RightMath => &[],
        Token::DoubleDash => &[RULE_STRIKETHROUGH_DASH, RULE_DASH],
        Token::TripleDash => &[RULE_HORIZONTAL_RULE, RULE_DASH_RUN],
        Token::DoubleTilde => &[RULE_STRIKETHROUGH_TILDE],
        Token::LeftDoubleAngle => &[RULE_DOUBLE_ANGLE],
        Token::ClearFloatBoth => &[RULE_CLEAR_FLOAT],
        Token::ClearFloatLeft => &[RULE_CLEAR_FLOAT],
        Token::ClearFloatRight => &[RULE_CLEAR_FLOAT],
        Token::Pipe => &[RULE_TEXT],
        Token::Equals => &[RULE_SECTION_MARKER, RULE_CENTER, RULE_TEXT],
        Token::Colon => &[RULE_DEFINITION_LIST, RULE_TEXT],
        Token::Underscore => &[RULE_UNDERSCORE_LINE_BREAK, RULE_TEXT],
        Token::Quote => &[RULE_BLOCKQUOTE, RULE_DOUBLE_ANGLE, RULE_TEXT],
        Token::Heading => &[RULE_HEADER, RULE_TEXT],
        Token::Whitespace => &[
            RULE_SECTION_MARKER,
            RULE_UNDERSCORE_LINE_BREAK,
            RULE_LIST,
            RULE_CENTER,
            RULE_TEXT,
        ],

        Token::Bold => &[RULE_BOLD],
        Token::Italics => &[RULE_ITALICS],
        Token::Underline => &[RULE_UNDERLINE],
        Token::Superscript => &[RULE_SUPERSCRIPT],
        Token::Subscript => &[RULE_SUBSCRIPT],
        Token::LeftMonospace => &[RULE_MONOSPACE],
        Token::RightMonospace => &[],
        Token::Color => &[RULE_COLOR],
        Token::Raw => &[RULE_RAW],
        Token::LeftRaw => &[RULE_RAW],
        Token::RightRaw => &[],

        Token::BulletItem => &[RULE_LIST, RULE_TEXT],
        Token::NumberedItem => &[RULE_LIST, RULE_TEXT],

        Token::LeftLink => &[RULE_LINK_TRIPLE],
        Token::LeftLinkStar => &[RULE_LINK_TRIPLE_NEW_TAB],
        Token::RightLink => &[],

        Token::TableColumn => &[RULE_TABLE],
        Token::TableColumnRight => &[RULE_TABLE],
        Token::TableColumnCenter => &[RULE_TABLE],
        Token::TableColumnTitle => &[RULE_TABLE],

        Token::Identifier => &[RULE_TEXT],
        Token::Email => &[RULE_EMAIL],
        Token::Url => &[RULE_URL],
        Token::Variable => &[RULE_VARIABLE, RULE_TEXT],
        Token::DoubleQuote => &[RULE_TEXT],
        Token::EscapedDoubleQuote => &[RULE_TEXT],
        Token::EscapedBackslash => &[RULE_TEXT],
        Token::DiscardedControl => &[RULE_TEXT],
        Token::GeneratedPageLink | Token::GeneratedTagLinks => &[RULE_DELAYED],

        Token::LineBreak => LINE_BREAK_RULES,
        Token::ParagraphBreak => &[RULE_LINE_BREAK_PARAGRAPH],
        Token::InputStart => &[RULE_NULL],
        Token::InputEnd => &[RULE_NULL],

        Token::LeftComment => &[RULE_COMMENT],

        Token::Other if current.slice == "&" => &[RULE_ENTITY, RULE_TEXT],
        Token::RuntimeText | Token::Other => &[RULE_TEXT],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule_names_for_token(token: Token) -> Vec<&'static str> {
        let extracted = ExtractedToken {
            token,
            slice: "",
            span: 0..0,
        };

        get_rules_for_token(&extracted)
            .iter()
            .map(|rule| rule.name())
            .collect()
    }

    #[test]
    fn rule_mapping_preserves_token_rule_order() {
        let cases = [
            (Token::LeftBracket, vec!["link-single", "text"]),
            (Token::LeftBracketAnchor, vec!["link-anchor"]),
            (Token::LeftBracketStar, vec!["link-single-new-tab"]),
            (Token::RightBracket, vec!["text"]),
            (Token::LeftBlock, vec!["block"]),
            (Token::LeftBlockEnd, vec![]),
            (
                Token::LeftBlockAnchor,
                vec!["delayed-conditional", "anchor"],
            ),
            (Token::LeftBlockStar, vec!["block-star"]),
            (Token::LeftMath, vec!["math"]),
            (Token::LeftParentheses, vec!["bibcite", "text"]),
            (Token::RightBlock, vec![]),
            (Token::RightMath, vec![]),
            (Token::RightParentheses, vec!["text"]),
            (Token::DoubleDash, vec!["strikethrough-dash", "dash"]),
            (Token::TripleDash, vec!["horizontal-rule", "dash-run"]),
            (Token::DoubleTilde, vec!["strikethrough-tilde"]),
            (Token::LeftDoubleAngle, vec!["double-angle"]),
            (Token::ClearFloatBoth, vec!["clear-float"]),
            (Token::ClearFloatLeft, vec!["clear-float"]),
            (Token::ClearFloatRight, vec!["clear-float"]),
            (Token::Pipe, vec!["text"]),
            (Token::Equals, vec!["section-marker", "center", "text"]),
            (Token::Colon, vec!["definition-list", "text"]),
            (Token::Underscore, vec!["underscore-line-break", "text"]),
            (Token::Quote, vec!["blockquote", "double-angle", "text"]),
            (Token::Heading, vec!["header", "text"]),
            (
                Token::LineBreak,
                vec!["block-skip", "definition-list-skip-newline", "line-break"],
            ),
            (Token::ParagraphBreak, vec!["line-break-paragraph"]),
            (
                Token::Whitespace,
                vec![
                    "section-marker",
                    "underscore-line-break",
                    "list",
                    "center",
                    "text",
                ],
            ),
            (Token::Bold, vec!["bold"]),
            (Token::Italics, vec!["italics"]),
            (Token::Underline, vec!["underline"]),
            (Token::Superscript, vec!["superscript"]),
            (Token::Subscript, vec!["subscript"]),
            (Token::LeftMonospace, vec!["monospace"]),
            (Token::RightMonospace, vec![]),
            (Token::Color, vec!["color"]),
            (Token::Raw, vec!["raw"]),
            (Token::LeftRaw, vec!["raw"]),
            (Token::RightRaw, vec![]),
            (Token::BulletItem, vec!["list", "text"]),
            (Token::NumberedItem, vec!["list", "text"]),
            (Token::LeftLink, vec!["link-triple"]),
            (Token::LeftLinkStar, vec!["link-triple-new-tab"]),
            (Token::RightLink, vec![]),
            (Token::TableColumn, vec!["table"]),
            (Token::TableColumnRight, vec!["table"]),
            (Token::TableColumnCenter, vec!["table"]),
            (Token::TableColumnTitle, vec!["table"]),
            (Token::Identifier, vec!["text"]),
            (Token::Email, vec!["email"]),
            (Token::Url, vec!["url"]),
            (Token::Variable, vec!["variable", "text"]),
            (Token::DoubleQuote, vec!["text"]),
            (Token::EscapedDoubleQuote, vec!["text"]),
            (Token::EscapedBackslash, vec!["text"]),
            (Token::DiscardedControl, vec!["text"]),
            (Token::RuntimeText, vec!["text"]),
            (Token::GeneratedPageLink, vec!["delayed"]),
            (Token::GeneratedTagLinks, vec!["delayed"]),
            (Token::LeftComment, vec!["comment"]),
            (Token::InputStart, vec!["null"]),
            (Token::InputEnd, vec!["null"]),
            (Token::Other, vec!["text"]),
        ];

        for (token, expected) in cases {
            assert_eq!(rule_names_for_token(token), expected, "{token:?}");
        }
    }
}
