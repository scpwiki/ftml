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

mod boolean;
mod check_step;
mod collect;
mod condition;
mod consume;
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
mod wikidot_text;

mod prelude {
    pub use crate::parsing::{
        ExtractedToken, ParseError, ParseErrorKind, ParseResult, ParseSuccess, Token,
        success_elements, success_elements_with_paragraph_safety, success_value,
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
    ParseResult, ParseSuccess, success_elements, success_elements_with_paragraph_safety,
    success_value,
};
pub(crate) use self::rule::impls::decode_semicolon_entities;
pub use self::token::{ExtractedToken, Token};
pub(crate) use self::wikidot_text::{
    discard_wikidot_controls, trim_wikidot_ascii, trim_wikidot_ascii_cow,
};

/// Parse through the given tokens and produce an AST.
///
/// This takes a list of [`ExtractedToken`] items produced by [tokenize](crate::tokenizer::tokenize()).
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

const DEEP_PARSE_STACK_BYTES: usize = 64 * 1024 * 1024;

fn parse_on_current_stack<'r, 't>(
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
        (token.token == Token::LeftComment && has_generated)
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
                    for (_, elements) in bibliography.slice_mut() {
                        inline_format::normalize_wikidot_inline_formats(elements);
                    }
                }
                inline_scope::lower_wikidot_inline_size_scopes(&mut elements);
                heading::normalize_wikidot_headings(&mut elements);
                for footnote in &mut footnotes {
                    inline_scope::lower_wikidot_inline_size_scopes_inline(footnote);
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
