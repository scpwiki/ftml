/*
 * preproc/parser_functions/mod.rs
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

mod expression;
mod literal;

use self::expression::{evaluate, format_value, truthy};
pub(crate) use self::literal::LiteralRegionIndex;
use std::ops::Range;

const MAX_DOCUMENT_CANDIDATES: usize = 65_536;
const MAX_PARSER_FUNCTION_NESTING: usize = 1_024;
const MAX_UNMATCHED_DELIMITERS: usize = MAX_DOCUMENT_CANDIDATES;
const MAX_CONDITIONAL_SCAN_MULTIPLIER: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WikidotZeroOperatorPolicy {
    #[default]
    RuntimeError,

    ReplaceOperationWithZero,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WikidotParserFunctionOptions {
    pub zero_operator_policy: WikidotZeroOperatorPolicy,
}

/// Resolve parser functions using Wikidot's generic runtime-error behavior.
///
/// This targeted entry point can be called before include expansion. Functions
/// in code, HTML, raw, and escaped regions remain byte-preserving. Parser
/// functions inside initial comments are evaluated because Wikidot uses them
/// to generate comment delimiters before final comment parsing.
/// Invalid or resource-bounded expressions remain literal.
pub fn resolve_wikidot_parser_functions(source: &str) -> String {
    resolve_wikidot_parser_functions_with_options(
        source,
        WikidotParserFunctionOptions::default(),
    )
}

/// Resolve parser functions with an explicit context-free evaluation policy.
///
/// At most 8,192 parser-function candidates and a document-proportional amount
/// of parser-function scan work are examined per document. Content beyond
/// those bounds remains literal.
pub fn resolve_wikidot_parser_functions_with_options(
    source: &str,
    options: WikidotParserFunctionOptions,
) -> String {
    let mut budget = CandidateBudget::default();
    let mut scan_budget = ConditionalScanBudget::new(source.len());
    resolve_function_pass(source, options, &mut budget, &mut scan_budget)
}

pub(super) fn substitute(text: &mut String) {
    if !text.contains("[[#") {
        return;
    }
    *text = resolve_wikidot_parser_functions(text);
}

#[derive(Debug)]
struct CandidateBudget {
    remaining: usize,
}

#[derive(Debug)]
struct ConditionalScanBudget {
    remaining: usize,
}

#[derive(Debug)]
struct WikidotDelimiterIndex {
    unmatched_openers: Vec<usize>,
    overflowed: bool,
}

impl Default for CandidateBudget {
    fn default() -> Self {
        Self {
            remaining: MAX_DOCUMENT_CANDIDATES,
        }
    }
}

impl ConditionalScanBudget {
    fn new(source_len: usize) -> Self {
        Self {
            remaining: source_len.saturating_mul(MAX_CONDITIONAL_SCAN_MULTIPLIER),
        }
    }

    fn take(&mut self) -> bool {
        if self.remaining == 0 {
            return false;
        }
        self.remaining -= 1;
        true
    }
}

impl WikidotDelimiterIndex {
    fn new(source: &str) -> Self {
        let bytes = source.as_bytes();
        let mut cursor = 0usize;
        let mut unmatched_openers = Vec::new();
        let mut overflowed = false;
        while cursor + 1 < bytes.len() {
            if bytes[cursor..].starts_with(b"[[") {
                if unmatched_openers.len() == MAX_UNMATCHED_DELIMITERS {
                    overflowed = true;
                } else if !overflowed {
                    unmatched_openers.push(cursor);
                }
                cursor += 2;
            } else if bytes[cursor..].starts_with(b"]]") {
                if !overflowed {
                    unmatched_openers.pop();
                }
                cursor += 2;
            } else {
                cursor += 1;
            }
        }
        Self {
            unmatched_openers,
            overflowed,
        }
    }

    fn is_unmatched_opener(&self, offset: usize) -> bool {
        !self.overflowed && self.unmatched_openers.binary_search(&offset).is_ok()
    }
}

impl CandidateBudget {
    fn take(&mut self) -> bool {
        if self.remaining == 0 {
            return false;
        }
        self.remaining -= 1;
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParserFunctionKind {
    If,
    IfExpr,
    Expr,
    Unsupported,
    UnsupportedEmptyName,
    WrongCase,
}

#[derive(Clone, Copy, Debug)]
struct ParserFunctionCandidate {
    body_start: usize,
    kind: ParserFunctionKind,
}

#[derive(Debug)]
struct FlatConditionalParts {
    end: usize,
    condition: Range<usize>,
    when_true: Option<Range<usize>>,
    when_false: Option<Range<usize>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScanResult<T> {
    Found(T),
    NotFound,
    Exhausted,
}

#[derive(Debug)]
enum ConditionalSearch {
    Found(ConditionalParts),
    CrossingLink(ConditionalParts),
    NotFound,
    Exhausted,
}

#[derive(Debug)]
struct ConditionalParts {
    end: usize,
    condition: Range<usize>,
    when_true: Range<usize>,
    when_false: Option<Range<usize>>,
}

fn resolve_function_pass(
    source: &str,
    options: WikidotParserFunctionOptions,
    budget: &mut CandidateBudget,
    scan_budget: &mut ConditionalScanBudget,
) -> String {
    let literal_regions = LiteralRegionIndex::for_parser_functions(source);
    let delimiter_index = WikidotDelimiterIndex::new(source);
    let mut replacements = Vec::new();
    let mut search_start = 0usize;

    while let Some(relative_start) = source[search_start..].find("[[#") {
        if !budget.take() {
            break;
        }

        let function_start = search_start + relative_start;
        search_start = function_start + "[[#".len();
        let Some(candidate) = classify_candidate(source, function_start) else {
            continue;
        };

        if literal_regions.contains(function_start) {
            continue;
        }
        if matches!(
            candidate.kind,
            ParserFunctionKind::If | ParserFunctionKind::IfExpr
        ) && delimiter_index.is_unmatched_opener(function_start)
        {
            if let Some(end) =
                recover_unmatched_conditional_prefix(source, function_start)
            {
                replacements.push((function_start..end, String::new()));
                search_start = end;
                continue;
            }
            let first_close = source[candidate.body_start..]
                .find("]]")
                .map(|offset| candidate.body_start + offset)
                .unwrap_or(source.len());
            if !source[candidate.body_start..first_close].contains('|') {
                continue;
            }
        }

        let resolution = match candidate.kind {
            ParserFunctionKind::If | ParserFunctionKind::IfExpr => {
                resolve_conditional_candidate(
                    source,
                    candidate,
                    options,
                    &delimiter_index,
                    scan_budget,
                )
            }
            ParserFunctionKind::Expr => {
                resolve_expression_candidate(source, candidate, options, scan_budget)
            }
            ParserFunctionKind::Unsupported
            | ParserFunctionKind::UnsupportedEmptyName => {
                resolve_unsupported_candidate(source, candidate, scan_budget)
            }
            ParserFunctionKind::WrongCase => {
                resolve_empty_candidate(source, candidate, scan_budget)
            }
        };

        match resolution {
            ScanResult::Found((end, replacement)) => {
                replacements.push((function_start..end, replacement));
                search_start = end;
            }
            ScanResult::NotFound => {}
            ScanResult::Exhausted => break,
        }
    }

    apply_replacements(source, replacements)
}

fn recover_unmatched_conditional_prefix(source: &str, start: usize) -> Option<usize> {
    let mut cursor = start;
    let mut count = 0usize;
    loop {
        let suffix = &source[cursor..];
        let prefix = if suffix.starts_with("[[#ifexpr ") {
            "[[#ifexpr "
        } else if suffix.starts_with("[[#if ") {
            "[[#if "
        } else {
            break;
        };
        count += 1;
        cursor += prefix.len();
    }
    if count == 0 || !source[cursor..].starts_with("[[a ") {
        return None;
    }
    source[cursor..].find("]]").map(|close| cursor + close + 2)
}

fn classify_candidate(source: &str, start: usize) -> Option<ParserFunctionCandidate> {
    let bytes = source.as_bytes();
    let name_start = start + "[[#".len();
    let mut name_end = name_start;
    while bytes.get(name_end).is_some_and(u8::is_ascii_alphanumeric) {
        name_end += 1;
    }

    let separator = *bytes.get(name_end)?;
    if !matches!(separator, b' ' | b'\t') {
        return None;
    }

    let name = &source[name_start..name_end];
    let kind = match name {
        "if" => ParserFunctionKind::If,
        "ifexpr" => ParserFunctionKind::IfExpr,
        "expr" => ParserFunctionKind::Expr,
        _ if ["if", "ifexpr", "expr"]
            .iter()
            .any(|known| name.eq_ignore_ascii_case(known)) =>
        {
            ParserFunctionKind::WrongCase
        }
        "" => ParserFunctionKind::UnsupportedEmptyName,
        _ => ParserFunctionKind::Unsupported,
    };

    Some(ParserFunctionCandidate {
        body_start: name_end + 1,
        kind,
    })
}

fn resolve_conditional_candidate(
    source: &str,
    candidate: ParserFunctionCandidate,
    options: WikidotParserFunctionOptions,
    delimiter_index: &WikidotDelimiterIndex,
    scan_budget: &mut ConditionalScanBudget,
) -> ScanResult<(usize, String)> {
    let flat_parts =
        match find_flat_conditional_parts(source, candidate.body_start, scan_budget) {
            ScanResult::Found(parts) => parts,
            ScanResult::NotFound => {
                return resolve_single_bracket_candidate(source, candidate, scan_budget);
            }
            ScanResult::Exhausted => return ScanResult::Exhausted,
        };

    let has_branches = flat_parts.when_true.is_some();
    let crossing_link_candidate = flat_parts
        .when_true
        .as_ref()
        .map(|range| source[range.clone()].trim_start_matches([' ', '\t']))
        .is_some_and(is_wikidot_crossing_link_opener);
    if !crossing_link_candidate {
        match contains_wikidot_double_bracket_link(
            source,
            candidate.body_start,
            flat_parts.end - 2,
            scan_budget,
        ) {
            ScanResult::Found(true) => return ScanResult::NotFound,
            ScanResult::Found(false) | ScanResult::NotFound => {}
            ScanResult::Exhausted => return ScanResult::Exhausted,
        }
    }
    if candidate.kind == ParserFunctionKind::IfExpr
        && source[flat_parts.end..]
            .trim_start_matches([' ', '\t'])
            .starts_with("[[a ")
    {
        return ScanResult::NotFound;
    }
    let mut crossing_link = false;
    let parts = if crossing_link_candidate {
        match find_conditional_parts_indexed(
            source,
            candidate.body_start,
            delimiter_index,
            scan_budget,
        ) {
            ConditionalSearch::Found(parts) => parts,
            ConditionalSearch::CrossingLink(parts) => {
                crossing_link = true;
                parts
            }
            ConditionalSearch::NotFound => return ScanResult::NotFound,
            ConditionalSearch::Exhausted => return ScanResult::Exhausted,
        }
    } else {
        ConditionalParts {
            end: flat_parts.end,
            condition: flat_parts.condition,
            when_true: flat_parts
                .when_true
                .unwrap_or(candidate.body_start..candidate.body_start),
            when_false: flat_parts.when_false,
        }
    };

    let raw_condition = &source[parts.condition.clone()];
    let condition = raw_condition.trim_matches([' ', '\t']);
    let truth = match candidate.kind {
        ParserFunctionKind::If => {
            if !has_branches {
                let condition = condition.trim_matches([' ', '\t']);
                if condition.is_empty()
                    || condition == "0"
                    || condition == "1"
                    || condition.starts_with("0|")
                    || condition.starts_with("1|")
                    || condition.starts_with("0 |")
                    || condition.starts_with("1 |")
                {
                    return ScanResult::Found((parts.end, String::new()));
                }
                return ScanResult::NotFound;
            }
            Ok(simple_condition(raw_condition))
        }
        ParserFunctionKind::IfExpr => {
            let condition = if has_branches {
                condition
            } else {
                source[candidate.body_start..parts.end - 2].trim_matches([' ', '\t'])
            };
            if condition.is_empty() {
                Ok(false)
            } else {
                match evaluate(condition, options) {
                    Ok(value) => Ok(truthy(value)),
                    Err(error) => match error.runtime_message() {
                        Some(message) => Err(message),
                        None => return ScanResult::NotFound,
                    },
                }
            }
        }
        _ => unreachable!("only conditionals reach conditional resolution"),
    };

    if crossing_link
        && (candidate.kind != ParserFunctionKind::IfExpr || !matches!(&truth, Ok(false)))
    {
        return ScanResult::NotFound;
    }

    let replacement = match truth {
        Ok(true) if has_branches => source[parts.when_true].trim_matches(' ').to_owned(),
        Ok(false) if has_branches => parts
            .when_false
            .map_or("", |range| &source[range])
            .trim_matches(' ')
            .to_owned(),
        Ok(_) => String::new(),
        Err(message) => message,
    };
    if let Some(recovered) =
        recover_branch_residual(source, parts.end, &replacement, scan_budget)
    {
        return ScanResult::Found(recovered);
    }
    ScanResult::Found((parts.end, replacement))
}

fn recover_branch_residual(
    source: &str,
    replacement_end: usize,
    replacement: &str,
    scan_budget: &mut ConditionalScanBudget,
) -> Option<(usize, String)> {
    for prefix in ["[[#expr ", "[[#if "] {
        let Some(payload) = replacement.strip_prefix(prefix) else {
            continue;
        };
        let payload = if prefix == "[[#if " {
            payload.split(" | ").next().unwrap_or(payload)
        } else {
            payload
        };
        if !is_safe_legacy_payload(payload) {
            return None;
        }
        let ScanResult::Found(close_start) =
            find_closer(source, replacement_end, scan_budget)
        else {
            return None;
        };
        let suffix = &source[replacement_end..close_start];
        if !is_safe_legacy_payload(suffix) {
            return None;
        }
        return Some((close_start + 2, format!("{prefix}{payload}{suffix}]]")));
    }

    let opener = replacement.strip_prefix("[[")?;
    let name_end = opener
        .bytes()
        .position(|byte| !byte.is_ascii_alphanumeric())
        .unwrap_or(opener.len());
    let name = &opener[..name_end];
    if name.is_empty() {
        return None;
    }
    let closing = format!("[[/{name}]]");
    let ScanResult::Found(closing_start) =
        find_bytes(source, replacement_end, closing.as_bytes(), scan_budget)
    else {
        return None;
    };
    let after_closing = closing_start + closing.len();
    let ScanResult::Found(outer_close) = find_closer(source, after_closing, scan_budget)
    else {
        return None;
    };
    let before_closing = &source[replacement_end..closing_start];
    let after_closing = source[after_closing..outer_close]
        .strip_prefix(' ')
        .unwrap_or(&source[after_closing..outer_close]);
    Some((
        outer_close + 2,
        format!("{replacement}{before_closing}{closing} {after_closing}]]"),
    ))
}

fn is_safe_legacy_payload(payload: &str) -> bool {
    payload.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || byte.is_ascii_whitespace()
            || b"_|+-*/%<>=!().,".contains(&byte)
    })
}

fn find_flat_conditional_parts(
    source: &str,
    condition_start: usize,
    scan_budget: &mut ConditionalScanBudget,
) -> ScanResult<FlatConditionalParts> {
    let bytes = source.as_bytes();
    let mut cursor = condition_start;
    let mut first_separator = None;
    let mut last_separator = None;
    let mut separator_count = 0usize;
    let mut nested_candidates = 0usize;

    while cursor + 1 < bytes.len() {
        if !scan_budget.take() {
            return ScanResult::Exhausted;
        }
        if bytes[cursor..].starts_with(b"]]") {
            let condition_end = first_separator.unwrap_or(cursor);
            let when_true = first_separator.map(|first| {
                let true_end = if separator_count > 1 {
                    last_separator.expect("multiple separators have a last separator")
                } else {
                    cursor
                };
                first + 1..true_end
            });
            let when_false = (separator_count > 1).then(|| {
                last_separator.expect("multiple separators have a last separator") + 1
                    ..cursor
            });
            return ScanResult::Found(FlatConditionalParts {
                end: cursor + 2,
                condition: condition_start..condition_end,
                when_true,
                when_false,
            });
        }
        if cursor > condition_start && bytes[cursor..].starts_with(b"[[#") {
            nested_candidates += 1;
            if nested_candidates > MAX_PARSER_FUNCTION_NESTING {
                return ScanResult::Exhausted;
            }
        }
        if bytes[cursor] == b'|'
            && bytes
                .get(cursor.wrapping_sub(1))
                .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
            && bytes
                .get(cursor + 1)
                .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
        {
            first_separator.get_or_insert(cursor);
            last_separator = Some(cursor);
            separator_count += 1;
        }
        cursor += 1;
    }
    ScanResult::NotFound
}

fn contains_wikidot_double_bracket_link(
    source: &str,
    start: usize,
    end: usize,
    scan_budget: &mut ConditionalScanBudget,
) -> ScanResult<bool> {
    let mut cursor = start;
    while cursor + 1 < end {
        if !scan_budget.take() {
            return ScanResult::Exhausted;
        }
        if source.is_char_boundary(cursor)
            && is_wikidot_crossing_link_opener(&source[cursor..])
        {
            return ScanResult::Found(true);
        }
        cursor += 1;
    }
    ScanResult::Found(false)
}

fn resolve_expression_candidate(
    source: &str,
    candidate: ParserFunctionCandidate,
    options: WikidotParserFunctionOptions,
    scan_budget: &mut ConditionalScanBudget,
) -> ScanResult<(usize, String)> {
    let close_start = match find_closer(source, candidate.body_start, scan_budget) {
        ScanResult::Found(close_start) => close_start,
        ScanResult::NotFound => {
            return resolve_single_bracket_candidate(source, candidate, scan_budget);
        }
        ScanResult::Exhausted => return ScanResult::Exhausted,
    };
    let expression = source[candidate.body_start..close_start].trim_matches([' ', '\t']);
    let replacement = if expression.is_empty() {
        String::new()
    } else {
        match evaluate(expression, options) {
            Ok(value) => format_value(value),
            Err(error) => match error.runtime_message() {
                Some(message) => message,
                None => return ScanResult::NotFound,
            },
        }
    };
    ScanResult::Found((close_start + 2, replacement))
}

fn resolve_unsupported_candidate(
    _source: &str,
    _candidate: ParserFunctionCandidate,
    _scan_budget: &mut ConditionalScanBudget,
) -> ScanResult<(usize, String)> {
    ScanResult::NotFound
}

fn resolve_empty_candidate(
    source: &str,
    candidate: ParserFunctionCandidate,
    scan_budget: &mut ConditionalScanBudget,
) -> ScanResult<(usize, String)> {
    match find_closer(source, candidate.body_start, scan_budget) {
        ScanResult::Found(close_start) => {
            ScanResult::Found((close_start + 2, String::new()))
        }
        ScanResult::NotFound => ScanResult::NotFound,
        ScanResult::Exhausted => ScanResult::Exhausted,
    }
}

fn resolve_single_bracket_candidate(
    source: &str,
    candidate: ParserFunctionCandidate,
    scan_budget: &mut ConditionalScanBudget,
) -> ScanResult<(usize, String)> {
    let bytes = source.as_bytes();
    let mut cursor = candidate.body_start;
    while cursor < bytes.len() {
        if !scan_budget.take() {
            return ScanResult::Exhausted;
        }
        if bytes[cursor] == b']' {
            return ScanResult::Found((
                cursor + 1,
                format!("[{}", &source[candidate.body_start..cursor]),
            ));
        }
        cursor += 1;
    }
    ScanResult::NotFound
}

fn find_closer(
    source: &str,
    start: usize,
    scan_budget: &mut ConditionalScanBudget,
) -> ScanResult<usize> {
    let bytes = source.as_bytes();
    let mut cursor = start;
    while cursor + 1 < bytes.len() {
        if !scan_budget.take() {
            return ScanResult::Exhausted;
        }
        if bytes[cursor..].starts_with(b"]]") {
            return ScanResult::Found(cursor);
        }
        cursor += 1;
    }
    ScanResult::NotFound
}

fn find_bytes(
    source: &str,
    start: usize,
    needle: &[u8],
    scan_budget: &mut ConditionalScanBudget,
) -> ScanResult<usize> {
    let bytes = source.as_bytes();
    let mut cursor = start;
    while cursor.saturating_add(needle.len()) <= bytes.len() {
        if !scan_budget.take() {
            return ScanResult::Exhausted;
        }
        if bytes[cursor..].starts_with(needle) {
            return ScanResult::Found(cursor);
        }
        cursor += 1;
    }
    ScanResult::NotFound
}

fn simple_condition(condition: &str) -> bool {
    let authored = condition.strip_suffix([' ', '\t']).unwrap_or(condition);
    let semantic = condition.trim_matches([' ', '\t']);
    !authored.is_empty()
        && semantic != "0"
        && semantic != "null"
        && !semantic.eq_ignore_ascii_case("false")
}

fn find_conditional_parts_indexed(
    source: &str,
    condition_start: usize,
    delimiter_index: &WikidotDelimiterIndex,
    scan_budget: &mut ConditionalScanBudget,
) -> ConditionalSearch {
    let bytes = source.as_bytes();
    let mut cursor = condition_start;
    let mut depth = 1usize;
    let mut separators = [None, None];
    let mut true_branch_whitespace_only = true;
    let mut crossing_link_depth = None;
    let mut crossing_link_fallback = None;
    let mut crossing_link_confirmed = false;
    let mut expected_adjacent_conditional = None;
    let mut adjacent_conditional_depth = None;
    let mut adjacent_conditional_has_separator = false;
    let mut crossing_suffix_is_adjacent_conditionals = false;

    while cursor < bytes.len() {
        if !scan_budget.take() {
            return ConditionalSearch::Exhausted;
        }
        if bytes[cursor..].starts_with(b"[[") {
            if depth == 1 && crossing_link_confirmed {
                if crossing_suffix_is_adjacent_conditionals
                    && delimiter_index.is_unmatched_opener(cursor)
                    && is_wikidot_ifexpr_opener(&source[cursor..])
                {
                    return crossing_link_fallback.map_or(
                        ConditionalSearch::NotFound,
                        ConditionalSearch::CrossingLink,
                    );
                }
                if !is_wikidot_conditional_opener(&source[cursor..]) {
                    crossing_suffix_is_adjacent_conditionals = false;
                }
            }
            let next_depth = depth + 1;
            if expected_adjacent_conditional == Some(cursor) {
                adjacent_conditional_depth = Some(next_depth);
                expected_adjacent_conditional = None;
            }
            let in_true_branch =
                depth == 1 && separators[0].is_some() && separators[1].is_none();
            let crossing_link = in_true_branch
                && true_branch_whitespace_only
                && crossing_link_fallback.is_none()
                && is_wikidot_crossing_link_opener(&source[cursor..]);
            if in_true_branch {
                true_branch_whitespace_only = false;
            }
            depth = next_depth;
            if crossing_link {
                crossing_link_depth = Some(depth);
            }
            cursor += 2;
            continue;
        }
        if bytes[cursor..].starts_with(b"]]") {
            if depth == 1 {
                let Some(first) = separators[0] else {
                    return ConditionalSearch::NotFound;
                };
                let true_end = separators[1].unwrap_or(cursor);
                return ConditionalSearch::Found(ConditionalParts {
                    end: cursor + 2,
                    condition: condition_start..first,
                    when_true: first + 1..true_end,
                    when_false: separators[1].map(|second| second + 1..cursor),
                });
            }
            if crossing_link_depth == Some(depth)
                && let Some(first) = separators[0]
            {
                let close_end = cursor + 2;
                if close_end == source.len()
                    || is_wikidot_conditional_opener(&source[close_end..])
                {
                    crossing_link_fallback = Some(ConditionalParts {
                        end: close_end,
                        condition: condition_start..first,
                        when_true: first + 1..close_end,
                        when_false: None,
                    });
                    if close_end == source.len() {
                        crossing_link_confirmed = true;
                    } else {
                        expected_adjacent_conditional = Some(close_end);
                    }
                }
            }
            if crossing_link_depth == Some(depth) {
                crossing_link_depth = None;
            }
            if adjacent_conditional_depth == Some(depth) {
                adjacent_conditional_depth = None;
                if adjacent_conditional_has_separator {
                    crossing_link_confirmed = true;
                    crossing_suffix_is_adjacent_conditionals = true;
                }
                adjacent_conditional_has_separator = false;
            }
            depth -= 1;
            cursor += 2;
            continue;
        }
        if bytes[cursor] == b'|' {
            if bytes.get(cursor + 1) == Some(&b'|') {
                if depth == 1 && separators[0].is_some() && separators[1].is_none() {
                    true_branch_whitespace_only = false;
                }
                if depth == 1 && crossing_link_confirmed {
                    crossing_suffix_is_adjacent_conditionals = false;
                }
                cursor += 2;
                continue;
            }
            if depth == 1 {
                if crossing_link_confirmed {
                    crossing_suffix_is_adjacent_conditionals = false;
                }
                if separators[0].is_none() {
                    separators[0] = Some(cursor);
                } else if separators[1].is_none() {
                    separators[1] = Some(cursor);
                }
            } else if adjacent_conditional_depth == Some(depth) {
                adjacent_conditional_has_separator = true;
            }
            cursor += 1;
            continue;
        }
        if depth == 1
            && separators[0].is_some()
            && separators[1].is_none()
            && !bytes[cursor].is_ascii_whitespace()
        {
            true_branch_whitespace_only = false;
            if crossing_link_confirmed {
                crossing_suffix_is_adjacent_conditionals = false;
            }
        }
        cursor += 1;
    }
    if depth == 1
        && separators[1].is_none()
        && crossing_link_confirmed
        && expected_adjacent_conditional.is_none()
        && adjacent_conditional_depth.is_none()
    {
        crossing_link_fallback
            .map_or(ConditionalSearch::NotFound, ConditionalSearch::CrossingLink)
    } else {
        ConditionalSearch::NotFound
    }
}

fn is_wikidot_crossing_link_opener(source: &str) -> bool {
    ["[[a", "[[/a"].iter().any(|prefix| {
        source
            .get(..prefix.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
            && source
                .as_bytes()
                .get(prefix.len())
                .is_some_and(u8::is_ascii_whitespace)
    })
}

fn is_wikidot_conditional_opener(source: &str) -> bool {
    is_wikidot_ifexpr_opener(source) || has_wikidot_opener(source, "[[#if")
}

fn is_wikidot_ifexpr_opener(source: &str) -> bool {
    has_wikidot_opener(source, "[[#ifexpr")
}

fn has_wikidot_opener(source: &str, prefix: &str) -> bool {
    source
        .get(..prefix.len())
        .is_some_and(|candidate| candidate == prefix)
        && source
            .as_bytes()
            .get(prefix.len())
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
}

fn apply_replacements(source: &str, replacements: Vec<(Range<usize>, String)>) -> String {
    if replacements.is_empty() {
        return source.to_owned();
    }

    let mut resolved = source.to_owned();
    for (range, replacement) in replacements.into_iter().rev() {
        resolved.replace_range(range, &replacement);
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_expr_ifexpr_and_simple_if() {
        let source = concat!(
            "[[#expr 7*6]] ",
            "[[#ifexpr 2*(2-1) == 2 | true branch | false branch]] ",
            "[[#if 0 | hidden | shown]]",
        );

        assert_eq!(
            resolve_wikidot_parser_functions(source),
            "42 true branch shown",
        );
    }

    #[test]
    fn resolves_expr_error_fidelity_cases() {
        let source = concat!(
            "[[#expr unknown_func(1)]]\n",
            "[[#expr 1..2]]\n",
            "[[#expr ]]\n",
            "[[#expr 2+2]]",
        );

        assert_eq!(
            resolve_wikidot_parser_functions(source),
            concat!(
                "run-time error: undefined function \"unknown_func\"\n",
                "run-time error: too many values in the stack\n",
                "\n",
                "4",
            ),
        );
    }

    #[test]
    fn simple_if_uses_wikidot_nonempty_string_truthiness() {
        for (condition, expected) in [
            ("foo", "true branch"),
            ("no", "true branch"),
            ("aroace", "true branch"),
            ("{$code}", "true branch"),
            ("", "false branch"),
            ("0", "false branch"),
            ("false", "false branch"),
            ("FALSE", "false branch"),
            ("null", "false branch"),
            ("NULL", "true branch"),
        ] {
            let source = format!("[[#if {condition} | true branch | false branch]]",);
            assert_eq!(
                resolve_wikidot_parser_functions(&source),
                expected,
                "{condition:?}",
            );
        }
    }

    #[test]
    fn conditional_scan_preserves_utf8_branch_text() {
        for branch in ["\u{041e}", "\u{2003}"] {
            let source = format!("[[#if 1 | {branch} | hidden]]");

            assert_eq!(resolve_wikidot_parser_functions(&source), branch);
        }
    }

    #[test]
    fn first_closer_owns_block_and_nested_branch_text() {
        let source = concat!(
            "[[#ifexpr 1 | [[span data-value=\"a|b\"]]shown[[/span]] | hidden]]",
            "[[#if 1 | [[#ifexpr 0 | no | nested]] | outer-hidden]]",
            "[[#if 1 | [[a href=\"/target\" | linked ]][[#if 1 | adjacent | no]] | hidden]]",
        );

        assert_eq!(
            resolve_wikidot_parser_functions(source),
            concat!(
                "[[span data-value=\"a|b\"shown[[/span]] | hidden]]",
                "[[#ifexpr 0 | no | outer-hidden]]",
                "[[a href=\"/target\" | linked ]][[#if 1 | adjacent | no]]",
            ),
        );
    }

    #[test]
    fn resolves_crossing_listpages_link_conditionals() {
        let source = concat!(
            "[[#ifexpr 0-0>0 | [[a class=\"point\" href=\"https://scp-jp.wikidot.com/example\" ",
            "style=\"bottom: calc(172px + 0px * sin(56)); left: calc(172px + 0px * cos(56));\" | ",
            "]][[#ifexpr 0-0>0 | ] | ]][[#ifexpr 0-0>0 | ] | ]]",
            "[[#ifexpr 0-0>0 | [[/a | ]][[#ifexpr 0-0>0 | ] | ]][[#ifexpr 0-0>0 | ] | ]]",
        );

        assert_eq!(resolve_wikidot_parser_functions(source), "");
    }

    #[test]
    fn crossing_link_fallback_rejects_an_unclosed_explicit_false_branch() {
        for (source, expected) in [
            (
                concat!(
                    "[[#if 1 | selected | [[a href=\"/x\" | hidden ]]",
                    "[[#if 1 | adjacent | no]]",
                ),
                "[[#if 1 | selected | [[a href=\"/x\" | hidden ]]adjacent",
            ),
            (
                concat!(
                    "[[#if 0 | prefix [[a href=\"/x\" | hidden ]]",
                    "[[#if 1 | adjacent | no]]",
                ),
                "[[#if 0 | prefix [[a href=\"/x\" | hidden ]]adjacent",
            ),
            (
                concat!(
                    "[[#if 0 | [[a href=\"/target\" | linked ]]",
                    "[[#if 1 | nested | no]] | explicit false",
                ),
                "[[#if 0 | [[a href=\"/target\" | linked ]]nested | explicit false",
            ),
        ] {
            assert_eq!(resolve_wikidot_parser_functions(source), expected);
        }
    }

    #[test]
    fn crossing_link_fallback_requires_a_complete_adjacent_conditional() {
        let source = concat!(
            "[[#ifexpr 0 | [[a href=\"/x\" | hidden ]]",
            "[[#if 1 | open",
        );

        assert_eq!(resolve_wikidot_parser_functions(source), source);
    }

    #[test]
    fn crossing_link_must_be_the_first_structural_true_branch_token() {
        for (source, expected) in [
            (
                concat!(
                    "[[#ifexpr 0 | [[#if 1 | prefix | no]] ",
                    "[[a href=\"/x\" | hidden ]]",
                    "[[#if 1 | adjacent | no]]",
                ),
                "[[#ifexpr 0 | prefix [[a href=\"/x\" | hidden ]]adjacent",
            ),
            (
                concat!(
                    "[[#ifexpr 0 | [[a href=\"/one\" | first ]] ",
                    "[[a href=\"/two\" | second ]]",
                    "[[#if 1 | adjacent | no]]",
                ),
                concat!(
                    "[[#ifexpr 0 | [[a href=\"/one\" | first ]] ",
                    "[[a href=\"/two\" | second ]]adjacent",
                ),
            ),
        ] {
            assert_eq!(resolve_wikidot_parser_functions(source), expected);
        }
    }

    #[test]
    fn crossing_link_rejects_a_double_pipe_true_branch_prefix() {
        let source = concat!(
            "[[#ifexpr 0 | || [[a href=\"/x\" | hidden ]]",
            "[[#if 1 | adjacent | no]]",
        );

        assert_eq!(
            resolve_wikidot_parser_functions(source),
            "[[#ifexpr 0 | || [[a href=\"/x\" | hidden ]]adjacent",
        );
    }

    #[test]
    fn crossing_link_rejects_a_terminal_false_separator() {
        let source = concat!(
            "[[#ifexpr 0 | [[a href=\"/x\" | hidden ]]",
            "[[#if 1 | adjacent | no]]|",
        );

        assert_eq!(
            resolve_wikidot_parser_functions(source),
            "[[#ifexpr 0 | [[a href=\"/x\" | hidden ]]adjacent|",
        );
    }

    #[test]
    fn crossing_link_requires_a_structurally_valid_adjacent_conditional() {
        let source = concat!(
            "[[#ifexpr 0 | [[a href=\"/x\" | hidden ]]",
            "[[#if malformed]]",
        );

        assert_eq!(resolve_wikidot_parser_functions(source), source);
    }

    #[test]
    fn crossing_link_fallback_keeps_the_first_candidate() {
        let source = concat!(
            "[[#ifexpr 0 | [[a href=\"/one\" | first ]]",
            "[[#if 1 | adjacent | no]]",
            "[[a href=\"/two\" | outside ]]",
        );

        assert_eq!(
            resolve_wikidot_parser_functions(source),
            "adjacent[[a href=\"/two\" | outside ]]",
        );
    }

    #[test]
    fn crossing_link_lookahead_is_strictly_anchored() {
        assert!(is_wikidot_conditional_opener("[[#ifexpr 1 | yes | no]]"));
        assert!(!is_wikidot_conditional_opener("[[#IF 1 | yes | no]]"));
        assert!(!is_wikidot_conditional_opener(
            "prefix [[#ifexpr 1 | yes | no]]",
        ));
        assert!(!is_wikidot_conditional_opener("[[#ifexpr]"));

        let links = "[[a href=\"/x\" | x ]]x".repeat(2_000);
        let source = format!("[[#ifexpr 0 | {links}");
        assert_eq!(resolve_wikidot_parser_functions(&source), source);
    }

    #[test]
    fn repeated_crossing_links_do_not_rescan_the_remaining_document() {
        let unit = concat!(
            "[[#ifexpr 0 | [[a href=\"/x\" | ]]",
            "[[#ifexpr 0 | ] | ]]",
            "[[#ifexpr 0 | ] | ]]",
            "[[#ifexpr 0 | [[/a | ]]",
            "[[#ifexpr 0 | ] | ]]",
            "[[#ifexpr 0 | ] | ]]",
        );
        let source = unit.repeat(200);
        let delimiter_index = WikidotDelimiterIndex::new(&source);
        assert!(delimiter_index.is_unmatched_opener(0));
        assert!(delimiter_index.is_unmatched_opener(unit.len()));
        let mut scan_budget = ConditionalScanBudget { remaining: 256 };
        let result = find_conditional_parts_indexed(
            &source,
            "[[#ifexpr ".len(),
            &delimiter_index,
            &mut scan_budget,
        );

        assert!(
            matches!(result, ConditionalSearch::CrossingLink(_),),
            "{result:?}"
        );

        let resolved = resolve_wikidot_parser_functions(&source);
        assert!(
            resolved.is_empty(),
            "{} bytes of repeated crossing source remain after the bounded passes",
            resolved.len(),
        );

        let scan_work = |source: &str| {
            let mut candidate_budget = CandidateBudget::default();
            let mut scan_budget = ConditionalScanBudget::new(source.len());
            let initial = scan_budget.remaining;
            let _ = resolve_function_pass(
                source,
                WikidotParserFunctionOptions::default(),
                &mut candidate_budget,
                &mut scan_budget,
            );
            initial - scan_budget.remaining
        };
        let work_200 = scan_work(&source);
        let source_400 = unit.repeat(400);
        let work_400 = scan_work(&source_400);
        assert!(
            work_400 <= work_200.saturating_mul(2).saturating_add(unit.len()),
            "conditional scan work must scale linearly: 200={work_200}, 400={work_400}",
        );
    }

    #[test]
    fn crossing_link_fallback_rejects_unevidenced_conditions() {
        for (source, expected) in [
            (
                concat!(
                    "[[#if 0 | [[a href=\"/x\" | hidden ]]",
                    "[[#if 1 | adjacent | no]]",
                ),
                "[[#if 0 | [[a href=\"/x\" | hidden ]]adjacent",
            ),
            (
                concat!(
                    "[[#ifexpr 1 | [[a href=\"/x\" | selected ]]",
                    "[[#if 1 | adjacent | no]]",
                ),
                "[[#ifexpr 1 | [[a href=\"/x\" | selected ]]adjacent",
            ),
            (
                concat!(
                    "[[#ifexpr 1/0 | [[a href=\"/x\" | error ]]",
                    "[[#if 1 | adjacent | no]]",
                ),
                "[[#ifexpr 1/0 | [[a href=\"/x\" | error ]]adjacent",
            ),
        ] {
            assert_eq!(resolve_wikidot_parser_functions(source), expected);
        }
    }

    #[test]
    fn first_nested_closer_recovers_an_unclosed_outer_conditional() {
        let source = concat!(
            "[[#ifexpr 0 || 1 | chosen | hidden]] ",
            "[[#if 1 | open [[#if 1 | nested | no]]",
        );

        assert_eq!(
            resolve_wikidot_parser_functions(source),
            "chosen open [[#if 1 | nested",
        );
    }

    #[test]
    fn preserves_parser_functions_in_literal_regions() {
        let source = concat!(
            "[[code]]\n[[#expr 1+1]]\n[[/code]]\n",
            "> [[html]]\n> [[#ifexpr 1 | html | hidden]]\n> [[/html]]\n",
            "[[raw]]\n[[#if 1 | raw | hidden]]\n[[/raw]]\n",
            "@@[[#expr 2+2]]@@\n",
            "[!-- [[#if 1 | comment | hidden]] --]\n",
            "[[code]]same-line[[/code]][[#expr 4+4]]\n",
            "[[#expr 3+3]]",
        );

        assert_eq!(
            resolve_wikidot_parser_functions(source),
            concat!(
                "[[code]]\n[[#expr 1+1]]\n[[/code]]\n",
                "> [[html]]\n> [[#ifexpr 1 | html | hidden]]\n> [[/html]]\n",
                "[[raw]]\n[[#if 1 | raw | hidden]]\n[[/raw]]\n",
                "@@[[#expr 2+2]]@@\n",
                "[!-- comment --]\n",
                "[[code]]same-line[[/code]]8\n",
                "6",
            ),
        );
    }

    #[test]
    fn emits_exact_generic_runtime_errors() {
        for (source, expected) in [
            (
                "[[#expr unknown(1)]]",
                r#"run-time error: undefined function "unknown""#,
            ),
            ("[[#expr 1/0]]", "run-time error: division by zero"),
            ("[[#expr 1/0+1]]", "run-time error: division by zero"),
            ("[[#expr 1%0]]", "run-time error: rest-division by zero"),
            (
                "[[#ifexpr 1/0 | leaked | hidden]]",
                "run-time error: division by zero",
            ),
        ] {
            assert_eq!(resolve_wikidot_parser_functions(source), expected);
        }
    }

    #[test]
    fn options_api_supports_caller_evidenced_zero_operator_policy() {
        let options = WikidotParserFunctionOptions {
            zero_operator_policy: WikidotZeroOperatorPolicy::ReplaceOperationWithZero,
        };
        assert_eq!(
            resolve_wikidot_parser_functions_with_options(
                "[[#expr 1/0+1]] [[#expr 5%0+2]]",
                options,
            ),
            "1 2",
        );
    }

    #[test]
    fn invalid_inputs_emit_the_live_runtime_diagnostics() {
        for (source, expected) in [
            (
                "[[#expr abs(1,2)]]",
                "run-time error: too many arguments for function \"abs\"(1 -> 2)",
            ),
            (
                "[[#expr 1 + <script>]]",
                "run-time error: too few parameters for operator \"+\" (2 -> 1)",
            ),
        ] {
            assert_eq!(resolve_wikidot_parser_functions(source), expected);
        }
    }

    #[test]
    fn expressions_beyond_the_old_256_byte_limit_still_evaluate() {
        let expression = format!("{}1", "1+".repeat(129));
        let source = format!("[[#expr {expression}]]");
        assert_eq!(resolve_wikidot_parser_functions(&source), "130");
    }

    #[test]
    fn document_candidate_limit_is_deterministic() {
        let source = "[[#expr 1]]".repeat(MAX_DOCUMENT_CANDIDATES + 1);
        let resolved = resolve_wikidot_parser_functions(&source);

        assert_eq!(resolved.matches("[[#expr 1]]").count(), 1);
        assert_eq!(resolved.matches('1').count(), MAX_DOCUMENT_CANDIDATES + 1);
        assert!(resolved.ends_with("[[#expr 1]]"));
    }

    #[test]
    fn malformed_conditional_scan_work_is_document_bounded() {
        let malformed = "[[#if ".repeat(MAX_DOCUMENT_CANDIDATES);
        let valid = "[[#if 1 | selected | hidden]]";
        let source = format!("{malformed}{valid}");

        let resolved = resolve_wikidot_parser_functions(&source);

        assert_eq!(resolved, source);
    }

    #[test]
    fn standard_preprocess_resolves_before_quote_compatibility_and_typography() {
        let mut source =
            concat!(">[[#expr 7*6]]\n", "> [[#if 1 | ``selected'' | hidden]]\n",)
                .to_owned();

        crate::preprocess_for_layout(&mut source, crate::layout::Layout::Wikidot);

        assert_eq!(source, "> “selected”");
    }
}
