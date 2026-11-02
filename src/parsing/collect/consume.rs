/*
 * parsing/collect/consume.rs
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

use super::generic::collect_before;
use super::prelude::*;

/// Convenience wrapper around `collect()` to consume each token iteration.
///
/// Since simply consuming to produce an `Element<'t>` is a typical pattern,
/// this function implements it here to avoid code duplication.
///
/// This call always sets `step_on_final` to `true`.
pub fn collect_consume<'r, 't>(
    parser: &mut Parser<'r, 't>,
    rule: Rule,
    closes: &[ParseCondition],
    invalids: &[ParseCondition],
    kind: Option<ParseErrorKind>,
) -> ParseResult<'r, 't, Vec<Element<'t>>> {
    let success = collect_consume_keep(parser, rule, closes, invalids, kind)?;
    Ok(success.map(|(elements, _)| elements))
}

/// Modified form of `collect_consume()` that also returns the last token.
///
/// The last token terminating the collection is kept, and returned
/// to the caller alongside the string slice.
///
/// Compare with `collect_text_keep()`.
pub fn collect_consume_keep<'r, 't>(
    parser: &mut Parser<'r, 't>,
    rule: Rule,
    closes: &[ParseCondition],
    invalids: &[ParseCondition],
    kind: Option<ParseErrorKind>,
) -> ParseResult<'r, 't, (Vec<Element<'t>>, &'r ExtractedToken<'t>)> {
    let mut all_elements = Vec::new();

    let mut comment_line_break_pending = false;
    let collection = collect(parser, rule, closes, invalids, kind, |parser| {
        let drop_line_break = comment_line_break_pending;
        comment_line_break_pending = false;
        let comment_line = parser.settings().layout.legacy()
            && parser.current().token == Token::LeftComment
            && parser.start_of_line();
        let success = consume(parser)?;
        if comment_line
            && matches!(
                parser.current().token,
                Token::LineBreak | Token::ParagraphBreak
            )
        {
            comment_line_break_pending = true;
        }
        Ok(success.map(|elements| {
            if !(drop_line_break
                && matches!(elements, Elements::Single(Element::LineBreak))
                && matches!(all_elements.last(), Some(Element::LineBreak)))
            {
                append_elements(&mut all_elements, elements);
            }
        }))
    })?;
    let (last, errors, paragraph_safe) = collection.into();
    if parser.settings().layout.legacy() {
        collapse_adjacent_ascii_spaces(&mut all_elements);
    }

    let item = (all_elements, last);
    Ok(ParseSuccess::new(item, errors, paragraph_safe))
}

/// Collect elements up to a closing condition without consuming its token.
pub fn collect_consume_before<'r, 't>(
    parser: &mut Parser<'r, 't>,
    rule: Rule,
    closes: &[ParseCondition],
    invalids: &[ParseCondition],
    kind: Option<ParseErrorKind>,
) -> ParseResult<'r, 't, (Vec<Element<'t>>, &'r ExtractedToken<'t>)> {
    let mut all_elements = Vec::new();

    let collection = collect_before(parser, rule, closes, invalids, kind, |parser| {
        consume(parser)?.map_ok(|elements| append_elements(&mut all_elements, elements))
    })?;
    let (last, errors, paragraph_safe) = collection.into();
    if parser.settings().layout.legacy() {
        collapse_adjacent_ascii_spaces(&mut all_elements);
    }

    Ok(ParseSuccess::new(
        (all_elements, last),
        errors,
        paragraph_safe,
    ))
}

fn collapse_adjacent_ascii_spaces(elements: &mut Vec<Element<'_>>) {
    let mut previous_was_space = false;
    elements.retain(|element| {
        let is_space = matches!(element, Element::Text(text) if text.as_ref() == " ");
        let keep = !is_space || !previous_was_space;
        previous_was_space = is_space;
        keep
    });
}

fn append_elements<'t>(all_elements: &mut Vec<Element<'t>>, elements: Elements<'t>) {
    match elements {
        Elements::None => all_elements.reserve(0),
        Elements::Single(element) => all_elements.push(element),
        Elements::Multiple(mut elements) => {
            if all_elements.is_empty() {
                *all_elements = elements;
            } else {
                all_elements.append(&mut elements);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::rule::impls::RULE_TEXT;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn append_elements_adopts_first_multiple_vector() {
        let mut all_elements = Vec::new();
        append_elements(&mut all_elements, Elements::None);
        assert!(all_elements.is_empty());

        append_elements(
            &mut all_elements,
            Elements::Multiple(vec![text!("a"), text!("b")]),
        );

        let capacity = all_elements.capacity();

        append_elements(&mut all_elements, Elements::Single(text!("c")));
        append_elements(&mut all_elements, Elements::Multiple(vec![text!("d")]));

        assert_eq!(
            all_elements,
            vec![text!("a"), text!("b"), text!("c"), text!("d")],
        );
        assert!(all_elements.capacity() >= capacity);
    }

    #[test]
    fn collect_before_leaves_the_owner_terminator_unconsumed() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("alpha]]tail");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().expect("identifier follows input start");

        let success = collect_consume_before(
            &mut parser,
            RULE_TEXT,
            &[ParseCondition::current(Token::RightBlock)],
            &[],
            None,
        )
        .expect("collection stops before the owner terminator");
        let ((elements, terminator), errors, paragraph_safe) = success.into();

        assert_eq!(elements, vec![text!("alpha")]);
        assert_eq!(terminator.token, Token::RightBlock);
        assert_eq!(parser.current().token, Token::RightBlock);
        assert!(errors.is_empty());
        assert!(paragraph_safe);
    }
}
