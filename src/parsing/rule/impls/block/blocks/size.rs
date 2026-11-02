/*
 * parsing/rule/impls/block/blocks/size.rs
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
use crate::tree::AttributeMap;
use crate::tree::PartialElement;
use std::borrow::Cow;

pub const BLOCK_SIZE: BlockRule = BlockRule {
    name: "block-size",
    accepts_names: &["size"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines: false,
    parse_fn,
};

fn parse_size_argument<'r, 't>(
    parser: &Parser<'r, 't>,
    value: Option<&str>,
) -> Result<String, ParseError> {
    match value {
        Some(size)
            if parser.settings().layout.legacy()
                && wikidot_size_value(size).is_none() =>
        {
            Err(parser.make_err(ParseErrorKind::BlockMalformedArguments))
        }
        Some(size) if parser.settings().layout.legacy() => {
            Ok(format!("font-size:{};", wikidot_size_value(size).unwrap()))
        }
        Some(size) => Ok(format!("font-size: {};", safe_size_value(size))),
        None => Err(parser.make_err(ParseErrorKind::BlockMissingArguments)),
    }
}

fn wikidot_size_value(size: &str) -> Option<&str> {
    let size = size.trim_matches([' ', '\t', '\r', '\n']);
    if matches!(
        size,
        "xx-small"
            | "x-small"
            | "small"
            | "medium"
            | "large"
            | "x-large"
            | "xx-large"
            | "smaller"
            | "larger"
    ) {
        return Some(size);
    }
    for suffix in ["%", "px", "em"] {
        if let Some(number) = size.strip_suffix(suffix)
            && !number.is_empty()
            && number != "."
            && number
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.')
            && number.bytes().filter(|byte| *byte == b'.').count() <= 1
        {
            return Some(size);
        }
    }
    None
}

fn safe_size_value(size: &str) -> &str {
    let size = size.trim();
    if !size.is_empty() && is_safe_size_value(size) {
        size
    } else {
        "inherit"
    }
}

fn is_safe_size_value(size: &str) -> bool {
    size.chars().all(|ch| {
        !ch.is_control()
            && !matches!(ch, ';' | '{' | '}' | '<' | '>' | '"' | '\'' | '\\' | '&')
    })
}

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing size block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Size doesn't allow star flag");
    assert!(!flag_score, "Size doesn't allow score flag");
    assert_block_name(&BLOCK_SIZE, name);

    let size = parser.get_head_value(&BLOCK_SIZE, in_head, parse_size_argument)?;

    if parser.settings().layout.legacy() {
        if !parser.has_body_end_block(&BLOCK_SIZE) {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
        return ok!(Element::Partial(PartialElement::InlineSizeOpen(
            Cow::Owned(size)
        )));
    }

    let body = parser.get_body_elements(&BLOCK_SIZE, false)?;
    let (elements, errors, paragraph_safe) = body.into();

    let mut attributes = AttributeMap::new();
    attributes.insert("style", Cow::Owned(size));

    let container = Container::new(ContainerType::Size, elements, attributes);
    let element = Element::Container(container);

    success_elements_with_paragraph_safety(paragraph_safe, element, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};
    use std::time::{Duration, Instant};

    fn render_wikidot(source: &str) -> String {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        HtmlRender.render(&tree, &page_info, &settings).body
    }

    #[test]
    fn wikidot_size_space_before_final_close_is_dropped() {
        assert_eq!(
            render_wikidot("[[size 120%]]x [[/size]]"),
            "<p><span style=\"font-size:120%;\">x</span></p>"
        );
    }

    #[test]
    fn size_block_requires_size_argument() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[size]]text[[/size]]");
        let (_tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(
            errors
                .iter()
                .any(|error| error.kind() == ParseErrorKind::BlockMissingArguments)
        );
    }

    #[test]
    fn size_block_wraps_body_with_style_attribute() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[size 80%]]small[[/size]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        let [Element::Container(paragraph)] = tree.elements.as_slice() else {
            panic!("expected paragraph, got {:?}", tree.elements);
        };
        let [Element::Container(size)] = paragraph.elements() else {
            panic!("expected size container, got {:?}", paragraph.elements());
        };

        assert_eq!(size.ctype(), ContainerType::Size);
        assert_eq!(
            size.attributes()
                .get()
                .get("style")
                .map(|value| value.as_ref()),
            Some("font-size:80%;"),
        );
        assert_eq!(size.elements(), &[text!("small")]);
    }

    #[test]
    fn wikidot_size_scope_crosses_structural_blocks_without_wrapping_them() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = concat!(
            "[[size larger]]\n",
            "[[div class=\"outer\"]]\n",
            "header\n",
            "[[div class=\"links\"]]\n",
            "[https://example.com/a A][https://example.com/b B]\n",
            "[[/div]]\n",
            "[[div class=\"content\"]]\n",
            "[[/size]]\n",
            "body\n",
            "[[/div]]\n",
            "[[/div]]\n",
        );
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("<div class=\"outer\">"), "{html}");
        assert!(
            html.contains("<span style=\"font-size:larger;\">header</span>"),
            "{html}"
        );
        assert!(
            html.contains("<div class=\"links\"><p><span style=\"font-size:larger;\"><a"),
            "{html}"
        );
        assert!(
            html.contains("<div class=\"content\"><p><br>\nbody</p>"),
            "{html}"
        );
        assert!(!html.contains("font-size:larger;\">body"), "{html}");
        assert!(
            !html.contains("<span style=\"font-size: larger;\"><div"),
            "{html}"
        );
    }

    #[test]
    fn unmatched_wikidot_size_scope_remains_literal() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[size larger]]text");
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert_eq!(html, "<p>[[size larger]]text</p>");
    }

    #[test]
    fn rejected_wikidot_size_preserves_literal_closer_spelling() {
        for (source, expected) in [
            (
                "[[SIZE]]v7 body[[/SIZE]]",
                "<p>[[SIZE]]v7 body[[/SIZE]]</p>",
            ),
            (
                "[[SiZe]]mixed case[[/sIzE]]",
                "<p>[[SiZe]]mixed case[[/sIzE]]</p>",
            ),
            (
                "[[ SIZE ]]spaced opener[[/ SIZE ]]",
                "<p>[[ SIZE ]]spaced opener[<a href=\"/\">SIZE</a>]</p>",
            ),
            (
                "[[SIZE_]]scored variant[[/SIZE_]]",
                "<p>[[SIZE_]]scored variant[[/SIZE_]]</p>",
            ),
            (
                r"[[SIZE bad\ value]]escaped head[[/SiZe]]",
                r"<p>[[SIZE bad\ value]]escaped head[[/SiZe]]</p>",
            ),
        ] {
            assert_eq!(render_wikidot(source), expected, "{source:?}");
        }
    }

    #[test]
    fn rejected_size_candidates_do_not_rewrite_later_valid_closers() {
        let source = concat!(
            "[[SIZE]]outer ",
            "[[size 80%]]valid[[/size]]",
            " tail[[/SIZE]] ",
            "[[size 90%]]later[[/size]]",
        );
        assert_eq!(
            render_wikidot(source),
            concat!(
                "<p>[[SIZE]]outer ",
                "<span style=\"font-size:80%;\">valid</span>",
                " tail[[/SIZE]] ",
                "<span style=\"font-size:90%;\">later</span>",
                "</p>",
            ),
        );
    }

    #[test]
    fn rejected_size_closer_preservation_stays_bounded() {
        const ROW_COUNT: usize = 2_048;
        let source = "[[SIZE]]body[[/SiZe]]\n".repeat(ROW_COUNT);
        let started = Instant::now();
        let html = render_wikidot(&source);
        let elapsed = started.elapsed();

        assert!(elapsed < Duration::from_secs(3), "took {elapsed:?}");
        assert_eq!(html.matches("[[SIZE]]body[[/SiZe]]").count(), ROW_COUNT);
    }
}
