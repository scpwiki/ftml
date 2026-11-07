/*
 * parsing/rule/impls/dash.rs
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
use crate::tree::Container;

pub const RULE_DASH: Rule = Rule {
    name: "dash",
    position: LineRequirement::Any,
    try_consume_fn,
};

pub const RULE_DASH_RUN: Rule = Rule {
    name: "dash-run",
    position: LineRequirement::Any,
    try_consume_fn: consume_run,
};

pub(super) fn double_dash_occupies_its_line(parser: &Parser) -> bool {
    if parser.current().token != Token::DoubleDash
        || !parser.current_at_source_line_start()
    {
        return false;
    }
    for token in parser.remaining() {
        match token.token {
            Token::Whitespace => continue,
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => return true,
            _ => return false,
        }
    }
    true
}

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Consuming token to create an em dash");

    if parser.settings().layout.legacy() {
        if double_dash_occupies_its_line(parser) {
            parser.step()?;
            while parser.current().token == Token::Whitespace {
                parser.step()?;
            }
        }
        return success_elements(text!("--"));
    }

    success_elements(text!("\u{2014}"))
}

fn consume_run<'r, 't>(parser: &mut Parser<'r, 't>) -> ParseResult<'r, 't, Elements<'t>> {
    let current = parser.current();
    assert_eq!(current.token, Token::TripleDash);
    debug!(
        "Consuming Wikidot dash run of length {}",
        current.slice.len()
    );

    if parser.settings().layout.legacy() {
        return success_elements(text!(current.slice));
    }

    let mut elements = Vec::with_capacity(current.slice.len() / 2);
    let strike_count = current.slice.len() / 5;
    let remainder = current.slice.len() % 5;

    for _ in 0..strike_count {
        elements.push(Element::Container(Container::new(
            ContainerType::Strikethrough,
            vec![text!("-")],
            AttributeMap::new(),
        )));
    }
    for _ in 0..remainder / 2 {
        elements.push(text!("\u{2014}"));
    }
    if remainder % 2 == 1 {
        elements.push(text!("-"));
    }

    success_elements(elements)
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    fn render_html(input: &str) -> String {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{input:?}: {errors:?}");
        HtmlRender.render(&tree, &page_info, &settings).body
    }

    #[test]
    fn wikidot_long_dash_runs_use_five_hyphen_strikethrough_chunks() {
        for (count, expected) in [
            (4, "<p>raw ——] tail</p>"),
            (
                5,
                "<p>raw <span style=\"text-decoration: line-through;\">-</span>] tail</p>",
            ),
            (
                9,
                "<p>raw <span style=\"text-decoration: line-through;\">-</span>——] tail</p>",
            ),
            (
                10,
                "<p>raw <span style=\"text-decoration: line-through;\">-</span><span style=\"text-decoration: line-through;\">-</span>] tail</p>",
            ),
            (
                15,
                "<p>raw <span style=\"text-decoration: line-through;\">-</span><span style=\"text-decoration: line-through;\">-</span><span style=\"text-decoration: line-through;\">-</span>] tail</p>",
            ),
        ] {
            let source = format!("raw {}] tail", "-".repeat(count));
            assert_eq!(render_html(&source), expected, "run length {count}");
        }
    }
}
