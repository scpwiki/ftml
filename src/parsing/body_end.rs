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

/// What an absorb-armed stray line looks like after a leading break run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WikidotRowAbsorbStrayRun {
    /// The stray line carries trailing inline content (`[[/div]] y`): live
    /// eats the break run and the trailing text joins the previous line.
    TrailingContent,
    /// The stray line is bare (only the closer, then the break): live
    /// renders the break run as one `<br>`, following content unwrapped.
    BareLine,
}

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
        let mix = |boundary: &BodyEndBoundary| -> u64 {
            let mut value = (boundary.accepts_names.as_ptr() as usize as u64)
                .wrapping_mul(0x9E37_79B9_7F4A_7C15);
            value ^= (boundary.accepts_names.len() as u64).rotate_left(23);
            value ^= u64::from(boundary.hard) << 41;
            value ^= u64::from(boundary.confined) << 42;
            value ^= value >> 29;
            value
        };
        let Some((innermost, ancestors)) = cache.body_end_boundaries.split_last() else {
            return 0;
        };
        let mut sum = 0u64;
        let mut xor = 0u64;
        for boundary in ancestors {
            let mixed = mix(boundary);
            sum = sum.wrapping_add(mixed);
            xor ^= mixed;
        }
        let mut combined = mix(innermost)
            .wrapping_add(sum.rotate_left(7))
            ^ xor.rotate_left(31)
            ^ (ancestors.len() as u64).wrapping_mul(0xD1B5_4A32_D192_ED03);
        combined ^= combined >> 32;
        combined
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
        let cache = self.block_end_scan_cache.borrow();
        for boundary in cache.body_end_boundaries.iter().rev() {
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

    /// Whether the innermost (topmost) non-confined body boundary is a list
    /// item body (directly in `[[li]]`, not under a nested body such as a
    /// footnote that itself sits in an item). d4-46 (a): the li-span break
    /// doubling is direct-item only; the footnote pin
    /// `collapsible/in-list-content-paragraph-heading-footnote` shows a
    /// heading under a footnote in an item stays un-doubled.
    pub(crate) fn wikidot_top_boundary_is_li(&self) -> bool {
        let cache = self.block_end_scan_cache.borrow();
        for boundary in cache.body_end_boundaries.iter().rev() {
            if boundary.confined {
                continue;
            }
            return boundary
                .accepts_names
                .iter()
                .any(|accepted| accepted.eq_ignore_ascii_case("li"));
        }
        false
    }

    pub(crate) fn body_end_boundary_accepts(&self, name: &str) -> bool {
        self.block_end_scan_cache
            .borrow()
            .body_end_boundaries
            .iter()
            .any(|boundary| {
                boundary
                    .accepts_names
                    .iter()
                    .any(|accepted| accepted.eq_ignore_ascii_case(name))
            })
    }

    /// L85: whether the innermost body boundary accepts div/collapsible.
    /// Confines implicit-list parsing to genuine div/collapsible bodies.
    pub(crate) fn wikidot_top_boundary_is_div_or_collapsible(&self) -> bool {
        self.block_end_scan_cache
            .borrow()
            .body_end_boundaries
            .last()
            .is_some_and(|boundary| {
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
        let cache = self.block_end_scan_cache.borrow();
        let Some((_, ancestors)) = cache.body_end_boundaries.split_last() else {
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

    pub(crate) fn wikidot_table_wrapped_by_flat_block(&self) -> bool {
        let cache = self.block_end_scan_cache.borrow();
        let Some((_, ancestors)) = cache.body_end_boundaries.split_last() else {
            return false;
        };
        let accepts = |boundary: &BodyEndBoundary, name: &str| {
            !boundary.confined
                && boundary
                    .accepts_names
                    .iter()
                    .any(|accepted| accepted.eq_ignore_ascii_case(name))
        };
        let Some(table_idx) = ancestors
            .iter()
            .rposition(|boundary| accepts(boundary, "table"))
        else {
            return false;
        };
        ancestors[..table_idx]
            .iter()
            .any(|boundary| accepts(boundary, "div") || accepts(boundary, "collapsible"))
    }

    /// Whether the end block at the current position names a table cell
    /// whose boundary sits on an ancestor body, so a collected inner body
    /// (an explicit list item inside the cell) must yield to the cell.
    pub(crate) fn wikidot_ancestor_boundary_accepts_table_cell_end(&self) -> bool
    where
        'r: 't,
    {
        if self.current().token != Token::LeftBlockEnd {
            return false;
        }
        let mut probe = self.clone();
        let Ok(name) = probe.get_end_block() else {
            return false;
        };
        let bare = name.strip_suffix('_').unwrap_or(&name);
        if !bare.eq_ignore_ascii_case("cell") && !bare.eq_ignore_ascii_case("hcell") {
            return false;
        }
        let cache = self.block_end_scan_cache.borrow();
        let Some((_, ancestors)) = cache.body_end_boundaries.split_last() else {
            return false;
        };
        for boundary in ancestors.iter().rev() {
            if boundary
                .accepts_names
                .iter()
                .any(|accepted| accepted.eq_ignore_ascii_case("footnote"))
            {
                return false;
            }
            if !boundary.confined
                && boundary
                    .accepts_names
                    .iter()
                    .any(|accepted| accepted.eq_ignore_ascii_case(bare))
            {
                return true;
            }
        }
        false
    }

    pub(crate) fn wikidot_stack_accepts_table_cell_end(&self) -> bool
    where
        'r: 't,
    {
        if self.current().token != Token::LeftBlockEnd {
            return false;
        }
        let mut probe = self.clone();
        let Ok(name) = probe.get_end_block() else {
            return false;
        };
        let bare = name.strip_suffix('_').unwrap_or(&name);
        if !bare.eq_ignore_ascii_case("cell") && !bare.eq_ignore_ascii_case("hcell") {
            return false;
        }
        let cache = self.block_end_scan_cache.borrow();
        for boundary in cache.body_end_boundaries.iter().rev() {
            if boundary
                .accepts_names
                .iter()
                .any(|accepted| accepted.eq_ignore_ascii_case("footnote"))
            {
                return false;
            }
            if !boundary.confined
                && boundary
                    .accepts_names
                    .iter()
                    .any(|accepted| accepted.eq_ignore_ascii_case(bare))
            {
                return true;
            }
        }
        false
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
        let accepted = {
            let cache = self.block_end_scan_cache.borrow();
            footnote_shadowed_boundaries(considered_boundaries(
                &cache.body_end_boundaries,
                ancestors_only,
            ))
            .iter()
            .filter(|boundary| boundary_qualifies(boundary, hard_only))
            .any(|boundary| {
                parsed_name
                    .as_deref()
                    .is_some_and(|name| boundary.accepts(name))
            })
        };
        if accepted {
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
        let cache = self.block_end_scan_cache.borrow();
        footnote_shadowed_boundaries(considered_boundaries(
            &cache.body_end_boundaries,
            ancestors_only,
        ))
        .iter()
        .filter(|boundary| boundary_qualifies(boundary, hard_only))
        .any(|boundary| {
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

fn footnote_shadowed_boundaries(boundaries: &[BodyEndBoundary]) -> &[BodyEndBoundary] {
    for index in (0..boundaries.len()).rev() {
        if boundaries[index].accepts("footnote") {
            return &boundaries[index..];
        }
    }
    boundaries
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

    /// Whether the break run at the current token leads to the next
    /// absorbable bare `[[/div]]` closer, and which shape the stray line
    /// takes. The caller must be positioned on a break token.
    pub(crate) fn wikidot_row_absorb_stray_break_run(
        &self,
    ) -> Option<WikidotRowAbsorbStrayRun>
    where
        'r: 't,
    {
        if !matches!(
            self.current().token,
            Token::LineBreak | Token::ParagraphBreak
        ) {
            return None;
        }
        if !self.wikidot_row_absorb_div_close() {
            return None;
        }
        let mut probe = self.clone();
        while matches!(
            probe.current().token,
            Token::LineBreak | Token::ParagraphBreak
        ) {
            if probe.step().is_err() {
                return None;
            }
        }
        if !probe.at_wikidot_row_absorb_div_close() {
            return None;
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
                    return None;
                }
                _ => {
                    if probe.step().is_err() {
                        return None;
                    }
                }
            }
        }
        while probe.current().token == Token::Whitespace {
            if probe.step().is_err() {
                return None;
            }
        }
        // Nothing follows the stray line at end of input: the absorb
        // neighborhood produces no visible break at all there.
        if probe.current().token == Token::InputEnd {
            return None;
        }
        Some(
            if matches!(
                probe.current().token,
                Token::LineBreak | Token::ParagraphBreak
            ) {
                WikidotRowAbsorbStrayRun::BareLine
            } else {
                WikidotRowAbsorbStrayRun::TrailingContent
            },
        )
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
