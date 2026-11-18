/*
 * tokenizer.rs
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

use crate::parsing::{ExtractedToken, Token};
use crate::text::FullText;

/// Struct that represents both a list of tokens and the text the tokens were generated from.
#[derive(Debug, Clone)]
pub struct Tokenization<'t> {
    tokens: Vec<ExtractedToken<'t>>,
    full_text: FullText<'t>,
    quote_paragraph_breaks: Vec<ExtractedToken<'t>>,
    quote_indents: Vec<ExtractedToken<'t>>,
}

impl<'t> Tokenization<'t> {
    #[inline]
    pub fn tokens<'r>(&'r self) -> &'r [ExtractedToken<'t>] {
        &self.tokens
    }

    #[inline]
    pub(crate) fn full_text(&self) -> FullText<'t> {
        self.full_text
    }

    #[inline]
    pub(crate) fn quote_paragraph_breaks<'r>(&'r self) -> &'r [ExtractedToken<'t>] {
        &self.quote_paragraph_breaks
    }

    #[inline]
    pub(crate) fn quote_indents<'r>(&'r self) -> &'r [ExtractedToken<'t>] {
        &self.quote_indents
    }
}

fn build_quote_tokens<'t>(
    tokens: &[ExtractedToken<'t>],
) -> (Vec<ExtractedToken<'t>>, Vec<ExtractedToken<'t>>) {
    let mut paragraph_breaks = Vec::new();
    let mut indents = Vec::new();

    for (index, quote) in tokens.iter().enumerate() {
        if quote.token != Token::Quote || index == 0 {
            continue;
        }

        // Only ">"s at the start of a line are prefixes
        let previous = &tokens[index - 1];
        match previous.token {
            Token::LineBreak => paragraph_breaks.push(ExtractedToken {
                token: Token::ParagraphBreak,
                slice: previous.slice,
                span: previous.span.clone(),
            }),
            Token::ParagraphBreak | Token::InputStart => (),
            _ => continue,
        }

        // Whitespace is always ASCII, so slicing off one byte is fine
        if let Some(space) = tokens.get(index + 1)
            && space.token == Token::Whitespace
            && space.slice.len() > 1
        {
            indents.push(ExtractedToken {
                token: Token::Whitespace,
                slice: &space.slice[1..],
                span: space.span.start + 1..space.span.end,
            });
        }
    }

    (paragraph_breaks, indents)
}

impl<'t> From<Tokenization<'t>> for Vec<ExtractedToken<'t>> {
    #[inline]
    fn from(tokenization: Tokenization<'t>) -> Vec<ExtractedToken<'t>> {
        tokenization.tokens
    }
}

/// Take an input string and produce a list of tokens for consumption by the parser.
pub fn tokenize(text: &str) -> Tokenization<'_> {
    info!(
        "Running lexer on text ({} bytes) to produce tokens",
        text.len(),
    );

    let tokens = Token::extract_all(text);
    let full_text = FullText::new(text);

    let (quote_paragraph_breaks, quote_indents) = build_quote_tokens(&tokens);

    Tokenization {
        tokens,
        full_text,
        quote_paragraph_breaks,
        quote_indents,
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1024))]

        #[test]
        fn tokenizer_prop(s in ".*") {
            let _ = tokenize(&s);
        }
    }
}
