/*
 * parsing/rule/impls/block/blocks/raw.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2025 Wikijump Team
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

pub const BLOCK_RAW: BlockRule = BlockRule {
    name: "block-raw",
    accepts_names: &["raw"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines: true,
    parse_fn,
};

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    trace!("Parsing raw block (name '{name}', in-head {in_head}, score {flag_score})");
    assert!(!flag_star, "Raw doesn't allow star flag");
    assert!(!flag_score, "Raw doesn't allow score flag");

    assert_block_name(&BLOCK_RAW, name);

    let (start, mut end) = {
        let current = parser.current();

        (current, current)
    };
    loop {
        // Check if we reach an end block token
        if let Ok(found_name) = parser.get_end_block() {
            // If so, check if it's a raw end block token
            if found_name == name {
                trace!("Parsing block start: {start:?}, end: {end:?}, name: {name})");
                let slice = parser.full_text().slice_partial(start, end);
                let element = Element::Raw(cow!(slice));
                return ok!(element);
            }
        }
        // Update last token and step.
        end = parser.step()?;
    }
}
