/*
 * preproc/typography.rs
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

use super::Replacer;
use super::parser_functions::LiteralRegionIndex;
use crate::parsing::Token;
use regex::Regex;
use std::ops::Range;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WikidotSeamEdit {
    pub range: Range<usize>,
    pub replacement: &'static str,
}

static SINGLE_QUOTES: LazyLock<Replacer> = LazyLock::new(|| Replacer::RegexSurround {
    regex: Regex::new(r"(?s)`(.*?)'").unwrap(),
    begin: "\u{2018}",
    end: "\u{2019}",
});

static DOUBLE_QUOTES: LazyLock<Replacer> = LazyLock::new(|| Replacer::RegexSurround {
    regex: Regex::new(r"(?s)``(.*?)''").unwrap(),
    begin: "\u{201c}",
    end: "\u{201d}",
});

static LOW_DOUBLE_QUOTES: LazyLock<Replacer> =
    LazyLock::new(|| Replacer::RegexSurround {
        regex: Regex::new(r"(?s),,(.*?)''").unwrap(),
        begin: "\u{201e}",
        end: "\u{201d}",
    });

static NATIVE_SINGLE_QUOTES: LazyLock<Replacer> =
    LazyLock::new(|| Replacer::RegexSurround {
        regex: Regex::new(r"`(.*?)'").unwrap(),
        begin: "\u{2018}",
        end: "\u{2019}",
    });

static NATIVE_DOUBLE_QUOTES: LazyLock<Replacer> =
    LazyLock::new(|| Replacer::RegexSurround {
        regex: Regex::new(r"``(.*?)''").unwrap(),
        begin: "\u{201c}",
        end: "\u{201d}",
    });

static NATIVE_LOW_DOUBLE_QUOTES: LazyLock<Replacer> =
    LazyLock::new(|| Replacer::RegexSurround {
        regex: Regex::new(r",,(.*?)''").unwrap(),
        begin: "\u{201e}",
        end: "\u{201d}",
    });

static LEFT_ANGLE_QUOTES: LazyLock<Replacer> = LazyLock::new(|| Replacer::RegexReplace {
    regex: Regex::new(r"<<").unwrap(),
    replacement: "\u{ab}",
});

static RIGHT_ANGLE_QUOTES: LazyLock<Replacer> =
    LazyLock::new(|| Replacer::RegexReplace {
        regex: Regex::new(r"[^\n>](?<repl>>>)").unwrap(),
        replacement: "\u{bb}",
    });

static NUMBER_UNITS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        (?P<number>[0-9])\x20
        (?P<unit>
            [pµmcdhkMGT]?
            (?:mol|rad|Hz|Pa|Wb|lm|lx|Bq|Gy|Sv|kat|cd|Ohm|Ω|&micro;|&\#0*181;|&\#[xX]0*[Bb]5;|&Omega;|&\#0*937;|&\#[xX]0*3[Aa]9;|[mgstAKNJWCVFSTHBL])
            |
            [kKMGT]?(?:[oBb](?:ps)?|flops)
            |
            ¢|&cent;|&\#0*162;|&\#[xX]0*[Aa]2;
            |
            M?(?:£|&pound;|&\#0*163;|&\#[xX]0*[Aa]3;|¥|&yen;|&\#0*165;|&\#[xX]0*[Aa]5;|€|&euro;|&\#0*8364;|&\#[xX]0*20[Aa][Cc])
            |
            (?:°|&deg;|&\#0*176;|&\#[xX]0*[Bb]0;)[CF]?
            |
            %|pt|pi|M?px|em|en|gal|lb|[NSEOW]|[NS][EOW]|ha|mbar
        )",
    )
    .unwrap()
});

static HORIZONTAL_ELLIPSIS: LazyLock<Replacer> =
    LazyLock::new(|| Replacer::RegexReplace {
        regex: Regex::new(r"(?<repl>\.\.\.|\. \. \.)").unwrap(),
        replacement: "\u{2026}",
    });

fn replace_within_paragraphs(
    replacer: &Replacer,
    text: &mut String,
    buffer: &mut String,
) {
    let literal_regions = LiteralRegionIndex::for_typography(text);
    let Replacer::RegexSurround { regex, .. } = replacer else {
        unreachable!("Wikidot quote replacement requires a surround replacer");
    };
    let (protected_ranges, owner_ranges) = if regex.find_iter(text).next().is_some() {
        (
            typography_protected_ranges(text, true),
            typography_owner_ranges(text),
        )
    } else {
        (Vec::new(), Vec::new())
    };
    let mut output = String::with_capacity(text.len());
    let mut paragraph_start = 0;
    for paragraph in text.split_inclusive("\n\n") {
        let source_len = paragraph.len();
        let mut paragraph = paragraph.to_owned();
        replace_surround_outside_closed_iftags(
            replacer,
            &mut paragraph,
            buffer,
            paragraph_start,
            &literal_regions,
            &protected_ranges,
            &owner_ranges,
        );
        output.push_str(&paragraph);
        paragraph_start += source_len;
    }
    *text = output;
}

fn replace_low_quotes_within_paragraphs(text: &mut String, buffer: &mut String) {
    let literal_regions = LiteralRegionIndex::for_typography(text);
    let (protected_ranges, owner_ranges) =
        if LOW_DOUBLE_QUOTES.regex().find_iter(text).next().is_some() {
            (
                typography_protected_ranges(text, true),
                typography_owner_ranges(text),
            )
        } else {
            (Vec::new(), Vec::new())
        };
    let mut output = String::with_capacity(text.len());
    let mut paragraph_start = 0;
    for paragraph in text.split_inclusive("\n\n") {
        let positions = paragraph
            .match_indices(",,")
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let paired_count = positions.len() / 2 * 2;
        let protected_end = paired_count
            .checked_sub(1)
            .map_or(0, |index| positions[index] + 2);
        output.push_str(&paragraph[..protected_end]);

        let mut suffix = paragraph[protected_end..].to_owned();
        replace_surround_outside_closed_iftags(
            &LOW_DOUBLE_QUOTES,
            &mut suffix,
            buffer,
            paragraph_start + protected_end,
            &literal_regions,
            &protected_ranges,
            &owner_ranges,
        );
        output.push_str(&suffix);
        paragraph_start += paragraph.len();
    }
    *text = output;
}

fn replace_surround_outside_closed_iftags(
    replacer: &Replacer,
    text: &mut String,
    buffer: &mut String,
    source_offset: usize,
    literal_regions: &LiteralRegionIndex,
    protected_ranges: &[Range<usize>],
    owner_ranges: &[Range<usize>],
) {
    let Replacer::RegexSurround { regex, begin, end } = replacer else {
        unreachable!("Wikidot quote replacement requires a surround replacer");
    };
    let mut last_copied = 0;
    buffer.clear();
    buffer.reserve(text.len());
    for captures in regex.captures_iter(text.as_str()) {
        let full = captures.get(0).unwrap();
        if literal_regions.contains(source_offset + full.start())
            || ranges_contain(protected_ranges, source_offset + full.start())
            || ranges_contain(owner_ranges, source_offset + full.end() - 1)
        {
            continue;
        }
        if contains_closed_conditional(
            full.as_str(),
            &["iftags"],
            source_offset + full.start(),
            literal_regions,
        ) {
            continue;
        }
        let content = captures.get(1).unwrap();
        buffer.push_str(&text[last_copied..full.start()]);
        buffer.push_str(begin);
        buffer.push_str(content.as_str());
        buffer.push_str(end);
        last_copied = full.end();
    }
    if last_copied == 0 {
        return;
    }
    buffer.push_str(&text[last_copied..]);
    std::mem::swap(text, buffer);
}

fn contains_closed_conditional(
    text: &str,
    names: &[&str],
    source_offset: usize,
    literal_regions: &LiteralRegionIndex,
) -> bool {
    let lower = text.to_ascii_lowercase();
    names.iter().any(|name| {
        let opener = format!("[[{name}");
        let closer = format!("[[/{name}");
        let mut search_start = 0;
        while let Some(relative_open) = lower[search_start..].find(&opener) {
            let open = search_start + relative_open;
            let open_tail = &lower[open + opener.len()..];
            let valid_head = open_tail.chars().next().is_some_and(|character| {
                character == ']' || character.is_ascii_whitespace()
            });
            if valid_head && !literal_regions.contains(source_offset + open) {
                let mut close_search_start = 0;
                while let Some(relative_close) =
                    open_tail[close_search_start..].find(&closer)
                {
                    let close = close_search_start + relative_close;
                    let close_tail = &open_tail[close + closer.len()..];
                    let close_offset = source_offset + open + opener.len() + close;
                    if !literal_regions.contains(close_offset)
                        && close_tail.trim_start_matches([' ', '\t']).starts_with("]]")
                    {
                        return true;
                    }
                    close_search_start = close + closer.len();
                }
            }
            search_start = open + opener.len();
        }
        false
    })
}

fn wikidot_local_link_separator_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative_open) = text[cursor..].find('[') {
        let open = cursor + relative_open;
        let body_start = open + '['.len_utf8();
        if text[body_start..].starts_with('[') {
            cursor = body_start + '['.len_utf8();
            continue;
        }
        let line_end = text[body_start..]
            .find('\n')
            .map_or(text.len(), |offset| body_start + offset);
        let Some(relative_close) = text[body_start..line_end].find(']') else {
            break;
        };
        let close = body_start + relative_close;
        let body = &text[body_start..close];
        let Some(separator) = body.find(' ') else {
            cursor = close + ']'.len_utf8();
            continue;
        };
        let target = &body[..separator];
        if target.starts_with('/')
            && target.len() > 1
            && !target.contains(char::is_whitespace)
        {
            let separator = body_start + separator;
            ranges.push(separator..separator + ' '.len_utf8());
        }
        cursor = close + ']'.len_utf8();
    }
    ranges
}

fn ranges_contain(ranges: &[Range<usize>], offset: usize) -> bool {
    let insertion = ranges.partition_point(|range| range.start <= offset);
    insertion > 0 && offset < ranges[insertion - 1].end
}

fn wikidot_head_value_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative_open) = text[cursor..].find("[[") {
        let open = cursor + relative_open;
        let Some(relative_close) = text[open + 2..].find("]]") else {
            break;
        };
        let head_end = open + 2 + relative_close;
        let head = &text[open + 2..head_end];
        let lead = head.len() - head.trim_start().len();
        let trimmed = &head[lead..];
        if trimmed.starts_with('#') {
            ranges.push(open + 2 + lead + 1..head_end);
        } else {
            let name_len = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
            let name = trimmed[..name_len]
                .strip_prefix('*')
                .unwrap_or(&trimmed[..name_len]);
            if [
                "tab", "user", "file", "image", "=image", "<image", ">image", "f<image",
                "f=image", "f>image",
            ]
            .iter()
            .any(|candidate| name.eq_ignore_ascii_case(candidate))
            {
                ranges.push(open + 2 + lead + name_len..head_end);
            }
        }
        cursor = head_end + 2;
        if cursor >= text.len() {
            break;
        }
    }

    let mut cursor = 0usize;
    while let Some(relative_open) = text[cursor..].find("[#") {
        let open = cursor + relative_open;
        if open > 0 && text.as_bytes()[open - 1] == b'[' {
            cursor = open + 2;
            continue;
        }
        let mut name_start = open + 2;
        while matches!(text.as_bytes().get(name_start), Some(b' ' | b'\t')) {
            name_start += 1;
        }
        let name_end = text[name_start..]
            .find(|character: char| character.is_whitespace() || character == ']')
            .map_or(text.len(), |relative| name_start + relative);
        if name_end > name_start {
            let span_end = text[name_start..]
                .find(']')
                .map_or(text.len(), |relative| name_start + relative);
            ranges.push(name_start..span_end);
        }
        cursor = name_end.max(open + 2);
        if cursor >= text.len() {
            break;
        }
    }
    ranges
}

fn native_head_value_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative_open) = text[cursor..].find("[[") {
        let open = cursor + relative_open;
        let Some(relative_close) = text[open + 2..].find("]]") else {
            break;
        };
        let head_end = open + 2 + relative_close;
        let head = &text[open + 2..head_end];
        let lead = head.len() - head.trim_start().len();
        let trimmed = &head[lead..];
        let name_len = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
        let name = trimmed[..name_len]
            .strip_prefix('*')
            .unwrap_or(&trimmed[..name_len]);
        if ["video", "audio", "target", "anchortarget"]
            .iter()
            .any(|candidate| name.eq_ignore_ascii_case(candidate))
        {
            ranges.push(open + 2 + lead + name_len..head_end);
        }
        cursor = head_end + 2;
        if cursor >= text.len() {
            break;
        }
    }
    ranges
}

fn typography_owner_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = wikidot_head_value_ranges(text);
    ranges.extend(wikidot_block_attribute_ranges(text));
    ranges.extend(wikidot_local_link_separator_ranges(text));
    let tokenization = crate::tokenize(text);
    collect_link_ranges(tokenization.tokens(), &mut ranges);
    merge_ranges(ranges)
}

fn typography_protected_ranges(text: &str, wikidot: bool) -> Vec<Range<usize>> {
    let mut ranges = wikidot_angle_quote_protected_ranges(text, wikidot);
    ranges.extend(wikidot_inline_math_ranges(text));
    ranges.extend(wikidot_block_attribute_ranges(text));
    ranges.extend(wikidot_local_link_separator_ranges(text));
    if !wikidot {
        ranges.extend(native_head_value_ranges(text));
    }
    merge_ranges(ranges)
}

fn wikidot_inline_math_ranges(text: &str) -> Vec<Range<usize>> {
    let tokenization = crate::tokenize(text);
    let mut ranges = Vec::new();
    let mut start = None;
    for token in tokenization.tokens() {
        match token.token {
            Token::LeftMath if start.is_none() => start = Some(token.span.start),
            Token::RightMath => {
                if let Some(start) = start.take() {
                    ranges.push(start..token.span.end);
                }
            }
            // Live Wikidot keeps typography inert inside a paired math marker
            // even when a paragraph break makes that pair invalid for the
            // parser and the source later falls back to literal text. Keep the
            // lexical owner until a matching right marker or true EOF.
            Token::InputEnd => start = None,
            _ => {}
        }
    }
    ranges
}

fn wikidot_block_attribute_ranges(text: &str) -> Vec<Range<usize>> {
    if !text.contains("=\"") {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative_open) = text[cursor..].find("[[") {
        let open = cursor + relative_open;
        let head_end = text[open + 2..]
            .find("]]")
            .map_or(text.len(), |relative| open + 2 + relative + 2);
        let head = &text[open..head_end];
        let mut value_cursor = 0usize;
        while let Some(relative_delimiter) = head[value_cursor..].find("=\"") {
            let value_start = value_cursor + relative_delimiter + 2;
            let Some(relative_quote) = head[value_start..].find('"') else {
                break;
            };
            let value_end = value_start + relative_quote;
            ranges.push(open + value_start..open + value_end);
            value_cursor = value_end + 1;
        }
        cursor = head_end.max(open + 2);
        if cursor >= text.len() {
            break;
        }
    }
    ranges
}

fn digit_space_positions(text: &str) -> Vec<usize> {
    text.match_indices(' ')
        .filter_map(|(space_index, _)| {
            space_index
                .checked_sub(1)
                .and_then(|index| text.as_bytes().get(index))
                .is_some_and(u8::is_ascii_digit)
                .then_some(space_index)
        })
        .collect()
}

fn index_in_ranges(ranges: &[Range<usize>], index: usize) -> bool {
    let candidate = ranges.partition_point(|range| range.end <= index);
    ranges
        .get(candidate)
        .is_some_and(|range| range.start <= index)
}

fn replace_number_spaces(
    text: &mut String,
    protected: &[Range<usize>],
    digit_spaces: &[usize],
) -> bool {
    let mut spaces = Vec::new();
    for &space_index in digit_spaces {
        if !text
            .as_bytes()
            .get(space_index + 1)
            .is_some_and(u8::is_ascii_digit)
        {
            continue;
        }
        if index_in_ranges(protected, space_index) {
            continue;
        }
        spaces.push(space_index);
    }
    if spaces.is_empty() {
        return false;
    }

    let mut buffer = String::with_capacity(text.len() + spaces.len());
    let mut last_copied = 0;
    for space_index in spaces {
        buffer.push_str(&text[last_copied..space_index]);
        buffer.push('\u{a0}');
        last_copied = space_index + 1;
    }
    buffer.push_str(&text[last_copied..]);
    *text = buffer;
    true
}

fn replace_unit_spaces(text: &mut String, protected: &[Range<usize>]) {
    let mut buffer = String::with_capacity(text.len());
    let mut last_copied = 0;

    for captures in NUMBER_UNITS.captures_iter(text) {
        let full_match = captures.get(0).unwrap();
        let following = text[full_match.end()..].chars().next();
        if following.is_some_and(|character| character.is_ascii_alphanumeric()) {
            continue;
        }

        let number = captures.name("number").unwrap();
        let unit = captures.name("unit").unwrap();
        if index_in_ranges(protected, number.end()) {
            continue;
        }
        buffer.push_str(&text[last_copied..number.end()]);
        buffer.push('\u{a0}');
        buffer.push_str(unit.as_str());
        last_copied = full_match.end();
    }

    if last_copied > 0 {
        buffer.push_str(&text[last_copied..]);
        *text = buffer;
    }
}

pub(crate) fn wikidot_nbsp_space_positions(text: &str) -> Vec<usize> {
    let mut positions = Vec::new();
    for space_index in digit_space_positions(text) {
        if text
            .as_bytes()
            .get(space_index + 1)
            .is_some_and(u8::is_ascii_digit)
        {
            positions.push(space_index);
        }
    }
    for captures in NUMBER_UNITS.captures_iter(text) {
        let full_match = captures.get(0).unwrap();
        let following = text[full_match.end()..].chars().next();
        if following.is_some_and(|character| character.is_ascii_alphanumeric()) {
            continue;
        }
        let number = captures.name("number").unwrap();
        positions.push(number.end());
    }
    positions.sort_unstable();
    positions.dedup();
    positions
}

pub fn substitute(text: &mut String) {
    let mut buffer = String::new();
    replace_native_quotes(&NATIVE_DOUBLE_QUOTES, text, &mut buffer);
    replace_native_quotes(&NATIVE_LOW_DOUBLE_QUOTES, text, &mut buffer);
    replace_native_quotes(&NATIVE_SINGLE_QUOTES, text, &mut buffer);
    let protected_ranges = if text.contains("...") || text.contains(". . .") {
        typography_protected_ranges(text, false)
    } else {
        Vec::new()
    };
    replace_ellipsis_outside_ranges(text, &mut buffer, &protected_ranges);
}

fn replace_native_quotes(replacer: &Replacer, text: &mut String, buffer: &mut String) {
    let Replacer::RegexSurround { regex, begin, end } = replacer else {
        unreachable!("native quote replacement requires a surround replacer");
    };
    if !regex.is_match(text.as_str()) {
        return;
    }
    let mut last_copied = 0;
    let literal_regions = LiteralRegionIndex::for_typography(text);
    let protected_ranges = typography_protected_ranges(text, false);
    buffer.clear();
    buffer.reserve(text.len());
    for captures in regex.captures_iter(text.as_str()) {
        let full = captures.get(0).unwrap();
        if literal_regions.contains(full.start())
            || literal_regions.contains(full.end() - 1)
            || ranges_contain(&protected_ranges, full.start())
            || ranges_contain(&protected_ranges, full.end() - 1)
        {
            continue;
        }
        if contains_closed_conditional(
            full.as_str(),
            &["iftags", "ifcategory"],
            full.start(),
            &literal_regions,
        ) {
            continue;
        }
        let content = captures.get(1).unwrap();
        buffer.push_str(&text[last_copied..full.start()]);
        buffer.push_str(begin);
        buffer.push_str(content.as_str());
        buffer.push_str(end);
        last_copied = full.end();
    }
    if last_copied == 0 {
        return;
    }
    buffer.push_str(&text[last_copied..]);
    std::mem::swap(text, buffer);
}

/// Performs Wikidot-compatible typographic substitutions in-place.
///
/// This takes self-contained text: every character run in the input is
/// grouped using only the context visible in this call. Delayed callers
/// typograph the whole row template with this function, not individual
/// segments, so a run that a hidden conditional block would have split
/// is resolved by the seam code after suppression instead.
///
/// This is available separately for typed delayed inputs whose authored
/// segments have already passed the other layout-specific preprocessing
/// phases.
pub fn substitute_wikidot(text: &mut String) {
    substitute_wikidot_impl(text, false);
}

pub(crate) fn substitute_wikidot_deferring_conditionals(text: &mut String) {
    substitute_wikidot_impl(text, true);
}

fn substitute_wikidot_impl(text: &mut String, defer_conditionals: bool) {
    let mut buffer = String::new();
    debug!("Performing typography substitutions");

    replace_within_paragraphs(&DOUBLE_QUOTES, text, &mut buffer);
    replace_low_quotes_within_paragraphs(text, &mut buffer);
    replace_within_paragraphs(&SINGLE_QUOTES, text, &mut buffer);
    if defer_conditionals {
        replace_wikidot_angle_quotes_deferring(text, &mut buffer);
    } else {
        replace_wikidot_angle_quotes_outside_owners(text, &mut buffer);
    }

    let digit_spaces = digit_space_positions(text);
    let mut protected_ranges = typography_protected_ranges(text, true);
    if replace_number_spaces(text, &protected_ranges, &digit_spaces) {
        protected_ranges = typography_protected_ranges(text, true);
    }
    replace_unit_spaces(text, &protected_ranges);

    if defer_conditionals {
        replace_wikidot_ellipsis_deferring(text, &mut buffer);
    } else {
        replace_wikidot_ellipsis_outside_literals(text, &mut buffer);
    }
}

fn conditional_deferral_spans(text: &str) -> Vec<Range<usize>> {
    let literal_regions = LiteralRegionIndex::for_typography(text);
    let lower = text.to_ascii_lowercase();
    let mut edges: Vec<usize> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut cursor = 0usize;
    while cursor < lower.len() {
        let opener = lower[cursor..].find("[[iftags").map(|rel| cursor + rel);
        let closer = lower[cursor..].find("[[/iftags").map(|rel| cursor + rel);
        let (start, is_closer, name_len) = match (opener, closer) {
            (Some(open), Some(close)) if close < open => (close, true, "[[/iftags".len()),
            (Some(open), _) => (open, false, "[[iftags".len()),
            (None, Some(close)) => (close, true, "[[/iftags".len()),
            (None, None) => break,
        };
        let after_name = start + name_len;
        let valid_head = lower[after_name..]
            .chars()
            .next()
            .is_some_and(|character| character == ']' || character.is_ascii_whitespace());
        if !valid_head || literal_regions.contains(start) {
            cursor = after_name;
            continue;
        }
        let Some(relative_end) = lower[after_name..].find("]]") else {
            break;
        };
        let tag_end = after_name + relative_end + 2;
        if is_closer {
            if let Some(opener_end) = stack.pop() {
                edges.push(opener_end);
                edges.push(tag_end);
            }
        } else {
            stack.push(tag_end);
        }
        cursor = tag_end;
    }

    let mut spans: Vec<Range<usize>> = Vec::new();
    for edge in edges {
        let rest = &text[edge..];
        if rest.starts_with('<') {
            let len = rest.bytes().take_while(|byte| *byte == b'<').count();
            spans.push(edge..edge + len);
        } else if rest.starts_with(['.', ' ']) {
            let len = rest
                .bytes()
                .take_while(|byte| *byte == b'.' || *byte == b' ')
                .count();
            if rest[..len].contains('.') {
                spans.push(edge..edge + len);
            }
        }
    }
    spans.sort_unstable_by_key(|range| (range.start, range.end));
    spans.dedup();
    spans
}

fn replace_wikidot_angle_quotes_deferring(text: &mut String, buffer: &mut String) {
    if !text.contains("<<") && !text.contains(">>") {
        return;
    }
    let deferred: Vec<Range<usize>> = conditional_deferral_spans(text)
        .into_iter()
        .filter(|range| text[range.start..].starts_with('<'))
        .collect();
    if deferred.is_empty() {
        replace_wikidot_angle_quotes_outside_owners(text, buffer);
        return;
    }

    let protected = wikidot_angle_quote_protected_ranges(text, true);
    let source = std::mem::take(text);
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    for range in protected {
        replace_angle_quotes_in_segment_deferring(
            &source[cursor..range.start],
            cursor,
            &deferred,
            &mut output,
            buffer,
        );
        output.push_str(&source[range.clone()]);
        cursor = range.end;
    }
    replace_angle_quotes_in_segment_deferring(
        &source[cursor..],
        cursor,
        &deferred,
        &mut output,
        buffer,
    );
    *text = output;
}

fn replace_angle_quotes_in_segment_deferring(
    segment: &str,
    segment_start: usize,
    deferred: &[Range<usize>],
    output: &mut String,
    buffer: &mut String,
) {
    let mut converted = String::with_capacity(segment.len());
    let segment_end = segment_start + segment.len();
    let mut cursor = 0usize;
    for range in deferred {
        if range.end <= segment_start || range.start >= segment_end {
            continue;
        }
        let local_start = range.start.max(segment_start) - segment_start;
        let local_end = range.end.min(segment_end) - segment_start;
        if local_start < cursor {
            continue;
        }
        let mut part = segment[cursor..local_start].to_owned();
        LEFT_ANGLE_QUOTES.replace(&mut part, buffer);
        converted.push_str(&part);
        converted.push_str(&segment[local_start..local_end]);
        cursor = local_end;
    }
    let mut part = segment[cursor..].to_owned();
    LEFT_ANGLE_QUOTES.replace(&mut part, buffer);
    converted.push_str(&part);
    replace_right_angle_quotes(&mut converted, buffer);
    output.push_str(&converted);
}

fn replace_wikidot_ellipsis_deferring(text: &mut String, buffer: &mut String) {
    let mut protected_ranges = typography_protected_ranges(text, true);
    protected_ranges.extend(
        conditional_deferral_spans(text)
            .into_iter()
            .filter(|range| !text[range.start..].starts_with('<')),
    );
    let protected_ranges = merge_ranges(protected_ranges);
    replace_ellipsis_outside_ranges(text, buffer, &protected_ranges);
}

pub(crate) fn wikidot_seam_edits(text: &str, seams: &[usize]) -> Vec<WikidotSeamEdit> {
    if seams.is_empty() {
        return Vec::new();
    }

    let mut edits = Vec::new();
    let mut paired_ranges = Vec::new();
    collect_surround_seam_edits(
        &DOUBLE_QUOTES,
        text,
        seams,
        "\u{201c}",
        "\u{201d}",
        &mut paired_ranges,
        &mut edits,
    );
    collect_surround_seam_edits(
        &LOW_DOUBLE_QUOTES,
        text,
        seams,
        "\u{201e}",
        "\u{201d}",
        &mut paired_ranges,
        &mut edits,
    );
    collect_surround_seam_edits(
        &SINGLE_QUOTES,
        text,
        seams,
        "\u{2018}",
        "\u{2019}",
        &mut paired_ranges,
        &mut edits,
    );

    for candidate in LEFT_ANGLE_QUOTES.regex().find_iter(text) {
        let range = candidate.range();
        let bytes = text.as_bytes();
        let mut run_start = range.start;
        while run_start > 0 && bytes[run_start - 1] == b'<' {
            run_start -= 1;
        }
        let mut run_end = range.end;
        while run_end < bytes.len() && bytes[run_end] == b'<' {
            run_end += 1;
        }
        if seam_touches_run(run_start, run_end, seams) {
            edits.push(WikidotSeamEdit {
                range,
                replacement: "\u{ab}",
            });
        }
    }
    for captures in RIGHT_ANGLE_QUOTES.regex().captures_iter(text) {
        let candidate = captures.name("repl").unwrap();
        let full = captures.get(0).unwrap().range();
        if range_crosses_seam(&full, seams) {
            let start = candidate.start();
            let run_len = text.as_bytes()[start..]
                .iter()
                .take_while(|byte| **byte == b'>')
                .count();
            for pair in 0..run_len / 2 {
                edits.push(WikidotSeamEdit {
                    range: start + pair * 2..start + pair * 2 + 2,
                    replacement: "\u{bb}",
                });
            }
        }
    }
    for captures in HORIZONTAL_ELLIPSIS.regex().captures_iter(text) {
        let candidate = captures.name("repl").unwrap();
        let range = candidate.range();
        let bytes = text.as_bytes();
        let mut run_start = range.start;
        while run_start > 0 && matches!(bytes[run_start - 1], b'.' | b' ') {
            run_start -= 1;
        }
        let mut run_end = range.end;
        while run_end < bytes.len() && matches!(bytes[run_end], b'.' | b' ') {
            run_end += 1;
        }
        if seam_touches_run(run_start, run_end, seams) {
            edits.push(WikidotSeamEdit {
                range,
                replacement: "\u{2026}",
            });
        }
    }

    edits.sort_unstable_by_key(|edit| (edit.range.start, edit.range.end));
    edits
}

pub(crate) fn native_seam_edits(text: &str, seams: &[usize]) -> Vec<WikidotSeamEdit> {
    if seams.is_empty() {
        return Vec::new();
    }

    let mut edits = Vec::new();
    let mut paired_ranges = Vec::new();
    collect_surround_seam_edits(
        &NATIVE_DOUBLE_QUOTES,
        text,
        seams,
        "\u{201c}",
        "\u{201d}",
        &mut paired_ranges,
        &mut edits,
    );
    collect_surround_seam_edits(
        &NATIVE_LOW_DOUBLE_QUOTES,
        text,
        seams,
        "\u{201e}",
        "\u{201d}",
        &mut paired_ranges,
        &mut edits,
    );
    collect_surround_seam_edits(
        &NATIVE_SINGLE_QUOTES,
        text,
        seams,
        "\u{2018}",
        "\u{2019}",
        &mut paired_ranges,
        &mut edits,
    );
    for captures in HORIZONTAL_ELLIPSIS.regex().captures_iter(text) {
        let candidate = captures.name("repl").unwrap();
        let range = candidate.range();
        if range_crosses_seam(&range, seams) {
            edits.push(WikidotSeamEdit {
                range,
                replacement: "\u{2026}",
            });
        }
    }
    edits.sort_unstable_by_key(|edit| (edit.range.start, edit.range.end));
    edits
}

fn collect_surround_seam_edits(
    replacer: &Replacer,
    text: &str,
    seams: &[usize],
    begin: &'static str,
    end: &'static str,
    paired_ranges: &mut Vec<Range<usize>>,
    edits: &mut Vec<WikidotSeamEdit>,
) {
    let regex = replacer.regex();
    for captures in regex.captures_iter(text) {
        let full = captures.get(0).unwrap().range();
        if paired_ranges
            .iter()
            .any(|range| ranges_overlap(range, &full))
            || !range_crosses_seam(&full, seams)
        {
            continue;
        }
        let content = captures.get(1).unwrap().range();
        edits.push(WikidotSeamEdit {
            range: full.start..content.start,
            replacement: begin,
        });
        edits.push(WikidotSeamEdit {
            range: content.end..full.end,
            replacement: end,
        });
        paired_ranges.push(full);
    }
}

fn range_crosses_seam(range: &Range<usize>, seams: &[usize]) -> bool {
    let candidate = seams.partition_point(|seam| *seam <= range.start);
    seams.get(candidate).is_some_and(|seam| *seam < range.end)
}

fn seam_touches_run(start: usize, end: usize, seams: &[usize]) -> bool {
    let candidate = seams.partition_point(|seam| *seam < start);
    seams.get(candidate).is_some_and(|seam| *seam <= end)
}

fn ranges_overlap(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

fn replace_wikidot_angle_quotes_outside_owners(text: &mut String, buffer: &mut String) {
    if !text.contains("<<") && !text.contains(">>") {
        return;
    }

    let protected = wikidot_angle_quote_protected_ranges(text, true);
    if protected.is_empty() {
        LEFT_ANGLE_QUOTES.replace(text, buffer);
        replace_right_angle_quotes(text, buffer);
        return;
    }

    let source = std::mem::take(text);
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    for range in protected {
        replace_angle_quotes_in_segment(
            &source[cursor..range.start],
            &mut output,
            buffer,
        );
        output.push_str(&source[range.clone()]);
        cursor = range.end;
    }
    replace_angle_quotes_in_segment(&source[cursor..], &mut output, buffer);
    *text = output;
}

fn replace_angle_quotes_in_segment(
    segment: &str,
    output: &mut String,
    buffer: &mut String,
) {
    let mut segment = segment.to_owned();
    LEFT_ANGLE_QUOTES.replace(&mut segment, buffer);
    replace_right_angle_quotes(&mut segment, buffer);
    output.push_str(&segment);
}

fn replace_right_angle_quotes(text: &mut String, buffer: &mut String) {
    if !text.contains(">>") {
        return;
    }

    buffer.clear();
    buffer.reserve(text.len());
    let bytes = text.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'>' {
            let end = cursor
                + text[cursor..]
                    .chars()
                    .next()
                    .expect("cursor remains on a character boundary")
                    .len_utf8();
            buffer.push_str(&text[cursor..end]);
            cursor = end;
            continue;
        }

        let start = cursor;
        while cursor < bytes.len() && bytes[cursor] == b'>' {
            cursor += 1;
        }
        let count = cursor - start;
        let eligible = start > 0 && bytes[start - 1] != b'\n';
        if eligible && count >= 2 {
            for _ in 0..count / 2 {
                buffer.push('\u{00bb}');
            }
            if count % 2 != 0 {
                buffer.push('>');
            }
        } else {
            buffer.push_str(&text[start..cursor]);
        }
    }
    std::mem::swap(text, buffer);
}

fn wikidot_angle_quote_protected_ranges(text: &str, wikidot: bool) -> Vec<Range<usize>> {
    let mut ranges = LiteralRegionIndex::for_typography(text).ranges().to_vec();
    ranges.extend(wikidot_block_attribute_ranges(text));
    ranges.extend(wikidot_head_value_ranges(text));
    let tokenization = crate::tokenize(text);
    let tokens = tokenization.tokens();

    for token in tokens {
        if matches!(token.token, Token::Url | Token::UrlStar) {
            ranges.push(token.span.clone());
        }
    }

    collect_link_ranges(tokens, &mut ranges);
    ranges = merge_ranges(ranges);
    let literal_ranges = ranges.clone();
    collect_math_ranges(tokens, text, &literal_ranges, &mut ranges, wikidot);
    merge_ranges(ranges)
}

fn merge_ranges(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
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

fn collect_link_ranges(
    tokens: &[crate::parsing::ExtractedToken<'_>],
    ranges: &mut Vec<Range<usize>>,
) {
    let mut triple_start = None;
    let mut single_start = None;
    for (index, token) in tokens.iter().enumerate() {
        match token.token {
            Token::LeftLink | Token::LeftLinkStar if triple_start.is_none() => {
                triple_start = Some(token.span.start);
            }
            Token::RightLink => {
                if let Some(start) = triple_start.take() {
                    ranges.push(start..token.span.end);
                }
            }
            Token::LeftBracket
                if tokens
                    .get(index + 1)
                    .is_some_and(|next| next.token == Token::Url) =>
            {
                single_start = Some(token.span.start);
            }
            Token::RightBracket => {
                if let Some(start) = single_start.take() {
                    ranges.push(start..token.span.end);
                }
            }
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                single_start = None;
            }
            _ => {}
        }
    }
}

fn collect_math_ranges(
    tokens: &[crate::parsing::ExtractedToken<'_>],
    text: &str,
    existing: &[Range<usize>],
    ranges: &mut Vec<Range<usize>>,
    wikidot: bool,
) {
    let mut next_right_block = vec![None; tokens.len() + 1];
    let mut next = None;
    for index in (0..tokens.len()).rev() {
        if tokens[index].token == Token::RightBlock {
            next = Some(index);
        }
        next_right_block[index] = next;
    }

    let mut active = None;
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if token.token == Token::LeftBlock
            && active.is_none()
            && !index_in_ranges(existing, token.span.start)
            && block_name_after(tokens, index + 1)
                .is_some_and(|name| name.eq_ignore_ascii_case("math"))
            && (!wikidot || math_head_protects(tokens, index, text, &next_right_block))
        {
            active = Some(token.span.start);
        } else if token.token == Token::LeftBlockEnd
            && active.is_some()
            && block_name_after(tokens, index + 1)
                .is_some_and(|name| name.eq_ignore_ascii_case("math"))
            && let Some(close_index) = next_right_block[index]
        {
            let close = &tokens[close_index];
            ranges.push(active.take().unwrap()..close.span.end);
        }
        index += 1;
    }
    if let Some(start) = active {
        ranges.push(start..text.len());
    }
}

fn math_head_protects(
    tokens: &[crate::parsing::ExtractedToken<'_>],
    opener: usize,
    text: &str,
    next_right_block: &[Option<usize>],
) -> bool {
    let Some(head_close) = next_right_block.get(opener + 1).copied().flatten() else {
        return true;
    };
    let head_end = tokens[head_close].span.start;
    let mut index = opener + 1;
    while tokens
        .get(index)
        .is_some_and(|token| token.token == Token::Whitespace)
    {
        index += 1;
    }
    let Some(name_token) = tokens.get(index) else {
        return true;
    };
    if name_token.token != Token::Identifier {
        return true;
    }
    let remainder = text[name_token.span.end..head_end].trim();
    if remainder.is_empty() {
        return true;
    }
    let attr_led = |chunk: &str| {
        chunk.split_once('=').is_some_and(|(key, _)| {
            !key.is_empty()
                && key.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
                })
        })
    };
    let chunks = math_head_chunks(remainder);
    if chunks.iter().all(|chunk| attr_led(chunk)) {
        return true;
    }
    let name =
        if !chunks[0].contains('=') && chunks[1..].iter().all(|chunk| attr_led(chunk)) {
            chunks[0]
        } else {
            remainder
        };
    crate::parsing::wikidot_math_name(name)
}

fn math_head_chunks(head: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut start = None;
    let mut quoted = false;
    for (index, byte) in head.bytes().enumerate() {
        if byte == b'"' {
            quoted = !quoted;
        }
        if byte.is_ascii_whitespace() && !quoted {
            if let Some(begin) = start.take() {
                chunks.push(&head[begin..index]);
            }
        } else if start.is_none() {
            start = Some(index);
        }
    }
    if let Some(begin) = start {
        chunks.push(&head[begin..]);
    }
    chunks
}

fn block_name_after<'t>(
    tokens: &[crate::parsing::ExtractedToken<'t>],
    mut index: usize,
) -> Option<&'t str> {
    while tokens.get(index)?.token == Token::Whitespace {
        index += 1;
    }
    let token = tokens.get(index)?;
    (token.token == Token::Identifier).then_some(token.slice)
}

fn replace_wikidot_ellipsis_outside_literals(text: &mut String, buffer: &mut String) {
    let protected_ranges = typography_protected_ranges(text, true);
    replace_ellipsis_outside_ranges(text, buffer, &protected_ranges);
}

fn replace_ellipsis_outside_ranges(
    text: &mut String,
    buffer: &mut String,
    protected_ranges: &[Range<usize>],
) {
    if protected_ranges.is_empty() {
        HORIZONTAL_ELLIPSIS.replace(text, buffer);
        return;
    }

    let source = std::mem::take(text);
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    for range in protected_ranges {
        let mut authored = source[cursor..range.start].to_owned();
        HORIZONTAL_ELLIPSIS.replace(&mut authored, buffer);
        output.push_str(&authored);
        output.push_str(&source[range.clone()]);
        cursor = range.end;
    }
    let mut authored = source[cursor..].to_owned();
    HORIZONTAL_ELLIPSIS.replace(&mut authored, buffer);
    output.push_str(&authored);
    *text = output;
}

#[cfg(test)]
const TEST_CASES: [(&str, &str); 39] = [
    (
        "John laughed. ``You'll never defeat me!''\n``That's where you're wrong...''",
        "John laughed. “You'll never defeat me!”\n“That's where you're wrong…”",
    ),
    ("``outer ``inner'' outer''", "“outer ‘`inner” outer’'"),
    (
        ",,あんたは馬鹿です！''\n``Ehh?''\n,,本当！''\n[[footnoteblock]]",
        ",,あんたは馬鹿です！''\n“Ehh?”\n,,本当！''\n[[footnoteblock]]",
    ),
    ("`one\nline'", "‘one\nline’"),
    ("`one\n\nline'", "`one\n\nline'"),
    (
        "**ENTITY MAKES DRAMATIC MOTION** . . . ",
        "**ENTITY MAKES DRAMATIC MOTION** … ",
    ),
    ("Whales... they are cool", "Whales… they are cool"),
    ("Whales ... they are cool", "Whales … they are cool"),
    ("Whales. . . they are cool", "Whales… they are cool"),
    ("Whales . . . they are cool", "Whales … they are cool"),
    ("...why would you think that?", "…why would you think that?"),
    (
        "... why would you think that?",
        "… why would you think that?",
    ),
    (
        ". . .why would you think that?",
        "…why would you think that?",
    ),
    (
        ". . . why would you think that?",
        "… why would you think that?",
    ),
    ("how could you...", "how could you…"),
    ("how could you ...", "how could you …"),
    ("how could you. . .", "how could you…"),
    ("how could you . . .", "how could you …"),
    (". . .. ....", "…. …."),
    ("... . . . . . .", "… … …"),
    (".... ..", "…. .."),
    (".....", "….."),
    ("......", "……"),
    (".......", "……."),
    ("........", "…….."),
    (".........", "………"),
    ("..........", "………."),
    ("...........", "……….."),
    ("............", "…………"),
    ("... ... ...", "… … …"),
    ("... . . . ...", "… … …"),
    ("x... ...y. . . z", "x… …y… z"),
    ("<<French quotes>>", "«French quotes»"),
    ("1 234 567", "1\u{a0}234\u{a0}567"),
    ("12 kg", "12\u{a0}kg"),
    ("12 m", "12\u{a0}m"),
    ("12 Mpx", "12\u{a0}Mpx"),
    ("12 foo", "12 foo"),
    ("12 kgx", "12 kgx"),
];

#[test]
fn regexes() {
    let _ = &*SINGLE_QUOTES;
    let _ = &*DOUBLE_QUOTES;
    let _ = &*LOW_DOUBLE_QUOTES;
    let _ = &*NATIVE_SINGLE_QUOTES;
    let _ = &*NATIVE_DOUBLE_QUOTES;
    let _ = &*NATIVE_LOW_DOUBLE_QUOTES;
    let _ = &*LEFT_ANGLE_QUOTES;
    let _ = &*RIGHT_ANGLE_QUOTES;
    let _ = &*NUMBER_UNITS;
    let _ = &*HORIZONTAL_ELLIPSIS;
}

#[test]
fn test_substitute() {
    use super::test::test_substitution;

    test_substitution("typography", substitute_wikidot, &TEST_CASES);
}

#[test]
fn wikidot_typography_leaves_an_unmatched_local_bracket_literal() {
    let mut text = "1 [".to_owned();

    substitute_wikidot(&mut text);

    assert_eq!(text, "1 [");
}

#[test]
fn wikijump_substitution_keeps_native_typography_scope() {
    let mut text = "`one\nline' << 1 234".to_owned();
    substitute(&mut text);
    assert_eq!(text, "`one\nline' << 1 234");
}

#[test]
fn literal_conditional_shapes_do_not_defer_existing_quote_typography() {
    let mut text = "@@``[[iftags]]x[[/iftags]]''@@".to_owned();

    substitute_wikidot(&mut text);

    assert_eq!(text, "@@``[[iftags]]x[[/iftags]]''@@");

    let mut literal_close = "`[[iftags +missing]]@@[[/iftags]]@@'".to_owned();
    substitute_wikidot(&mut literal_close);
    assert_eq!(literal_close, "‘[[iftags +missing]]@@[[/iftags]]@@’",);
}

#[test]
fn wikidot_preprocessing_keeps_literal_dot_runs_outside_authored_prose() {
    let mut text = concat!(
        "PROSE:x....x\n",
        "SPACED:x. . .x\n",
        "[[code]]\n",
        "CODE:x....x\n",
        "[[/code]]\n",
        "@@ESCAPED:x....x@@",
    )
    .to_owned();

    crate::preprocess_for_layout(&mut text, crate::layout::Layout::Wikidot);

    assert!(text.contains("PROSE:x….x"), "{text}");
    assert!(text.contains("SPACED:x…x"), "{text}");
    assert!(text.contains("CODE:x....x"), "{text}");
    assert!(text.contains("ESCAPED:x....x"), "{text}");
}

#[test]
fn wikidot_typography_preserves_invalid_multiline_math_literal_bytes() {
    let mut text = "NEWLINES\n[[$ E =\n\nmc^2 $]]".to_owned();

    substitute_wikidot(&mut text);

    assert!(text.contains("mc^2 $]]"), "{text}");
    assert!(!text.contains("mc^2\u{00a0}$]]"), "{text}");
}

#[test]
fn wikidot_unit_typography_skips_quoted_block_attributes() {
    let mut text = concat!(
        "NUMBER: 1 2\n",
        "[[hcell style =\"padding: 0 2px; width: 75%;\"]]CELL[[/hcell]]\n",
        "[[hcell style=\"padding: 0\u{00a0}2px; width: 75%;\"]]NBSP[[/hcell]]\n",
        "PROSE: 0 2px",
    )
    .to_owned();

    crate::preprocess_for_layout(&mut text, crate::layout::Layout::Wikidot);

    assert!(text.contains("NUMBER: 1\u{00a0}2"), "{text}");
    assert!(
        text.contains(r#"style ="padding: 0 2px; width: 75%;""#),
        "{text}",
    );
    assert!(
        text.contains("style=\"padding: 0\u{00a0}2px; width: 75%;\""),
        "{text}",
    );
    assert!(text.contains("PROSE: 0\u{00a0}2px"), "{text}");
}

#[test]
fn wikidot_unit_typography_keeps_local_link_label_separators() {
    let mut text = concat!(
        "[/scp-536-fr/offset/0 V]\n",
        "[/other/12 M]\n",
        "PROSE: 0 V",
    )
    .to_owned();

    substitute_wikidot(&mut text);

    assert!(text.contains("[/scp-536-fr/offset/0 V]"), "{text}");
    assert!(text.contains("[/other/12 M]"), "{text}");
    assert!(text.contains("PROSE: 0\u{00a0}V"), "{text}");
}

#[test]
fn wikidot_inline_escape_markers_do_not_pair_across_lines() {
    let mut text = concat!(
        "UNCLOSED:@@OPEN...\n",
        "CROSS:@@OPEN\n",
        "CLOSE:END...@@ AFTER...\n",
        "CLOSED:@@KEEP...@@ CHANGE...",
    )
    .to_owned();

    crate::preprocess_for_layout(&mut text, crate::layout::Layout::Wikidot);

    assert!(text.contains("UNCLOSED:@@OPEN…"), "{text}");
    assert!(text.contains("CROSS:@@OPEN"), "{text}");
    assert!(text.contains("CLOSE:END…@@ AFTER…"), "{text}");
    assert!(text.contains("CLOSED:@@KEEP...@@ CHANGE…"), "{text}");
}

#[test]
fn wikidot_angle_quotes_respect_literal_and_link_owners() {
    let mut text = concat!(
        "PROSE:<<A>>\n",
        "[[code]]\n<<CODE>>\n[[/code]]\n",
        "[[math]]\n<<MATH>>\n[[/math]]\n",
        "[[[scp-002|<<LINK>>]]]\n",
        "[https://example.com/x <<EXTERNAL>>]\n",
        "https://example.com/a>>b\n",
        "@@<<RAW>>@@",
    )
    .to_owned();

    substitute_wikidot(&mut text);

    assert!(text.contains("PROSE:«A»"), "{text}");
    assert!(text.contains("<<CODE>>"), "{text}");
    assert!(text.contains("<<MATH>>"), "{text}");
    assert!(text.contains("[[[scp-002|<<LINK>>]]]"), "{text}");
    assert!(
        text.contains("[https://example.com/x <<EXTERNAL>>]"),
        "{text}",
    );
    assert!(text.contains("https://example.com/a>>b"), "{text}");
    assert!(text.contains("@@<<RAW>>@@"), "{text}");
}

#[test]
fn deferring_conditionals_holds_back_only_left_edge_runs() {
    use super::test::test_substitution;

    const DEFERRED_CASES: [(&str, &str); 8] = [
        (
            "<[[iftags +zz-no-such-tag]]x[[/iftags]]<<<",
            "<[[iftags +zz-no-such-tag]]x[[/iftags]]<<<",
        ),
        (
            ".[[iftags +zz-no-such-tag]]x[[/iftags]]...",
            ".[[iftags +zz-no-such-tag]]x[[/iftags]]...",
        ),
        (
            ".[[iftags +zz-no-such-tag]]x[[/iftags]].....",
            ".[[iftags +zz-no-such-tag]]x[[/iftags]].....",
        ),
        (
            "<[[iftags -zz-no-such-tag]]<<<[[/iftags]]",
            "<[[iftags -zz-no-such-tag]]<<<[[/iftags]]",
        ),
        (
            "<[[iftags -zz-no-such-tag]]x[[/iftags]]<<<",
            "<[[iftags -zz-no-such-tag]]x[[/iftags]]<<<",
        ),
        (
            "<[[iftags -zz-no-such-tag]]**b**[[/iftags]]<<<",
            "<[[iftags -zz-no-such-tag]]**b**[[/iftags]]<<<",
        ),
        (
            ". [[iftags +zz-no-such-tag]]x[[/iftags]]. . .",
            ". [[iftags +zz-no-such-tag]]x[[/iftags]]. . .",
        ),
        (
            "[[iftags +a]]1 234[[/iftags]]",
            "[[iftags +a]]1\u{a0}234[[/iftags]]",
        ),
    ];
    test_substitution(
        "deferring typography",
        substitute_wikidot_deferring_conditionals,
        &DEFERRED_CASES,
    );
}

#[test]
fn deferring_conditionals_still_converts_undeferred_runs() {
    use super::test::test_substitution;

    const CONVERTED_CASES: [(&str, &str); 9] = [
        ("[[/iftags]]...", "[[/iftags]]\u{2026}"),
        ("[[/iftags]]<<<", "[[/iftags]]\u{ab}<"),
        (
            "<[[iftags -zz-no-such-tag]]<<<",
            "<[[iftags -zz-no-such-tag]]\u{ab}<",
        ),
        (
            "[[code]][[iftags +x]][[/code]]y[[/iftags]]...",
            "[[code]][[iftags +x]][[/code]]y[[/iftags]]\u{2026}",
        ),
        (
            "@@[[iftags +x]]@@y[[/iftags]]...",
            "@@[[iftags +x]]@@y[[/iftags]]\u{2026}",
        ),
        (
            "[[ifcategory x]]y[[/ifcategory]]...",
            "[[ifcategory x]]y[[/ifcategory]]\u{2026}",
        ),
        (
            "[[iftags +a]]<<<[[iftags +b]]x[[/iftags]]",
            "[[iftags +a]]\u{ab}<[[iftags +b]]x[[/iftags]]",
        ),
        (
            "x[[iftags +a]]y[[/iftags]]<>>",
            "x[[iftags +a]]y[[/iftags]]<\u{bb}",
        ),
        (
            "x[[iftags +a]]y[[/iftags]]<<>>",
            "x[[iftags +a]]y[[/iftags]]<<\u{bb}",
        ),
    ];
    test_substitution(
        "deferring typography negatives",
        substitute_wikidot_deferring_conditionals,
        &CONVERTED_CASES,
    );
}

#[test]
fn deferring_conditionals_is_idempotent_and_public_call_undoes_deferral() {
    for source in [
        "<[[iftags +zz-no-such-tag]]x[[/iftags]]<<<",
        ".[[iftags +zz-no-such-tag]]x[[/iftags]].....",
        "<[[iftags -zz-no-such-tag]]<<<[[/iftags]]",
        "[[/iftags]]...",
        "``q'' [[iftags +a]]x[[/iftags]]...",
    ] {
        let mut once = source.to_owned();
        substitute_wikidot_deferring_conditionals(&mut once);
        let mut twice = once.clone();
        substitute_wikidot_deferring_conditionals(&mut twice);
        assert_eq!(once, twice, "deferring entry must be idempotent: {source}");

        let mut deferred_then_public = once.clone();
        substitute_wikidot(&mut deferred_then_public);
        let mut public_only = source.to_owned();
        substitute_wikidot(&mut public_only);
        assert_eq!(
            deferred_then_public, public_only,
            "public-after-preprocess must match public alone: {source}",
        );
    }

    let mut stray = "[[/iftags]]...".to_owned();
    substitute_wikidot(&mut stray);
    assert_eq!(stray, "[[/iftags]]\u{2026}");
}

#[test]
fn seam_edits_expand_runs_touching_a_seam() {
    assert_eq!(
        wikidot_seam_edits("<<<", &[0]),
        vec![WikidotSeamEdit {
            range: 0..2,
            replacement: "\u{ab}",
        }],
    );
    assert_eq!(
        wikidot_seam_edits("...", &[3]),
        vec![WikidotSeamEdit {
            range: 0..3,
            replacement: "\u{2026}",
        }],
    );
    assert_eq!(
        wikidot_seam_edits(". . .", &[0]),
        vec![WikidotSeamEdit {
            range: 0..5,
            replacement: "\u{2026}",
        }],
    );
    assert!(wikidot_seam_edits("<\u{ab}<", &[1]).is_empty());
    assert!(wikidot_seam_edits("a<<<b", &[0]).is_empty());
    assert_eq!(wikidot_seam_edits("<<<", &[3]).len(), 1);
}

#[test]
fn typography_preserves_literal_regions_and_module_bodies_in_both_layouts() {
    use crate::layout::Layout;

    for source in [
        "[[code]]\na... {...x} `q'\n[[/code]]",
        "[[html]]\n<script>f({...o})</script>\n[[/html]]",
        "[[raw]]\n...\n[[/raw]]",
        "@@ ... @@",
        "[[module CSS]]\na::after { content: \"...\"; }\n[[/module]]",
    ] {
        for layout in [Layout::Wikidot, Layout::Wikijump] {
            let mut text = source.to_owned();
            crate::preproc::preprocess_for_layout(&mut text, layout);
            assert!(!text.contains('\u{2026}'), "{layout:?}: {source:?}");
            assert!(!text.contains('\u{2018}'), "{layout:?}: {source:?}");
        }
    }

    for layout in [Layout::Wikidot, Layout::Wikijump] {
        let mut text =
            "[[module Rate]]\nWait... [[module CSS]]\n...\n[[/module]]\nend..."
                .to_owned();
        crate::preproc::preprocess_for_layout(&mut text, layout);
        assert_eq!(
            text, "[[module Rate]]\nWait… [[module CSS]]\n...\n[[/module]]\nend…",
            "{layout:?}"
        );
    }
}
