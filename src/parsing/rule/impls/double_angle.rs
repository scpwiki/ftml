/*
 * parsing/rule/impls/double_angle.rs
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

pub const RULE_DOUBLE_ANGLE: Rule = Rule {
    name: "double-angle",
    position: LineRequirement::Any,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    let current = parser.current();
    debug!("Creating double angle quote");

    match current.token {
        //
        // In the Wikidot layout, preprocess typography has already
        // converted every `<<` in ordinary text, so a `<<` token that
        // still reaches this rule in text flow is one the preprocess
        // deferral left raw at a conditional seam (protected owners
        // such as links and math consume theirs elsewhere). Converting
        // it here would pair it within its own segment, before the
        // seam code can regroup it against its joined-text neighbours,
        // so fail instead: the token falls back to raw text and the
        // seam code performs the conversion with the seam in view.
        Token::LeftDoubleAngle if parser.settings().layout.legacy() => {
            Err(parser.make_err(ParseErrorKind::RuleFailed))
        }

        Token::LeftDoubleAngle => success_elements(text!("\u{0ab}")),

        Token::Quote if current.slice == ">>" => success_elements(text!("\u{0bb}")),

        Token::Quote => Err(parser.make_err(ParseErrorKind::RuleFailed)),

        _ => unreachable!(),
    }
}
