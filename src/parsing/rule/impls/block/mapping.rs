/*
 * parsing/rule/impls/block/mapping.rs
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

use super::{BlockRule, blocks::*};
use std::collections::HashMap;
use std::sync::LazyLock;
use unicase::UniCase;

pub const BLOCK_RULES: [BlockRule; 61] = [
    BLOCK_ALIGN_CENTER,
    BLOCK_ALIGN_JUSTIFY,
    BLOCK_ALIGN_LEFT,
    BLOCK_ALIGN_RIGHT,
    BLOCK_ANCHOR,
    BLOCK_BIBCITE,
    BLOCK_BIBLIOGRAPHY,
    BLOCK_BLOCKQUOTE,
    BLOCK_BOLD,
    BLOCK_CHAR,
    BLOCK_CHECKBOX,
    BLOCK_CODE,
    BLOCK_COLLAPSIBLE,
    BLOCK_DATE,
    BLOCK_DEL,
    BLOCK_DIV,
    BLOCK_EMBED,
    BLOCK_EQUATION_REF,
    BLOCK_FOOTNOTE,
    BLOCK_FOOTNOTE_BLOCK,
    BLOCK_HIDDEN,
    BLOCK_HTML,
    BLOCK_IFCATEGORY,
    BLOCK_IFRAME,
    BLOCK_IFTAGS,
    BLOCK_IMAGE,
    BLOCK_INCLUDE_ELEMENTS,
    BLOCK_INCLUDE_WIKIDOT,
    BLOCK_INS,
    BLOCK_INVISIBLE,
    BLOCK_ITALICS,
    BLOCK_LATER,
    BLOCK_LI,
    BLOCK_LINES,
    BLOCK_MARK,
    BLOCK_MATH,
    BLOCK_MODULE,
    BLOCK_MONOSPACE,
    BLOCK_OL,
    BLOCK_PARAGRAPH,
    BLOCK_RADIO,
    BLOCK_RAW,
    BLOCK_RB,
    BLOCK_RT,
    BLOCK_RUBY,
    BLOCK_SIZE,
    BLOCK_SPAN,
    BLOCK_STRIKETHROUGH,
    BLOCK_SUBSCRIPT,
    BLOCK_SUPERSCRIPT,
    BLOCK_TAB,
    BLOCK_TABLE,
    BLOCK_TABLE_CELL_HEADER,
    BLOCK_TABLE_CELL_REGULAR,
    BLOCK_TABLE_OF_CONTENTS,
    BLOCK_TABLE_ROW,
    BLOCK_TABVIEW,
    BLOCK_TARGET,
    BLOCK_UL,
    BLOCK_UNDERLINE,
    BLOCK_USER,
];

pub type BlockRuleMap = HashMap<UniCase<&'static str>, &'static BlockRule>;

pub static BLOCK_RULE_MAP: LazyLock<BlockRuleMap> =
    LazyLock::new(|| build_block_rule_map(&BLOCK_RULES));

#[inline]
pub fn get_block_rule_with_name(name: &str) -> Option<&'static BlockRule> {
    let name = name.strip_suffix('_').unwrap_or(name); // score flag
    let name = UniCase::ascii(name); // case-insensitive

    BLOCK_RULE_MAP.get(&name).copied()
}

fn build_block_rule_map(block_rules: &'static [BlockRule]) -> BlockRuleMap {
    let mut map = HashMap::new();

    for block_rule in block_rules {
        assert!(
            block_rule.name.starts_with("block-"),
            "Block name does not start with 'block-'.",
        );

        assert!(
            !block_rule.accepts_names.is_empty(),
            "Rule has no accepted names",
        );

        for name in block_rule.accepts_names {
            let name = UniCase::ascii(*name);
            let previous = map.insert(name, block_rule);

            assert!(
                previous.is_none(),
                "Overwrote previous block rule during rule population! Duplicate block detected.",
            );
        }
    }

    map
}

#[test]
fn block_rule_map() {
    let _ = &*BLOCK_RULE_MAP;
}
