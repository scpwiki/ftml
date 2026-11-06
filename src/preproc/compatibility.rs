/*
 * preproc/compatibility.rs
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marker {
    CenterOpen,
    CenterClose,
    CollapsibleOpen,
    CollapsibleClose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IfTagsMarker {
    Open,
    Close,
}

pub(crate) const WIKIDOT_CROSSED_LIST_BREAK_SENTINEL: &str =
    "[!--ftml-crossed-list-break--]";
pub(crate) const WIKIDOT_CROSSED_OL_TAIL_SENTINEL: &str = "[!--ftml-crossed-ol-tail--]";

fn split_line(line: &str) -> (&str, &str) {
    line.strip_suffix('\n')
        .map_or((line, ""), |body| (body, "\n"))
}

fn marker_line(line: &str) -> Option<(&str, Marker)> {
    let (body, _) = split_line(line);
    let marker_start = body.find("[[")?;
    let prefix = &body[..marker_start];
    if !prefix.chars().all(|ch| matches!(ch, '>' | ' ' | '\t')) {
        return None;
    }

    let marker = body[marker_start..].to_ascii_lowercase();
    let kind = match marker.as_str() {
        "[[=]]" => Marker::CenterOpen,
        "[[/=]]" => Marker::CenterClose,
        "[[/collapsible]]" => Marker::CollapsibleClose,
        "[[collapsible]]" => Marker::CollapsibleOpen,
        marker if marker.starts_with("[[collapsible ") && marker.ends_with("]]") => {
            Marker::CollapsibleOpen
        }
        _ => return None,
    };

    Some((prefix, kind))
}

fn standalone_div_marker_line(line: &str) -> Option<(&str, bool)> {
    let (body, _) = split_line(line);
    let marker_start = body.find("[[")?;
    let prefix = &body[..marker_start];
    if !prefix.chars().all(|ch| matches!(ch, '>' | ' ' | '\t')) {
        return None;
    }
    let marker = body[marker_start..].to_ascii_lowercase();
    if marker == "[[div]]"
        || marker.starts_with("[[div ") && marker.find("]]") == Some(marker.len() - 2)
    {
        Some((prefix, true))
    } else if marker == "[[/div]]" {
        Some((prefix, false))
    } else {
        None
    }
}

fn standalone_div_marker(line: &str, prefix: &str) -> Option<bool> {
    let (line_prefix, open) = standalone_div_marker_line(line)?;
    (line_prefix == prefix).then_some(open)
}

fn centered_collapsible_opener(
    lines: &[String],
    literal_lines: &[bool],
    center_index: usize,
    prefix: &str,
) -> Option<(usize, usize)> {
    let mut index = center_index + 1;
    let mut wrapper_divs = 0;
    let required_quote_depth = prefix.bytes().filter(|&byte| byte == b'>').count();
    while index < lines.len() && !literal_lines[index] {
        let (body, _) = split_line(&lines[index]);
        let (line_quote_depth, _) = quote_depth_and_body(body);
        if line_quote_depth < required_quote_depth {
            return None;
        }
        if marker_line(&lines[index]) == Some((prefix, Marker::CollapsibleOpen)) {
            return Some((index, wrapper_divs));
        }
        if marker_line(&lines[index]).is_some() {
            return None;
        }
        match standalone_div_marker(&lines[index], prefix) {
            Some(true) => wrapper_divs += 1,
            Some(false) => return None,
            None => {}
        }
        index += 1;
    }
    None
}

fn crossed_center_close_insertion_line(
    lines: &[String],
    literal_lines: &[bool],
    collapsible_close: usize,
    prefix: &str,
    wrapper_divs: usize,
) -> Option<usize> {
    if wrapper_divs == 0 {
        return Some(collapsible_close);
    }
    let required_quote_depth = prefix.bytes().filter(|&byte| byte == b'>').count();
    let mut found = 0;
    let mut extra_div_depth = 0usize;
    let mut index = collapsible_close + 1;
    while index < lines.len() {
        if literal_lines[index] {
            return None;
        }
        let (body, _) = split_line(&lines[index]);
        let (line_quote_depth, _) = quote_depth_and_body(body);
        if line_quote_depth < required_quote_depth {
            return None;
        }
        if marker_line(&lines[index])
            .is_some_and(|(line_prefix, _)| line_prefix == prefix)
        {
            return None;
        }
        match standalone_div_marker(&lines[index], prefix) {
            Some(true) => extra_div_depth += 1,
            Some(false) => {
                if extra_div_depth > 0 {
                    extra_div_depth -= 1;
                } else {
                    found += 1;
                    if found == wrapper_divs {
                        return Some(index);
                    }
                }
            }
            None => {}
        }
        index += 1;
    }
    None
}

#[cfg(test)]
fn substitute(text: &mut String) {
    substitute_for_layout(text, false);
}

pub(super) fn substitute_for_layout(text: &mut String, wikidot: bool) {
    let mut lines = text
        .split_inclusive('\n')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let literal_lines = literal_line_mask(&lines);
    canonicalize_quote_prefixed_iftags(&mut lines, &literal_lines);

    lines = lines
        .concat()
        .split_inclusive('\n')
        .map(str::to_owned)
        .collect();
    let literal_lines = literal_line_mask(&lines);
    if !wikidot {
        canonicalize_unquoted_collapsible_closers(&mut lines, &literal_lines);
    }
    canonicalize_unmatched_quoted_tab_closers(&mut lines, &literal_lines);
    canonicalize_crossed_collapsible_div_closers(&mut lines, &literal_lines);
    if wikidot {
        canonicalize_crossed_outer_owner_closers(&mut lines, &literal_lines);
    }
    canonicalize_crossed_div_center_closers(&mut lines, &literal_lines);
    canonicalize_crossed_center_div_closers(&mut lines, &literal_lines);
    canonicalize_crossed_center_collapsible_closers(&mut lines, &literal_lines, wikidot);
    canonicalize_crossed_bold_size_closers(&mut lines, &literal_lines);
    remove_tight_quote_lines(&mut lines, &literal_lines);
    canonicalize_root_collapsible_inline_quoted_closers(&mut lines, &literal_lines);
    if wikidot {
        split_div_collapsible_div_lines(&mut lines);
        canonicalize_early_closed_div_wrapped_collapsible(&mut lines);
        canonicalize_early_closed_align_wrapped_collapsible(&mut lines);
        canonicalize_collapsible_closers_sharing_lines(&mut lines);
        close_unclosed_divs_before_collapsible_close(&mut lines);
    }

    *text = lines.concat();
}

pub(super) fn substitute_wikidot(text: &mut String) {
    let mut lines = text
        .split_inclusive('\n')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let literal_lines = literal_line_mask(&lines);
    close_unclosed_div_after_literal_iftags(&mut lines, &literal_lines);
    canonicalize_extra_block_head_closers(&mut lines, &literal_lines);
    *text = lines.concat();
}

/// Live Wikidot's footnoteblock rule (position 21) replaces the first
/// `[[footnoteblock]]` with `"\n" + footer + "\n"` before the line rules run,
/// so a marker authored on a definition-list or table line splits that line.
/// The tree hoist models the list, quote and header cases; definition lists and
/// tables depend on which line fragments the line rules then claim, so the
/// split happens at the source level here.
pub(super) fn inject_wikidot_footnote_block_lines(text: &mut String) {
    let Some((start, end)) = first_footnoteblock_range(text) else {
        return;
    };
    let literal_ranges = super::whitespace::wikidot_literal_body_ranges(text);
    if literal_ranges
        .iter()
        .any(|range| range.contains(&start) || range.contains(&end))
    {
        return;
    }

    let line_start = text[..start].rfind('\n').map_or(0, |index| index + 1);
    let line_end = text[end..]
        .find('\n')
        .map_or(text.len(), |index| end + index);
    let line = &text[line_start..line_end];
    let trimmed = line.trim_start_matches([' ', '\t']);
    let definition_line = trimmed.starts_with(": ");
    let table_line = trimmed.starts_with("||");
    if !definition_line && !table_line {
        return;
    }

    let mut output = String::with_capacity(text.len() + 2);
    output.push_str(&text[..start]);
    output.push('\n');
    output.push_str(&text[start..end]);
    if table_line {
        output.push('\n');
    }
    output.push_str(&text[end..]);
    *text = output;
}

/// Byte range of the first `[[footnoteblock]]` or `[[footnoteblock args]]`.
fn first_footnoteblock_range(text: &str) -> Option<(usize, usize)> {
    const NEEDLE: &[u8] = b"[[footnoteblock";
    let bytes = text.as_bytes();
    let mut cursor = 0usize;
    while cursor + NEEDLE.len() <= bytes.len() {
        let relative = bytes[cursor..]
            .windows(NEEDLE.len())
            .position(|window| window.eq_ignore_ascii_case(NEEDLE))?;
        let start = cursor + relative;
        let after_name = start + NEEDLE.len();
        let ends_name = matches!(
            bytes.get(after_name),
            Some(b' ' | b'\t' | b'\r' | b'\n' | b']')
        );
        if !ends_name {
            cursor = after_name;
            continue;
        }
        let tail = &text[after_name..];
        let relative_close = tail.find("]]")?;
        return Some((start, after_name + relative_close + 2));
    }
    None
}

fn canonicalize_extra_block_head_closers(lines: &mut [String], literal_lines: &[bool]) {
    const OWNERS: &[&str] = &[
        "bibliography",
        "button",
        "collapsible",
        "date",
        "eref",
        "file",
        "footnote",
        "iframe",
        "image",
        "ol",
        "size",
        "span",
        "toc",
        "ul",
        "user",
    ];
    const BARRIER: &str = "]][!--ftml-residual-bracket--]]";

    for (line, literal) in lines.iter_mut().zip(literal_lines) {
        if *literal {
            continue;
        }
        let (body, ending) = split_line(line);
        let Some(open) = body.find("[[") else {
            continue;
        };
        if body[open..].starts_with("[[[") {
            continue;
        }
        let head = &body[open + 2..];
        let name_end = head
            .find(|character: char| {
                character.is_ascii_whitespace() || matches!(character, ']' | '_')
            })
            .unwrap_or(head.len());
        let name = &head[..name_end];
        if head[name_end..].starts_with("_]]]") {
            continue;
        }
        if !OWNERS.iter().any(|owner| name.eq_ignore_ascii_case(owner)) {
            continue;
        }
        let Some(close) = head.find("]]") else {
            continue;
        };
        let close = open + 2 + close;
        if body.as_bytes().get(close + 2) != Some(&b']') {
            continue;
        }
        let mut rewritten = String::with_capacity(line.len() + BARRIER.len());
        rewritten.push_str(&body[..close]);
        rewritten.push_str(BARRIER);
        rewritten.push_str(&body[close + 3..]);
        rewritten.push_str(ending);
        *line = rewritten;
    }
}

fn close_unclosed_div_after_literal_iftags(
    lines: &mut Vec<String>,
    literal_lines: &[bool],
) {
    if lines.len() != 5 || literal_lines.iter().any(|literal| *literal) {
        return;
    }
    let opener = split_line(&lines[0]).0.to_ascii_lowercase();
    let div = split_line(&lines[1]).0;
    let body = split_line(&lines[2]).0;
    let malformed_close = split_line(&lines[3]).0;
    let visible = split_line(&lines[4]).0;
    if !opener.starts_with("[[iftags ")
        || !opener.ends_with("]]")
        || !div.eq_ignore_ascii_case("[[div]]")
        || body.contains("[[")
        || !malformed_close.eq_ignore_ascii_case("[[/iftags_]]")
        || visible.contains("[[")
    {
        return;
    }
    if lines.last().is_some_and(|line| !line.ends_with('\n')) {
        lines.push("\n".to_owned());
    }
    lines.push("[[/div]]".to_owned());
}

fn canonicalize_quote_prefixed_iftags(lines: &mut [String], literal_lines: &[bool]) {
    let mut openers = Vec::new();
    let mut pairs = Vec::new();

    for (index, (line, literal)) in lines.iter().zip(literal_lines).enumerate() {
        if *literal {
            continue;
        }

        let Some((prefix, marker)) = iftags_marker_line(line) else {
            continue;
        };

        match marker {
            IfTagsMarker::Open => openers.push((index, prefix.contains('>'))),
            IfTagsMarker::Close => {
                if let Some((open_index, quoted)) = openers.pop()
                    && quoted
                {
                    pairs.push((open_index, index));
                }
            }
        }
    }

    for (open_index, close_index) in pairs {
        let open_prefix = iftags_marker_line(&lines[open_index])
            .expect("paired iftags opener remains a marker line")
            .0;
        let shape = quote_prefix_shape(open_prefix);

        if shape.tight {
            hoist_tight_iftags_marker(&mut lines[open_index]);
            hoist_tight_iftags_marker(&mut lines[close_index]);
        } else if shape.spaced_inner {
            preserve_spaced_inner_marker(&mut lines[open_index], &shape.native_prefix);
            preserve_spaced_inner_marker(&mut lines[close_index], &shape.native_prefix);
        } else {
            let close_prefix = iftags_marker_line(&lines[close_index])
                .expect("paired iftags closer remains a marker line")
                .0;
            if close_prefix != shape.native_prefix {
                normalize_iftags_marker_prefix(
                    &mut lines[close_index],
                    &shape.native_prefix,
                );
            }
        }
    }
}

#[derive(Debug)]
struct QuotePrefixShape {
    native_prefix: String,
    spaced_inner: bool,
    tight: bool,
}

fn quote_prefix_shape(prefix: &str) -> QuotePrefixShape {
    let indentation_len = prefix
        .bytes()
        .take_while(|byte| matches!(byte, b' ' | b'\t'))
        .count();
    let quote_run = prefix[indentation_len..]
        .bytes()
        .take_while(|byte| *byte == b'>')
        .count();
    let after_quote_run = &prefix[indentation_len + quote_run..];
    let tight = !after_quote_run.starts_with([' ', '\t']);
    let spaced_inner = !tight
        && after_quote_run
            .trim_start_matches([' ', '\t'])
            .contains('>');
    let native_prefix_end = if tight {
        indentation_len
    } else {
        indentation_len
            + quote_run
            + after_quote_run
                .bytes()
                .take_while(|byte| matches!(byte, b' ' | b'\t'))
                .count()
    };

    QuotePrefixShape {
        native_prefix: prefix[..native_prefix_end].to_owned(),
        spaced_inner,
        tight,
    }
}

fn iftags_marker_line(line: &str) -> Option<(&str, IfTagsMarker)> {
    let (body, _) = split_line(line);
    let marker_start = body.find("[[")?;
    let prefix = &body[..marker_start];
    if !prefix.chars().all(|ch| matches!(ch, '>' | ' ' | '\t')) {
        return None;
    }

    let marker = body[marker_start..].trim_end_matches([' ', '\t', '\r']);
    let inner = marker.strip_prefix("[[")?.strip_suffix("]]")?.trim();
    let (kind, head) = match inner.strip_prefix('/') {
        Some(head) => (IfTagsMarker::Close, head.trim()),
        None => (IfTagsMarker::Open, inner),
    };
    let name = head.split_ascii_whitespace().next()?;
    let name = if kind == IfTagsMarker::Close {
        name.strip_suffix('_').unwrap_or(name)
    } else {
        name
    };
    if !name.eq_ignore_ascii_case("iftags") {
        return None;
    }
    if kind == IfTagsMarker::Close && head.split_ascii_whitespace().nth(1).is_some() {
        return None;
    }

    Some((prefix, kind))
}

fn marker_and_ending(line: &str) -> (&str, &str) {
    let (body, ending) = split_line(line);
    let marker_start = body
        .find("[[")
        .expect("matched iftags line contains a block marker");
    (&body[marker_start..], ending)
}

fn hoist_tight_iftags_marker(line: &mut String) {
    let (body, _) = split_line(line);
    let indentation_len = body
        .bytes()
        .take_while(|byte| matches!(byte, b' ' | b'\t'))
        .count();
    let indentation = body[..indentation_len].to_owned();
    let (marker, ending) = marker_and_ending(line);
    *line = format!("{indentation}{marker}{ending}");
}

fn preserve_spaced_inner_marker(line: &mut String, native_prefix: &str) {
    let (body, _) = split_line(line);
    let marker_start = body
        .find("[[")
        .expect("matched iftags line contains a block marker");
    let residual = body[..marker_start].trim_end().to_owned();
    let native_prefix = native_prefix.to_owned();
    let (marker, ending) = marker_and_ending(line);
    *line = format!("{residual}\n{native_prefix}{marker}{ending}");
}

fn normalize_iftags_marker_prefix(line: &mut String, native_prefix: &str) {
    let native_prefix = native_prefix.to_owned();
    let (marker, ending) = marker_and_ending(line);
    *line = format!("{native_prefix}{marker}{ending}");
}

/// Move a root collapsible's closer out of a quoted content line.
///
/// Wikidot pages commonly put an unquoted collapsible opener before a quoted
/// report and append the closer to the final quoted line. Wikidot closes the
/// root collapsible after that line; leaving the marker inside the native
/// blockquote makes the tree parser treat each following collapsible as a
/// child and repeatedly reparse the remaining page.
fn canonicalize_root_collapsible_inline_quoted_closers(
    lines: &mut Vec<String>,
    literal_lines: &[bool],
) {
    const CLOSE: &str = "[[/collapsible]]";

    let mut opener_depths = Vec::new();
    let mut output = Vec::with_capacity(lines.len());

    for (line, literal) in std::mem::take(lines).into_iter().zip(literal_lines) {
        let (body, ending) = split_line(&line);
        let (quote_depth, quoted_body) = quote_depth_and_body(body);

        if !*literal {
            if marker_line(&line)
                .is_some_and(|(_, marker)| marker == Marker::CollapsibleOpen)
            {
                opener_depths.push(quote_depth);
            } else if marker_line(&line)
                .is_some_and(|(_, marker)| marker == Marker::CollapsibleClose)
            {
                opener_depths.pop();
            } else if quote_depth > 0 && opener_depths.last() == Some(&0) {
                let trimmed = quoted_body.trim_end_matches([' ', '\t']);
                let has_close = trimmed.len() >= CLOSE.len()
                    && trimmed
                        .get(trimmed.len() - CLOSE.len()..)
                        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(CLOSE));
                if !has_close {
                    output.push(line);
                    continue;
                }
                let trailing = quoted_body.len() - trimmed.len();
                let close_start = body.len() - trailing - CLOSE.len();
                output.push(format!("{}{ending}", &body[..close_start]));
                output.push(format!("{CLOSE}{ending}"));
                opener_depths.pop();
                continue;
            }
        }

        output.push(line);
    }

    *lines = output;
}

fn line_opens_collapsible(line: &str) -> bool {
    let (body, _) = split_line(line);
    let lower = body.to_ascii_lowercase();
    let Some(start) = lower.find("[[collapsible") else {
        return false;
    };
    let after = &lower[start + "[[collapsible".len()..];
    after
        .chars()
        .next()
        .is_some_and(|ch| matches!(ch, ' ' | '\t' | ']' | '_' | '*'))
        && after.contains("]]")
}

fn resplit_lines(lines: &mut Vec<String>) -> Vec<bool> {
    *lines = lines
        .concat()
        .split_inclusive('\n')
        .map(str::to_owned)
        .collect();
    literal_line_mask(lines)
}

fn split_div_collapsible_div_lines(lines: &mut Vec<String>) {
    let literal_lines = resplit_lines(lines);
    for (line, literal) in lines.iter_mut().zip(literal_lines.iter()) {
        if *literal {
            continue;
        }
        let (body, ending) = split_line(line);
        let trimmed = body.trim();
        let lower = trimmed.to_ascii_lowercase();
        if !lower.starts_with("[[div") {
            continue;
        }
        let Some(div_head_end) = trimmed.find("]]") else {
            continue;
        };
        let rest = trimmed[div_head_end + 2..].trim_start();
        let rest_lower = rest.to_ascii_lowercase();
        if !rest_lower.starts_with("[[collapsible") {
            continue;
        }
        let Some(head_end) = rest.find("]]") else {
            continue;
        };
        if rest[head_end + 2..].trim() != "[[/div]]" {
            continue;
        }
        let div_head = &trimmed[..div_head_end + 2];
        let collapsible_head = &rest[..head_end + 2];
        *line = format!("{div_head}\n{collapsible_head}\n[[/div]]{ending}");
    }
}

fn canonicalize_early_closed_div_wrapped_collapsible(lines: &mut Vec<String>) {
    let literal_lines = resplit_lines(lines);
    let lines: &mut [String] = lines;
    let literal_lines: &[bool] = &literal_lines;
    let mut index = 0;
    while index + 3 < lines.len() {
        if literal_lines[index] {
            index += 1;
            continue;
        }
        let (body, _) = split_line(&lines[index]);
        let Some(marker_start) = body.find("[[") else {
            index += 1;
            continue;
        };
        let prefix = body[..marker_start].to_owned();
        if standalone_div_marker(&lines[index], &prefix) != Some(true)
            || literal_lines[index + 1]
            || standalone_named_block_marker(&lines[index + 1], &prefix, "collapsible")
                != Some(true)
            || literal_lines[index + 2]
            || standalone_div_marker(&lines[index + 2], &prefix) != Some(false)
            || literal_lines[index + 3]
            || standalone_div_marker(&lines[index + 3], &prefix) != Some(true)
        {
            index += 1;
            continue;
        }

        let mut close = index + 4;
        let mut collapsible_close = None;
        while close < lines.len() {
            if literal_lines[close]
                || standalone_named_block_marker(&lines[close], &prefix, "collapsible")
                    == Some(true)
                || standalone_div_marker(&lines[close], &prefix).is_some()
            {
                break;
            }
            if standalone_named_block_marker(&lines[close], &prefix, "collapsible")
                == Some(false)
            {
                collapsible_close = Some(close);
                break;
            }
            close += 1;
        }
        let Some(collapsible_close) = collapsible_close else {
            index += 1;
            continue;
        };

        let mut div_close = collapsible_close + 1;
        while div_close < lines.len()
            && !literal_lines[div_close]
            && split_line(&lines[div_close]).0.trim().is_empty()
        {
            div_close += 1;
        }
        if div_close >= lines.len()
            || literal_lines[div_close]
            || standalone_div_marker(&lines[div_close], &prefix) != Some(false)
        {
            index += 1;
            continue;
        }

        let early_ending = split_line(&lines[index + 2]).1.to_owned();
        lines[index + 2] = format!("{prefix}{early_ending}");
        let close_ending = split_line(&lines[collapsible_close]).1.to_owned();
        lines[collapsible_close] =
            format!("{prefix}[[/div]]\n{prefix}[[/collapsible]]{close_ending}");
        index = div_close + 1;
    }
}

fn canonicalize_early_closed_align_wrapped_collapsible(lines: &mut Vec<String>) {
    let literal_lines = resplit_lines(lines);
    let lines: &mut [String] = lines;
    let literal_lines: &[bool] = &literal_lines;
    for align in [">", "<"] {
        let mut index = 0;
        while index + 2 < lines.len() {
            if literal_lines[index] {
                index += 1;
                continue;
            }
            let (body, _) = split_line(&lines[index]);
            let Some(marker_start) = body.find("[[") else {
                index += 1;
                continue;
            };
            let prefix = body[..marker_start].to_owned();
            if standalone_named_block_marker(&lines[index], &prefix, align) != Some(true)
                || literal_lines[index + 1]
                || !line_opens_collapsible(&lines[index + 1])
                || literal_lines[index + 2]
                || standalone_named_block_marker(&lines[index + 2], &prefix, align)
                    != Some(false)
            {
                index += 1;
                continue;
            }

            let mut close = index + 3;
            let mut collapsible_close = None;
            while close < lines.len() {
                if literal_lines[close]
                    || standalone_named_block_marker(&lines[close], &prefix, align)
                        == Some(false)
                {
                    break;
                }
                if standalone_named_block_marker(&lines[close], &prefix, "collapsible")
                    == Some(false)
                {
                    collapsible_close = Some(close);
                    break;
                }
                close += 1;
            }
            let Some(collapsible_close) = collapsible_close else {
                index += 1;
                continue;
            };
            if collapsible_close == 0 || collapsible_close + 1 >= lines.len() {
                index += 1;
                continue;
            }
            let Some(reopened) = [">", "<"].into_iter().find(|name| {
                !literal_lines[collapsible_close - 1]
                    && !literal_lines[collapsible_close + 1]
                    && standalone_named_block_marker(
                        &lines[collapsible_close - 1],
                        &prefix,
                        name,
                    ) == Some(true)
                    && standalone_named_block_marker(
                        &lines[collapsible_close + 1],
                        &prefix,
                        name,
                    ) == Some(false)
            }) else {
                index += 1;
                continue;
            };

            let early_ending = split_line(&lines[index + 2]).1.to_owned();
            lines[index + 2] = format!("{prefix}{early_ending}");
            let close_ending = split_line(&lines[collapsible_close]).1.to_owned();
            lines[collapsible_close] = format!("{prefix}[[/{reopened}]]{close_ending}");
            let after_ending = split_line(&lines[collapsible_close + 1]).1.to_owned();
            lines[collapsible_close + 1] =
                format!("{prefix}[[/collapsible]]\n{prefix}[[/{align}]]{after_ending}");
            index = collapsible_close + 2;
        }
    }
}

fn canonicalize_collapsible_closers_sharing_lines(lines: &mut Vec<String>) {
    let literal_lines = resplit_lines(lines);
    let mut open_depths: Vec<(usize, bool)> = Vec::new();
    let mut output = Vec::with_capacity(lines.len());
    for (line_number, (line, literal)) in
        lines.iter().zip(literal_lines.iter()).enumerate()
    {
        let (body, ending) = split_line(line);
        if *literal {
            output.push(line.clone());
            continue;
        }
        let (quote_depth, _) = quote_depth_and_body(body);
        // L8.5: pipe-headed openers pair with their quoted closers
        // directly (render-interior flat); stripping the quote would
        // forge a drop-shape pair, so they never unquote here.
        let pipe_headed = body.trim_start_matches([' ', '\t']).starts_with("||");
        match marker_line(line) {
            Some((_, Marker::CollapsibleOpen)) => {
                open_depths.push((quote_depth, pipe_headed));
                output.push(line.clone());
            }
            Some((_, Marker::CollapsibleClose)) => {
                // L8.5: a quoted closer with live content after it in
                // its quote run keeps its quote so the run splits and
                // ejects (live q-coll-close shape); only run-final
                // closers unquote to their root opener.
                let mut run_has_content = false;
                if quote_depth > 0 {
                    for following in
                        lines.iter().skip(line_number + 1).take_while(|next| {
                            let (next_body, _) = split_line(next);
                            let (next_depth, _) = quote_depth_and_body(next_body);
                            next_depth > 0
                        })
                    {
                        let (following_body, _) = split_line(following);
                        let (_, quoted_body) = quote_depth_and_body(following_body);
                        if !quoted_body.trim_matches([' ', '\t']).is_empty() {
                            run_has_content = true;
                            break;
                        }
                    }
                }
                let unquote = quote_depth > 0
                    && open_depths.last() == Some(&(0, false))
                    && !run_has_content;
                open_depths.pop();
                if unquote {
                    output.push(format!("[[/collapsible]]{ending}"));
                } else {
                    output.push(line.clone());
                }
            }
            _ => {
                if quote_depth == 0
                    && open_depths.last().is_some_and(|&(depth, _)| depth == 0)
                    && split_list_item_collapsible_close(&mut output, body, ending)
                {
                    open_depths.pop();
                    continue;
                }
                let lower = body.to_ascii_lowercase();
                if line_opens_collapsible(line) && !lower.contains("[[/collapsible]]") {
                    open_depths.push((quote_depth, pipe_headed));
                } else if lower.contains("[[/collapsible]]") {
                    open_depths.pop();
                }
                output.push(line.clone());
            }
        }
    }
    *lines = output;
}

fn split_list_item_collapsible_close(
    output: &mut Vec<String>,
    body: &str,
    ending: &str,
) -> bool {
    const CLOSE: &str = "[[/collapsible]]";
    let trimmed = body.trim_end();
    if !trimmed.to_ascii_lowercase().ends_with(CLOSE) {
        return false;
    }
    let Some(head_raw) = trimmed.get(..trimmed.len() - CLOSE.len()) else {
        return false;
    };
    let head = head_raw.trim_end();
    let content = head.trim_start();
    let Some(marker_end) = content.find(|ch| ch != '*' && ch != '#') else {
        return false;
    };
    if marker_end == 0 {
        return false;
    }
    let item_text = &content[marker_end..];
    if !item_text.starts_with([' ', '\t']) || item_text.trim().is_empty() {
        return false;
    }
    output.push(format!("{head}{ending}"));
    output.push(format!("{CLOSE}{ending}"));
    true
}

fn close_unclosed_divs_before_collapsible_close(lines: &mut Vec<String>) {
    let literal_lines = resplit_lines(lines);
    let mut div_depth = 0usize;
    let mut baselines: Vec<usize> = Vec::new();
    let mut output = Vec::with_capacity(lines.len());
    for (index, (line, literal)) in lines.iter().zip(literal_lines.iter()).enumerate() {
        if !*literal {
            let (body, _) = split_line(line);
            let trimmed = body.trim_end();
            match marker_line(trimmed) {
                Some((_, Marker::CollapsibleOpen)) => {
                    baselines.push(div_depth);
                }
                Some((_, Marker::CollapsibleClose)) => {
                    if let Some(baseline) = baselines.pop() {
                        // Rescue only tight nesting tails (a div re-close on
                        // the very next line). Crossings (content between, or
                        // no re-close at all) go to the flat path, which
                        // pairs the div across the collapsible; synthesizing
                        // a closer would defeat that pairing (live evidence:
                        // q2-cd-own, q2-d-unclosed-in-c keep/cross the div).
                        let tight_tail = !literal_lines
                            .get(index + 1)
                            .is_some_and(|literal| *literal)
                            && lines.get(index + 1).is_some_and(|next| {
                                let (body, _) = split_line(next);
                                standalone_div_marker(body.trim_end(), "") == Some(false)
                            });
                        if tight_tail {
                            while div_depth > baseline {
                                output.push("[[/div]]\n".to_owned());
                                div_depth -= 1;
                            }
                        }
                    }
                }
                _ => {
                    if standalone_div_marker(trimmed, "") == Some(true) {
                        div_depth += 1;
                    } else if standalone_div_marker(trimmed, "") == Some(false) {
                        div_depth = div_depth.saturating_sub(1);
                    } else if line_opens_collapsible(line) {
                        baselines.push(div_depth);
                    }
                }
            }
        }
        output.push(line.clone());
    }
    *lines = output;
}

fn remove_tight_quote_lines(lines: &mut [String], literal_lines: &[bool]) {
    const MAX_SYNTHESIZED_QUOTE_DEPTH: usize = 30;

    let mut active_quote_depth = 0;
    for (line, literal) in lines.iter_mut().zip(literal_lines) {
        if *literal {
            let (body, _) = split_line(line);
            let depth = valid_quote_depth(body);
            if depth > 0 {
                active_quote_depth = depth;
            }
            continue;
        }

        let (body, ending) = split_line(line);
        let trimmed = body.trim_start_matches([' ', '\t']);
        if !trimmed.starts_with('>') {
            active_quote_depth = 0;
            continue;
        }
        let valid_depth = valid_quote_depth(body);
        if valid_depth > 0 {
            active_quote_depth = valid_depth;
            continue;
        }

        let indentation = body
            .strip_suffix(trimmed)
            .expect("trimmed quote body must remain a suffix of the source line");
        // Live extracts a footnote body before a tight quote line is dropped,
        // so the body survives even though the line renders nothing. Keep a
        // tight line that opens with a footnote reference in the spaced-quote
        // shape the footnote-first ownership rule understands: the parser then
        // drops the reference with the row and leaves the body registered.
        let marker_depth = trimmed.bytes().take_while(|byte| *byte == b'>').count();
        if trimmed[marker_depth..].starts_with(FOOTNOTE_HEAD) {
            let depth = marker_depth
                .max(active_quote_depth)
                .max(1)
                .min(MAX_SYNTHESIZED_QUOTE_DEPTH);
            *line = format!(
                "{indentation}{} {}{ending}",
                ">".repeat(depth),
                &trimmed[marker_depth..],
            );
            active_quote_depth = depth;
            continue;
        }
        if active_quote_depth == 0 {
            *line = ending.to_owned();
        } else {
            let synthesized_depth = active_quote_depth.min(MAX_SYNTHESIZED_QUOTE_DEPTH);
            *line = format!("{indentation}{}{ending}", ">".repeat(synthesized_depth));
        }
    }
}

/// A footnote reference head that opens a tight quote line. The whitespace
/// before a reference is removed before this pass runs, so a quote marker
/// directly followed by the reference no longer forms a valid quote depth.
const FOOTNOTE_HEAD: &str = "[[footnote]]";

fn valid_quote_depth(body: &str) -> usize {
    let body = body.trim_start_matches([' ', '\t']);
    let depth = body.bytes().take_while(|byte| *byte == b'>').count();
    let rest = &body[depth..];
    if depth > 0 && (rest.is_empty() || rest.starts_with([' ', '\t', '\r'])) {
        depth
    } else {
        0
    }
}

fn canonicalize_crossed_bold_size_closers(lines: &mut [String], literal_lines: &[bool]) {
    const OPEN: &str = "**[[size ";
    const CROSSED_CLOSE: &str = "**[[/size]]";

    for (line, literal) in lines.iter_mut().zip(literal_lines) {
        if *literal {
            continue;
        }

        let mut search_from = 0;
        while let Some(open_start) = line[search_from..].find(OPEN) {
            let open_start = search_from + open_start;
            let Some(open_end) = line[open_start + OPEN.len()..].find("]]") else {
                break;
            };
            let body_start = open_start + OPEN.len() + open_end + 2;
            let Some(close_start) = line[body_start..].find(CROSSED_CLOSE) else {
                break;
            };
            let close_start = body_start + close_start;

            if line[body_start..close_start].contains("**") {
                search_from = body_start;
                continue;
            }

            line.replace_range(
                close_start..close_start + CROSSED_CLOSE.len(),
                "[[/size]]**",
            );
            search_from = close_start + "[[/size]]**".len();
        }
    }
}

#[derive(Debug)]
struct QuotedCollapsible {
    prefix: String,
    quote_depth: usize,
}

fn canonicalize_unquoted_collapsible_closers(
    lines: &mut [String],
    literal_lines: &[bool],
) {
    let mut openers: Vec<QuotedCollapsible> = Vec::new();

    for (index, line) in lines.iter_mut().enumerate() {
        let (body, ending) = split_line(line);
        let (line_quote_depth, _) = quote_depth_and_body(body);

        if literal_lines[index] {
            while openers
                .last()
                .is_some_and(|opener| opener.quote_depth > line_quote_depth)
            {
                openers.pop();
            }
            continue;
        }

        if body.trim().is_empty() {
            openers.clear();
            continue;
        }

        let marker = marker_line(line);
        if marker == Some(("", Marker::CollapsibleClose)) {
            if let Some(opener) = openers.pop() {
                *line = format!("{}[[/collapsible]]{ending}", opener.prefix);
            }
            continue;
        }

        while openers
            .last()
            .is_some_and(|opener| opener.quote_depth > line_quote_depth)
        {
            openers.pop();
        }

        match marker {
            Some((prefix, Marker::CollapsibleOpen)) => {
                let quote_depth =
                    prefix.chars().filter(|&character| character == '>').count();
                if quote_depth > 0 {
                    openers.push(QuotedCollapsible {
                        prefix: prefix.to_owned(),
                        quote_depth,
                    });
                }
            }
            Some((prefix, Marker::CollapsibleClose)) => {
                while let Some(opener) = openers.last() {
                    if opener.quote_depth < line_quote_depth {
                        break;
                    }
                    let matching_prefix = opener.prefix == prefix;
                    openers.pop();
                    if matching_prefix {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
}

#[derive(Debug)]
struct CrossedClose {
    prefix: String,
    quote_depth: usize,
    wrapper_divs: usize,
    early_center_close: Option<usize>,
    reopened_center: Option<usize>,
    nested_collapsible_depth: usize,
}

/// Repair a collapsible whose wrapper div is closed one line too late.
///
/// Wikidot accepts `collapsible -> div -> /collapsible -> /div` as if the
/// final two markers were in stack order. Repeated instances otherwise make
/// the tree parser reconsider the remaining siblings as nested collapsibles.
fn canonicalize_crossed_collapsible_div_closers(
    lines: &mut [String],
    literal_lines: &[bool],
) {
    let mut index = 0;
    while index + 1 < lines.len() {
        if literal_lines[index] {
            index += 1;
            continue;
        }
        let Some((prefix, Marker::CollapsibleOpen)) = marker_line(&lines[index]) else {
            index += 1;
            continue;
        };
        let prefix = prefix.to_owned();
        let required_quote_depth = prefix.bytes().filter(|&byte| byte == b'>').count();

        let mut div_open = index + 1;
        let div_found = loop {
            if div_open >= lines.len() {
                break false;
            }
            let (body, _) = split_line(&lines[div_open]);
            let (line_quote_depth, _) = quote_depth_and_body(body);
            if line_quote_depth < required_quote_depth {
                break false;
            }
            if !literal_lines[div_open] {
                match marker_line(&lines[div_open]) {
                    Some((line_prefix, Marker::CollapsibleOpen))
                    | Some((line_prefix, Marker::CollapsibleClose))
                    | Some((line_prefix, Marker::CenterOpen))
                    | Some((line_prefix, Marker::CenterClose))
                        if line_prefix == prefix =>
                    {
                        break false;
                    }
                    _ => match standalone_div_marker(&lines[div_open], &prefix) {
                        Some(true) => break true,
                        Some(false) => break false,
                        None => {}
                    },
                }
            }
            div_open += 1;
        };
        if !div_found {
            index += 1;
            continue;
        }

        let search_end = lines.len();
        let mut collapsible_depth = 1;
        let mut div_depth = 1;
        let mut close = div_open + 1;
        let mut rewritten = false;
        while close < search_end {
            let (body, _) = split_line(&lines[close]);
            let (line_quote_depth, _) = quote_depth_and_body(body);
            if line_quote_depth < required_quote_depth {
                break;
            }
            if literal_lines[close] {
                close += 1;
                continue;
            }

            match marker_line(&lines[close]) {
                Some((line_prefix, Marker::CollapsibleOpen)) if line_prefix == prefix => {
                    collapsible_depth += 1;
                }
                Some((line_prefix, Marker::CollapsibleClose))
                    if line_prefix == prefix =>
                {
                    if collapsible_depth > 1 {
                        collapsible_depth -= 1;
                    } else if div_depth == 1 {
                        let div_close = close + 1;
                        if div_close < lines.len()
                            && !literal_lines[div_close]
                            && standalone_div_marker(&lines[div_close], &prefix)
                                == Some(false)
                        {
                            let collapsible_ending =
                                split_line(&lines[close]).1.to_owned();
                            let div_ending = split_line(&lines[div_close]).1.to_owned();
                            lines[close] =
                                format!("{prefix}[[/div]]{collapsible_ending}");
                            lines[div_close] =
                                format!("{prefix}[[/collapsible]]{div_ending}");
                            index = div_close + 1;
                            rewritten = true;
                        }
                        break;
                    }
                }
                _ => match standalone_div_marker(&lines[close], &prefix) {
                    Some(true) => div_depth += 1,
                    Some(false) if div_depth > 1 => div_depth -= 1,
                    Some(false) => break,
                    None => {}
                },
            }
            close += 1;
        }
        if !rewritten {
            index += 1;
        }
    }
}

fn standalone_named_block_marker(line: &str, prefix: &str, name: &str) -> Option<bool> {
    let (body, _) = split_line(line);
    let marker_start = body.find("[[")?;
    if &body[..marker_start] != prefix
        || !prefix.chars().all(|ch| matches!(ch, '>' | ' ' | '\t'))
    {
        return None;
    }
    let marker = body[marker_start..].to_ascii_lowercase();
    let opener = format!("[[{name}]]");
    let opener_with_head = format!("[[{name} ");
    let closer = format!("[[/{name}]]");
    if marker == opener || marker.starts_with(&opener_with_head) && marker.ends_with("]]")
    {
        Some(true)
    } else if marker == closer {
        Some(false)
    } else {
        None
    }
}

fn canonicalize_crossed_outer_owner_closers(
    lines: &mut [String],
    literal_lines: &[bool],
) {
    const PAIRS: &[(&str, &str, bool)] = &[
        ("div", "collapsible", false),
        ("div", "ul", true),
        ("div", "ol", true),
        ("collapsible", "ul", true),
        ("collapsible", "ol", true),
    ];

    for &(outer, inner, retain_outer_break) in PAIRS {
        let mut index = 0;
        while index + 1 < lines.len() {
            if literal_lines[index] || literal_lines[index + 1] {
                index += 1;
                continue;
            }
            let (body, _) = split_line(&lines[index]);
            let Some(marker_start) = body.find("[[") else {
                index += 1;
                continue;
            };
            let prefix = body[..marker_start].to_owned();
            if standalone_named_block_marker(&lines[index], &prefix, outer) != Some(true)
                || standalone_named_block_marker(&lines[index + 1], &prefix, inner)
                    != Some(true)
            {
                index += 1;
                continue;
            }

            let mut outer_depth = 1usize;
            let mut inner_depth = 1usize;
            let mut cursor = index + 2;
            let mut rewritten = false;
            while cursor + 1 < lines.len() {
                if literal_lines[cursor] {
                    break;
                }
                if let Some(open) =
                    standalone_named_block_marker(&lines[cursor], &prefix, outer)
                {
                    if open {
                        outer_depth += 1;
                    } else if outer_depth > 1 {
                        outer_depth -= 1;
                    } else if inner_depth == 1
                        && !literal_lines[cursor + 1]
                        && standalone_named_block_marker(
                            &lines[cursor + 1],
                            &prefix,
                            inner,
                        ) == Some(false)
                    {
                        let outer_ending = split_line(&lines[cursor]).1.to_owned();
                        let inner_ending = split_line(&lines[cursor + 1]).1.to_owned();
                        if retain_outer_break {
                            let sentinel = if inner == "ol" {
                                WIKIDOT_CROSSED_OL_TAIL_SENTINEL
                            } else {
                                WIKIDOT_CROSSED_LIST_BREAK_SENTINEL
                            };
                            lines[cursor] = format!(
                                "{prefix}[[/{inner}]][[/{outer}]]{sentinel}{inner_ending}"
                            );
                            lines[cursor + 1].clear();
                        } else {
                            lines[cursor] = format!("{prefix}[[/{inner}]]{outer_ending}");
                            lines[cursor + 1] =
                                format!("{prefix}[[/{outer}]]{inner_ending}");
                        }
                        rewritten = true;
                        break;
                    } else {
                        break;
                    }
                }

                if let Some(open) =
                    standalone_named_block_marker(&lines[cursor], &prefix, inner)
                {
                    if open {
                        inner_depth += 1;
                    } else if inner_depth > 1 {
                        inner_depth -= 1;
                    }
                }
                cursor += 1;
            }
            if rewritten {
                index = cursor + 2;
            } else {
                index += 1;
            }
        }
    }
}

fn canonicalize_crossed_div_center_closers(lines: &mut [String], literal_lines: &[bool]) {
    let mut index = 0;
    while index + 1 < lines.len() {
        if literal_lines[index] {
            index += 1;
            continue;
        }
        let Some((prefix, true)) = standalone_div_marker_line(&lines[index]) else {
            index += 1;
            continue;
        };
        if literal_lines[index + 1]
            || marker_line(&lines[index + 1]) != Some((prefix, Marker::CenterOpen))
        {
            index += 1;
            continue;
        }
        let prefix = prefix.to_owned();
        let required_quote_depth = prefix.bytes().filter(|&byte| byte == b'>').count();
        let search_end = lines.len();
        let mut div_depth = 1;
        let mut center_depth = 1;
        let mut close = index + 2;
        let mut rewritten = false;
        while close < search_end {
            if literal_lines[close] {
                break;
            }
            let (body, _) = split_line(&lines[close]);
            let (line_quote_depth, _) = quote_depth_and_body(body);
            if line_quote_depth < required_quote_depth {
                break;
            }

            match marker_line(&lines[close]) {
                Some((line_prefix, Marker::CenterOpen)) if line_prefix == prefix => {
                    center_depth += 1;
                }
                Some((line_prefix, Marker::CenterClose)) if line_prefix == prefix => {
                    if center_depth > 1 {
                        center_depth -= 1;
                    } else {
                        break;
                    }
                }
                _ => match standalone_div_marker(&lines[close], &prefix) {
                    Some(true) => div_depth += 1,
                    Some(false) if div_depth > 1 => div_depth -= 1,
                    Some(false) if center_depth == 1 => {
                        let center_close = close + 1;
                        if center_close < lines.len()
                            && !literal_lines[center_close]
                            && marker_line(&lines[center_close])
                                == Some((prefix.as_str(), Marker::CenterClose))
                        {
                            let div_ending = split_line(&lines[close]).1.to_owned();
                            let center_ending =
                                split_line(&lines[center_close]).1.to_owned();
                            lines[close] = format!("{prefix}[[/=]]{div_ending}");
                            lines[center_close] =
                                format!("{prefix}[[/div]]{center_ending}");
                            index = center_close + 1;
                            rewritten = true;
                        }
                        break;
                    }
                    Some(false) => break,
                    None => {}
                },
            }
            close += 1;
        }
        if !rewritten {
            index += 1;
        }
    }
}

fn canonicalize_crossed_center_div_closers(lines: &mut [String], literal_lines: &[bool]) {
    let mut index = 0;
    while index + 1 < lines.len() {
        if literal_lines[index] {
            index += 1;
            continue;
        }
        let Some((prefix, Marker::CenterOpen)) = marker_line(&lines[index]) else {
            index += 1;
            continue;
        };
        let prefix = prefix.to_owned();
        if literal_lines[index + 1]
            || standalone_div_marker(&lines[index + 1], &prefix) != Some(true)
        {
            index += 1;
            continue;
        }

        let required_quote_depth = prefix.bytes().filter(|&byte| byte == b'>').count();
        let search_end = lines.len();
        let mut center_close = index + 2;
        let mut rewritten = false;
        while center_close < search_end {
            if literal_lines[center_close] {
                break;
            }
            let (body, _) = split_line(&lines[center_close]);
            let (line_quote_depth, _) = quote_depth_and_body(body);
            if line_quote_depth < required_quote_depth {
                break;
            }
            if standalone_div_marker(&lines[center_close], &prefix).is_some() {
                break;
            }
            if marker_line(&lines[center_close])
                == Some((prefix.as_str(), Marker::CenterClose))
            {
                let div_close = center_close + 1;
                if div_close < lines.len()
                    && !literal_lines[div_close]
                    && standalone_div_marker(&lines[div_close], &prefix) == Some(false)
                {
                    let (_, early_ending) = split_line(&lines[center_close]);
                    lines[center_close] = format!("{prefix}{early_ending}");
                    let (div_body, div_ending) = split_line(&lines[div_close]);
                    lines[div_close] = format!("{div_body}\n{prefix}[[/=]]{div_ending}");
                    index = div_close + 1;
                    rewritten = true;
                }
                break;
            }
            center_close += 1;
        }
        if !rewritten {
            index += 1;
        }
    }
}

fn canonicalize_crossed_center_collapsible_closers(
    lines: &mut [String],
    literal_lines: &[bool],
    preserve_early_close: bool,
) {
    let mut pending: Option<CrossedClose> = None;
    let mut index = 0;

    while index < lines.len() {
        if pending.as_ref().is_some_and(|candidate| {
            let (body, _) = split_line(&lines[index]);
            let (line_quote_depth, _) = quote_depth_and_body(body);
            line_quote_depth < candidate.quote_depth
        }) {
            pending = None;
        }

        if literal_lines[index] {
            index += 1;
            continue;
        }

        let marker = marker_line(&lines[index])
            .map(|(prefix, marker)| (prefix.to_owned(), marker));
        if let (Some(candidate), Some((prefix, marker))) = (&mut pending, &marker)
            && prefix == &candidate.prefix
        {
            match marker {
                Marker::CenterClose if candidate.early_center_close.is_none() => {
                    candidate.early_center_close = Some(index);
                    index += 1;
                    continue;
                }
                Marker::CenterOpen
                    if candidate.early_center_close.is_some()
                        && candidate.reopened_center.is_none() =>
                {
                    candidate.reopened_center = Some(index);
                    index += 1;
                    continue;
                }
                Marker::CollapsibleClose if candidate.early_center_close.is_some() => {
                    if candidate.nested_collapsible_depth > 0 {
                        candidate.nested_collapsible_depth -= 1;
                        index += 1;
                        continue;
                    }
                    if let Some(reopened) = candidate.reopened_center {
                        let mut tail_matches = reopened + 1 == index;
                        if tail_matches {
                            for offset in 1..=candidate.wrapper_divs {
                                let div_close = index + offset;
                                if div_close >= lines.len()
                                    || literal_lines[div_close]
                                    || standalone_div_marker(
                                        &lines[div_close],
                                        &candidate.prefix,
                                    ) != Some(false)
                                {
                                    tail_matches = false;
                                    break;
                                }
                            }
                        }
                        let final_center_close = index + 1 + candidate.wrapper_divs;
                        if tail_matches
                            && final_center_close < lines.len()
                            && !literal_lines[final_center_close]
                            && marker_line(&lines[final_center_close])
                                == Some((candidate.prefix.as_str(), Marker::CenterClose))
                        {
                            let early = candidate
                                .early_center_close
                                .expect("crossed close candidate has an early closer");
                            let early_ending = split_line(&lines[early]).1.to_owned();
                            let reopened_ending =
                                split_line(&lines[reopened]).1.to_owned();
                            lines[early] = format!("{}{early_ending}", candidate.prefix);
                            lines[reopened] =
                                format!("{}{reopened_ending}", candidate.prefix);
                            pending = None;
                            index += 1;
                            continue;
                        }
                        pending = None;
                        index += 1;
                        continue;
                    }
                    let early = candidate
                        .early_center_close
                        .expect("crossed close candidate has an early closer");
                    if preserve_early_close && early + 1 != index {
                        pending = None;
                        index += 1;
                        continue;
                    }
                    let Some(insertion_line) = crossed_center_close_insertion_line(
                        lines,
                        literal_lines,
                        index,
                        &candidate.prefix,
                        candidate.wrapper_divs,
                    ) else {
                        pending = None;
                        index += 1;
                        continue;
                    };
                    if !preserve_early_close || early + 1 == index {
                        let (_, early_ending) = split_line(&lines[early]);
                        lines[early] = format!("{}{early_ending}", candidate.prefix);
                    }

                    let (insertion_body, insertion_ending) =
                        split_line(&lines[insertion_line]);
                    lines[insertion_line] = format!(
                        "{insertion_body}\n{}[[/=]]{insertion_ending}",
                        candidate.prefix,
                    );
                    pending = None;
                    index = insertion_line + 1;
                    continue;
                }
                Marker::CollapsibleOpen if candidate.early_center_close.is_some() => {
                    candidate.nested_collapsible_depth += 1;
                }
                Marker::CenterOpen | Marker::CollapsibleOpen => pending = None,
                _ => {}
            }
        }

        if marker
            .as_ref()
            .is_some_and(|(_, marker)| *marker == Marker::CenterOpen)
        {
            let (prefix, _) = marker.expect("center marker exists");
            if let Some((opener_index, wrapper_divs)) =
                centered_collapsible_opener(lines, literal_lines, index, prefix.as_str())
            {
                let quote_depth = prefix.bytes().filter(|&byte| byte == b'>').count();
                pending = Some(CrossedClose {
                    prefix,
                    quote_depth,
                    wrapper_divs,
                    early_center_close: None,
                    reopened_center: None,
                    nested_collapsible_depth: 0,
                });
                index = opener_index + 1;
                continue;
            }
        }

        index += 1;
    }
}

fn canonicalize_unmatched_quoted_tab_closers(
    lines: &mut [String],
    literal_lines: &[bool],
) {
    let has_tab_open = lines.iter().zip(literal_lines).any(|(line, literal)| {
        if *literal {
            return false;
        }
        let lower = line.to_ascii_lowercase();
        lower.contains("[[tab]]") || lower.contains("[[tab ")
    });
    let has_tabview_open = lines.iter().zip(literal_lines).any(|(line, literal)| {
        !literal && line.to_ascii_lowercase().contains("[[tabview]]")
    });

    for (index, line) in lines.iter_mut().enumerate() {
        if literal_lines[index] {
            continue;
        }
        let ending = if line.ends_with('\n') { "\n" } else { "" };
        let (body, _) = split_line(line);
        let Some(marker_start) = body.find("[[") else {
            continue;
        };
        let prefix = body[..marker_start].to_owned();
        if !prefix.contains('>')
            || !prefix.chars().all(|ch| matches!(ch, '>' | ' ' | '\t'))
        {
            continue;
        }

        let marker = &body[marker_start..];
        let unmatched = (marker.eq_ignore_ascii_case("[[/tab]]") && !has_tab_open)
            || (marker.eq_ignore_ascii_case("[[/tabview]]") && !has_tabview_open);
        if unmatched {
            *line = format!("{prefix}{ending}");
        }
    }
}

fn literal_line_mask(lines: &[String]) -> Vec<bool> {
    #[derive(Clone, Copy)]
    struct LiteralBlock {
        close: &'static str,
        quote_depth: usize,
    }

    let mut block: Option<LiteralBlock> = None;
    let mut in_comment = false;
    let mut mask = Vec::with_capacity(lines.len());

    for line in lines {
        let (body, _) = split_line(line);
        let (quote_depth, logical) = quote_depth_and_body(body);
        let lower = logical.to_ascii_lowercase();

        if let Some(literal) = block {
            if literal.quote_depth > 0 && quote_depth < literal.quote_depth {
                block = None;
            } else {
                mask.push(true);
                let close_depth_matches =
                    literal.quote_depth == 0 || quote_depth == literal.quote_depth;
                if close_depth_matches && lower.contains(literal.close) {
                    block = None;
                }
                continue;
            }
        }

        if in_comment {
            mask.push(true);
            if logical.contains("--]") {
                in_comment = false;
            }
            continue;
        }

        if is_tight_first_quote(body) {
            mask.push(false);
            continue;
        }

        if let Some(close) = literal_block_close(&lower) {
            mask.push(true);
            if !lower.contains(close) {
                block = Some(LiteralBlock { close, quote_depth });
            }
            continue;
        }

        if let Some(open) = logical.find("[!--") {
            mask.push(true);
            if !logical[open + 4..].contains("--]") {
                in_comment = true;
            }
            continue;
        }

        if logical.starts_with("@@") && logical.ends_with("@@") && logical.len() >= 4 {
            mask.push(true);
            continue;
        }

        mask.push(false);
    }

    mask
}

fn quote_depth_and_body(mut body: &str) -> (usize, &str) {
    let mut quote_depth = 0;
    body = body.trim_start_matches([' ', '\t']);
    while let Some(rest) = body.strip_prefix('>') {
        quote_depth += 1;
        body = rest.trim_start_matches([' ', '\t']);
    }
    (quote_depth, body)
}

fn is_tight_first_quote(body: &str) -> bool {
    let body = body.trim_start_matches([' ', '\t']);
    let depth = body.bytes().take_while(|byte| *byte == b'>').count();
    if depth == 0 {
        return false;
    }
    let rest = &body[depth..];
    !rest.is_empty() && !rest.starts_with([' ', '\t', '\r'])
}

fn literal_block_close(lower: &str) -> Option<&'static str> {
    let marker = lower.strip_prefix("[[")?.trim_start();
    let head = marker.split_once("]]")?.0.trim_end();
    let mut words = head.split_ascii_whitespace();

    match words.next()? {
        "code" => Some("[[/code]]"),
        "raw" => Some("[[/raw]]"),
        "html" => Some("[[/html]]"),
        "math" => Some("[[/math]]"),
        "embed" if words.next().is_none() => Some("[[/embed]]"),
        "module" if words.next() == Some("css") => Some("[[/module]]"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};
    use std::time::{Duration, Instant};

    #[test]
    fn malformed_iftags_score_close_leaves_the_following_div_open() {
        let mut source = concat!(
            "[[iftags +missing]]\n",
            "[[div]]\n",
            "hidden child\n",
            "[[/iftags_]]\n",
            "visible",
        )
        .to_owned();

        substitute(&mut source);
        let wikijump_source = source.clone();
        substitute_wikidot(&mut source);

        assert_eq!(
            source,
            concat!(
                "[[iftags +missing]]\n",
                "[[div]]\n",
                "hidden child\n",
                "[[/iftags_]]\n",
                "visible\n",
                "[[/div]]",
            ),
        );
        assert_eq!(
            wikijump_source,
            concat!(
                "[[iftags +missing]]\n",
                "[[div]]\n",
                "hidden child\n",
                "[[/iftags_]]\n",
                "visible",
            ),
        );

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let (tree, _) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(html.contains("<p>[[iftags +missing]]</p>"), "{html}");
        assert!(html.contains("<div><p>hidden child"), "{html}");
        assert!(html.contains("[[/iftags_]]<br>"), "{html}");
        assert!(html.contains("visible</p></div>"), "{html}");
    }

    #[test]
    fn wikidot_unclosed_div_repair_requires_the_exact_observed_shape() {
        let mut short = "[[iftags +missing]]\n[[div]]\nvisible".to_owned();
        substitute_wikidot(&mut short);
        assert_eq!(short, "[[iftags +missing]]\n[[div]]\nvisible");

        let mut literal = concat!(
            "[[iftags +missing]]\n",
            "[[div]]\n",
            "[[code]]body[[/code]]\n",
            "[[/iftags_]]\n",
            "visible",
        )
        .to_owned();
        substitute_wikidot(&mut literal);
        assert!(!literal.ends_with("[[/div]]"));

        let mut ordinary_close = concat!(
            "[[iftags +missing]]\n",
            "[[div]]\n",
            "hidden child\n",
            "[[/iftags]]\n",
            "visible\n",
        )
        .to_owned();
        substitute_wikidot(&mut ordinary_close);
        assert!(!ordinary_close.ends_with("[[/div]]"));

        let mut terminated = concat!(
            "[[iftags +missing]]\n",
            "[[div]]\n",
            "hidden child\n",
            "[[/iftags_]]\n",
            "visible\n",
        )
        .to_owned();
        substitute_wikidot(&mut terminated);
        assert!(terminated.ends_with("visible\n[[/div]]"));
    }

    #[test]
    fn incompatible_crossing_candidates_remain_unmodified() {
        let quoted_lines = vec![
            "> [[=]]\n".to_owned(),
            "[[collapsible show=\"open\"]]\n".to_owned(),
        ];
        assert_eq!(
            centered_collapsible_opener(&quoted_lines, &[false, false], 0, "> "),
            None,
        );
        assert_eq!(iftags_marker_line("[[/iftags extra]]"), None);

        let mut ambiguous_size = vec!["**[[size 110%]]a**b**[[/size]]".to_owned()];
        canonicalize_crossed_bold_size_closers(&mut ambiguous_size, &[false]);
        assert_eq!(ambiguous_size, ["**[[size 110%]]a**b**[[/size]]"]);
    }

    #[test]
    fn tight_quote_lines_are_consumed_but_spaced_quotes_render() {
        let mut source = concat!(
            ">ALPHA_PLAIN_TIGHT\n",
            ">**ALPHA_BOLD_TIGHT**\n",
            ">[[[https://example.com | ALPHA_LINK_TIGHT]]]\n",
            ">[[div]]\n",
            ">ALPHA_DIV_TIGHT\n",
            ">[[/div]]\n",
            "> ALPHA_PLAIN_SPACED\n",
            "> **ALPHA_BOLD_SPACED**\n",
            "> [[[https://example.com | ALPHA_LINK_SPACED]]]\n",
            "> [[div]]\n",
            "> ALPHA_DIV_SPACED\n",
            "> [[/div]]\n",
        )
        .to_owned();

        substitute(&mut source);

        assert_eq!(
            source,
            concat!(
                "\n\n\n\n\n\n",
                "> ALPHA_PLAIN_SPACED\n",
                "> **ALPHA_BOLD_SPACED**\n",
                "> [[[https://example.com | ALPHA_LINK_SPACED]]]\n",
                "> [[div]]\n",
                "> ALPHA_DIV_SPACED\n",
                "> [[/div]]\n",
            ),
        );

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(!html.contains("_TIGHT"), "{html}");
        assert!(html.contains("ALPHA_PLAIN_SPACED"), "{html}");
        assert!(html.contains("ALPHA_BOLD_SPACED"), "{html}");
        assert!(html.contains("ALPHA_LINK_SPACED"), "{html}");
        assert!(html.contains("ALPHA_DIV_SPACED"), "{html}");
    }

    #[test]
    fn tight_quote_text_inside_root_literal_blocks_is_preserved() {
        let mut source = concat!(
            "[[code]]\n",
            ">ALPHA_CODE_LITERAL\n",
            "[[/code]]\n",
            "[[raw]]\n",
            ">ALPHA_RAW_LITERAL\n",
            "[[/raw]]\n",
        )
        .to_owned();
        let original = source.clone();

        substitute(&mut source);

        assert_eq!(source, original);
    }

    #[test]
    fn standalone_tight_quote_canonicalizes_to_a_pruned_empty_quote() {
        let mut source = ">ALPHA_TIGHT\n".to_owned();

        substitute(&mut source);
        assert_eq!(source, "\n");

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(!html.contains("ALPHA_TIGHT"), "{html}");
        assert!(html.trim().is_empty(), "{html}");
    }

    #[test]
    fn discarded_tight_quote_retains_a_paragraph_boundary() {
        let mut source = "BEFORE_MARKER\n>DROP_MARKER\nAFTER_MARKER".to_owned();

        substitute(&mut source);
        assert_eq!(source, "BEFORE_MARKER\n\nAFTER_MARKER");

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("<p>").count(), 2, "{html}");
        assert!(!html.contains("DROP_MARKER"), "{html}");
    }

    #[test]
    fn nested_spaced_quote_keeps_a_tight_marker_as_literal_content() {
        let mut source = "> >ALPHA_NESTED_LITERAL\n".to_owned();

        substitute(&mut source);

        assert_eq!(source, "> >ALPHA_NESTED_LITERAL\n");
        assert_eq!(valid_quote_depth(&source), 1);
        assert_eq!(quote_depth_and_body(&source), (2, "ALPHA_NESTED_LITERAL\n"));
    }

    #[test]
    fn contiguous_quote_run_uses_final_spacing_for_tightness() {
        let mut source = concat!(
            ">>ALPHA_DOUBLE_TIGHT\n",
            ">> ALPHA_DOUBLE_SPACED\n",
            "> >ALPHA_INNER_LITERAL\n",
        )
        .to_owned();

        substitute(&mut source);

        assert_eq!(
            source,
            concat!("\n", ">> ALPHA_DOUBLE_SPACED\n", "> >ALPHA_INNER_LITERAL\n",),
        );
        assert_eq!(valid_quote_depth(">> ALPHA_DOUBLE_SPACED"), 2);
        assert_eq!(valid_quote_depth("> >ALPHA_INNER_LITERAL"), 1);
    }

    #[test]
    fn tight_quote_blank_scope_is_capped_to_prevent_amplification() {
        let mut source = format!("{} spaced\n>DROP\n", ">".repeat(31));

        substitute(&mut source);

        assert_eq!(
            source,
            format!("{} spaced\n{}\n", ">".repeat(31), ">".repeat(30)),
        );
    }

    #[test]
    fn repeated_tight_quote_lines_do_not_multiply_deep_active_depth() {
        let depth = 1024;
        let tight_lines = 128;
        let mut source = format!("{} spaced\n", ">".repeat(depth));
        source.push_str(&">DROP\n".repeat(tight_lines));
        let input_len = source.len();

        substitute(&mut source);

        let capped_empty_quote_line = format!("{}\n", ">".repeat(30));
        assert!(source.starts_with(&format!("{} spaced\n", ">".repeat(depth))));
        assert_eq!(
            source.matches(&capped_empty_quote_line).count(),
            tight_lines
        );
        assert!(source.len() <= input_len + tight_lines * 30);
    }

    #[test]
    fn closed_quoted_iftags_are_canonicalized_before_quote_parsing() {
        for (mut source, expected) in [
            (
                "> [[iftags -missing]]\n> INCLUDED\n> [[/iftags]]\nAFTER".to_owned(),
                "> [[iftags -missing]]\n> INCLUDED\n> [[/iftags]]\nAFTER",
            ),
            (
                ">[[iftags +missing]]\nHIDDEN\n>[[/iftags]]\nAFTER".to_owned(),
                "[[iftags +missing]]\nHIDDEN\n[[/iftags]]\nAFTER",
            ),
            (
                ">> [[iftags +missing]]\n>> HIDDEN\n> [[/iftags]]\nAFTER".to_owned(),
                ">> [[iftags +missing]]\n>> HIDDEN\n>> [[/iftags]]\nAFTER",
            ),
            (
                "> > [[iftags +missing]]\n> > HIDDEN\n> > [[/iftags]]\nAFTER".to_owned(),
                "> >\n> [[iftags +missing]]\n> > HIDDEN\n> >\n> [[/iftags]]\nAFTER",
            ),
        ] {
            substitute(&mut source);
            assert_eq!(source, expected);
        }
    }

    #[test]
    fn unclosed_quoted_iftags_and_literal_region_markers_are_not_hoisted() {
        let mut quoted = "> [[iftags +missing]]\n> BODY\nAFTER".to_owned();
        let quoted_original = quoted.clone();
        substitute(&mut quoted);
        assert_eq!(quoted, quoted_original);

        let mut literal = concat!(
            "[[code]]\n",
            "> [[iftags +missing]]\n",
            "> BODY\n",
            "> [[/iftags]]\n",
            "[[/code]]\n",
        )
        .to_owned();
        let literal_original = literal.clone();
        substitute(&mut literal);
        assert_eq!(literal, literal_original);
    }

    #[test]
    fn quoted_crossed_center_and_collapsible_closers_are_canonicalized() {
        let original = concat!(
            "> [[=]]\n",
            "> [[collapsible show=\"poetry\" hide=\"poetry\"]]\n",
            "> [[/=]]\n",
            "> originally written here\n",
            "> [[/collapsible]]\n",
            "outside\n",
        );
        let mut source = original.to_owned();

        substitute(&mut source);
        assert_eq!(
            source,
            concat!(
                "> [[=]]\n",
                "> [[collapsible show=\"poetry\" hide=\"poetry\"]]\n",
                "> \n",
                "> originally written here\n",
                "> [[/collapsible]]\n",
                "> [[/=]]\n",
                "outside\n",
            ),
        );

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("originally written here"), "{html}");
        assert!(html.contains("text-align: center"), "{html}");
        assert!(html.contains("collapsible-block"), "{html}");
        assert!(html.contains("outside"), "{html}");

        let mut wikidot = original.to_owned();
        substitute_for_layout(&mut wikidot, true);
        assert_eq!(wikidot, original);
    }

    #[test]
    fn crossed_center_and_div_closers_are_canonicalized() {
        let mut source = concat!(
            "[[=]]\n",
            "[[div class=\"blockquote\"]]\n",
            "card body\n",
            "[[/=]]\n",
            "[[/div]]\n",
            "outside\n",
        )
        .to_owned();

        substitute(&mut source);
        assert_eq!(
            source,
            concat!(
                "[[=]]\n",
                "[[div class=\"blockquote\"]]\n",
                "card body\n",
                "\n",
                "[[/div]]\n",
                "[[/=]]\n",
                "outside\n",
            ),
        );
    }

    #[test]
    fn crossed_center_and_div_run_is_bounded() {
        let mut source = (0..64)
            .map(|index| {
                format!(
                    "[[=]]\n[[div class=\"blockquote\"]]\ncard body {index}\n[[/=]]\n[[/div]]\n"
                )
            })
            .collect::<String>();
        let started = Instant::now();

        substitute(&mut source);
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("class=\"blockquote\"").count(), 64);
        assert_eq!(html.matches("text-align: center").count(), 64);
    }

    #[test]
    fn crossed_collapsible_and_div_closers_are_canonicalized() {
        let mut source = concat!(
            "[[collapsible show=\"episode\" hide=\"close\"]]\n",
            "[[div class=\"content-panel\"]]\n",
            "[[div class=\"nested\"]]\n",
            "nested body\n",
            "[[/div]]\n",
            "[[div class=\"inline\"]]inline body[[/div]]\n",
            "[[/collapsible]]\n",
            "[[/div]]\n",
            "outside\n",
        )
        .to_owned();

        substitute(&mut source);
        assert_eq!(
            source,
            concat!(
                "[[collapsible show=\"episode\" hide=\"close\"]]\n",
                "[[div class=\"content-panel\"]]\n",
                "[[div class=\"nested\"]]\n",
                "nested body\n",
                "[[/div]]\n",
                "[[div class=\"inline\"]]inline body[[/div]]\n",
                "[[/div]]\n",
                "[[/collapsible]]\n",
                "outside\n",
            ),
        );
    }

    #[test]
    fn crossed_div_and_center_closers_are_canonicalized() {
        let mut source = concat!(
            "[[div class=\"record-header\"]]\n",
            "[[=]]\n",
            "record title\n",
            "[[/div]]\n",
            "[[/=]]\n",
            "outside\n",
        )
        .to_owned();

        substitute(&mut source);
        assert_eq!(
            source,
            concat!(
                "[[div class=\"record-header\"]]\n",
                "[[=]]\n",
                "record title\n",
                "[[/=]]\n",
                "[[/div]]\n",
                "outside\n",
            ),
        );
    }

    #[test]
    fn crossed_collapsible_div_and_div_center_runs_are_bounded() {
        let mut source = (0..32)
            .map(|index| {
                format!(
                    "[[collapsible show=\"episode {index}\" hide=\"close\"]]\n[[div class=\"episode\"]]\nepisode body {index}\n[[/collapsible]]\n[[/div]]\n[[div class=\"record-header\"]]\n[[=]]\nrecord title {index}\n[[/div]]\n[[/=]]\n"
                )
            })
            .collect::<String>();
        let started = Instant::now();

        substitute(&mut source);
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 32);
        assert_eq!(html.matches("class=\"episode\"").count(), 32);
        assert_eq!(html.matches("class=\"record-header\"").count(), 32);
        assert_eq!(html.matches("text-align: center").count(), 32);
    }

    #[test]
    fn crossed_block_repairs_reject_incomplete_nested_and_literal_shapes() {
        let unchanged = [
            concat!(
                "[[collapsible show=\"outer\"]]\n",
                "[[div class=\"wrapper\"]]\n",
                "[[code]]\n",
                "[[/collapsible]]\n",
                "[[/code]]\n",
                "[[/div]]\n",
            ),
            concat!(
                "> [[collapsible show=\"quoted\"]]\n",
                "> [[div class=\"wrapper\"]]\n",
                "unquoted boundary\n",
                "> [[/collapsible]]\n",
                "> [[/div]]\n",
            ),
            concat!(
                "[[collapsible show=\"outer\"]]\n",
                "[[div class=\"wrapper\"]]\n",
                "[[collapsible show=\"inner\"]]\n",
                "inner body\n",
                "[[/collapsible]]\n",
                "[[/div]]\n",
                "[[/collapsible]]\n",
            ),
            concat!(
                "[[collapsible show=\"outer\"]]\n",
                "[[div class=\"wrapper\"]]\n",
                "body\n",
                "[[/collapsible]]\n",
                "gap\n",
                "[[/div]]\n",
            ),
            concat!(
                "[[div class=\"outer\"]]\n",
                "[[=]]\n",
                "[[=]]\n",
                "nested center\n",
                "[[/=]]\n",
                "[[/=]]\n",
                "[[/div]]\n",
            ),
            concat!(
                "[[div class=\"outer\"]]\n",
                "[[=]]\n",
                "[[div class=\"inner\"]]\n",
                "nested div\n",
                "[[/div]]\n",
                "[[/div]]\n",
                "not a center close\n",
                "[[/=]]\n",
            ),
            concat!(
                "[[div class=\"outer\"]]\n",
                "[[=]]\n",
                "[[=]]\n",
                "[[/div]]\n",
                "[[/=]]\n",
                "[[/=]]\n",
            ),
            concat!(
                "[[=]]\n",
                "[[div class=\"outer\"]]\n",
                "[[div class=\"inner\"]]\n",
                "[[/=]]\n",
                "[[/div]]\n",
                "[[/div]]\n",
            ),
            concat!(
                "[[=]]\n",
                "[[collapsible show=\"update\"]]\n",
                "[[/=]]\n",
                "body\n",
                "[[=]]\n",
                "gap\n",
                "[[/collapsible]]\n",
                "[[/=]]\n",
            ),
            concat!(
                "[[=]]\n",
                "[[div class=\"wrapper\"]]\n",
                "[[collapsible show=\"details\"]]\n",
                "[[/=]]\n",
                "body\n",
                "[[/collapsible]]\n",
            ),
        ];

        for original in unchanged {
            let mut source = original.to_owned();
            substitute(&mut source);
            assert_eq!(source, original);
        }
    }

    #[test]
    fn empty_head_crossed_center_collapsible_is_canonicalized() {
        let mut source = concat!(
            "[[=]]\n",
            "[[collapsible]]\n",
            "BODY\n",
            "[[/=]]\n",
            "[[/collapsible]]\n",
        )
        .to_owned();

        substitute_for_layout(&mut source, true);

        assert_eq!(
            source,
            concat!(
                "[[=]]\n",
                "[[collapsible]]\n",
                "BODY\n",
                "\n",
                "[[/collapsible]]\n",
                "[[/=]]\n",
            ),
        );
    }

    #[test]
    fn repeated_empty_head_crossed_center_collapsibles_stay_bounded() {
        const COUNT: usize = 128;
        let mut source =
            "[[=]]\n[[collapsible]]\nBODY\n[[/=]]\n[[/collapsible]]\n".repeat(COUNT);
        let started = Instant::now();

        crate::preprocess_for_layout(&mut source, Layout::Wikidot);
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(
            started.elapsed() < Duration::from_secs(2),
            "empty-head crossed siblings took {:?}",
            started.elapsed(),
        );
        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("class=\"collapsible-block\"").count(), COUNT);
    }

    #[test]
    fn anchored_crossed_center_collapsibles_are_canonicalized_once() {
        let mut source = concat!(
            "[[=]]\n",
            "[[# 1900]]\n",
            "[[collapsible show=\"+ 1900s\" hide=\"- 1900s\"]]\n",
            "[[/=]]\n",
            "timeline entry\n",
            "[[/collapsible]]\n",
            "outside\n",
        )
        .to_owned();

        substitute(&mut source);
        assert_eq!(
            source,
            concat!(
                "[[=]]\n",
                "[[# 1900]]\n",
                "[[collapsible show=\"+ 1900s\" hide=\"- 1900s\"]]\n",
                "\n",
                "timeline entry\n",
                "[[/collapsible]]\n",
                "[[/=]]\n",
                "outside\n",
            ),
        );
    }

    #[test]
    fn reopened_center_around_crossed_collapsible_is_canonicalized() {
        let mut source = concat!(
            "[[=]]\n",
            "[[collapsible show=\"show update\" hide=\"hide update\"]]\n",
            "[[/=]]\n",
            "[[<]]\n",
            "update body\n",
            "[[/<]]\n",
            "[[=]]\n",
            "[[/collapsible]]\n",
            "[[/=]]\n",
            "outside\n",
        )
        .to_owned();

        substitute(&mut source);
        assert_eq!(
            source,
            concat!(
                "[[=]]\n",
                "[[collapsible show=\"show update\" hide=\"hide update\"]]\n",
                "\n",
                "[[<]]\n",
                "update body\n",
                "[[/<]]\n",
                "\n",
                "[[/collapsible]]\n",
                "[[/=]]\n",
                "outside\n",
            ),
        );
    }

    #[test]
    fn wrapped_crossed_center_collapsibles_close_after_the_wrapper() {
        let mut source = concat!(
            "[[=]]\n",
            "[[div class=\"card\"]]\n",
            "card title\n",
            "[[collapsible show=\"details\"]]\n",
            "[[/=]]\n",
            "[[div style=\"text-align: left\"]]body[[/div]]\n",
            "[[/collapsible]]\n",
            "[[/div]]\n",
            "outside\n",
        )
        .to_owned();

        substitute(&mut source);
        assert_eq!(
            source,
            concat!(
                "[[=]]\n",
                "[[div class=\"card\"]]\n",
                "card title\n",
                "[[collapsible show=\"details\"]]\n",
                "\n",
                "[[div style=\"text-align: left\"]]body[[/div]]\n",
                "[[/collapsible]]\n",
                "[[/div]]\n",
                "[[/=]]\n",
                "outside\n",
            ),
        );
    }

    #[test]
    fn anchored_crossed_center_collapsible_run_is_bounded() {
        let mut source = (0..32)
            .map(|index| {
                format!(
                    "[[=]]\n[[div class=\"card\"]]\n[[# year-{index}]]\ncard title {index}\n[[collapsible show=\"show\" hide=\"hide\"]]\n[[/=]]\n* year {index}\n * timeline entry\n[[/collapsible]]\n[[/div]]\n----\n"
                )
            })
            .collect::<String>();
        let started = Instant::now();

        substitute(&mut source);
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 32);
    }

    #[test]
    fn ordinary_center_and_collapsible_blocks_are_unchanged() {
        let mut source = concat!(
            "[[=]]\n",
            "centered\n",
            "[[/=]]\n",
            "[[collapsible show=\"open\" hide=\"close\"]]\n",
            "body\n",
            "[[/collapsible]]\n",
        )
        .to_owned();
        let original = source.clone();

        substitute(&mut source);

        assert_eq!(source, original);
    }

    #[test]
    fn crossed_bold_and_size_closers_are_canonicalized() {
        let mut source = concat!(
            "**[[size 120%]]SITE PT1[[/size]]**\n",
            "**[[size 110%]]OVERWATCH COUNCIL**[[/size]]\n",
            "**[[size 110%]]CABINET OFFICE**[[/size]]\n",
        )
        .repeat(6);

        substitute(&mut source);

        assert!(!source.contains("COUNCIL**[[/size]]"), "{source}");
        assert!(source.contains("COUNCIL[[/size]]**"), "{source}");

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("<strong><span style=\"font-size:110%;\">OVERWATCH COUNCIL</span></strong>"), "{html}");
    }

    #[test]
    fn crossed_bold_and_size_markers_inside_literals_are_unchanged() {
        let mut source = concat!(
            "[[code]]\n",
            "**[[size 110%]]literal**[[/size]]\n",
            "[[/code]]\n",
        )
        .to_owned();
        let original = source.clone();

        substitute(&mut source);

        assert_eq!(source, original);
    }

    #[test]
    fn quoted_collapsible_accepts_corpus_unquoted_standalone_closer() {
        let mut source = concat!(
            "> [[collapsible show=\"open\" hide=\"close\"]]\n",
            ">user example\n",
            ">------\n",
            "> body\n",
            "[[/collapsible]]\n",
            "outside\n",
        )
        .to_owned();

        substitute(&mut source);
        assert!(source.contains("> [[/collapsible]]\n"), "{source}");

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("collapsible-block"), "{html}");
        assert!(!html.contains("user example"), "{html}");
        assert!(html.contains("outside"), "{html}");
    }

    #[test]
    fn root_collapsibles_close_after_inline_quoted_report_lines() {
        let section = concat!(
            "[[collapsible show=\"report\"]]\n",
            "> quoted report\n",
            "> End log.[[/collapsible]]\n",
        );
        let mut source = section.repeat(128);
        let started = std::time::Instant::now();

        substitute(&mut source);

        assert!(started.elapsed() < std::time::Duration::from_millis(100));
        assert_eq!(source.matches("> End log.\n[[/collapsible]]").count(), 128);

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 128);
        assert_eq!(html.matches("End log.").count(), 128);
    }

    #[test]
    fn root_collapsible_scan_handles_multibyte_quoted_lines_without_a_closer() {
        let mut source = concat!(
            "[[collapsible show=\"report\"]]\n",
            "> **Author:** San José Public Library\n",
            "> End log.[[/collapsible]]\n",
        )
        .to_owned();

        substitute(&mut source);

        assert!(source.contains("> **Author:** San José Public Library\n"));
        assert!(source.contains("> End log.\n[[/collapsible]]\n"));
    }

    #[test]
    fn root_collapsible_scan_ignores_neighboring_center_markers() {
        let mut source = concat!(
            "[[collapsible show=\"report\"]]\n",
            "[[=]]\n",
            "centered\n",
            "[[/=]]\n",
            "> End log.[[/collapsible]]\n",
        )
        .to_owned();

        substitute(&mut source);

        assert!(source.contains("[[=]]\ncentered\n[[/=]]\n"));
        assert!(source.contains("> End log.\n[[/collapsible]]\n"));
    }

    #[test]
    fn root_collapsible_scan_does_not_rewrite_unquoted_inline_closers() {
        let mut source = concat!(
            "[[collapsible show=\"report\"]]\n",
            "body[[/collapsible]]\n",
        )
        .to_owned();
        let original = source.clone();

        substitute(&mut source);

        assert_eq!(source, original);
    }

    #[test]
    fn quoted_collapsible_inline_closer_keeps_its_quote_depth() {
        let mut source = concat!(
            "> [[collapsible show=\"report\"]]\n",
            "> body[[/collapsible]]\n",
        )
        .to_owned();
        let original = source.clone();

        substitute(&mut source);

        assert_eq!(source, original);
    }

    #[test]
    fn unquoted_content_boundary_is_not_absorbed_into_quoted_collapsible() {
        let mut source = concat!(
            "> [[collapsible show=\"open\"]]\n",
            "unquoted body\n",
            "[[/collapsible]]\n",
        )
        .to_owned();
        let original = source.clone();

        substitute(&mut source);

        assert_eq!(source, original);
    }

    #[test]
    fn unmatched_quoted_tab_closers_are_invisible_like_wikidot() {
        let mut source = concat!(
            "> template body\n",
            ">[[/tab]]\n",
            ">[[/tabview]]\n",
            "outside\n",
        )
        .to_owned();

        substitute(&mut source);
        assert!(!source.contains("[[/tab"), "{source}");

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("template body"), "{html}");
        assert!(html.contains("outside"), "{html}");
    }

    #[test]
    fn complete_quoted_orphan_tab_is_not_mistaken_for_an_unmatched_closer() {
        let mut source = concat!(
            "> [[tab]]\n",
            "> orphan body\n",
            "> [[/tab]]\n",
            "outside\n",
        )
        .to_owned();
        let original = source.clone();

        substitute(&mut source);

        assert_eq!(source, original);
    }

    #[test]
    fn compatibility_markers_inside_literal_regions_are_unchanged() {
        for mut source in [
            concat!(
                "[[code]]\n",
                "[[=]]\n",
                "[[collapsible show=\"sample\"]]\n",
                "[[/=]]\n",
                "[[/collapsible]]\n",
                "[[/tab]]\n",
                "[[/code]]\n",
            )
            .to_owned(),
            concat!("> [[raw]]\n", "> [[/tabview]]\n", "> [[/raw]]\n",).to_owned(),
            concat!("[!--\n", "[[/tabview]]\n", "--]\n").to_owned(),
        ] {
            let original = source.clone();

            substitute(&mut source);

            assert_eq!(source, original);
        }
    }

    #[test]
    fn deeper_quote_closer_does_not_end_a_literal_block() {
        let mut source = concat!(
            "> [[code]]\n",
            ">> [[/code]]\n",
            "> [[/tab]]\n",
            "> [[/code]]\n",
        )
        .to_owned();
        let original = source.clone();

        substitute(&mut source);

        assert_eq!(source, original);
    }

    #[test]
    fn root_orphan_tab_closer_after_quote_boundary_is_preserved() {
        let mut source =
            concat!("> [[code]]\n", "[[/tab]]\n", "> [[/code]]\n",).to_owned();
        let original = source.clone();

        substitute(&mut source);

        assert_eq!(source, original);
    }

    #[test]
    fn root_orphan_tab_closer_after_inline_raw_is_preserved() {
        let mut source = concat!("@@\n", "[[/tab]]\n", "@@\n").to_owned();
        let original = source.clone();

        substitute(&mut source);

        assert_eq!(source, original);
    }

    #[test]
    fn literal_line_at_shallower_depth_terminates_quoted_candidates() {
        for mut source in [
            concat!(
                "> [[collapsible show=\"open\"]]\n",
                "[[code]]\n",
                "literal body\n",
                "[[/code]]\n",
                "[[/collapsible]]\n",
            )
            .to_owned(),
            concat!(
                "> [[=]]\n",
                "> [[collapsible show=\"open\"]]\n",
                "[[code]]\n",
                "literal body\n",
                "[[/code]]\n",
                "> [[/=]]\n",
                "> [[/collapsible]]\n",
            )
            .to_owned(),
        ] {
            let original = source.clone();

            substitute(&mut source);

            assert_eq!(source, original);
        }
    }

    #[test]
    fn root_literal_block_accepts_a_quote_prefixed_closer() {
        let lines = concat!("[[code]]\n", "> [[/code]]\n", "[[/tab]]\n",)
            .split_inclusive('\n')
            .map(str::to_owned)
            .collect::<Vec<_>>();

        assert_eq!(literal_line_mask(&lines), [true, true, false]);
    }

    #[test]
    fn literal_block_detection_requires_an_exact_block_name_and_shape() {
        for lookalike in [
            "[[codeexample]]",
            "[[raw-data]]",
            "[[html5]]",
            "[[mathref theorem]]",
            "[[equation theorem]]",
            "[[equationref theorem]]",
            "[[embed youtube video=abc]]",
            "[[module css-reset]]",
        ] {
            assert_eq!(literal_block_close(lookalike), None, "{lookalike}");
        }

        for (opener, closer) in [
            ("[[code]]", "[[/code]]"),
            ("[[code type=rust]]", "[[/code]]"),
            ("[[ raw ]]", "[[/raw]]"),
            ("[[html class=frame]]", "[[/html]]"),
            ("[[math quadratic]]", "[[/math]]"),
            ("[[embed]]", "[[/embed]]"),
            ("[[module css]]", "[[/module]]"),
        ] {
            assert_eq!(literal_block_close(opener), Some(closer), "{opener}");
        }
    }

    #[test]
    fn compatibility_scans_adversarial_marker_runs_in_bounded_time() {
        const MARKERS: usize = 8_192;

        let quoted_open = "> [[collapsible show=\"open\"]]\n";
        let crossed_open = "> [[=]]\n> [[collapsible show=\"open\"]]\n";
        let quoted_iftags =
            concat!("> [[iftags +missing]]\n", "> hidden\n", "> [[/iftags]]\n",);
        for mut source in [
            quoted_open.repeat(MARKERS),
            crossed_open.repeat(MARKERS),
            quoted_iftags.repeat(MARKERS),
        ] {
            let original = source.clone();
            let started = std::time::Instant::now();

            substitute(&mut source);

            assert!(
                started.elapsed() < std::time::Duration::from_secs(5),
                "compatibility scan took {:?}",
                started.elapsed(),
            );
            assert_eq!(source, original);
        }
    }

    #[test]
    fn crossed_collapsible_div_closers_after_content_and_raw_lines() {
        let mut source = concat!(
            "[[collapsible show=\"Addenda\"]]\n",
            "intro line\n",
            " \n",
            "[[div class=\"chat\"]]\n",
            "**A** posted:\n",
            "@@ @@\n",
            "##white|OO## reply\n",
            "[[/collapsible]]\n",
            "[[/div]]\n",
        )
        .to_owned();

        substitute_for_layout(&mut source, true);

        assert_eq!(
            source,
            concat!(
                "[[collapsible show=\"Addenda\"]]\n",
                "intro line\n",
                " \n",
                "[[div class=\"chat\"]]\n",
                "**A** posted:\n",
                "@@ @@\n",
                "##white|OO## reply\n",
                "[[/div]]\n",
                "[[/collapsible]]\n",
            ),
        );
    }

    #[test]
    fn reopened_center_erase_with_wrapper_div() {
        let mut source = concat!(
            "[[=]]\n",
            "[[div class=\"licensebox\"]]\n",
            "[[collapsible show=\"Licensing\"]]\n",
            "cite text\n",
            "[[/=]]\n",
            "[[<]]\n",
            "middle\n",
            "[[/<]]\n",
            "[[=]]\n",
            "[[/collapsible]]\n",
            "[[/div]]\n",
            "[[/=]]",
        )
        .to_owned();

        substitute_for_layout(&mut source, true);

        assert_eq!(
            source,
            concat!(
                "[[=]]\n",
                "[[div class=\"licensebox\"]]\n",
                "[[collapsible show=\"Licensing\"]]\n",
                "cite text\n",
                "\n",
                "[[<]]\n",
                "middle\n",
                "[[/<]]\n",
                "\n",
                "[[/collapsible]]\n",
                "[[/div]]\n",
                "[[/=]]",
            ),
        );
    }

    #[test]
    fn collapsible_closer_hoisted_off_quote_and_list_lines() {
        let mut quoted = concat!(
            "**Addendum:** [[collapsible show=\"Test Log\"]]\n",
            "> first quoted line\n",
            "> [[/collapsible]]\n",
        )
        .to_owned();
        substitute_for_layout(&mut quoted, true);
        assert_eq!(
            quoted,
            concat!(
                "**Addendum:** [[collapsible show=\"Test Log\"]]\n",
                "> first quoted line\n",
                "[[/collapsible]]\n",
            ),
        );

        let mut listed = concat!(
            "[[collapsible show=\"Fiction\"]]\n",
            "* First Book, Author One\n",
            "* Second Book, Author Two [[/collapsible]]\n",
        )
        .to_owned();
        substitute_for_layout(&mut listed, true);
        assert_eq!(
            listed,
            concat!(
                "[[collapsible show=\"Fiction\"]]\n",
                "* First Book, Author One\n",
                "* Second Book, Author Two\n",
                "[[/collapsible]]\n",
            ),
        );
    }

    #[test]
    fn unclosed_div_closed_before_collapsible_close() {
        // Crossings (content between, or no re-close at all) are not rescued:
        // live pairs the div across the collapsible (or leaves it literal),
        // and synthesizing a closer would defeat that pairing. Live evidence:
        // q2-cd-own, q2-d-unclosed-in-c.
        let mut source = concat!(
            "[[collapsible show=\"Breach\"]]\n",
            "[[div class=\"blockquote\"]]\n",
            "body text\n",
            "[[/collapsible]]\n",
        )
        .to_owned();

        substitute_for_layout(&mut source, true);

        assert_eq!(
            source,
            concat!(
                "[[collapsible show=\"Breach\"]]\n",
                "[[div class=\"blockquote\"]]\n",
                "body text\n",
                "[[/collapsible]]\n",
            ),
        );

        // Tight nesting tails (a div re-close on the very next line) still
        // rescue the div before the collapsible close.
        let mut tight = vec![
            "[[collapsible show=\"Breach\"]]\n".to_owned(),
            "[[div class=\"blockquote\"]]\n".to_owned(),
            "body text\n".to_owned(),
            "[[/collapsible]]\n".to_owned(),
            "[[/div]]\n".to_owned(),
        ];
        close_unclosed_divs_before_collapsible_close(&mut tight);
        assert_eq!(
            tight,
            vec![
                "[[collapsible show=\"Breach\"]]\n".to_owned(),
                "[[div class=\"blockquote\"]]\n".to_owned(),
                "body text\n".to_owned(),
                "[[/div]]\n".to_owned(),
                "[[/collapsible]]\n".to_owned(),
                "[[/div]]\n".to_owned(),
            ],
        );

        // Content between the collapsible close and the div re-close is a
        // crossing: no rescue, the flat path pairs across.
        let mut crossed = vec![
            "[[collapsible]]\n".to_owned(),
            "A\n".to_owned(),
            "[[div]]\n".to_owned(),
            "B\n".to_owned(),
            "[[/collapsible]]\n".to_owned(),
            "C\n".to_owned(),
            "[[/div]]\n".to_owned(),
        ];
        close_unclosed_divs_before_collapsible_close(&mut crossed);
        assert_eq!(
            crossed,
            vec![
                "[[collapsible]]\n".to_owned(),
                "A\n".to_owned(),
                "[[div]]\n".to_owned(),
                "B\n".to_owned(),
                "[[/collapsible]]\n".to_owned(),
                "C\n".to_owned(),
                "[[/div]]\n".to_owned(),
            ],
        );
    }

    #[test]
    fn early_closed_div_wrapper_reordered() {
        let mut source = concat!(
            "[[div style=\"font-weight: bold;\"]]\n",
            "[[collapsible show=\"Level 5\" hide=\"Granted\"]]\n",
            "[[/div]]\n",
            "[[div style=\"font-weight: normal;\"]]\n",
            "> body line\n",
            "[[/collapsible]]\n",
            "\n",
            "[[/div]]",
        )
        .to_owned();

        substitute_for_layout(&mut source, true);

        assert_eq!(
            source,
            concat!(
                "[[div style=\"font-weight: bold;\"]]\n",
                "[[collapsible show=\"Level 5\" hide=\"Granted\"]]\n",
                "\n",
                "[[div style=\"font-weight: normal;\"]]\n",
                "> body line\n",
                "[[/div]]\n",
                "[[/collapsible]]\n",
                "\n",
                "[[/div]]",
            ),
        );
    }

    #[test]
    fn early_closed_align_wrapper_reordered() {
        let mut source = concat!(
            "[[>]]\n",
            "[[span class=\"blur\"]][[collapsible show=\"V\" hide=\"H\"]][[/span]]\n",
            "[[/>]]\n",
            "[[<]]\n",
            "body text\n",
            "[[/<]]\n",
            "[[>]]\n",
            "[[/collapsible]]\n",
            "[[/>]]",
        )
        .to_owned();

        substitute_for_layout(&mut source, true);

        assert_eq!(
            source,
            concat!(
                "[[>]]\n",
                "[[span class=\"blur\"]][[collapsible show=\"V\" hide=\"H\"]][[/span]]\n",
                "\n",
                "[[<]]\n",
                "body text\n",
                "[[/<]]\n",
                "[[>]]\n",
                "[[/>]]\n",
                "[[/collapsible]]\n",
                "[[/>]]",
            ),
        );
    }
}
