/*
 * tree/content.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2026 Wikijump Team
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

use super::{Element, SyntaxTree};

/// One ordered item in a content-section stream.
///
/// A section is emitted before the first boundary and after the last boundary,
/// so leading, trailing, and adjacent separators retain their empty sections.
/// Callers can therefore consume the parser's typed boundaries without
/// rescanning source text or inferring structure from rendered HTML.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentSegment<'e, 't> {
    Section(&'e [Element<'t>]),
    Boundary,
}

#[derive(Debug, Clone)]
pub struct ContentSegments<'e, 't> {
    elements: &'e [Element<'t>],
    cursor: usize,
    boundary_pending: bool,
    finished: bool,
}

impl<'e, 't> ContentSegments<'e, 't> {
    #[inline]
    fn new(elements: &'e [Element<'t>]) -> Self {
        Self {
            elements,
            cursor: 0,
            boundary_pending: false,
            finished: false,
        }
    }
}

impl<'e, 't> Iterator for ContentSegments<'e, 't> {
    type Item = ContentSegment<'e, 't>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.boundary_pending {
            debug_assert!(self.elements[self.cursor].is_content_separator());
            self.cursor += 1;
            self.boundary_pending = false;
            return Some(ContentSegment::Boundary);
        }
        if self.finished {
            return None;
        }

        let start = self.cursor;
        while self
            .elements
            .get(self.cursor)
            .is_some_and(|element| !element.is_content_separator())
        {
            self.cursor += 1;
        }
        if self.cursor == self.elements.len() {
            self.finished = true;
        } else {
            self.boundary_pending = true;
        }
        Some(ContentSegment::Section(&self.elements[start..self.cursor]))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.elements.len().saturating_sub(self.cursor);
        (
            usize::from(!self.finished || self.boundary_pending),
            Some(remaining.saturating_mul(2).saturating_add(1)),
        )
    }
}

#[inline]
pub fn content_segments<'e, 't>(elements: &'e [Element<'t>]) -> ContentSegments<'e, 't> {
    ContentSegments::new(elements)
}

impl<'t> SyntaxTree<'t> {
    #[inline]
    pub fn content_segments(&self) -> ContentSegments<'_, 't> {
        content_segments(&self.elements)
    }
}
