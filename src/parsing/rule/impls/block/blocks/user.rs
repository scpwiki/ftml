/*
 * parsing/rule/impls/block/blocks/user.rs
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

pub const BLOCK_USER: BlockRule = BlockRule {
    name: "block-user",
    accepts_names: &["user"],
    accepts_star: true,
    accepts_score: false,
    accepts_newlines_outside_body: false,
    parse_fn,
};

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing user block (name '{name}', in-head {in_head})");
    assert!(!flag_score, "User doesn't allow score flag");
    assert_block_name(&BLOCK_USER, name);

    let block = &BLOCK_USER;
    let name = parser.get_head_value_resolved(
        block,
        in_head,
        require_trimmed_block_argument_resolved,
    )?;

    let element = Element::User {
        name,
        show_avatar: flag_star,
    };

    success_elements(element)
}
