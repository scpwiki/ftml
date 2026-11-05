/*
 * parsing/parser/closer_scan.rs
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

//! Monotone "no closer ahead" memos for parser rules that scan forward from an
//! inline opener to its closing token.
//!
//! Two rules do the same repeated work: `inline_delimiter::validate_reachable_close`
//! (called once per `{{` / `**` / ... opener) and the Wikidot-layout
//! `math::collect_wikidot_math_source` (called once per `[[$` opener). When the
//! closer is missing, each walks every remaining token to `InputEnd`; with `n`
//! unmatched openers that is `O(n)` per opener, so `O(n^2)` in total.
//!
//! Both walks prove the same monotone fact: from a given token start index no
//! closer of that class occurs before `InputEnd`. The token stream is
//! immutable for a whole parse, so the walks that start later see a suffix of
//! the same tokens: once a walk from index `p` has reached `InputEnd`, every
//! walk that starts at `q >= p` reaches it too. The memo stores the smallest
//! proven `p` per closer class, so every later opener is answered in `O(1)`.
//!
//! The memo is written only by walks that provably reproduce the original
//! error, and each recorded field carries the extra state needed to rebuild
//! that error exactly:
//!
//! - `validate_reachable_close` returns `RuleFailed` when it saw a
//!   `ParagraphBreak` and `EndOfInput` otherwise; the memo keeps the last
//!   `ParagraphBreak` index so a later start can still tell the two apart.
//!   Block bookkeeping (`LeftBlock` / `LeftBlockEnd` / `LeftLink`) consults
//!   parser context through `end_block_fails`, which is not a property of the
//!   token stream, so a walk that touched any of those tokens is never
//!   recorded (and a split-delimiter scan consults context too, so those are
//!   never recorded either).
//! - `collect_wikidot_math_source` returns `EndOfInput` only when it reaches
//!   `InputEnd` by plain single-token steps; a walk that consumed a comment or
//!   a raw region jumps over tokens, so it is never recorded.
//!
//! Because a recorded start only ever answers queries at or after itself, and
//! the suffix of a suffix is a suffix, no later query needs parser state: the
//! walk it replaces would have visited exactly the same tokens it already did.

use crate::parsing::Token;

/// The last scan for one inline delimiter closer that reached `InputEnd`
/// without finding it.
#[derive(Debug, Clone, Copy)]
struct DelimiterAbsence {
    closer: Token,
    /// Smallest split-free scan start index proven to have no closer at or
    /// after it.
    start: usize,
    /// Absolute index of the last `ParagraphBreak` at or after `start`, if
    /// any: such scans return `RuleFailed` instead of `EndOfInput`.
    last_paragraph_break: Option<usize>,
}

/// Per-parse memo of "no closer of this class at or after this token index".
#[derive(Debug, Default)]
pub(crate) struct CloserScanCache {
    /// Number of tokens in the parse's tokenization, used to turn a
    /// `remaining()` length into an absolute token index.
    token_count: usize,
    delimiter_absences: Vec<DelimiterAbsence>,
    /// Smallest index from which the Wikidot math scan reaches `InputEnd`.
    math_absence: Option<usize>,
}

impl CloserScanCache {
    pub(crate) fn new(token_count: usize) -> Self {
        Self {
            token_count,
            delimiter_absences: Vec::new(),
            math_absence: None,
        }
    }

    /// Absolute token index of the parser's current token.
    pub(crate) fn index_of_current(&self, remaining_len: usize) -> usize {
        self.token_count - remaining_len - 1
    }

    /// Absolute token index of the first token after the current one, which is
    /// where a forward closer scan starts.
    pub(crate) fn index_ahead(&self, remaining_len: usize) -> usize {
        self.token_count - remaining_len
    }

    /// `Some(has_paragraph_break)` when a split-free scan for `closer`
    /// starting at `start` provably walks to `InputEnd`.
    pub(crate) fn delimiter_absence(&self, closer: Token, start: usize) -> Option<bool> {
        self.delimiter_absences
            .iter()
            .find(|absence| absence.closer == closer && start >= absence.start)
            .map(|absence| {
                absence
                    .last_paragraph_break
                    .is_some_and(|paragraph_break| start <= paragraph_break)
            })
    }

    pub(crate) fn record_delimiter_absence(
        &mut self,
        closer: Token,
        start: usize,
        last_paragraph_break: Option<usize>,
    ) {
        match self
            .delimiter_absences
            .iter_mut()
            .find(|absence| absence.closer == closer)
        {
            Some(absence) if start < absence.start => {
                absence.start = start;
                absence.last_paragraph_break = last_paragraph_break;
            }
            Some(_) => {}
            None => self.delimiter_absences.push(DelimiterAbsence {
                closer,
                start,
                last_paragraph_break,
            }),
        }
    }

    /// Whether a Wikidot math scan starting at `start` provably reaches
    /// `InputEnd` without a `RightMath` closer.
    pub(crate) fn math_absence(&self, start: usize) -> bool {
        self.math_absence.is_some_and(|absence| start >= absence)
    }

    pub(crate) fn record_math_absence(&mut self, start: usize) {
        match self.math_absence {
            Some(absence) if absence <= start => {}
            _ => self.math_absence = Some(start),
        }
    }
}

/// Count one token visited by a forward closer-scan walk.
///
/// Test builds count the work these scans do so a regression test can assert it
/// stays linear in the token count instead of relying on wall-clock timing;
/// release builds have no counter to update.
#[cfg(test)]
pub(crate) fn count_scan_token_visit() {
    SCAN_TOKEN_VISITS.with(|visits| visits.set(visits.get() + 1));
}

#[cfg(not(test))]
#[inline(always)]
pub(crate) fn count_scan_token_visit() {}

#[cfg(test)]
thread_local! {
    static SCAN_TOKEN_VISITS: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
}

#[cfg(test)]
pub(crate) fn reset_scan_token_visits() {
    SCAN_TOKEN_VISITS.with(|visits| visits.set(0));
}

#[cfg(test)]
pub(crate) fn scan_token_visits() -> usize {
    SCAN_TOKEN_VISITS.with(|visits| visits.get())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_indices_are_absolute() {
        let cache = CloserScanCache::new(10);
        assert_eq!(cache.index_of_current(3), 6);
        assert_eq!(cache.index_ahead(3), 7);
        assert_eq!(cache.index_of_current(0), 9);
    }

    #[test]
    fn delimiter_absence_only_answers_later_starts() {
        let mut cache = CloserScanCache::new(100);
        assert_eq!(cache.delimiter_absence(Token::RightMonospace, 10), None);

        cache.record_delimiter_absence(Token::RightMonospace, 10, Some(20));
        assert_eq!(
            cache.delimiter_absence(Token::RightMonospace, 10),
            Some(true)
        );
        assert_eq!(
            cache.delimiter_absence(Token::RightMonospace, 15),
            Some(true)
        );
        assert_eq!(
            cache.delimiter_absence(Token::RightMonospace, 21),
            Some(false)
        );
        assert_eq!(cache.delimiter_absence(Token::RightMonospace, 9), None);
        // A different closer class is a different fact.
        assert_eq!(cache.delimiter_absence(Token::Bold, 10), None);

        // An earlier proven start subsumes the recorded one.
        cache.record_delimiter_absence(Token::RightMonospace, 5, None);
        assert_eq!(
            cache.delimiter_absence(Token::RightMonospace, 6),
            Some(false)
        );
        assert_eq!(cache.delimiter_absence(Token::RightMonospace, 0), None);
    }

    #[test]
    fn delimiter_absence_keeps_the_smallest_start() {
        let mut cache = CloserScanCache::new(100);
        cache.record_delimiter_absence(Token::Bold, 30, Some(40));
        cache.record_delimiter_absence(Token::Bold, 35, Some(90));
        assert_eq!(cache.delimiter_absence(Token::Bold, 31), Some(true));
        assert_eq!(cache.delimiter_absence(Token::Bold, 41), Some(false));
    }

    #[test]
    fn math_absence_only_answers_later_starts() {
        let mut cache = CloserScanCache::new(100);
        assert!(!cache.math_absence(10));

        cache.record_math_absence(10);
        assert!(cache.math_absence(10));
        assert!(cache.math_absence(11));
        assert!(!cache.math_absence(9));

        cache.record_math_absence(20);
        assert!(cache.math_absence(10));
    }

    fn closer_scan_visits(input: &str) -> (usize, usize) {
        use crate::data::PageInfo;
        use crate::layout::Layout;
        use crate::settings::{WikitextMode, WikitextSettings};

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut text = input.to_owned();
        crate::preprocess_for_layout(&mut text, Layout::Wikidot);
        let tokenization = crate::tokenize(&text);
        reset_scan_token_visits();
        let _ = crate::parsing::parse_for_test(&tokenization, &page_info, &settings);
        (scan_token_visits(), tokenization.tokens().len())
    }

    /// Every unmatched opener used to walk the remaining tokens to `InputEnd`,
    /// once per opener: quadratic. The memo answers every opener after the
    /// first in `O(1)`, so the total walk stays linear in the token count.
    #[test]
    fn unmatched_inline_openers_scan_the_document_once() {
        for (name, unit) in [("monospace", "{"), ("inline math", "[[$ a ")] {
            for count in [512usize, 1024, 2048] {
                let input = unit.repeat(count);
                let (visits, tokens) = closer_scan_visits(&input);
                assert!(
                    visits <= tokens * 2,
                    "{name} x {count}: {visits} closer-scan token visits for {tokens} tokens",
                );
            }
        }
    }

    /// A closed form must still take the real walk and must not be memoized as
    /// "absent" for later openers.
    #[test]
    fn closed_openers_still_scan_their_closer() {
        for input in ["{{x}}", "[[$ x $]]", "a {{b}} c {{d}} e"] {
            let (visits, tokens) = closer_scan_visits(input);
            assert!(visits <= tokens * 2, "{input:?}: {visits} visits");
        }
    }
}
