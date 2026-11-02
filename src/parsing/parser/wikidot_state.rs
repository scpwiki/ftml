use super::Parser;
use crate::parsing::Token;

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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct WikidotState {
    div_body_depth: u16,
    scored_div_body_depth: u16,
    scored_span_body_depth: u8,
    span_body_depth: u8,
    bibliography_body_depth: u16,
    literal_triple_link_depth: u16,
    simple_table_crossed_closers: u16,
    center_body_depth: u16,
    flags: u16,
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
    pub(crate) fn wikidot_div_body_depth(&self) -> usize {
        usize::from(self.wikidot.div_body_depth)
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

    pub(crate) fn mark_wikidot_simple_table_crossed_closer(&mut self, token: Token) {
        self.wikidot.simple_table_crossed_closers |= simple_table_closer_bit(token);
    }

    pub(crate) fn take_wikidot_simple_table_crossed_closers(&mut self) -> u16 {
        std::mem::take(&mut self.wikidot.simple_table_crossed_closers)
    }

    pub(crate) fn wikidot_simple_table_closer_bit(token: Token) -> u16 {
        simple_table_closer_bit(token)
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
