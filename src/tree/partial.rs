/*
 * tree/partial.rs
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

use super::{
    AnchorTarget, AttributeMap, ContainerType, ListItem, RubyText, Tab, TableCell, TableRow,
};
use crate::parsing::ParseErrorKind;
use std::borrow::Cow;

#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum InlineFormatKind {
    Bold,
    Italics,
    Underline,
    Superscript,
    Subscript,
    Strikethrough,
    Monospace,
}

impl InlineFormatKind {
    #[inline]
    pub fn container_type(self) -> ContainerType {
        match self {
            InlineFormatKind::Bold => ContainerType::Bold,
            InlineFormatKind::Italics => ContainerType::Italics,
            InlineFormatKind::Underline => ContainerType::Underline,
            InlineFormatKind::Superscript => ContainerType::Superscript,
            InlineFormatKind::Subscript => ContainerType::Subscript,
            InlineFormatKind::Strikethrough => ContainerType::Strikethrough,
            InlineFormatKind::Monospace => ContainerType::Monospace,
        }
    }

    #[inline]
    pub fn open_marker(self) -> &'static str {
        match self {
            InlineFormatKind::Bold => "**",
            InlineFormatKind::Italics => "//",
            InlineFormatKind::Underline => "__",
            InlineFormatKind::Superscript => "^^",
            InlineFormatKind::Subscript => ",,",
            InlineFormatKind::Strikethrough => "--",
            InlineFormatKind::Monospace => "{{",
        }
    }

    #[inline]
    pub fn close_marker(self) -> &'static str {
        match self {
            InlineFormatKind::Monospace => "}}",
            kind => kind.open_marker(),
        }
    }
}

/// Part of an element, as returned by a rule.
///
/// These are used by specific rules attempting to
/// build complex or nested structures. From any other
/// context, they are errors are parsing will fail.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum PartialElement<'t> {
    WikidotEmptyInlineOwner,

    InlineSizeOpen(Cow<'t, str>, Cow<'t, str>),

    InlineSizeClose(Cow<'t, str>),

    InlineSpanOpen(AttributeMap<'t>, Cow<'t, str>, bool),

    InlineSpanClose(Cow<'t, str>),

    InlineFormatOpen(InlineFormatKind),

    InlineFormatClose(InlineFormatKind),

    InlineColorOpen(Cow<'t, str>, bool),

    InlineColorClose,

    InlineAnchorOpen(AttributeMap<'t>, Option<AnchorTarget>),

    InlineAnchorClose(Cow<'t, str>),

    ListItem(ListItem<'t>),

    TableRow(TableRow<'t>),

    TableCell(TableCell<'t>),

    Tab(Tab<'t>),

    RubyText(RubyText<'t>),
}

impl PartialElement<'_> {
    pub fn name(&self) -> &'static str {
        match self {
            PartialElement::WikidotEmptyInlineOwner => "WikidotEmptyInlineOwner",
            PartialElement::InlineSizeOpen(..) => "InlineSizeOpen",
            PartialElement::InlineSizeClose(_) => "InlineSizeClose",
            PartialElement::InlineSpanOpen(..) => "InlineSpanOpen",
            PartialElement::InlineSpanClose(_) => "InlineSpanClose",
            PartialElement::InlineFormatOpen(_) => "InlineFormatOpen",
            PartialElement::InlineFormatClose(_) => "InlineFormatClose",
            PartialElement::InlineColorOpen(..) => "InlineColorOpen",
            PartialElement::InlineColorClose => "InlineColorClose",
            PartialElement::InlineAnchorOpen(..) => "InlineAnchorOpen",
            PartialElement::InlineAnchorClose(_) => "InlineAnchorClose",
            PartialElement::ListItem(_) => "ListItem",
            PartialElement::TableRow(_) => "TableRow",
            PartialElement::TableCell(_) => "TableCell",
            PartialElement::Tab(_) => "Tab",
            PartialElement::RubyText(_) => "RubyText",
        }
    }

    #[inline]
    pub fn parse_error_kind(&self) -> ParseErrorKind {
        match self {
            PartialElement::WikidotEmptyInlineOwner
            | PartialElement::InlineSizeOpen(..)
            | PartialElement::InlineSizeClose(_)
            | PartialElement::InlineSpanOpen(..)
            | PartialElement::InlineSpanClose(_)
            | PartialElement::InlineFormatOpen(_)
            | PartialElement::InlineFormatClose(_)
            | PartialElement::InlineColorOpen(..)
            | PartialElement::InlineColorClose
            | PartialElement::InlineAnchorOpen(..)
            | PartialElement::InlineAnchorClose(_) => ParseErrorKind::NoRulesMatch,
            PartialElement::ListItem(_) => ParseErrorKind::ListItemOutsideList,
            PartialElement::TableRow(_) => ParseErrorKind::TableRowOutsideTable,
            PartialElement::TableCell(_) => ParseErrorKind::TableCellOutsideTable,
            PartialElement::Tab(_) => ParseErrorKind::TabOutsideTabView,
            PartialElement::RubyText(_) => ParseErrorKind::RubyTextOutsideRuby,
        }
    }

    pub fn to_owned(&self) -> PartialElement<'static> {
        match self {
            PartialElement::WikidotEmptyInlineOwner => {
                PartialElement::WikidotEmptyInlineOwner
            }
            PartialElement::InlineSizeOpen(value, head) => {
                PartialElement::InlineSizeOpen(
                    Cow::Owned(value.to_string()),
                    Cow::Owned(head.to_string()),
                )
            }
            PartialElement::InlineSizeClose(source) => {
                PartialElement::InlineSizeClose(Cow::Owned(source.to_string()))
            }
            PartialElement::InlineSpanOpen(attributes, head, separator) => {
                PartialElement::InlineSpanOpen(
                    attributes.to_owned(),
                    Cow::Owned(head.to_string()),
                    *separator,
                )
            }
            PartialElement::InlineSpanClose(source) => {
                PartialElement::InlineSpanClose(Cow::Owned(source.to_string()))
            }
            PartialElement::InlineFormatOpen(kind) => {
                PartialElement::InlineFormatOpen(*kind)
            }
            PartialElement::InlineFormatClose(kind) => {
                PartialElement::InlineFormatClose(*kind)
            }
            PartialElement::InlineColorOpen(color, background) => {
                PartialElement::InlineColorOpen(
                    Cow::Owned(color.to_string()),
                    *background,
                )
            }
            PartialElement::InlineColorClose => PartialElement::InlineColorClose,
            PartialElement::InlineAnchorOpen(attributes, target) => {
                PartialElement::InlineAnchorOpen(attributes.to_owned(), *target)
            }
            PartialElement::InlineAnchorClose(source) => {
                PartialElement::InlineAnchorClose(Cow::Owned(source.to_string()))
            }
            PartialElement::ListItem(list_item) => {
                PartialElement::ListItem(list_item.to_owned())
            }
            PartialElement::TableRow(table_row) => {
                PartialElement::TableRow(table_row.to_owned())
            }
            PartialElement::TableCell(table_cell) => {
                PartialElement::TableCell(table_cell.to_owned())
            }
            PartialElement::Tab(tab) => PartialElement::Tab(tab.to_owned()),
            PartialElement::RubyText(text) => PartialElement::RubyText(text.to_owned()),
        }
    }

    #[inline]
    pub fn paragraph_safe(&self) -> bool {
        self.is_inline_format_control()
    }

    #[inline]
    pub(crate) fn is_inline_format_control(&self) -> bool {
        matches!(
            self,
            PartialElement::WikidotEmptyInlineOwner
                | PartialElement::InlineSizeOpen(..)
                | PartialElement::InlineSizeClose(_)
                | PartialElement::InlineSpanOpen(..)
                | PartialElement::InlineSpanClose(_)
                | PartialElement::InlineFormatOpen(_)
                | PartialElement::InlineFormatClose(_)
                | PartialElement::InlineColorOpen(..)
                | PartialElement::InlineColorClose
                | PartialElement::InlineAnchorOpen(..)
                | PartialElement::InlineAnchorClose(_)
        )
    }
}

#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash)]
pub enum AcceptsPartial {
    #[default]
    None,
    ListItem,
    TableRow,
    TableCell,
    Tab,
    Ruby,
}

impl AcceptsPartial {
    pub fn matches(self, partial: &PartialElement) -> bool {
        matches!(
            (self, partial),
            (AcceptsPartial::ListItem, PartialElement::ListItem(_))
                | (AcceptsPartial::TableRow, PartialElement::TableRow(_))
                | (AcceptsPartial::TableCell, PartialElement::TableCell(_))
                | (AcceptsPartial::Tab, PartialElement::Tab(_))
                | (AcceptsPartial::Ruby, PartialElement::RubyText(_))
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{AttributeMap, ListItem, RubyText, Tab, TableCell, TableRow};
    use std::num::NonZeroU32;

    fn partials() -> Vec<(PartialElement<'static>, &'static str, ParseErrorKind)> {
        vec![
            (
                PartialElement::ListItem(ListItem::Elements {
                    attributes: AttributeMap::new(),
                    elements: vec![text!("item")],
                }),
                "ListItem",
                ParseErrorKind::ListItemOutsideList,
            ),
            (
                PartialElement::TableRow(TableRow {
                    attributes: AttributeMap::new(),
                    cells: vec![],
                }),
                "TableRow",
                ParseErrorKind::TableRowOutsideTable,
            ),
            (
                PartialElement::TableCell(TableCell {
                    header: false,
                    column_span: NonZeroU32::new(1).unwrap(),
                    align: None,
                    attributes: AttributeMap::new(),
                    elements: vec![text!("cell")],
                }),
                "TableCell",
                ParseErrorKind::TableCellOutsideTable,
            ),
            (
                PartialElement::Tab(Tab {
                    label: cow!("tab"),
                    elements: vec![text!("contents")],
                }),
                "Tab",
                ParseErrorKind::TabOutsideTabView,
            ),
            (
                PartialElement::RubyText(RubyText {
                    attributes: AttributeMap::new(),
                    elements: vec![text!("annotation")],
                }),
                "RubyText",
                ParseErrorKind::RubyTextOutsideRuby,
            ),
        ]
    }

    #[test]
    fn partial_element_helpers_cover_all_variants() {
        for (partial, name, parse_error_kind) in partials() {
            assert_eq!(partial.name(), name);
            assert_eq!(partial.parse_error_kind(), parse_error_kind);
            assert_eq!(partial.to_owned(), partial);
        }
        for partial in [
            PartialElement::InlineSizeOpen(cow!("font-size: larger;"), cow!("")),
            PartialElement::InlineSizeClose(cow!("[[/SiZe]]")),
            PartialElement::InlineSpanOpen(AttributeMap::new(), cow!(""), false),
            PartialElement::InlineSpanClose(cow!("[[/span]]")),
            PartialElement::InlineFormatOpen(InlineFormatKind::Bold),
            PartialElement::InlineFormatClose(InlineFormatKind::Monospace),
            PartialElement::InlineColorOpen(cow!("red"), false),
            PartialElement::InlineColorClose,
            PartialElement::InlineAnchorOpen(AttributeMap::new(), None),
            PartialElement::InlineAnchorClose(cow!("[[/a]]")),
        ] {
            assert!(partial.is_inline_format_control());
            assert_eq!(partial.to_owned(), partial);
        }
    }

    #[test]
    fn accepts_partial_matches_only_expected_variants() {
        let partials = partials();
        let acceptances = [
            AcceptsPartial::ListItem,
            AcceptsPartial::TableRow,
            AcceptsPartial::TableCell,
            AcceptsPartial::Tab,
            AcceptsPartial::Ruby,
        ];
        assert_eq!(
            partials.len(),
            acceptances.len(),
            "partials and acceptances arrays must have the same length",
        );

        for (index, (partial, _, _)) in partials.iter().enumerate() {
            assert!(!AcceptsPartial::None.matches(partial));

            for (acceptance_index, acceptance) in acceptances.iter().copied().enumerate()
            {
                assert_eq!(
                    acceptance.matches(partial),
                    acceptance_index == index,
                    "{acceptance:?} matched {} unexpectedly",
                    partial.name(),
                );
            }
        }
    }
}
