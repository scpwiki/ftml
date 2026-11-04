/*
 * parsing/rule/impls/section_marker.rs
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

pub const RULE_SECTION_MARKER: Rule = Rule {
    name: "section-marker",
    position: LineRequirement::StartOfLine,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Trying to consume section marker");

    let wikidot = parser.settings().layout.legacy();
    if !wikidot && parser.current().token == Token::Whitespace {
        parser.step()?;
    }

    let mut count = 0;
    while parser.current().token == Token::Equals {
        count += 1;
        parser.step()?;
    }

    if count < 4 {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if !wikidot && parser.current().token == Token::Whitespace {
        parser.step()?;
    }

    match parser.current().token {
        Token::LineBreak => {
            parser.step()?;
        }
        Token::ParagraphBreak | Token::InputEnd => {}
        _ => return Err(parser.make_err(ParseErrorKind::RuleFailed)),
    }

    ok!(Element::ContentSeparator)
}

#[test]
fn wikidot_section_markers_render_hidden_separators_and_split_paragraphs() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize("before\n====\nmiddle\n=====\nafter");
    let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
    assert!(errors.is_empty());

    let html = HtmlRender.render(&tree, &page_info, &settings).body;
    assert_eq!(
        html,
        r#"<p>before</p><div class="content-separator" style="display: none:"></div><p>middle</p><div class="content-separator" style="display: none:"></div><p>after</p>"#,
    );
}

#[test]
fn wikidot_section_marker_preserves_unwrapped_line_ending() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization =
        crate::tokenize("[[div class=\"box\"]]\ninside\n[[/div]]\nafter\n=====\nend");
    let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
    assert!(errors.is_empty());

    let html = HtmlRender.render(&tree, &page_info, &settings).body;
    assert!(
        html.contains(
            "after\n<div class=\"content-separator\" style=\"display: none:\"></div>",
        ),
        "{html}",
    );
}
