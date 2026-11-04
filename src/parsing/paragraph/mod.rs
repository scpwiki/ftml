/*
 * parsing/paragraph/mod.rs
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

mod stack;
#[cfg(test)]
mod stack_orders;

pub use self::stack::ParagraphStack;
pub(crate) use self::stack::collapsible_has_direct_literal_nested_opener;

use super::collect::{TokenWindow, comment_bridge_end};
use super::consume::consume;
use super::parser::Parser;
use super::parser::QuoteBodyLineStatus;
use super::prelude::*;
use super::rule::Rule;
use super::token::Token;
use crate::tree::ContainerType;

type CloseConditionFn = fn(&mut Parser) -> Result<bool, ParseError>;

pub fn no_close_condition() -> Option<CloseConditionFn> {
    None
}

pub fn gather_paragraphs<'r, 't, F>(
    parser: &mut Parser<'r, 't>,
    rule: Rule,
    mut close_condition_fn: Option<F>,
) -> ParseResult<'r, 't, Vec<Element<'t>>>
where
    'r: 't,
    F: FnMut(&mut Parser<'r, 't>) -> Result<bool, ParseError>,
{
    parser.set_rule(rule);

    let mut stack = if parser.settings().layout.legacy() {
        ParagraphStack::new_wikidot()
    } else {
        ParagraphStack::new()
    };
    if parser.settings().layout.legacy() && close_condition_fn.is_none() {
        stack.enable_wikidot_div_literal_run_wrap();
    }

    let mut finished = false;
    let mut literal_gallery_pending = false;
    let mut line_prefix_whitespace_only = true;
    while !finished {
        let quote_line_status = parser.prepare_quote_body_line()?;
        if quote_line_status == QuoteBodyLineStatus::Boundary {
            if parser.quote_boundary_closes_body() {
                stack.pop_line_break();
                finished = true;
                continue;
            }
            return Err(parser.make_err(ParseErrorKind::EndOfInput));
        }
        if parser.settings().layout.legacy()
            && quote_line_status == QuoteBodyLineStatus::Prepared
            && parser.quote_body_line_is_empty_spaced()
        {
            stack.pop_line_break();
            stack.end_paragraph();
            parser.step()?;
            line_prefix_whitespace_only = true;
            continue;
        }

        match wikidot_empty_raw_line_kind(parser) {
            Some(WikidotEmptyRawLineKind::Complete) => {
                stack.mark_wikidot_complete_raw_line_occupancy();
            }
            Some(WikidotEmptyRawLineKind::Paired) => {
                stack.mark_wikidot_invisible_line_occupancy();
            }
            None => {}
        }
        let terminal_backslash =
            parser.current().token == Token::LineBreak && parser.current().slice == "\\";
        let complete_raw_line_break = parser.current().token == Token::LineBreak
            && stack.wikidot_complete_raw_line_occupied();
        let continued_block_boundary = parser.current().token == Token::LineBreak
            && parser.current().slice == "\\\n";
        if continued_block_boundary && parser.settings().layout.legacy() {
            stack.mark_wikidot_continued_block_boundary();
        }
        let terminal_spaced_underscore = parser.settings().layout.legacy()
            && parser.current().token == Token::Whitespace
            && parser
                .look_ahead(0)
                .is_some_and(|token| token.token == Token::Underscore)
            && parser
                .look_ahead(1)
                .is_some_and(|token| token.token == Token::InputEnd);
        let comment = parser.current().token == Token::LeftComment;
        let comment_started_line = comment && parser.start_of_line();
        let empty_quote_control =
            parser.current().token == Token::Quote && parser.start_of_line();
        let explicit_list_opener = wikidot_explicit_list_opener(parser);
        let div_score = wikidot_div_score(parser);
        let div_opener = div_score.is_some();
        let same_line_div_candidate =
            div_opener && wikidot_div_closes_on_current_line(parser);
        let embed_opener = wikidot_embed_opener(parser);
        let inline_div_opener = div_opener && !parser.start_of_line();
        stack.record_next_element_line_start(
            parser.start_of_line()
                || matches!(
                    parser.current().token,
                    Token::LineBreak | Token::ParagraphBreak
                ),
        );
        let iteration_token = parser.current().token;
        let consumed = match parser.current().token {
            Token::InputEnd => {
                if close_condition_fn.is_some() {
                    if parser.settings().layout.legacy() && rule.name() == "block-div" {
                        finished = true;
                        continue;
                    }

                    warn!("Hit the end of input, producing an error");
                    return Err(parser.make_err(ParseErrorKind::EndOfInput));
                } else {
                    warn!("Hit the end of input, terminating token iteration");
                    finished = true;
                    None
                }
            }

            Token::ParagraphBreak => {
                if stack.wikidot_complete_raw_line_occupied() {
                    stack.clear_wikidot_complete_raw_line_occupancy();
                }
                let demote_chain_end =
                    if parser.settings().layout.legacy() && !stack.current_empty() {
                        let window = TokenWindow::at(parser);
                        wikidot_comment_chain_occupied(window, 1)
                            .filter(|end| !wikidot_footnote_opener_at(window, *end))
                    } else {
                        None
                    };
                if demote_chain_end.is_some() {
                    push_element(&mut stack, Element::LineBreak, true);
                    stack.mark_wikidot_demoted_comment_break();
                } else {
                    stack.end_paragraph_at_break();
                }

                parser.step()?;
                None
            }

            _ => {
                let close_condition_met = match close_condition_fn.as_mut() {
                    Some(close_condition_fn) => close_condition_fn(parser)?,
                    None => false,
                };

                if close_condition_met {
                    finished = true;
                    None
                } else {
                    if literal_gallery_pending && wikidot_literal_gallery_close(parser) {
                        stack.end_paragraph();
                        literal_gallery_pending = false;
                    }
                    if parser.current().token == Token::DiscardedControl {
                        stack.mark_discarded_control();
                    }
                    if comment
                        && parser.settings().layout.legacy()
                        && stack.current_empty()
                        && (comment_started_line || line_prefix_whitespace_only)
                    {
                        let window = TokenWindow::at(parser);
                        if wikidot_comment_chain_occupied(window, 0)
                            .is_some_and(|end| !wikidot_footnote_opener_at(window, end))
                        {
                            stack.mark_wikidot_invisible_line_occupancy();
                        }
                    }
                    match consume(parser) {
                        Ok(consumed) => Some(consumed),
                        Err(error)
                            if parser.discarding_hidden_body()
                                && parser.at_hidden_body_boundary() =>
                        {
                            let close_condition = close_condition_fn
                                .as_mut()
                                .expect("body parser must have a close condition");
                            let close_condition_met = close_condition(parser)?;

                            finished =
                                finish_hidden_boundary(close_condition_met, error)?;
                            None
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
        };

        if let Some(consumed) = consumed {
            let mixed_paragraph_ownership =
                parser.take_wikidot_mixed_paragraph_ownership();
            let div_literal_recovered = parser.take_wikidot_div_literal_recovered();
            let mut consumed = consumed;
            let paragraph_split = consumed.paragraph_split.take();
            let (elements, mut errors, paragraph_safe) = consumed.into();
            let mut elements = elements;
            let literal_list_item = stack.wikidot_list_break_follows_list()
                && errors
                    .iter()
                    .any(|error| error.kind() == ParseErrorKind::ListItemOutsideList);
            let split_batch = paragraph_split_batch(&elements, paragraph_split);
            if split_batch.is_none() {
                defer_suppression_seams(&mut stack, &mut elements);
            }
            let active_div = elements_contain_div(&elements);
            let list_has_same_line_residual = explicit_list_opener
                && !paragraph_safe
                && elements_end_with_list(&elements)
                && !matches!(
                    parser.current().token,
                    Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
                );
            let embed_error_has_same_line_residual = embed_opener
                && !paragraph_safe
                && active_div
                && !matches!(
                    parser.current().token,
                    Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
                );
            let div_has_same_line_residual = div_opener
                && active_div
                && !matches!(
                    parser.current().token,
                    Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
                );
            let literal_iftags_line = parser.settings().layout.legacy()
                && errors.iter().any(|error| {
                    error.kind() == ParseErrorKind::RuleFailed
                        && error.rule() == "block-iftags"
                });
            let literal_div_line = parser.settings().layout.legacy()
                && div_score != Some(true)
                && !same_line_div_candidate
                && errors.iter().any(|error| {
                    error.kind() == ParseErrorKind::RuleFailed
                        && error.rule() == "block-div"
                });
            let literal_gallery_line = parser.settings().layout.legacy()
                && errors.iter().any(|error| {
                    error.kind() == ParseErrorKind::RuleFailed
                        && error.rule() == "block-gallery"
                });

            if complete_raw_line_break && elements_begin_with_wikidot_center(&elements) {
                stack.push_element(Element::LineBreak, true);
            }
            if inline_div_opener && active_div {
                stack.mark_wikidot_continued_block_boundary();
            }
            if div_literal_recovered {
                stack.note_wikidot_div_literal_push();
            }
            if empty_quote_control && !paragraph_safe && elements.is_empty() {
                stack.end_paragraph();
            } else {
                if explicit_list_opener
                    && matches!(&elements, Elements::Single(Element::LineBreak))
                {
                    stack.mark_wikidot_empty_list_break();
                }
                if let Some(counts) = split_batch {
                    let elements = match elements {
                        Elements::Multiple(elements) => elements,
                        Elements::Single(element) => vec![element],
                        Elements::None => Vec::new(),
                    };
                    let seam_bounded = batch_is_seam_bounded(&elements);
                    let mut elements = elements.into_iter();
                    let last_index = counts.len() - 1;
                    for (index, (count, segment_safe, _)) in counts.iter().enumerate() {
                        if index > 0 {
                            stack.end_paragraph_at_break();
                        }
                        let mut slice: Vec<Element<'t>> =
                            elements.by_ref().take(*count).collect();
                        if index < last_index
                            && matches!(slice.last(), Some(Element::LineBreak))
                        {
                            slice.pop();
                        }
                        let per_element = seam_bounded
                            && !*segment_safe
                            && slice.iter().any(|element| {
                                !element.paragraph_safe()
                                    && !matches!(
                                        element,
                                        Element::List { .. } | Element::DefinitionList(_)
                                    )
                            });
                        for element in slice {
                            let element_safe = if per_element {
                                element.paragraph_safe()
                            } else {
                                *segment_safe
                            };
                            push_element(&mut stack, element, element_safe);
                        }
                    }
                } else if mixed_paragraph_ownership {
                    let Elements::Multiple(mut elements) = elements else {
                        unreachable!(
                            "mixed paragraph ownership requires multiple elements"
                        );
                    };
                    let owner = elements.remove(0);
                    push_element(&mut stack, owner, false);
                    for element in elements {
                        push_element(&mut stack, element, true);
                    }
                } else {
                    push_elements(&mut stack, elements, paragraph_safe);
                }
            }
            if list_has_same_line_residual {
                stack.mark_next_unwrapped_after_block();
            }
            if div_has_same_line_residual {
                stack.mark_wikidot_same_line_div_residual();
            }
            if embed_error_has_same_line_residual {
                stack.mark_wikidot_same_line_embed_residual();
            }
            if complete_raw_line_break {
                stack.clear_wikidot_complete_raw_line_occupancy();
            }
            if comment_started_line
                && parser.settings().layout.legacy()
                && parser.current().token == Token::Whitespace
            {
                parser.step()?;
            }
            if literal_list_item {
                stack.mark_current_unwrapped();
            }
            if literal_iftags_line {
                stack.mark_wikidot_literal_iftags_line();
            }
            if literal_div_line {
                stack.mark_wikidot_literal_div_line();
            }
            if literal_gallery_line {
                literal_gallery_pending = true;
            }
            if terminal_backslash && parser.settings().layout.legacy() {
                if parser.full_text().inner().ends_with("\u{fffd}\\") {
                    stack.pop_line_break();
                }
                stack.mark_wikidot_terminal_backslash();
            }
            if terminal_spaced_underscore {
                stack.mark_wikidot_terminal_backslash();
            }
            if comment_started_line
                && !stack.current_empty()
                && matches!(
                    parser.current().token,
                    Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
                )
                && !stack.take_wikidot_demoted_comment_break()
            {
                stack.pop_line_break();
            }

            stack.push_errors(&mut errors);
        }

        match iteration_token {
            Token::LineBreak | Token::ParagraphBreak => {
                line_prefix_whitespace_only = true;
            }
            Token::Whitespace
            | Token::LeftComment
            | Token::DiscardedControl
            | Token::InputEnd => {}
            _ => {
                line_prefix_whitespace_only = false;
            }
        }
    }

    stack.into_result()
}

fn wikidot_literal_gallery_close<'r, 't>(parser: &Parser<'r, 't>) -> bool
where
    'r: 't,
{
    if !parser.start_of_line() || parser.current().token != Token::LeftBlockEnd {
        return false;
    }

    let mut close = parser.clone();
    close
        .get_end_block()
        .is_ok_and(|name| name.eq_ignore_ascii_case("gallery"))
        && matches!(
            close.current().token,
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
        )
}

fn wikidot_explicit_list_opener<'r, 't>(parser: &Parser<'r, 't>) -> bool
where
    'r: 't,
{
    if !parser.settings().layout.legacy() || parser.current().token != Token::LeftBlock {
        return false;
    }

    let mut probe = parser.clone();
    probe.get_block_name(false).is_ok_and(|(name, _, segment)| {
        let view = name.strip_suffix('_').unwrap_or(&name);
        (view.eq_ignore_ascii_case("ol") || view.eq_ignore_ascii_case("ul"))
            && probe.head_word_view_is_actionable(&name, segment, view)
    })
}

fn wikidot_div_score<'r, 't>(parser: &Parser<'r, 't>) -> Option<bool>
where
    'r: 't,
{
    if !parser.settings().layout.legacy() || parser.current().token != Token::LeftBlock {
        return None;
    }

    let source = parser.full_text().inner();
    let candidate = &source[parser.current().span.start..];
    let after_open = candidate.strip_prefix("[[")?;
    let after_open = after_open.trim_start_matches([' ', '\t']);
    let name_end = after_open
        .find(|character: char| character.is_whitespace() || character == ']')
        .unwrap_or(after_open.len());
    let name = &after_open[..name_end];
    let (name, scored) = name
        .strip_suffix('_')
        .map_or((name, false), |name| (name, true));
    name.eq_ignore_ascii_case("div").then_some(scored)
}

fn wikidot_embed_opener<'r, 't>(parser: &Parser<'r, 't>) -> bool
where
    'r: 't,
{
    if !parser.settings().layout.legacy() || parser.current().token != Token::LeftBlock {
        return false;
    }

    let source = parser.full_text().inner();
    let candidate = &source[parser.current().span.start..];
    let Some(after_open) = candidate.strip_prefix("[[") else {
        return false;
    };
    let after_open = after_open.trim_start_matches([' ', '\t']);
    let name_end = after_open
        .find(|character: char| character.is_whitespace() || character == ']')
        .unwrap_or(after_open.len());
    let name = &after_open[..name_end];
    let name = name.strip_suffix('_').unwrap_or(name);
    name.eq_ignore_ascii_case("embed")
}

fn wikidot_div_closes_on_current_line(parser: &Parser<'_, '_>) -> bool {
    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let line = source[start..]
        .split_once(['\n', '\r'])
        .map_or(&source[start..], |(line, _)| line);
    line.as_bytes()
        .windows("[[/div]]".len())
        .any(|window| window.eq_ignore_ascii_case(b"[[/div]]"))
}

fn elements_contain_div(elements: &Elements<'_>) -> bool {
    let is_div = |element: &Element<'_>| {
        matches!(
            element,
            Element::Container(container) if container.ctype() == ContainerType::Div
        )
    };
    match elements {
        Elements::None => false,
        Elements::Single(element) => is_div(element),
        Elements::Multiple(elements) => elements.iter().any(is_div),
    }
}

fn elements_end_with_list(elements: &Elements<'_>) -> bool {
    match elements {
        Elements::Single(element) => matches!(element, Element::List { .. }),
        Elements::Multiple(elements) => {
            matches!(elements.last(), Some(Element::List { .. }))
        }
        Elements::None => false,
    }
}

#[derive(Clone, Copy)]
enum WikidotEmptyRawLineKind {
    Complete,
    Paired,
}

fn elements_begin_with_wikidot_center(elements: &Elements<'_>) -> bool {
    let first = match elements {
        Elements::Single(element) => Some(element),
        Elements::Multiple(elements) => elements.first(),
        Elements::None => None,
    };
    matches!(
        first,
        Some(Element::Container(container))
            if container.ctype()
                == crate::tree::ContainerType::Align(crate::tree::Alignment::Center)
    )
}

fn wikidot_empty_raw_line_kind(
    parser: &Parser<'_, '_>,
) -> Option<WikidotEmptyRawLineKind> {
    let boundary = |token: Option<&ExtractedToken<'_>>| {
        token.is_some_and(|token| {
            matches!(
                token.token,
                Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
            )
        })
    };
    if !(parser.settings().layout.legacy()
        && parser.start_of_line()
        && parser.current().token == Token::Raw
        && parser
            .look_ahead(0)
            .is_some_and(|token| token.token == Token::Raw))
    {
        return None;
    }
    if boundary(parser.look_ahead(1)) {
        return Some(WikidotEmptyRawLineKind::Complete);
    }
    (parser
        .look_ahead(1)
        .is_some_and(|token| token.token == Token::Whitespace && token.slice == " ")
        && parser
            .look_ahead(2)
            .is_some_and(|token| token.token == Token::Raw)
        && parser
            .look_ahead(3)
            .is_some_and(|token| token.token == Token::Raw)
        && boundary(parser.look_ahead(4)))
    .then_some(WikidotEmptyRawLineKind::Paired)
}

fn wikidot_comment_chain_occupied(
    window: TokenWindow<'_, '_>,
    mut index: usize,
) -> Option<usize> {
    let mut saw_whitespace = false;
    loop {
        if !matches!(
            window.get(index),
            Some(token) if token.token == Token::LeftComment
        ) {
            return None;
        }
        loop {
            let token = window.get(index)?;
            match token.token {
                Token::LeftComment => {
                    let end = window.offset(index).and_then(comment_bridge_end)?;
                    index += end;
                }
                Token::Whitespace => {
                    saw_whitespace = true;
                    index += 1;
                }
                Token::LineBreak => {
                    index += 1;
                    break;
                }
                Token::ParagraphBreak | Token::InputEnd => {
                    return saw_whitespace.then_some(index);
                }
                _ => return None,
            }
        }
        if !matches!(
            window.get(index),
            Some(token) if token.token == Token::LeftComment
        ) {
            return saw_whitespace.then_some(index);
        }
    }
}

fn wikidot_footnote_opener_at(window: TokenWindow<'_, '_>, index: usize) -> bool {
    matches!(
        window.get(index),
        Some(token) if token.token == Token::LeftBlock
    ) && matches!(
        window.get(index + 1),
        Some(token) if token.token == Token::Identifier
            && token.slice.eq_ignore_ascii_case("footnote")
    )
}

fn finish_hidden_boundary(
    close_condition_met: bool,
    error: ParseError,
) -> Result<bool, ParseError> {
    if close_condition_met {
        Ok(true)
    } else {
        Err(error)
    }
}

fn paragraph_split_batch<'t>(
    elements: &Elements<'t>,
    counts: Option<Vec<ParagraphSplitEntry>>,
) -> Option<Vec<ParagraphSplitEntry>> {
    let counts = counts?;
    if counts.len() < 2 {
        return None;
    }
    if counts.iter().map(|(count, _, _)| count).sum::<usize>() != elements.len() {
        return None;
    }
    Some(counts)
}

pub(crate) fn batch_is_seam_bounded(elements: &[Element]) -> bool {
    elements.iter().any(|element| {
        matches!(element, Element::Delayed(delayed) if delayed.is_typography_boundary())
    })
}

fn push_elements<'t>(
    stack: &mut ParagraphStack<'t>,
    elements: Elements<'t>,
    paragraph_safe: bool,
) {
    match elements {
        Elements::None if paragraph_safe => {}
        Elements::None => stack.mark_current_unwrapped(),
        Elements::Single(element) => push_element(stack, element, paragraph_safe),
        Elements::Multiple(elements) if paragraph_safe => {
            stack.push_paragraph_safe_elements(elements);
        }
        Elements::Multiple(elements) => {
            if elements
                .iter()
                .any(|element| matches!(element, Element::TabView(_)))
            {
                stack.mark_wikidot_tabview_boundary();
            }
            let per_element = batch_is_seam_bounded(&elements)
                && elements.iter().any(|element| !element.paragraph_safe());
            for element in elements {
                let element_safe = if per_element {
                    element.paragraph_safe()
                } else {
                    paragraph_safe
                };
                push_element(stack, element, element_safe);
            }
        }
    }
}

fn defer_suppression_seams<'t>(
    stack: &mut ParagraphStack<'t>,
    elements: &mut Elements<'t>,
) {
    let original = std::mem::replace(elements, Elements::None);
    *elements = match original {
        Elements::Single(element) if matches!(&element, Element::Delayed(delayed) if delayed.is_suppression_seam()) =>
        {
            stack.defer_suppression_seam(element);
            Elements::None
        }
        Elements::Multiple(items) => {
            let mut items = items;
            let trailing = match items.split_last() {
                Some((last, rest))
                    if items.len() > 1
                        && matches!(last, Element::Delayed(delayed) if delayed.is_suppression_seam())
                        && rest.iter().any(|element| {
                            !matches!(element, Element::Delayed(delayed) if delayed.is_suppression_seam())
                        }) =>
                {
                    items.pop()
                }
                _ => None,
            };
            let mut retained = Vec::with_capacity(items.len() + 1);
            for element in items {
                if matches!(&element, Element::Delayed(delayed) if delayed.is_suppression_seam())
                {
                    stack.defer_suppression_seam(element);
                } else {
                    retained.push(element);
                }
            }
            if let Some(trailing) = trailing {
                retained.push(trailing);
            }
            match retained.len() {
                0 => Elements::None,
                1 => Elements::Single(retained.pop().unwrap()),
                _ => Elements::Multiple(retained),
            }
        }
        other => other,
    };
}

fn push_element<'t>(
    stack: &mut ParagraphStack<'t>,
    element: Element<'t>,
    paragraph_safe: bool,
) {
    if paragraph_safe && stack.wikidot_inline_residual_follows_collapsible() {
        stack.mark_next_unwrapped_after_block();
    }

    if !(paragraph_safe
        && stack.current_empty()
        && element == Element::LineBreak
        && !stack.wikidot_line_break_follows_block())
    {
        if paragraph_safe
            && element == Element::LineBreak
            && stack.wikidot_line_break_follows_block()
        {
            stack.mark_next_unwrapped_after_block();
        }
        stack.push_element(element, paragraph_safe);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    fn render_wikidot(source: &str) -> String {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = source.to_owned();
        crate::preprocess_for_layout(&mut source, settings.layout);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:?}");
        HtmlRender.render(&tree, &page_info, &settings).body
    }

    fn render_wikijump(source: &str) -> String {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:?}");
        HtmlRender.render(&tree, &page_info, &settings).body
    }

    #[test]
    fn wikijump_inline_containers_split_at_paragraph_breaks() {
        for (source, expected) in [
            (
                "Para [[span]]A1 one\n\npara two B2[[/span]] C3",
                "<p>Para <span>A1 one</span></p><p><span>para two B2</span> C3</p>",
            ),
            (
                "Para [[span class=\"x\"]]A1 one\n\npara two B2[[/span]] C3",
                concat!(
                    "<p>Para <span class=\"x\">A1 one</span></p>",
                    "<p><span class=\"x\">para two B2</span> C3</p>",
                ),
            ),
            (
                "Para [[size 150%]]A1 one\n\npara two B2[[/size]] C3",
                concat!(
                    "<p>Para <span style=\"font-size: 150%;\">A1 one</span></p>",
                    "<p><span style=\"font-size: 150%;\">para two B2</span> C3</p>",
                ),
            ),
            (
                "Para [[b]]A1 one\n\npara two B2[[/b]] C3",
                "<p>Para <strong>A1 one</strong></p><p><strong>para two B2</strong> C3</p>",
            ),
            (
                "Para [[i]]A1 one\n\npara two B2[[/i]] C3",
                "<p>Para <em>A1 one</em></p><p><em>para two B2</em> C3</p>",
            ),
            (
                "[[span]]A\n\nB\n\nC[[/span]]",
                "<p><span>A</span></p><p><span>B</span></p><p><span>C</span></p>",
            ),
            (
                "[[span]]A [[span]]B\n\nC[[/span]] D[[/span]]",
                concat!(
                    "<p><span>A <span>B</span></span></p>",
                    "<p><span><span>C</span> D</span></p>",
                ),
            ),
            (
                "x [[span]]\n\nA[[/span]] y",
                "<p>x</p><p><span>A</span> y</p>",
            ),
            (
                "x [[span]]A\n\n[[/span]] y",
                "<p>x <span>A</span></p><p> y</p>",
            ),
            ("x [[span]]\n\n[[/span]] y", "<p>x</p><p> y</p>"),
            (
                "[[div]]Para [[span]]A1\n\nB2[[/span]] C3[[/div]]",
                "<div><p>Para <span>A1</span></p><p><span>B2</span> C3</p></div>",
            ),
        ] {
            assert_eq!(render_wikijump(source), expected, "{source}");
        }

        assert_eq!(
            render_wikijump("Para [[span]]A1 one\npara two B2[[/span]] C3"),
            "<p>Para <span>A1 one<br>para two B2</span> C3</p>",
        );
    }

    #[test]
    fn non_paragraph_safe_multiple_elements_do_not_reserve_current_paragraph() {
        let mut stack = ParagraphStack::new();
        push_elements(
            &mut stack,
            Elements::Multiple(vec![Element::HorizontalRule, Element::HorizontalRule]),
            false,
        );

        assert_eq!(stack.current_capacity(), 0);
    }

    #[test]
    fn empty_non_paragraph_safe_result_unwraps_pending_content() {
        let mut stack = ParagraphStack::new();
        stack.push_element(text!("alpha"), true);
        stack.push_element(Element::LineBreak, true);

        push_elements(&mut stack, Elements::None, false);

        assert_eq!(
            stack.into_elements(),
            vec![text!("alpha"), Element::LineBreak],
        );
    }

    #[test]
    fn empty_quote_control_does_not_leave_a_line_break() {
        assert_eq!(render_wikidot("a\n>b"), "<p>a</p>");
    }

    #[test]
    fn wikidot_nbsp_only_line_stays_in_its_surrounding_paragraph() {
        assert_eq!(
            render_wikidot("Alpha\n\u{00a0}\nBeta"),
            "<p>Alpha<br>\n\u{00a0}<br>\nBeta</p>",
        );
    }

    #[test]
    fn wikidot_html_block_starts_a_new_paragraph() {
        assert_eq!(
            render_wikidot(
                "before\n[[html]]\n<b>first</b>\n[[/html]]\nmiddle\n[[html]]\n<b>second</b>\n[[/html]]\nafter",
            ),
            concat!(
                "<p>before</p>",
                "<p><iframe src=\"https://example.com/\" allowtransparency=\"true\" frameborder=\"0\" class=\"html-block-iframe\"></iframe><br>\n",
                "middle</p>",
                "<p><iframe src=\"https://example.com/\" allowtransparency=\"true\" frameborder=\"0\" class=\"html-block-iframe\"></iframe><br>\n",
                "after</p>",
            ),
        );
    }

    #[test]
    fn hidden_boundary_propagates_a_mismatched_child_close_error() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("plain");
        let parser = Parser::new(&tokenization, &page_info, &settings);
        let finished = finish_hidden_boundary(
            true,
            parser.make_err(ParseErrorKind::BlockExpectedEnd),
        )
        .unwrap();
        let error = parser.make_err(ParseErrorKind::BlockExpectedEnd);
        let propagated = finish_hidden_boundary(false, error).unwrap_err();

        assert!(finished);
        assert_eq!(propagated.kind(), ParseErrorKind::BlockExpectedEnd);
    }

    #[test]
    fn discarded_controls_are_invisible_without_losing_physical_line_occupancy() {
        assert_eq!(render_wikidot("\0\t \t\t\nΩ"), "<p><br>\nΩ</p>",);
        assert_eq!(
            render_wikidot("\u{0007}\talpha \u{000b} beta"),
            "<p>alpha beta</p>",
        );
    }

    #[test]
    fn discarded_control_remains_a_structural_syntax_barrier() {
        assert_eq!(render_wikidot("\0> quote"), "<p>&gt; quote</p>",);
    }

    #[test]
    fn discarded_control_does_not_preserve_leading_line_whitespace() {
        assert_eq!(
            render_wikidot("a\r\u{0001}\t\u{fffd}\0b"),
            "<p>a<br>\nb</p>",
        );
    }

    #[test]
    fn wikidot_preserves_an_underscore_at_the_start_of_a_line() {
        assert_eq!(render_wikidot("_\na"), "<p>_<br>\na</p>");
        assert_eq!(render_wikidot(" _\na"), "<p><br>\na</p>");
    }

    #[test]
    fn wikidot_terminal_spaced_underscore_emits_two_line_breaks() {
        assert_eq!(render_wikidot("a\\ _"), "\n\na\\<br>\n<br>\n");
    }

    #[test]
    fn wikidot_terminal_backslash_becomes_an_unwrapped_line_break() {
        assert_eq!(render_wikidot("\\"), "");
        assert_eq!(render_wikidot("alpha\\"), "\n\nalpha<br>\n");
        assert_eq!(
            render_wikidot("first\nsecond\\"),
            "\n\nfirst<br>\nsecond<br>\n",
        );
        assert_eq!(
            render_wikidot("paragraph\n\nlast\\"),
            "<p>paragraph</p>\nlast<br>\n",
        );
        assert_eq!(render_wikidot("a\u{fffd}\\"), "\n\na1");
    }

    #[test]
    fn wikidot_comment_only_line_does_not_leave_a_line_break() {
        assert_eq!(
            render_wikidot("alpha\n[!-- hidden --]\nomega"),
            "<p>alpha<br>\nomega</p>",
        );
        assert_eq!(
            render_wikidot("alpha\n[!-- hidden --]\n\nomega"),
            "<p>alpha</p><p>omega</p>",
        );
        assert_eq!(
            render_wikidot("alpha\n[!-- hidden --] visible\nomega"),
            "<p>alpha visible<br>\nomega</p>",
        );
        assert_eq!(
            render_wikidot("before [!-- hidden --] after"),
            "<p>before after</p>",
        );
        assert_eq!(
            render_wikidot(
                "[!-- --]OMEGA_TRUE[!-- --]\n[!-- OMEGA_FALSE[!-- --]\n[!-- 2 OMEGA_COMMENT --]\nOMEGA_AFTER",
            ),
            "<p>OMEGA_TRUE<br>\nOMEGA_AFTER</p>",
        );
        assert_eq!(
            render_wikidot("before\n\n[!-- --]\nbranch body\n[!----]\n\n\nmiddle"),
            "<p>before</p><p>branch body</p><p>middle</p>",
        );
    }

    #[test]
    fn wikidot_comment_after_advanced_list_preserves_the_list_break() {
        assert_eq!(
            render_wikidot(
                "[[ol]][[li]]before[[/li]][[/ol]]\n\n[!-- hidden --]\n[[ul]][[li]]after[[/li]][[/ul]]",
            ),
            "<ol>\n<li>before</li>\n</ol><br>\n<ul>\n<li>after</li>\n</ul><br>\n",
        );
    }

    #[test]
    fn wikidot_aligned_image_paragraph_break_keeps_newline_before_bare_run() {
        assert_eq!(
            render_wikidot(
                "FLOAT RIGHT [[f>image landscape.jpg]]\n\nEXTERNAL [[image https://example.com/some-image.webp]]",
            ),
            concat!(
                "<p>FLOAT RIGHT</p>",
                "<div class=\"image-container floatright\">",
                "<img src=\"https://sandbox.wjfiles.com/local--files/some-page/landscape.jpg\" class=\"image\" alt=\"landscape.jpg\">",
                "</div>",
                "\nEXTERNAL ",
                "<img src=\"https://example.com/some-image.webp\" class=\"image\" alt=\"some-image.webp\">",
            ),
        );

        assert_eq!(
            render_wikidot("[[f>image landscape.jpg]]\n\nplain"),
            concat!(
                "<div class=\"image-container floatright\">",
                "<img src=\"https://sandbox.wjfiles.com/local--files/some-page/landscape.jpg\" class=\"image\" alt=\"landscape.jpg\">",
                "</div>",
                "<p>plain</p>",
            ),
        );

        assert_eq!(
            render_wikidot(
                "[[f>image landscape.jpg]]\nEXTERNAL [[image https://example.com/some-image.webp]]",
            ),
            concat!(
                "<div class=\"image-container floatright\">",
                "<img src=\"https://sandbox.wjfiles.com/local--files/some-page/landscape.jpg\" class=\"image\" alt=\"landscape.jpg\">",
                "</div>",
                "EXTERNAL ",
                "<img src=\"https://example.com/some-image.webp\" class=\"image\" alt=\"some-image.webp\">",
            ),
        );

        assert_eq!(
            render_wikidot("[[f>image landscape.jpg]]\n\n"),
            concat!(
                "<div class=\"image-container floatright\">",
                "<img src=\"https://sandbox.wjfiles.com/local--files/some-page/landscape.jpg\" class=\"image\" alt=\"landscape.jpg\">",
                "</div>",
            ),
        );
    }

    #[test]
    fn line_after_block_is_unwrapped_only_in_wikidot_layout() {
        let page_info = PageInfo::dummy();
        let input = "[[div]]\nblock\n[[/div]]\nafter";
        let render = |layout| {
            let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
            let tokenization = crate::tokenize(input);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            assert!(errors.is_empty(), "{layout:?}: {errors:#?}");
            HtmlRender.render(&tree, &page_info, &settings).body
        };

        let wikidot = render(Layout::Wikidot);
        let wikijump = render(Layout::Wikijump);
        assert!(wikidot.contains("</div><br>\nafter"), "{wikidot}");
        assert!(!wikidot.contains("<p>after</p>"), "{wikidot}");
        assert!(wikijump.contains("</div><p>after</p>"), "{wikijump}");
    }
}
