/*
 * parsing/rule/impls/block/blocks/note.rs
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

pub const BLOCK_NOTE: BlockRule = BlockRule {
    name: "block-note",
    accepts_names: &["note"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines: true,
    parse_fn: parse_note_block,
};

fn parse_note_block<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing highlight block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Note doesn't allow star flag");
    assert!(!flag_score, "Note doesn't allow score flag");
    assert_block_name(&BLOCK_NOTE, name);

    let arguments = parser.get_head_map(&BLOCK_NOTE, in_head)?;

    // `[[note]]` is a block element and must start on a new line.
    // Legacy `Text_Wiki` parser also rejected inline note blocks, so preserve
    // that behavior for compatibility.
    if !parser.start_of_line() {
        return Err(parser.make_err(ParseErrorKind::NotSupportedInline));
    }

    // Get body content, without paragraphs
    let (elements, errors, paragraph_safe) =
        parser.get_body_elements(&BLOCK_NOTE, false)?.into();

    // Build and return element
    let element = Element::Container(Container::new(
        ContainerType::Note,
        elements,
        arguments.to_attribute_map(parser.settings()),
    ));

    ok!(paragraph_safe; element, errors)
}
