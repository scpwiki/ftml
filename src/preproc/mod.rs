/*
 * preproc/mod.rs
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

pub mod typography;
pub mod whitespace;

mod comments;
mod compatibility;
mod parser_functions;

pub(crate) use self::compatibility::{
    WIKIDOT_CROSSED_LIST_BREAK_SENTINEL, WIKIDOT_CROSSED_OL_TAIL_SENTINEL,
    wikidot_outer_div_still_open_at_collapsible_close,
};
pub(crate) use self::parser_functions::LiteralRegionIndex;
pub use self::parser_functions::{
    WikidotParserFunctionOptions, WikidotZeroOperatorPolicy,
    resolve_wikidot_parser_functions, resolve_wikidot_parser_functions_with_options,
};

#[cfg(test)]
mod test;

use crate::layout::Layout;
use regex::Regex;

#[derive(Debug)]
pub enum Replacer {
    RegexReplace {
        regex: Regex,
        replacement: &'static str,
    },

    RegexSurround {
        regex: Regex,
        begin: &'static str,
        end: &'static str,
    },
}

impl Replacer {
    fn regex(&self) -> &Regex {
        match self {
            Replacer::RegexReplace { regex, .. }
            | Replacer::RegexSurround { regex, .. } => regex,
        }
    }

    fn replace(&self, text: &mut String, buffer: &mut String) {
        use self::Replacer::*;

        match *self {
            RegexReplace {
                ref regex,
                replacement,
            } => {
                trace!(
                    "Running regex regular expression replacement (pattern {}, replacement {})",
                    regex.as_str(),
                    replacement,
                );

                let mut offset = 0;
                let Some(mut capture) = regex.captures_at(text, offset) else {
                    return;
                };

                buffer.clear();
                buffer.reserve(text.len());
                let mut last_copied = 0;

                loop {
                    let mtch = capture
                        .name("repl")
                        .unwrap_or_else(|| capture.get(0).unwrap());

                    debug_assert!(mtch.start() < mtch.end());

                    buffer.push_str(&text[last_copied..mtch.start()]);
                    buffer.push_str(replacement);

                    last_copied = mtch.end();
                    offset = mtch.end();

                    let Some(next_capture) = regex.captures_at(text, offset) else {
                        break;
                    };
                    capture = next_capture;
                }

                buffer.push_str(&text[last_copied..]);
                std::mem::swap(text, buffer);
            }
            RegexSurround {
                ref regex,
                begin,
                end,
            } => {
                trace!(
                    "Running surround regular expression capture replacement (pattern {}, begin {}, end {})",
                    regex.as_str(),
                    begin,
                    end,
                );

                let mut offset = 0;
                let Some(mut capture) = regex.captures_at(text, offset) else {
                    return;
                };

                buffer.clear();
                buffer.reserve(text.len());
                let mut last_copied = 0;

                loop {
                    let mtch = capture
                        .get(1)
                        .expect("Regular expression lacks a content group");

                    let full_mtch = capture
                        .get(0)
                        .expect("Regular expression lacks a full match");

                    debug_assert!(full_mtch.start() < full_mtch.end());

                    buffer.push_str(&text[last_copied..full_mtch.start()]);
                    buffer.push_str(begin);
                    buffer.push_str(mtch.as_str());
                    buffer.push_str(end);

                    last_copied = full_mtch.end();
                    offset = full_mtch.end();

                    let Some(next_capture) = regex.captures_at(text, offset) else {
                        break;
                    };
                    capture = next_capture;
                }

                buffer.push_str(&text[last_copied..]);
                std::mem::swap(text, buffer);
            }
        }
    }
}

/// Run the preprocessor on the given wikitext, which is modified in-place.
///
/// The following modifications are performed:
/// * Resolve context-free Wikidot parser functions
/// * Replacing DOS and legacy Mac newlines
/// * Trimming whitespace lines
/// * Concatenating lines that end with backslashes
/// * Convert tabs to four spaces
/// * Wikidot typography transformations
///
/// This call always succeeds. The return value designates where issues occurred
/// to allow programmatic determination of where things were not as expected.
#[track_caller]
pub fn preprocess(text: &mut String) {
    preprocess_internal(text, false);
}

#[track_caller]
pub fn preprocess_for_layout(text: &mut String, layout: Layout) {
    preprocess_internal(text, layout.legacy());
}

fn preprocess_internal(text: &mut String, wikidot_compatibility: bool) {
    #[cfg(feature = "test-source-recorder")]
    crate::source_recorder::record(
        "preprocess-input",
        text,
        std::panic::Location::caller(),
    );

    debug!("Beginning preprocessing of text ({} bytes)", text.len());
    if wikidot_compatibility {
        whitespace::remove_wikidot_space_before_footnote(text);
        whitespace::preserve_wikidot_document_indentation_barrier(text);
        whitespace::expose_wikidot_replacement_markers(text);
        whitespace::preserve_wikidot_terminal_backslash_run(text);
    }
    parser_functions::substitute(text);
    if wikidot_compatibility {
        comments::substitute_wikidot(text);
        whitespace::substitute_wikidot(text);
    } else {
        whitespace::substitute(text);
    }
    compatibility::substitute_for_layout(text, wikidot_compatibility);
    if wikidot_compatibility {
        compatibility::substitute_wikidot(text);
        compatibility::inject_wikidot_footnote_block_lines(text);
        typography::substitute_wikidot_deferring_conditionals(text);
        whitespace::filter_characters(text, true);
    } else {
        typography::substitute(text);
    }
    #[cfg(feature = "test-source-recorder")]
    crate::source_recorder::record(
        "preprocess-output",
        text,
        std::panic::Location::caller(),
    );
    debug!("Finished preprocessing of text ({} bytes)", text.len());
}

#[test]
fn fn_type() {
    type SubstituteFn = fn(&mut String);

    let _: SubstituteFn = whitespace::substitute;
    let _: SubstituteFn = typography::substitute;
}

#[test]
fn replacement_marker_compatibility_is_wikidot_layout_only() {
    let mut wikidot = "a\u{fffd}".to_owned();
    let mut wikijump = wikidot.clone();

    preprocess_for_layout(&mut wikidot, Layout::Wikidot);
    preprocess_for_layout(&mut wikijump, Layout::Wikijump);

    assert_eq!(wikidot, "a2");
    assert_eq!(wikijump, "a\u{fffd}");

    let mut wikidot_terminal = "a\u{fffd}\\".to_owned();
    preprocess_for_layout(&mut wikidot_terminal, Layout::Wikidot);
    assert_eq!(wikidot_terminal, "a1\u{fffd}\\");
}
