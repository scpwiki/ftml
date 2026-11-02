/*
 * parsing/rule/impls/header.rs
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
use crate::tree::Heading;
use std::convert::TryInto;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum WikidotHeadingContent {
    Empty,
    FootnoteFirst,
    Other,
}

pub const RULE_HEADER: Rule = Rule {
    name: "header",
    position: LineRequirement::StartOfLine,
    try_consume_fn,
};

fn step_expected<'r, 't>(
    parser: &mut Parser<'r, 't>,
    token: Token,
) -> Result<&'r ExtractedToken<'t>, ParseError> {
    let current = parser.current();
    if current.token != token {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    parser.step()?;
    Ok(current)
}

fn heading_from_token(token: &ExtractedToken<'_>) -> Heading {
    token
        .slice
        .try_into()
        .expect("Received invalid heading length token slice")
}

fn consume_header_once<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Trying to create header container");

    let heading_token = step_expected(parser, Token::Heading)?;
    let heading = heading_from_token(heading_token);

    step_expected(parser, Token::Whitespace)?;

    let close = [
        ParseCondition::current(Token::InputEnd),
        ParseCondition::current(Token::LineBreak),
        ParseCondition::current(Token::ParagraphBreak),
    ];
    let ctype = ContainerType::Header(heading);
    let collected = collect_container(parser, RULE_HEADER, ctype, &close, &[], None)?;
    let (mut elements, mut all_errors, _) = collected.into();

    let content = match &elements {
        Elements::Single(Element::Container(container)) => {
            wikidot_heading_content(container.elements())
        }
        _ => unreachable!("header collection always returns one container"),
    };

    if parser.settings().layout.legacy()
        && content == WikidotHeadingContent::FootnoteFirst
    {
        return ok!(
            wikidot_literal_heading(heading_token.slice, elements),
            all_errors,
        );
    }

    if parser.settings().layout.legacy()
        && content == WikidotHeadingContent::Empty
        && parser.current().token == Token::LeftBlock
        && parser
            .look_ahead(0)
            .is_some_and(|token| token.slice.eq_ignore_ascii_case("footnote"))
    {
        let mut candidate = parser.clone_with_rule(RULE_HEADER);
        if let Ok(footnote) = super::RULE_BLOCK.try_consume(&mut candidate)
            && let Elements::Single(Element::Footnote(index)) = footnote.item
        {
            let mut literal = wikidot_literal_heading(heading_token.slice, elements);
            literal.push(Element::Footnote(index));
            all_errors.extend(footnote.errors);
            parser.update(&candidate);
            return ok!(literal, all_errors);
        }
    }

    if heading.has_toc
        && let Elements::Single(Element::Container(ref container)) = elements
    {
        parser.push_table_of_contents_entry(heading.level, container.elements());
    }

    // Wikidot joins a footnote that starts the next physical line to the
    // preceding heading instead of wrapping it in a new paragraph.
    if parser.settings().layout.legacy()
        && content == WikidotHeadingContent::Other
        && parser.current().token == Token::LeftBlock
        && parser
            .look_ahead(0)
            .is_some_and(|token| token.slice.eq_ignore_ascii_case("footnote"))
    {
        let mut candidate = parser.clone_with_rule(RULE_HEADER);
        if let Ok(footnote) = super::RULE_BLOCK.try_consume(&mut candidate)
            && let Elements::Single(Element::Footnote(index)) = footnote.item
            && let Elements::Single(Element::Container(container)) = &mut elements
        {
            container.elements_mut().push(Element::Footnote(index));
            all_errors.extend(footnote.errors);
            parser.update(&candidate);
        }
    }

    ok!(false; elements, all_errors)
}

fn wikidot_literal_heading<'t>(
    marker: &'t str,
    elements: Elements<'t>,
) -> Vec<Element<'t>> {
    let Elements::Single(Element::Container(container)) = elements else {
        unreachable!("header collection always returns one container");
    };
    let mut literal = Vec::with_capacity(container.elements().len() + 1);
    literal.push(text!(marker));
    literal.extend(Vec::from(container));
    literal
}

fn wikidot_heading_content(elements: &[Element<'_>]) -> WikidotHeadingContent {
    for element in elements {
        let content = match element {
            Element::Text(text) | Element::Raw(text)
                if text.chars().all(char::is_whitespace) =>
            {
                WikidotHeadingContent::Empty
            }
            Element::Container(container) => {
                wikidot_heading_content(container.elements())
            }
            Element::Anchor { elements, .. } | Element::Color { elements, .. } => {
                wikidot_heading_content(elements)
            }
            Element::Footnote(_) => WikidotHeadingContent::FootnoteFirst,
            Element::LineBreak => WikidotHeadingContent::Empty,
            Element::Partial(partial) if partial.is_inline_format_control() => {
                WikidotHeadingContent::Empty
            }
            _ => WikidotHeadingContent::Other,
        };
        if content != WikidotHeadingContent::Empty {
            return content;
        }
    }
    WikidotHeadingContent::Empty
}

fn indented_after_quote_marker(parser: &Parser<'_, '_>) -> bool {
    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let line_start = source[..start].rfind('\n').map_or(0, |offset| offset + 1);
    let prefix = &source[line_start..start];
    let Some(marker) = prefix.rfind('>') else {
        return false;
    };
    if !prefix[..marker]
        .chars()
        .all(|c| matches!(c, '>' | ' ' | '\t'))
    {
        return false;
    }
    let tail = &prefix[marker + 1..];
    tail.len() >= 2 && tail.chars().all(|c| matches!(c, ' ' | '\t'))
}

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    if parser.settings().layout.legacy()
        && parser.in_native_blockquote_line()
        && indented_after_quote_marker(parser)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let success = consume_header_once(parser)?;
    if success.paragraph_safe {
        return Ok(success);
    }
    let (elements, mut all_errors, _) = success.into();
    let mut all_elements: Vec<_> = elements.into_iter().collect();

    if parser.native_blockquote_depth().is_some() {
        return ok!(false; all_elements, all_errors);
    }

    loop {
        let parser_state = parser.get_mutable_state();
        let mut sub_parser = parser.clone_with_rule(RULE_HEADER);

        let success = match consume_header_once(&mut sub_parser) {
            Ok(success) => success,
            Err(_) => {
                parser.reset_mutable_state(parser_state);
                return ok!(false; all_elements, all_errors);
            }
        };

        if success.paragraph_safe {
            parser.reset_mutable_state(parser_state);
            return ok!(false; all_elements, all_errors);
        }

        parser.update(&sub_parser);

        let (elements, mut errors, _) = success.into();
        all_elements.extend(elements);
        all_errors.append(&mut errors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::{ContainerType, Element, HeadingLevel};

    #[test]
    fn header_rule_collects_adjacent_headings_and_toc_flags() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("+ One\n++* Two\nplain");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(tree.table_of_contents.len(), 1);
        assert_eq!(tree.elements.len(), 3);

        let Element::Container(first) = &tree.elements[0] else {
            panic!("expected first heading, got {:?}", tree.elements[0]);
        };
        assert_eq!(
            first.ctype(),
            ContainerType::Header(Heading {
                level: HeadingLevel::One,
                has_toc: true,
            }),
        );

        let Element::Container(second) = &tree.elements[1] else {
            panic!("expected second heading, got {:?}", tree.elements[1]);
        };
        assert_eq!(
            second.ctype(),
            ContainerType::Header(Heading {
                level: HeadingLevel::Two,
                has_toc: false,
            }),
        );
    }

    #[test]
    fn wikidot_heading_absorbs_footnote_from_next_line() {
        use crate::render::{Render, html::HtmlRender};

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization =
            crate::tokenize("+ Secret heading\n[[footnote]]secret[[/footnote]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(
            html.contains("Secret heading<sup class=\"footnoteref\""),
            "{html}",
        );
        assert!(!html.contains("</h1><p><sup"), "{html}");
    }
}
