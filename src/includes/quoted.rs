/*
 * includes/quoted.rs
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

use super::parse::parse_include_block;

#[derive(Debug)]
pub(super) struct ParsedQuotedInclude {
    pub end: usize,
}

#[derive(Clone, Copy, Debug)]
struct OffsetSegment {
    normalized_start: usize,
    normalized_end: usize,
    original_start: usize,
}

#[derive(Clone, Copy, Debug)]
enum QuotedIncludeParse {
    CandidateEnd,
    WholeRegion,
}

impl QuotedIncludeParse {
    fn end_bound(self, input: &str, candidate_end: usize) -> usize {
        match self {
            QuotedIncludeParse::CandidateEnd => candidate_end,
            QuotedIncludeParse::WholeRegion => input.len(),
        }
    }

    fn run(
        self,
        input: &str,
        line_start: usize,
        marker_start: usize,
        quote_depth: usize,
        candidate_end: usize,
    ) -> Option<ParsedQuotedInclude> {
        normalize_and_parse(
            input,
            line_start,
            marker_start,
            quote_depth,
            self.end_bound(input, candidate_end),
        )
    }
}

pub(super) fn parse_quoted_include(
    input: &str,
    line_start: usize,
    marker_start: usize,
    quote_depth: usize,
    candidate_end: usize,
) -> Option<ParsedQuotedInclude> {
    debug_assert!(marker_start > line_start);
    debug_assert!(quote_depth > 0);

    [
        QuotedIncludeParse::CandidateEnd,
        QuotedIncludeParse::WholeRegion,
    ]
    .into_iter()
    .find_map(|strategy| {
        strategy.run(input, line_start, marker_start, quote_depth, candidate_end)
    })
}

fn normalize_and_parse(
    input: &str,
    line_start: usize,
    marker_start: usize,
    quote_depth: usize,
    end_bound: usize,
) -> Option<ParsedQuotedInclude> {
    let mut normalized = String::new();
    let mut segments = Vec::new();
    let mut original_line_start = line_start;

    for (line_index, line) in input[line_start..].split_inclusive('\n').enumerate() {
        let content_offset = if line_index == 0 {
            marker_start - line_start
        } else {
            let Some(content_offset) = strip_quote_prefix(line, quote_depth) else {
                break;
            };
            content_offset
        };
        let content = &line[content_offset..];
        let normalized_start = normalized.len();
        normalized.push_str(content);
        segments.push(OffsetSegment {
            normalized_start,
            normalized_end: normalized.len(),
            original_start: original_line_start + content_offset,
        });

        original_line_start += line.len();
        if original_line_start >= end_bound {
            break;
        }
    }

    let (_, normalized_end) = parse_include_block(&normalized, 0).ok()?;
    let end = original_offset(&segments, normalized_end)?;
    Some(ParsedQuotedInclude { end })
}
fn strip_quote_prefix(line: &str, quote_depth: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut offset = skip_horizontal_space(bytes, 0);

    for _ in 0..quote_depth {
        if bytes.get(offset) != Some(&b'>') {
            return None;
        }
        offset += 1;
        offset = skip_horizontal_space(bytes, offset);
    }

    Some(offset)
}

fn skip_horizontal_space(bytes: &[u8], mut offset: usize) -> usize {
    while matches!(bytes.get(offset), Some(b' ' | b'\t')) {
        offset += 1;
    }
    offset
}

fn original_offset(segments: &[OffsetSegment], normalized: usize) -> Option<usize> {
    segments.iter().find_map(|segment| {
        (normalized > segment.normalized_start && normalized <= segment.normalized_end)
            .then_some(segment.original_start + normalized - segment.normalized_start)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_parser_normalizes_spacing_and_maps_original_end() {
        let source = concat!(
            "before\n",
            "> [[include :scp-wiki:component:author-label-source start=—\n",
            ">|name=toadking07]]\n",
            "after\n",
        );
        let line_start = source.find("> [[include").unwrap();
        let marker_start = source[line_start..].find("[[").unwrap() + line_start;

        let parsed = parse_quoted_include(
            source,
            line_start,
            marker_start,
            1,
            source.find("]]").unwrap() + 2,
        )
        .expect("quoted include should parse");

        assert_eq!(&source[parsed.end..], "\nafter\n");
    }

    #[test]
    fn quoted_parser_requires_every_continuation_line_to_remain_quoted() {
        let source = "> [[include component:box\n|name=unquoted]]\n";
        let marker_start = source.find("[[").unwrap();

        assert!(
            parse_quoted_include(
                source,
                0,
                marker_start,
                1,
                source.find("]]").unwrap() + 2
            )
            .is_none()
        );
    }

    #[test]
    fn quoted_parser_stops_normalizing_after_the_quote_boundary() {
        let source = "> [[include component:box]]\noutside\n";
        let marker_start = source.find("[[").unwrap();

        let parsed = parse_quoted_include(
            source,
            0,
            marker_start,
            1,
            source.find("]]").unwrap() + 2,
        )
        .expect("complete include before quote boundary should parse");

        assert_eq!(&source[parsed.end..], "\noutside\n");
    }

    #[test]
    fn quoted_parser_scans_many_candidate_terminators_in_bounded_time() {
        const CANDIDATE_LINES: usize = 8_192;

        let mut source = String::from("> [[include component:box\n> [!--\n");
        for _ in 0..CANDIDATE_LINES {
            source.push_str("> malformed candidate ]]\n");
        }
        let marker_start = source.find("[[").unwrap();
        let started = std::time::Instant::now();

        assert!(
            parse_quoted_include(
                &source,
                0,
                marker_start,
                1,
                source.find("]]").unwrap() + 2
            )
            .is_none()
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "quoted include scan took {:?}",
            started.elapsed(),
        );
    }
}
