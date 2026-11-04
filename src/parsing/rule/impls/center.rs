/*
 * parsing/rule/impls/center.rs
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

use super::prelude::*;
use crate::tree::Alignment;

pub const RULE_CENTER: Rule = Rule {
    name: "center",
    position: LineRequirement::StartOfLine,
    try_consume_fn,
};

fn step_expected<'r, 't>(
    parser: &mut Parser<'r, 't>,
    token: Token,
) -> Result<(), ParseError> {
    let current = parser.current().token;
    if current != token {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    parser.step()?;
    Ok(())
}

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Trying to create centered container");

    step_expected(parser, Token::Equals)?;
    step_expected(parser, Token::Whitespace)?;

    if parser.settings().layout.legacy()
        && matches!(
            parser.current().token,
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
        )
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let close = [
        ParseCondition::current(Token::LineBreak),
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::current(Token::InputEnd),
    ];
    if parser.settings().layout.legacy() {
        let collected = collect_container(
            parser,
            RULE_CENTER,
            ContainerType::Paragraph,
            &close,
            &[],
            None,
        )?;
        let (mut elements, errors, _) = collected.into();
        let Elements::Single(Element::Container(paragraph)) = &mut elements else {
            unreachable!("center rule always creates one container");
        };
        assert!(
            paragraph
                .attributes_mut()
                .insert("style", cow!(Alignment::Center.wd_html_style()))
        );
        ok!(false; elements, errors)
    } else {
        let ctype = ContainerType::Align(Alignment::Center);
        collect_container(parser, RULE_CENTER, ctype, &close, &[], None)
    }
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn wikidot_center_line_is_a_styled_paragraph() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("= centered");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html, r#"<p style="text-align: center;">centered</p>"#);
    }
}
