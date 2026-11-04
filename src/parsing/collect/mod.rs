/*
 * parsing/collect/mod.rs
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
    pub use super::collect;
    pub use crate::parsing::condition::{ParseCondition, SplitDelimiter};
    pub use crate::parsing::consume::consume;
    pub use crate::parsing::error::{ParseError, ParseErrorKind};
    pub use crate::parsing::parser::Parser;
    pub use crate::parsing::prelude::*;
    pub use crate::parsing::rule::Rule;
    pub use crate::parsing::token::{ExtractedToken, Token};
}

mod comment;
mod consume;
mod container;
mod generic;
mod iftags;
mod text;

pub(crate) use self::comment::{
    CommentElidedText, collect_comment_elided_keep, collect_comment_elided_keep_from,
    comment_bridge_end, comment_closer_token_count_at, consume_valid_comment,
};
pub use self::consume::{
    CollectSplitEscape, collect_consume, collect_consume_before, collect_consume_keep,
    collect_consume_keep_split,
};
pub use self::container::collect_container;
pub use self::generic::collect;
pub(crate) use self::iftags::{
    SegmentCtx, TokenWindow, complete_fused_bracket_close, consume_iftags_segment,
    consume_iftags_segment_fused, iftags_segment_fused, split_delimiter_end,
};
#[cfg(test)]
pub(crate) use self::iftags::{
    iftags_segment, reset_segment_scan_token_visits, segment_scan_token_visits,
};
pub use self::text::{
    collect_text, collect_text_keep, collect_text_resolved, collect_text_resolved_fused,
    collect_text_resolved_fused_tracked,
};
