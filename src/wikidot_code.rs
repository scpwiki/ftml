/*
 * wikidot_code.rs
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

use std::collections::BTreeMap;
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WikidotCodeCandidate {
    pub(crate) body: Range<usize>,
    pub(crate) owner_end: usize,
    pub(crate) end_blocks_to_skip: usize,
}

pub(crate) fn candidate_at(
    source: &str,
    opener_start: usize,
) -> Option<WikidotCodeCandidate> {
    candidates(source).remove(&opener_start)
}

pub(crate) fn candidate_at_owned_opener(
    source: &str,
    opener_start: usize,
    allow_inner: bool,
) -> Option<WikidotCodeCandidate> {
    if has_physical_line_ownership(source, opener_start) {
        return candidate_at(source, opener_start);
    }
    if allow_inner {
        return candidate_at_inner(source, opener_start, false);
    }
    None
}

fn candidate_at_inner(
    source: &str,
    opener_start: usize,
    require_line_owner: bool,
) -> Option<WikidotCodeCandidate> {
    let mut ownership = LineOwnership::new();
    let mut close_brackets = CloseBracketCursor::new(source);
    let body_start = code_opener_end_checked(
        source,
        opener_start,
        require_line_owner.then(|| ownership.at(source, opener_start)),
        &mut close_brackets,
    )?;
    let mut cursor = body_start;
    let mut nested_depth = 0usize;
    let mut end_blocks_to_skip = 0usize;

    while let Some(relative_marker) = source[cursor..].find("[[") {
        let marker_start = cursor + relative_marker;
        if let Some(marker_end) =
            code_closer_end(source, marker_start, &mut close_brackets)
        {
            if nested_depth > 0 {
                nested_depth -= 1;
                end_blocks_to_skip += 1;
                cursor = marker_end;
                continue;
            }

            let body = body_start..marker_start;
            if crosses_bold_closer(&source[body.clone()], &source[marker_end..]) {
                return None;
            }
            return Some(WikidotCodeCandidate {
                body,
                owner_end: marker_end,
                end_blocks_to_skip,
            });
        }

        if let Some(nested_body_start) = code_opener_end_checked(
            source,
            marker_start,
            Some(ownership.at(source, marker_start)),
            &mut close_brackets,
        ) {
            nested_depth += 1;
            cursor = nested_body_start;
            continue;
        }
        cursor = marker_start + 2;
    }

    None
}

pub(crate) fn candidates(source: &str) -> BTreeMap<usize, WikidotCodeCandidate> {
    #[derive(Debug)]
    struct OpenCode {
        opener_start: usize,
        body_start: usize,
        nested_closes: usize,
        fallback_closer: Option<(usize, usize)>,
    }

    let mut output = BTreeMap::new();
    let mut stack = Vec::<OpenCode>::new();
    let mut ownership = LineOwnership::new();
    let mut close_brackets = CloseBracketCursor::new(source);
    let mut cursor = 0usize;
    while let Some(relative_marker) = source[cursor..].find("[[") {
        let marker_start = cursor + relative_marker;
        if let Some(marker_end) =
            code_closer_marker_end(source, marker_start, &mut close_brackets)
        {
            if code_closer_terminates(source, marker_end) {
                if let Some(open) = stack.pop() {
                    let candidate = WikidotCodeCandidate {
                        body: open.body_start..marker_start,
                        owner_end: marker_end,
                        end_blocks_to_skip: open.nested_closes,
                    };
                    if !crosses_bold_closer(
                        &source[candidate.body.clone()],
                        &source[marker_end..],
                    ) {
                        output.insert(open.opener_start, candidate);
                    }
                    if let Some(parent) = stack.last_mut() {
                        parent.nested_closes += 1 + open.nested_closes;
                        parent.fallback_closer = Some((marker_start, marker_end));
                    }
                }
            }
            // A `[[/code]]` not followed by whitespace or the end of the
            // source is body text; scanning resumes after it and later
            // openers still nest.
            cursor = marker_end;
            continue;
        }

        if let Some(body_start) = code_opener_end_checked(
            source,
            marker_start,
            Some(ownership.at(source, marker_start)),
            &mut close_brackets,
        ) {
            stack.push(OpenCode {
                opener_start: marker_start,
                body_start,
                nested_closes: 0,
                fallback_closer: None,
            });
            // The opener head is one lexical owner. Do not rescan `[[`
            // lookalikes inside it as independent block markers; otherwise
            // a malformed head can observe a closer before its body starts.
            cursor = body_start;
            continue;
        }
        cursor = marker_start + 2;
    }

    while let Some(open) = stack.pop() {
        let Some((closer_start, closer_end)) = open.fallback_closer else {
            continue;
        };
        let candidate = WikidotCodeCandidate {
            body: open.body_start..closer_start,
            owner_end: closer_end,
            end_blocks_to_skip: open.nested_closes - 1,
        };
        if !crosses_bold_closer(&source[candidate.body.clone()], &source[closer_end..]) {
            output.insert(open.opener_start, candidate);
        }
        if let Some(parent) = stack.last_mut() {
            parent.nested_closes += open.nested_closes;
            parent.fallback_closer = Some((closer_start, closer_end));
        }
    }

    output
}

pub(crate) fn active_ranges(source: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let candidates = candidates(source);
    let mut cursor = 0usize;

    while let Some(relative_marker) = source[cursor..].find("[[") {
        let opener_start = cursor + relative_marker;
        if let Some(candidate) = candidates.get(&opener_start) {
            ranges.push(opener_start..candidate.owner_end);
            cursor = candidate.owner_end;
        } else {
            cursor = opener_start + 2;
        }
    }

    ranges
}

pub(crate) fn active_body_ranges(source: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let candidates = candidates(source);
    let mut cursor = 0usize;

    while let Some(relative_marker) = source[cursor..].find("[[") {
        let opener_start = cursor + relative_marker;
        if let Some(candidate) = candidates.get(&opener_start) {
            ranges.push(candidate.body.clone());
            cursor = candidate.owner_end;
        } else {
            cursor = opener_start + 2;
        }
    }

    ranges
}

/// Monotone memo for "the next `]]` at or after this byte".
///
/// Every candidate head and closer head in a source asks for the first close
/// marker ahead of it. Answering each ask with `str::find` rescans the whole
/// tail, so a page full of `[[` fragments without any `]]` (for example
/// `[[/ a ` or `[[/math` repeated) cost O(n²) in preprocessing and again in
/// the parse. The asks in each pass only move forward, so one shared cursor
/// answers them in linear total work while recomputing the identical answer.
#[derive(Debug)]
struct CloseBracketCursor<'a> {
    source: &'a str,
    scanned_from: usize,
    found: Option<usize>,
}

impl<'a> CloseBracketCursor<'a> {
    fn new(source: &'a str) -> Self {
        CloseBracketCursor {
            source,
            scanned_from: 0,
            found: None,
        }
    }

    /// The first `]]` at or after `position`, or `None`.
    ///
    /// `found` is the first close marker at or after `scanned_from`,
    /// and a scanned tail with no close marker stays empty for every later
    /// position, so both a found answer and an empty one are reusable forward.
    /// Rescanning the empty answer is what made a page of `[[/name`
    /// fragments quadratic in preprocessing and again in the parse.
    fn next_at(&mut self, position: usize) -> Option<usize> {
        if position < self.scanned_from {
            // A backwards ask cannot reuse the forward cursor; answer it
            // directly and leave the cursor untouched.
            return self
                .source
                .get(position..)?
                .find("]]")
                .map(|offset| position + offset);
        }
        if let Some(found) = self.found {
            if found >= position {
                return Some(found);
            }
        } else if self.scanned_from != 0 {
            // The scanned tail had no close marker at all, so neither does
            // a later position in it.
            return None;
        }
        let found = self
            .source
            .get(position..)?
            .find("]]")
            .map(|offset| position + offset);
        self.scanned_from = position;
        self.found = found;
        found
    }
}

fn code_opener_end_checked(
    source: &str,
    opener_start: usize,
    line_ownership: Option<bool>,
    close_brackets: &mut CloseBracketCursor<'_>,
) -> Option<usize> {
    let suffix = source.get(opener_start..)?;
    let head_start = opener_start + "[[".len();
    if !suffix.starts_with("[[") || line_ownership == Some(false) {
        return None;
    }

    let relative_end = close_brackets.next_at(head_start)? - head_start;
    let head_end = head_start + relative_end;
    let head = &source[head_start..head_end];
    if head_has_malformed_nested_marker(head) {
        return None;
    }
    let name_end = head
        .find(|character: char| character.is_ascii_whitespace())
        .unwrap_or(head.len());
    let name = &head[..name_end];

    name.eq_ignore_ascii_case("code").then_some(head_end + 2)
}

fn head_has_malformed_nested_marker(head: &str) -> bool {
    // The head ends at the first close marker, so a nested opener inside
    // it is an unclosed fragment. An iftags segment opener fragment is
    // live behavior: Wikidot ends the head at the segment's own close
    // marker and treats the rest as body, so the candidate stands. Any
    // other fragment (a closer, or another block's opener) leaves the
    // head malformed, since it could let the head steal a closer that
    // belongs to a later block.
    let mut rest = head;
    while let Some(index) = rest.find("[[") {
        let fragment = &rest[index + 2..];
        let name_end = fragment
            .find(|character: char| character.is_ascii_whitespace())
            .unwrap_or(fragment.len());
        if !fragment[..name_end].eq_ignore_ascii_case("iftags") {
            return true;
        }
        rest = &fragment[name_end..];
    }
    false
}

fn code_closer_end(
    source: &str,
    closer_start: usize,
    close_brackets: &mut CloseBracketCursor<'_>,
) -> Option<usize> {
    let closer_end = code_closer_marker_end(source, closer_start, close_brackets)?;
    code_closer_terminates(source, closer_end).then_some(closer_end)
}

fn code_closer_marker_end(
    source: &str,
    closer_start: usize,
    close_brackets: &mut CloseBracketCursor<'_>,
) -> Option<usize> {
    let suffix = source.get(closer_start..)?;
    if !suffix.starts_with("[[/") {
        return None;
    }
    let head_start = closer_start + "[[/".len();
    let relative_end = close_brackets.next_at(head_start)? - head_start;
    let head_end = head_start + relative_end;
    let name = &source[head_start..head_end];

    if !name.eq_ignore_ascii_case("code") {
        return None;
    }
    Some(head_end + 2)
}

fn code_closer_terminates(source: &str, closer_end: usize) -> bool {
    // Live Wikidot closes a code block at the first `[[/code]]` followed by
    // whitespace or the end of the source; whatever follows the whitespace
    // is outer content (scp-9844's `[[/code]] ` blocks render, and a closer
    // followed by ` x` closes the same way). A closer followed directly by
    // any other byte (a letter, `[[`, `]`) is body text instead.
    match source[closer_end..].bytes().next() {
        None => true,
        Some(byte) => matches!(byte, b' ' | b'\t' | b'\r' | b'\n'),
    }
}

#[cfg(test)]
thread_local! {
    static LINE_OWNERSHIP_SCANNED_BYTES: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

struct LineOwnership {
    scanned_upto: usize,
    clean: bool,
}

impl LineOwnership {
    fn new() -> Self {
        LineOwnership {
            scanned_upto: 0,
            clean: true,
        }
    }

    fn at(&mut self, source: &str, position: usize) -> bool {
        #[cfg(test)]
        LINE_OWNERSHIP_SCANNED_BYTES
            .with(|scanned| scanned.set(scanned.get() + position - self.scanned_upto));
        for byte in &source.as_bytes()[self.scanned_upto..position] {
            match byte {
                b'\n' => self.clean = true,
                b' ' | b'\t' | b'\0' => {}
                _ => self.clean = false,
            }
        }
        self.scanned_upto = position;
        self.clean
    }
}

fn has_physical_line_ownership(source: &str, opener_start: usize) -> bool {
    let line_start = source[..opener_start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    source[line_start..opener_start]
        .bytes()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\0'))
}

fn crosses_bold_closer(body: &str, suffix: &str) -> bool {
    let Some((closing, name, _)) = marker_at(suffix, 0) else {
        return false;
    };
    if !closing || !name.eq_ignore_ascii_case("bold") {
        return false;
    }

    let mut depth = 0usize;
    let mut cursor = 0usize;
    while let Some(relative_marker) = body[cursor..].find("[[") {
        let marker_start = cursor + relative_marker;
        let Some((closing, name, marker_end)) = marker_at(body, marker_start) else {
            cursor = marker_start + 2;
            continue;
        };
        cursor = marker_end;
        if !name.eq_ignore_ascii_case("bold") {
            continue;
        }
        if closing {
            depth = depth.saturating_sub(1);
        } else {
            depth += 1;
        }
    }
    depth > 0
}

fn marker_at(source: &str, marker_start: usize) -> Option<(bool, &str, usize)> {
    let suffix = source.get(marker_start..)?;
    if !suffix.starts_with("[[") {
        return None;
    }
    let closing = suffix.starts_with("[[/");
    let head_start = marker_start + if closing { "[[/".len() } else { "[[".len() };
    let relative_end = source[head_start..].find("]]")?;
    let head_end = head_start + relative_end;
    let head = source[head_start..head_end]
        .trim_matches(|character: char| character.is_ascii_whitespace());
    let name_end = head
        .find(|character: char| character.is_ascii_whitespace())
        .unwrap_or(head.len());
    let name = &head[..name_end];
    (!name.is_empty()).then_some((closing, name, head_end + 2))
}

#[cfg(test)]
mod tests {
    #[test]
    fn candidates_line_ownership_scans_source_once() {
        let source = "[[span]]".repeat(512);
        LINE_OWNERSHIP_SCANNED_BYTES.with(|scanned| scanned.set(0));
        let found = candidates(&source);
        assert!(found.is_empty());
        let scanned = LINE_OWNERSHIP_SCANNED_BYTES.with(|scanned| scanned.get());
        assert!(
            scanned <= source.len(),
            "scanned {scanned} bytes for {} source bytes",
            source.len(),
        );
    }

    use super::*;

    #[test]
    fn candidate_tracks_nested_closer_ownership() {
        let source = "[[code]]\n[[code]]\nnested\n[[/code]]\n[[/code]]";
        let candidate = candidate_at(source, 0).expect("outer code candidate");

        assert_eq!(candidate.body, 8..35);
        assert_eq!(candidate.owner_end, source.len());
        assert_eq!(candidate.end_blocks_to_skip, 1);
    }

    #[test]
    fn candidate_rejects_unclosed_prefixed_and_crossed_blocks() {
        for source in [
            "[[code]]\nunclosed",
            "prefix[[code]]\nbody\n[[/code]]",
            "[[code]]\nouter [[bold]]inner\n[[/code]][[/bold]]",
        ] {
            let opener = source.find("[[code]]").unwrap();
            assert!(candidate_at(source, opener).is_none(), "{source:?}");
        }
    }

    #[test]
    fn candidate_pairs_outer_opener_with_the_last_jumped_closer() {
        let source = "[[code]]\n[[code]]\ninner\n[[/code]]\nafter\n[[/code ]]";
        let candidate = candidate_at(source, 0).expect("outer code candidate");
        let closer = source.find("[[/code]]").unwrap();

        assert_eq!(&source[candidate.body.clone()], "\n[[code]]\ninner\n");
        assert_eq!(candidate.owner_end, closer + "[[/code]]".len());
        assert_eq!(candidate.end_blocks_to_skip, 0);
    }

    #[test]
    fn candidate_fallback_skips_closers_of_earlier_jumped_blocks() {
        let source = "[[code]]\n[[code]]\na\n[[/code]]\n[[code]]\nb\n[[/code]]";
        let candidate = candidate_at(source, 0).expect("outer code candidate");
        let closer = source.rfind("[[/code]]").unwrap();

        assert_eq!(
            &source[candidate.body.clone()],
            "\n[[code]]\na\n[[/code]]\n[[code]]\nb\n",
        );
        assert_eq!(candidate.owner_end, closer + "[[/code]]".len());
        assert_eq!(candidate.end_blocks_to_skip, 1);
    }

    #[test]
    fn candidate_rejects_spaced_code_closer() {
        let source = "[[code]]\nbody\n[[/code ]]\nafter";

        assert!(candidates(source).is_empty());
    }

    #[test]
    fn candidate_accepts_trailing_line_whitespace_after_closer() {
        // scp-9844's real shape: both closers carry one trailing space, and
        // the second opener also carries one. Live Wikidot renders both
        // blocks and hosts their contents.
        let source =
            "[[code]]\nfn one() {}\n[[/code]] \n[[code]] \nfn two() {}\n[[/code]]\t\n";
        let candidates = candidates(source);
        let first = source.find("[[code]]").unwrap();
        let second = source[first + 1..]
            .find("[[code]]")
            .map(|index| index + first + 1)
            .unwrap();

        let first_candidate = candidates.get(&first).expect("first code candidate");
        assert_eq!(&source[first_candidate.body.clone()], "\nfn one() {}\n");
        let second_candidate = candidates.get(&second).expect("second code candidate");
        assert_eq!(&source[second_candidate.body.clone()], " \nfn two() {}\n");
    }

    #[test]
    fn candidate_closes_at_a_closer_followed_by_whitespace_and_text() {
        // The closer terminates the block; the text after the whitespace
        // is outer content, not part of the body.
        let source = "[[code]]\nbody\n[[/code]] x\n";
        let candidate = candidate_at(source, 0).expect("code candidate");

        assert_eq!(&source[candidate.body.clone()], "\nbody\n");
        assert_eq!(candidate.end_blocks_to_skip, 0);
    }

    #[test]
    fn candidate_rejects_closer_followed_directly_by_text() {
        // A closer followed immediately by a non-whitespace byte is body
        // text, so the block never closes.
        for source in [
            "[[code]]\nbody\n[[/code]]x\n",
            "[[code]]\nbody\n[[/code]], x\n",
            "[[code]]\nbody\n[[/code]]]\n",
            "[[code]]\nbody\n[[/code]][[div]]\nx\n[[/div]]",
        ] {
            assert!(
                candidates(source).is_empty(),
                "directly suffixed closer must not close: {source:?}"
            );
        }
    }

    #[test]
    fn candidate_accepts_an_iftags_fragment_in_the_head() {
        let source = "[[code type=\"py[[iftags -zzz]]thon[[/iftags]]\"]]x = 1[[/code]]";
        let candidate = candidate_at(source, 0).expect("code candidate");

        assert_eq!(&source[candidate.body.clone()], "thon[[/iftags]]\"]]x = 1",);
        assert_eq!(candidate.owner_end, source.len());
        assert_eq!(candidate.end_blocks_to_skip, 0);
    }

    #[test]
    fn malformed_head_cannot_create_a_reverse_body_range() {
        let source = "[[code type=\"cssV][[/code]]\n[[code]][[/code]]";

        let malformed = source.find("[[code type=\"cssV]").unwrap();
        let later = source.rfind("[[code]]").unwrap();
        let candidates = candidates(source);

        assert!(!candidates.contains_key(&malformed));
        let candidate = candidates.get(&later).expect("later valid code candidate");
        assert!(candidate.body.start <= candidate.body.end);
    }
}
