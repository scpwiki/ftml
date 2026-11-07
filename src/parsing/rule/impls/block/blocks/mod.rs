/*
 * parsing/rule/impls/block/blocks/mod.rs
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

mod prelude {
    pub use super::super::{Arguments, BlockRule};
    pub use crate::parsing::ParseError;
    pub use crate::parsing::parser::Parser;
    pub use crate::parsing::prelude::*;
    pub use crate::tree::{Container, ContainerType, Element};

    #[cfg(debug_assertions)]
    pub fn assert_generic_name(
        expected_names: &[&str],
        actual_name: &str,
        name_type: &str,
    ) {
        let matched = expected_names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(actual_name));
        assert!(
            matched,
            "Actual {name_type} name doesn't match any expected: {expected_names:?} (was {actual_name})",
        );
    }

    #[cfg(not(debug_assertions))]
    #[inline]
    pub fn assert_generic_name(_: &[&str], _: &str, _: &str) {}

    #[inline]
    pub fn assert_block_name(block_rule: &BlockRule, actual_name: &str) {
        assert_generic_name(block_rule.accepts_names, actual_name, "block")
    }

    pub fn require_block_argument_resolved<'r, 't>(
        parser: &Parser<'r, 't>,
        value: Option<std::borrow::Cow<'t, str>>,
    ) -> Result<std::borrow::Cow<'t, str>, ParseError> {
        match value {
            Some(value) => Ok(value),
            None => Err(parser.make_err(ParseErrorKind::BlockMissingArguments)),
        }
    }

    pub fn require_trimmed_block_argument_resolved<'r, 't>(
        parser: &Parser<'r, 't>,
        value: Option<std::borrow::Cow<'t, str>>,
    ) -> Result<std::borrow::Cow<'t, str>, ParseError> {
        require_block_argument_resolved(parser, value).map(|value| {
            if parser.settings().layout.legacy() {
                crate::parsing::trim_wikidot_ascii_cow(value)
            } else {
                trim_cow(value)
            }
        })
    }

    pub(crate) use crate::parsing::trim_cow;
}

#[macro_use]
mod align;

mod align_center;
mod align_justify;
mod align_left;
mod align_right;
mod anchor;
mod bibcite;
mod bibliography;
pub(crate) use bibliography::wikidot_split_tail_bibitems;
mod blockquote;
mod bold;
mod button;
mod char;
mod checkbox;
mod code;
mod collapsible;
mod date;
mod del;
mod div;
mod embed;
mod embed_audio;
mod embed_video;
mod equation_ref;
mod file;
mod footnote;
mod gallery;
mod hidden;
mod html;
mod ifcategory;
mod iframe;
mod iftags;
mod image;
mod include_elements;
mod include_wikidot;
mod ins;
mod invisible;
mod italics;
mod later;
mod lines;
mod list;
mod mark;
mod math;
mod media;
mod module;
mod monospace;
mod note;
mod paragraph;
mod radio;
mod raw;
mod ruby;
mod size;
mod social;
mod span;
mod strikethrough;
mod subscript;
mod superscript;
mod table;
mod tabs;
mod target;
mod toc;
mod underline;
mod user;

pub use self::align_center::BLOCK_ALIGN_CENTER;
pub use self::align_justify::BLOCK_ALIGN_JUSTIFY;
pub use self::align_left::BLOCK_ALIGN_LEFT;
pub use self::align_right::BLOCK_ALIGN_RIGHT;
pub use self::anchor::BLOCK_ANCHOR;
pub use self::bibcite::BLOCK_BIBCITE;
pub use self::bibliography::BLOCK_BIBLIOGRAPHY;
pub use self::blockquote::BLOCK_BLOCKQUOTE;
pub use self::bold::BLOCK_BOLD;
pub use self::button::BLOCK_BUTTON;
pub use self::char::BLOCK_CHAR;
pub use self::checkbox::BLOCK_CHECKBOX;
pub use self::code::BLOCK_CODE;
pub use self::collapsible::BLOCK_COLLAPSIBLE;
pub(crate) use self::collapsible::{CollapsibleHead, parse_collapsible_head};
pub use self::date::BLOCK_DATE;
pub use self::del::BLOCK_DEL;
pub use self::div::BLOCK_DIV;
pub(crate) use self::div::wikidot_div_head_attributes;
pub use self::embed::BLOCK_EMBED;
pub use self::embed_audio::BLOCK_EMBED_AUDIO;
pub use self::embed_video::BLOCK_EMBED_VIDEO;
pub use self::equation_ref::BLOCK_EQUATION_REF;
pub use self::file::BLOCK_FILE;
pub(in crate::parsing::rule::impls::block) use self::footnote::parse_footnote_ref;
pub use self::footnote::{BLOCK_FOOTNOTE, BLOCK_FOOTNOTE_BLOCK};
pub use self::gallery::BLOCK_GALLERY;
pub use self::hidden::BLOCK_HIDDEN;
pub use self::html::BLOCK_HTML;
pub use self::ifcategory::BLOCK_IFCATEGORY;
pub use self::iframe::BLOCK_IFRAME;
pub(crate) use self::iframe::wikidot_greedy_tail;
pub use self::iftags::BLOCK_IFTAGS;
pub(crate) use self::iftags::{iftags_conditions_match, iftags_head_keeps_inline_body};
pub use self::image::BLOCK_IMAGE;
pub use self::include_elements::BLOCK_INCLUDE_ELEMENTS;
pub use self::include_wikidot::BLOCK_INCLUDE_WIKIDOT;
pub use self::ins::BLOCK_INS;
pub use self::invisible::BLOCK_INVISIBLE;
pub use self::italics::BLOCK_ITALICS;
pub use self::later::BLOCK_LATER;
pub use self::lines::BLOCK_LINES;
pub(crate) use self::list::parse_wikidot_implicit_list_block;
pub use self::list::{BLOCK_LI, BLOCK_OL, BLOCK_UL};
pub use self::mark::BLOCK_MARK;
pub use self::math::BLOCK_MATH;
pub(crate) use self::math::wikidot_math_name;
pub use self::media::{BLOCK_AUDIO, BLOCK_VIDEO};
pub use self::module::BLOCK_MODULE;
pub use self::monospace::BLOCK_MONOSPACE;
pub use self::note::BLOCK_NOTE;
pub use self::paragraph::BLOCK_PARAGRAPH;
pub use self::radio::BLOCK_RADIO;
pub use self::raw::BLOCK_RAW;
pub use self::ruby::{BLOCK_RB, BLOCK_RT, BLOCK_RUBY};
pub use self::size::BLOCK_SIZE;
pub use self::social::BLOCK_SOCIAL;
pub use self::span::BLOCK_SPAN;
pub use self::strikethrough::BLOCK_STRIKETHROUGH;
pub use self::subscript::BLOCK_SUBSCRIPT;
pub use self::superscript::BLOCK_SUPERSCRIPT;
pub use self::table::{
    BLOCK_TABLE, BLOCK_TABLE_CELL_HEADER, BLOCK_TABLE_CELL_REGULAR, BLOCK_TABLE_ROW,
};
pub use self::tabs::{BLOCK_TAB, BLOCK_TABVIEW};
pub use self::target::BLOCK_TARGET;
pub use self::toc::BLOCK_TABLE_OF_CONTENTS;
pub use self::underline::BLOCK_UNDERLINE;
pub use self::user::BLOCK_USER;

use crate::parsing::parser::Parser;
use crate::parsing::parser::block_pairs::{WikidotBlockKind, WikidotBlockPair};
use crate::parsing::{ParseError, Token};
use crate::tree::{Element, ListItem, PartialElement};

/// Live Wikidot keeps no trailing `<br>` after an explicit `[[ul]]`/`[[ol]]`
/// block that ends a note, alignment, collapsible or tab body: the block's
/// line break is dropped there, while it survives at the page root, in a plain
/// div and in a table cell.
pub(crate) fn trim_wikidot_trailing_explicit_list_break(elements: &mut Vec<Element<'_>>) {
    if matches!(elements.last(), Some(Element::LineBreak))
        && matches!(
            elements.get(elements.len().saturating_sub(2)),
            Some(Element::List {
                wikidot_explicit_block: true,
                ..
            })
        )
    {
        elements.pop();
    }
}

pub(crate) fn drop_wikidot_trailing_vanish_breaks(elements: &mut Vec<Element<'_>>) {
    let mut popped = false;
    while matches!(elements.last(), Some(Element::LineBreak)) {
        if popped {
            let before = elements.get(elements.len().saturating_sub(2));
            let stops = matches!(before, Some(element) if !element.paragraph_safe()
                && !matches!(element, Element::Container(container)
                    if container.ctype() == crate::tree::ContainerType::Paragraph));
            if stops {
                break;
            }
        }
        elements.pop();
        popped = true;
    }
}

/// Live glues the text runs parked directly after an explicit list in a
/// flat body (align/div/note/collapsible/cell): they are the list's
/// continuation and stay bare, rather than opening a paragraph. A blank
/// line between the list and the text already lands as [LineBreak, Text]
/// and needs no unwrap; a block predecessor keeps following text wrapped
/// (hr/table), so only an immediately preceding list unwraps its paragraph.
pub(crate) fn unwrap_wikidot_paragraph_after_explicit_list(elements: &mut Vec<Element<'_>>) {
    let mut index = 0;
    while index < elements.len() {
        let explicit_list = matches!(
            &elements[index],
            Element::List {
                wikidot_explicit_block: true,
                ..
            }
        );
        let next = index + 1;
        let paragraph = elements
            .get(next)
            .is_some_and(|element| {
                matches!(
                    element,
                    Element::Container(container)
                        if container.ctype() == crate::tree::ContainerType::Paragraph
                )
            });
        if explicit_list && paragraph {
            let Element::Container(container) = elements.remove(next) else {
                unreachable!("the following element was a paragraph");
            };
            let children: Vec<Element<'_>> = container.into();
            let added = children.len();
            elements.splice(next..next, children);
            index += 1 + added;
            continue;
        }
        index += 1;
    }
}

/// Live Wikidot keeps the line break an explicit list leaves at the end of a
/// footnote body, unlike the other paragraph bodies.
pub(crate) fn append_wikidot_footnote_trailing_list_break(
    elements: &mut Vec<Element<'_>>,
) {
    if matches!(
        elements.last(),
        Some(Element::List {
            wikidot_explicit_block: true,
            ..
        })
    ) {
        elements.push(Element::LineBreak);
    }
}

/// Live Wikidot closes a div or note at the closer line of a collapsible
/// opened on a list line inside it: everything in the body gathered after
/// that line belongs outside the block, in order. The boundary is the
/// saved block-pair scan's closer for a list-line collapsible head in
/// the body; elements decide by the source position of the text they
/// carry. Anything without carried text keeps the body's side until the
/// first carried position past the boundary.
// The boundary is carried text, not head offsets: elements store no
// spans, so only the borrowed slices' addresses tell where they came
// from. This is not head-offset math.
#[allow(clippy::disallowed_methods)]
pub(crate) fn wikidot_eject_after_lost_owner_collapsible<'t>(
    parser: &Parser<'_, '_>,
    elements: &mut Vec<Element<'t>>,
    body_start_byte: usize,
    body_end_byte: usize,
) -> Option<Vec<Element<'t>>> {
    let source = parser.full_text().inner();
    let closer_byte = parser
        .wikidot_block_pairs_closed_between(body_start_byte, body_end_byte)
        .into_iter()
        .filter_map(|(_, pair)| {
            if pair.kind == WikidotBlockKind::Collapsible
                && (body_start_byte..body_end_byte).contains(&pair.head_byte)
                && byte_head_on_list_item_line(source, pair.head_byte)
            {
                Some(pair.closer_byte)
            } else {
                None
            }
        })
        .min()?;
    let source_ptr = source.as_ptr() as usize;
    let index = elements.iter().position(|element| {
        element_source_offset(source_ptr, element).is_some_and(|offset| offset > closer_byte)
    })?;
    if index == 0 {
        return None;
    }
    Some(elements.split_off(index))
}

fn byte_head_on_list_item_line(source: &str, byte: usize) -> bool {
    let line_start = source[..byte].rfind('\n').map_or(0, |at| at + 1);
    let line_end = source[byte..]
        .find('\n')
        .map_or(source.len(), |at| byte + at);
    let line = &source[line_start..line_end];
    let after_leading = line.trim_start_matches([' ', '\t']);
    let Some(marker) = after_leading.strip_prefix(['*', '#']) else {
        return false;
    };
    let Some(after_marker) = marker.strip_prefix([' ', '\t']) else {
        return false;
    };
    line_start + (line.len() - after_marker.len()) == byte
}

fn element_source_offset<'t>(source_ptr: usize, element: &Element<'t>) -> Option<usize> {
    let mut result = None;
    scan_element_source_offset(source_ptr, element, &mut result);
    result
}

fn consider_source_ptr(source_ptr: usize, result: &mut Option<usize>, ptr: usize) {
    if ptr >= source_ptr {
        let offset = ptr - source_ptr;
        *result = Some(result.map_or(offset, |current| current.min(offset)));
    }
}

#[allow(clippy::disallowed_methods)]
fn scan_element_source_offset<'t>(
    source_ptr: usize,
    element: &Element<'t>,
    result: &mut Option<usize>,
) {
    match element {
        Element::Text(text)
        | Element::Raw(text)
        | Element::Variable(text)
        | Element::Email(text)
        | Element::AnchorName(text) => {
            if let std::borrow::Cow::Borrowed(text) = text {
                consider_source_ptr(source_ptr, result, text.as_ptr() as usize);
            }
        }
        Element::Container(container) => {
            for child in container.elements() {
                scan_element_source_offset(source_ptr, child, result);
            }
        }
        Element::Collapsible {
            elements,
            show_text,
            hide_text,
            ..
        } => {
            for child in elements {
                scan_element_source_offset(source_ptr, child, result);
            }
            for optional in [show_text, hide_text] {
                if let Some(std::borrow::Cow::Borrowed(text)) = optional {
                    consider_source_ptr(source_ptr, result, text.as_ptr() as usize);
                }
            }
        }
        Element::Anchor { elements, .. } => {
            for child in elements {
                scan_element_source_offset(source_ptr, child, result);
            }
        }
        Element::List { items, .. } => {
            for item in items {
                scan_list_item_source_offset(source_ptr, item, result);
            }
        }
        Element::Table(table) => {
            for row in &table.rows {
                for cell in &row.cells {
                    for child in &cell.elements {
                        scan_element_source_offset(source_ptr, child, result);
                    }
                }
            }
        }
        Element::Partial(partial) => match partial {
            PartialElement::ListItem(item) => {
                scan_list_item_source_offset(source_ptr, item, result);
            }
            PartialElement::WikidotBlockOpen(shell) => {
                consider_source_ptr(source_ptr, result, shell.head_byte());
            }
            PartialElement::WikidotBlockClose(kind) => {
                consider_source_ptr(source_ptr, result, kind.head_byte());
            }
            _ => {}
        },
        _ => {}
    }
}

fn scan_list_item_source_offset<'t>(
    source_ptr: usize,
    item: &ListItem<'t>,
    result: &mut Option<usize>,
) {
    match item {
        ListItem::Elements { elements, .. } => {
            for child in elements {
                scan_element_source_offset(source_ptr, child, result);
            }
        }
        ListItem::SubList { element } => {
            scan_element_source_offset(source_ptr, element, result);
        }
    }
}

pub(crate) fn wikidot_block_pair_for_block_head_byte(
    parser: &Parser<'_, '_>,
    block_opener_start: usize,
) -> Option<WikidotBlockPair> {
    for head_try in [
        block_opener_start,
        block_opener_start.saturating_sub(2),
        block_opener_start.saturating_add(2),
    ] {
        if let Some(pair) = parser.wikidot_block_pair_at_head_byte(head_try) {
            return Some(pair);
        }
    }
    None
}

pub(crate) fn wikidot_flat_pair_for_head(
    parser: &Parser<'_, '_>,
    opener_start: usize,
) -> Option<WikidotBlockPair> {
    if !parser.settings().layout.legacy() {
        return None;
    }
    if std::env::var("L83NOFLAT").is_ok() {
        return None;
    }
    let pair = wikidot_block_pair_for_block_head_byte(parser, opener_start)?;
    parser.wikidot_pair_goes_flat(&pair).then_some(pair)
}

pub(crate) fn wikidot_scored_div_close_compatible<'r, 't>(
    parser: &Parser<'r, 't>,
    head_byte: usize,
) -> bool
where
    'r: 't,
{
    parser.wikidot_alias_has_compatible_close(&BLOCK_DIV, head_byte)
}

pub(crate) fn wikidot_scan_pairs_head(
    parser: &Parser<'_, '_>,
    opener_start: usize,
) -> bool {
    parser.settings().layout.legacy()
        && !parser.discarding_hidden_body()
        && wikidot_block_pair_for_block_head_byte(parser, opener_start).is_some()
}

pub(crate) fn consume_wikidot_flat_head_break<'r, 't>(
    parser: &mut Parser<'r, 't>,
    keep_break_after_spaces: bool,
) -> Result<(), ParseError>
where
    'r: 't,
{
    if parser.in_wikidot_simple_table_cell()
        || (parser.in_wikidot_single_line_content()
            && parser.accepts_partial() != crate::tree::AcceptsPartial::ListItem)
    {
        return Ok(());
    }
    if parser.current().token == Token::Whitespace {
        let mut lookahead = parser.clone();
        lookahead.step()?;
        if lookahead.current().token != Token::LineBreak {
            return Ok(());
        }
        parser.step()?;
        if keep_break_after_spaces {
            return Ok(());
        }
    }
    if parser.current().token == Token::LineBreak {
        parser.step()?;
    }
    Ok(())
}
