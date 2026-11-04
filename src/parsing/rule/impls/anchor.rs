/*
 * parsing/rule/impls/anchor.rs
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
use crate::id_prefix::isolate_ids;
use crate::tree::{LinkLabel, LinkLocation, LinkType};
use std::borrow::Cow;

pub const RULE_ANCHOR: Rule = Rule {
    name: "anchor",
    position: LineRequirement::Any,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Trying to create a named anchor");
    let nested_in_literal_triple_link = parser.in_wikidot_literal_triple_link();
    let source_start = parser.current().span.start;
    assert_step(parser, Token::LeftBlockAnchor)?;

    parser.get_token(Token::Whitespace, ParseErrorKind::RuleFailed)?;

    // Gather name for anchor
    // The scanner assigns a three-bracket run to `RightLink`. Wikidot gives
    // the first two brackets to a valid named anchor and leaves the third as
    // source text, so the rule must make that split transactionally.
    let close = [
        ParseCondition::current(Token::RightBlock),
        ParseCondition::current(Token::RightLink),
    ];
    let invalid = if parser.settings().layout.legacy() {
        vec![
            ParseCondition::current(Token::ParagraphBreak),
            ParseCondition::current(Token::LineBreak),
            ParseCondition::current(Token::RuntimeText),
        ]
    } else {
        vec![
            ParseCondition::current(Token::Whitespace),
            ParseCondition::current(Token::ParagraphBreak),
            ParseCondition::current(Token::LineBreak),
            ParseCondition::current(Token::RuntimeText),
        ]
    };
    let (name, closer) =
        collect_text_resolved(parser, RULE_ANCHOR, &close, &invalid, None)?;
    let residual_closer = closer.token == Token::RightLink;
    let valid_wikidot_name = name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character));
    if residual_closer
        && (!parser.settings().layout.legacy()
            || name.is_empty()
            || !valid_wikidot_name && !nested_in_literal_triple_link)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if name.is_empty() {
        let source = parser.full_text().inner();
        return ok!(text!(&source[source_start..closer.span.end]));
    }

    if parser.settings().layout.legacy() && !valid_wikidot_name {
        let without_trailing = name.trim_end_matches([' ', '\t', '\r', '\n']);
        let label = without_trailing.trim_start_matches([' ', '\t', '\r', '\n']);
        let trailing = &name[without_trailing.len()..];
        let mut elements = vec![
            text!("["),
            Element::Link {
                ltype: LinkType::Anchor,
                link: LinkLocation::Url(Cow::Borrowed("javascript:;")),
                label: LinkLabel::Text(Cow::Owned(label.to_string())),
                target: None,
            },
        ];
        if !trailing.is_empty() {
            elements.push(Element::Text(Cow::Owned(trailing.to_string())));
        }
        elements.push(text!("]"));
        if residual_closer {
            elements.push(text!("]"));
        }
        return ok!(Elements::Multiple(elements));
    }

    let name = if parser.settings().isolate_user_ids && !parser.settings().layout.legacy()
    {
        Cow::Owned(isolate_ids(&name))
    } else {
        name
    };

    let anchor = Element::AnchorName(name);
    if residual_closer {
        ok!(Elements::Multiple(vec![anchor, text!("]")]))
    } else {
        ok!(anchor)
    }
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn wikidot_named_anchor_keeps_the_live_name() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[# apple]] X");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html, "<p><a name=\"apple\"></a> X</p>");
    }

    #[test]
    fn wikidot_named_anchor_with_spaces_becomes_a_fake_link() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[# apple banana-cherry]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            "<p>[<a href=\"javascript:;\">apple banana-cherry</a>]</p>",
        );
    }

    #[test]
    fn wikidot_named_anchor_with_symbols_becomes_a_fake_link() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[# symbol$%_foo]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html, "<p>[<a href=\"javascript:;\">symbol$%_foo</a>]</p>",);
    }
}
