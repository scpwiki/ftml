/*
 * wikidot_raw_escape.rs
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

//! Wikidot raw inline escape placeholders.
//!
//! Live Wikidot escapes every complete single-line raw inline construct
//! (`@@...@@` or `@<...>@`) before block parsing and replaces it with a
//! placeholder token. The token is the byte `0xFF`, the 1-based ordinal of the
//! construct in source order as decimal digits, and `0xFF` again. Block heads
//! and attribute values observe the placeholders, which is why e.g. a date
//! `format` attribute containing raw markup serializes as `format_%FF1%FF`.
//!
//! Rust strings cannot carry `0xFF` (it is not valid UTF-8), so placeholders
//! are represented with private-use sentinels here and rendered to the live
//! byte spelling only where live exposes it.

/// Opening sentinel of an escaped raw placeholder (`0xFF` in live).
pub(crate) const PLACEHOLDER_OPEN: char = '\u{E000}';

/// Closing sentinel of an escaped raw placeholder (`0xFF` in live).
pub(crate) const PLACEHOLDER_CLOSE: char = '\u{E001}';

/// A complete raw inline construct as live's pre-parse escape pass finds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RawEscape {
    pub start: usize,
    pub end: usize,
}

/// Scan `source` for the complete `@<...>@` raw constructs live escapes.
fn scan_raw_html_escapes(source: &str) -> Vec<RawEscape> {
    let bytes = source.as_bytes();
    let mut escapes = Vec::new();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'@'
            && bytes[index + 1] == b'<'
            && let Some(relative) = source[index + 2..]
                .split(['\n', '\r'])
                .next()
                .and_then(|line| line.find(">@"))
        {
            let end = index + 2 + relative + 2;
            escapes.push(RawEscape { start: index, end });
            index = end;
            continue;
        }
        index += 1;
    }
    escapes
}

/// Scan `source` for the complete `@@...@@` raw constructs live escapes.
///
/// Runs of adjacent closing/opening pairs (`@@x@@@@y@@`) count as one escaped
/// construct: live does not start a new construct at the junction of two
/// doubled-at runs.
fn scan_raw_at_escapes(source: &str, html: &[RawEscape]) -> Vec<RawEscape> {
    let bytes = source.as_bytes();
    let mut escapes = Vec::new();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if let Some(range) = html
            .iter()
            .find(|escape| index >= escape.start && index < escape.end)
        {
            index = range.end;
            continue;
        }
        if bytes[index] == b'@' && bytes[index + 1] == b'@' {
            let mut cursor = index + 2;
            let mut closed = None;
            while cursor + 1 < bytes.len() {
                if html
                    .iter()
                    .any(|escape| cursor >= escape.start && cursor < escape.end)
                {
                    break;
                }
                if matches!(bytes[cursor], b'\n' | b'\r') {
                    break;
                }
                if bytes[cursor] == b'@' && bytes[cursor + 1] == b'@' {
                    closed = Some(cursor);
                    break;
                }
                cursor += 1;
            }
            if let Some(close) = closed {
                escapes.push(RawEscape {
                    start: index,
                    end: close + 2,
                });
                index = close + 2;
                while index + 1 < bytes.len()
                    && bytes[index] == b'@'
                    && bytes[index + 1] == b'@'
                {
                    index += 2;
                }
                continue;
            }
        }
        index += 1;
    }
    escapes
}

/// All raw inline constructs live escapes, in live's numbering order: the
/// `@<...>@` pass runs first, then the `@@...@@` pass.
pub(crate) fn raw_escapes(source: &str) -> Vec<RawEscape> {
    let html = scan_raw_html_escapes(source);
    let mut escapes = html.clone();
    escapes.extend(scan_raw_at_escapes(source, &html));
    escapes
}

/// Append `text` to `output` using live's date-format percent encoding,
/// spelling escaped placeholders as their `%FF<ordinal>%FF` byte run.
pub(crate) fn push_percent_encoded_placeholders(output: &mut String, text: &str) {
    fn push_plain(output: &mut String, character: char) {
        for byte in character.to_string().bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    output.push(char::from(byte));
                }
                _ => {
                    output.push('%');
                    let written = format!("{byte:02X}");
                    output.push_str(&written);
                }
            }
        }
    }

    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character != PLACEHOLDER_OPEN {
            push_plain(output, character);
            continue;
        }
        let mut digits = String::new();
        while let Some(&next) = chars.peek() {
            if next == PLACEHOLDER_CLOSE {
                chars.next();
                break;
            }
            if next.is_ascii_digit() {
                digits.push(next);
                chars.next();
                continue;
            }
            digits.clear();
            break;
        }
        if digits.is_empty() {
            push_plain(output, character);
            continue;
        }
        output.push_str("%FF");
        output.push_str(&digits);
        output.push_str("%FF");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_complete_single_line_raw_constructs() {
        assert_eq!(raw_escapes("@@a@@"), vec![RawEscape { start: 0, end: 5 }]);
        assert_eq!(raw_escapes("@<b>@"), vec![RawEscape { start: 0, end: 5 }],);
        assert!(raw_escapes("@@a").is_empty());
        assert!(raw_escapes("@@a\nb@@").is_empty());
        assert_eq!(
            raw_escapes("x@@a@@ @@b@@"),
            vec![
                RawEscape { start: 1, end: 6 },
                RawEscape { start: 7, end: 12 },
            ],
        );
    }

    #[test]
    fn percent_encodes_placeholders_like_live() {
        let mut output = String::new();
        push_percent_encoded_placeholders(
            &mut output,
            &format!("x{PLACEHOLDER_OPEN}1{PLACEHOLDER_CLOSE}y"),
        );
        assert_eq!(output, "x%FF1%FFy");
    }
}
