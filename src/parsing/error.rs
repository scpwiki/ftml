/*
 * parsing/exception.rs
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

use super::{ExtractedToken, Token, rule::Rule};
use crate::utf16::Utf16IndexMap;
use serde::{Serializer, ser::SerializeTuple};
use std::borrow::Cow;
use std::ops::Range;
use strum_macros::IntoStaticStr;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct ParseError {
    token: Token,
    rule: Cow<'static, str>,
    #[serde(serialize_with = "serialize_span")]
    span: Range<usize>,
    kind: ParseErrorKind,
}

impl ParseError {
    #[inline]
    pub fn new(kind: ParseErrorKind, rule: Rule, current: &ExtractedToken) -> Self {
        let token = current.token;
        let span = Range::clone(&current.span);
        let rule = cow!(rule.name());

        ParseError {
            token,
            rule,
            span,
            kind,
        }
    }

    #[inline]
    pub fn token(&self) -> Token {
        self.token
    }

    #[inline]
    pub fn rule(&self) -> &str {
        &self.rule
    }

    #[inline]
    pub fn span(&self) -> Range<usize> {
        Range::clone(&self.span)
    }

    #[inline]
    pub fn kind(&self) -> ParseErrorKind {
        self.kind
    }

    #[must_use]
    pub fn to_utf16_indices(&self, map: &Utf16IndexMap) -> Self {
        let ParseError {
            token,
            rule,
            span,
            kind,
        } = self.clone();

        let start = map.get_index(span.start);
        let end = map.get_index(span.end);
        let span = start..end;

        ParseError {
            token,
            rule,
            span,
            kind,
        }
    }
}

#[derive(Serialize, Deserialize, IntoStaticStr, Debug, Copy, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ParseErrorKind {
    RecursionDepthExceeded,

    EndOfInput,

    NoRulesMatch,

    RuleFailed,

    NotSupportedMode,

    NotStartOfLine,

    InvalidInclude,

    ListEmpty,

    ListContainsNonItem,

    ListItemOutsideList,

    ListDepthExceeded,

    TableContainsNonRow,

    TableRowContainsNonCell,

    TableRowOutsideTable,

    TableCellOutsideTable,

    TabViewEmpty,

    TabViewContainsNonTab,

    TabOutsideTabView,

    FootnotesNested,

    BlockquoteDepthExceeded,

    RubyTextOutsideRuby,

    BibliographyContainsNonDefinitionList,

    NoSuchBlock,

    BlockDisallowsStar,

    BlockDisallowsScore,

    BlockMissingName,

    BlockMissingCloseBrackets,

    BlockMalformedArguments,

    BlockMissingArguments,

    BlockExpectedEnd,

    BlockEndMismatch,

    NoSuchEmbed,

    NoSuchModule,

    ModuleMissingName,

    NoSuchPage,

    NoSuchVariable,

    InvalidUrl,

    NotSupportedInline,

    AnchorBlockWithInsufficientClosers,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HiddenBodyFailure {
    Recoverable,
    MustCloseThroughEndOfInput,
}

impl ParseErrorKind {
    #[inline]
    pub fn name(self) -> &'static str {
        self.into()
    }

    pub fn recoverable_local_failure() -> Self {
        ParseErrorKind::RuleFailed
    }

    pub fn hidden_body_failure(self) -> HiddenBodyFailure {
        match self {
            ParseErrorKind::RecursionDepthExceeded
            | ParseErrorKind::EndOfInput
            | ParseErrorKind::BlockDisallowsStar
            | ParseErrorKind::BlockDisallowsScore
            | ParseErrorKind::BlockMissingCloseBrackets
            | ParseErrorKind::BlockMalformedArguments
            | ParseErrorKind::BlockMissingArguments => {
                HiddenBodyFailure::MustCloseThroughEndOfInput
            }
            _ => HiddenBodyFailure::Recoverable,
        }
    }

    pub fn must_close_hidden_body_through_end_of_input(self) -> bool {
        self.hidden_body_failure() == HiddenBodyFailure::MustCloseThroughEndOfInput
    }
}

fn serialize_span<S>(span: &Range<usize>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut tuple = serializer.serialize_tuple(2)?;
    tuple.serialize_element(&span.start)?;
    tuple.serialize_element(&span.end)?;
    tuple.end()
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::utf16::Utf16IndexMap;

    #[test]
    fn parse_error_kind_names_match_variant_names() {
        assert_eq!(
            ParseErrorKind::RecursionDepthExceeded.name(),
            "RecursionDepthExceeded",
        );
        assert_eq!(
            ParseErrorKind::BlockMalformedArguments.name(),
            "BlockMalformedArguments",
        );
        assert_eq!(ParseErrorKind::InvalidUrl.name(), "InvalidUrl");
    }

    #[test]
    fn parse_error_to_utf16_indices_converts_span() {
        let text = "a🦀bc";
        let map = Utf16IndexMap::new(text);
        let error = ParseError {
            token: Token::Identifier,
            rule: Cow::Borrowed("rule-text"),
            span: 1..6,
            kind: ParseErrorKind::RuleFailed,
        };

        let converted = error.to_utf16_indices(&map);

        assert_eq!(converted.token(), Token::Identifier);
        assert_eq!(converted.rule(), "rule-text");
        assert_eq!(converted.span(), 1..4);
        assert_eq!(converted.kind(), ParseErrorKind::RuleFailed);
        assert_eq!(error.span(), 1..6);
    }

    #[test]
    fn parse_error_serializes_span_as_tuple() {
        let error = ParseError {
            token: Token::Identifier,
            rule: Cow::Borrowed("rule-text"),
            span: 2..5,
            kind: ParseErrorKind::RuleFailed,
        };

        let value = serde_json::to_value(&error).expect("parse error should serialize");

        assert_eq!(value["span"], serde_json::json!([2, 5]));

        let round_tripped: ParseError = serde_json::from_value(value.clone())
            .expect("serialized parse error should deserialize");
        assert_eq!(round_tripped, error);

        let mut object_span_value = value;
        object_span_value["span"] = serde_json::json!({
            "start": 2,
            "end": 5,
        });

        let object_span_error: ParseError = serde_json::from_value(object_span_value)
            .expect("object-shaped spans should remain deserializable");
        assert_eq!(object_span_error, error);
    }
}
