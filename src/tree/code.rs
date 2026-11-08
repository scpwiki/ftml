/*
 * code.rs
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

use super::clone::{elements_to_owned, option_string_to_owned, string_to_owned};
use super::element::Element;
use crate::tree::AttributeMap;
use std::borrow::Cow;

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeBlock<'t> {
    pub contents: Cow<'t, str>,
    pub language: Option<Cow<'t, str>>,
    pub name: Option<Cow<'t, str>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wikidot_block_open_elements: Vec<Element<'t>>,
    /// Inline span/size scopes that cross this Wikidot code block also
    /// wrap its rendered content inside `<pre>`. These are renderer-owned
    /// compatibility wrappers, not authored code attributes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wikidot_label_spans: Vec<AttributeMap<'t>>,
    /// Inline format scopes (bold/italics/underline/…) that straddle this
    /// Wikidot code block as they cross an explicit list also reopen their
    /// wrapper inside `<pre>` around the code contents, like live does when
    /// the format pair forms across the block. Renderer-owned compatibility
    /// wrappers, not authored code attributes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wikidot_code_format_wrappers: Vec<crate::tree::InlineFormatKind>,
}

impl CodeBlock<'_> {
    pub fn to_owned(&self) -> CodeBlock<'static> {
        CodeBlock {
            contents: string_to_owned(&self.contents),
            language: option_string_to_owned(&self.language),
            name: option_string_to_owned(&self.name),
            wikidot_block_open_elements: elements_to_owned(
                &self.wikidot_block_open_elements,
            ),
            wikidot_label_spans: self
                .wikidot_label_spans
                .iter()
                .map(AttributeMap::to_owned)
                .collect(),
            wikidot_code_format_wrappers: self.wikidot_code_format_wrappers.clone(),
        }
    }
}
