/*
 * parsing/paragraph/stack.rs
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

use crate::parsing::prelude::*;
use crate::tree::{
    AttributeMap, Container, ContainerType, ListItem, PartialElement, TableType,
    WikidotBlockCloseKind, WikidotBlockShell,
};
use std::borrow::Cow;
use std::mem;

pub(crate) fn collapsible_has_direct_literal_nested_opener(
    element: &Element<'_>,
) -> bool {
    let Element::Collapsible { elements, .. } = element else {
        return false;
    };
    elements.iter().any(|element| {
        let Element::Container(container) = element else {
            return false;
        };
        if container.ctype() != ContainerType::Paragraph {
            return false;
        }
        let literal: String = container
            .elements()
            .iter()
            .filter_map(|element| match element {
                Element::Text(text) | Element::Raw(text) => Some(text.as_ref()),
                _ => None,
            })
            .collect();
        literal.contains("[[collapsible")
    })
}

#[derive(Debug, Default)]
pub struct ParagraphStack<'t> {
    wikidot: bool,

    current: Vec<Element<'t>>,

    pending_suppression_seams: Vec<Element<'t>>,

    current_unwrapped: bool,

    current_has_discarded_control: bool,
    wikidot_complete_raw_line_occupied: bool,
    wikidot_complete_raw_run_after_block: bool,
    wikidot_demoted_comment_break: bool,

    pending_unwrapped_separator: bool,
    paragraph_break_since_element: bool,

    pending_aligned_image_separator: Option<usize>,
    unwrapped_after_block_line: bool,
    wikidot_footnote_joined_math_break: bool,
    wikidot_literal_iftags_line: bool,
    wikidot_literal_div_line: bool,
    wikidot_div_literal_run_wrap: bool,
    wikidot_div_literal_run: Option<(usize, usize)>,
    wikidot_div_literal_run_bare: bool,
    pending_wikidot_div_literal: bool,
    trim_unwrapped_trailing_line_break: bool,
    suppress_next_line_break: bool,
    suppressed_line_break_restarts_paragraph: bool,
    wikidot_empty_list_break: bool,
    wikidot_embed_error_residual: bool,
    next_element_started_line: bool,
    wikidot_simple_table_boundary: bool,
    wikidot_reopen_for_footnote: bool,
    wikidot_bare_after_explicit_list: bool,
    wikidot_explicit_pre_candidate: bool,
    wikidot_suppress_explicit_pre_dissolve: bool,
    wikidot_post_empty_list_break: bool,

    finished: Vec<Element<'t>>,

    errors: Vec<ParseError>,
}

impl<'t> ParagraphStack<'t> {
    #[inline]
    pub fn new() -> Self {
        ParagraphStack::default()
    }

    #[inline]
    pub fn new_wikidot() -> Self {
        ParagraphStack {
            wikidot: true,
            ..ParagraphStack::default()
        }
    }

    #[inline]
    pub fn current_empty(&self) -> bool {
        self.current.is_empty() && !self.current_has_discarded_control
    }

    #[inline]
    pub(crate) fn wikidot_line_break_follows_block(&self) -> bool {
        self.wikidot
            && self.current_empty()
            && self.finished.last().is_some_and(|element| {
                matches!(
                    element,
                    Element::Container(container)
                        if matches!(
                            container.ctype(),
                            ContainerType::Div
                                | ContainerType::Note
                                | ContainerType::Blockquote
                                | ContainerType::Align(_)
                        )
                ) || matches!(
                    element,
                    Element::Code(_) | Element::Collapsible { .. } | Element::TabView(_)
                ) || matches!(
                    element,
                    Element::Partial(
                        PartialElement::WikidotBlockClose(
                            WikidotBlockCloseKind::Div { .. }
                                | WikidotBlockCloseKind::Collapsible { .. }
                                | WikidotBlockCloseKind::CollapsibleCrossedBottom { .. }
                                | WikidotBlockCloseKind::Note { .. }
                                | WikidotBlockCloseKind::Align { .. }
                        ) | PartialElement::WikidotBlockOpen(WikidotBlockShell::Div {
                            scored: true,
                            ..
                        })
                    )
                )
            })
    }

    /// Live's `Deflist` rule (position 46) matches `\n((: ).*\n)` and re-emits the
    /// leading newline before its tokens. When the previous element is a closed
    /// block whose closer leaves a single newline, that re-emitted newline becomes
    /// a `<br>` before the definition list. `Blockquote` is excluded: its closer
    /// emits `"\n\n"` instead, so live has no `<br>` there.
    #[inline]
    pub(crate) fn wikidot_definition_list_break_follows_block(&self) -> bool {
        self.wikidot
            && self.current_empty()
            && self.finished.last().is_some_and(|element| {
                matches!(
                    element,
                    Element::Container(container)
                        if matches!(
                            container.ctype(),
                            ContainerType::Div | ContainerType::Note | ContainerType::Align(_)
                        )
                ) || matches!(
                    element,
                    Element::Code(_) | Element::Collapsible { .. } | Element::TabView(_)
                ) || matches!(
                    element,
                    Element::Partial(PartialElement::WikidotBlockClose(
                        WikidotBlockCloseKind::Div { .. }
                            | WikidotBlockCloseKind::Collapsible { .. }
                            | WikidotBlockCloseKind::CollapsibleCrossedBottom { .. }
                            | WikidotBlockCloseKind::Note { .. }
                            | WikidotBlockCloseKind::Align { .. }
                    ))
                )
            })
    }

    /// Whether the current chunk starts with a line break immediately after a
    /// math block. Live's `Math` rule pads its token with blank lines, but the
    /// `Footnote` rule then deletes all whitespace before a reference, joining
    /// the reference to the math token's own chunk (C1: not wrapped).
    #[inline]
    pub(crate) fn wikidot_line_break_follows_math(&self) -> bool {
        self.wikidot
            && self.current_empty()
            && matches!(self.finished.last(), Some(Element::Math { .. }))
    }

    /// Live's `Math` rule pads its token with blank lines and the `Footnote`
    /// rule then deletes the whitespace before a reference, so the reference
    /// joins the math token's chunk instead of starting a wrapped paragraph.
    /// Its trailing blank line is part of the math chunk's join, so no
    /// paragraph-break separator follows it.
    #[inline]
    pub(crate) fn mark_wikidot_footnote_joined_math_break(&mut self) {
        if self.wikidot {
            self.current_unwrapped = true;
            self.unwrapped_after_block_line = true;
            self.wikidot_footnote_joined_math_break = true;
        }
    }

    #[inline]
    pub(crate) fn wikidot_list_break_follows_list(&self) -> bool {
        self.wikidot
            && self.current_empty()
            && matches!(self.finished.last(), Some(Element::LineBreak))
            && self
                .finished
                .iter()
                .rev()
                .nth(1)
                .is_some_and(|element| matches!(element, Element::List { .. }))
    }

    #[inline]
    pub(crate) fn mark_wikidot_empty_list_break(&mut self) {
        if self.wikidot {
            self.wikidot_empty_list_break = true;
        }
    }

    fn wikidot_post_explicit_list_break(&self) -> bool {
        if !self.wikidot || self.finished.len() < 2 {
            return false;
        }
        let len = self.finished.len();
        matches!(self.finished[len - 1], Element::LineBreak)
            && matches!(
                self.finished[len - 2],
                Element::List {
                    wikidot_explicit_block: true,
                    ..
                }
            )
    }

    fn wikidot_poppable_post_list_break(&self) -> bool {
        self.wikidot_post_explicit_list_break()
            || (self.wikidot_post_empty_list_break
                && matches!(self.finished.last(), Some(Element::LineBreak)))
    }

    fn wikidot_pop_post_list_break(&mut self) {
        if self.wikidot_poppable_post_list_break() {
            let index = self.finished.len() - 1;
            self.finished.remove(index);
            self.shift_wikidot_div_literal_run_before(index);
        }
        self.wikidot_post_empty_list_break = false;
    }

    fn wikidot_post_list_break_absorber(element: &Element<'_>) -> bool {
        match element {
            Element::List { .. } | Element::Code(_) | Element::TabView(_) => true,
            Element::Table(table) => table.table_type != TableType::Simple,
            _ => false,
        }
    }

    fn wikidot_post_list_blank_swallower(element: &Element<'_>) -> bool {
        match element {
            Element::List {
                wikidot_explicit_block: false,
                ..
            }
            | Element::Code(_)
            | Element::Collapsible { .. }
            | Element::HorizontalRule
            | Element::BibliographyBlock { .. }
            | Element::DefinitionList(_)
            | Element::Gallery(_)
            | Element::Math { .. }
            | Element::Embed(_)
            | Element::Iframe { .. }
            | Element::Html { .. }
            | Element::TableOfContents { .. }
            | Element::FootnoteBlock { .. }
            | Element::Module(_)
            | Element::ClearFloat(_)
            | Element::Image {
                alignment: Some(_), ..
            } => true,
            Element::Table(table) => table.table_type != TableType::Simple,
            Element::Container(container) => matches!(
                container.ctype(),
                ContainerType::Div
                    | ContainerType::Note
                    | ContainerType::Blockquote
                    | ContainerType::Align(_)
                    | ContainerType::Header(_)
            ),
            _ => false,
        }
    }

    fn wikidot_explicit_pre_dissolver(element: &Element<'_>, attached: bool) -> bool {
        if !attached {
            return match element {
                Element::List {
                    wikidot_explicit_block: true,
                    ..
                }
                | Element::TabView(_) => true,
                Element::Table(table) => table.table_type == TableType::Simple,
                _ => false,
            };
        }
        match element {
            Element::List {
                wikidot_explicit_block: false,
                ..
            }
            | Element::Code(_) => false,
            Element::Table(table) => table.table_type == TableType::Simple,
            _ => true,
        }
    }

    fn wikidot_dissolve_explicit_pre_candidate(&mut self) {
        self.wikidot_explicit_pre_candidate = false;
        if self.wikidot_suppress_explicit_pre_dissolve {
            return;
        }
        let len = self.finished.len();
        if len < 2 {
            return;
        }
        let paragraph_index = if matches!(self.finished[len - 1], Element::LineBreak) {
            if len < 3 {
                return;
            }
            len - 3
        } else {
            len - 2
        };
        if !matches!(
            self.finished.get(paragraph_index),
            Some(Element::Container(container))
                if container.ctype() == ContainerType::Paragraph
        ) || !matches!(
            self.finished.get(paragraph_index + 1),
            Some(Element::List {
                wikidot_explicit_block: true,
                ..
            })
        ) {
            return;
        }
        let paragraph = self.finished.remove(paragraph_index);
        self.shift_wikidot_div_literal_run_before(paragraph_index);
        let Element::Container(container) = paragraph else {
            return;
        };
        let elements: Vec<Element<'t>> = container.into();
        for (offset, element) in elements.into_iter().enumerate() {
            self.finished.insert(paragraph_index + offset, element);
        }
    }

    fn wikidot_note_explicit_pre_candidate(&mut self, pre_attached: bool) {
        if !self.wikidot
            || self.wikidot_suppress_explicit_pre_dissolve
            || !pre_attached
            || self.finished.len() < 2
        {
            return;
        }
        let len = self.finished.len();
        if matches!(
            self.finished[len - 2],
            Element::Container(ref container)
                if container.ctype() == ContainerType::Paragraph
        ) {
            self.wikidot_explicit_pre_candidate = true;
        }
    }

    fn wikidot_clear_post_explicit_list_state(&mut self) {
        self.wikidot_bare_after_explicit_list = false;
        self.wikidot_explicit_pre_candidate = false;
        self.wikidot_post_empty_list_break = false;
    }

    fn wikidot_element_contains_unaligned_image(element: &Element<'_>) -> bool {
        match element {
            Element::Image {
                alignment: None, ..
            } => true,
            Element::Delayed(delayed) => delayed.image_alignment() == Some(None),
            Element::Container(container) => container
                .elements()
                .iter()
                .any(Self::wikidot_element_contains_unaligned_image),
            Element::Color { elements, .. } | Element::Anchor { elements, .. } => {
                elements
                    .iter()
                    .any(Self::wikidot_element_contains_unaligned_image)
            }
            _ => false,
        }
    }

    pub(crate) fn wikidot_current_ends_with_scored_alias(&self) -> bool {
        self.wikidot && Self::wikidot_run_ends_with_scored_alias(&self.current)
    }

    pub(crate) fn wikidot_run_ends_with_scored_alias(elements: &[Element<'_>]) -> bool {
        let mut rest = elements;
        while let Some((last, head)) = rest.split_last() {
            let skip = match last {
                Element::LineBreak | Element::LineBreaks(_) => true,
                Element::Delayed(delayed) => delayed.is_suppression_seam(),
                _ => false,
            };
            if !skip {
                break;
            }
            rest = head;
        }
        let scored_kind = match rest.last() {
            Some(Element::Partial(PartialElement::InlineSpanClose(_))) => 0,
            Some(Element::Partial(PartialElement::InlineAnchorClose(_))) => 1,
            _ => return false,
        };
        let mut depth = 0usize;
        for element in rest.iter().rev() {
            match element {
                Element::Partial(PartialElement::InlineSpanClose(_))
                    if scored_kind == 0 =>
                {
                    depth += 1
                }
                Element::Partial(PartialElement::InlineAnchorClose(_))
                    if scored_kind == 1 =>
                {
                    depth += 1
                }
                Element::Partial(PartialElement::InlineSpanOpen(attributes, ..))
                    if scored_kind == 0 =>
                {
                    if depth == 0 {
                        return false;
                    }
                    depth -= 1;
                    if depth == 0 {
                        return attributes.get().contains_key("data-ftml-score-span");
                    }
                }
                Element::Partial(PartialElement::InlineAnchorOpen(attributes, ..))
                    if scored_kind == 1 =>
                {
                    if depth == 0 {
                        return false;
                    }
                    depth -= 1;
                    if depth == 0 {
                        return attributes.get().contains_key(
                            crate::parsing::inline_scope::WIKIDOT_SCORE_ANCHOR_ATTRIBUTE,
                        );
                    }
                }
                _ => {}
            }
        }
        false
    }

    pub(crate) fn wikidot_element_is_scored_alias_open(element: &Element<'_>) -> bool {
        match element {
            Element::Partial(PartialElement::InlineSpanOpen(attributes, ..)) => {
                attributes.get().contains_key("data-ftml-score-span")
            }
            Element::Partial(PartialElement::InlineAnchorOpen(attributes, ..)) => {
                attributes.get().contains_key(
                    crate::parsing::inline_scope::WIKIDOT_SCORE_ANCHOR_ATTRIBUTE,
                )
            }
            _ => false,
        }
    }

    /// Whether the current run would end with a paired scored span or anchor
    /// close once trailing ASCII spaces are trimmed.
    fn wikidot_trailing_space_after_scored_alias(&self) -> bool {
        let mut index = self.current.len();
        let mut saw_space = false;
        while index > 0 {
            index -= 1;
            match &self.current[index] {
                Element::Text(text) if text == " " => saw_space = true,
                Element::Delayed(delayed) if delayed.is_suppression_seam() => {}
                _ => {
                    return saw_space
                        && Self::wikidot_run_ends_with_scored_alias(
                            &self.current[..=index],
                        );
                }
            }
        }
        false
    }

    fn wikidot_element_has_run_content(element: &Element<'_>) -> bool {
        match element {
            Element::LineBreak | Element::LineBreaks(_) | Element::Delayed(_) => false,
            Element::Text(text) | Element::Raw(text) => {
                !text.chars().all(|character| character.is_whitespace())
            }
            _ => true,
        }
    }

    fn wikidot_current_has_run_content(&self) -> bool {
        self.current
            .iter()
            .any(Self::wikidot_element_has_run_content)
    }

    fn wikidot_absorb_post_list_break_for_prose(&mut self, attached: bool) {
        if !self.wikidot {
            return;
        }
        if attached {
            self.wikidot_pop_post_list_break();
        }
        self.wikidot_explicit_pre_candidate = false;
    }

    fn wikidot_handle_post_list_block_follower(
        &mut self,
        element: &Element<'_>,
        attached: bool,
    ) {
        if !self.wikidot
            || (!self.wikidot_bare_after_explicit_list
                && !self.wikidot_explicit_pre_candidate)
        {
            return;
        }
        if self.wikidot_poppable_post_list_break()
            && (attached && Self::wikidot_post_list_break_absorber(element)
                || !attached && Self::wikidot_post_list_blank_swallower(element))
        {
            self.wikidot_pop_post_list_break();
        }
        if self.wikidot_explicit_pre_candidate
            && Self::wikidot_explicit_pre_dissolver(element, attached)
        {
            self.wikidot_dissolve_explicit_pre_candidate();
        }
        self.wikidot_clear_post_explicit_list_state();
    }

    #[inline]
    pub(crate) fn wikidot_inline_residual_follows_collapsible(&self) -> bool {
        self.wikidot
            && self.current_empty()
            && self.unwrapped_after_block_line
            && matches!(self.finished.last(), Some(Element::Collapsible { .. }))
    }

    #[inline]
    pub(crate) fn record_next_element_line_start(&mut self, start_of_line: bool) {
        self.next_element_started_line = start_of_line;
    }

    #[cfg(test)]
    pub fn current_capacity(&self) -> usize {
        self.current.capacity()
    }

    #[inline]
    pub fn mark_current_unwrapped(&mut self) {
        if !self.current.is_empty() {
            self.current_unwrapped = true;
        }
    }

    #[inline]
    pub fn mark_wikidot_terminal_backslash(&mut self) {
        if self.wikidot && !self.current.is_empty() && !self.current_unwrapped {
            let separator = if self.finished.is_empty() {
                "\n\n"
            } else {
                "\n"
            };
            self.current.insert(0, text!(separator));
            self.current_unwrapped = true;
        }
    }

    #[inline]
    pub fn mark_wikidot_continued_block_boundary(&mut self) {
        if self.wikidot && !self.current.is_empty() {
            self.current.push(text!("\n"));
            self.current_unwrapped = true;
        }
    }

    #[inline]
    pub(crate) fn mark_next_unwrapped(&mut self) {
        self.current_unwrapped = true;
    }

    #[inline]
    pub(crate) fn mark_next_unwrapped_after_block(&mut self) {
        self.current_unwrapped = true;
        self.unwrapped_after_block_line = true;
    }

    #[inline]
    pub(crate) fn mark_wikidot_same_line_div_residual(&mut self) {
        if self.wikidot {
            self.current.push(text!("\n"));
            self.current_unwrapped = true;
            self.unwrapped_after_block_line = true;
        }
    }

    /// Live Wikidot leaves text on the note closer's line as a bare
    /// continuation, exactly like a div closer's same-line residual.
    #[inline]
    pub(crate) fn mark_wikidot_same_line_note_residual(&mut self) {
        self.mark_wikidot_same_line_div_residual();
    }

    #[inline]
    pub(crate) fn mark_wikidot_same_line_code_residual(&mut self) {
        if self.wikidot {
            self.current.push(text!("\n"));
            self.current_unwrapped = true;
            self.unwrapped_after_block_line = true;
        }
    }

    pub(crate) fn mark_wikidot_same_line_embed_residual(&mut self) {
        if self.wikidot {
            self.current.push(text!("\n"));
            self.current_unwrapped = true;
            self.unwrapped_after_block_line = true;
            self.wikidot_embed_error_residual = true;
        }
    }

    #[inline]
    pub(crate) fn mark_wikidot_literal_div_line(&mut self) {
        if self.wikidot && !self.wikidot_literal_iftags_line {
            let continues_raw_text = matches!(
                self.finished.last(),
                Some(Element::Text(_) | Element::Raw(_))
            );
            if !continues_raw_text
                && !self.finished.is_empty()
                && !self.current.is_empty()
            {
                self.current.insert(0, text!("\n"));
            }
            self.current_unwrapped = true;
            self.wikidot_literal_div_line = true;
        }
    }

    #[inline]
    pub(crate) fn enable_wikidot_div_literal_run_wrap(&mut self) {
        self.wikidot_div_literal_run_wrap = true;
    }

    /// Body parsers own their own paragraph splitting: live keeps the
    /// paragraph around text that precedes an explicit list inside a block
    /// body (`<div><p>A<br></p><ul>`), so the top-level pre-list dissolve
    /// must not run there.
    #[inline]
    pub(crate) fn suppress_wikidot_explicit_pre_dissolve(&mut self) {
        self.wikidot_suppress_explicit_pre_dissolve = true;
    }

    #[inline]
    pub(crate) fn note_wikidot_div_literal_push(&mut self) {
        if self.wikidot_div_literal_run_wrap {
            self.pending_wikidot_div_literal = true;
        }
    }

    fn register_wikidot_div_literal_range(&mut self, start: usize, end: usize) {
        if !self.wikidot_div_literal_run_wrap || start >= end {
            return;
        }
        if let Some((_, run_end)) = &mut self.wikidot_div_literal_run {
            let previous = *run_end - 1;
            *run_end = end;
            if let Some(Element::Text(text)) = self.finished.get_mut(previous)
                && text.ends_with('\n')
            {
                let kept = text.len() - 1;
                *text = Cow::Owned(text[..kept].to_owned());
            }
            if let Some(Element::Text(text)) = self.finished.get_mut(start)
                && text.starts_with('\n')
            {
                *text = Cow::Owned(text[1..].to_owned());
            }
        } else {
            let bare = start > 0
                && matches!(
                    self.finished.get(start - 1),
                    Some(Element::Container(container))
                        if container.ctype() == ContainerType::Div
                );
            self.wikidot_div_literal_run = Some((start, end));
            self.wikidot_div_literal_run_bare = bare;
        }
    }

    fn close_wikidot_div_literal_run(&mut self, followed_by_div: bool) {
        if !self.wikidot_div_literal_run_wrap {
            return;
        }
        let Some((start, end)) = self.wikidot_div_literal_run.take() else {
            return;
        };
        let bare = self.wikidot_div_literal_run_bare;
        self.wikidot_div_literal_run_bare = false;
        if bare || followed_by_div || start >= end || end > self.finished.len() {
            return;
        }
        let mut elements: Vec<Element<'t>> = self.finished.drain(start..end).collect();
        elements.retain(
            |element| !matches!(element, Element::Text(text) if text.as_ref() == "\n"),
        );
        if let Some(Element::Text(text)) = elements.first_mut()
            && text.starts_with('\n')
        {
            *text = Cow::Owned(text[1..].to_owned());
        }
        if elements.is_empty() {
            return;
        }
        let paragraph = Element::Container(Container::new(
            ContainerType::Paragraph,
            elements,
            AttributeMap::new(),
        ));
        self.finished.insert(start, paragraph);
    }

    fn shift_wikidot_div_literal_run_before(&mut self, removed: usize) {
        if let Some((start, end)) = &mut self.wikidot_div_literal_run {
            if removed < *start {
                *start -= 1;
                *end -= 1;
            } else if removed < *end {
                *end -= 1;
            }
        }
    }

    #[inline]
    pub(crate) fn mark_wikidot_literal_iftags_line(&mut self) {
        if self.wikidot {
            self.wikidot_literal_iftags_line = true;
        }
    }

    pub(crate) fn mark_wikidot_simple_table_boundary(&mut self) {
        if self.wikidot {
            self.wikidot_simple_table_boundary = true;
        }
    }

    pub(crate) fn merge_wikidot_adjacent_simple_tables(&mut self) {
        if !self.wikidot || self.wikidot_simple_table_boundary {
            self.wikidot_simple_table_boundary = false;
            return;
        }

        let mut current = match self.finished.pop() {
            Some(Element::Table(table)) => table,
            Some(other) => {
                self.finished.push(other);
                return;
            }
            None => return,
        };

        if let Some(Element::Table(previous)) = self.finished.last_mut()
            && previous.table_type == TableType::Simple
            && current.table_type == TableType::Simple
        {
            previous.rows.append(&mut current.rows);
        } else {
            self.finished.push(Element::Table(current));
        }
    }

    #[inline]
    pub fn mark_wikidot_tabview_boundary(&mut self) {
        if !self.wikidot || self.current.is_empty() {
            return;
        }
        self.trim_trailing_ascii_space();
        if !matches!(self.current.last(), Some(Element::LineBreak)) {
            self.current.push(Element::LineBreak);
        }
        self.current_unwrapped = true;
    }

    #[inline]
    pub fn mark_discarded_control(&mut self) {
        self.mark_wikidot_invisible_line_occupancy();
    }

    #[inline]
    pub(crate) fn mark_wikidot_invisible_line_occupancy(&mut self) {
        if self.wikidot {
            self.current_has_discarded_control = true;
        }
    }

    #[inline]
    pub(crate) fn mark_wikidot_complete_raw_line_occupancy(&mut self) {
        self.mark_wikidot_invisible_line_occupancy();
        self.wikidot_complete_raw_line_occupied = self.wikidot;
        if self.current_unwrapped && self.unwrapped_after_block_line {
            self.wikidot_complete_raw_run_after_block = true;
        }
    }

    #[inline]
    pub(crate) fn wikidot_complete_raw_line_occupied(&self) -> bool {
        self.wikidot_complete_raw_line_occupied
    }

    #[inline]
    pub(crate) fn clear_wikidot_complete_raw_line_occupancy(&mut self) {
        self.wikidot_complete_raw_line_occupied = false;
    }

    #[inline]
    pub(crate) fn mark_wikidot_demoted_comment_break(&mut self) {
        self.wikidot_demoted_comment_break = self.wikidot;
    }

    #[inline]
    pub(crate) fn take_wikidot_demoted_comment_break(&mut self) -> bool {
        mem::take(&mut self.wikidot_demoted_comment_break)
    }

    #[inline]
    pub fn push_element(&mut self, element: Element<'t>, paragraph_safe: bool) {
        // L85: a lone leading break before an explicit list vanishes (live
        // keeps real breaks only with content: R1 keeps, para-start drops).
        if self.wikidot
            && matches!(
                &element,
                Element::List {
                    wikidot_explicit_block: true,
                    ..
                }
            )
            && matches!(self.current.last(), Some(Element::LineBreak))
            && self.current.iter().rev().skip(1).all(|element| {
                matches!(
                    element,
                    Element::Delayed(delayed) if delayed.is_suppression_seam()
                )
            })
        {
            self.current.pop();
        }
        let paragraph_break_separated = self.paragraph_break_since_element;
        if element != Element::LineBreak {
            self.paragraph_break_since_element = false;
        }
        if self.wikidot && matches!(element, Element::Footnote(_)) {
            if let Some(index) = self.current.iter().rposition(|element| {
                !matches!(element, Element::Delayed(delayed) if delayed.is_suppression_seam())
            }) && self.current[index] == Element::LineBreak
            {
                self.current.remove(index);
            }
        }
        if self.wikidot_reopen_for_footnote && matches!(element, Element::Footnote(_)) {
            self.wikidot_reopen_for_footnote = false;
            let Some(Element::Container(paragraph)) = self.finished.pop() else {
                unreachable!("footnote reopening requires a preceding paragraph");
            };
            debug_assert_eq!(paragraph.ctype(), ContainerType::Paragraph);
            self.current = paragraph.into();
            self.current.append(&mut self.pending_suppression_seams);
            self.current.push(element);
            return;
        }
        self.wikidot_reopen_for_footnote = false;

        if element != Element::LineBreak {
            if paragraph_safe || !self.current.is_empty() {
                self.current.append(&mut self.pending_suppression_seams);
            } else {
                self.finished.append(&mut self.pending_suppression_seams);
            }
        }
        if self.suppress_next_line_break {
            self.suppress_next_line_break = false;
            if element == Element::LineBreak {
                if self.suppressed_line_break_restarts_paragraph {
                    self.current_unwrapped = false;
                    self.unwrapped_after_block_line = false;
                    self.suppressed_line_break_restarts_paragraph = false;
                }
                return;
            }
            self.suppressed_line_break_restarts_paragraph = false;
        }
        if self.pending_wikidot_div_literal
            && !paragraph_break_separated
            && !self.current.is_empty()
            && matches!(element, Element::Text(_))
        {
            self.pending_wikidot_div_literal = false;
            let mut element = element;
            if let Element::Text(text) = &element
                && text.starts_with('\n')
            {
                if !matches!(self.current.last(), Some(Element::LineBreak)) {
                    self.current.push(Element::LineBreak);
                }
                element = Element::Text(Cow::Owned(text[1..].to_owned()));
            }
            self.current.push(element);
            return;
        }
        let aligned_image = matches!(
            element,
            Element::Image {
                alignment: Some(_),
                ..
            }
        ) || matches!(&element, Element::Delayed(delayed) if matches!(delayed.image_alignment(), Some(Some(_))));
        let unaligned_image = matches!(
            element,
            Element::Image {
                alignment: None,
                ..
            }
        ) || matches!(&element, Element::Delayed(delayed) if delayed.image_alignment() == Some(None));
        if self.pending_aligned_image_separator.is_some()
            && (aligned_image || !paragraph_safe)
        {
            self.remove_pending_aligned_image_separator();
        }
        let wikidot_html_block = self.wikidot && matches!(element, Element::Html { .. });
        let wikidot_section_marker =
            self.wikidot && matches!(&element, Element::ContentSeparator);
        let wikidot_simple_table = self.wikidot
            && matches!(
                &element,
                Element::Table(table) if table.table_type == TableType::Simple
            );
        let wikidot_alignment = self.wikidot
            && (matches!(
                &element,
                Element::Container(container)
                    if matches!(container.ctype(), ContainerType::Align(_))
            ) || matches!(
                &element,
                Element::Partial(PartialElement::WikidotBlockOpen(
                    WikidotBlockShell::Align { .. }
                ))
            ));

        let wikidot_collapsible =
            self.wikidot && matches!(element, Element::Collapsible { .. });
        let wikidot_collapsible_ends_with_blockquote = self.wikidot
            && matches!(
                &element,
                Element::Collapsible { elements, .. }
                    if matches!(
                        elements.last(),
                        Some(Element::Container(container))
                            if container.ctype() == ContainerType::Blockquote
                    )
            );

        if wikidot_collapsible
            && !self.current.is_empty()
            && !self.next_element_started_line
        {
            self.current_unwrapped = true;
        }

        if self.wikidot
            && !self.current.is_empty()
            && !self.next_element_started_line
            && matches!(
                element,
                Element::Partial(
                    PartialElement::WikidotBlockOpen(_)
                        | PartialElement::WikidotBlockClose(WikidotBlockCloseKind::Div {
                            scored: true,
                            ..
                        })
                )
            )
        {
            self.current_unwrapped = true;
        }

        if self.wikidot && matches!(element, Element::DefinitionList(_)) {
            if !self.current.is_empty() {
                self.trim_trailing_ascii_space();
                self.current.push(Element::LineBreak);
                self.current_unwrapped = true;
            } else if !paragraph_break_separated
                && self.wikidot_definition_list_break_follows_block()
            {
                self.current.push(Element::LineBreak);
                self.current_unwrapped = true;
            }
        }

        if wikidot_simple_table && !self.current.is_empty() {
            self.trim_trailing_ascii_space();
            self.current.push(Element::LineBreak);
            self.current_unwrapped = true;
        }

        if wikidot_alignment
            && !self.current.is_empty()
            && !matches!(self.current.last(), Some(Element::LineBreak))
        {
            self.trim_trailing_ascii_space();
            self.current.push(Element::LineBreak);
        }

        if self.wikidot
            && matches!(
                element,
                Element::Container(ref container)
                    if matches!(container.ctype(), ContainerType::Align(_))
            )
            && self.current.is_empty()
            && !paragraph_break_separated
            && matches!(
                self.finished.last(),
                Some(Element::Container(container))
                    if matches!(container.ctype(), ContainerType::Align(_))
            )
        {
            self.finished.push(Element::LineBreak);
        }

        if matches!(element, Element::TabView(_)) {
            self.mark_wikidot_tabview_boundary();
        }

        if wikidot_html_block {
            self.end_paragraph();
            if !self.wikidot_current_has_run_content() {
                self.wikidot_absorb_post_list_break_for_prose(!paragraph_break_separated);
            }
            self.current.push(element);
        } else if aligned_image {
            if self.wikidot && !self.current.is_empty() {
                self.pop_line_break();
            }
            self.end_paragraph();
            self.wikidot_handle_post_list_block_follower(
                &element,
                !paragraph_break_separated,
            );
            self.finished.push(element);
        } else if unaligned_image {
            if self.wikidot
                && self.finished.is_empty()
                && !self.current.is_empty()
                && !matches!(self.current.first(), Some(Element::Text(text)) if text.starts_with("\n\n"))
            {
                self.current.insert(0, text!("\n\n"));
            }
            if self.pending_unwrapped_separator
                && matches!(self.finished.last(), Some(Element::Text(text)) if text == " ")
            {
                self.finished.pop();
            }
            self.pending_unwrapped_separator = false;
            if !self.wikidot_current_has_run_content() {
                self.wikidot_absorb_post_list_break_for_prose(!paragraph_break_separated);
            }
            self.current.push(element);
            self.current_unwrapped = true;
        } else if paragraph_safe {
            // Content starting a fresh chunk right after a flat block closer
            // on the same line is a residual and stays bare (line-start
            // content is already covered by the break-after-block mark, and
            // blank lines must start wrapped paragraphs).
            if self.wikidot
                && self.current.is_empty()
                && !self.next_element_started_line
                && matches!(
                    self.finished.last(),
                    Some(Element::Partial(PartialElement::WikidotBlockClose(kind)))
                        if !matches!(kind, WikidotBlockCloseKind::BibliographyCrossed { .. })
                )
            {
                self.mark_next_unwrapped_after_block();
            }
            // A chunk-leading single space is indentation at a line start and
            // keeps the historical drop everywhere except directly after an
            // inline container flat lowering will literalise: its closer
            // ended the previous chunk mid-line, so the space is content the
            // rejoined run must keep.
            let chunk_leading_line_start = self.current.is_empty()
                && (self.next_element_started_line
                    || !self.wikidot_finished_is_literalise_bound_container());
            if self.wikidot
                && matches!(element, Element::Text(ref text) if text == " ")
                && (chunk_leading_line_start
                    || self.current.iter().all(|element| {
                        matches!(
                            element,
                            Element::Delayed(delayed) if delayed.is_suppression_seam()
                        )
                    })
                    || matches!(
                        self.current.last(),
                        Some(Element::Text(text)) if text == " "
                    )
                    || matches!(self.current.last(), Some(Element::LineBreak))
                    || (self.wikidot_embed_error_residual && self.current.len() == 1))
            {
                return;
            }
            if element == Element::LineBreak {
                self.trim_trailing_ascii_space();
            }
            if Self::wikidot_element_has_run_content(&element)
                && !self.wikidot_current_has_run_content()
            {
                self.wikidot_absorb_post_list_break_for_prose(!paragraph_break_separated);
            }
            if self.wikidot && Self::wikidot_element_contains_unaligned_image(&element) {
                self.current_unwrapped = true;
            }
            self.current.push(element);
        } else {
            let invisible_block_line = self.wikidot && element == Element::LineBreak;
            let escaped_note_prefix = self.wikidot_note_follows_escaped_prefix(&element);
            let nested_literal_collapsible =
                self.wikidot && collapsible_has_direct_literal_nested_opener(&element);
            let literal_table_before_explicit_list =
                self.wikidot_literal_table_opener_before_synthetic_list(&element);
            let literal_table_block_boundary = self.current_unwrapped
                && self.wikidot_current_line_is_literal_advanced_table_opener();
            if wikidot_section_marker
                && self.current_unwrapped
                && !self.current.is_empty()
                && !matches!(self.current.last(), Some(Element::LineBreak))
            {
                self.current.push(text!("\n"));
            }
            if literal_table_block_boundary {
                self.current.push(text!("\n"));
            }
            if literal_table_before_explicit_list {
                if self.finished.is_empty()
                    && !matches!(
                        self.current.first(),
                        Some(Element::Text(text)) if text.starts_with("\n\n")
                    )
                {
                    self.current.insert(0, text!("\n\n"));
                }
                self.current.push(Element::LineBreak);
                self.current_unwrapped = true;
            }
            let empty_list_break = if invisible_block_line {
                if !self.wikidot_empty_list_break {
                    self.pop_line_break();
                }
                mem::take(&mut self.wikidot_empty_list_break)
            } else {
                false
            };
            if escaped_note_prefix {
                self.current_unwrapped = true;
            }
            if self.wikidot_embed_error_residual && !self.current.is_empty() {
                self.trim_trailing_ascii_space();
                self.current.push(text!("\n"));
            }
            let is_explicit_list = matches!(
                &element,
                Element::List {
                    wikidot_explicit_block: true,
                    ..
                }
            );
            if is_explicit_list && self.wikidot {
                let len = self.current.len();
                if len >= 2
                    && self.current[len - 1] == Element::LineBreak
                    && self.current[len - 2] == Element::LineBreak
                {
                    self.current.pop();
                }
            }
            self.end_paragraph();
            if !invisible_block_line {
                if element.paragraph_safe() {
                    self.wikidot_absorb_post_list_break_for_prose(
                        !paragraph_break_separated,
                    );
                } else {
                    self.wikidot_handle_post_list_block_follower(
                        &element,
                        !paragraph_break_separated,
                    );
                }
            }
            if self.pending_wikidot_div_literal {
                self.pending_wikidot_div_literal = false;
                self.finished.push(element);
                let index = self.finished.len() - 1;
                self.register_wikidot_div_literal_range(index, index + 1);
            } else {
                let followed_by_div = matches!(
                    &element,
                    Element::Container(container)
                        if container.ctype() == ContainerType::Div
                );
                self.close_wikidot_div_literal_run(followed_by_div);
                self.finished.push(element);
            }
            if invisible_block_line {
                if empty_list_break {
                    self.wikidot_post_empty_list_break = true;
                    self.wikidot_bare_after_explicit_list = true;
                } else if self.wikidot_post_explicit_list_break() {
                    self.wikidot_bare_after_explicit_list = true;
                }
            } else if is_explicit_list {
                self.wikidot_note_explicit_pre_candidate(!paragraph_break_separated);
            }
            if wikidot_collapsible {
                self.unwrapped_after_block_line =
                    !wikidot_collapsible_ends_with_blockquote;
                if wikidot_collapsible_ends_with_blockquote {
                    self.suppress_next_line_break = true;
                    self.suppressed_line_break_restarts_paragraph = true;
                }
            }
            if invisible_block_line {
                self.current_unwrapped = true;
                self.trim_unwrapped_trailing_line_break = true;
            } else if nested_literal_collapsible {
                self.current.push(Element::LineBreak);
                self.current_unwrapped = true;
                self.suppress_next_line_break = true;
            }
        }
    }

    fn wikidot_note_follows_escaped_prefix(&self, element: &Element<'_>) -> bool {
        if !self.wikidot
            || !matches!(
                self.current.as_slice(),
                [Element::Text(prefix)] if prefix.as_ref() == "\\"
            )
        {
            return false;
        }

        matches!(
            element,
            Element::Container(container)
                if container.ctype() == ContainerType::Div
                    && container
                        .attributes()
                        .get()
                        .get("class")
                        .is_some_and(|class| class.as_ref() == "wiki-note")
        )
    }

    fn wikidot_current_line_is_literal_advanced_table_opener(&self) -> bool {
        if !self.wikidot || self.current.is_empty() {
            return false;
        }

        let mut chunks = Vec::new();
        let mut byte_len = 0usize;
        for element in self.current.iter().rev() {
            match element {
                Element::Text(text) | Element::Raw(text) => {
                    byte_len = byte_len.saturating_add(text.len());
                    if byte_len > 4_096 {
                        return false;
                    }
                    chunks.push(text.as_ref());
                }
                Element::LineBreak => break,
                _ => return false,
            }
        }
        chunks.reverse();
        let line = chunks.concat();
        let line = line.trim();
        let lower = line.to_ascii_lowercase();
        let Some(suffix) = lower
            .strip_prefix("[[cell")
            .or_else(|| lower.strip_prefix("[[hcell"))
        else {
            return false;
        };
        suffix
            .chars()
            .next()
            .is_some_and(|character| character == ']' || character.is_whitespace())
            && line.ends_with("]]")
    }

    fn wikidot_literal_table_opener_before_synthetic_list(
        &self,
        element: &Element<'_>,
    ) -> bool {
        if !self.wikidot || self.current.is_empty() {
            return false;
        }
        let Element::List { items, .. } = element else {
            return false;
        };
        let synthetic_item = items.iter().any(|item| {
            let ListItem::Elements { attributes, .. } = item else {
                return false;
            };
            attributes
                .get()
                .get("style")
                .is_some_and(|value| value.as_ref() == "list-style: none")
        });
        if !synthetic_item {
            return false;
        }
        let line = self
            .current
            .iter()
            .filter_map(|element| match element {
                Element::Text(text) | Element::Raw(text) => Some(text.as_ref()),
                _ => None,
            })
            .collect::<String>();
        line == "[[table]]"
    }

    pub fn push_paragraph_safe_elements(&mut self, mut elements: Vec<Element<'t>>) {
        if self.wikidot
            && elements
                .iter()
                .any(Self::wikidot_element_contains_unaligned_image)
        {
            self.current_unwrapped = true;
        }
        if self.wikidot
            && !self.wikidot_current_has_run_content()
            && elements.iter().any(Self::wikidot_element_has_run_content)
        {
            self.wikidot_absorb_post_list_break_for_prose(
                !self.paragraph_break_since_element,
            );
        }
        if let Some(index) = elements
            .iter()
            .position(|element| *element != Element::LineBreak)
        {
            elements.splice(index..index, self.pending_suppression_seams.drain(..));
        }
        if self.current.is_empty() {
            if let Some(index) = elements
                .iter()
                .position(|element| *element != Element::LineBreak)
            {
                if index != 0 {
                    elements.drain(..index);
                }
                self.current = elements;
            }
        } else {
            self.current.append(&mut elements);
        }
    }

    #[inline]
    pub fn push_errors(&mut self, errors: &mut Vec<ParseError>) {
        self.errors.append(errors);
    }

    pub(crate) fn wikidot_finished_ends_with_explicit_list(&self) -> bool {
        matches!(
            self.finished.last(),
            Some(Element::List {
                wikidot_explicit_block: true,
                ..
            })
        )
    }

    #[inline]
    pub fn pop_line_break(&mut self) {
        if let Some(index) = self.current.iter().rposition(|element| {
            !matches!(element, Element::Delayed(delayed) if delayed.is_suppression_seam())
        }) && self.current[index] == Element::LineBreak
        {
            self.current.remove(index);
        } else if self.wikidot
            && self.current.is_empty()
            && let Some(index) = self.finished.iter().rposition(|element| {
                !matches!(element, Element::Delayed(delayed) if delayed.is_suppression_seam())
            })
            && self.finished[index] == Element::LineBreak
        {
            self.finished.remove(index);
            self.shift_wikidot_div_literal_run_before(index);
        }
    }

    pub(crate) fn ensure_wikidot_trailing_line_break(&mut self) {
        if !self.wikidot {
            return;
        }
        if !self.current.is_empty() {
            if !matches!(self.current.last(), Some(Element::LineBreak)) {
                self.current.push(Element::LineBreak);
            }
            self.trim_unwrapped_trailing_line_break = false;
        } else if matches!(self.finished.last(), Some(Element::Text(_))) {
            self.finished.push(Element::LineBreak);
        }
    }

    pub fn build_paragraph(&mut self) -> Option<Element<'t>> {
        self.trim_trailing_ascii_space();
        if self.current.is_empty() {
            return None;
        }

        let elements = mem::take(&mut self.current);
        let attributes = AttributeMap::new();
        let container = Container::new(ContainerType::Paragraph, elements, attributes);
        let element = Element::Container(container);
        Some(element)
    }

    fn wikidot_finished_is_literalise_bound_container(&self) -> bool {
        let Some(Element::Container(container)) = self.finished.last() else {
            return false;
        };
        if !matches!(
            container.ctype(),
            ContainerType::Bold
                | ContainerType::Italics
                | ContainerType::Underline
                | ContainerType::Superscript
                | ContainerType::Subscript
                | ContainerType::Strikethrough
                | ContainerType::Monospace
        ) {
            return false;
        }
        container.elements().iter().any(|element| {
            matches!(
                element,
                Element::Partial(
                    PartialElement::WikidotBlockOpen(_)
                        | PartialElement::WikidotBlockClose(_)
                )
            )
        })
    }

    fn trim_trailing_ascii_space(&mut self) {
        // Live Wikidot trims the trailing space from the output but still lets
        // it count as content after a scored close: `[[span_]]x[[/span]] `
        // keeps its paragraph wrapper. Leave the space for the root lowering,
        // which decides the wrapper and then ejects it.
        if self.wikidot
            && !self.current_unwrapped
            && self.wikidot_trailing_space_after_scored_alias()
        {
            return;
        }
        while let Some(index) = self.current.iter().rposition(|element| {
            !matches!(element, Element::Delayed(delayed) if delayed.is_suppression_seam())
        }) {
            if !matches!(&self.current[index], Element::Text(text) if text == " ") {
                break;
            }
            self.current.remove(index);
            if !self.wikidot {
                break;
            }
        }
    }

    fn remove_pending_aligned_image_separator(&mut self) {
        if let Some(index) = self.pending_aligned_image_separator.take()
            && matches!(self.finished.get(index), Some(Element::Text(text)) if text == "\n")
        {
            self.finished.remove(index);
            self.shift_wikidot_div_literal_run_before(index);
        }
    }

    pub fn end_paragraph(&mut self) {
        if self.wikidot_bare_after_explicit_list && !self.current.is_empty() {
            self.current_unwrapped = true;
            self.unwrapped_after_block_line = true;
            self.wikidot_bare_after_explicit_list = false;
            self.trim_unwrapped_trailing_line_break = false;
        }
        let finished_before = self.finished.len();
        if self.pending_aligned_image_separator.is_some() && !self.current.is_empty() {
            if self.current_unwrapped {
                self.pending_aligned_image_separator = None;
            } else {
                self.remove_pending_aligned_image_separator();
            }
        }
        if self.current.is_empty() {
            self.finished.append(&mut self.pending_suppression_seams);
        } else {
            let index = self
                .current
                .iter()
                .rposition(|element| *element != Element::LineBreak)
                .map_or(0, |index| index + 1);
            self.current
                .splice(index..index, self.pending_suppression_seams.drain(..));
        }
        let div_literal_flush = self.wikidot_div_literal_run_wrap
            && self.current_unwrapped
            && self.wikidot_literal_div_line
            && !self.current.is_empty();
        if self.wikidot_div_literal_run_wrap
            && !div_literal_flush
            && !self.current.is_empty()
        {
            self.close_wikidot_div_literal_run(false);
        }
        let flush_start = self.finished.len();
        if self.current_unwrapped {
            if self.wikidot_embed_error_residual {
                self.trim_trailing_ascii_space();
            }
            if self.trim_unwrapped_trailing_line_break
                && matches!(self.current.last(), Some(Element::LineBreak))
            {
                self.current.pop();
            }
            self.finished.append(&mut self.current);
            if div_literal_flush {
                let flush_end = self.finished.len();
                self.register_wikidot_div_literal_range(flush_start, flush_end);
            }
            self.current_unwrapped = false;
        } else if let Some(paragraph) = self.build_paragraph() {
            self.finished.push(paragraph);
        }
        self.current_has_discarded_control = false;
        self.wikidot_complete_raw_line_occupied = false;
        self.wikidot_complete_raw_run_after_block = false;
        self.wikidot_demoted_comment_break = false;
        self.pending_unwrapped_separator = false;
        self.unwrapped_after_block_line = false;
        self.wikidot_footnote_joined_math_break = false;
        self.wikidot_literal_iftags_line = false;
        self.wikidot_literal_div_line = false;
        self.wikidot_embed_error_residual = false;
        self.trim_unwrapped_trailing_line_break = false;
        self.suppress_next_line_break = false;
        self.suppressed_line_break_restarts_paragraph = false;
        if self.finished.len() != finished_before {
            self.wikidot_reopen_for_footnote = false;
        }
    }

    #[inline]
    pub(crate) fn defer_suppression_seam(&mut self, element: Element<'t>) {
        debug_assert!(
            matches!(&element, Element::Delayed(delayed) if delayed.is_suppression_seam())
        );
        self.pending_suppression_seams.push(element);
    }

    pub fn end_paragraph_at_break(&mut self) {
        let unwrapped = self.current_unwrapped && !self.current.is_empty();
        let literal_div_line = self.wikidot_literal_div_line;
        let unwrapped_after_block_line = self.unwrapped_after_block_line;
        let footnote_joined_math_break = self.wikidot_footnote_joined_math_break;
        let complete_raw_run_after_block = self.wikidot_complete_raw_run_after_block;
        let last_is_aligned_image = match self.finished.last() {
            Some(Element::Image {
                alignment: Some(_), ..
            }) => true,
            Some(Element::Delayed(delayed)) => delayed
                .image_alignment()
                .is_some_and(|alignment| alignment.is_some()),
            _ => false,
        };
        let aligned_image_boundary = self.wikidot
            && self.current.is_empty()
            && self.pending_aligned_image_separator.is_none()
            && last_is_aligned_image;
        self.end_paragraph();
        self.wikidot_reopen_for_footnote = self.wikidot
            && matches!(
                self.finished.last(),
                Some(Element::Container(container))
                    if container.ctype() == ContainerType::Paragraph
            );
        if unwrapped && !complete_raw_run_after_block && !footnote_joined_math_break {
            self.finished
                .push(if literal_div_line || unwrapped_after_block_line {
                    text!("\n")
                } else {
                    text!(" ")
                });
            self.pending_unwrapped_separator = true;
        } else if aligned_image_boundary {
            self.finished.push(text!("\n"));
            self.pending_aligned_image_separator = Some(self.finished.len() - 1);
        }
        self.paragraph_break_since_element = true;
    }

    pub fn into_result<'r>(mut self) -> ParseResult<'r, 't, Vec<Element<'t>>> {
        if self.wikidot_explicit_pre_candidate {
            self.wikidot_dissolve_explicit_pre_candidate();
        }
        self.end_paragraph();
        self.remove_pending_aligned_image_separator();
        self.close_wikidot_div_literal_run(false);

        let elements = self.finished;
        let errors = self.errors;

        let paragraph_safe = elements.iter().all(|element| element.paragraph_safe());

        ok!(paragraph_safe; elements, errors)
    }

    pub fn into_elements(mut self) -> Vec<Element<'t>> {
        if self.wikidot_explicit_pre_candidate {
            self.wikidot_dissolve_explicit_pre_candidate();
        }
        self.end_paragraph();
        self.remove_pending_aligned_image_separator();
        self.close_wikidot_div_literal_run(false);

        debug_assert!(self.errors.is_empty(), "ParagraphStack errors");

        self.finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paragraph<'t>(elements: Vec<Element<'t>>) -> Element<'t> {
        Element::Container(Container::new(
            ContainerType::Paragraph,
            elements,
            AttributeMap::new(),
        ))
    }

    #[test]
    fn pop_line_break_removes_trailing_break_before_finishing() {
        let mut stack = ParagraphStack::new();

        stack.push_element(text!("alpha"), true);
        stack.push_element(Element::LineBreak, true);
        stack.pop_line_break();

        assert_eq!(stack.into_elements(), vec![paragraph(vec![text!("alpha")])]);
    }

    #[test]
    fn non_paragraph_safe_elements_finish_pending_paragraph() {
        let mut stack = ParagraphStack::new();

        stack.push_element(text!("alpha"), true);
        stack.push_element(Element::HorizontalRule, false);

        assert_eq!(
            stack.into_elements(),
            vec![paragraph(vec![text!("alpha")]), Element::HorizontalRule],
        );
    }

    #[test]
    fn an_empty_unsafe_construct_can_leave_the_current_paragraph_unwrapped() {
        let mut stack = ParagraphStack::new();
        stack.push_element(text!("alpha"), true);
        stack.push_element(Element::LineBreak, true);
        stack.mark_current_unwrapped();

        assert_eq!(
            stack.into_elements(),
            vec![text!("alpha"), Element::LineBreak],
        );
    }

    #[test]
    fn an_empty_unsafe_construct_does_not_unwrap_future_content() {
        let mut stack = ParagraphStack::new();
        stack.mark_current_unwrapped();
        stack.push_element(text!("alpha"), true);

        assert_eq!(stack.into_elements(), vec![paragraph(vec![text!("alpha")])]);
    }

    #[test]
    fn paragraph_safe_elements_adopt_vectors_and_skip_empty_leading_breaks() {
        let mut stack = ParagraphStack::new();

        stack.push_paragraph_safe_elements(vec![
            Element::LineBreak,
            text!("alpha"),
            Element::LineBreak,
        ]);

        assert_eq!(
            stack.into_elements(),
            vec![paragraph(vec![text!("alpha"), Element::LineBreak])],
        );
    }

    #[test]
    fn paragraph_safe_elements_ignore_all_leading_breaks() {
        let mut stack = ParagraphStack::new();

        stack.push_paragraph_safe_elements(vec![Element::LineBreak, Element::LineBreak]);

        assert_eq!(stack.into_elements(), Vec::<Element>::new());
    }

    #[test]
    fn paragraph_safe_elements_append_to_existing_paragraph() {
        let mut stack = ParagraphStack::new();

        stack.push_element(text!("alpha"), true);
        stack.push_paragraph_safe_elements(vec![Element::LineBreak, text!("beta")]);

        assert_eq!(
            stack.into_elements(),
            vec![paragraph(vec![
                text!("alpha"),
                Element::LineBreak,
                text!("beta"),
            ])],
        );
    }

    #[test]
    fn empty_inline_construct_space_is_trimmed_before_break_or_paragraph_end() {
        let mut before_break = ParagraphStack::new();
        before_break.push_element(text!("alpha"), true);
        before_break.push_element(text!(" "), true);
        before_break.push_element(Element::LineBreak, true);
        before_break.push_element(text!("beta"), true);
        assert_eq!(
            before_break.into_elements(),
            vec![paragraph(vec![
                text!("alpha"),
                Element::LineBreak,
                text!("beta"),
            ])],
        );

        let mut at_end = ParagraphStack::new();
        at_end.push_element(text!("alpha"), true);
        at_end.push_element(text!(" "), true);
        assert_eq!(
            at_end.into_elements(),
            vec![paragraph(vec![text!("alpha")])],
        );
    }

    #[test]
    fn discarded_control_trims_surrounding_spaces_but_preserves_a_line_break() {
        let mut stack = ParagraphStack::new_wikidot();
        stack.mark_discarded_control();
        stack.push_element(text!(" "), true);
        stack.push_element(Element::LineBreak, true);
        stack.push_element(text!("omega"), true);

        assert_eq!(
            stack.into_elements(),
            vec![paragraph(vec![Element::LineBreak, text!("omega")])],
        );

        let mut between_spaces = ParagraphStack::new_wikidot();
        between_spaces.push_element(text!("alpha"), true);
        between_spaces.push_element(text!(" "), true);
        between_spaces.mark_discarded_control();
        between_spaces.push_element(text!(" "), true);
        between_spaces.push_element(text!("omega"), true);

        assert_eq!(
            between_spaces.into_elements(),
            vec![paragraph(vec![text!("alpha"), text!(" "), text!("omega")])],
        );
    }

    #[test]
    fn paragraph_break_after_unwrapped_image_preserves_wikidot_separator() {
        let mut stack = ParagraphStack::new();
        stack.push_element(text!("alpha"), true);
        stack.current_unwrapped = true;
        stack.end_paragraph_at_break();
        stack.push_element(text!("beta"), true);

        assert_eq!(
            stack.into_elements(),
            vec![text!("alpha"), text!(" "), paragraph(vec![text!("beta")])],
        );
    }

    #[test]
    fn wikidot_definition_list_unwraps_adjacent_prose_with_a_break() {
        let mut stack = ParagraphStack::new_wikidot();
        stack.push_element(text!("alpha"), true);
        stack.push_element(Element::DefinitionList(Vec::new()), false);

        assert_eq!(
            stack.into_elements(),
            vec![
                text!("alpha"),
                Element::LineBreak,
                Element::DefinitionList(Vec::new()),
            ],
        );
    }

    #[test]
    fn footnote_after_seam_flushed_past_a_break_does_not_reopen_the_paragraph() {
        let mut stack = ParagraphStack::new_wikidot();

        stack.push_element(text!("alpha"), true);
        stack.end_paragraph_at_break();
        stack.defer_suppression_seam(Element::Delayed(
            crate::delayed::DelayedElement::typography_boundary(),
        ));
        stack.end_paragraph();
        stack.push_element(Element::Footnote(0), true);

        let elements = stack.into_elements();
        assert!(elements.contains(&paragraph(vec![text!("alpha")])));
        assert!(elements.contains(&paragraph(vec![Element::Footnote(0)])));
    }
}
