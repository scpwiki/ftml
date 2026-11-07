/*
 * parsing/rule/impls/mod.rs
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

mod prelude {
    pub use crate::parsing::check_step::assert_step;
    pub use crate::parsing::collect::*;
    pub use crate::parsing::condition::{ParseCondition, SplitDelimiter};
    pub use crate::parsing::consume::consume;
    pub use crate::parsing::error::{ParseError, ParseErrorKind};
    pub use crate::parsing::parser::Parser;
    pub use crate::parsing::result::{ParseResult, success_elements};
    pub use crate::parsing::rule::{LineRequirement, Rule};
    pub use crate::parsing::token::{ExtractedToken, Token};
    pub use crate::tree::{AttributeMap, ContainerType, Element, Elements};
}

mod anchor;
mod bibcite;
pub(crate) mod block;
mod blockquote;
mod bold;
mod center;
mod clear_float;
mod color;
mod comment;
mod dash;
mod definition_list;
mod delayed;
mod double_angle;
mod email;
mod entity;
mod fallback;
mod footnote_first;
mod header;
mod horizontal_rule;
mod inline_delimiter;
mod italics;
mod line_break;
mod line_owner;
mod link_anchor;
mod link_single;
mod link_triple;
mod link_triple_transliteration;
mod list;
mod math;
mod monospace;
mod null;
mod page;
mod raw;
mod section_marker;
mod strikethrough;
mod subscript;
mod superscript;
mod table;
pub(crate) use self::table::{
    eject_wikidot_row_suffix, keep_wikidot_row_suffix, wikidot_row_suffix_start_index,
};
mod text;
mod underline;
mod underscore_line_break;
mod url;
mod variable;

pub use self::anchor::RULE_ANCHOR;
pub use self::bibcite::RULE_BIBCITE;
pub(crate) use self::block::blocks::parse_wikidot_implicit_list_block;
pub(crate) use self::block::blocks::wikidot_greedy_tail;
pub(crate) use self::block::blocks::wikidot_math_name;
pub(crate) use self::block::blocks::wikidot_scored_div_close_compatible;
pub(crate) use self::block::head_word_names_known_block;
pub use self::block::{RULE_BLOCK, RULE_BLOCK_SKIP_NEWLINE, RULE_BLOCK_STAR};
pub(crate) use self::block::{
    get_block_rule_with_name_for_layout, head_seam_forms_block, head_word_never_resolves,
};
pub use self::blockquote::RULE_BLOCKQUOTE;
pub use self::bold::RULE_BOLD;
pub use self::center::RULE_CENTER;
pub use self::clear_float::RULE_CLEAR_FLOAT;
pub use self::color::RULE_COLOR;
pub(crate) use self::color::normalize_wikidot_color;
pub use self::comment::RULE_COMMENT;
pub(crate) use self::comment::comment_closer_token_count;
pub use self::dash::{RULE_DASH, RULE_DASH_RUN};
pub use self::definition_list::{
    RULE_DEFINITION_LIST, RULE_DEFINITION_LIST_SKIP_NEWLINE,
};
pub use self::delayed::{RULE_DELAYED, RULE_DELAYED_CONDITIONAL};
pub use self::double_angle::RULE_DOUBLE_ANGLE;
pub use self::email::RULE_EMAIL;
pub use self::entity::RULE_ENTITY;
pub(crate) use self::entity::decode_semicolon_entities;
pub use self::fallback::RULE_FALLBACK;
pub use self::header::RULE_HEADER;
pub use self::horizontal_rule::RULE_HORIZONTAL_RULE;
pub use self::italics::RULE_ITALICS;
pub use self::line_break::{RULE_LINE_BREAK, RULE_LINE_BREAK_PARAGRAPH};
pub(crate) use self::line_break::{
    line_break_skipped_for_own_line_block, starts_own_line_rule,
};
pub use self::link_anchor::RULE_LINK_ANCHOR;
pub use self::link_single::{
    RULE_LINK_SINGLE, RULE_LINK_SINGLE_NEW_TAB, try_consume_wikidot_end_block_link,
};
pub use self::link_triple::{RULE_LINK_TRIPLE, RULE_LINK_TRIPLE_NEW_TAB};
pub use self::list::RULE_LIST;
pub(crate) use self::list::{
    build_list_element, collect_gate_body_list_line, list_item_marker_here,
};
pub use self::math::RULE_MATH;
pub use self::monospace::RULE_MONOSPACE;
pub use self::null::RULE_NULL;
pub use self::page::RULE_PAGE;
pub use self::raw::RULE_RAW;
pub use self::section_marker::RULE_SECTION_MARKER;
pub use self::strikethrough::{RULE_STRIKETHROUGH_DASH, RULE_STRIKETHROUGH_TILDE};
pub use self::subscript::RULE_SUBSCRIPT;
pub use self::superscript::RULE_SUPERSCRIPT;
pub use self::table::RULE_TABLE;
pub(crate) use self::table::wikidot_simple_table_markers_literal_on_line_after_explicit_li;
pub use self::text::RULE_TEXT;
pub use self::underline::RULE_UNDERLINE;
pub use self::underscore_line_break::RULE_UNDERSCORE_LINE_BREAK;
pub use self::url::{RULE_URL, RULE_WIKIDOT_SCHEME_URL};
pub(crate) use self::url::{
    glued_scheme_url_start, identifier_may_start_wikidot_url,
    is_wikidot_only_autolink_url, starts_wikidot_only_scheme_url, url_elements,
};
pub use self::variable::RULE_VARIABLE;
