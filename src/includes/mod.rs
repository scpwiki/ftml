/*
 * includes/mod.rs
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

#[warn(missing_docs)]
#[cfg(test)]
mod test;

mod include_ref;
mod includer;
mod parse;
mod quoted;

pub use self::include_ref::IncludeRef;
pub use self::includer::{DebugIncluder, FetchedPage, Includer, NullIncluder};

use self::parse::parse_include_block;
use self::quoted::parse_quoted_include;
use crate::data::PageRef;
use crate::settings::WikitextSettings;
use crate::tree::VariableMap;
use regex::{Regex, RegexBuilder};
use std::sync::LazyLock;

static INCLUDE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    // Wikidot only expands an include immediately following native quote
    // markers. A horizontal space after `>` makes the marker quoted example
    // text instead (corpus provenance: scp-wiki/svg-animation).
    RegexBuilder::new(r"^(?:[ \t]*>)*\[\[\s*include\s+")
        .case_insensitive(true)
        .multi_line(true)
        .dot_matches_new_line(true)
        .build()
        .unwrap()
});
static VARIABLE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{\$(?P<name>[a-zA-Z0-9_\-]+)\}").unwrap());

/// Replaces the include blocks in a string with the content of the pages referenced by those
/// blocks.
#[track_caller]
pub fn include<'t, I, E, F>(
    input: &'t str,
    settings: &WikitextSettings,
    mut includer: I,
    invalid_return: F,
) -> Result<(String, Vec<PageRef>), E>
where
    I: Includer<'t, Error = E>,
    F: FnOnce() -> E,
{
    #[cfg(feature = "test-source-recorder")]
    crate::source_recorder::record(
        "include-input",
        input,
        std::panic::Location::caller(),
    );

    if !settings.enable_page_syntax {
        debug!("Includes are disabled for this input, skipping");

        let output = str!(input);
        let pages = vec![];
        return Ok((output, pages));
    }

    let input_len = input.len();
    debug!("Inserting text for all include blocks in text ({input_len} bytes)");

    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    let mut consumed_quoted_ranges: Vec<std::ops::Range<usize>> = Vec::new();
    let mut includes = Vec::new();

    let mut search_start = 0;

    while let Some(mtch) = INCLUDE_REGEX.find_at(input, search_start) {
        let start = mtch.start();

        let slice = mtch.as_str();
        trace!("Found include regex match (start {start}, slice '{slice}')");
        let marker_offset = slice
            .find("[[")
            .expect("include scanner match must contain a block opener");
        let marker_start = start + marker_offset;
        let quote_prefix = &input[start..marker_start];

        let Some(candidate_end) = find_include_end(input, mtch.end(), input.len()) else {
            warn!("Unable to find include terminator, skipping remaining input");
            search_start = input.len();
            continue;
        };

        let parsed = if quote_prefix.is_empty() {
            parse_include_block(input, start).ok()
        } else {
            let quote_depth = quote_prefix.bytes().filter(|&byte| byte == b'>').count();
            let parsed = parse_quoted_include(
                input,
                start,
                marker_start,
                quote_depth,
                candidate_end,
            );
            if let Some(parsed) = parsed {
                // Wikidot consumes a tight quote-prefixed include without
                // resolving or rendering its target. Spaced quote examples
                // never reach this branch because the scanner excludes them.
                consumed_quoted_ranges.push(start..parsed.end);
                search_start = parsed.end;
                continue;
            }
            None
        };

        match parsed {
            Some((include, end)) => {
                ranges.push(start..end);
                includes.push(include);
                search_start = end;
            }
            None => {
                search_start = candidate_end;
                warn!("Unable to parse include regex match, resuming at {search_start}");
            }
        }
    }

    let fetched_pages = includer.include_pages(&includes)?;

    if includes.len() != fetched_pages.len() {
        return Err(invalid_return());
    }

    let mut replacements: Vec<_> = ranges
        .into_iter()
        .zip(includes.into_iter().zip(fetched_pages).map(Some))
        .collect();
    replacements.extend(
        consumed_quoted_ranges
            .into_iter()
            .map(|range| (range, None)),
    );
    replacements.sort_unstable_by_key(|(range, _)| range.start);

    substitute_replacements(input, replacements, &mut includer, invalid_return)
}

type IncludeReplacement<'t> = (
    std::ops::Range<usize>,
    Option<(IncludeRef<'t>, FetchedPage<'t>)>,
);

fn substitute_replacements<'t, I, E, F>(
    input: &str,
    replacements: Vec<IncludeReplacement<'t>>,
    includer: &mut I,
    invalid_return: F,
) -> Result<(String, Vec<PageRef>), E>
where
    I: Includer<'t, Error = E>,
    F: FnOnce() -> E,
{
    let mut output = String::from(input);
    let mut pages = Vec::new();

    for (range, replacement) in replacements.into_iter().rev() {
        let Some((include, fetched)) = replacement else {
            output.replace_range(range, "");
            continue;
        };
        let (page_ref, variables) = include.into();

        let range_start = range.start;
        let range_end = range.end;
        debug!("Replacing range for included page ({range_start}..{range_end})");

        if page_ref != fetched.page_ref {
            return Err(invalid_return());
        }

        let replace_with = match fetched.content {
            Some(mut content) => {
                replace_variables(content.to_mut(), &variables);
                content
            }

            None => includer.no_such_include(&page_ref)?,
        };
        pages.push(page_ref);

        if replace_with.is_empty() {
            output.replace_range(range, "");
        } else {
            // Wikidot treats an expanded include as its own block run. Add one
            // line ending on each side that has caller source, turning the
            // caller's ordinary line ending into a blank-line boundary. Do not
            // pad a document edge, so an include-only source remains unchanged.
            let before = (range.start > 0).then(|| {
                if input[..range.start].ends_with("\r\n") {
                    "\r\n"
                } else {
                    "\n"
                }
            });
            let after = (range.end < input.len()).then(|| {
                if input[range.end..].starts_with("\r\n") {
                    "\r\n"
                } else {
                    "\n"
                }
            });
            let mut separated = String::with_capacity(
                replace_with.len()
                    + before.map_or(0, str::len)
                    + after.map_or(0, str::len),
            );
            if let Some(line_ending) = before {
                separated.push_str(line_ending);
            }
            separated.push_str(&replace_with);
            if let Some(line_ending) = after {
                separated.push_str(line_ending);
            }

            output.replace_range(range, &separated);
        }
    }

    pages.reverse();

    Ok((output, pages))
}

fn find_include_end(input: &str, start: usize, end_bound: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut index = start;

    while index + 2 <= end_bound {
        let end = index + 2;
        if bytes[index] == b']'
            && bytes[index + 1] == b']'
            && (end == input.len()
                || bytes.get(end..).is_some_and(|rest| {
                    rest.starts_with(b"\n") || rest.starts_with(b"\r\n")
                }))
        {
            return Some(end);
        }

        index += 1;
    }

    None
}

/// Replaces all specified variables in the content to be included.
///
/// Read <https://www.wikidot.com/doc-wiki-syntax:include> for more details.
fn replace_variables(content: &mut String, variables: &VariableMap) {
    let mut matches = Vec::new();

    for capture in VARIABLE_REGEX.captures_iter(content) {
        let mtch = capture.get(0).unwrap();
        let name = &capture["name"];

        if let Some(value) = variables.get(name) {
            matches.push((value, mtch.range()));
        }
    }

    matches.reverse();
    for (value, range) in matches {
        content.replace_range(range, value);
    }
}
