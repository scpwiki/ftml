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
use crate::parsing::{ParagraphSplitEntry, ParagraphSplitKind};
use crate::tree::PartialElement;

/// Convenience wrapper around `collect()` to consume each token iteration.
///
/// Since simply consuming to produce an `Element<'t>` is a typical pattern,
/// this function implements it here to avoid code duplication.
///
/// This call always sets `step_on_final` to `true`.
fn delimiter_inert_block_head<'r, 't>(
    parser: &mut Parser<'r, 't>,
    rule: Rule,
) -> Option<Element<'t>> {
    if parser.settings().layout.legacy() || !matches!(rule.name(), "bold" | "italics") {
        return None;
    }
    let name = parser.block_head_name_here()?;
    if !matches!(name.as_str(), "div" | "blockquote" | "code") {
        return None;
    }
    parser.take_block_head_text().ok()?
}

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
        let success = match delimiter_inert_block_head(parser, rule) {
            Some(element) => {
                ParseSuccess::new(Elements::Single(element), Vec::new(), true)
            }
            None => consume(parser)?,
        };
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
    let split_close_leftover = collection.split_close_leftover.clone();
    let (last, errors, paragraph_safe) = collection.into();
    if parser.settings().layout.legacy() {
        collapse_adjacent_ascii_spaces(&mut all_elements);
    }

    let item = (all_elements, last);
    let mut success = ParseSuccess::new(item, errors, paragraph_safe);
    success.split_close_leftover = split_close_leftover;
    Ok(success)
}

pub struct CollectSplitEscape<'t> {
    pub segments: Vec<(Vec<Element<'t>>, bool, ParagraphSplitKind)>,
}

impl<'t> CollectSplitEscape<'t> {
    pub fn into_layout(self) -> (Vec<Element<'t>>, Vec<ParagraphSplitEntry>, bool) {
        let mut flat = Vec::new();
        let mut entries = Vec::new();
        let mut safe = true;
        let last_index = self.segments.len() - 1;
        for (index, (segment, segment_safe, kind)) in
            self.segments.into_iter().enumerate()
        {
            safe &= segment_safe;
            let mut count = segment.len();
            let ends_with_break = matches!(segment.last(), Some(Element::LineBreak),);
            flat.extend(segment);
            if index < last_index && !ends_with_break {
                flat.push(Element::LineBreak);
                count += 1;
            }
            entries.push((count, segment_safe, kind));
        }
        (flat, entries, safe)
    }
}

/// Like `collect_consume_keep()`, but watches for the first consumed
/// unit that crosses a paragraph break: a unit carrying a paragraph
/// split layout contributes only its first segment to the collected
/// elements, and the remaining segments are returned as an escape; a
/// unit whose flat elements contain a line break with real content
/// after it (a kept gate body whose segments were flattened into one
/// by line-scoped body gathers) escapes at that break instead.
/// Everything consumed after the escape point joins the last escaped
/// segment.
pub fn collect_consume_keep_split<'r, 't>(
    parser: &mut Parser<'r, 't>,
    rule: Rule,
    closes: &[ParseCondition],
    invalids: &[ParseCondition],
    kind: Option<ParseErrorKind>,
) -> ParseResult<
    'r,
    't,
    (
        Vec<Element<'t>>,
        &'r ExtractedToken<'t>,
        Option<CollectSplitEscape<'t>>,
    ),
> {
    let mut item_elements = Vec::new();
    let mut escape: Option<Vec<(Vec<Element<'t>>, bool, ParagraphSplitKind)>> = None;

    let mut comment_line_break_pending = false;
    let mut inline_scope_depth = 0_usize;
    let collection = collect(parser, rule, closes, invalids, kind, |parser| {
        parser.set_gate_items_allowed(inline_scope_depth == 0);
        let drop_line_break = comment_line_break_pending;
        comment_line_break_pending = false;
        let comment_line = parser.settings().layout.legacy()
            && parser.current().token == Token::LeftComment
            && parser.start_of_line();
        let success = match delimiter_inert_block_head(parser, rule) {
            Some(element) => {
                ParseSuccess::new(Elements::Single(element), Vec::new(), true)
            }
            None => consume(parser)?,
        };
        if comment_line
            && matches!(
                parser.current().token,
                Token::LineBreak | Token::ParagraphBreak
            )
        {
            comment_line_break_pending = true;
        }
        let ParseSuccess {
            item: elements,
            errors,
            paragraph_safe: step_safe,
            paragraph_split,
            ..
        } = success;
        for element in elements.as_ref() {
            match element {
                Element::Partial(PartialElement::InlineSpanOpen(_))
                | Element::Partial(PartialElement::InlineSizeOpen(_)) => {
                    inline_scope_depth += 1;
                }
                Element::Partial(PartialElement::InlineSpanClose(_))
                | Element::Partial(PartialElement::InlineSizeClose(_)) => {
                    inline_scope_depth = inline_scope_depth.saturating_sub(1);
                }
                _ => {}
            }
        }
        let single_line_break = matches!(elements, Elements::Single(Element::LineBreak));
        match &mut escape {
            Some(segments) => {
                let (tail, tail_safe, _) =
                    segments.last_mut().expect("escape has a last segment");
                *tail_safe &= step_safe;
                if !(drop_line_break
                    && single_line_break
                    && matches!(tail.last(), Some(Element::LineBreak)))
                {
                    append_elements(tail, elements);
                }
            }
            None => {
                let split = paragraph_split.filter(|split| {
                    split.len() >= 2
                        && split.iter().map(|(count, _, _)| count).sum::<usize>()
                            == elements.len()
                });
                match split {
                    Some(split) => {
                        let flat: Vec<Element<'t>> = elements.as_ref().to_vec();
                        let mut iter = flat.into_iter();
                        let mut segments = Vec::new();
                        for (index, (count, segment_safe, kind)) in
                            split.into_iter().enumerate()
                        {
                            let mut segment: Vec<Element<'t>> =
                                iter.by_ref().take(count).collect();
                            if index == 0 {
                                while matches!(segment.last(), Some(Element::LineBreak)) {
                                    segment.pop();
                                }
                                item_elements.append(&mut segment);
                            } else {
                                segments.push((segment, segment_safe, kind));
                            }
                        }
                        escape = Some(segments);
                    }
                    None => {
                        let flat: Vec<Element<'t>> = elements.as_ref().to_vec();
                        let break_at = flat
                            .iter()
                            .position(|element| matches!(element, Element::LineBreak))
                            .filter(|&index| {
                                flat[index + 1..].iter().any(|element| {
                                    !matches!(element, Element::Delayed(_))
                                })
                            });
                        match break_at {
                            Some(index) => {
                                let mut iter = flat.into_iter();
                                item_elements.extend(iter.by_ref().take(index));
                                let rest: Vec<Element<'t>> = iter.skip(1).collect();
                                escape = Some(vec![(
                                    rest,
                                    step_safe,
                                    ParagraphSplitKind::Paragraph,
                                )]);
                            }
                            None => {
                                if !(drop_line_break
                                    && single_line_break
                                    && matches!(
                                        item_elements.last(),
                                        Some(Element::LineBreak)
                                    ))
                                {
                                    item_elements.extend(flat);
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(ParseSuccess::new((), errors, step_safe))
    })?;
    let split_close_leftover = collection.split_close_leftover.clone();
    let (last, errors, paragraph_safe) = collection.into();
    if parser.settings().layout.legacy() {
        collapse_adjacent_ascii_spaces(&mut item_elements);
        if let Some(segments) = &mut escape {
            for (segment, _, _) in segments.iter_mut() {
                collapse_adjacent_ascii_spaces(segment);
            }
        }
    }

    let item = (
        item_elements,
        last,
        escape.map(|segments| CollectSplitEscape { segments }),
    );
    let mut success = ParseSuccess::new(item, errors, paragraph_safe);
    success.split_close_leftover = split_close_leftover;
    Ok(success)
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
    let split_close_leftover = collection.split_close_leftover.clone();
    let (last, errors, paragraph_safe) = collection.into();
    if parser.settings().layout.legacy() {
        collapse_adjacent_ascii_spaces(&mut all_elements);
    }

    let mut success = ParseSuccess::new((all_elements, last), errors, paragraph_safe);
    success.split_close_leftover = split_close_leftover;
    Ok(success)
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
