/*
 * parsing/parser.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2025 Wikijump Team
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

use super::RULE_PAGE;
use super::condition::ParseCondition;
use super::prelude::*;
use super::rule::Rule;
use crate::data::PageInfo;
use crate::render::text::TextRender;
use crate::tokenizer::Tokenization;
use crate::tree::{
    AcceptsPartial, Bibliography, BibliographyList, CodeBlock, HeadingLevel,
};
use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;
use std::{mem, ptr};

const MAX_RECURSION_DEPTH: usize = 100;

/// Parser for a set of tokens.
#[derive(Debug, Clone)]
pub struct Parser<'r, 't> {
    // Page and parse information
    page_info: &'r PageInfo<'t>,
    settings: &'r WikitextSettings,

    // Parse state
    current: &'r ExtractedToken<'t>,
    remaining: &'r [ExtractedToken<'t>],
    full_text: FullText<'t>,

    // Rule state
    rule: Rule,
    depth: usize,

    // Table of Contents
    //
    // Schema: Vec<(depth, _, name)>
    //
    // Note: These three are in Rc<_> items so that the Parser
    //       can be cloned. This struct is intended as a
    //       cheap pointer object, with the true contents
    //       here preserved across parser child instances.
    table_of_contents: Rc<RefCell<Vec<(usize, String)>>>,

    // HTML blocks with data to expose
    html_blocks: Rc<RefCell<Vec<Cow<'t, str>>>>,

    // Code blocks with data to expose
    code_blocks: Rc<RefCell<Vec<CodeBlock<'t>>>>,

    // Footnotes
    //
    // Schema: Vec<List of elements in a footnote>
    footnotes: Rc<RefCell<Vec<Vec<Element<'t>>>>>,

    // Bibliographies
    //
    // Each bibliography block is separate, but the citations
    // can be referenced anywheres, with earlier ones
    // overriding later ones.
    bibliographies: Rc<RefCell<BibliographyList<'t>>>,

    // Flags
    accepts_partial: AcceptsPartial,
    in_footnote: bool, // Whether we're currently inside [[footnote]] ... [[/footnote]].
    has_footnote_block: bool, // Whether a [[footnoteblock]] was created.
    start_of_line: bool,
}

impl<'r, 't> Parser<'r, 't> {
    /// Constructor. Should only be created by `parse()`.
    ///
    /// All other instances should be `.clone()` or `.clone_with_rule()`d from
    /// the main instance used during parsing.
    pub(crate) fn new(
        tokenization: &'r Tokenization<'t>,
        page_info: &'r PageInfo<'t>,
        settings: &'r WikitextSettings,
    ) -> Self {
        let full_text = tokenization.full_text();
        let (current, remaining) = tokenization
            .tokens()
            .split_first()
            .expect("Parsed tokens list was empty (expected at least one element)");

        Parser {
            page_info,
            settings,
            current,
            remaining,
            full_text,
            rule: RULE_PAGE,
            depth: 0,
            table_of_contents: make_shared_vec(),
            html_blocks: make_shared_vec(),
            code_blocks: make_shared_vec(),
            footnotes: make_shared_vec(),
            bibliographies: Rc::new(RefCell::new(BibliographyList::new())),
            accepts_partial: AcceptsPartial::None,
            in_footnote: false,
            has_footnote_block: false,
            start_of_line: true,
        }
    }

    // Getters
    #[inline]
    pub fn page_info(&self) -> &PageInfo<'t> {
        self.page_info
    }

    #[inline]
    pub fn settings(&self) -> &WikitextSettings {
        self.settings
    }

    #[inline]
    pub fn full_text(&self) -> FullText<'t> {
        self.full_text
    }

    #[inline]
    pub fn rule(&self) -> Rule {
        self.rule
    }

    #[inline]
    pub fn accepts_partial(&self) -> AcceptsPartial {
        self.accepts_partial
    }

    #[inline]
    pub fn in_footnote(&self) -> bool {
        self.in_footnote
    }

    #[inline]
    pub fn has_footnote_block(&self) -> bool {
        self.has_footnote_block
    }

    #[inline]
    pub fn start_of_line(&self) -> bool {
        self.start_of_line
    }

    // Setters
    #[inline]
    pub fn set_rule(&mut self, rule: Rule) {
        self.rule = rule;
    }

    pub fn clone_with_rule(&self, rule: Rule) -> Self {
        let mut clone = self.clone();
        clone.set_rule(rule);
        clone
    }

    pub fn depth_increment(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        trace!("Incrementing recursion depth to {}", self.depth);

        if self.depth > MAX_RECURSION_DEPTH {
            return Err(self.make_err(ParseErrorKind::RecursionDepthExceeded));
        }

        Ok(())
    }

    #[inline]
    pub fn depth_decrement(&mut self) {
        self.depth -= 1;
        trace!("Decrementing recursion depth to {}", self.depth);
    }

    #[inline]
    pub fn set_accepts_partial(&mut self, value: AcceptsPartial) {
        self.accepts_partial = value;
    }

    #[inline]
    pub fn set_footnote_flag(&mut self, value: bool) {
        self.in_footnote = value;
    }

    #[inline]
    pub fn set_footnote_block(&mut self) {
        self.has_footnote_block = true;
    }

    /// Gets the parser's mutable state to enable resetting later if needed.
    ///
    /// See `reset_mutable_state()`.
    pub fn get_mutable_state(&self) -> ParserMutableState {
        ParserMutableState {
            footnote_index: self.footnotes.borrow().len(),
            html_block_index: self.html_blocks.borrow().len(),
            code_block_index: self.code_blocks.borrow().len(),
            table_of_contents_index: self.table_of_contents.borrow().len(),
        }
    }

    /// Reset the parser's mutable state to the point described in the struct.
    ///
    /// This structure should be retrieved from `get_mutable_state()`.
    pub fn reset_mutable_state(
        &mut self,
        ParserMutableState {
            footnote_index,
            html_block_index,
            code_block_index,
            table_of_contents_index,
        }: ParserMutableState,
    ) {
        self.footnotes.borrow_mut().truncate(footnote_index);
        self.html_blocks.borrow_mut().truncate(html_block_index);
        self.code_blocks.borrow_mut().truncate(code_block_index);
        self.table_of_contents
            .borrow_mut()
            .truncate(table_of_contents_index);
    }

    // Parse settings helpers
    pub fn check_page_syntax(&self) -> Result<(), ParseError> {
        if self.settings.enable_page_syntax {
            Ok(())
        } else {
            Err(self.make_err(ParseErrorKind::NotSupportedMode))
        }
    }

    /// Add heading element to table of contents.
    pub fn push_table_of_contents_entry(
        &mut self,
        heading: HeadingLevel,
        name_elements: &[Element],
    ) {
        // Headings are 1-indexed (e.g. H1), but depth lists are 0-indexed
        let level = usize::from(heading.value()) - 1;

        // Render name as text, so it lacks formatting
        let name =
            TextRender.render_partial(name_elements, self.page_info, self.settings, 0);

        self.table_of_contents.borrow_mut().push((level, name));
    }

    #[cold]
    pub fn remove_html_blocks(&mut self) -> Vec<Cow<'t, str>> {
        mem::take(&mut self.html_blocks.borrow_mut())
    }

    #[cold]
    pub fn remove_code_blocks(&mut self) -> Vec<CodeBlock<'t>> {
        mem::take(&mut self.code_blocks.borrow_mut())
    }

    #[cold]
    pub fn remove_table_of_contents(&mut self) -> Vec<(usize, String)> {
        mem::take(&mut self.table_of_contents.borrow_mut())
    }

    // Footnotes
    pub fn push_footnote(&mut self, contents: Vec<Element<'t>>) {
        self.footnotes.borrow_mut().push(contents);
    }

    pub fn footnote_count(&self) -> usize {
        self.footnotes.borrow().len()
    }

    pub fn truncate_footnotes(&mut self, count: usize) {
        self.footnotes.borrow_mut().truncate(count);
    }

    #[cold]
    pub fn remove_footnotes(&mut self) -> Vec<Vec<Element<'t>>> {
        mem::take(&mut self.footnotes.borrow_mut())
    }

    // Blocks
    pub fn push_html_block(&mut self, new_block: Cow<'t, str>) {
        self.html_blocks.borrow_mut().push(new_block);
    }

    pub fn push_code_block(&mut self, new_block: CodeBlock<'t>) {
        // NOTE: We do not check if code block names are unique.
        //       It is the responsibility of downstream callers
        //       (such as deepwell) to handle these when doing
        //       hosted text block processing.
        self.code_blocks.borrow_mut().push(new_block);
    }

    // Bibliography
    pub fn push_bibliography(&mut self, bibliography: Bibliography<'t>) -> usize {
        let mut guard = self.bibliographies.borrow_mut();
        let index = guard.next_index();
        guard.push(bibliography);
        index
    }

    #[cold]
    pub fn remove_bibliographies(&mut self) -> BibliographyList<'t> {
        mem::take(&mut self.bibliographies.borrow_mut())
    }

    // Special for [[include]], appending a SyntaxTree
    pub fn append_shared_items(
        &mut self,
        html_blocks: &mut Vec<Cow<'t, str>>,
        code_blocks: &mut Vec<CodeBlock<'t>>,
        table_of_contents: &mut Vec<(usize, String)>,
        footnotes: &mut Vec<Vec<Element<'t>>>,
        bibliographies: &mut BibliographyList<'t>,
    ) {
        self.html_blocks.borrow_mut().append(html_blocks);

        self.code_blocks.borrow_mut().append(code_blocks);

        self.table_of_contents
            .borrow_mut()
            .append(table_of_contents);

        self.footnotes.borrow_mut().append(footnotes);

        self.bibliographies.borrow_mut().append(bibliographies);
    }

    // State evaluation
    pub fn evaluate(&self, condition: ParseCondition) -> bool {
        debug!(
            "Evaluating parser condition (token {}, slice '{}', span {}..{})",
            self.current.token.name(),
            self.current.slice,
            self.current.span.start,
            self.current.span.end,
        );

        match condition {
            ParseCondition::CurrentToken(token) => self.current.token == token,
            ParseCondition::TokenPair(current, next) => {
                if self.current().token != current {
                    trace!(
                        "Current token in pair doesn't match, failing (expected '{}', actual '{}')",
                        current.name(),
                        self.current().token.name(),
                    );
                    return false;
                }

                match self.look_ahead(0) {
                    Some(actual) => {
                        if actual.token != next {
                            trace!(
                                "Second token in pair doesn't match, failing (expected {}, actual {})",
                                next.name(),
                                actual.token.name(),
                            );
                            return false;
                        }
                    }
                    None => {
                        trace!(
                            "Second token in pair doesn't exist (token {})",
                            next.name(),
                        );
                        return false;
                    }
                }

                true
            }
        }
    }

    #[inline]
    pub fn evaluate_any(&self, conditions: &[ParseCondition]) -> bool {
        debug!(
            "Evaluating to see if any parser condition is true (conditions length {})",
            conditions.len(),
        );

        conditions.iter().any(|&condition| self.evaluate(condition))
    }

    #[inline]
    pub fn evaluate_fn<F>(&self, f: F) -> bool
    where
        F: FnOnce(&mut Parser<'r, 't>) -> Result<bool, ParseError>,
    {
        debug!("Evaluating closure for parser condition");
        f(&mut self.clone()).unwrap_or(false)
    }

    pub fn save_evaluate_fn<F>(&mut self, f: F) -> Option<&'r ExtractedToken<'t>>
    where
        F: FnOnce(&mut Parser<'r, 't>) -> Result<bool, ParseError>,
    {
        debug!("Evaluating closure for parser condition, saving progress on success");

        let mut parser = self.clone();
        if f(&mut parser).unwrap_or(false) {
            let last = self.current;
            self.update(&parser);
            Some(last)
        } else {
            None
        }
    }

    // Token pointer state and manipulation
    #[inline]
    pub fn current(&self) -> &'r ExtractedToken<'t> {
        self.current
    }

    #[inline]
    pub fn remaining(&self) -> &'r [ExtractedToken<'t>] {
        self.remaining
    }

    #[inline]
    pub fn update(&mut self, parser: &Parser<'r, 't>) {
        // Flags
        self.accepts_partial = parser.accepts_partial;
        self.in_footnote = parser.in_footnote;
        self.has_footnote_block = parser.has_footnote_block;
        self.start_of_line = parser.start_of_line;

        // Token pointers
        self.current = parser.current;
        self.remaining = parser.remaining;
    }

    #[inline]
    pub fn same_pointer(&self, old_remaining: &'r [ExtractedToken<'t>]) -> bool {
        ptr::eq(self.remaining, old_remaining)
    }

    /// Move the token pointer forward one step.
    ///
    /// # Returns
    /// Returns the new current token.
    #[inline]
    pub fn step(&mut self) -> Result<&'r ExtractedToken<'t>, ParseError> {
        trace!("Stepping to the next token");

        // Set the start-of-line flag.
        self.start_of_line = matches!(
            self.current.token,
            Token::InputStart | Token::LineBreak | Token::ParagraphBreak,
        );

        // Step to the next token.
        match self.remaining.split_first() {
            Some((current, remaining)) => {
                self.current = current;
                self.remaining = remaining;
                Ok(current)
            }
            None => {
                warn!("Exhausted all tokens, yielding end of input error");
                Err(self.make_err(ParseErrorKind::EndOfInput))
            }
        }
    }

    /// Move the token pointer forward `count` steps.
    #[inline]
    pub fn step_n(&mut self, count: usize) -> Result<(), ParseError> {
        trace!("Stepping {count} times");

        for _ in 0..count {
            self.step()?;
        }

        Ok(())
    }

    /// Look for the token `offset + 1` beyond the current one.
    ///
    /// For instance, submitting `0` will yield the first item of `parser.remaining()`.
    #[inline]
    pub fn look_ahead(&self, offset: usize) -> Option<&'r ExtractedToken<'t>> {
        trace!("Looking ahead to a token (offset {offset})");
        self.remaining.get(offset)
    }

    /// Like `look_ahead`, except returns an error if the token isn't found.
    #[inline]
    pub fn look_ahead_err(
        &self,
        offset: usize,
    ) -> Result<&'r ExtractedToken<'t>, ParseError> {
        self.look_ahead(offset)
            .ok_or_else(|| self.make_err(ParseErrorKind::EndOfInput))
    }

    /// Retrieves the current and next tokens.
    pub fn next_two_tokens(&self) -> (Token, Option<Token>) {
        let first = self.current.token;
        let second = self.look_ahead(0).map(|next| next.token);
        (first, second)
    }

    /// Retrieves the current, second, and third tokens.
    pub fn next_three_tokens(&self) -> (Token, Option<Token>, Option<Token>) {
        let first = self.current.token;
        let second = self.look_ahead(0).map(|next| next.token);
        let third = self.look_ahead(1).map(|next| next.token);
        (first, second, third)
    }

    // Helpers to get individual tokens
    pub fn get_token(
        &mut self,
        token: Token,
        kind: ParseErrorKind,
    ) -> Result<&'t str, ParseError> {
        trace!("Looking for token {} (error {})", token.name(), kind.name());

        let current = self.current();
        if current.token == token {
            let text = current.slice;
            self.step()?;
            Ok(text)
        } else {
            Err(self.make_err(kind))
        }
    }

    pub fn get_optional_token(&mut self, token: Token) -> Result<(), ParseError> {
        trace!("Looking for optional token {}", token.name());

        if self.current().token == token {
            self.step()?;
        }

        Ok(())
    }

    pub fn get_optional_line_break(&mut self) -> Result<(), ParseError> {
        debug!("Looking for optional line break");
        self.get_optional_token(Token::LineBreak)
    }

    #[inline]
    pub fn get_optional_space(&mut self) -> Result<(), ParseError> {
        debug!("Looking for optional space");
        self.get_optional_token(Token::Whitespace)
    }

    pub fn get_optional_spaces_any(&mut self) -> Result<(), ParseError> {
        debug!("Looking for optional spaces (any)");

        let tokens = &[
            Token::Whitespace,
            Token::LineBreak,
            Token::ParagraphBreak,
            Token::Equals,
        ];

        loop {
            let current_token = self.current().token;
            if !tokens.contains(&current_token) {
                return Ok(());
            }

            self.step()?;
        }
    }

    // Utilities
    #[cold]
    #[inline]
    pub fn make_err(&self, kind: ParseErrorKind) -> ParseError {
        ParseError::new(kind, self.rule, self.current)
    }
}

/// This struct stores the state of the mutable fields in `Parser`.
///
/// This way, on rule failure, we can revert to the state these
/// fields were in prior to rule execution.
///
/// See the "revert" tests for examples of how this reset data is
/// needed to ensure proper AST formation:
/// * `test/footnotes/revert`
/// * `test/html/revert`
/// * `test/code/revert`
/// * `test/toc/revert`
#[derive(Debug, Copy, Clone)]
pub struct ParserMutableState {
    footnote_index: usize,
    html_block_index: usize,
    code_block_index: usize,
    table_of_contents_index: usize,
}

#[inline]
fn make_shared_vec<T>() -> Rc<RefCell<Vec<T>>> {
    Rc::new(RefCell::new(Vec::new()))
}

// Tests

#[test]
fn parser_newline_flag() {
    use crate::layout::Layout;
    use crate::settings::WikitextMode;

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

    macro_rules! test {
        ($input:expr, $expected_steps:expr $(,)?) => {{
            let tokens = crate::tokenize($input);
            let mut parser = Parser::new(&tokens, &page_info, &settings);
            let mut actual_steps = Vec::new();

            // Iterate through the tokens.
            while let Ok(_) = parser.step() {
                actual_steps.push(parser.start_of_line());
            }

            // Pop off flag corresponding to Token::InputEnd.
            actual_steps.pop();

            assert_eq!(
                &actual_steps, &$expected_steps,
                "Series of start-of-line flags does not match expected",
            );
        }};
    }

    test!("A", [true]);
    test!("A\nB C", [true, false, true, false, false]);
    test!(
        "A\nB\n\nC D\nE",
        [true, false, true, false, true, false, false, false, true],
    );
    test!(
        "\nA\n\nB\n\n\nC D",
        [true, true, false, true, false, true, false, false],
    );
}
