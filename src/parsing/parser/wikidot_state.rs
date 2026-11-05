use super::Parser;
use crate::parsing::condition::SplitDelimiter;
use crate::parsing::{ExtractedToken, Token};
use std::collections::BTreeMap;

const COLLAPSIBLE: u16 = 1 << 0;
const QUOTE_BOUNDARY_CLOSES_BODY: u16 = 1 << 1;
const PENDING_COLLAPSIBLE_CLOSER: u16 = 1 << 2;
const COLLAPSIBLE_CLOSED_AT_DEEPER_QUOTE: u16 = 1 << 3;
const SIMPLE_TABLE_CELL: u16 = 1 << 4;
const MIXED_PARAGRAPH_OWNERSHIP: u16 = 1 << 5;
const COLLAPSIBLE_CONTENT_CLOSED: u16 = 1 << 6;
const IFTAGS_LITERAL_SPAN: u16 = 1 << 7;
const DEFINITION_LIST_LINE: u16 = 1 << 8;
const SINGLE_LINE_CONTENT: u16 = 1 << 9;
const DIV_LITERAL_RECOVERED: u16 = 1 << 10;

/// Sentinel for "no scored div direct body" in the packed field.
const SCORED_DIV_DIRECT_INACTIVE: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct WikidotState {
    div_body_depth: u16,
    scored_div_body_depth: u16,
    scored_div_direct: (u32, u16),
    scored_span_body_depth: u16,
    span_body_depth: u16,
    anchor_body_depth: u16,
    bibliography_body_depth: u16,
    literal_triple_link_depth: u16,
    simple_table_crossed_closers: u16,
    center_body_depth: u16,
    flags: u16,
}

impl Default for WikidotState {
    fn default() -> Self {
        Self {
            div_body_depth: 0,
            scored_div_body_depth: 0,
            scored_div_direct: (SCORED_DIV_DIRECT_INACTIVE, 0),
            scored_span_body_depth: 0,
            span_body_depth: 0,
            anchor_body_depth: 0,
            bibliography_body_depth: 0,
            literal_triple_link_depth: 0,
            simple_table_crossed_closers: 0,
            center_body_depth: 0,
            flags: 0,
        }
    }
}

impl WikidotState {
    pub(super) fn attempt_key(self) -> Self {
        Self {
            div_body_depth: self.div_body_depth.min(1),
            ..self
        }
    }
}

impl Parser<'_, '_> {
    #[inline]
    pub(crate) fn in_wikidot_bibliography_body(&self) -> bool {
        self.wikidot.bibliography_body_depth > 0
    }

    #[inline]
    pub(crate) fn enter_wikidot_bibliography_body(&mut self) {
        self.wikidot.bibliography_body_depth =
            self.wikidot.bibliography_body_depth.saturating_add(1);
    }

    #[inline]
    pub(crate) fn leave_wikidot_bibliography_body(&mut self) {
        self.wikidot.bibliography_body_depth =
            self.wikidot.bibliography_body_depth.saturating_sub(1);
    }

    #[inline]
    pub(crate) fn in_wikidot_div_body(&self) -> bool {
        self.wikidot.div_body_depth > 0
    }

    #[inline]
    pub(crate) fn wikidot_div_opener_is_excess(&self, opener_start: usize) -> bool {
        self.block_end_scan_cache
            .borrow()
            .wikidot_div_stream_marks
            .get(&opener_start)
            .is_some_and(|&(balance_before, ends_after)| ends_after <= balance_before)
    }

    #[inline]
    pub(crate) fn enter_wikidot_div_body(&mut self) {
        self.wikidot.div_body_depth = self.wikidot.div_body_depth.saturating_add(1);
    }

    #[inline]
    pub(crate) fn leave_wikidot_div_body(&mut self) {
        self.wikidot.div_body_depth = self.wikidot.div_body_depth.saturating_sub(1);
    }

    #[inline]
    pub(crate) fn enter_wikidot_scored_div_body(&mut self) {
        self.wikidot.scored_div_body_depth =
            self.wikidot.scored_div_body_depth.saturating_add(1);
    }

    #[inline]
    pub(crate) fn leave_wikidot_scored_div_body(&mut self) {
        self.wikidot.scored_div_body_depth =
            self.wikidot.scored_div_body_depth.saturating_sub(1);
    }

    #[inline]
    pub(crate) fn enter_wikidot_scored_div_direct_body(
        &mut self,
        body_start: usize,
        boundary_depth: usize,
    ) -> (u32, u16) {
        let saved = self.wikidot.scored_div_direct;
        self.wikidot.scored_div_direct = (
            u32::try_from(body_start).unwrap_or(SCORED_DIV_DIRECT_INACTIVE - 1),
            boundary_depth.min(u16::MAX as usize) as u16,
        );
        saved
    }

    #[inline]
    pub(crate) fn leave_wikidot_scored_div_direct_body(&mut self, saved: (u32, u16)) {
        self.wikidot.scored_div_direct = saved;
    }

    pub(crate) fn wikidot_scored_div_first_line_suppresses_at(&self, pos: usize) -> bool {
        if !self.settings().layout.legacy() || self.discarding_hidden_body() {
            return false;
        }
        let (body_start, _) = self.wikidot.scored_div_direct;
        if body_start == SCORED_DIV_DIRECT_INACTIVE {
            return false;
        }
        let body_start = body_start as usize;
        if pos < body_start {
            return false;
        }
        self.full_text()
            .inner()
            .get(body_start..pos)
            .is_some_and(|gap| {
                gap.bytes()
                    .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
            })
    }

    pub(crate) fn wikidot_in_scored_div_direct_body(&self) -> bool {
        if !self.settings().layout.legacy() || self.discarding_hidden_body() {
            return false;
        }
        let (body_start, boundary_depth) = self.wikidot.scored_div_direct;
        if body_start == SCORED_DIV_DIRECT_INACTIVE {
            return false;
        }
        self.body_end_boundary_depth() == boundary_depth as usize + 1
    }

    #[inline]
    pub(crate) fn enter_wikidot_span_body(&mut self, scored: bool) {
        if scored {
            self.wikidot.scored_span_body_depth =
                self.wikidot.scored_span_body_depth.saturating_add(1);
        } else {
            self.wikidot.span_body_depth = self.wikidot.span_body_depth.saturating_add(1);
        }
    }

    #[inline]
    pub(crate) fn leave_wikidot_span_body(&mut self, scored: bool) {
        if scored {
            self.wikidot.scored_span_body_depth =
                self.wikidot.scored_span_body_depth.saturating_sub(1);
        } else {
            self.wikidot.span_body_depth = self.wikidot.span_body_depth.saturating_sub(1);
        }
    }

    #[inline]
    pub(crate) fn in_wikidot_span_body(&self) -> bool {
        self.wikidot.span_body_depth > 0
    }

    #[inline]
    pub(crate) fn enter_wikidot_anchor_body(&mut self) {
        self.wikidot.anchor_body_depth = self.wikidot.anchor_body_depth.saturating_add(1);
    }

    #[inline]
    pub(crate) fn leave_wikidot_anchor_body(&mut self) {
        self.wikidot.anchor_body_depth = self.wikidot.anchor_body_depth.saturating_sub(1);
    }

    #[inline]
    pub(crate) fn in_wikidot_anchor_body(&self) -> bool {
        self.wikidot.anchor_body_depth > 0
    }

    #[inline]
    pub(crate) fn in_wikidot_literal_alias_body(&self) -> bool {
        self.wikidot.scored_div_body_depth > 0 || self.wikidot.scored_span_body_depth > 0
    }

    #[inline]
    pub(crate) fn in_wikidot_literal_triple_link(&self) -> bool {
        self.wikidot.literal_triple_link_depth > 0
    }

    #[inline]
    pub(crate) fn enter_wikidot_literal_triple_link(&mut self) {
        self.wikidot.literal_triple_link_depth =
            self.wikidot.literal_triple_link_depth.saturating_add(1);
    }

    #[inline]
    pub(crate) fn leave_wikidot_literal_triple_link(&mut self) {
        self.wikidot.literal_triple_link_depth =
            self.wikidot.literal_triple_link_depth.saturating_sub(1);
    }

    #[inline]
    pub(crate) fn clear_wikidot_literal_triple_links(&mut self) {
        self.wikidot.literal_triple_link_depth = 0;
    }

    pub(crate) fn in_wikidot_collapsible(&self) -> bool {
        self.wikidot.flags & COLLAPSIBLE != 0
    }

    pub(crate) fn quote_boundary_closes_body(&self) -> bool {
        self.wikidot.flags & QUOTE_BOUNDARY_CLOSES_BODY != 0
    }

    pub(crate) fn pending_wikidot_collapsible_closer(&self) -> bool {
        self.wikidot.flags & PENDING_COLLAPSIBLE_CLOSER != 0
    }

    pub(crate) fn wikidot_collapsible_closed_at_deeper_quote(&self) -> bool {
        self.wikidot.flags & COLLAPSIBLE_CLOSED_AT_DEEPER_QUOTE != 0
    }

    pub(crate) fn set_in_wikidot_collapsible(&mut self, value: bool) {
        set_flag(&mut self.wikidot.flags, COLLAPSIBLE, value);
    }

    pub(crate) fn set_quote_boundary_closes_body(&mut self, value: bool) {
        set_flag(&mut self.wikidot.flags, QUOTE_BOUNDARY_CLOSES_BODY, value);
    }

    pub(crate) fn set_pending_wikidot_collapsible_closer(&mut self, value: bool) {
        set_flag(&mut self.wikidot.flags, PENDING_COLLAPSIBLE_CLOSER, value);
    }

    pub(crate) fn record_wikidot_crossed_close(&mut self, block_rule_name: &str) {
        let (shift, max) = match block_rule_name {
            "block-div" => (11, 3),
            "block-collapsible" => (13, 1),
            "block-blockquote" => (14, 1),
            "block-anchor" => (15, 1),
            _ => return,
        };
        if (self.wikidot.flags >> shift) & max < max {
            self.wikidot.flags += 1 << shift;
        }
    }

    pub(crate) fn has_wikidot_crossed_close(&self) -> bool {
        self.wikidot.flags >> 11 != 0
    }

    pub(crate) fn take_wikidot_crossed_close(&mut self, end_name: &str) -> bool {
        let (shift, max) = if end_name.eq_ignore_ascii_case("div") {
            (11, 3)
        } else if end_name.eq_ignore_ascii_case("collapsible") {
            (13, 1)
        } else if end_name.eq_ignore_ascii_case("blockquote") {
            (14, 1)
        } else if end_name.eq_ignore_ascii_case("a") {
            (15, 1)
        } else {
            return false;
        };
        if (self.wikidot.flags >> shift) & max == 0 {
            return false;
        }
        self.wikidot.flags -= 1 << shift;
        true
    }

    pub(crate) fn in_wikidot_center_body(&self) -> bool {
        self.wikidot.center_body_depth > 0
    }

    pub(crate) fn enter_wikidot_center_body(&mut self) {
        self.wikidot.center_body_depth = self.wikidot.center_body_depth.saturating_add(1);
    }

    pub(crate) fn leave_wikidot_center_body(&mut self) {
        self.wikidot.center_body_depth = self.wikidot.center_body_depth.saturating_sub(1);
    }

    pub(crate) fn mark_wikidot_collapsible_content_closed(&mut self) {
        set_flag(&mut self.wikidot.flags, COLLAPSIBLE_CONTENT_CLOSED, true);
    }

    pub(crate) fn take_wikidot_collapsible_content_closed(&mut self) -> bool {
        let closed = self.wikidot.flags & COLLAPSIBLE_CONTENT_CLOSED != 0;
        set_flag(&mut self.wikidot.flags, COLLAPSIBLE_CONTENT_CLOSED, false);
        closed
    }

    pub(crate) fn set_wikidot_collapsible_closed_at_deeper_quote(&mut self, value: bool) {
        set_flag(
            &mut self.wikidot.flags,
            COLLAPSIBLE_CLOSED_AT_DEEPER_QUOTE,
            value,
        );
    }

    pub(crate) fn in_wikidot_iftags_span(&self) -> bool {
        self.wikidot.flags & IFTAGS_LITERAL_SPAN != 0
    }

    pub(crate) fn set_in_wikidot_iftags_span(&mut self, value: bool) {
        set_flag(&mut self.wikidot.flags, IFTAGS_LITERAL_SPAN, value);
    }

    pub(crate) fn in_wikidot_definition_list_line(&self) -> bool {
        self.wikidot.flags & DEFINITION_LIST_LINE != 0
    }

    pub(crate) fn set_in_wikidot_definition_list_line(&mut self, value: bool) {
        set_flag(&mut self.wikidot.flags, DEFINITION_LIST_LINE, value);
    }

    pub(crate) fn in_wikidot_single_line_content(&self) -> bool {
        self.wikidot.flags & SINGLE_LINE_CONTENT != 0
    }

    pub(crate) fn set_in_wikidot_single_line_content(&mut self, value: bool) {
        set_flag(&mut self.wikidot.flags, SINGLE_LINE_CONTENT, value);
    }

    pub(crate) fn in_wikidot_simple_table_cell(&self) -> bool {
        self.wikidot.flags & SIMPLE_TABLE_CELL != 0
    }

    pub(crate) fn set_in_wikidot_simple_table_cell(&mut self, value: bool) {
        set_flag(&mut self.wikidot.flags, SIMPLE_TABLE_CELL, value);
    }

    pub(crate) fn mark_wikidot_mixed_paragraph_ownership(&mut self) {
        set_flag(&mut self.wikidot.flags, MIXED_PARAGRAPH_OWNERSHIP, true);
    }

    pub(crate) fn take_wikidot_mixed_paragraph_ownership(&mut self) -> bool {
        let marked = self.wikidot.flags & MIXED_PARAGRAPH_OWNERSHIP != 0;
        set_flag(&mut self.wikidot.flags, MIXED_PARAGRAPH_OWNERSHIP, false);
        marked
    }

    pub(crate) fn mark_wikidot_div_literal_recovered(&mut self) {
        set_flag(&mut self.wikidot.flags, DIV_LITERAL_RECOVERED, true);
    }

    pub(crate) fn take_wikidot_div_literal_recovered(&mut self) -> bool {
        let marked = self.wikidot.flags & DIV_LITERAL_RECOVERED != 0;
        set_flag(&mut self.wikidot.flags, DIV_LITERAL_RECOVERED, false);
        marked
    }

    pub(crate) fn set_wikidot_line_follows_block(&mut self, value: bool) {
        self.block_end_scan_cache
            .borrow()
            .line_follows_block
            .set(value);
    }

    pub(crate) fn wikidot_line_follows_block(&self) -> bool {
        self.block_end_scan_cache.borrow().line_follows_block.get()
    }

    pub(crate) fn mark_wikidot_bare_div_literal(&mut self) {
        self.block_end_scan_cache
            .borrow()
            .bare_div_literal
            .set(true);
    }

    pub(crate) fn take_wikidot_bare_div_literal(&mut self) -> bool {
        self.block_end_scan_cache
            .borrow()
            .bare_div_literal
            .replace(false)
    }

    pub(crate) fn mark_wikidot_colon_line_vanished(&mut self) {
        self.block_end_scan_cache
            .borrow()
            .colon_line_vanished
            .set(true);
    }

    pub(crate) fn take_wikidot_colon_line_vanished(&mut self) -> bool {
        self.block_end_scan_cache
            .borrow()
            .colon_line_vanished
            .replace(false)
    }

    pub(crate) fn mark_wikidot_simple_table_crossed_closer(&mut self, token: Token) {
        self.wikidot.simple_table_crossed_closers |= simple_table_closer_bit(token);
    }

    pub(crate) fn mark_wikidot_simple_table_crossed_split_closer(
        &mut self,
        delimiter: SplitDelimiter,
    ) {
        self.wikidot.simple_table_crossed_closers |=
            simple_table_split_closer_bit(delimiter);
    }

    pub(crate) fn take_wikidot_simple_table_crossed_closers(&mut self) -> u16 {
        std::mem::take(&mut self.wikidot.simple_table_crossed_closers)
    }

    pub(crate) fn wikidot_simple_table_closer_bit(token: Token) -> u16 {
        simple_table_closer_bit(token)
    }

    pub(crate) fn wikidot_simple_table_split_closer_bit(
        delimiter: SplitDelimiter,
    ) -> u16 {
        simple_table_split_closer_bit(delimiter)
    }
}

fn simple_table_split_closer_bit(delimiter: SplitDelimiter) -> u16 {
    match delimiter {
        SplitDelimiter::Bold => 1 << 8,
        SplitDelimiter::Italics => 1 << 9,
    }
}

fn set_flag(flags: &mut u16, flag: u16, value: bool) {
    if value {
        *flags |= flag;
    } else {
        *flags &= !flag;
    }
}

fn simple_table_closer_bit(token: Token) -> u16 {
    match token {
        Token::Bold => 1 << 0,
        Token::Italics => 1 << 1,
        Token::DoubleDash => 1 << 2,
        Token::Underline => 1 << 3,
        Token::Superscript => 1 << 4,
        Token::Subscript => 1 << 5,
        Token::RightMonospace => 1 << 6,
        Token::Color => 1 << 7,
        _ => 0,
    }
}

fn div_stream_skip_segment(tokens: &[ExtractedToken<'_>], index: usize) -> Option<usize> {
    let mut depth = 0_usize;
    let mut head_closed = false;
    let mut in_end_head = false;
    let mut index = index;
    while let Some(token) = tokens.get(index) {
        match token.token {
            Token::LeftBlock => depth += 1,
            Token::LeftBlockEnd => {
                if head_closed && depth == 0 {
                    in_end_head = true;
                }
                depth += 1;
            }
            Token::RightBlock => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    if in_end_head {
                        return Some(index + 1);
                    }
                    head_closed = true;
                }
            }
            Token::InputEnd => return None,
            _ => {}
        }
        index += 1;
    }
    None
}

fn div_stream_skip_comment(tokens: &[ExtractedToken<'_>], index: usize) -> Option<usize> {
    let mut index = index + 1;
    let mut hyphens = 0_usize;
    while let Some(token) = tokens.get(index) {
        if token.token == Token::InputEnd {
            return None;
        }
        let trailing_hyphens = token
            .slice
            .bytes()
            .rev()
            .take_while(|byte| *byte == b'-')
            .count();
        if trailing_hyphens >= 2
            && trailing_hyphens < token.slice.len()
            && tokens.get(index + 1).is_some_and(|next| {
                next.token == Token::RightBracket && next.slice == "]"
            })
        {
            return Some(index + 2);
        }
        if !token.slice.is_empty() && token.slice.bytes().all(|byte| byte == b'-') {
            hyphens += token.slice.len();
            index += 1;
            continue;
        }
        if hyphens >= 2 && token.token == Token::RightBracket && token.slice == "]" {
            return Some(index + 1);
        }
        hyphens = 0;
        index += 1;
    }
    None
}

fn div_stream_resolved_name<'a>(
    tokens: &[ExtractedToken<'a>],
    index: usize,
) -> Option<std::borrow::Cow<'a, str>> {
    let mut name = std::borrow::Cow::Borrowed("");
    let mut index = index + 1;
    while let Some(token) = tokens.get(index) {
        match token.token {
            Token::Identifier => {
                name.to_mut().push_str(token.slice);
                index += 1;
            }
            Token::LeftComment => match div_stream_skip_comment(tokens, index) {
                Some(next) => index = next,
                None => break,
            },
            Token::LeftBlock => match div_stream_skip_segment(tokens, index) {
                Some(next) => index = next,
                None => break,
            },
            _ => break,
        }
    }
    (!name.is_empty()).then_some(name)
}

pub(super) fn compute_wikidot_div_stream_marks(
    tokens: &[ExtractedToken<'_>],
) -> BTreeMap<usize, (u32, u32)> {
    fn block_name<'a>(tokens: &[ExtractedToken<'a>], index: usize) -> Option<&'a str> {
        tokens
            .get(index + 1)
            .and_then(|token| (token.token == Token::Identifier).then_some(token.slice))
    }

    fn is_div_name(tokens: &[ExtractedToken<'_>], index: usize) -> bool {
        div_stream_resolved_name(tokens, index)
            .is_some_and(|name| name.eq_ignore_ascii_case("div"))
    }

    let mut marks = BTreeMap::new();
    let mut balance: u32 = 0;
    let mut region: Vec<(usize, u32, u32)> = Vec::new();
    let mut region_ends: u32 = 0;
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if token.token == Token::LeftBlock {
            match block_name(tokens, index) {
                Some(name)
                    if name.eq_ignore_ascii_case("code")
                        || name.eq_ignore_ascii_case("html") =>
                {
                    let mut scan = index + 1;
                    while scan < tokens.len() {
                        if tokens[scan].token == Token::LeftBlockEnd
                            && block_name(tokens, scan)
                                .is_some_and(|end| end.eq_ignore_ascii_case(name))
                        {
                            break;
                        }
                        scan += 1;
                    }
                    index = scan;
                }
                _ if is_div_name(tokens, index) => {
                    region.push((token.span.start, balance, region_ends));
                    balance += 1;
                }
                _ => {}
            }
        } else if token.token == Token::LeftBlockEnd
            && balance > 0
            && is_div_name(tokens, index)
        {
            region_ends += 1;
            balance -= 1;
            if balance == 0 {
                for (span, balance_before, ends_before) in region.drain(..) {
                    marks.insert(span, (balance_before, region_ends - ends_before));
                }
                region_ends = 0;
            }
        }
        index += 1;
    }
    for (span, balance_before, ends_before) in region {
        marks.insert(span, (balance_before, region_ends - ends_before));
    }
    marks
}
