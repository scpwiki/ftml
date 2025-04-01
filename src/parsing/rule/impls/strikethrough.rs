/*
 * parsing/rule/impls/strikethrough.rs
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

//! Rules for strikethrough.
//!
//! Wikidot had implemented strikethrough using --text--
//! however we also added the more conventional way ~~text~~

use super::prelude::*;

pub const RULE_DASH_STRIKETHROUGH: Rule = Rule {
    name: "dash-strikethrough",
    position: LineRequirement::Any,
    try_consume_fn: dash,
};

pub const RULE_TILDE_STRIKETHROUGH: Rule = Rule {
    name: "tilde-strikethrough",
    position: LineRequirement::Any,
    try_consume_fn: tilde,
};

fn dash<'r, 't>(parser: &mut Parser<'r, 't>) -> ParseResult<'r, 't, Elements<'t>> {
    trace!("Trying to create a double dash strikethrough");
    check_step(parser, Token::DoubleDash)?;
    try_consume_strikethrough(parser, RULE_DASH_STRIKETHROUGH, Token::DoubleDash)
}

fn tilde<'r, 't>(parser: &mut Parser<'r, 't>) -> ParseResult<'r, 't, Elements<'t>> {
    trace!("Trying to create a double tilde strikethrough");
    check_step(parser, Token::DoubleTilde)?;
    try_consume_strikethrough(parser, RULE_TILDE_STRIKETHROUGH, Token::DoubleTilde)
}

/// Build a strikethrough with the given rule and token.
fn try_consume_strikethrough<'r, 't>(
    parser: &mut Parser<'r, 't>,
    rule: Rule,
    token: Token,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!(
        "Trying to create a strikethrough (token {})",
        token.name(),
    );

    collect_container(
        parser,
        rule,
        ContainerType::Strikethrough,
        &[ParseCondition::current(token)],
        &[
            ParseCondition::current(Token::ParagraphBreak),
            ParseCondition::token_pair(token, Token::Whitespace),
            ParseCondition::token_pair(Token::Whitespace, token),
        ],
        None,
    )
}
