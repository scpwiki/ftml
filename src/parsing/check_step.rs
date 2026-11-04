/*
 * parsing/check_step.rs
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

use super::{ExtractedToken, ParseError, ParseErrorKind, Parser, Token};

pub fn assert_step<'r, 't>(
    parser: &mut Parser<'r, 't>,
    token: Token,
) -> Result<&'r ExtractedToken<'t>, ParseError> {
    let current = parser.current();
    assert_eq!(current.token, token, "Opening token isn't {}", token.name());
    parser.step()?;
    Ok(current)
}

pub fn check_step<'r, 't>(
    parser: &mut Parser<'r, 't>,
    token: Token,
    kind: ParseErrorKind,
) -> Result<&'r ExtractedToken<'t>, ParseError> {
    let current = parser.current();
    if current.token != token {
        error!("check_step failed: {}", kind.name());
        return Err(parser.make_err(kind));
    }
    parser.step()?;
    Ok(current)
}

#[test]
fn test_assert_step() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize("//Apple// banana");
    let mut parser = Parser::new(&tokenization, &page_info, &settings);
    parser.step().expect("cannot step");

    let _ = assert_step(&mut parser, Token::Italics);
}

#[test]
fn test_check_step() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    let error_kind = ParseErrorKind::InvalidInclude;
    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize("//Apple// banana");
    let mut parser = Parser::new(&tokenization, &page_info, &settings);
    parser.step().expect("cannot step");

    let result = check_step(&mut parser, Token::Bold, error_kind);
    let error = result.expect_err("check_step() succeeded when it was supposed to fail");
    assert_eq!(error.token(), Token::Italics);
    assert_eq!(error.kind(), error_kind);
}
