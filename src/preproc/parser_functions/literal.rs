/*
 * preproc/parser_functions/literal.rs
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

use crate::preproc::quote_depth_and_body;
use std::ops::Range;

#[derive(Debug, Default)]
pub(crate) struct LiteralRegionIndex {
    ranges: Vec<Range<usize>>,
}

impl LiteralRegionIndex {
    pub(crate) fn new(source: &str) -> Self {
        let mut ranges = Vec::new();
        collect_wikidot_literal_blocks(source, &mut ranges);
        ranges.extend(crate::wikidot_code::active_ranges(source));
        collect_same_line_paired_ranges(source, "@@", "@@", &mut ranges);
        // Live Wikidot parses @<...>@ with a same-line closer as raw, so
        // preprocessing (typography, comments, whitespace, block scans)
        // must leave its contents alone exactly like @@...@@.
        collect_same_line_paired_ranges(source, "@<", ">@", &mut ranges);
        collect_html_literal_ranges(source, &mut ranges);
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
        Self { ranges: merged }
    }

    pub(crate) fn for_typography(source: &str) -> Self {
        let mut ranges = Self::new(source).ranges;
        collect_module_body_ranges(source, &mut ranges);
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
        Self { ranges: merged }
    }

    pub(crate) fn for_parser_functions(source: &str) -> Self {
        let mut ranges = Vec::new();
        collect_parser_function_literal_blocks(source, &mut ranges);
        collect_same_line_paired_ranges(source, "@@", "@@", &mut ranges);
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
        Self { ranges: merged }
    }

    pub(crate) fn contains(&self, offset: usize) -> bool {
        let insertion = self.ranges.partition_point(|range| range.start <= offset);
        insertion > 0 && offset < self.ranges[insertion - 1].end
    }

    pub(crate) fn ranges(&self) -> &[Range<usize>] {
        &self.ranges
    }
}

const BODYLESS_MODULES: [&str; 5] =
    ["rate", "backlinks", "categories", "join", "pagetree"];

fn collect_module_body_ranges(source: &str, ranges: &mut Vec<Range<usize>>) {
    let lower = source.to_ascii_lowercase();
    let mut cursor = 0usize;
    while let Some(relative) = lower[cursor..].find("[[module") {
        let open = cursor + relative;
        let after = open + "[[module".len();
        cursor = after;
        let Some(relative_head_end) = lower[after..].find("]]") else {
            break;
        };
        let head_end = after + relative_head_end + 2;
        let head = &lower[after..head_end - 2];
        if !head.starts_with(|character: char| character.is_ascii_whitespace()) {
            continue;
        }
        let name = head.split_ascii_whitespace().next().unwrap_or_default();
        if name.is_empty() || BODYLESS_MODULES.contains(&name) {
            continue;
        }
        let Some(relative_close) = lower[head_end..].find("[[/module") else {
            continue;
        };
        let close = head_end + relative_close;
        if lower[close + "[[/module".len()..]
            .trim_start_matches([' ', '\t'])
            .starts_with("]]")
        {
            ranges.push(head_end..close);
            cursor = close;
        }
    }
}

fn collect_parser_function_literal_blocks(source: &str, ranges: &mut Vec<Range<usize>>) {
    let mut offset = 0usize;
    let mut active: Option<ParserFunctionLiteralBlock> = None;

    for line in source.split_inclusive('\n') {
        let body = line
            .strip_suffix('\n')
            .unwrap_or(line)
            .strip_suffix('\r')
            .unwrap_or_else(|| line.strip_suffix('\n').unwrap_or(line));
        let (_, logical) = quote_depth_and_body(body);
        let logical_start = offset + body.len() - logical.len();
        let lower = logical.to_ascii_lowercase();

        if let Some(block) = active.as_mut() {
            if strict_parser_function_literal_name(&lower) == Some(block.name) {
                block.depth += 1;
                offset += line.len();
                continue;
            }
            if let Some(close_start) = lower.find(block.close) {
                if block.depth > 1 {
                    block.depth -= 1;
                } else {
                    ranges.push(
                        block.start..logical_start + close_start + block.close.len(),
                    );
                    active = None;
                }
            }
            offset += line.len();
            continue;
        }

        if !logical.starts_with("[[") {
            offset += line.len();
            continue;
        }
        let Some(head_end) = lower.find("]]") else {
            offset += line.len();
            continue;
        };
        let head = &lower[2..head_end];
        if head.starts_with(|character: char| character.is_ascii_whitespace()) {
            offset += line.len();
            continue;
        }
        let name = head.split_ascii_whitespace().next().unwrap_or_default();
        let (name, close) = match name {
            "code" => ("code", "[[/code]]"),
            "html" => ("html", "[[/html]]"),
            "raw" => ("raw", "[[/raw]]"),
            _ => {
                offset += line.len();
                continue;
            }
        };
        let body_start = head_end + 2;
        if let Some(relative_close) = lower[body_start..].find(close) {
            ranges.push(
                logical_start..logical_start + body_start + relative_close + close.len(),
            );
        } else {
            active = Some(ParserFunctionLiteralBlock {
                name,
                close,
                start: logical_start,
                depth: 1,
            });
        }
        offset += line.len();
    }

    if let Some(block) = active {
        ranges.push(block.start..source.len());
    }
}

#[derive(Clone, Copy, Debug)]
struct ParserFunctionLiteralBlock {
    name: &'static str,
    close: &'static str,
    start: usize,
    depth: usize,
}

fn strict_parser_function_literal_name(lower: &str) -> Option<&str> {
    let marker = lower.strip_prefix("[[")?;
    if marker.starts_with(|character: char| character.is_ascii_whitespace()) {
        return None;
    }
    let (head, _) = marker.split_once("]]")?;
    Some(head.split_ascii_whitespace().next().unwrap_or_default())
}

#[derive(Clone, Copy, Debug)]
struct LiteralBlock {
    close: &'static str,
    quote_depth: usize,
    start: usize,
}

fn collect_wikidot_literal_blocks(source: &str, ranges: &mut Vec<Range<usize>>) {
    let mut offset = 0usize;
    let mut active: Option<LiteralBlock> = None;

    for line in source.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let (quote_depth, logical) = quote_depth_and_body(body);
        let logical_start = offset + body.len() - logical.len();
        let lower = logical.to_ascii_lowercase();

        if let Some(block) = active {
            if block.quote_depth > 0 && quote_depth < block.quote_depth {
                ranges.push(block.start..offset);
                active = None;
            } else {
                let close_depth_matches =
                    block.quote_depth == 0 || quote_depth == block.quote_depth;
                if close_depth_matches && let Some(close_start) = lower.find(block.close)
                {
                    ranges.push(
                        block.start..logical_start + close_start + block.close.len(),
                    );
                    active = None;
                }
                offset += line.len();
                continue;
            }
        }

        if let Some((close, opener_end)) = literal_block(&lower) {
            let block = LiteralBlock {
                close,
                quote_depth,
                start: logical_start,
            };
            if let Some(relative_close) = lower[opener_end..].find(close) {
                ranges.push(
                    logical_start
                        ..logical_start + opener_end + relative_close + close.len(),
                );
            } else {
                active = Some(block);
            }
        }
        offset += line.len();
    }

    if let Some(block) = active {
        ranges.push(block.start..source.len());
    }
}

fn literal_block(lower: &str) -> Option<(&'static str, usize)> {
    let marker = lower.strip_prefix("[[")?.trim_start();
    let (head, _) = marker.split_once("]]")?;
    let opener_end = lower.find("]]")? + 2;
    let close = match head.trim_end().split_ascii_whitespace().next()? {
        "code" => "[[/code]]",
        "html" => "[[/html]]",
        "raw" => "[[/raw]]",
        _ => return None,
    };
    Some((close, opener_end))
}

fn collect_same_line_paired_ranges(
    source: &str,
    opening: &str,
    closing: &str,
    ranges: &mut Vec<Range<usize>>,
) {
    let mut line_offset = 0usize;
    for line in source.split_inclusive('\n') {
        let mut cursor = 0usize;
        while let Some(relative_start) = line[cursor..].find(opening) {
            let start = cursor + relative_start;
            let body_start = start + opening.len();
            let Some(relative_end) = line[body_start..].find(closing) else {
                break;
            };
            let end = body_start + relative_end + closing.len();
            ranges.push(line_offset + start..line_offset + end);
            cursor = end;
        }
        line_offset += line.len();
    }
}

fn collect_html_literal_ranges(source: &str, ranges: &mut Vec<Range<usize>>) {
    let mut cursor = 0usize;
    let mut active: Option<(String, usize, usize)> = None;

    while let Some(relative_start) = source[cursor..].find('<') {
        let tag_start = cursor + relative_start;
        let Some(tag_end) = html_tag_end(source, tag_start) else {
            break;
        };
        let tag = &source[tag_start..tag_end];
        let Some((name, closing, self_closing)) = html_tag_name(tag) else {
            cursor = tag_end;
            continue;
        };

        if let Some((root_name, content_start, depth)) = active.as_mut() {
            if name == *root_name {
                if closing {
                    *depth = depth.saturating_sub(1);
                    if *depth == 0 {
                        ranges.push(*content_start..tag_start);
                        active = None;
                    }
                } else if !self_closing {
                    *depth += 1;
                }
            }
        } else if !closing && !self_closing && html_tag_starts_literal(&name, tag) {
            active = Some((name, tag_end, 1));
        }
        cursor = tag_end;
    }

    if let Some((_, content_start, _)) = active {
        ranges.push(content_start..source.len());
    }
}

fn html_tag_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut cursor = start + 1;
    let mut quote = None;
    while let Some(byte) = bytes.get(cursor).copied() {
        match (quote, byte) {
            (Some(expected), actual) if expected == actual => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => return Some(cursor + 1),
            _ => {}
        }
        cursor += 1;
    }
    None
}

fn html_tag_name(tag: &str) -> Option<(String, bool, bool)> {
    let inner = tag.strip_prefix('<')?.strip_suffix('>')?.trim();
    if inner.is_empty() || inner.starts_with('!') || inner.starts_with('?') {
        return None;
    }
    let closing = inner.starts_with('/');
    let inner = if closing {
        inner[1..].trim_start()
    } else {
        inner
    };
    let name = inner
        .split(|character: char| {
            character.is_ascii_whitespace() || character == '/' || character == '>'
        })
        .next()?
        .to_ascii_lowercase();
    (!name.is_empty()).then(|| (name, closing, inner.ends_with('/')))
}

fn html_tag_starts_literal(name: &str, tag: &str) -> bool {
    if matches!(name, "code" | "pre" | "script" | "style" | "textarea") {
        return true;
    }
    if name != "div" {
        return false;
    }
    let lower = tag.to_ascii_lowercase();
    lower.contains(r#"class="code""#) || lower.contains("class='code'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_code_html_raw_and_escape_regions() {
        let source = concat!(
            "outside\n",
            "[[code]]\ncode-example\n[[/code]]\n",
            "> [[html]]\n> html-example\n> [[/html]]\n",
            "[[raw]]\nraw-example\n[[/raw]]\n",
            "@@escaped-example@@\n",
            "[!-- comment-example --]\n",
            "<code>html-code-example</code>\n",
            "<pre><pre>nested-pre-example</pre></pre>\n",
            r#"<div class="code"><div>panel-example</div></div>"#,
        );
        let index = LiteralRegionIndex::new(source);

        assert!(!index.contains(source.find("outside").unwrap()));
        for needle in [
            "code-example",
            "html-example",
            "raw-example",
            "escaped-example",
            "html-code-example",
            "nested-pre-example",
            "panel-example",
        ] {
            assert!(index.contains(source.find(needle).unwrap()), "{needle}");
        }
        assert!(!index.contains(source.find("comment-example").unwrap()));
    }

    #[test]
    fn ends_literal_region_at_exact_closer_boundary() {
        let source = "[[code]]inside[[/code]]outside";
        let index = LiteralRegionIndex::new(source);

        assert!(index.contains(source.find("inside").unwrap()));
        assert!(!index.contains(source.find("outside").unwrap()));
    }

    #[test]
    fn nested_code_keeps_the_complete_outer_body_literal() {
        let source = concat!(
            "[[code]]\n",
            "[[code]]\ninner\n[[/code]]\n",
            "[[#expr 1 + 1]]\n",
            "[[/code]]\n",
            "outside",
        );
        let index = LiteralRegionIndex::new(source);

        assert!(index.contains(source.find("[[#expr").unwrap()));
        assert!(!index.contains(source.find("outside").unwrap()));
    }

    #[test]
    fn inline_escape_regions_require_a_same_line_closer() {
        let source = concat!(
            "closed @@inside@@ outside\n",
            "unclosed @@not-inside...\n",
            "later @@ still-outside...\n",
        );
        let index = LiteralRegionIndex::new(source);

        assert!(index.contains(source.find("inside").unwrap()));
        assert!(!index.contains(source.find("not-inside").unwrap()));
        assert!(!index.contains(source.find("still-outside").unwrap()));
    }

    #[test]
    fn shallower_quote_ends_unclosed_quoted_literal_candidate() {
        let source = "> [[code]]\n> inside\noutside";
        let index = LiteralRegionIndex::new(source);

        assert!(index.contains(source.find("inside").unwrap()));
        assert!(!index.contains(source.find("outside").unwrap()));
    }

    #[test]
    fn leaves_html_attributes_and_following_text_outside_literal_body() {
        let source = r#"<code data-example="marker">inside</code> marker <textarea>body"#;
        let index = LiteralRegionIndex::new(source);
        let markers = source
            .match_indices("marker")
            .map(|(offset, _)| offset)
            .collect::<Vec<_>>();

        assert!(!index.contains(markers[0]));
        assert!(index.contains(source.find("inside").unwrap()));
        assert!(!index.contains(markers[1]));
        assert!(index.contains(source.find("body").unwrap()));
    }

    #[test]
    fn merges_nested_ranges_into_an_unclosed_wikidot_literal_block() {
        let source = "[[code]]@@inside@@[!-- comment --]";
        let index = LiteralRegionIndex::new(source);

        assert_eq!(index.ranges, vec![0..source.len()]);
        assert!(index.contains(source.find("inside").unwrap()));
        assert!(index.contains(source.find("comment").unwrap()));
    }

    #[test]
    fn ignores_non_literal_and_unclosed_html_tags() {
        let source = "<!doctype html><span>outside</span><code";
        let index = LiteralRegionIndex::new(source);

        assert!(!index.contains(source.find("outside").unwrap()));
        assert!(index.ranges.is_empty());
    }

    #[test]
    fn angle_raw_regions_cover_a_same_line_closer() {
        let source = "outside @<inside>@ outside\nunclosed @<not-inside...\n@<a>@<b>@";
        let index = LiteralRegionIndex::new(source);

        assert!(!index.contains(source.find("outside").unwrap()));
        assert!(index.contains(source.find("inside").unwrap()));
        assert!(!index.contains(source.find("not-inside").unwrap()));
        assert!(index.contains(source.find("@<a>").unwrap()));
        assert!(index.contains(source.find("@<b>").unwrap()));
    }
}
