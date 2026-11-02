/*
 * parsing/rule/impls/block/mapping.rs
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

use super::{BlockRule, blocks::*};
use crate::layout::Layout;
use std::collections::HashMap;
use std::sync::LazyLock;
use unicase::UniCase;

pub const BLOCK_RULES: [BlockRule; 69] = [
    BLOCK_ALIGN_CENTER,
    BLOCK_ALIGN_JUSTIFY,
    BLOCK_ALIGN_LEFT,
    BLOCK_ALIGN_RIGHT,
    BLOCK_ANCHOR,
    BLOCK_AUDIO,
    BLOCK_BIBCITE,
    BLOCK_BIBLIOGRAPHY,
    BLOCK_BLOCKQUOTE,
    BLOCK_BOLD,
    BLOCK_BUTTON,
    BLOCK_CHAR,
    BLOCK_CHECKBOX,
    BLOCK_CODE,
    BLOCK_COLLAPSIBLE,
    BLOCK_DATE,
    BLOCK_DEL,
    BLOCK_DIV,
    BLOCK_EMBED,
    BLOCK_EMBED_VIDEO,
    BLOCK_EQUATION_REF,
    BLOCK_FILE,
    BLOCK_FOOTNOTE,
    BLOCK_FOOTNOTE_BLOCK,
    BLOCK_GALLERY,
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
    BLOCK_NOTE,
    BLOCK_OL,
    BLOCK_PARAGRAPH,
    BLOCK_RADIO,
    BLOCK_RAW,
    BLOCK_RB,
    BLOCK_RT,
    BLOCK_RUBY,
    BLOCK_SIZE,
    BLOCK_SOCIAL,
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
    BLOCK_VIDEO,
];

pub type BlockRuleMap = HashMap<UniCase<&'static str>, &'static BlockRule>;

pub static BLOCK_RULE_MAP: LazyLock<BlockRuleMap> =
    LazyLock::new(|| build_block_rule_map(&BLOCK_RULES));

#[inline]
pub fn get_block_rule_with_name(name: &str) -> Option<&'static BlockRule> {
    let name = name.strip_suffix('_').unwrap_or(name);
    let name = UniCase::ascii(name);

    BLOCK_RULE_MAP.get(&name).copied()
}

#[inline]
pub fn get_block_rule_with_name_for_layout(
    name: &str,
    layout: Layout,
) -> Option<&'static BlockRule> {
    if layout.legacy() && !wikidot_supports_block_name(name) {
        return None;
    }
    get_block_rule_with_name(name)
}

fn wikidot_supports_block_name(name: &str) -> bool {
    const FTML_ONLY_NAMES: &[&str] = &[
        "anchor",
        "anchortarget",
        "audio",
        "b",
        "bibcite",
        "blockquote",
        "bold",
        "checkbox",
        "char",
        "character",
        "del",
        "deletion",
        "em",
        "emphasis",
        "equation",
        "eqref",
        "hidden",
        "highlight",
        "i",
        "include-elements",
        "ins",
        "insertion",
        "invisible",
        "italics",
        "later",
        "lines",
        "mark",
        "mono",
        "monospace",
        "newlines",
        "p",
        "paragraph",
        "quote",
        "radio",
        "radio-button",
        "rb",
        "rt",
        "ruby",
        "ruby2",
        "rubytext",
        "s",
        "strikethrough",
        "strong",
        "target",
        "tt",
        "u",
        "underline",
        "video",
    ];

    let name = name.strip_suffix('_').unwrap_or(name);
    !FTML_ONLY_NAMES
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
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

#[test]
fn block_rule_map_accepts_case_insensitive_names() {
    static BLOCK_RULES: [BlockRule; 1] = [BlockRule {
        name: "block-custom",
        accepts_names: &["CustomBlock"],
        accepts_star: false,
        accepts_score: false,
        accepts_newlines: false,
        parse_fn: BLOCK_BOLD.parse_fn,
    }];

    let map = build_block_rule_map(&BLOCK_RULES);
    assert_eq!(
        map.get(&UniCase::ascii("customblock"))
            .map(|rule| rule.name),
        Some("block-custom"),
    );
}

#[test]
fn wikidot_layout_rejects_ftml_only_block_names() {
    assert!(get_block_rule_with_name_for_layout("anchor", Layout::Wikidot).is_none());
    assert!(get_block_rule_with_name_for_layout("strong", Layout::Wikidot).is_none());
    assert!(get_block_rule_with_name_for_layout("video", Layout::Wikidot).is_none());
    assert_eq!(
        get_block_rule_with_name_for_layout("embed", Layout::Wikidot)
            .map(|rule| rule.name),
        Some("block-embed"),
    );
    assert_eq!(
        get_block_rule_with_name_for_layout("math", Layout::Wikidot)
            .map(|rule| rule.name),
        Some("block-math"),
    );
    assert!(get_block_rule_with_name_for_layout("char", Layout::Wikidot).is_none());
    assert!(get_block_rule_with_name_for_layout("radio", Layout::Wikidot).is_none());
    assert_eq!(
        get_block_rule_with_name_for_layout("note", Layout::Wikidot)
            .map(|rule| rule.name),
        Some("block-note"),
    );
    assert_eq!(
        get_block_rule_with_name_for_layout("anchor", Layout::Wikijump)
            .map(|rule| rule.name),
        Some("block-anchor"),
    );
    assert_eq!(
        get_block_rule_with_name_for_layout("strong", Layout::Wikijump)
            .map(|rule| rule.name),
        Some("block-bold"),
    );
    assert_eq!(
        get_block_rule_with_name_for_layout("div", Layout::Wikidot).map(|rule| rule.name),
        Some("block-div"),
    );
}

#[test]
fn unmatched_case_variant_closers_preserve_source_for_every_block_name() {
    use crate::data::PageInfo;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    for name in BLOCK_RULES
        .iter()
        .flat_map(|block_rule| block_rule.accepts_names)
    {
        let mut uppercase = true;
        let mixed = name
            .chars()
            .map(|character| {
                if character.is_ascii_alphabetic() {
                    let output = if uppercase {
                        character.to_ascii_uppercase()
                    } else {
                        character.to_ascii_lowercase()
                    };
                    uppercase = !uppercase;
                    output
                } else {
                    character
                }
            })
            .collect::<String>();
        let source = format!("[[/{mixed}]]");
        let tokenization = crate::tokenize(&source);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        let escaped_source = source
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");

        assert_eq!(
            html,
            format!("<p>{escaped_source}</p>"),
            "block name {name:?}"
        );
    }
}

#[test]
#[should_panic(expected = "Rule has no accepted names")]
fn block_rule_map_rejects_empty_accepted_names() {
    static BLOCK_RULES: [BlockRule; 1] = [BlockRule {
        name: "block-empty",
        accepts_names: &[],
        accepts_star: false,
        accepts_score: false,
        accepts_newlines: false,
        parse_fn: BLOCK_BOLD.parse_fn,
    }];

    build_block_rule_map(&BLOCK_RULES);
}

#[test]
#[should_panic(expected = "Block name does not start with 'block-'.")]
fn block_rule_map_rejects_invalid_rule_name_prefix() {
    static BLOCK_RULES: [BlockRule; 1] = [BlockRule {
        name: "custom",
        accepts_names: &["custom"],
        accepts_star: false,
        accepts_score: false,
        accepts_newlines: false,
        parse_fn: BLOCK_BOLD.parse_fn,
    }];

    build_block_rule_map(&BLOCK_RULES);
}

#[test]
#[should_panic(expected = "Overwrote previous block rule")]
fn block_rule_map_rejects_duplicate_names() {
    static BLOCK_RULES: [BlockRule; 2] = [
        BlockRule {
            name: "block-first",
            accepts_names: &["duplicate"],
            accepts_star: false,
            accepts_score: false,
            accepts_newlines: false,
            parse_fn: BLOCK_BOLD.parse_fn,
        },
        BlockRule {
            name: "block-second",
            accepts_names: &["DUPLICATE"],
            accepts_star: false,
            accepts_score: false,
            accepts_newlines: false,
            parse_fn: BLOCK_BOLD.parse_fn,
        },
    ];

    build_block_rule_map(&BLOCK_RULES);
}

#[test]
fn block_rules_match_conf() {
    crate::test::conf_names::assert_rules_match_conf(
        "blocks.toml",
        "block-",
        false,
        &[
            "align-left",
            "align-center",
            "align-right",
            "align-justify",
            "social",
            "later",
        ],
        BLOCK_RULES
            .iter()
            .map(|rule| (rule.name, rule.accepts_names)),
    );
}
