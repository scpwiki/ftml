/*
 * parsing/rule/impls/clear_float.rs
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
use crate::tree::ClearFloat;

pub const RULE_CLEAR_FLOAT: Rule = Rule {
    name: "clear-float",
    position: LineRequirement::StartOfLine,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    let current = parser.current();
    debug!("Consuming token to create a clear float");

    if parser.settings().layout.legacy() {
        let source = parser.full_text().inner();
        let starts_line = current.span.start == 0
            || source.as_bytes().get(current.span.start - 1) == Some(&b'\n');
        let line_end = source[current.span.end..]
            .find(['\r', '\n'])
            .map_or(source.len(), |offset| current.span.end + offset);
        if !(starts_line || parser.in_native_blockquote_line())
            || current.span.end != line_end
        {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
    }

    let clear_float = match current.token {
        Token::ClearFloatBoth => ClearFloat::Both,
        Token::ClearFloatLeft => ClearFloat::Left,
        Token::ClearFloatRight => ClearFloat::Right,
        _ => return Err(parser.make_err(ParseErrorKind::RuleFailed)),
    };

    parser.get_optional_line_break()?;

    ok!(Element::ClearFloat(clear_float))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn clear_float_rejects_non_clear_float_token() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("text");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.set_rule(RULE_CLEAR_FLOAT);

        let error = RULE_CLEAR_FLOAT
            .try_consume(&mut parser)
            .expect_err("non-clear-float token should fail");
        assert_eq!(error.kind(), ParseErrorKind::RuleFailed);
    }
}
