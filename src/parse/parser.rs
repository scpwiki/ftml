/*
 * parse/parser.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2020 Ammon Smith
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

use super::condition::ParseCondition;
use super::prelude::*;
use super::rule::Rule;
use super::upcoming::UpcomingTokens;
use super::RULE_PAGE;
use crate::tokenize::Tokenization;

#[derive(Debug, Clone)]
pub struct Parser<'l, 'r, 't> {
    log: &'l slog::Logger,
    current: &'r ExtractedToken<'t>,
    remaining: &'r [ExtractedToken<'t>],
    full_text: FullText<'t>,
    rule: Rule,
}

impl<'l, 'r, 't> Parser<'l, 'r, 't> {
    /// Constructor. Should only be created by `parse()`.
    ///
    /// All other instances should be `.clone()` or `.clone_with_rule()`d from
    /// the main instance used during parsing.
    pub(crate) fn new(log: &'l slog::Logger, tokenization: &'r Tokenization<'t>) -> Self {
        let full_text = tokenization.full_text();
        let (current, remaining) = tokenization
            .tokens()
            .split_first()
            .expect("Parsed tokens list was empty (expected at least one element)");

        Parser {
            log,
            current,
            remaining,
            full_text,
            rule: RULE_PAGE,
        }
    }

    // Getters
    #[inline]
    pub fn log(&self) -> &'l slog::Logger {
        self.log
    }

    #[inline]
    pub fn upcoming(&self) -> UpcomingTokens<'r, 't> {
        UpcomingTokens::Split {
            current: self.current,
            remaining: self.remaining,
        }
    }

    #[inline]
    pub fn full_text(&self) -> FullText<'t> {
        self.full_text
    }

    // Setters
    pub fn set_rule(&mut self, rule: Rule) {
        self.rule = rule;
    }

    pub fn clone_with_rule(&self, rule: Rule) -> Self {
        let mut clone = self.clone();
        clone.set_rule(rule);
        clone
    }

    // State evaluation
    pub fn evaluate(&self, condition: ParseCondition) -> bool {
        match condition {
            ParseCondition::CurrentToken { token } => self.current.token == token,
            ParseCondition::Function { f } => self.evaluate_fn(f),
            ParseCondition::TokenPair { current, next } => {
                self.evaluate_fn(|mut parser| {
                    macro_rules! check {
                        ($expected:expr) => {
                            if parser.current().token != $expected {
                                return Ok(false);
                            }
                        };
                    }

                    check!(current);
                    parser.step()?;
                    check!(next);

                    Ok(false)
                })
            }
        }
    }

    #[inline]
    pub fn evaluate_any(&self, conditions: &[ParseCondition]) -> bool {
        conditions.iter().any(|&condition| self.evaluate(condition))
    }

    #[inline]
    pub fn evaluate_fn<F>(&self, f: F) -> bool
    where
        F: FnOnce(Parser<'l, 'r, 't>) -> Result<bool, ParseError>,
    {
        f(self.clone()).unwrap_or(false)
    }

    // Token pointer state and manipulation
    #[inline]
    pub fn current(&self) -> &'r ExtractedToken<'t> {
        self.current
    }

    #[inline]
    pub fn remaining(&self) -> &'r [ExtractedToken<'t>] {
        self.remaining
    }

    #[inline]
    pub fn step(&mut self) -> Result<(), ParseError> {
        debug!(self.log, "Stepping to the next token");

        match self.remaining.split_first() {
            Some((current, remaining)) => {
                self.current = current;
                self.remaining = remaining;
                Ok(())
            }

            #[cold]
            None => Err(self.make_error(ParseErrorKind::EndOfInput)),
        }
    }

    #[inline]
    pub fn update_remaining(&mut self, remaining: &'r [ExtractedToken<'t>]) {
        self.remaining = remaining;
    }

    // Utilities
    #[cold]
    #[inline]
    pub fn make_error(&self, kind: ParseErrorKind) -> ParseError {
        ParseError::new(kind, self.rule, self.current)
    }
}
