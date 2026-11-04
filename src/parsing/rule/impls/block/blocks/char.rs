/*
 * parsing/rule/impls/block/blocks/char.rs
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
use crate::parsing::rule::impls::entity::{find_entity, strip_entity};
use std::borrow::Cow;

pub const BLOCK_CHAR: BlockRule = BlockRule {
    name: "block-char",
    accepts_names: &["char", "character"],
    accepts_star: false,
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
    debug!("Parsing character / HTML entity block (in-head {in_head})");
    assert!(!flag_star, "Char doesn't allow star flag");
    assert!(!flag_score, "Char doesn't allow score flag");
    assert_block_name(&BLOCK_CHAR, name);

    let string = parser.get_head_value(&BLOCK_CHAR, in_head, parse_entity)?;

    ok!(Element::Text(string))
}

fn parse_entity<'t>(
    parser: &Parser<'_, 't>,
    argument: Option<&'t str>,
) -> Result<Cow<'t, str>, ParseError> {
    let argument = match argument {
        Some(arg) => strip_entity(arg),
        None => return Err(parser.make_err(ParseErrorKind::BlockMissingArguments)),
    };

    match find_entity(argument) {
        Some(string) => Ok(string),
        None => Err(parser.make_err(ParseErrorKind::BlockMalformedArguments)),
    }
}

#[test]
fn parse_entity_rejects_missing_argument() {
    let page_info = crate::data::PageInfo::dummy();
    let settings = crate::settings::WikitextSettings::from_mode(
        crate::settings::WikitextMode::Page,
        crate::layout::Layout::Wikidot,
    );
    let tokenization = crate::tokenize("[[char]]");
    let parser = Parser::new(&tokenization, &page_info, &settings);

    let error = parse_entity(&parser, None).expect_err("missing entity should fail");
    assert_eq!(error.kind(), ParseErrorKind::BlockMissingArguments);
}

#[test]
fn parse_entity_rejects_unknown_entity() {
    let page_info = crate::data::PageInfo::dummy();
    let settings = crate::settings::WikitextSettings::from_mode(
        crate::settings::WikitextMode::Page,
        crate::layout::Layout::Wikidot,
    );
    let tokenization = crate::tokenize("[[char not-an-entity]]");
    let parser = Parser::new(&tokenization, &page_info, &settings);

    let error = parse_entity(&parser, Some("not-an-entity"))
        .expect_err("unknown entity should fail");
    assert_eq!(error.kind(), ParseErrorKind::BlockMalformedArguments);
}
