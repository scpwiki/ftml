/*
 * parsing/rule/impls/block/blocks/tabs.rs
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
use crate::parsing::ParserWrap;
use crate::tree::{AcceptsPartial, PartialElement, Tab};

pub const BLOCK_TABVIEW: BlockRule = BlockRule {
    name: "block-tabview",
    accepts_names: &["tabview", "tabs"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn: parse_tabview,
};

pub const BLOCK_TAB: BlockRule = BlockRule {
    name: "block-tab",
    accepts_names: &["tab"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn: parse_tab,
};

fn parse_tabview<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    let parser = &mut ParserWrap::new(parser, AcceptsPartial::Tab);

    debug!("Parsing tabview block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Tabview doesn't allow star flag");
    assert!(!flag_score, "Tabview doesn't allow score flag");
    assert_block_name(&BLOCK_TABVIEW, name);

    let tabview_name =
        parser.get_head_value_resolved(&BLOCK_TABVIEW, in_head, |_, value| Ok(value))?;

    let (elements, errors, _) = parser.get_body_elements(&BLOCK_TABVIEW, false)?.into();

    let mut tabs = Vec::new();

    for element in elements {
        match element {
            Element::Partial(PartialElement::Tab(tab)) => tabs.push(tab),

            element if element.is_whitespace() => (),

            _ => return Err(parser.make_err(ParseErrorKind::TabViewContainsNonTab)),
        }
    }

    if tabs.is_empty() {
        return Err(parser.make_err(ParseErrorKind::TabViewEmpty));
    }

    let tabview = Element::TabView(tabs);
    if parser.settings().layout.legacy()
        && let Some(name) = tabview_name
    {
        return ok!(
            false;
            vec![
                Element::Text(std::borrow::Cow::Owned(format!("{name} {name}\n"))),
                tabview,
            ],
            errors
        );
    }
    success_elements_with_paragraph_safety(false, tabview, errors)
}

fn parse_tab<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing tab block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Tab doesn't allow star flag");
    assert!(!flag_score, "Tab doesn't allow score flag");
    assert_block_name(&BLOCK_TAB, name);

    let label =
        parser.get_head_value_resolved(&BLOCK_TAB, in_head, |parser, value| {
            if parser.settings().layout.legacy() {
                Ok(value.unwrap_or(std::borrow::Cow::Borrowed("untitled")))
            } else {
                match value {
                    Some(value) => Ok(value),
                    None => Err(parser.make_err(ParseErrorKind::BlockMissingArguments)),
                }
            }
        })?;

    let (elements, errors, _) = parser.get_body_elements(&BLOCK_TAB, true)?.into();

    let element = Element::Partial(PartialElement::Tab(Tab { label, elements }));

    success_elements_with_paragraph_safety(false, element, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    fn render_wikidot(source: &str) -> (String, Vec<ParseError>) {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        (HtmlRender.render(&tree, &page_info, &settings).body, errors)
    }

    #[test]
    fn wikidot_named_tabview_and_untitled_tab_match_live_behavior() {
        let (html, errors) = render_wikidot(
            "NAME:\n[[tabview Foo]]\n[[tab Bar]].[[/tab]]\n[[/tabview]]\nNO LABEL:\n[[tabview]]\n[[tab]].[[/tab]]\n[[/tabview]]",
        );

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(
            html.starts_with("NAME:<br>\nFoo Foo\n<div id=\"wiki-tabview-"),
            "{html}",
        );
        assert!(html.contains("<em>Bar</em>"), "{html}");
        assert!(html.contains("NO LABEL:<br>\n"), "{html}");
        assert!(html.contains("<em>untitled</em>"), "{html}");
        assert!(!html.contains("<p>NAME:"), "{html}");
        assert!(!html.contains("<script"), "{html}");
    }

    #[test]
    fn underclosed_wikidot_tabviews_reuse_semantic_failure_cache() {
        use std::time::{Duration, Instant};

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = format!(
            "{}X{}",
            "[[tabview]]".repeat(128),
            "[[/tabview]]".repeat(64),
        );
        let tokenization = crate::tokenize(&source);
        let started = Instant::now();
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(
            started.elapsed() < Duration::from_secs(1),
            "underclosed Wikidot tabviews took {:?}",
            started.elapsed(),
        );
        assert_eq!(errors.len(), 512);
        assert!(html.contains("[[tabview]]"), "{html}");
    }

    #[test]
    fn tabviews_reuse_unclosed_collapsible_failures() {
        use std::time::{Duration, Instant};

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = format!(
            "{}{}{}",
            "[[tabview]]".repeat(64),
            "[[collapsible]]".repeat(64),
            "[[/tabview]]".repeat(128),
        );
        let tokenization = crate::tokenize(&source);
        let started = Instant::now();
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(
            started.elapsed() < Duration::from_secs(1),
            "tabview/collapsible recovery took {:?}",
            started.elapsed(),
        );
        assert!(!errors.is_empty());
        assert!(html.contains("[[tabview]]"), "{html}");
    }
}
