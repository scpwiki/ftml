/*
 * parsing/rule/impls/inline_delimiter.rs
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

use super::prelude::*;
use crate::parsing::ParseSuccess;
use crate::parsing::collect::{SegmentCtx, TokenWindow, split_delimiter_end};
use crate::tree::Container;
use std::borrow::Cow;

pub(super) fn assert_unpadded_open<'r, 't>(
    parser: &mut Parser<'r, 't>,
    token: Token,
) -> Result<(), ParseError> {
    assert_step(parser, token)?;
    if parser.current().token == Token::Whitespace {
        Err(parser.make_err(ParseErrorKind::RuleFailed))
    } else {
        Ok(())
    }
}

pub(super) fn append_wikidot_line_close_space(
    elements: Elements<'_>,
    wikidot: bool,
) -> Elements<'_> {
    let closes_at_line_start = wikidot
        && matches!(
            &elements,
            Elements::Single(Element::Container(container))
                if matches!(container.elements().last(), Some(Element::LineBreak))
        );
    if !closes_at_line_start {
        return elements;
    }
    match elements {
        Elements::Single(element) => Elements::Multiple(vec![element, text!(" ")]),
        _ => unreachable!("line-close check requires one container"),
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum ReachableCloseMode {
    Standard,
    BlockBarrier,
}

pub(super) fn validate_reachable_close(
    parser: &Parser<'_, '_>,
    token: Token,
    split: Option<SplitDelimiter>,
    mode: ReachableCloseMode,
) -> Result<(), ParseError> {
    let tokens = parser.remaining();
    let mut paragraph_error = None;
    let mut open_blocks: Vec<String> = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if let Some(delimiter) = split
            && delimiter.is_half(&tokens[index])
            && tokens.get(index + 1).is_some_and(|next| {
                matches!(next.token, Token::LeftBlock | Token::LeftComment)
            })
            && TokenWindow::from_slice(&tokens[index..]).is_some_and(|window| {
                split_delimiter_end(window, SegmentCtx::from_parser(parser), delimiter)
                    .is_some()
            })
        {
            if let Some(error) = paragraph_error {
                return Err(error);
            }
            if mode == ReachableCloseMode::Standard
                && index > 0
                && tokens[index - 1].token == Token::Whitespace
            {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            return Ok(());
        }
        match tokens[index].token {
            Token::InputEnd => {
                return Err(paragraph_error
                    .unwrap_or_else(|| parser.make_err(ParseErrorKind::EndOfInput)));
            }
            Token::ParagraphBreak => {
                paragraph_error
                    .get_or_insert_with(|| parser.make_err(ParseErrorKind::RuleFailed));
            }
            Token::LeftBlock | Token::LeftBlockStar => {
                let name = block_name_at(tokens, index);
                if is_block_name(&name) {
                    open_blocks.push(name);
                }
            }
            Token::LeftLink => {
                if let Some(name) = link_name_at(tokens, index) {
                    open_blocks.push(name);
                }
            }
            Token::LeftBlockEnd => {
                let name = block_name_at(tokens, index);
                if !is_block_name(&name) {
                } else if let Some(pos) = open_blocks.iter().rposition(|n| *n == name) {
                    open_blocks.truncate(pos);
                } else if end_block_fails(parser, &name, mode) {
                    return Err(parser.make_err(ParseErrorKind::RuleFailed));
                }
            }
            current if current == token => {
                if let Some(error) = paragraph_error {
                    return Err(error);
                }
                if mode == ReachableCloseMode::Standard
                    && index > 0
                    && tokens[index - 1].token == Token::Whitespace
                {
                    return Err(parser.make_err(ParseErrorKind::RuleFailed));
                }
                return Ok(());
            }
            _ => {}
        }
        index += 1;
    }
    Err(parser.make_err(ParseErrorKind::EndOfInput))
}

fn end_block_fails(
    parser: &Parser<'_, '_>,
    name: &str,
    mode: ReachableCloseMode,
) -> bool {
    if parser.discarding_hidden_body() && parser.hidden_body_boundary_accepts(name) {
        return true;
    }
    match mode {
        ReachableCloseMode::Standard => {
            parser.body_end_boundary_accepts(name)
                || (name == "span"
                    && (parser.in_wikidot_span_body()
                        || parser.in_wikidot_literal_alias_body()))
                || (name == "a"
                    && (parser.in_wikidot_anchor_body()
                        || parser.in_wikidot_literal_alias_body()))
                || (name == "div"
                    && (parser.in_wikidot_div_body()
                        || parser.in_wikidot_literal_alias_body()))
                || (name == "center" && parser.in_wikidot_center_body())
                || (name == "bibliography" && parser.in_wikidot_bibliography_body())
        }
        ReachableCloseMode::BlockBarrier => {
            matches!(
                name,
                "div" | "collapsible" | "footnote" | "center" | "bibliography"
            ) && (parser.body_end_boundary_accepts(name)
                || (name == "div" && parser.in_wikidot_div_body())
                || (name == "center" && parser.in_wikidot_center_body())
                || (name == "bibliography" && parser.in_wikidot_bibliography_body())
                || (name == "collapsible" && parser.in_wikidot_collapsible()))
        }
    }
}

pub(super) fn block_name_at(tokens: &[ExtractedToken<'_>], index: usize) -> String {
    let mut name = String::new();
    let mut pos = index + 1;
    while let Some(tok) = tokens.get(pos) {
        match tok.token {
            Token::Whitespace
            | Token::LineBreak
            | Token::ParagraphBreak
            | Token::RightBlock => break,
            Token::LeftBlockStar => {}
            Token::LeftBlock => match segment_close_at(tokens, pos) {
                Some(next) => {
                    pos = next;
                    continue;
                }
                None => name.push_str(tok.slice),
            },
            _ => name.push_str(tok.slice),
        }
        pos += 1;
    }
    name.strip_suffix('_').unwrap_or(&name).to_ascii_lowercase()
}

fn segment_head_at(tokens: &[ExtractedToken<'_>], pos: usize) -> Option<usize> {
    let mut inner = pos + 1;
    if tokens
        .get(inner)
        .is_some_and(|tok| tok.token == Token::Whitespace)
    {
        inner += 1;
    }
    let tok = tokens.get(inner)?;
    if tok.token == Token::Identifier
        && (tok.slice.eq_ignore_ascii_case("iftags")
            || tok.slice.eq_ignore_ascii_case("ifcategory"))
    {
        Some(inner + 1)
    } else {
        None
    }
}

fn segment_close_at(tokens: &[ExtractedToken<'_>], start: usize) -> Option<usize> {
    segment_head_at(tokens, start)?;
    let mut depth = 1usize;
    let mut pos = start + 1;
    while let Some(tok) = tokens.get(pos) {
        match tok.token {
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                return None;
            }
            Token::LeftBlock => {
                if segment_head_at(tokens, pos).is_some() {
                    depth += 1;
                }
            }
            Token::LeftBlockEnd => {
                if let Some(after_name) = segment_head_at(tokens, pos) {
                    depth -= 1;
                    if depth == 0 {
                        return match tokens.get(after_name).map(|tok| tok.token) {
                            Some(Token::RightBlock) | Some(Token::RightLink) => {
                                Some(after_name + 1)
                            }
                            _ => None,
                        };
                    }
                }
            }
            _ => {}
        }
        pos += 1;
    }
    None
}

pub(super) fn is_block_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub(super) fn link_name_at(
    tokens: &[ExtractedToken<'_>],
    index: usize,
) -> Option<String> {
    match tokens.get(index + 1) {
        Some(tok) if tok.token == Token::Identifier => Some(tok.slice.to_lowercase()),
        _ => None,
    }
}

pub(super) fn append_split_close_leftover<'t>(
    elements: Elements<'t>,
    leftover: String,
) -> Elements<'t> {
    let text = Element::Text(Cow::Owned(leftover));
    match elements {
        Elements::None => Elements::Single(text),
        Elements::Single(element) => Elements::Multiple(vec![element, text]),
        Elements::Multiple(mut elements) => {
            elements.push(text);
            Elements::Multiple(elements)
        }
    }
}

pub(super) fn prepend_split_close_leftover<'r, 't>(
    collected: ParseSuccess<'r, 't, Elements<'t>>,
    leftover: &str,
) -> ParseSuccess<'r, 't, Elements<'t>> {
    let text = leftover.to_owned();
    collected.map(|elements| match elements {
        Elements::Single(Element::Container(container)) => {
            let mut children = Vec::with_capacity(container.elements().len() + 1);
            children.push(Element::Text(Cow::Owned(text)));
            children.extend(container.elements().iter().cloned());
            Elements::Single(Element::Container(Container::new(
                container.ctype(),
                children,
                container.attributes().clone(),
            )))
        }
        elements => elements,
    })
}
