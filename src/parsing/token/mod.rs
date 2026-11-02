/*
 * parsing/token/mod.rs
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

#[cfg(test)]
mod test;

mod scanner;

#[cfg(test)]
mod lexer {
    // Since pest generates some code that clippy doesn't like
    #![allow(clippy::upper_case_acronyms, clippy::empty_docs)]

    #[derive(Parser, Debug)]
    #[grammar = "parsing/lexer.pest"]
    pub struct TokenLexer;
}

#[cfg(test)]
use self::lexer::*;
use crate::utf16::Utf16IndexMap;
#[cfg(test)]
use pest::Parser;
#[cfg(test)]
use pest::iterators::{Pair, Pairs};
#[cfg(test)]
use std::fmt::Display;
use std::ops::Range;
use strum_macros::IntoStaticStr;

/// Struct that represents a token in a specific text.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct ExtractedToken<'a> {
    pub token: Token,
    pub slice: &'a str,
    pub span: Range<usize>,
}

impl ExtractedToken<'_> {
    /// Returns a new object with the same values, except with span refering to the byte indicies
    /// of the text if it were in UTF-16 rather than in UTF-8.
    #[must_use]
    pub fn to_utf16_indices(&self, map: &Utf16IndexMap) -> Self {
        let ExtractedToken { token, slice, span } = self.clone();

        let start = map.get_index(span.start);
        let end = map.get_index(span.end);
        let span = start..end;

        ExtractedToken { token, slice, span }
    }
}

/// Enum that represents the type of a parsed token. For a struct with additional context
/// surrounding the positioning and content of the token, see [`ExtractedToken`].
#[derive(
    Serialize, Deserialize, Enum, IntoStaticStr, Debug, Copy, Clone, PartialEq, Eq,
)]
#[serde(rename_all = "kebab-case")]
pub enum Token {
    LeftBracket,
    LeftBracketAnchor,
    LeftBracketStar,
    RightBracket,
    LeftBlock,
    LeftBlockEnd,
    LeftBlockAnchor,
    LeftBlockStar,
    LeftMath,
    LeftParentheses,
    RightBlock,
    RightMath,
    RightParentheses,
    DoubleDash,
    TripleDash,
    DoubleTilde,
    LeftDoubleAngle,
    ClearFloatBoth,
    ClearFloatLeft,
    ClearFloatRight,
    Pipe,
    Equals,
    Colon,
    Underscore,
    Quote,
    Heading,

    LineBreak,
    ParagraphBreak,
    Whitespace,

    Bold,
    Italics,
    Underline,
    Superscript,
    Subscript,
    LeftMonospace,
    RightMonospace,
    Color,
    Raw,
    LeftRaw,
    RightRaw,

    BulletItem,
    NumberedItem,

    LeftLink,
    LeftLinkStar,
    RightLink,

    TableColumn,
    TableColumnRight,
    TableColumnCenter,
    TableColumnTitle,

    Identifier,
    Email,
    Url,
    UrlStar,
    Variable,
    DoubleQuote,
    EscapedDoubleQuote,
    EscapedBackslash,
    DiscardedControl,
    RuntimeText,
    GeneratedPageLink,
    GeneratedTagLinks,

    LeftComment,
    InputStart,
    InputEnd,

    Other,
}

impl Token {
    pub(crate) fn extract_all(text: &str) -> Vec<ExtractedToken<'_>> {
        debug!("Running lexer on input");

        scanner::extract_all(text)
    }

    pub(crate) fn extract_all_with_trailing_comment_closer(
        text: &str,
        trailing_comment_closer: bool,
    ) -> Vec<ExtractedToken<'_>> {
        debug!("Running lexer on delayed authored input");

        scanner::extract_all_with_trailing_comment_closer(text, trailing_comment_closer)
    }

    #[cfg(test)]
    fn extract_all_pest(text: &str) -> Vec<ExtractedToken<'_>> {
        Self::extract_tokens_from_pairs(text, TokenLexer::parse(Rule::document, text))
    }

    #[cfg(test)]
    fn extract_tokens_from_pairs<'a>(
        text: &'a str,
        result: Result<Pairs<'a, Rule>, impl Display>,
    ) -> Vec<ExtractedToken<'a>> {
        match result {
            Ok(pairs) => {
                debug!("Lexer produced pairs for processing");

                let token = Token::InputStart;
                let slice = "";
                let span = 0..0;
                let start = ExtractedToken { token, slice, span };

                let mut tokens = vec![start];
                tokens.extend(pairs.map(Token::convert_pair));
                tokens
            }
            Err(error) => lexer_error_tokens(text, error),
        }
    }

    #[cfg(test)]
    fn convert_pair(pair: Pair<Rule>) -> ExtractedToken {
        let rule = pair.as_rule();
        let slice = pair.as_str();
        let start = pair.as_span().start();
        let end = pair.as_span().end();
        let span = start..end;

        let token = Token::get_from_rule(rule);
        trace!("Converting pair '{:?}' into token {}", rule, token.name());

        ExtractedToken { token, slice, span }
    }

    #[cfg(test)]
    fn get_from_rule(rule: Rule) -> Token {
        match rule {
            Rule::left_comment => Token::LeftComment,
            Rule::left_bracket => Token::LeftBracket,
            Rule::left_bracket_anchor => Token::LeftBracketAnchor,
            Rule::left_bracket_star => Token::LeftBracketStar,
            Rule::right_bracket => Token::RightBracket,
            Rule::left_parens => Token::LeftParentheses,
            Rule::right_parens => Token::RightParentheses,
            Rule::left_block => Token::LeftBlock,
            Rule::left_block_end => Token::LeftBlockEnd,
            Rule::left_block_anchor => Token::LeftBlockAnchor,
            Rule::left_block_star => Token::LeftBlockStar,
            Rule::left_math => Token::LeftMath,
            Rule::right_block => Token::RightBlock,
            Rule::right_math => Token::RightMath,
            Rule::color => Token::Color,
            Rule::double_dash => Token::DoubleDash,
            Rule::triple_dash => Token::TripleDash,
            Rule::double_tilde => Token::DoubleTilde,
            Rule::left_double_angle => Token::LeftDoubleAngle,
            Rule::clear_float => Token::ClearFloatBoth,
            Rule::clear_float_left => Token::ClearFloatLeft,
            Rule::clear_float_right => Token::ClearFloatRight,
            Rule::pipe => Token::Pipe,
            Rule::colon => Token::Colon,
            Rule::underscore => Token::Underscore,
            Rule::equals => Token::Equals,
            Rule::quote => Token::Quote,
            Rule::heading => Token::Heading,

            Rule::line_break => Token::LineBreak,
            Rule::paragraph_break => Token::ParagraphBreak,
            Rule::space => Token::Whitespace,

            Rule::bold => Token::Bold,
            Rule::italics => Token::Italics,
            Rule::underline => Token::Underline,
            Rule::superscript => Token::Superscript,
            Rule::subscript => Token::Subscript,
            Rule::left_monospace => Token::LeftMonospace,
            Rule::right_monospace => Token::RightMonospace,
            Rule::raw => Token::Raw,
            Rule::left_raw => Token::LeftRaw,
            Rule::right_raw => Token::RightRaw,

            Rule::bullet_item => Token::BulletItem,
            Rule::numbered_item => Token::NumberedItem,

            Rule::left_link => Token::LeftLink,
            Rule::left_link_star => Token::LeftLinkStar,
            Rule::right_link => Token::RightLink,

            Rule::table_column => Token::TableColumn,
            Rule::table_column_right => Token::TableColumnRight,
            Rule::table_column_center => Token::TableColumnCenter,
            Rule::table_column_title => Token::TableColumnTitle,

            Rule::identifier => Token::Identifier,
            Rule::email => Token::Email,
            Rule::url => Token::Url,
            Rule::url_star => Token::UrlStar,
            Rule::url_star_pre => Token::Other,
            Rule::variable => Token::Variable,
            Rule::double_quote => Token::DoubleQuote,
            Rule::escaped_double_quote => Token::EscapedDoubleQuote,
            Rule::escaped_backslash => Token::EscapedBackslash,
            Rule::terminal_backslash => Token::LineBreak,
            Rule::discarded_control => Token::DiscardedControl,

            Rule::other => Token::Other,
            Rule::EOI => Token::InputEnd,

            Rule::document | Rule::token => {
                panic!("Received invalid pest rule: {rule:?}")
            }
        }
    }

    #[inline]
    pub fn name(self) -> &'static str {
        self.into()
    }
}

#[cfg(test)]
fn lexer_error_tokens<'a>(text: &'a str, error: impl Display) -> Vec<ExtractedToken<'a>> {
    error!("Error while lexing input in pest: {error}");
    let token = ExtractedToken {
        token: Token::Other,
        slice: text,
        span: 0..text.len(),
    };
    vec![token]
}
