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
use crate::parsing::trim_wikidot_ascii_cow;
use std::borrow::Cow;

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
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing user block (name '{name}', in-head {in_head})");
    assert!(!flag_score, "User doesn't allow score flag");
    assert_block_name(&BLOCK_USER, name);

    let block = &BLOCK_USER;
    let name = parser.get_head_value_resolved(block, in_head, |parser, value| {
        let value = match value {
            Some(value) => value,
            None => {
                return Err(parser.make_err(ParseErrorKind::BlockMissingArguments));
            }
        };

        let name = if parser.settings().layout.legacy() {
            trim_wikidot_ascii_cow(value)
        } else {
            match value {
                Cow::Borrowed(value) => Cow::Borrowed(value.trim()),
                Cow::Owned(value) => Cow::Owned(value.trim().to_owned()),
            }
        };

        Ok(name)
    })?;

    let element = Element::User {
        name,
        show_avatar: flag_star,
    };

    success_elements(element)
}
