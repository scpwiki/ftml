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

pub(super) fn validate_reachable_close(
    parser: &Parser<'_, '_>,
    token: Token,
) -> Result<(), ParseError> {
    let tokens = parser.remaining();
    let mut paragraph_error = None;
    let mut open_blocks: Vec<String> = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
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
                } else {
                    return Err(parser.make_err(ParseErrorKind::RuleFailed));
                }
            }
            current if current == token => {
                if let Some(error) = paragraph_error {
                    return Err(error);
                }
                if index > 0 && tokens[index - 1].token == Token::Whitespace {
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

pub(super) fn block_name_at(tokens: &[ExtractedToken<'_>], index: usize) -> String {
    let mut name = String::new();
    for tok in &tokens[index + 1..] {
        match tok.token {
            Token::Whitespace
            | Token::LineBreak
            | Token::ParagraphBreak
            | Token::RightBlock => break,
            Token::LeftBlockStar => continue,
            _ => name.push_str(tok.slice),
        }
    }
    name.strip_suffix('_').unwrap_or(&name).to_ascii_lowercase()
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
