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

use super::clone::elements_to_owned;
use super::{
    Alignment, AnchorTarget, AttributeMap, ContainerType, Element, HeadingLevel,
    ListItem, RubyText, Tab, TableCell, TableRow,
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

/// A Wikidot heading that a collapsible head on its line reopens inside the
/// collapsible shell frames.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct HeadingReopen<'t> {
    pub level: HeadingLevel,
    pub id: Option<Cow<'t, str>>,
}

impl<'t> HeadingReopen<'t> {
    pub fn to_owned(&self) -> HeadingReopen<'static> {
        HeadingReopen {
            level: self.level,
            id: self.id.as_ref().map(|id| Cow::Owned(id.to_string())),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum WikidotBlockShell<'t> {
    Div {
        head: usize,
        attributes: AttributeMap<'t>,
        scored: bool,
    },
    Collapsible {
        head: usize,
        start_open: bool,
        show_text: Option<Cow<'t, str>>,
        hide_text: Option<Cow<'t, str>>,
        show_top: bool,
        show_bottom: bool,
        reopen_heading: Option<HeadingReopen<'t>>,
    },
    Note {
        head: usize,
    },
    Align {
        head: usize,
        alignment: Alignment,
    },
}

impl<'t> WikidotBlockShell<'t> {
    pub fn head_byte(&self) -> usize {
        match self {
            WikidotBlockShell::Div { head, .. }
            | WikidotBlockShell::Collapsible { head, .. }
            | WikidotBlockShell::Note { head }
            | WikidotBlockShell::Align { head, .. } => *head,
        }
    }

    pub fn to_owned(&self) -> WikidotBlockShell<'static> {
        match self {
            WikidotBlockShell::Div {
                head,
                attributes,
                scored,
            } => WikidotBlockShell::Div {
                head: *head,
                attributes: attributes.to_owned(),
                scored: *scored,
            },
            WikidotBlockShell::Collapsible {
                head,
                start_open,
                show_text,
                hide_text,
                show_top,
                show_bottom,
                reopen_heading,
            } => WikidotBlockShell::Collapsible {
                head: *head,
                start_open: *start_open,
                show_text: show_text.as_ref().map(|text| Cow::Owned(text.to_string())),
                hide_text: hide_text.as_ref().map(|text| Cow::Owned(text.to_string())),
                show_top: *show_top,
                show_bottom: *show_bottom,
                reopen_heading: reopen_heading.as_ref().map(HeadingReopen::to_owned),
            },
            WikidotBlockShell::Note { head } => WikidotBlockShell::Note { head: *head },
            WikidotBlockShell::Align { head, alignment } => WikidotBlockShell::Align {
                head: *head,
                alignment: *alignment,
            },
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum WikidotBlockCloseKind<'t> {
    Div {
        head: usize,
        scored: bool,
    },
    Collapsible {
        head: usize,
    },
    CollapsibleCrossedBottom {
        head: usize,
        hide_text: Option<Cow<'t, str>>,
    },
    Note {
        head: usize,
    },
    Align {
        head: usize,
    },
    BibliographyCrossed {
        head: usize,
        frames: usize,
    },
}

impl WikidotBlockCloseKind<'_> {
    pub fn rendered_frames(&self) -> usize {
        match self {
            WikidotBlockCloseKind::Collapsible { .. } => 3,
            WikidotBlockCloseKind::CollapsibleCrossedBottom { .. } => 3,
            WikidotBlockCloseKind::BibliographyCrossed { .. } => 0,
            _ => 1,
        }
    }

    pub fn to_owned(&self) -> WikidotBlockCloseKind<'static> {
        match self {
            WikidotBlockCloseKind::Div { head, scored } => WikidotBlockCloseKind::Div {
                head: *head,
                scored: *scored,
            },
            WikidotBlockCloseKind::Collapsible { head } => {
                WikidotBlockCloseKind::Collapsible { head: *head }
            }
            WikidotBlockCloseKind::CollapsibleCrossedBottom { head, hide_text } => {
                WikidotBlockCloseKind::CollapsibleCrossedBottom {
                    head: *head,
                    hide_text: hide_text
                        .as_ref()
                        .map(|text| Cow::Owned(text.to_string())),
                }
            }
            WikidotBlockCloseKind::Note { head } => {
                WikidotBlockCloseKind::Note { head: *head }
            }
            WikidotBlockCloseKind::Align { head } => {
                WikidotBlockCloseKind::Align { head: *head }
            }
            WikidotBlockCloseKind::BibliographyCrossed { head, frames } => {
                WikidotBlockCloseKind::BibliographyCrossed {
                    head: *head,
                    frames: *frames,
                }
            }
        }
    }

    pub fn head_byte(&self) -> usize {
        match self {
            WikidotBlockCloseKind::Div { head, .. }
            | WikidotBlockCloseKind::Collapsible { head }
            | WikidotBlockCloseKind::CollapsibleCrossedBottom { head, .. }
            | WikidotBlockCloseKind::Note { head }
            | WikidotBlockCloseKind::Align { head }
            | WikidotBlockCloseKind::BibliographyCrossed { head, .. } => *head,
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

    WikidotBlockOpen(WikidotBlockShell<'t>),

    WikidotBlockClose(WikidotBlockCloseKind<'t>),

    WikidotFoster(Vec<Element<'t>>),

    /// The rest of a heading line after a collapsible head, placed inside the
    /// reopened heading frame.
    WikidotCollapsibleTail(Vec<Element<'t>>),

    /// Ends the reopened heading and closes the collapsible shell frames that
    /// the heading line opened.
    WikidotCollapsibleHeadingEnd,

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
            PartialElement::WikidotBlockOpen(_) => "WikidotBlockOpen",
            PartialElement::WikidotBlockClose(_) => "WikidotBlockClose",
            PartialElement::WikidotFoster(_) => "WikidotFoster",
            PartialElement::WikidotCollapsibleTail(_) => "WikidotCollapsibleTail",
            PartialElement::WikidotCollapsibleHeadingEnd => {
                "WikidotCollapsibleHeadingEnd"
            }
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
            | PartialElement::WikidotBlockOpen(_)
            | PartialElement::WikidotBlockClose(_)
            | PartialElement::WikidotFoster(_)
            | PartialElement::WikidotCollapsibleTail(_)
            | PartialElement::WikidotCollapsibleHeadingEnd
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
            PartialElement::WikidotBlockOpen(shell) => {
                PartialElement::WikidotBlockOpen(shell.to_owned())
            }
            PartialElement::WikidotBlockClose(kind) => {
                PartialElement::WikidotBlockClose(kind.to_owned())
            }
            PartialElement::WikidotFoster(elements) => {
                PartialElement::WikidotFoster(elements_to_owned(elements))
            }
            PartialElement::WikidotCollapsibleTail(elements) => {
                PartialElement::WikidotCollapsibleTail(elements_to_owned(elements))
            }
            PartialElement::WikidotCollapsibleHeadingEnd => {
                PartialElement::WikidotCollapsibleHeadingEnd
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
    pub(crate) fn is_wikidot_block_control(&self) -> bool {
        matches!(
            self,
            PartialElement::WikidotBlockOpen(_) | PartialElement::WikidotBlockClose(_)
        )
    }

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
                | (AcceptsPartial::TableRow, PartialElement::WikidotFoster(_))
                | (AcceptsPartial::TableCell, PartialElement::WikidotFoster(_))
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
