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

use crate::delayed::{GeneratedInput, GeneratedKind, InputSegment, TextOrigin};
use crate::parsing::{ExtractedToken, Token};
use crate::text::FullText;
use std::collections::BTreeMap;
use std::ops::Range;

/// Struct that represents both a list of tokens and the text the tokens were generated from.
#[derive(Debug, Clone)]
pub struct Tokenization<'t> {
    tokens: Vec<ExtractedToken<'t>>,
    full_text: FullText<'t>,
    delayed_markers: BTreeMap<usize, DelayedMarker>,
}

#[derive(Debug, Clone)]
pub(crate) enum DelayedMarker {
    Generated(GeneratedInput),
    RuntimeLiteral(Range<usize>),
    RuntimeScalar(Range<usize>),
}

impl DelayedMarker {
    pub(crate) fn generated(&self) -> Option<&GeneratedInput> {
        match self {
            Self::Generated(generated) => Some(generated),
            Self::RuntimeLiteral(_) | Self::RuntimeScalar(_) => None,
        }
    }

    pub(crate) fn runtime_literal(&self) -> Option<&Range<usize>> {
        match self {
            Self::Generated(_) | Self::RuntimeScalar(_) => None,
            Self::RuntimeLiteral(range) => Some(range),
        }
    }

    pub(crate) fn runtime_scalar(&self) -> Option<&Range<usize>> {
        match self {
            Self::RuntimeScalar(range) => Some(range),
            Self::Generated(_) | Self::RuntimeLiteral(_) => None,
        }
    }
}

#[cfg(not(tarpaulin))]
impl<'t> Tokenization<'t> {
    #[inline]
    pub fn tokens<'r>(&'r self) -> &'r [ExtractedToken<'t>] {
        &self.tokens
    }

    pub(crate) fn with_tokens(&self, tokens: Vec<ExtractedToken<'t>>) -> Self {
        Tokenization {
            tokens,
            full_text: self.full_text,
            delayed_markers: self.delayed_markers.clone(),
        }
    }

    #[inline]
    pub(crate) fn full_text(&self) -> FullText<'t> {
        self.full_text
    }

    #[inline]
    pub(crate) fn delayed_markers(&self) -> &BTreeMap<usize, DelayedMarker> {
        &self.delayed_markers
    }

    #[inline]
    pub(crate) fn has_generated(&self) -> bool {
        self.delayed_markers
            .values()
            .any(|marker| marker.generated().is_some())
    }
}

// Tarpaulin maps the generic impl header as executable unless the first method
// starts on the same line.
#[cfg(tarpaulin)]
#[rustfmt::skip]
impl<'t> Tokenization<'t> { pub fn tokens<'r>(&'r self) -> &'r [ExtractedToken<'t>] { &self.tokens }
    pub(crate) fn full_text(&self) -> FullText<'t> { self.full_text }
    pub(crate) fn delayed_markers(&self) -> &BTreeMap<usize, DelayedMarker> { &self.delayed_markers }
    pub(crate) fn has_generated(&self) -> bool { self.delayed_markers.values().any(|marker| marker.generated().is_some()) }
}

impl<'t> From<Tokenization<'t>> for Vec<ExtractedToken<'t>> {
    #[inline]
    fn from(tokenization: Tokenization<'t>) -> Vec<ExtractedToken<'t>> {
        tokenization.tokens
    }
}

/// Take an input string and produce a list of tokens for consumption by the parser.
#[track_caller]
pub fn tokenize(text: &str) -> Tokenization<'_> {
    #[cfg(feature = "test-source-recorder")]
    crate::source_recorder::record("tokenize", text, std::panic::Location::caller());

    debug!(
        "Running lexer on text ({} bytes) to produce tokens",
        text.len(),
    );

    let tokens = Token::extract_all(text);
    let full_text = FullText::new(text);

    Tokenization {
        tokens,
        full_text,
        delayed_markers: BTreeMap::new(),
    }
}

pub(crate) fn tokenize_delayed_segments<'t>(
    text: &'t str,
    segments: &[InputSegment],
) -> Tokenization<'t> {
    let mut tokens = vec![ExtractedToken {
        token: Token::InputStart,
        slice: &text[..0],
        span: 0..0,
    }];

    let mut delayed_markers = BTreeMap::new();
    let trailing_authored_comment_closers =
        trailing_authored_comment_closers(text, segments);
    let mut segment_index = 0;
    while let Some(segment) = segments.get(segment_index) {
        match segment {
            InputSegment::Text {
                source_range,
                origin,
            } => {
                let start = source_range.start;
                let mut end = source_range.end;
                while let Some(InputSegment::Text {
                    source_range: next_range,
                    origin: next_origin,
                }) = segments.get(segment_index + 1)
                {
                    if next_origin != origin {
                        break;
                    }
                    debug_assert_eq!(end, next_range.start);
                    end = next_range.end;
                    segment_index += 1;
                }
                let segment_text = &text[start..end];
                match origin {
                    TextOrigin::Authored | TextOrigin::RuntimeLiteral => {
                        if *origin == TextOrigin::RuntimeLiteral {
                            delayed_markers
                                .insert(start, DelayedMarker::RuntimeLiteral(start..end));
                        }
                        let trailing_comment_closer =
                            trailing_authored_comment_closers[segment_index];
                        tokens.extend(
                            Token::extract_all_with_trailing_comment_closer(
                                segment_text,
                                trailing_comment_closer,
                            )
                            .into_iter()
                            .filter(|token| {
                                !matches!(
                                    token.token,
                                    Token::InputStart | Token::InputEnd
                                )
                            })
                            .map(|mut token| {
                                token.span.start += start;
                                token.span.end += start;
                                token
                            }),
                        );
                    }
                    TextOrigin::RuntimeScalar if !segment_text.is_empty() => {
                        delayed_markers
                            .insert(start, DelayedMarker::RuntimeScalar(start..end));
                        tokens.push(ExtractedToken {
                            token: Token::RuntimeText,
                            slice: segment_text,
                            span: start..end,
                        });
                    }
                    TextOrigin::RuntimeScalar => {}
                }
            }
            InputSegment::Generated(generated) => {
                let start = generated.source_range.start;
                delayed_markers
                    .insert(start, DelayedMarker::Generated(generated.clone()));
                tokens.push(ExtractedToken {
                    token: match generated.kind {
                        GeneratedKind::PageLink => Token::GeneratedPageLink,
                        GeneratedKind::TagLinks => Token::GeneratedTagLinks,
                    },
                    slice: &text[start..start],
                    span: start..start,
                });
            }
        }
        segment_index += 1;
    }
    tokens.push(ExtractedToken {
        token: Token::InputEnd,
        slice: &text[text.len()..],
        span: text.len()..text.len(),
    });
    Tokenization {
        tokens,
        full_text: FullText::new(text),
        delayed_markers,
    }
}

fn trailing_authored_comment_closers(text: &str, segments: &[InputSegment]) -> Vec<bool> {
    // Authored and runtime-literal comments may span delayed slots, but
    // runtime scalars and generated bytes cannot create a closer. Record only
    // later source-grammar boundaries.
    let mut trailing = vec![false; segments.len()];
    let mut found = false;

    for (index, segment) in segments.iter().enumerate().rev() {
        trailing[index] = found;
        if let InputSegment::Text {
            source_range,
            origin,
        } = segment
            && matches!(origin, TextOrigin::Authored | TextOrigin::RuntimeLiteral)
        {
            found |= text[source_range.clone()]
                .as_bytes()
                .windows(3)
                .any(|window| window == b"--]");
        }
    }

    trailing
}

#[cfg(test)]
mod test {
    use super::*;
    use proptest::prelude::*;
    use std::sync::mpsc;
    use std::time::Duration;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1024))]

        #[test]
        fn tokenizer_prop(s in ".*") {
            let _ = tokenize(&s);
        }
    }

    #[test]
    fn tokenizer_handles_long_punctuation_runs_without_email_scan_blowup() {
        let input = "%".repeat(16_384);
        let expected_tokens = input.len() + 2;
        let (sender, receiver) = mpsc::channel();

        std::thread::spawn(move || {
            let tokenization = tokenize(&input);
            let _ = sender.send(tokenization.tokens().len());
        });

        let token_count = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("tokenizing a punctuation-only run should stay bounded");

        assert_eq!(token_count, expected_tokens);
    }

    #[test]
    fn email_scan_does_not_swallow_a_wikidot_block_closer() {
        // Corpus provenance: scp-wiki/scp-9945.
        let input = "[[span]]name[[/span]]19@scip.net";
        let tokenization = tokenize(input);

        assert!(
            tokenization
                .tokens()
                .iter()
                .any(|token| token.token == Token::Email && token.slice == "19@scip.net"),
            "{:#?}",
            tokenization.tokens(),
        );
        assert!(
            tokenization
                .tokens()
                .iter()
                .all(|token| token.slice != "span]]19@scip.net"),
            "{:#?}",
            tokenization.tokens(),
        );
    }

    #[test]
    fn discarded_control_is_an_invisible_email_barrier() {
        let input = "name@\u{0006}site.com";
        let tokenization = tokenize(input);

        assert!(
            tokenization
                .tokens()
                .iter()
                .any(|token| token.token == Token::DiscardedControl),
            "{:#?}",
            tokenization.tokens(),
        );
        assert!(
            tokenization
                .tokens()
                .iter()
                .all(|token| token.token != Token::Email),
            "{:#?}",
            tokenization.tokens(),
        );
    }
}
