/*
 * parsing/rule/bibliography_order.rs
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

//! Live Wikidot's ordered Text_Wiki rule list, as it applies to a bibliography body.
//!
//! Live Wikidot does not build a tree. It runs one ordered rewrite rule list over
//! the whole source and only then hands the flat markup to libtidy. `Bibliography`
//! is position 23: it claims the first range, turns `: label : text` lines into
//! bibitem tokens, and replaces every newline in the body with a space. The whole
//! bibliography is one line from then on.
//!
//! A construct written inside that body is therefore visible in the joined body
//! exactly when its rule ran before position 23, or when it runs after 23 with a
//! pattern that needs neither a line start nor a newline. The joined body has no
//! line starts and no newlines, so line-anchored rules never form.
//!
//! The tables below are the classification the model requires. The parser uses
//! them instead of hand-maintained ban lists; new lines through this module keep
//! the parser's bibliography behavior derived from the rule order alone.

/// Where a Text_Wiki rule sits relative to `Bibliography` (position 23), and
/// what that means for text it would claim *inside* a bibliography body.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum BibliographyBodyFormation {
    /// Ran before the body was joined, so its token is already in the body.
    BeforeJoin,
    /// Runs after the join, and its pattern needs no line start and no newline.
    AfterJoinUnanchored,
    /// Runs after the join but needs a line start or a newline. The joined body
    /// has neither, so the construct never forms there.
    AfterJoinAnchored,
}

impl BibliographyBodyFormation {
    #[inline]
    pub(crate) fn forms_in_body(self) -> bool {
        matches!(
            self,
            BibliographyBodyFormation::BeforeJoin
                | BibliographyBodyFormation::AfterJoinUnanchored
        )
    }
}

use self::BibliographyBodyFormation::{
    AfterJoinAnchored, AfterJoinUnanchored, BeforeJoin,
};

/// The ordered Text_Wiki rule list live Wikidot runs (fossil `Text/Wiki.php`
/// `$rules`; `Ullist` is live's addition after `List`).
///
/// `AfterJoinAnchored` is used for rules whose pattern needs a line start or a
/// newline; `AfterJoinUnanchored` only when neither is required anywhere in the
/// matched text. The last three entries are the text passes, not constructs.
const WIKIDOT_RULES: &[(&str, BibliographyBodyFormation)] = &[
    ("include", BeforeJoin),
    ("prefilter", BeforeJoin),
    ("delimiter", BeforeJoin),
    ("code", BeforeJoin),
    ("form", BeforeJoin),
    ("raw", BeforeJoin),
    ("rawold", BeforeJoin),
    ("modulepre", BeforeJoin),
    ("module", BeforeJoin),
    ("module654", BeforeJoin),
    ("iftags", BeforeJoin),
    ("comment", BeforeJoin),
    ("iframe", BeforeJoin),
    ("date", BeforeJoin),
    ("math", BeforeJoin),
    ("concatlines", BeforeJoin),
    ("freelink", BeforeJoin),
    ("equationreference", BeforeJoin),
    ("footnote", BeforeJoin),
    ("footnoteitem", BeforeJoin),
    ("footnoteblock", BeforeJoin),
    ("bibitem", BeforeJoin),
    ("bibliography", BeforeJoin),
    ("bibcite", AfterJoinUnanchored),
    ("divprefilter", AfterJoinUnanchored),
    ("anchor", AfterJoinUnanchored),
    ("user", AfterJoinUnanchored),
    ("blockquote", AfterJoinAnchored),
    ("heading", AfterJoinAnchored),
    ("toc", AfterJoinAnchored),
    ("horiz", AfterJoinAnchored),
    ("separator", AfterJoinAnchored),
    ("clearfloat", AfterJoinAnchored),
    ("break", AfterJoinUnanchored),
    ("ahref", AfterJoinUnanchored),
    ("span", AfterJoinUnanchored),
    ("size", AfterJoinUnanchored),
    ("div", AfterJoinAnchored),
    ("divalign", AfterJoinAnchored),
    ("collapsible", AfterJoinUnanchored),
    ("tabview", AfterJoinAnchored),
    ("note", AfterJoinAnchored),
    ("gallery", AfterJoinAnchored),
    ("list", AfterJoinAnchored),
    ("ullist", AfterJoinUnanchored),
    ("deflist", AfterJoinAnchored),
    ("table", AfterJoinAnchored),
    ("tableadv", AfterJoinUnanchored),
    ("button", AfterJoinUnanchored),
    ("image", AfterJoinUnanchored),
    ("embed", AfterJoinUnanchored),
    ("social", AfterJoinUnanchored),
    ("file", AfterJoinUnanchored),
    ("center", AfterJoinAnchored),
    ("newline", BeforeJoin),
    ("paragraph", BeforeJoin),
    ("url", AfterJoinUnanchored),
    ("email", AfterJoinUnanchored),
    ("mathinline", AfterJoinUnanchored),
    ("interwiki", AfterJoinUnanchored),
    ("colortext", AfterJoinUnanchored),
    ("strong", AfterJoinUnanchored),
    ("emphasis", AfterJoinUnanchored),
    ("underline", AfterJoinUnanchored),
    ("strikethrough", AfterJoinUnanchored),
    ("tt", AfterJoinUnanchored),
    ("superscript", AfterJoinUnanchored),
    ("subscript", AfterJoinUnanchored),
    ("typography", AfterJoinUnanchored),
    ("tighten", BeforeJoin),
];

#[inline]
fn formation_for_live_rule(live_name: &str) -> Option<BibliographyBodyFormation> {
    WIKIDOT_RULES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(live_name))
        .map(|&(_, formation)| formation)
}

/// Whether the live rule with that name can form inside a joined bibliography body.
///
/// Unknown names are not classified: callers keep their existing behavior.
pub(crate) fn live_rule_forms_in_bibliography_body(live_name: &str) -> Option<bool> {
    formation_for_live_rule(live_name).map(BibliographyBodyFormation::forms_in_body)
}

/// Whether a live rule with that name can never form inside a joined body.
#[inline]
pub(crate) fn live_rule_cannot_form_in_bibliography_body(live_name: &str) -> bool {
    live_rule_forms_in_bibliography_body(live_name) == Some(false)
}

/// Live rule name for a block parser rule, when the block is part of the
/// Text_Wiki order. `None` means the construct has no live rule (Wikijump-native
/// or save-time syntax), so the caller keeps its existing behavior.
fn live_name_for_block_rule(block_name: &str) -> Option<&'static str> {
    Some(match block_name {
        "block-code" => "code",
        "block-math" => "math",
        "block-module" => "module",
        "block-iftags" => "iftags",
        "block-comment" => "comment",
        "block-iframe" => "iframe",
        "block-date" => "date",
        "block-footnote" => "footnote",
        "block-footnote-block" => "footnoteblock",
        "block-bibcite" => "bibcite",
        "block-anchor" => "anchor",
        "block-user" => "user",
        "block-blockquote" => "blockquote",
        "block-toc" => "toc",
        "block-div" => "div",
        "block-align-center"
        | "block-align-right"
        | "block-align-left"
        | "block-align-justify" => "divalign",
        "block-collapsible" => "collapsible",
        "block-tabview" => "tabview",
        "block-note" => "note",
        "block-gallery" => "gallery",
        "block-list-unordered" | "block-list-ordered" | "block-list-item" => "ullist",
        "block-table" => "tableadv",
        "block-button" => "button",
        "block-embed" => "embed",
        "block-embed-audio" | "block-embed-video" | "block-embedvideo" => "embed",
        "block-social" => "social",
        "block-file" => "file",
        "block-raw" => "raw",
        "block-html" => return None,
        _ => return None,
    })
}

/// Whether the block parser must refuse `block_name` while inside a Wikidot
/// bibliography body because live's rule for it is line-anchored (or needs a
/// newline) and the joined body has neither.
pub(crate) fn block_rule_cannot_form_in_bibliography_body(block_name: &str) -> bool {
    live_name_for_block_rule(block_name)
        .is_some_and(live_rule_cannot_form_in_bibliography_body)
}

/// Live rule name for a non-block parser rule, when that rule is part of the
/// Text_Wiki order.
fn live_name_for_line_rule(rule_name: &str) -> Option<&'static str> {
    Some(match rule_name {
        "center" => "center",
        "header" => "heading",
        "blockquote" => "blockquote",
        "table" => "table",
        "horizontal-rule" => "horiz",
        "clear-float" => "clearfloat",
        "list" => "list",
        _ => return None,
    })
}

/// Whether the parser must skip `rule_name` while inside a Wikidot bibliography
/// body because live's rule for it is line-anchored and the joined body has no
/// line starts.
pub(crate) fn line_rule_cannot_form_in_bibliography_body(rule_name: &str) -> bool {
    live_name_for_line_rule(rule_name)
        .is_some_and(live_rule_cannot_form_in_bibliography_body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bibliography_position_is_the_rule_list_position() {
        let position = WIKIDOT_RULES
            .iter()
            .position(|(name, _)| *name == "bibliography")
            .expect("bibliography is in the live rule list");
        assert_eq!(position, 22, "live position 23, zero-based");
        assert_eq!(
            WIKIDOT_RULES[position].1,
            BibliographyBodyFormation::BeforeJoin,
        );
    }

    #[test]
    fn rule_order_classifies_constructs_against_bibliography_23() {
        for name in [
            "code",
            "math",
            "module",
            "iftags",
            "comment",
            "iframe",
            "date",
            "footnote",
            "footnoteblock",
            "bibitem",
        ] {
            assert_eq!(
                live_rule_forms_in_bibliography_body(name),
                Some(true),
                "{name} ran before Bibliography (23)",
            );
        }

        for name in ["bibcite", "span", "size", "collapsible", "ahref", "ullist"] {
            assert_eq!(
                live_rule_forms_in_bibliography_body(name),
                Some(true),
                "{name} runs after 23 without needing a line start or newline",
            );
        }

        for name in [
            "list",
            "deflist",
            "table",
            "blockquote",
            "heading",
            "divalign",
            "tabview",
            "center",
            "horiz",
            "clearfloat",
            "div",
        ] {
            assert_eq!(
                live_rule_forms_in_bibliography_body(name),
                Some(false),
                "{name} is line-anchored and never forms in the joined body",
            );
        }

        assert_eq!(live_rule_forms_in_bibliography_body("unknown-rule"), None);
        assert!(block_rule_cannot_form_in_bibliography_body("block-div"));
        assert!(block_rule_cannot_form_in_bibliography_body("block-tabview"));
        assert!(!block_rule_cannot_form_in_bibliography_body("block-code"));
        assert!(!block_rule_cannot_form_in_bibliography_body("block-table"));
        assert!(!block_rule_cannot_form_in_bibliography_body("block-html"));
        assert!(!block_rule_cannot_form_in_bibliography_body(
            "block-paragraph"
        ));
        assert!(line_rule_cannot_form_in_bibliography_body("list"));
        assert!(line_rule_cannot_form_in_bibliography_body("header"));
        assert!(!line_rule_cannot_form_in_bibliography_body("bibcite"));
    }
}
