/*
 * preproc/whitespace.rs
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

//! This performs the various miscellaneous substitutions that Wikidot does
//! in preparation for its parsing and handling processes. These are:
//! * Replacing DOS and legacy Mac newlines
//! * Trimming whitespace lines
//! * Concatenating lines that end with backslashes
//! * Convert tabs to four spaces
//! * Remove characters discarded by Wikidot's input filter
//! * Compress groups of 3+ newlines into 2 newlines

use super::Replacer;
use super::parser_functions::LiteralRegionIndex;
use regex::{Regex, RegexBuilder};
use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::LazyLock;

static LEADING_NONSTANDARD_WHITESPACE: LazyLock<Regex> = LazyLock::new(|| {
    RegexBuilder::new("^[\u{00a0}\u{2007}]+")
        .multi_line(true)
        .build()
        .unwrap()
});
static WHITESPACE_ONLY_LINE: LazyLock<Replacer> =
    LazyLock::new(|| Replacer::RegexReplace {
        regex: RegexBuilder::new(r"^\s+$")
            .multi_line(true)
            .build()
            .unwrap(),
        replacement: "",
    });
// Wikidot treats a line containing a non-breaking space as authored content:
// it remains in the surrounding paragraph and renders as line breaks around
// the space. Strip only ordinary indentation lines in the legacy layout;
// the leading-space pass below preserves an NBSP-only line so the parser can
// emit that authored content instead of manufacturing a paragraph break.
static WIKIDOT_WHITESPACE_ONLY_LINE: LazyLock<Replacer> =
    LazyLock::new(|| Replacer::RegexReplace {
        regex: RegexBuilder::new(r"^[ \t\r\n\x0B\x0C]+$")
            .multi_line(true)
            .build()
            .unwrap(),
        replacement: "",
    });
static LEADING_DOCUMENT_WHITESPACE: LazyLock<Replacer> =
    LazyLock::new(|| Replacer::RegexReplace {
        regex: Regex::new(r"^[ \t\n]+").unwrap(),
        replacement: "",
    });
static TRAILING_NEWLINES: LazyLock<Replacer> = LazyLock::new(|| Replacer::RegexReplace {
    regex: Regex::new(r"\n+$").unwrap(),
    replacement: "",
});
static DOS_MAC_NEWLINES: LazyLock<Replacer> = LazyLock::new(|| Replacer::RegexReplace {
    regex: Regex::new(r"\r\n?").unwrap(),
    replacement: "\n",
});
static WIKIDOT_CONTINUED_DIV_OPENER: LazyLock<Regex> = LazyLock::new(|| {
    RegexBuilder::new(r"^\[\[div_?(?:[ \t].*)?\]\]$")
        .case_insensitive(true)
        .build()
        .unwrap()
});

pub(super) fn expose_wikidot_replacement_markers(text: &mut String) {
    if !text.contains('\u{fffd}') {
        return;
    }

    let characters = text.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    let mut line_has_content = false;
    let mut paragraph_line = 1;
    let mut document_has_content = false;

    while index < characters.len() {
        if characters[index] != '\u{fffd}' {
            let character = characters[index];
            if character == '\r' || character == '\n' {
                let mut newline_end = logical_newline_end(&characters, index);
                let mut newline_count = 1;
                while matches!(characters.get(newline_end), Some('\r' | '\n')) {
                    newline_end = logical_newline_end(&characters, newline_end);
                    newline_count += 1;
                }
                output.extend(characters[index..newline_end].iter().copied());
                if document_has_content {
                    if newline_count >= 2 {
                        paragraph_line = 1;
                    } else {
                        paragraph_line += 1;
                    }
                }
                line_has_content = false;
                index = newline_end;
            } else {
                output.push(character);
                line_has_content = true;
                if !character.is_ascii_whitespace() {
                    document_has_content = true;
                }
                index += 1;
            }
            continue;
        }

        let run_start = index;
        while characters.get(index) == Some(&'\u{fffd}') {
            index += 1;
        }
        if index - run_start != 1 {
            continue;
        }

        match characters.get(index).copied() {
            None => {
                output.push('2');
                line_has_content = true;
            }
            Some('\\') if index + 1 == characters.len() => {
                output.push_str(&paragraph_line.to_string());
                output.push('\u{fffd}');
                line_has_content = true;
            }
            Some('\r' | '\n') => {
                let first_end = logical_newline_end(&characters, index);
                if first_end == characters.len() {
                    output.push('2');
                    index = first_end;
                } else if matches!(characters.get(first_end), Some('\r' | '\n')) {
                    output.push_str("23");
                    index = logical_newline_end(&characters, first_end);
                } else {
                    output.push(if line_has_content { '1' } else { '2' });
                    index = first_end;
                }
                line_has_content = true;
            }
            Some(_) => {}
        }
    }

    *text = output;
}

pub(super) fn preserve_wikidot_document_indentation_barrier(text: &mut String) {
    let leading_len = text
        .bytes()
        .take_while(|byte| matches!(byte, b' ' | b'\t' | b'\n'))
        .count();
    if wikidot_spaced_inner_include_example(text, leading_len) {
        return;
    }
    let structural = text.as_bytes().get(leading_len).copied();
    let own_line_indented = text[..leading_len]
        .rsplit('\n')
        .next()
        .is_some_and(|line| line.bytes().any(|byte| matches!(byte, b' ' | b'\t')));
    if own_line_indented
        && matches!(
            structural,
            Some(
                b'>' | b'='
                    | b'+'
                    | b'|'
                    | b'['
                    | b'_'
                    | b'*'
                    | b'#'
                    | b'~'
                    | b':'
                    | b'-',
            )
        )
    {
        let replacement = if structural == Some(b'_') {
            "\0 "
        } else {
            "\0"
        };
        text.replace_range(..leading_len, replacement);
    }
}

pub(super) fn preserve_wikidot_terminal_backslash_run(text: &mut String) {
    let trailing_newline_start = text.trim_end_matches(['\r', '\n']).len();
    if trailing_newline_start < text.len()
        && text[..trailing_newline_start].ends_with('\\')
    {
        text.truncate(trailing_newline_start);
    }
    if !text.ends_with('\\') {
        return;
    }
    let prefix = text
        .strip_suffix('\\')
        .expect("terminal backslash preservation requires a trailing backslash");
    if prefix.ends_with('\r') || prefix.ends_with('\n') {
        text.pop();
        return;
    }
    if prefix.ends_with('\\') {
        text.insert(text.len() - 1, '\u{0001}');
    }
}

fn logical_newline_end(characters: &[char], start: usize) -> usize {
    if characters[start] == '\r' && characters.get(start + 1) == Some(&'\n') {
        start + 2
    } else {
        start + 1
    }
}
/// Performs all whitespace substitutions in-place in the given text.
pub fn substitute(text: &mut String) {
    substitute_for_layout(text, false);
}

pub(super) fn substitute_wikidot(text: &mut String) {
    substitute_for_layout(text, true);
}

/// Collapse physical lines containing only whitespace without applying the
/// rest of the document-level whitespace preprocessor.
///
/// Delayed List-mode callers already hold source ranges for runtime values,
/// so they cannot safely rerun all preprocessing after substitution. This
/// source-preserving boundary is the one Wikidot operation they need before
/// constructing those ranges.
pub fn normalize_wikidot_whitespace_only_lines(text: &mut String) {
    let mut buffer = String::new();
    WHITESPACE_ONLY_LINE.replace(text, &mut buffer);
}

fn substitute_for_layout(text: &mut String, wikidot_compatibility: bool) {
    let mut buffer = String::new();

    macro_rules! replace {
        ($replacer:expr) => {
            $replacer.replace(text, &mut buffer)
        };
    }

    replace!(DOS_MAC_NEWLINES);

    if wikidot_compatibility {
        preserve_document_leading_indentation_barrier(text);
    }
    replace!(LEADING_DOCUMENT_WHITESPACE);

    if wikidot_compatibility {
        replace!(WIKIDOT_WHITESPACE_ONLY_LINE);
    } else {
        replace_leading_spaces(text);
        replace!(WHITESPACE_ONLY_LINE);
    }

    join_continued_lines(text, &mut buffer, wikidot_compatibility);

    if wikidot_compatibility {
        normalize_wikidot_code_body_bytes(text);
        expand_wikidot_list_indentation_tabs(text);
        preserve_adjacent_wikidot_tab_link_spacing(text);
    }

    if wikidot_compatibility {
        preserve_wikidot_quote_tab_separator(text);
    }

    replace_tabs(text, &mut buffer, wikidot_compatibility);

    if !wikidot_compatibility && text.contains('\0') {
        *text = text.replace('\0', " ");
    }

    replace!(TRAILING_NEWLINES);
}

fn normalize_wikidot_code_body_bytes(text: &mut String) {
    let has_discarded_control = text.chars().any(is_wikidot_discarded_control);
    if !text.contains(['\t', '\u{00a0}']) && !has_discarded_control {
        return;
    }

    let code_ranges = crate::wikidot_code::active_body_ranges(text);
    let raw_ranges = wikidot_inline_raw_body_ranges(text);
    if code_ranges.is_empty() && raw_ranges.is_empty() {
        return;
    }

    let mut output = String::with_capacity(text.len());
    let mut code_index = 0usize;
    let mut raw_index = 0usize;
    for (index, character) in text.char_indices() {
        while code_ranges
            .get(code_index)
            .is_some_and(|range| range.end <= index)
        {
            code_index += 1;
        }
        while raw_ranges
            .get(raw_index)
            .is_some_and(|range| range.end <= index)
        {
            raw_index += 1;
        }
        let in_code = code_ranges
            .get(code_index)
            .is_some_and(|range| range.contains(&index));
        let in_raw = raw_ranges
            .get(raw_index)
            .is_some_and(|range| range.contains(&index));

        match character {
            '\t' if in_code || in_raw => output.push_str("    "),
            '\u{00a0}' if in_code => output.push(' '),
            character
                if (in_code || in_raw) && is_wikidot_discarded_control(character) => {}
            _ => output.push(character),
        }
    }
    *text = output;
}

fn wikidot_inline_raw_body_ranges(source: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut line_offset = 0usize;
    for line in source.split_inclusive('\n') {
        let mut cursor = 0usize;
        while let Some(relative_open) = line[cursor..].find("@@") {
            let open = cursor + relative_open;
            let body_start = open + 2;
            let Some(relative_close) = line[body_start..].find("@@") else {
                break;
            };
            let close = body_start + relative_close;
            ranges.push(line_offset + body_start..line_offset + close);
            cursor = close + 2;
        }
        line_offset += line.len();
    }
    ranges
}

#[inline]
fn is_wikidot_discarded_control(character: char) -> bool {
    matches!(character as u32, 0x00..=0x08 | 0x0b..=0x0c | 0x0e..=0x1a | 0x1c..=0x1f)
}

fn expand_wikidot_list_indentation_tabs(text: &mut String) {
    if !text.contains('\t') {
        return;
    }

    let literal_regions = LiteralRegionIndex::new(text);
    let mut protected_ranges = literal_regions.ranges().to_vec();
    protected_ranges.extend(wikidot_comment_ranges(text));
    let protected_ranges = merge_protected_ranges(protected_ranges);

    let source = std::mem::take(text);
    let mut output = String::with_capacity(source.len());
    let mut line_start = 0;
    while line_start < source.len() {
        let line_end = source[line_start..]
            .find('\n')
            .map_or(source.len(), |offset| line_start + offset);
        let line = &source[line_start..line_end];
        let indent_len = line
            .bytes()
            .take_while(|byte| matches!(byte, b' ' | b'\t'))
            .count();
        let marker = line.as_bytes().get(indent_len).copied();
        let repeated_marker = line.as_bytes().get(indent_len + 1) == marker.as_ref();
        let list_marker = matches!(marker, Some(b'*' | b'#')) && !repeated_marker;
        let protected = index_in_ranges(&protected_ranges, line_start + indent_len);

        if list_marker && !protected && line[..indent_len].contains('\t') {
            for character in line[..indent_len].chars() {
                if character == '\t' {
                    output.push_str("    ");
                } else {
                    output.push(character);
                }
            }
            output.push_str(&line[indent_len..]);
        } else {
            output.push_str(line);
        }
        if line_end < source.len() {
            output.push('\n');
            line_start = line_end + 1;
        } else {
            break;
        }
    }
    *text = output;
}

fn merge_protected_ranges(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.sort_unstable_by_key(|range| (range.start, range.end));
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        if let Some(previous) = merged.last_mut()
            && range.start <= previous.end
        {
            previous.end = previous.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

fn preserve_adjacent_wikidot_tab_link_spacing(text: &mut String) {
    if !text.contains('\t') || !text.contains('[') {
        return;
    }

    let literal_regions = LiteralRegionIndex::new(text);
    let mut protected_ranges = literal_regions.ranges().to_vec();
    protected_ranges.extend(wikidot_comment_ranges(text));
    let merged_ranges = merge_protected_ranges(protected_ranges);

    let mut insertions = BTreeSet::new();
    let mut cursor = 0;
    while let Some(relative_open) = text[cursor..].find('[') {
        let open = cursor + relative_open;
        cursor = open + 1;
        if index_in_ranges(&merged_ranges, open) || text[open..].starts_with("[[") {
            continue;
        }

        let target_start = if text[open..].starts_with("[*") {
            open + 2
        } else {
            open + 1
        };
        let mut target_end = target_start;
        let mut tab = None;
        while target_end < text.len() {
            let character = text[target_end..]
                .chars()
                .next()
                .expect("target cursor remains on a character boundary");
            if character == '\t' {
                tab = Some(target_end);
                break;
            }
            if character.is_whitespace() {
                break;
            }
            target_end += character.len_utf8();
        }
        let Some(tab) = tab else {
            cursor = target_end.max(cursor);
            continue;
        };
        cursor = tab + 1;
        if index_in_ranges(&merged_ranges, tab) {
            continue;
        }

        let target = &text[target_start..target_end];
        if target.is_empty()
            || target.ends_with(']')
            || !(crate::url::is_url(target) || target.starts_with('/'))
        {
            continue;
        }

        if text[..open]
            .chars()
            .next_back()
            .is_some_and(|character| !character.is_whitespace())
        {
            insertions.insert(open);
        }
    }

    if insertions.is_empty() {
        return;
    }

    let mut output = String::with_capacity(text.len() + insertions.len());
    let mut copied = 0;
    for insertion in insertions {
        output.push_str(&text[copied..insertion]);
        output.push(' ');
        copied = insertion;
    }
    output.push_str(&text[copied..]);
    *text = output;
}

fn wikidot_comment_ranges(source: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut cursor = 0;
    while let Some(relative_start) = source[cursor..].find("[!--") {
        let start = cursor + relative_start;
        let body_start = start + "[!--".len();
        let end = source[body_start..]
            .find("--]")
            .map_or(source.len(), |relative_end| {
                body_start + relative_end + "--]".len()
            });
        ranges.push(start..end);
        cursor = end;
        if cursor == source.len() {
            break;
        }
    }
    ranges
}

fn index_in_ranges(ranges: &[Range<usize>], index: usize) -> bool {
    let candidate = ranges.partition_point(|range| range.end <= index);
    ranges
        .get(candidate)
        .is_some_and(|range| range.start <= index)
}

fn append_replaced_tabs(output: &mut String, input: &str, width: usize) {
    for ch in input.chars() {
        if ch == '\t' {
            output.extend(std::iter::repeat_n(' ', width));
        } else {
            output.push(ch);
        }
    }
}

fn append_wikidot_block_head_tabs(output: &mut String, head: &str) {
    if wikidot_user_block_head(head) {
        append_replaced_tabs(output, head, 1);
        return;
    }

    let delimiters = head
        .match_indices("=\"")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let mut value_ranges = Vec::with_capacity(delimiters.len());

    for (index, delimiter) in delimiters.iter().copied().enumerate() {
        let value_start = delimiter + 2;
        let segment_end = delimiters.get(index + 1).copied().unwrap_or(head.len());
        let segment = &head[value_start..segment_end];
        if let Some(value_end) = segment.rfind('"') {
            value_ranges.push(value_start..value_start + value_end);
        }
    }

    let mut range_index = 0;
    for (index, ch) in head.char_indices() {
        while value_ranges
            .get(range_index)
            .is_some_and(|range| range.end <= index)
        {
            range_index += 1;
        }
        let in_value = value_ranges
            .get(range_index)
            .is_some_and(|range| range.contains(&index));

        if ch == '\t' {
            output.extend(std::iter::repeat_n(' ', if in_value { 4 } else { 1 }));
        } else {
            output.push(ch);
        }
    }
}

fn wikidot_user_block_head(head: &str) -> bool {
    let Some(field) = head.strip_prefix("[[") else {
        return false;
    };
    let name_end = field
        .find(|character: char| character.is_ascii_whitespace() || character == ']')
        .unwrap_or(field.len());
    field[..name_end].eq_ignore_ascii_case("user")
}

fn replace_tabs(text: &mut String, buffer: &mut String, wikidot_compatibility: bool) {
    if !text.contains('\t') {
        return;
    }

    buffer.clear();
    buffer.reserve(text.len());
    if !wikidot_compatibility {
        append_replaced_tabs(buffer, text, 4);
        std::mem::swap(text, buffer);
        return;
    }

    let mut cursor = 0;
    while let Some(open_offset) = text[cursor..].find("[[") {
        let open = cursor + open_offset;
        append_replaced_tabs(buffer, &text[cursor..open], 1);

        let end = text[open + 2..]
            .find("]]")
            .map_or(text.len(), |close_offset| open + 2 + close_offset + 2);
        append_wikidot_block_head_tabs(buffer, &text[open..end]);
        cursor = end;
        if cursor == text.len() {
            break;
        }
    }
    append_replaced_tabs(buffer, &text[cursor..], 1);
    std::mem::swap(text, buffer);
}

fn preserve_wikidot_quote_tab_separator(text: &mut String) {
    if !text.contains('\t') {
        return;
    }

    let mut output = String::with_capacity(text.len() + 8);
    for line in text.split_inclusive('\n') {
        let bytes = line.as_bytes();
        let mut markers = 0;
        while bytes.get(markers) == Some(&b'>') {
            markers += 1;
        }
        if markers > 0 && bytes.get(markers) == Some(&b'\t') {
            output.push_str(&line[..markers]);
            output.push(' ');
            output.push_str(&line[markers..]);
        } else {
            output.push_str(line);
        }
    }
    *text = output;
}

fn preserve_document_leading_indentation_barrier(text: &mut String) {
    let leading_len = text
        .bytes()
        .take_while(|byte| matches!(byte, b' ' | b'\t' | b'\n'))
        .count();
    if leading_len == 0 {
        return;
    }
    if wikidot_spaced_inner_include_example(text, leading_len) {
        return;
    }
    let own_line_indented = text[..leading_len]
        .rsplit('\n')
        .next()
        .is_some_and(|line| line.bytes().any(|byte| matches!(byte, b' ' | b'\t')));
    let structural = text.as_bytes().get(leading_len).copied();
    if own_line_indented && structural == Some(b'>') {
        text.replace_range(..leading_len, "\0");
    }
}

fn wikidot_spaced_inner_include_example(text: &str, leading_len: usize) -> bool {
    const PREFIX: &str = "> > [[include ";
    let line = text[leading_len..]
        .split_once('\n')
        .map_or(&text[leading_len..], |(line, _)| line);
    line.get(..PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(PREFIX))
}

pub(super) fn filter_characters(text: &mut String, preserve_terminal_marker: bool) {
    let terminal_marker = preserve_terminal_marker && text.ends_with("\u{fffd}\\");
    if terminal_marker {
        text.truncate(text.len() - "\u{fffd}\\".len());
    }
    text.retain(|character| character != '\u{fffd}');
    if terminal_marker {
        text.push_str("\u{fffd}\\");
    }
    let mut buffer = String::new();
    LEADING_DOCUMENT_WHITESPACE.replace(text, &mut buffer);
}

/// Removes line-continuation pairs, including pairs exposed by earlier removals.
///
/// The output buffer acts as a stack: a newline cancels the immediately preceding
/// backslash, whether that backslash was adjacent in the input or exposed by a
/// previous cancellation. Each character is pushed at most once and popped at
/// most once, so cascading continuations are handled in linear time.
fn join_continued_lines(
    text: &mut String,
    buffer: &mut String,
    wikidot_compatibility: bool,
) {
    if !text.contains("\\\n") {
        return;
    }

    let raw_body_ranges = if wikidot_compatibility {
        let mut ranges = wikidot_math_body_ranges(text);
        ranges.extend(crate::wikidot_code::active_body_ranges(text));
        ranges.sort_by_key(|range| range.start);
        ranges
    } else {
        Vec::new()
    };
    let mut raw_body_index = 0usize;

    buffer.clear();
    buffer.reserve(text.len());

    for (index, character) in text.char_indices() {
        if character == '\n' && buffer.as_bytes().last() == Some(&b'\\') {
            while raw_body_ranges
                .get(raw_body_index)
                .is_some_and(|range| range.end <= index)
            {
                raw_body_index += 1;
            }
            let in_raw_body = raw_body_ranges
                .get(raw_body_index)
                .is_some_and(|range| range.contains(&index));
            if in_raw_body
                || (wikidot_compatibility
                    && wikidot_continued_div_opener(&text[index + 1..]))
            {
                buffer.push(character);
            } else {
                let removed = buffer.pop();
                debug_assert_eq!(removed, Some('\\'));
            }
        } else {
            buffer.push(character);
        }
    }

    std::mem::swap(text, buffer);
}

fn wikidot_math_body_ranges(source: &str) -> Vec<Range<usize>> {
    if !source.contains("[[") {
        return Vec::new();
    }
    let code_ranges = crate::wikidot_code::active_ranges(source);
    let mut code_index = 0usize;
    let mut ranges = Vec::new();
    let mut open: Option<(usize, usize)> = None;
    let mut line_offset = 0usize;

    for line in source.split_inclusive('\n') {
        let line_start = line_offset;
        line_offset += line.len();
        while code_ranges
            .get(code_index)
            .is_some_and(|range| range.end <= line_start)
        {
            code_index += 1;
        }
        if code_ranges
            .get(code_index)
            .is_some_and(|range| range.contains(&line_start))
        {
            continue;
        }

        let marker = line.trim();
        match &mut open {
            None if wikidot_standalone_math_opener(marker) => {
                open = Some((line_offset, 0));
            }
            Some((body_start, depth)) => {
                if marker.eq_ignore_ascii_case("[[/math]]") {
                    if *depth == 0 {
                        ranges.push(*body_start..line_start);
                        open = None;
                    } else {
                        *depth -= 1;
                    }
                } else if wikidot_standalone_math_opener(marker) {
                    *depth += 1;
                }
            }
            None => {}
        }
    }

    ranges
}

fn wikidot_standalone_math_opener(marker: &str) -> bool {
    let Some(head) = marker
        .strip_prefix("[[")
        .and_then(|rest| rest.strip_suffix("]]"))
    else {
        return false;
    };
    if head.contains("[[") || head.contains("]]") {
        return false;
    }
    let mut parts = head.split_whitespace();
    parts
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("math"))
}

fn wikidot_continued_div_opener(text: &str) -> bool {
    let line = text.split_once('\n').map_or(text, |(line, _)| line);
    let marker = line.trim_matches([' ', '\t']);
    WIKIDOT_CONTINUED_DIV_OPENER.is_match(marker)
}

fn replace_leading_spaces(text: &mut String) {
    trace!("Replacing leading non-standard spaces with regular spaces");

    let mut captures = LEADING_NONSTANDARD_WHITESPACE.captures_iter(text);
    let Some(first_capture) = captures.next() else {
        return;
    };

    let mut buffer = String::with_capacity(text.len());
    let mut last_copied = 0;

    for capture in std::iter::once(first_capture).chain(captures) {
        let mtch = capture
            .get(0)
            .expect("Regular expression lacks a full match");

        let count = mtch.as_str().chars().count();

        buffer.push_str(&text[last_copied..mtch.start()]);
        let line_suffix = text[mtch.end()..]
            .split_once('\n')
            .map_or(&text[mtch.end()..], |(line, _)| line);
        if line_suffix
            .chars()
            .all(|character| matches!(character, ' ' | '\t' | '\r'))
        {
            buffer.push_str(mtch.as_str());
        } else {
            buffer.extend(std::iter::repeat_n(' ', count));
        }
        last_copied = mtch.end();
    }

    buffer.push_str(&text[last_copied..]);
    *text = buffer;
}

#[cfg(test)]
const TEST_CASES: [(&str, &str); 10] = [
    ("\tapple\n\tbanana\tcherry\n", "apple\n banana cherry"),
    (
        "newlines:\r\n* apple\r* banana\r\ncherry\n\r* durian",
        "newlines:\n* apple\n* banana\ncherry\n\n* durian",
    ),
    (
        "apple\nbanana\n\ncherry\n\n\npineapple\n\n\n\nstrawberry\n\n\n\n\nblueberry\n\n\n\n\n\n",
        "apple\nbanana\n\ncherry\n\npineapple\n\nstrawberry\n\nblueberry",
    ),
    (
        "apple\rbanana\r\rcherry\r\r\rpineapple\r\r\r\rstrawberry\r\r\r\r\rblueberry\r\r\r\r\r\r",
        "apple\nbanana\n\ncherry\n\npineapple\n\nstrawberry\n\nblueberry",
    ),
    (
        "concat:\napple banana \\\nCherry\\\nPineapple \\ grape\nblueberry\n",
        "concat:\napple banana CherryPineapple \\ grape\nblueberry",
    ),
    ("<\n        \n      \n  \n      \n>", "<\n\n>"),
    (
        "\u{00a0}\u{00a0}\u{2007} apple",
        "\u{00a0}\u{00a0}\u{2007} apple",
    ),
    ("x\\\\\n\ny", "xy"),
    ("\\\\\n\nX", "X"),
    (
        "\u{00a0}apple\n\u{2007}\u{00a0}banana\ncherry\u{00a0}",
        "\u{00a0}apple\n\u{2007}\u{00a0}banana\ncherry\u{00a0}",
    ),
];

#[test]
fn regexes() {
    let _ = &*LEADING_NONSTANDARD_WHITESPACE;
    let _ = &*WHITESPACE_ONLY_LINE;
    let _ = &*WIKIDOT_WHITESPACE_ONLY_LINE;
    let _ = &*LEADING_DOCUMENT_WHITESPACE;
    let _ = &*TRAILING_NEWLINES;
    let _ = &*DOS_MAC_NEWLINES;
}

#[test]
fn test_substitute() {
    use super::test::test_substitution;

    test_substitution("miscellaneous", substitute_wikidot, &TEST_CASES);
}

#[test]
fn wikidot_preserves_nbsp_only_lines_inside_a_paragraph() {
    let mut text = "Alpha\n\u{00a0}\nBeta\n\nGamma".to_owned();

    substitute_wikidot(&mut text);

    assert_eq!(text, "Alpha\n\u{00a0}\nBeta\n\nGamma");
}

#[test]
fn wikidot_preserves_ascii_padding_on_nonblank_internal_lines() {
    let mut text = "A\n ====\nB\n".to_owned();

    crate::preprocess_for_layout(&mut text, crate::layout::Layout::Wikidot);

    assert_eq!(text, "A\n ====\nB");
}

#[test]
fn wikijump_substitution_keeps_native_tabs_and_null_handling() {
    let mut text = "a\tb\0c".to_owned();
    substitute(&mut text);
    assert_eq!(text, "a    b c");
}

#[test]
fn wikidot_substitution_preserves_tab_width_inside_getattrs_values() {
    let mut text = concat!(
        "plain\ttext\n",
        "[[collapsible show=\"OPEN\tSECOND\" ",
        "bogus=bare id=\"alpha\tbeta\"]]",
    )
    .to_owned();

    substitute_wikidot(&mut text);

    assert_eq!(
        text,
        concat!(
            "plain text\n",
            "[[collapsible show=\"OPEN    SECOND\" ",
            "bogus=bare id=\"alpha    beta\"]]",
        ),
    );
}

#[test]
fn wikidot_list_indentation_uses_four_columns_per_tab() {
    let mut text = concat!(
        "* A\n",
        "\t* B\n",
        "\t\t* C\n",
        " \t* D\n",
        "\t * E\n",
        "[[code]]\n",
        "\t* literal\n",
        "[[/code]]",
    )
    .to_owned();

    substitute_wikidot(&mut text);

    assert_eq!(
        text,
        concat!(
            "* A\n",
            "    * B\n",
            "        * C\n",
            "     * D\n",
            "     * E\n",
            "[[code]]\n",
            "    * literal\n",
            "[[/code]]",
        ),
    );
}

#[test]
fn wikidot_list_indentation_keeps_nested_comment_literal_ranges_merged() {
    let mut text = concat!(
        "[[raw]]\n",
        "[!-- nested comment --]\n",
        "\t* literal\n",
        "[[/raw]]",
    )
    .to_owned();

    substitute_wikidot(&mut text);

    assert_eq!(
        text,
        concat!(
            "[[raw]]\n",
            "[!-- nested comment --]\n",
            " * literal\n",
            "[[/raw]]",
        ),
    );
}

#[test]
fn wikidot_adjacent_tab_link_separator_keeps_its_visible_spacing() {
    let mut text = concat!(
        "BEGIN|[http://example.com/path\tEND]\n",
        "BEGIN|[*https://example.com/path\tEND]\n",
        "[http://example.com/path\tEND]\n",
        "BEGIN|[http://example.com/path END]\n",
        "[!--BEGIN|[http://example.com/path\tEND]--]\n",
        "[[code]]\nBEGIN|[http://example.com/path\tEND]\n[[/code]]\n",
        "@@BEGIN|[http://example.com/path\tEND]@@",
    )
    .to_owned();

    substitute_wikidot(&mut text);

    assert_eq!(
        text,
        concat!(
            "BEGIN| [http://example.com/path END]\n",
            "BEGIN| [*https://example.com/path END]\n",
            "[http://example.com/path END]\n",
            "BEGIN|[http://example.com/path END]\n",
            "[!--BEGIN|[http://example.com/path END]--]\n",
            "[[code]]\nBEGIN|[http://example.com/path    END]\n[[/code]]\n",
            "@@BEGIN|[http://example.com/path    END]@@",
        ),
    );
}

#[test]
fn preserves_a_syntax_barrier_for_an_indented_leading_quote() {
    let mut text = "\n\t  > first\n  > second".to_owned();

    substitute_wikidot(&mut text);

    assert_eq!(text, "\0> first\n  > second");
}

#[test]
fn trims_document_indentation_before_a_spaced_inner_include_example() {
    let mut text = "  > > [[include component:box |name=x]]".to_owned();

    preserve_wikidot_document_indentation_barrier(&mut text);
    substitute_wikidot(&mut text);

    assert_eq!(text, "> > [[include component:box |name=x]]");

    let mut source = "  > > [[include component:box |name=x]]".to_owned();
    let page_info = crate::data::PageInfo::dummy();
    let settings = crate::settings::WikitextSettings::from_mode(
        crate::settings::WikitextMode::Page,
        crate::layout::Layout::Wikidot,
    );
    crate::preprocess_for_layout(&mut source, settings.layout);
    let tokens = crate::tokenize(&source);
    let (tree, _) = crate::parse(&tokens, &page_info, &settings).into();
    let html = crate::render::Render::render(
        &crate::render::html::HtmlRender,
        &tree,
        &page_info,
        &settings,
    )
    .body;
    assert_eq!(
        html,
        "<blockquote><p>&gt; [[include component:box |name=x]]</p></blockquote>",
    );
}

#[test]
fn preserves_a_syntax_barrier_for_indented_non_center_document_openers() {
    for (input, expected) in [
        ("\t== x", "\0== x"),
        (" + x", "\0+ x"),
        (" [[div]]x[[/div]]", "\0[[div]]x[[/div]]"),
        (" : A : 1", "\0: A : 1"),
        ("\n\n : A : 1", "\0: A : 1"),
        (" \n: A : 1", ": A : 1"),
        (" ----", "\0----"),
        ("\n\n ----", "\0----"),
        (" \n----", "----"),
        (" = centered", "\0= centered"),
        (" \n= centered", "= centered"),
        (" \n* item", "* item"),
        (" \n> quoted", "> quoted"),
        (" _", "\0 _"),
        (" \n_\nfoo", "_\nfoo"),
    ] {
        let mut text = input.to_owned();
        preserve_wikidot_document_indentation_barrier(&mut text);
        substitute_wikidot(&mut text);
        assert_eq!(text, expected);
    }
}

#[test]
fn trims_document_indentation_before_a_centered_line_after_a_spaced_line() {
    let mut text = " \n= centered".to_owned();

    preserve_wikidot_document_indentation_barrier(&mut text);
    substitute_wikidot(&mut text);

    assert_eq!(text, "= centered");
}

#[test]
fn preserves_document_indentation_before_list_openers() {
    for (input, expected) in [(" * x", "\0* x"), ("\t# x", "\0# x")] {
        let mut text = input.to_owned();
        preserve_wikidot_document_indentation_barrier(&mut text);
        substitute_wikidot(&mut text);
        assert_eq!(text, expected);
    }
}

#[test]
fn trims_indentation_before_plain_document_text() {
    let mut text = "\t\rplain".to_owned();
    preserve_wikidot_document_indentation_barrier(&mut text);
    substitute_wikidot(&mut text);
    assert_eq!(text, "plain");
}

#[test]
fn preserves_wikidot_terminal_backslash_runs() {
    for (input, expected) in [
        ("a\\", "a\\"),
        ("a\\\\", "a\\\u{0001}\\"),
        ("a\\\\\\", "a\\\\\u{0001}\\"),
        ("a\\\n\n", "a\\"),
        ("a\n\\", "a\n"),
        ("a\r\\", "a\r"),
    ] {
        let mut text = input.to_owned();
        preserve_wikidot_terminal_backslash_run(&mut text);
        assert_eq!(text, expected);
    }
}

#[test]
fn preserves_indentation_after_non_whitespace_content() {
    let mut text = "[!-- comment --]\n  > literal".to_owned();

    substitute(&mut text);

    assert_eq!(text, "[!-- comment --]\n  > literal");
}

#[test]
fn retains_control_barriers_and_removes_unsafe_replacement_marker() {
    let mut text = String::new();
    text.extend([
        'A', '\0', '\u{0001}', '\u{0008}', '\u{000b}', '\u{000c}', '\u{000e}',
        '\u{001a}', '\u{001b}', '\u{001c}', '\u{001f}', '\u{007f}', '\u{fffd}', 'B',
    ]);

    filter_characters(&mut text, false);

    assert_eq!(
        text,
        "A\0\u{0001}\u{0008}\u{000b}\u{000c}\u{000e}\u{001a}\u{001b}\u{001c}\u{001f}\u{007f}B"
    );
}

#[test]
fn exposes_wikidot_replacement_markers_at_input_boundaries() {
    for (input, expected) in [
        ("\u{fffd}", "2"),
        ("a\u{fffd}", "a2"),
        ("\u{fffd}a", "a"),
        ("a\u{fffd}b", "ab"),
        ("\u{fffd}\u{fffd}", ""),
        ("a\u{fffd}\nb", "a1b"),
        ("a\u{fffd}\r\nb", "a1b"),
        ("a\u{fffd}\n\nb", "a23b"),
        ("a\n\u{fffd}\nb", "a\n2b"),
        ("a\u{fffd}\\", "a1\u{fffd}\\"),
        ("a\rb\u{fffd}\\", "a\rb2\u{fffd}\\"),
        ("a\rb\rc\u{fffd}\\", "a\rb\rc3\u{fffd}\\"),
        ("a\n\nb\u{fffd}\\", "a\n\nb1\u{fffd}\\"),
        ("\ra\u{fffd}\\", "\ra1\u{fffd}\\"),
    ] {
        let mut text = input.to_owned();
        expose_wikidot_replacement_markers(&mut text);
        assert_eq!(text, expected, "{input:?}");
    }
}

#[test]
fn character_filter_preserves_a_control_as_a_line_continuation_barrier() {
    let mut text = "A\\\u{0001}\rB".to_owned();

    crate::preprocess_for_layout(&mut text, crate::layout::Layout::Wikidot);

    assert_eq!(text, "A\\\u{0001}\nB");
}

#[test]
fn wikidot_preserves_a_continued_standalone_div_boundary() {
    for (opener, normalized) in [
        ("[[div]]", "[[div]]"),
        ("[[div class=\"box\"]]", "[[div class=\"box\"]]"),
        ("[[div_ class=\"box\"]]", "[[div_ class=\"box\"]]"),
        ("[[div\tclass=\"box\"]]", "[[div class=\"box\"]]"),
        ("[[div_\tclass=\"box\"]]", "[[div_ class=\"box\"]]"),
        ("[[div @=\"value\"]]", "[[div @=\"value\"]]"),
        ("[[div_ class]]", "[[div_ class]]"),
    ] {
        let mut wikidot = format!("alpha\\\n{opener}\nbody\n[[/div]]");
        substitute_wikidot(&mut wikidot);
        assert_eq!(wikidot, format!("alpha\\\n{normalized}\nbody\n[[/div]]"),);

        let mut wikijump = format!("alpha\\\n{opener}\nbody\n[[/div]]");
        substitute(&mut wikijump);
        assert_eq!(
            wikijump,
            format!("alpha{}\nbody\n[[/div]]", opener.replace('\t', "    ")),
        );
    }

    let mut inline = "alpha\\\nbeta".to_owned();
    substitute_wikidot(&mut inline);
    assert_eq!(inline, "alphabeta");
}

#[test]
fn malformed_div_like_lines_remain_ordinary_continuations() {
    for opener in ["[[division]]", "[[div__]]", "[[divx]]"] {
        let mut text = format!("alpha\\\n{opener}");
        substitute_wikidot(&mut text);
        assert_eq!(text, format!("alpha{opener}"));
    }
}

#[test]
fn control_barrier_prevents_late_leading_whitespace_trimming() {
    let mut text = "\u{0001}\tA".to_owned();

    crate::preprocess_for_layout(&mut text, crate::layout::Layout::Wikidot);

    assert_eq!(text, "\u{0001} A");
}

#[test]
fn line_continuations_cascade_across_exposed_boundaries() {
    for depth in [1, 2, 3, 8, 32] {
        let mut text =
            format!("prefix{}{}suffix", "\\".repeat(depth), "\n".repeat(depth),);
        let mut buffer = String::new();

        join_continued_lines(&mut text, &mut buffer, false);

        assert_eq!(text, "prefixsuffix", "cascade depth {depth}");
    }
}

#[test]
fn linear_line_continuation_join_matches_repeated_replacement() {
    const ALPHABET: [char; 3] = ['\\', '\n', 'x'];

    for length in 0..=9 {
        let combinations = ALPHABET.len().pow(length);
        for mut encoded in 0..combinations {
            let mut input = String::with_capacity(length as usize);
            for _ in 0..length {
                input.push(ALPHABET[encoded % ALPHABET.len()]);
                encoded /= ALPHABET.len();
            }

            let mut expected = input.clone();
            while expected.contains("\\\n") {
                expected = expected.replace("\\\n", "");
            }

            let mut actual = input.clone();
            let mut buffer = String::new();
            join_continued_lines(&mut actual, &mut buffer, false);

            assert_eq!(actual, expected, "input {input:?}");
        }
    }
}

#[test]
fn line_continuation_cascade_scales_to_large_inputs() {
    const DEPTH: usize = 32 * 1024;
    let mut text = format!("prefix{}{}suffix", "\\".repeat(DEPTH), "\n".repeat(DEPTH),);
    let mut buffer = String::new();

    join_continued_lines(&mut text, &mut buffer, false);

    assert_eq!(text, "prefixsuffix");
}

#[test]
fn wikidot_math_bodies_are_exempt_from_line_continuation() {
    use crate::layout::Layout;

    let mut wikidot = "a \\\nb\n[[math]]\nx \\\\\ny \\\n[[/math]]\nc \\\nd".to_owned();
    super::preprocess_for_layout(&mut wikidot, Layout::Wikidot);
    assert_eq!(wikidot, "a b\n[[math]]\nx \\\\\ny \\\n[[/math]]\nc d");

    let mut unclosed = "[[math]]\nx \\\ny".to_owned();
    super::preprocess_for_layout(&mut unclosed, Layout::Wikidot);
    assert_eq!(unclosed, "[[math]]\nx y");

    let mut in_code =
        "[[code]]\n[[math]]\nx \\\ny\n[[/math]]\n[[/code]]\nu \\\nv".to_owned();
    super::preprocess_for_layout(&mut in_code, Layout::Wikidot);
    assert_eq!(
        in_code,
        "[[code]]\n[[math]]\nx \\\ny\n[[/math]]\n[[/code]]\nu v"
    );

    let mut native = "[[math]]\nx \\\ny\n[[/math]]".to_owned();
    super::preprocess_for_layout(&mut native, Layout::Wikijump);
    assert!(native.contains("x y"), "{native}");
}
