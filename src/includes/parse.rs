/*
 * includes/parse.rs
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

mod parser {
    // Since pest generates some code that clippy doesn't like
    #![allow(clippy::empty_docs)]

    #[derive(Parser, Debug)]
    #[grammar = "includes/grammar.pest"]
    pub struct IncludeParser;
}

use self::parser::*;
use super::{IncludeRef, VARIABLE_REGEX};
use crate::data::{PageRef, PageRefParseError};
use crate::tree::VariableMap;
use pest::Parser;
use pest::iterators::Pairs;
use std::collections::HashMap;

const INCLUDE_ARGUMENT_EXPANSION_ALLOWANCE: usize = 64 * 1024;

/// The character set PHP's default `trim()` strips, which is what live applies
/// to each include argument value (at both ends).
const INCLUDE_ARGUMENT_TRIM: [char; 6] = [' ', '\t', '\n', '\r', '\0', '\u{000b}'];

#[derive(Debug)]
struct RawIncludeArgument<'t> {
    key: &'t str,
    value: &'t str,
    authored_empty_value: bool,
}

/// Parses a single include block in the text.
///
/// # Arguments
/// The "start" argument is the index at which the include block starts.
/// It does not necessarily relate to the index of the include within the text str.
///
/// # Return values
/// Returns a tuple of an [`IncludeRef`] that represents the included text and a usize that
/// represents the end index of the include block, such that start..end covers the full include
/// block (before the include goes through).
pub fn parse_include_block<'t>(
    text: &'t str,
    start: usize,
) -> Result<(IncludeRef<'t>, usize), IncludeParseError> {
    match IncludeParser::parse(Rule::include, &text[start..]) {
        Ok(mut pairs) => {
            let first = pairs.next().expect("No pairs returned on successful parse");
            let span = first.as_span();

            debug!("Parsed include block");

            let include = process_pairs(first.into_inner())?;

            Ok((include, start + span.end()))
        }
        Err(error) => {
            warn!("Include block was invalid: {error}");
            Err(IncludeParseError)
        }
    }
}

fn process_pairs(mut pairs: Pairs<Rule>) -> Result<IncludeRef, IncludeParseError> {
    let page_raw = pairs.next().ok_or(IncludeParseError)?.as_str();
    let page_ref = validate_include_page_ref(PageRef::parse_include(page_raw)?)?;

    trace!("Got page for include {page_ref:?}");
    let mut raw_arguments = Vec::new();
    let argument_tail = pairs
        .next()
        .map(|pair| {
            debug_assert_eq!(pair.as_rule(), Rule::argument_tail);
            pair.as_str()
        })
        .unwrap_or_default();
    scan_include_arguments(argument_tail, &mut raw_arguments)?;

    let spaced_empty_separator = argument_tail.strip_prefix('|').is_some_and(|spacing| {
        !spacing.is_empty() && spacing.bytes().all(|byte| matches!(byte, b' ' | b'\t'))
    });
    let arguments = resolve_include_arguments(&raw_arguments);

    Ok(IncludeRef::new(page_ref, arguments)
        .with_spaced_empty_separator(spaced_empty_separator))
}

fn scan_include_arguments<'t>(
    tail: &'t str,
    arguments: &mut Vec<RawIncludeArgument<'t>>,
) -> Result<(), IncludeParseError> {
    let bytes = tail.as_bytes();
    let mut segment_start = 0;
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index..].starts_with(b"[!--") {
            if let Some(end) = complete_comment_end(bytes, index) {
                index = end;
                continue;
            }
            return Err(IncludeParseError);
        }
        if bytes[index] == b'|' {
            scan_include_argument_segment(&tail[segment_start..index], arguments);
            segment_start = index + 1;
        }
        index += 1;
    }

    scan_include_argument_segment(&tail[segment_start..], arguments);
    Ok(())
}

fn scan_include_argument_segment<'t>(
    segment: &'t str,
    arguments: &mut Vec<RawIncludeArgument<'t>>,
) {
    let source = trim_include_space_start(segment);
    let key_end = include_argument_key_end(source);
    if key_end == 0 {
        return;
    }

    let key = &source[..key_end];
    let after_key = trim_include_space_start(&source[key_end..]);
    let Some(after_equals) = after_key.strip_prefix('=') else {
        return;
    };
    let value = trim_include_space_start(after_equals);
    // Live trims each argument value with PHP's default trim set at both ends.
    // Comments are not stripped here: live keeps them inside the value and only
    // drops them when the substituted page text is rendered.
    let value = value.trim_end_matches(INCLUDE_ARGUMENT_TRIM);
    let authored_empty_value = value.is_empty();

    trace!("Adding argument for include (key '{key}', value '{value}')");
    arguments.push(RawIncludeArgument {
        key,
        value,
        authored_empty_value,
    });
}

fn include_argument_key_end(source: &str) -> usize {
    let bytes = source.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'-' | b'_') {
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(b"{$") {
            let reference_start = index;
            index += 2;
            let name_start = index;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric()
                    || matches!(bytes[index], b'-' | b'_'))
            {
                index += 1;
            }
            if index == name_start || bytes.get(index) != Some(&b'}') {
                return reference_start;
            }
            index += 1;
            continue;
        }
        break;
    }

    index
}

/// Leading PHP `trim()` of one include argument segment or field.
///
/// This deliberately does not skip comments: live keeps a comment as part of
/// the argument text, so whitespace after a comment is interior whitespace.
fn trim_include_space_start(source: &str) -> &str {
    let bytes = source.as_bytes();
    let mut index = 0;

    while matches!(
        bytes.get(index),
        Some(b' ' | b'\n' | b'\t' | b'\r' | b'\0' | b'\x0b')
    ) {
        index += 1;
    }

    &source[index..]
}

fn complete_comment_end(bytes: &[u8], start: usize) -> Option<usize> {
    if !bytes.get(start..)?.starts_with(b"[!--") {
        return None;
    }

    let mut index = start + 4;
    while index + 3 <= bytes.len() {
        if bytes[index..].starts_with(b"--]") {
            return Some(index + 3);
        }
        index += 1;
    }
    None
}

fn resolve_include_arguments(
    raw_arguments: &[RawIncludeArgument<'_>],
) -> VariableMap<'static> {
    let round_limit = raw_arguments.len().saturating_mul(2).clamp(1, 128);
    let expansion_limit = raw_arguments
        .iter()
        .fold(0usize, |size, argument| {
            size.saturating_add(argument.key.len())
                .saturating_add(argument.value.len())
        })
        .saturating_add(INCLUDE_ARGUMENT_EXPANSION_ALLOWANCE);
    let mut arguments = HashMap::<String, String>::new();

    for argument in raw_arguments {
        if argument.authored_empty_value
            || !is_static_identifier(argument.key)
            || argument.value.contains("{$")
        {
            continue;
        }
        arguments
            .entry(argument.key.to_owned())
            .or_insert_with(|| argument.value.to_owned());
    }

    for _ in 0..round_limit {
        let mut next = HashMap::<String, String>::new();
        let mut next_size = 0usize;
        for argument in raw_arguments {
            if argument.authored_empty_value {
                continue;
            }
            let key = expand_argument_expression(
                argument.key,
                &arguments,
                round_limit,
                None,
                expansion_limit,
            );
            if !is_static_identifier(&key) {
                continue;
            }
            let self_reference = format!("{{${key}}}");
            let blocked_name = argument
                .value
                .contains(&self_reference)
                .then_some(key.as_str())
                .filter(|_| {
                    arguments
                        .get(&key)
                        .is_some_and(|value| value.contains(&self_reference))
                });
            let value = expand_argument_expression(
                argument.value,
                &arguments,
                round_limit,
                blocked_name,
                expansion_limit,
            );
            let fallback_reference = value.trim_end_matches([' ', '\t', '\r', '\n']);
            if fallback_reference == format!("{{${key}}}") {
                continue;
            }
            if next.contains_key(&key) {
                continue;
            }
            next_size = next_size
                .saturating_add(key.len())
                .saturating_add(value.len());
            if next_size > expansion_limit {
                return arguments
                    .into_iter()
                    .map(|(key, value)| (key.into(), value.into()))
                    .collect();
            }
            next.insert(key, value);
        }
        if next == arguments {
            break;
        }
        arguments = next;
    }

    arguments
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect()
}

fn expand_argument_expression(
    expression: &str,
    arguments: &HashMap<String, String>,
    round_limit: usize,
    blocked_name: Option<&str>,
    expansion_limit: usize,
) -> String {
    let mut output = expression.to_owned();
    for _ in 0..round_limit {
        let Some(expanded) = expand_argument_expression_once(
            &output,
            arguments,
            blocked_name,
            expansion_limit,
        ) else {
            break;
        };
        if expanded == output {
            break;
        }
        output = expanded;
    }
    output
}

fn expand_argument_expression_once(
    expression: &str,
    arguments: &HashMap<String, String>,
    blocked_name: Option<&str>,
    expansion_limit: usize,
) -> Option<String> {
    let mut expanded = String::with_capacity(expression.len().min(expansion_limit));
    let mut copied_until = 0;

    for capture in VARIABLE_REGEX.captures_iter(expression) {
        let matched = capture.get(0).expect("variable capture has a full match");
        let replacement = if blocked_name.is_some_and(|name| name == &capture["name"]) {
            matched.as_str()
        } else {
            arguments
                .get(&capture["name"])
                .map(|value| value.trim_end_matches([' ', '\t', '\r', '\n']))
                .unwrap_or(matched.as_str())
        };
        let literal = &expression[copied_until..matched.start()];
        let new_len = expanded
            .len()
            .checked_add(literal.len())?
            .checked_add(replacement.len())?;
        if new_len > expansion_limit {
            return None;
        }
        expanded.push_str(literal);
        expanded.push_str(replacement);
        copied_until = matched.end();
    }

    let remainder = &expression[copied_until..];
    if expanded.len().checked_add(remainder.len())? > expansion_limit {
        return None;
    }
    expanded.push_str(remainder);
    Some(expanded)
}

fn is_static_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
        })
}

fn validate_include_page_ref(page_ref: PageRef) -> Result<PageRef, IncludeParseError> {
    if page_ref.page().is_empty() {
        return Err(IncludeParseError);
    }

    Ok(page_ref)
}

#[derive(Debug, PartialEq, Eq)]
pub struct IncludeParseError;

impl From<PageRefParseError> for IncludeParseError {
    #[inline]
    fn from(_: PageRefParseError) -> Self {
        std::convert::identity(IncludeParseError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn include_parse_error_converts_from_page_ref_parse_error() {
        let error = IncludeParseError::from(PageRefParseError);
        assert_eq!(error, IncludeParseError);
    }

    #[test]
    fn validate_include_page_ref_rejects_empty_page() {
        let empty = PageRef {
            site: Some(str!("scp-wiki")),
            page: String::new(),
            extra: None,
        };

        assert_eq!(
            validate_include_page_ref(empty),
            Err(IncludeParseError),
            "empty include page references should reject",
        );
    }

    #[test]
    fn validate_include_page_ref_accepts_non_empty_page() {
        let page_ref = PageRef::page_only("component:ok");

        assert_eq!(
            validate_include_page_ref(page_ref.clone()),
            Ok(page_ref),
            "non-empty include page references should pass through",
        );
    }

    #[test]
    fn parse_include_block_rejects_empty_page_reference() {
        let error = parse_include_block("[[include :scp-wiki:/]]", 0)
            .expect_err("empty include page should reject");

        assert_eq!(error, IncludeParseError);
    }

    #[test]
    fn parse_include_block_canonicalizes_dotted_host_site() {
        let (include, _) =
            parse_include_block("[[include :scp-jp.wikidot.com:component:jstyles]]", 0)
                .expect("dotted include should parse");
        assert_eq!(
            include.page_ref(),
            &PageRef::page_and_site("scp-jp", "component:jstyles"),
        );

        let (include, _) =
            parse_include_block("[[include :scp-jp-wikidot-com:component:jstyles]]", 0)
                .expect("dashed include should parse");
        assert_eq!(
            include.page_ref(),
            &PageRef::page_and_site("scp-jp-wikidot-com", "component:jstyles"),
        );
    }

    #[test]
    fn parse_include_block_ignores_commented_arguments() {
        let source = concat!(
            "[[include :scp-wiki:component:mapping-source\n",
            "facility=-- |\n",
            "x=66 |\n",
            "[!--textclass=subtitlemap|\n",
            "text=G-GOC|--]\n",
            "]]\n",
        );

        let (include, end) = parse_include_block(source, 0)
            .expect("comments may disable arguments inside an include");

        assert_eq!(end, source.len() - 1);
        assert_eq!(
            include.page_ref(),
            &PageRef::page_and_site("scp-wiki", "component:mapping-source")
        );
        assert_eq!(
            include.variables().get("facility").map(Cow::as_ref),
            Some("--")
        );
        assert_eq!(include.variables().get("x").map(Cow::as_ref), Some("66"));
        assert!(!include.variables().contains_key("textclass"));
        assert!(!include.variables().contains_key("text"));
    }

    #[test]
    fn parse_include_block_records_only_the_live_spaced_empty_separator_shape() {
        for source in ["[[include page | ]]", "[[include PAGE |\t]]"] {
            let (include, end) =
                parse_include_block(source, 0).expect("include should parse");
            assert_eq!(end, source.len(), "{source:?}");
            assert!(include.has_spaced_empty_separator(), "{source:?}");
            assert!(include.variables().is_empty(), "{source:?}");
        }

        for source in [
            "[[include page]]",
            "[[include page |]]",
            "[[include page ||]]",
            "[[include page || ]]",
            "[[include page | a=1]]",
            "[[include page a=1 | ]]",
        ] {
            let (include, _) = parse_include_block(source, 0)
                .unwrap_or_else(|_| panic!("include should parse: {source:?}"));
            assert!(!include.has_spaced_empty_separator(), "{source:?}");
        }
    }

    #[test]
    fn parse_include_block_enforces_the_live_target_boundary() {
        for source in [
            "[[include page |x=1]]",
            "[[include page\t|x=1]]",
            "[[include page\n|x=1\n]]",
            "[[include page x=1]]",
            "[[include page]]",
        ] {
            parse_include_block(source, 0).unwrap_or_else(|_| {
                panic!("spaced or target-only include should parse: {source:?}")
            });
        }

        for source in [
            "[[include page|x=1]]",
            "[[include \"page\" |x=1]]",
            "[[include 'page' |x=1]]",
        ] {
            assert!(
                parse_include_block(source, 0).is_err(),
                "tight separators and quoted targets must remain literal: {source:?}",
            );
        }

        let (include, _) =
            parse_include_block("[[include :run-owned:component |x=1]]", 0)
                .expect("a leading-colon target with a site and page should parse");
        assert_eq!(
            include.page_ref(),
            &PageRef::page_and_site("run-owned", "component"),
        );
    }

    #[test]
    fn inert_segments_resume_without_changing_duplicate_state() {
        let source = concat!(
            "[[include page ",
            "|FLAG|x=|日本語=discarded|x=2|x=3|n=\"A|quoted-suffix\"",
            "|[!-- ignored=1|also-ignored=2 --]|z=4|trailing-key]]",
        );
        let (include, end) =
            parse_include_block(source, 0).expect("include should recover");

        assert_eq!(end, source.len());
        assert_eq!(include.variables().get("x").map(Cow::as_ref), Some("2"));
        assert_eq!(include.variables().get("n").map(Cow::as_ref), Some("\"A"),);
        assert_eq!(include.variables().get("z").map(Cow::as_ref), Some("4"));
        for inert in ["FLAG", "日本語", "quoted-suffix", "ignored", "also-ignored"] {
            assert!(!include.variables().contains_key(inert), "{inert}");
        }
    }

    #[test]
    fn include_argument_segment_scans_are_bounded() {
        const RUNS: usize = 4_096;

        let mut source = String::from("[[include page ");
        for index in 0..RUNS {
            source.push_str(&format!(
                "|FLAG{index}|empty{index}=|日本語{index}=discarded|quoted{index}=\"A|B\"|[!--ignored{index}=1|still-ignored=2--]"
            ));
        }
        source.push_str("|x=winner]]");

        let started = std::time::Instant::now();
        let (include, end) =
            parse_include_block(&source, 0).expect("long include should parse");
        assert_eq!(end, source.len());
        assert_eq!(
            include.variables().get("x").map(Cow::as_ref),
            Some("winner"),
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "include segment scan took {:?}",
            started.elapsed(),
        );

        let mut incomplete = String::from("[[include page ");
        for _ in 0..RUNS {
            incomplete.push_str("|FLAG|x=|日本語=1|\"A|B\"|[!--");
        }
        incomplete.push(']');
        let started = std::time::Instant::now();
        assert!(parse_include_block(&incomplete, 0).is_err());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "incomplete include scan took {:?}",
            started.elapsed(),
        );
    }

    #[test]
    fn recursive_include_argument_growth_is_bounded() {
        let mut source = String::from("[[include page |a={$a}{$a}|a=x");
        for index in 0..24 {
            source.push_str(&format!("|dummy{index}=x"));
        }
        source.push_str("]]");

        let (include, _) = parse_include_block(&source, 0).expect("include should parse");
        let value = include.variables().get("a").expect("a should resolve");

        assert!(
            value.len() <= source.len() + INCLUDE_ARGUMENT_EXPANSION_ALLOWANCE,
            "recursive expansion exceeded its source-relative allowance: {} bytes",
            value.len(),
        );
    }

    #[test]
    fn mutually_recursive_include_arguments_remain_bounded() {
        let source = format!(
            "[[include page |a={{$b}}{{$b}}|b={{$a}}{{$a}}|a=x|{}]]",
            (0..24)
                .map(|index| format!("dummy{index}=x"))
                .collect::<Vec<_>>()
                .join("|"),
        );
        let (include, _) = parse_include_block(&source, 0).expect("include should parse");
        let total_size = include
            .variables()
            .iter()
            .map(|(key, value)| key.len() + value.len())
            .sum::<usize>();

        assert!(
            total_size <= source.len() + INCLUDE_ARGUMENT_EXPANSION_ALLOWANCE,
            "resolved map exceeded its source-relative allowance: {total_size} bytes",
        );
    }

    #[test]
    fn expansion_budget_is_checked_before_copying_a_replacement() {
        let arguments = HashMap::from([("a".to_owned(), "1234".to_owned())]);

        assert_eq!(
            expand_argument_expression_once("{$a}", &arguments, None, 4),
            Some("1234".to_owned()),
        );
        assert_eq!(
            expand_argument_expression_once("{$a}", &arguments, None, 3),
            None,
        );
    }

    #[test]
    fn include_argument_values_use_the_live_php_trim_set() {
        for whitespace in [' ', '\t', '\n', '\r', '\0', '\u{000b}'] {
            for source in [
                format!("[[include page | a={whitespace}x{whitespace}]]"),
                format!("[[include page | a={whitespace}x{whitespace}| b=2]]"),
            ] {
                let (include, _) = parse_include_block(&source, 0)
                    .unwrap_or_else(|_| panic!("include should parse: {source:?}"));
                assert_eq!(
                    include.variables().get("a").map(Cow::as_ref),
                    Some("x"),
                    "{source:?}",
                );
            }
        }

        for source in [
            "[[include page | a=\u{00a0}x\u{00a0}]]",
            "[[include page | a=\u{000c}x\u{000c}]]",
            "[[include page | a=\u{2003}x\u{2003}]]",
        ] {
            let (include, _) = parse_include_block(source, 0)
                .unwrap_or_else(|_| panic!("include should parse: {source:?}"));
            let value = include.variables().get("a").map(Cow::as_ref);
            let expected = &source["[[include page | a=".len()..source.len() - 2];
            assert_eq!(value, Some(expected), "{source:?}");
        }
    }

    #[test]
    fn include_argument_trim_does_not_cross_comments() {
        let (include, _) = parse_include_block("[[include page | a= [!--c--] x ]]", 0)
            .expect("include should parse");
        assert_eq!(
            include.variables().get("a").map(Cow::as_ref),
            Some("[!--c--] x"),
        );

        for source in [
            "[[include page | [!--c--]a=1]]",
            "[[include page | [!--c--] a=1]]",
            "[[include page | a [!--c--]=1]]",
        ] {
            let (include, _) = parse_include_block(source, 0)
                .unwrap_or_else(|_| panic!("include should parse: {source:?}"));
            assert!(
                !include.variables().contains_key("a"),
                "{source:?}: {:?}",
                include.variables(),
            );
        }
    }

    #[test]
    fn ordinary_acyclic_include_argument_expansion_is_unchanged() {
        let (include, _) =
            parse_include_block("[[include page | a=x | b={$a}{$a} | c=prefix-{$b}]]", 0)
                .expect("include should parse");

        assert_eq!(include.variables().get("a").map(Cow::as_ref), Some("x"));
        assert_eq!(include.variables().get("b").map(Cow::as_ref), Some("xx"));
        assert_eq!(
            include.variables().get("c").map(Cow::as_ref),
            Some("prefix-xx"),
        );
    }
}
