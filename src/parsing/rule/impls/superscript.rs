/*
 * parsing/rule/impls/superscript.rs
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

use super::inline_delimiter::{
    ReachableCloseMode, append_wikidot_line_close_space, assert_unpadded_open,
    inline_body_padded_through_seams, validate_reachable_close,
};
use super::prelude::*;

pub const RULE_SUPERSCRIPT: Rule = Rule {
    name: "superscript",
    position: LineRequirement::Any,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Trying to create superscript container");
    validate_reachable_close(
        parser,
        Token::Superscript,
        None,
        ReachableCloseMode::BlockBarrier,
    )?;
    assert_unpadded_open(parser, Token::Superscript)?;
    let close = [ParseCondition::current(Token::Superscript)];
    let invalid = [
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::token_pair(Token::Superscript, Token::Whitespace),
        ParseCondition::token_pair(Token::Whitespace, Token::Superscript),
    ];
    let ctype = ContainerType::Superscript;
    let collected =
        collect_container(parser, RULE_SUPERSCRIPT, ctype, &close, &invalid, None)?;
    let (elements, errors, paragraph_safe) = collected.into();
    if parser.settings().layout.legacy()
        && matches!(
            &elements,
            Elements::Single(Element::Container(container))
                if inline_body_padded_through_seams(container.elements())
        )
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.settings().layout.legacy()
        && matches!(
            &elements,
            Elements::Single(Element::Container(container)) if container.elements().is_empty()
        )
    {
        return ok!(paragraph_safe; Elements::None, errors);
    }
    let elements =
        append_wikidot_line_close_space(elements, parser.settings().layout.legacy());
    ok!(paragraph_safe; elements, errors)
}
