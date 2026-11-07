/*
 * parsing/condition.rs
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

use super::token::{ExtractedToken, Token};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SplitDelimiter {
    Bold,
    Italics,
}

impl SplitDelimiter {
    pub(crate) fn half_char(self) -> char {
        match self {
            SplitDelimiter::Bold => '*',
            SplitDelimiter::Italics => '/',
        }
    }

    pub(crate) fn delimiter_token(self) -> Token {
        match self {
            SplitDelimiter::Bold => Token::Bold,
            SplitDelimiter::Italics => Token::Italics,
        }
    }

    pub(crate) fn is_half(self, token: &ExtractedToken) -> bool {
        match self {
            SplitDelimiter::Bold => token.token == Token::BulletItem,
            SplitDelimiter::Italics => token.token == Token::Other && token.slice == "/",
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum ParseCondition {
    CurrentToken(Token),

    /// The current token is `token` and is the first non-blank content on its
    /// physical source line. The stream-level `start_of_line` flag only follows
    /// break tokens, so a consumed construct (an elided comment, a kept
    /// `[[iftags]]` body) can hide a physical line start that live Wikidot
    /// still honors.
    CurrentTokenAtSourceLineStart(Token),

    TokenPair(Token, Token),

    /// The current token starts at or after this source byte. Used to split a
    /// line construct at a block closer that must not be consumed as content.
    AtSourceByte(usize),

    /// A list marker that is the first content token of a quote-prefixed line.
    /// The physical line starts at `>`, so a source-line-start check misses it.
    QuotePreparedListMarker,

    SplitClose(SplitDelimiter),

    PaddedSplitClose(SplitDelimiter),
}

impl ParseCondition {
    #[inline]
    pub fn current(token: Token) -> Self {
        ParseCondition::CurrentToken(token)
    }

    #[inline]
    pub fn source_line_start(token: Token) -> Self {
        ParseCondition::CurrentTokenAtSourceLineStart(token)
    }

    #[inline]
    pub fn token_pair(current: Token, next: Token) -> Self {
        ParseCondition::TokenPair(current, next)
    }
}
