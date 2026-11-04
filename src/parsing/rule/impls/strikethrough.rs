/*
 * parsing/rule/impls/strikethrough.rs
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

pub const RULE_STRIKETHROUGH_DASH: Rule = Rule {
    name: "strikethrough-dash",
    position: LineRequirement::Any,
    try_consume_fn: dash,
};

pub const RULE_STRIKETHROUGH_TILDE: Rule = Rule {
    name: "strikethrough-tilde",
    position: LineRequirement::Any,
    try_consume_fn: tilde,
};

fn dash<'r, 't>(parser: &mut Parser<'r, 't>) -> ParseResult<'r, 't, Elements<'t>> {
    trace!("Trying to create a double dash strikethrough");
    try_consume_strikethrough(parser, RULE_STRIKETHROUGH_DASH, Token::DoubleDash)
}

fn tilde<'r, 't>(parser: &mut Parser<'r, 't>) -> ParseResult<'r, 't, Elements<'t>> {
    trace!("Trying to create a double tilde strikethrough");
    if parser.settings().layout.legacy() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    try_consume_strikethrough(parser, RULE_STRIKETHROUGH_TILDE, Token::DoubleTilde)
}

fn try_consume_strikethrough<'r, 't>(
    parser: &mut Parser<'r, 't>,
    rule: Rule,
    token: Token,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Trying to create a strikethrough (token {})", token.name());
    validate_reachable_close(parser, token, None)?;
    if token == Token::DoubleDash {
        assert_unpadded_open(parser, token)?;
    } else {
        assert_step(parser, token)?;
    }
    let close = [ParseCondition::current(token)];
    let invalid = [
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::token_pair(token, Token::Whitespace),
        ParseCondition::token_pair(Token::Whitespace, token),
    ];
    let ctype = ContainerType::Strikethrough;
    collect_container(parser, rule, ctype, &close, &invalid, None)
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender, text::TextRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    fn render_text(input: &str) -> String {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:?}");
        TextRender.render(&tree, &page_info, &settings)
    }

    fn render_html(input: &str) -> String {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:?}");
        HtmlRender.render(&tree, &page_info, &settings).body
    }

    #[test]
    fn paired_double_dashes_still_render_as_strikethrough() {
        assert_eq!(
            render_text("before --removed-- after"),
            "before removed after"
        );
    }

    #[test]
    fn wikidot_keeps_double_tildes_literal() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("Some ~~struck through~~ text");
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert_eq!(
            HtmlRender.render(&tree, &page_info, &settings).body,
            "<p>Some ~~struck through~~ text</p>",
        );
    }

    #[test]
    fn unmatched_dash_does_not_swallow_the_next_block() {
        // Real shape from jp/archived:log-of-anomalous-items-jp: a
        // collapsible whose body ends in an unmatched `--` attribution,
        // followed by a collapsible whose body strikes a number through.
        // The unmatched delimiter must not pair across the block
        // boundary; Wikidot renders both collapsibles.
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(concat!(
            "[[collapsible show=\"A\" hide=\"A\"]]\n",
            "//見た目以上の重たさからか、生徒間での負傷事故が起きなかったのは幸いです。//--█研究員\n",
            "[[/collapsible]]\n",
            "[[collapsible show=\"B\" hide=\"B\"]]\n",
            "現状: サイト-81██の低脅威度物品保管庫に--30--25個を保管。管理人の許可を得ればサイト内に限り持ち出し可能。\n",
            "[[/collapsible]]\n",
        ));
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(
            html.matches("collapsible-block-folded").count(),
            2,
            "{html}"
        );
        assert_eq!(html.matches("line-through").count(), 1, "{html}");
    }

    #[test]
    fn double_dashes_do_not_form_strikethrough_across_paragraphs() {
        assert_eq!(
            render_html("--\n* item\n\n--"),
            "<p>—</p><ul>\n<li>item</li>\n</ul><p>—</p>"
        );
    }

    #[test]
    fn dense_table_prose_dashes_remain_literal_without_recursive_backtracking() {
        let row = "[[row]]\n[[cell]]\nterm -- explanatory prose\n[[/cell]]\n[[/row]]\n";
        let input = format!("[[table]]\n{}[[/table]]", row.repeat(200));
        let html = render_html(&input);

        assert_eq!(html.matches("term").count(), 200);
        assert_eq!(html.matches("explanatory prose").count(), 200);
        assert!(!html.contains("<s>"), "{html}");
    }
}
