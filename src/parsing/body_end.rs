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
use std::hash::{DefaultHasher, Hash, Hasher};

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
        let crosses_later_list_line = self.settings().layout.legacy()
            && !self.in_wikidot_anchor_body()
            && accepts_names.iter().any(|name| {
                name.eq_ignore_ascii_case("footnote")
                    || name.eq_ignore_ascii_case("li")
                    || name.eq_ignore_ascii_case("ul")
                    || name.eq_ignore_ascii_case("ol")
            });
        let boundary = BodyEndBoundary {
            accepts_names,
            hard,
            confined: self.in_wikidot_single_line_content() && !crosses_later_list_line,
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

    /// Identifies what can end a body walk here: the innermost boundary, plus
    /// the set of its ancestors. The ancestors' order and repeats do not change
    /// any boundary check, so walks at different nesting depths share it.
    pub(crate) fn body_end_boundary_fingerprint(&self) -> u64 {
        let cache = self.block_end_scan_cache.borrow();
        let identity = |boundary: &BodyEndBoundary| {
            (
                boundary.accepts_names.as_ptr() as usize,
                boundary.accepts_names.len(),
                boundary.hard,
                boundary.confined,
            )
        };
        let Some((innermost, ancestors)) = cache.body_end_boundaries.split_last() else {
            return 0;
        };
        let mut ancestors: Vec<_> = ancestors.iter().map(identity).collect();
        ancestors.sort_unstable();
        ancestors.dedup();
        let mut hasher = DefaultHasher::new();
        identity(innermost).hash(&mut hasher);
        ancestors.hash(&mut hasher);
        hasher.finish()
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
        self.at_body_end_boundary_in(false, false)
    }

    pub(crate) fn at_hard_body_end_boundary(&self) -> bool
    where
        'r: 't,
    {
        self.at_body_end_boundary_in(false, true)
    }

    pub(crate) fn at_ancestor_body_end_boundary(&self) -> bool
    where
        'r: 't,
    {
        self.at_body_end_boundary_in(true, true)
    }

    fn at_body_end_boundary_in(&self, ancestors_only: bool, hard_only: bool) -> bool
    where
        'r: 't,
    {
        if self.current().token != Token::LeftBlockEnd {
            return false;
        }
        if !self.qualifying_body_end_boundary_exists(ancestors_only, hard_only) {
            return false;
        }

        let start = self.current().span.start;
        let considered: Vec<BodyEndBoundary> = {
            let cache = self.block_end_scan_cache.borrow();
            considered_boundaries(&cache.body_end_boundaries, ancestors_only)
                .iter()
                .copied()
                .filter(|boundary| boundary_qualifies(boundary, hard_only))
                .collect()
        };
        {
            let source = self.full_text().inner();
            let Some(close_found) = self.cached_close_marker(source, start) else {
                return false;
            };
            if source[start..close_found]
                .chars()
                .last()
                .is_some_and(char::is_whitespace)
            {
                return false;
            }
        }

        let mut probe = self.clone();
        let parsed_name = probe.get_end_block().ok();
        if considered.iter().any(|boundary| {
            parsed_name
                .as_deref()
                .is_some_and(|name| boundary.accepts(name))
        }) {
            if let Some(name) = parsed_name.as_deref() {
                let bare = name.strip_suffix('_').unwrap_or(name);
                if (bare.eq_ignore_ascii_case("row") || bare.eq_ignore_ascii_case("table"))
                    && self.wikidot_list_line_table_closer_is_item_text(start)
                {
                    return false;
                }
            }
            return true;
        }
        if !self.settings().layout.legacy() {
            return false;
        }

        let ahead = self.full_text().inner().get(start..).unwrap_or("");
        considered.iter().any(|boundary| {
            boundary.accepts_names.iter().any(|name| {
                let fused = format!("[[/{name}]]]");
                ahead
                    .get(..fused.len())
                    .is_some_and(|head| head.eq_ignore_ascii_case(&fused))
            })
        })
    }

    fn qualifying_body_end_boundary_exists(
        &self,
        ancestors_only: bool,
        hard_only: bool,
    ) -> bool {
        let cache = self.block_end_scan_cache.borrow();
        considered_boundaries(&cache.body_end_boundaries, ancestors_only)
            .iter()
            .any(|boundary| boundary_qualifies(boundary, hard_only))
    }
}

fn considered_boundaries(
    boundaries: &[BodyEndBoundary],
    ancestors_only: bool,
) -> &[BodyEndBoundary] {
    if !ancestors_only {
        return boundaries;
    }
    boundaries
        .split_last()
        .map(|(_, ancestors)| ancestors)
        .unwrap_or(&[])
}

fn boundary_qualifies(boundary: &BodyEndBoundary, hard_only: bool) -> bool {
    !boundary.confined && (!hard_only || boundary.hard)
}

impl BodyEndBoundary {
    fn accepts(&self, name: &str) -> bool {
        self.accepts_names
            .iter()
            .any(|item| name.eq_ignore_ascii_case(item))
    }
}

impl<'r, 't> Parser<'r, 't> {
    /// Whether the current token is a bare `[[/div]]` closer that the
    /// row-close absorb flag will silently consume: exact `div` name,
    /// exact non-fused `]]` terminator.
    pub(crate) fn at_wikidot_row_absorb_div_close(&self) -> bool
    where
        'r: 't,
    {
        if self.current().token != Token::LeftBlockEnd
            || !self.wikidot_row_absorb_div_close()
        {
            return false;
        }
        let mut probe = self.clone();
        if probe.step().is_err() {
            return false;
        }
        let mut name = String::new();
        loop {
            match probe.current().token {
                Token::RightBlock => break,
                Token::InputEnd | Token::ParagraphBreak | Token::RightLink => {
                    return false;
                }
                _ => {
                    name.push_str(probe.current().slice);
                    if probe.step().is_err() {
                        return false;
                    }
                }
            }
        }
        name.eq_ignore_ascii_case("div")
    }

    /// Whether the break at the current token precedes an absorbed div
    /// closer that is itself followed by an enclosing-body closer line:
    /// consecutive closer lines share no break in live, so this break
    /// vanishes.
    pub(crate) fn wikidot_row_absorb_break_before_closer_line(&self) -> bool
    where
        'r: 't,
    {
        if self.current().token != Token::LineBreak {
            return false;
        }
        let mut probe = self.clone();
        if probe.step().is_err() {
            return false;
        }
        if !probe.at_wikidot_row_absorb_div_close() {
            return false;
        }
        // Step past the absorbed closer.
        probe.step().ok();
        loop {
            match probe.current().token {
                Token::RightBlock => {
                    probe.step().ok();
                    break;
                }
                Token::InputEnd | Token::ParagraphBreak | Token::RightLink => {
                    return false;
                }
                _ => {
                    if probe.step().is_err() {
                        return false;
                    }
                }
            }
        }
        // Step past the absorbed line's break, if any.
        while probe.current().token == Token::LineBreak {
            if probe.step().is_err() {
                return false;
            }
        }
        if probe.current().token != Token::LeftBlockEnd {
            return false;
        }
        let mut end = probe.clone();
        let Ok(name) = end.get_end_block() else {
            return false;
        };
        self.body_end_boundary_accepts(name.strip_suffix('_').unwrap_or(name.as_ref()))
    }
}
