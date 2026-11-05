/*
 * parsing/body_end.rs
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

use super::{Parser, Token};

#[derive(Debug, Copy, Clone)]
pub(super) struct BodyEndBoundary {
    accepts_names: &'static [&'static str],
    hard: bool,
    confined: bool,
}

impl<'r, 't> Parser<'r, 't> {
    pub(crate) fn push_body_end_boundary(
        &mut self,
        accepts_names: &'static [&'static str],
        hard: bool,
    ) {
        let boundary = BodyEndBoundary {
            accepts_names,
            hard,
            confined: self.in_wikidot_single_line_content(),
        };
        self.block_end_scan_cache
            .borrow_mut()
            .body_end_boundaries
            .push(boundary);
    }

    pub(crate) fn pop_body_end_boundary(&mut self) {
        self.block_end_scan_cache
            .borrow_mut()
            .body_end_boundaries
            .pop();
    }

    fn body_end_boundaries(&self) -> Vec<BodyEndBoundary> {
        self.block_end_scan_cache
            .borrow()
            .body_end_boundaries
            .clone()
    }

    pub(crate) fn body_end_boundary_depth(&self) -> usize {
        self.block_end_scan_cache.borrow().body_end_boundaries.len()
    }

    pub(crate) fn body_end_boundaries_is_empty(&self) -> bool {
        self.block_end_scan_cache
            .borrow()
            .body_end_boundaries
            .is_empty()
    }

    pub(crate) fn wikidot_li_inside_item_body(&self) -> bool {
        for boundary in self.body_end_boundaries().iter().rev() {
            if boundary.confined {
                continue;
            }
            let accepts = |want: &str| {
                boundary
                    .accepts_names
                    .iter()
                    .any(|accepted| accepted.eq_ignore_ascii_case(want))
            };
            if accepts("li") {
                return true;
            }
            if accepts("ul") || accepts("ol") {
                return false;
            }
        }
        false
    }

    pub(crate) fn body_end_boundary_accepts(&self, name: &str) -> bool {
        self.body_end_boundaries().iter().any(|boundary| {
            boundary
                .accepts_names
                .iter()
                .any(|accepted| accepted.eq_ignore_ascii_case(name))
        })
    }

    /// L85: whether the innermost body boundary accepts div/collapsible.
    /// Confines implicit-list parsing to genuine div/collapsible bodies.
    pub(crate) fn wikidot_top_boundary_is_div_or_collapsible(&self) -> bool {
        self.body_end_boundaries().last().is_some_and(|boundary| {
            !boundary.confined
                && boundary.accepts_names.iter().any(|accepted| {
                    accepted.eq_ignore_ascii_case("div")
                        || accepted.eq_ignore_ascii_case("collapsible")
                })
        })
    }

    /// L85: whether a non-top (ancestor) body boundary accepts `name`.
    /// Used to confine explicit-list cross stops to genuinely outer opens.
    pub(crate) fn wikidot_ancestor_boundary_accepts(&self, name: &str) -> bool {
        let boundaries = self.body_end_boundaries();
        let Some((_, ancestors)) = boundaries.split_last() else {
            return false;
        };
        ancestors.iter().any(|boundary| {
            !boundary.confined
                && boundary
                    .accepts_names
                    .iter()
                    .any(|accepted| accepted.eq_ignore_ascii_case(name))
        })
    }

    pub(crate) fn at_body_end_boundary(&self) -> bool
    where
        'r: 't,
    {
        self.at_body_end_boundary_in(&self.body_end_boundaries(), false)
    }

    pub(crate) fn at_hard_body_end_boundary(&self) -> bool
    where
        'r: 't,
    {
        self.at_body_end_boundary_in(&self.body_end_boundaries(), true)
    }

    pub(crate) fn at_ancestor_body_end_boundary(&self) -> bool
    where
        'r: 't,
    {
        let boundaries = self.body_end_boundaries();
        let Some((_, ancestors)) = boundaries.split_last() else {
            return false;
        };

        self.at_body_end_boundary_in(ancestors, true)
    }

    fn at_body_end_boundary_in(
        &self,
        boundaries: &[BodyEndBoundary],
        hard_only: bool,
    ) -> bool
    where
        'r: 't,
    {
        let considered: Vec<&BodyEndBoundary> = boundaries
            .iter()
            .filter(|boundary| !boundary.confined && (!hard_only || boundary.hard))
            .collect();
        if considered.is_empty() {
            return false;
        }

        if self.current().token != Token::LeftBlockEnd {
            return false;
        }

        let source = self.full_text().inner();
        let start = self.current().span.start;
        let ahead = &source[start..];
        let Some(close_found) = self.cached_close_marker(source, start) else {
            return false;
        };
        let close_offset = close_found - start;
        if ahead[..close_offset]
            .chars()
            .last()
            .is_some_and(char::is_whitespace)
        {
            return false;
        }

        let mut probe = self.clone();
        if let Ok(name) = probe.get_end_block()
            && considered.iter().any(|boundary| boundary.accepts(&name))
        {
            return true;
        }

        if self.settings().layout.legacy() {
            for boundary in considered {
                for name in boundary.accepts_names {
                    let fused = format!("[[/{name}]]]");
                    if ahead
                        .get(..fused.len())
                        .is_some_and(|head| head.eq_ignore_ascii_case(&fused))
                    {
                        return true;
                    }
                }
            }
        }

        false
    }
}

impl BodyEndBoundary {
    fn accepts(&self, name: &str) -> bool {
        self.accepts_names
            .iter()
            .any(|item| name.eq_ignore_ascii_case(item))
    }
}
