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

    let whitespace = parser.get_token(Token::Whitespace, ParseErrorKind::RuleFailed)?;
    // One whitespace character separates `[[#` from the name. Further
    // characters belong to the name, so `[[#  a]]` is a spaced name.
    let surplus = if parser.settings().layout.legacy() {
        whitespace.chars().next().map_or_else(String::new, |first| {
            whitespace[first.len_utf8()..].to_owned()
        })
    } else {
        String::new()
    };

    try_consume_anchor_body(
        parser,
        source_start,
        nested_in_literal_triple_link,
        &surplus,
    )
}

pub(super) fn try_consume_wikidot_triple_anchor<'r, 't>(
    parser: &mut Parser<'r, 't>,
    source_start: usize,
) -> ParseResult<'r, 't, Elements<'t>>
where
    'r: 't,
{
    if parser.current().token != Token::NumberedItem || parser.current().slice != "#" {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    parser.step()?;
    parser.get_token(Token::Whitespace, ParseErrorKind::RuleFailed)?;
    try_consume_anchor_body(parser, source_start, true, "")
}

fn try_consume_anchor_body<'r, 't>(
    parser: &mut Parser<'r, 't>,
    source_start: usize,
    nested_in_literal_triple_link: bool,
    surplus: &str,
) -> ParseResult<'r, 't, Elements<'t>>
where
    'r: 't,
{
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
        collect_text_resolved_fused(parser, RULE_ANCHOR, &close, &invalid, None, false)?;
    let name = if surplus.is_empty() {
        name
    } else {
        Cow::Owned(format!("{surplus}{name}"))
    };
    let residual_closer = closer.token == Token::RightLink;
    let valid_wikidot_name = name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-_.%".contains(character));
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
        let leading = &without_trailing[..without_trailing.len() - label.len()];
        let trailing = &name[without_trailing.len()..];
        let mut elements = vec![text!("[")];
        if !leading.is_empty() {
            elements.push(Element::Text(Cow::Owned(leading.to_string())));
        }
        elements.push(Element::Link {
            ltype: LinkType::Anchor,
            link: LinkLocation::Url(Cow::Borrowed("javascript:;")),
            label: LinkLabel::Text(Cow::Owned(label.to_string())),
            target: None,
        });
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
    fn wikidot_named_anchor_with_dots_keeps_the_live_name() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[# abc.def]] X");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html, "<p><a name=\"abc.def\"></a> X</p>");
    }

    #[test]
    fn wikidot_named_anchor_with_mixed_dots_keeps_the_live_name() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[# a_b-c.d]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html, "<p><a name=\"a_b-c.d\"></a></p>");
    }

    #[test]
    fn wikidot_named_anchor_with_percent_keeps_the_live_name() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[# a%b]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html, "<p><a name=\"a%b\"></a></p>");
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
