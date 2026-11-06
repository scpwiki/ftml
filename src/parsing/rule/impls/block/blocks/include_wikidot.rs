/*
 * parsing/rule/impls/block/blocks/include_wikidot.rs
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

use super::prelude::*;

pub const BLOCK_INCLUDE_WIKIDOT: BlockRule = BlockRule {
    name: "block-include",
    accepts_names: &["include"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn,
};

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    parser.check_page_syntax()?;
    assert!(!flag_star, "Include (Wikidot) doesn't allow star flag");
    assert!(!flag_score, "Include (Wikidot) doesn't allow score flag");
    assert_block_name(&BLOCK_INCLUDE_WIKIDOT, name);

    if !in_head
        && let Some(marker) =
            targetless_include_marker(parser.full_text().inner(), opener_start, name)
    {
        debug!("Preserving exact targetless include marker as literal text");
        return success_elements(text!(marker));
    }

    debug!("Found invalid include block");
    Err(parser.make_err(ParseErrorKind::InvalidInclude))
}

fn targetless_include_marker<'t>(
    source: &'t str,
    opener_start: usize,
    name: &str,
) -> Option<&'t str> {
    let rest = source.get(opener_start..)?;
    if !rest.starts_with("[[") {
        return None;
    }
    let close = rest.find("]]")?;
    let marker = &rest[..close + 2];
    let inner = marker.strip_prefix("[[")?.strip_suffix("]]")?;
    if inner.trim().eq_ignore_ascii_case(name) {
        Some(marker)
    } else {
        None
    }
}
