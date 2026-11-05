/*
 * parsing/mod.rs
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

#[macro_use]
mod macros;

mod body_end;
mod boolean;
mod check_step;
mod collect;
mod condition;
pub(crate) mod consume;
mod depth;
mod element_condition;
mod error;
mod footnote;
mod heading;
mod hidden_body;
mod inline_format;
mod inline_scope;
mod outcome;
mod paragraph;
mod parser;
mod parser_wrap;
mod result;
mod rule;
mod string;
mod strip;
mod token;
mod unmatched_close_scan;
mod wikidot_text;

mod prelude {
    pub use crate::parsing::{
        ExtractedToken, ParagraphSplitEntry, ParseError, ParseErrorKind, ParseResult,
        ParseSuccess, Token, success_elements, success_elements_with_paragraph_safety,
        success_value,
    };
    pub use crate::settings::WikitextSettings;
    pub use crate::text::FullText;
    pub use crate::tree::{Element, Elements};
}

use self::depth::{DepthItem, DepthList, process_depths};
use self::element_condition::{ElementCondition, ElementConditionType};
use self::paragraph::{gather_paragraphs, no_close_condition};
use self::parser::{DEFAULT_MAX_RECURSION_DEPTH, Parser};
use self::parser_wrap::ParserWrap;
use self::rule::impls::RULE_PAGE;
use self::strip::{strip_newlines, strip_whitespace};
use crate::data::PageInfo;
use crate::next_index::{Incrementer, NextIndex};
use crate::settings::WikitextSettings;
use crate::tokenizer::Tokenization;
use crate::tree::{
    AttributeMap, BibliographyList, CodeBlock, Element, LinkLabel, LinkLocation,
    LinkType, ListItem, ListType, SyntaxTree,
};
use std::borrow::Cow;

pub use self::boolean::{NonBooleanValue, parse_boolean};
pub use self::error::{HiddenBodyFailure, ParseError, ParseErrorKind};
pub(crate) use self::inline_format::wikidot_dash_run_elements;
pub use self::outcome::ParseOutcome;
pub(crate) use self::parser::DEEP_MAX_RECURSION_DEPTH;
pub use self::result::{
    ParagraphSplitEntry, ParagraphSplitKind, ParseResult, ParseSuccess, success_elements,
    success_elements_with_paragraph_safety, success_value,
};
pub(crate) use self::rule::impls::decode_semicolon_entities;
pub(crate) use self::rule::impls::wikidot_math_name;
pub use self::token::{ExtractedToken, Token};
pub(crate) use self::wikidot_text::{
    discard_wikidot_controls, trim_wikidot_ascii, trim_wikidot_ascii_cow,
};

#[cfg(not(target_arch = "wasm32"))]
pub fn parse<'r, 't>(
    tokenization: &'r Tokenization<'t>,
    page_info: &'r PageInfo<'t>,
    settings: &'r WikitextSettings,
) -> ParseOutcome<SyntaxTree<'t>>
where
    'r: 't,
{
    std::thread::scope(|scope| {
        let handle =
            match std::thread::Builder::new()
                .name("ftml-parse".to_owned())
                .stack_size(DEEP_PARSE_STACK_BYTES)
                .spawn_scoped(scope, || {
                    let ordinary = parse_on_current_stack(
                        tokenization,
                        page_info,
                        settings,
                        DEFAULT_MAX_RECURSION_DEPTH,
                    );
                    if !ordinary.errors().iter().any(|error| {
                        error.kind() == ParseErrorKind::RecursionDepthExceeded
                    }) {
                        return ordinary;
                    }
                    parse_on_current_stack(
                        tokenization,
                        page_info,
                        settings,
                        DEEP_MAX_RECURSION_DEPTH,
                    )
                }) {
                Ok(handle) => handle,
                Err(error) => {
                    warn!("Unable to start bounded parser worker: {error}");
                    return parse_on_current_stack(
                        tokenization,
                        page_info,
                        settings,
                        DEFAULT_MAX_RECURSION_DEPTH,
                    );
                }
            };

        match handle.join() {
            Ok(outcome) => outcome,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    })
}

#[cfg(test)]
pub(crate) fn parse_for_test<'r, 't>(
    tokenization: &'r Tokenization<'t>,
    page_info: &'r PageInfo<'t>,
    settings: &'r WikitextSettings,
) -> ParseOutcome<SyntaxTree<'t>>
where
    'r: 't,
{
    parse_on_current_stack(
        tokenization,
        page_info,
        settings,
        DEFAULT_MAX_RECURSION_DEPTH,
    )
}

#[cfg(test)]
pub(crate) fn repaired_tokenization_for_test<'t>(
    tokenization: &Tokenization<'t>,
    page_info: &PageInfo<'t>,
    settings: &WikitextSettings,
) -> Option<Tokenization<'t>> {
    repair_bracket_run_tokenization(tokenization, page_info, settings)
}

/// WebAssembly hosts do not have a portable configurable thread-stack API.
#[cfg(target_arch = "wasm32")]
pub fn parse<'r, 't>(
    tokenization: &'r Tokenization<'t>,
    page_info: &'r PageInfo<'t>,
    settings: &'r WikitextSettings,
) -> ParseOutcome<SyntaxTree<'t>>
where
    'r: 't,
{
    parse_on_current_stack(
        tokenization,
        page_info,
        settings,
        DEFAULT_MAX_RECURSION_DEPTH,
    )
}

pub(crate) const DEEP_PARSE_STACK_BYTES: usize = 64 * 1024 * 1024;

fn parse_on_current_stack<'r, 't>(
    tokenization: &'r Tokenization<'t>,
    page_info: &'r PageInfo<'t>,
    settings: &'r WikitextSettings,
    max_recursion_depth: usize,
) -> ParseOutcome<SyntaxTree<'t>>
where
    'r: 't,
{
    match repair_bracket_run_tokenization(tokenization, page_info, settings) {
        Some(repaired) => parse_repaired_tokenization(
            repaired,
            page_info,
            settings,
            max_recursion_depth,
        ),
        None => parse_and_convert(tokenization, page_info, settings, max_recursion_depth),
    }
}

/// Parses with a tokenization whose bracket runs were re-split. The
/// re-split tokens borrow the caller's text, but the parse machinery
/// requires the tokenization borrow to outlive the text lifetime, so
/// the parse runs over an owned copy of the text and the resulting
/// tree is converted to owned data before it leaves this scope.
fn parse_repaired_tokenization<'t>(
    repaired: Tokenization<'t>,
    page_info: &PageInfo<'t>,
    settings: &WikitextSettings,
    max_recursion_depth: usize,
) -> ParseOutcome<SyntaxTree<'t>> {
    let source: String = repaired.full_text().inner().into();
    let remapped: Vec<ExtractedToken<'_>> = repaired
        .tokens()
        .iter()
        .map(|token| ExtractedToken {
            token: token.token,
            slice: &source[token.span.clone()],
            span: token.span.clone(),
        })
        .collect();
    let owned = crate::tokenize(&source).with_tokens(remapped);
    let (tree, errors) =
        parse_and_convert(&owned, page_info, settings, max_recursion_depth).into();
    ParseOutcome::new(tree.to_owned(), errors)
}

fn parse_and_convert<'r, 't>(
    tokenization: &'r Tokenization<'t>,
    page_info: &'r PageInfo<'t>,
    settings: &'r WikitextSettings,
    max_recursion_depth: usize,
) -> ParseOutcome<SyntaxTree<'t>>
where
    'r: 't,
{
    let parsed = parse_internal_with_recursion_limit(
        page_info,
        settings,
        tokenization,
        max_recursion_depth,
    );
    let result = parsed.result;
    let html_blocks = parsed.html_blocks;
    let code_blocks = parsed.code_blocks;
    let table_of_contents_depths = parsed.table_of_contents_depths;
    let mut footnotes = parsed.footnotes;
    let has_footnote_block = parsed.has_footnote_block;
    let mut bibliographies = parsed.bibliographies;
    let has_generated = tokenization.has_generated();
    let has_suppression_seam_candidate = tokenization.tokens().iter().any(|token| {
        (token.token == Token::LeftComment
            && (has_generated || !settings.layout.legacy()))
            || (token.token == Token::Identifier
                && (token.slice.eq_ignore_ascii_case("iftags")
                    || token.slice.eq_ignore_ascii_case("ifcategory")
                    || (has_generated && token.slice.eq_ignore_ascii_case("span"))))
    });

    let mut toc_indexer = settings.id_indexer();

    debug!("Finished paragraph gathering, matching on consumption");
    match result {
        Ok(success) => {
            let mut elements = success.item;
            let errors = success.errors;
            if has_suppression_seam_candidate {
                crate::delayed::resolve_static_suppressions(
                    &mut elements,
                    settings.layout.legacy(),
                );
                for footnote in &mut footnotes {
                    crate::delayed::resolve_static_suppressions(
                        footnote,
                        settings.layout.legacy(),
                    );
                }
                for bibliography in bibliographies.slice_mut() {
                    for (_, elements) in bibliography.slice_mut() {
                        crate::delayed::resolve_static_suppressions(
                            elements,
                            settings.layout.legacy(),
                        );
                    }
                }
            }
            if settings.layout.legacy() {
                if has_footnote_block {
                    footnote::normalize_wikidot_blocks(
                        &mut elements,
                        footnotes.is_empty(),
                    );
                }
                inline_format::normalize_wikidot_inline_formats(&mut elements);
                for footnote in &mut footnotes {
                    inline_format::normalize_wikidot_inline_formats(footnote);
                }
                for bibliography in bibliographies.slice_mut() {
                    inline_format::normalize_wikidot_bibliography(bibliography);
                }
                let lowered_bibliographies =
                    inline_scope::lower_wikidot_inline_size_scopes_with_bibliographies(
                        &mut elements,
                        Some(&mut bibliographies),
                    );
                heading::normalize_wikidot_headings(&mut elements);
                for footnote in &mut footnotes {
                    inline_scope::lower_wikidot_inline_size_scopes_inline(footnote);
                }
                for (index, bibliography) in
                    bibliographies.slice_mut().iter_mut().enumerate()
                {
                    if !lowered_bibliographies.get(index).copied().unwrap_or(false) {
                        inline_scope::lower_wikidot_bibliography_scopes(bibliography);
                    }
                }
            }
            debug!(
                "Finished parsing into syntax tree ({} errors)",
                errors.len()
            );

            let table_of_contents_depths = table_of_contents_depths
                .into_iter()
                .map(|(depth, contents)| (depth, (), contents));

            let table_of_contents = process_depths((), table_of_contents_depths)
                .into_iter()
                .map(|(_, items)| build_toc_list_element(&mut toc_indexer, items))
                .collect::<Vec<_>>();

            let needs_footnote_block = !footnotes.is_empty() && !has_footnote_block;

            SyntaxTree::from_element_result(
                elements,
                errors,
                (html_blocks, code_blocks),
                table_of_contents,
                (footnotes, needs_footnote_block),
                bibliographies,
                tokenization.full_text().len(),
            )
        }
        Err(error) => {
            error!("Fatal error occurred at highest-level parsing: {error:#?}");
            let wikitext = tokenization.full_text().inner();
            let elements = vec![text!(wikitext)];
            let errors = vec![error];
            let table_of_contents = vec![];
            let footnotes = vec![];
            let needs_footnote_block = true;
            let bibliographies = BibliographyList::new();

            SyntaxTree::from_element_result(
                elements,
                errors,
                (html_blocks, code_blocks),
                table_of_contents,
                (footnotes, needs_footnote_block),
                bibliographies,
                tokenization.full_text().len(),
            )
        }
    }
}

/// A run of four `[` at a token boundary lexes as a single bracket plus
/// a triple-link opener, which hides the two readings Wikidot gives the
/// same bytes: a block whose head opens with an iftags segment (the
/// second `[[` opens the segment inside the head), and a block preceded
/// by two literal brackets (the block opens at the second `[[`). Each
/// candidate pair is re-split and the reading whose block actually
/// parses is committed; a complete segment in the prefix position also
/// commits the segment reading so a refused head falls back to its
/// resolved literal form instead of manufacturing a link from the
/// segment's text. Anything else keeps the original tokens.
fn repair_bracket_run_tokenization<'t>(
    tokenization: &Tokenization<'t>,
    page_info: &PageInfo<'t>,
    settings: &WikitextSettings,
) -> Option<Tokenization<'t>> {
    if tokenization.has_generated() {
        return None;
    }
    let source = tokenization.full_text().inner();
    let mut tokens: Vec<ExtractedToken<'t>> = tokenization.tokens().to_vec();
    let mut repaired = false;
    let mut index = 0;
    // Monotone forward scan backing the cheap pre-gate for the probes
    // below: a candidate that has nothing left which could end a block
    // head cannot form a block, so it never needs a parser built for it.
    let mut head_gate = BracketRunHeadGate::default();
    while index + 1 < tokens.len() {
        let comment_head = tokens[index].token == Token::LeftLink
            && tokens[index].span.end - tokens[index].span.start == 3
            && source[tokens[index].span.start..].starts_with("[[[!--")
            && tokens.get(index + 1).is_some_and(|token| {
                token.token == Token::Other
                    && token.slice == "!"
                    && token.span.start == tokens[index].span.start + 3
                    && token.span.end == tokens[index].span.start + 4
            })
            && tokens.get(index + 2).is_some_and(|token| {
                token.token == Token::DoubleDash
                    && token.slice == "--"
                    && token.span.start == tokens[index].span.start + 4
                    && token.span.end == tokens[index].span.start + 6
            });
        if comment_head
            && head_gate.has_head_terminator_at_or_after(&tokens, source, index + 3)
        {
            let start = tokens[index].span.start;
            let token_at = |token: Token, span: std::ops::Range<usize>| ExtractedToken {
                token,
                slice: &source[span.clone()],
                span,
            };
            let mut candidate = tokens.clone();
            candidate.splice(
                index..index + 3,
                [
                    token_at(Token::LeftBlock, start..start + 2),
                    token_at(Token::LeftComment, start + 2..start + 6),
                ],
            );
            if bracket_run_block_parses(
                &candidate,
                index,
                tokenization,
                page_info,
                settings,
            ) || bracket_run_comment_head_resolves(
                &candidate,
                index,
                tokenization,
                page_info,
                settings,
            ) {
                tokens = candidate;
                repaired = true;
                index += 2;
                head_gate.reset(index);
                continue;
            }
        }
        let pair = tokens[index].token == Token::LeftBracket
            && tokens[index + 1].token == Token::LeftLink
            && tokens[index + 1].span.start == tokens[index].span.end
            && tokens[index].span.end - tokens[index].span.start == 1;
        if !pair {
            index += 1;
            continue;
        }
        if !head_gate.has_head_terminator_at_or_after(&tokens, source, index + 2) {
            index += 1;
            continue;
        }
        let start = tokens[index].span.start;
        let token_at = |token: Token, span: std::ops::Range<usize>| ExtractedToken {
            token,
            slice: &source[span.clone()],
            span,
        };
        let mut candidate = tokens.clone();
        candidate.splice(
            index..index + 2,
            [
                token_at(Token::LeftBlock, start..start + 2),
                token_at(Token::LeftBlock, start + 2..start + 4),
            ],
        );
        if bracket_run_block_parses(&candidate, index, tokenization, page_info, settings)
        {
            tokens = candidate;
            repaired = true;
            index += 2;
            head_gate.reset(index);
            continue;
        }
        let segment_candidate = candidate;
        let mut candidate = tokens.clone();
        candidate.splice(
            index..index + 2,
            [
                token_at(Token::LeftBracket, start..start + 1),
                token_at(Token::LeftBracket, start + 1..start + 2),
                token_at(Token::LeftBlock, start + 2..start + 4),
            ],
        );
        if bracket_run_block_parses(
            &candidate,
            index + 2,
            tokenization,
            page_info,
            settings,
        ) {
            tokens = candidate;
            repaired = true;
            index += 3;
            head_gate.reset(index);
            continue;
        }
        if bracket_run_segment_prefix(
            &segment_candidate,
            index + 1,
            tokenization,
            page_info,
            settings,
            source,
        ) {
            tokens = segment_candidate;
            repaired = true;
            index += 2;
            head_gate.reset(index);
            continue;
        }
        index += 1;
    }
    repaired.then(|| tokenization.with_tokens(tokens))
}

/// Monotone forward scan that answers the repair probes' cheap pre-gate:
/// does the token stream at or after a position still contain anything
/// that can end a block head?
///
/// `RULE_BLOCK` cannot accept a re-split candidate unless its block head
/// collects successfully. Every success path of the head-word collection
/// (`collect_text_resolved_impl`, and the raw `collect_text_keep` used by
/// the residual-opener head) terminates on a token that ends the head:
/// whitespace, a line break, a paragraph break, `]]`, `]]]`, or `]` (the
/// fused-close path, conservatively included). A fused `[[iftags ...]]`
/// segment inside the head can also close it, so a `[[` that may start one
/// counts as a candidate too. Without either, the collection runs to end
/// of input and fails, so the probe can only report failure.
///
/// Callers query non-decreasing positions, so a repair pass visits each
/// token at most once; `reset` is only needed after committing a repair,
/// which rewrites the token list.
#[derive(Debug, Default)]
struct BracketRunHeadGate {
    next: Option<usize>,
    scanned: usize,
}

impl BracketRunHeadGate {
    fn has_head_terminator_at_or_after(
        &mut self,
        tokens: &[ExtractedToken<'_>],
        source: &str,
        from: usize,
    ) -> bool {
        match self.next {
            Some(next) if next >= from => return true,
            None if self.scanned > from => return false,
            _ => {}
        }
        let mut index = self.scanned.max(from);
        while index < tokens.len() {
            if bracket_run_token_can_end_head(tokens, index, source) {
                self.next = Some(index);
                self.scanned = index + 1;
                return true;
            }
            index += 1;
        }
        self.next = None;
        self.scanned = tokens.len();
        false
    }

    fn reset(&mut self, index: usize) {
        self.next = None;
        self.scanned = index;
    }
}

fn bracket_run_token_can_end_head(
    tokens: &[ExtractedToken<'_>],
    index: usize,
    source: &str,
) -> bool {
    match tokens[index].token {
        Token::Whitespace
        | Token::LineBreak
        | Token::ParagraphBreak
        | Token::RightBlock
        | Token::RightLink
        | Token::RightBracket => true,
        Token::LeftBlock => bracket_run_iftags_head_may_start(tokens, index, source),
        _ => false,
    }
}

/// Superset of the fused `[[iftags` segment check that `iftags_segment_fused`
/// performs before scanning a segment: the token must be `[[` and the head
/// word must spell `iftags`. Being a superset is safe here; it can only
/// make the repair run a probe it would have run anyway.
fn bracket_run_iftags_head_may_start(
    tokens: &[ExtractedToken<'_>],
    index: usize,
    source: &str,
) -> bool {
    let bytes = source.as_bytes();
    let mut offset = tokens[index].span.end;
    while matches!(bytes.get(offset), Some(b' ' | b'\t')) {
        offset += 1;
    }
    const NAME: &[u8] = b"iftags";
    if !bytes
        .get(offset..offset + NAME.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(NAME))
    {
        return false;
    }
    matches!(
        bytes.get(offset + NAME.len()),
        None | Some(b' ' | b'\t' | b'\r' | b'\n' | b']')
    )
}

fn bracket_run_block_parses<'t>(
    tokens: &[ExtractedToken<'t>],
    target: usize,
    tokenization: &Tokenization<'t>,
    page_info: &PageInfo<'t>,
    settings: &WikitextSettings,
) -> bool {
    let candidate = tokenization.with_tokens(tokens.to_vec());
    let mut parser = Parser::new(&candidate, page_info, settings);
    for _ in 0..target {
        if parser.step().is_err() {
            return false;
        }
    }
    parser.current().token == Token::LeftBlock
        && self::rule::impls::RULE_BLOCK
            .try_consume(&mut parser)
            .is_ok()
}

fn bracket_run_comment_head_resolves<'t>(
    tokens: &[ExtractedToken<'t>],
    target: usize,
    tokenization: &Tokenization<'t>,
    page_info: &PageInfo<'t>,
    settings: &WikitextSettings,
) -> bool {
    let candidate = tokenization.with_tokens(tokens.to_vec());
    let mut parser = Parser::new(&candidate, page_info, settings);
    for _ in 0..target {
        if parser.step().is_err() {
            return false;
        }
    }
    parser.current().token == Token::LeftBlock
        && self::rule::impls::head_word_names_known_block(&mut parser)
}

fn bracket_run_segment_prefix<'t>(
    tokens: &[ExtractedToken<'t>],
    target: usize,
    tokenization: &Tokenization<'t>,
    page_info: &PageInfo<'t>,
    settings: &WikitextSettings,
    source: &'t str,
) -> bool {
    let candidate = tokenization.with_tokens(tokens.to_vec());
    let mut parser = Parser::new(&candidate, page_info, settings);
    for _ in 0..target {
        if parser.step().is_err() {
            return false;
        }
    }
    let Some(segment) = self::collect::iftags_segment_fused(&parser) else {
        return false;
    };
    let mut word = String::new();
    if segment.include_body {
        word.push_str(&source[segment.body.clone()]);
    }
    let rest = &source[segment.range.end..];
    let tail: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
        .collect();
    word.push_str(&tail);
    if word.is_empty() {
        return false;
    }
    let after = &rest[tail.len()..];
    after.is_empty() || after.starts_with(']') || after.starts_with(char::is_whitespace)
}

/// Runs the parser, but returns the raw internal results prior to conversion.
pub fn parse_internal<'r, 't>(
    page_info: &'r PageInfo<'t>,
    settings: &'r WikitextSettings,
    tokenization: &'r Tokenization<'t>,
) -> UnstructuredParseResult<'r, 't>
where
    'r: 't,
{
    parse_internal_with_recursion_limit(
        page_info,
        settings,
        tokenization,
        DEFAULT_MAX_RECURSION_DEPTH,
    )
}

fn parse_internal_with_recursion_limit<'r, 't>(
    page_info: &'r PageInfo<'t>,
    settings: &'r WikitextSettings,
    tokenization: &'r Tokenization<'t>,
    max_recursion_depth: usize,
) -> UnstructuredParseResult<'r, 't>
where
    'r: 't,
{
    let mut parser = if max_recursion_depth == DEFAULT_MAX_RECURSION_DEPTH {
        Parser::new(tokenization, page_info, settings)
    } else {
        Parser::new_with_recursion_limit(
            tokenization,
            page_info,
            settings,
            max_recursion_depth,
        )
    };

    debug!("Running parser on {} tokens", tokenization.tokens().len());
    let result = gather_paragraphs(&mut parser, RULE_PAGE, no_close_condition());

    let html_blocks = parser.remove_html_blocks();
    let code_blocks = parser.remove_code_blocks();
    let table_of_contents_depths = parser.remove_table_of_contents();
    let footnotes = parser.remove_footnotes();
    let has_footnote_block = parser.has_footnote_block();
    let bibliographies = parser.remove_bibliographies();

    UnstructuredParseResult {
        result,
        html_blocks,
        code_blocks,
        table_of_contents_depths,
        footnotes,
        has_footnote_block,
        bibliographies,
    }
}

fn build_toc_list_element(
    incr: &mut Incrementer,
    list: DepthList<(), String>,
) -> Element<'static> {
    let build_item = |item| match item {
        DepthItem::List(_, list) => ListItem::SubList {
            element: Box::new(build_toc_list_element(incr, list)),
        },
        DepthItem::Item(name) => {
            let anchor = match incr.next() {
                None => Cow::Borrowed("javascript:;"),
                Some(index) => Cow::Owned(format!("#toc{index}")),
            };
            let link = Element::Link {
                ltype: LinkType::TableOfContents,
                link: LinkLocation::Url(anchor),
                label: LinkLabel::Text(Cow::Owned(name)),
                target: None,
            };

            ListItem::Elements {
                elements: vec![link],
                attributes: AttributeMap::new(),
            }
        }
    };

    let items = list.into_iter().map(build_item).collect();
    let attributes = AttributeMap::new();

    Element::List {
        ltype: ListType::Bullet,
        items,
        attributes,
        wikidot_explicit_block: false,
    }
}

/// Represents the result of an internal parse.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UnstructuredParseResult<'r, 't> {
    /// The returned result from parsing.
    pub result: ParseResult<'r, 't, Vec<Element<'t>>>,

    /// The list of HTML blocks to emit from this page.
    pub html_blocks: Vec<Cow<'t, str>>,

    /// The list of code blocks to emit from this page.
    pub code_blocks: Vec<CodeBlock<'t>>,

    /// The "depths" list for table of content entries.
    ///
    /// Each value is a zero-indexed depth of how
    pub table_of_contents_depths: Vec<(usize, String)>,

    /// The list of footnotes.
    ///
    /// Each entry is a series of elements, in combination
    /// they make the contents of one footnote.
    pub footnotes: Vec<Vec<Element<'t>>>,

    /// Whether a footnote block was placed during parsing.
    pub has_footnote_block: bool,

    /// The list of bibliographies.
    ///
    /// See `src/tree/bibliography.rs`.
    pub bibliographies: BibliographyList<'t>,
}
