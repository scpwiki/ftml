/*
 * parse/rule/impls/block/impls/code.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2020 Ammon Smith
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

pub const BLOCK_CODE: BlockRule = BlockRule {
    name: "block-code",
    accepts_names: &["code"],
    accepts_special: false,
    parse_fn,
};

fn parse_fn<'l, 'r, 't>(
    log: &'l slog::Logger,
    parser: &mut BlockParser<'l, 'r, 't>,
    name: &'t str,
    special: bool,
) -> Result<BlockParseOutcome<'r, 't>, ParseError> {
    let arguments = parser.get_argument_map()?;
    let language = arguments.get("type");
    parser.get_line_break()?;

    // Proceed until we find "[[/code]]"


    todo!()
}
