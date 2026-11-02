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
}
