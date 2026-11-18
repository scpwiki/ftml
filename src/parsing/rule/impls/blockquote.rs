/*
 * parsing/rule/impls/blockquote.rs
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
use crate::parsing::paragraph::{NO_CLOSE_CONDITION, gather_paragraphs};
use crate::tree::{AttributeMap, Container, ContainerType};

const MAX_BLOCKQUOTE_DEPTH: usize = 30;

pub const RULE_BLOCKQUOTE: Rule = Rule {
    name: "blockquote",
    position: LineRequirement::StartOfLine,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing native blockquote");

    // 1 or more ">"s in one token.
    let current = parser.current();
    if current.token != Token::Quote {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    // Check that the depth isn't obscenely deep, to avoid DOS attacks via stack overflow.
    let depth = current.slice.len();
    if depth > MAX_BLOCKQUOTE_DEPTH {
        debug!(
            "Native blockquote has a depth ({depth}) greater than the maximum ({MAX_BLOCKQUOTE_DEPTH})! Failing"
        );
        return Err(parser.make_err(ParseErrorKind::BlockquoteDepthExceeded));
    }

    // Inside a blockquote, if no `>` was stripped from the current token,
    // the line prefix was already skipped. This token is part of the content so it cannot start a nested blockquote
    if parser.quote_depth() > 0 && parser.quote_consumed() == 0 {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    // Parse the contents as if they were their own document, with the ">"
    // line prefixes removed.
    let mut quote_parser = parser.quote_parser();
    let (elements, errors, _) =
        gather_paragraphs(&mut quote_parser, RULE_BLOCKQUOTE, NO_CLOSE_CONDITION)?.into();

    parser.finish_quote_parser(&quote_parser)?;

    let element = Element::Container(Container::new(
        ContainerType::Blockquote,
        elements,
        AttributeMap::new(),
    ));

    ok!(false; element, errors)
}
