/*
 * parsing/rule/impls/subscript.rs
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
    inline_body_padded_through_seams, legacy_format_body_code_preceded_by_content,
    legacy_format_body_contains_table, validate_reachable_close,
};
use super::prelude::*;

pub const RULE_SUBSCRIPT: Rule = Rule {
    name: "subscript",
    position: LineRequirement::Any,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Trying to create subscript container");
    validate_reachable_close(
        parser,
        Token::Subscript,
        None,
        ReachableCloseMode::BlockBarrier,
    )?;
    assert_unpadded_open(parser, Token::Subscript)?;
    let close = [ParseCondition::current(Token::Subscript)];
    let invalid = [
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::token_pair(Token::Subscript, Token::Whitespace),
        ParseCondition::token_pair(Token::Whitespace, Token::Subscript),
    ];
    let ctype = ContainerType::Subscript;
    let collected =
        collect_container(parser, RULE_SUBSCRIPT, ctype, &close, &invalid, None)?;
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
        && legacy_format_body_code_preceded_by_content(&elements)
    {
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
    let elements =
        append_wikidot_line_close_space(elements, parser.settings().layout.legacy());
    let elements = super::bold::mark_format_stop_at_list(parser, elements);
    let elements = super::bold::mark_span_list_close_tail(parser, elements);
    ok!(paragraph_safe; elements, errors)
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    fn render(source: &str) -> String {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:#?}");
        HtmlRender.render(&tree, &page_info, &settings).body
    }

    #[test]
    fn wikidot_line_started_subscript_and_superscript_closers_add_space() {
        assert_eq!(render("a,,x\n,,.b"), "<p>a<sub>x<br></sub> .b</p>");
        assert_eq!(render("a^^x\n^^.b"), "<p>a<sup>x<br></sup> .b</p>");
        assert_eq!(render("a,,x,,.b"), "<p>a<sub>x</sub>.b</p>");
    }
}
