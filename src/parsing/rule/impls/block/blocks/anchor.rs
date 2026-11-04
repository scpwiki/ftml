/*
 * parsing/rule/impls/block/blocks/anchor.rs
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
use crate::parsing::strip_newlines;
use crate::tree::AnchorTarget;

pub const BLOCK_ANCHOR: BlockRule = BlockRule {
    name: "block-anchor",
    accepts_names: &["a", "anchor"],
    accepts_star: true,
    accepts_score: true,
    accepts_newlines_outside_body: false,
    parse_fn,
};

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing anchor block (name '{name}', in-head {in_head}, star {flag_star})");
    assert_block_name(&BLOCK_ANCHOR, name);
    if parser.settings().layout.legacy() && flag_star {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let arguments = parser.get_head_map_wikidot(&BLOCK_ANCHOR, in_head)?;
    if parser.settings().layout.legacy() && !parser.has_body_end_block(&BLOCK_ANCHOR) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let attributes = if parser.settings().layout.legacy() {
        arguments.to_wikidot_anchor_attribute_map(parser.settings())
    } else {
        arguments.to_attribute_map(parser.settings())
    };

    let target = if flag_star {
        Some(AnchorTarget::NewTab)
    } else {
        None
    };

    // Wikidot moves ASCII whitespace at either edge of an inline anchor body
    // outside the link as one separator. A separator is dropped at the start
    // or end of a line and merges with adjacent authored whitespace, including
    // a separator already moved out of a preceding inline closer.
    let legacy = parser.settings().layout.legacy();
    let source = parser.full_text().inner();
    let body_source_start = parser.current().span.start;
    let anchor_body_start = source
        .get(..body_source_start)
        .and_then(|source| source.rfind("]]"))
        .map(|close| close + 2);
    let body = parser.get_body_elements(&BLOCK_ANCHOR, false)?;
    let fused_close = parser.wikidot_body_closed_with_fused_close(&BLOCK_ANCHOR);
    let (mut elements, errors, paragraph_safe) = body.into();
    let body_source = anchor_body_start.and_then(|start| {
        let close_end = parser.current().span.start;
        let close_start = source.get(..close_end)?.rfind("[[/")?;
        source.get(start..close_start)
    });
    let body_has_content =
        body_source.is_some_and(|body| !body.trim_matches(is_ascii_separator).is_empty());
    let leading_separator = legacy
        && body_source.is_some_and(|body| body.starts_with(is_ascii_separator))
        && !follows_separator_or_line_start(&source[..opener_start]);
    let trailing_separator = legacy
        && body_has_content
        && body_source.is_some_and(|body| body.ends_with(is_ascii_separator))
        && !matches!(
            parser.current().token,
            Token::Whitespace
                | Token::LineBreak
                | Token::ParagraphBreak
                | Token::InputEnd
        );
    if legacy && body_source.is_some_and(|body| body.starts_with(is_ascii_separator)) {
        trim_leading_separator(&mut elements);
    }
    if legacy && body_source.is_some_and(|body| body.ends_with(is_ascii_separator)) {
        trim_trailing_separator(&mut elements);
    }
    if flag_score {
        strip_newlines(&mut elements);
        if parser.settings().layout.legacy()
            && matches!(
                parser.current().token,
                Token::LineBreak | Token::ParagraphBreak
            )
        {
            parser.step()?;
        }
    }

    let element = Element::Anchor {
        elements,
        attributes,
        target,
    };
    let output = match (leading_separator, trailing_separator) {
        (false, false) => Elements::Single(element),
        (true, false) => Elements::Multiple(vec![text!(" "), element]),
        (false, true) => Elements::Multiple(vec![element, text!(" ")]),
        (true, true) => Elements::Multiple(vec![text!(" "), element, text!(" ")]),
    };

    let output = if fused_close {
        match output {
            Elements::Single(element) => Elements::Multiple(vec![element, text!("]")]),
            Elements::Multiple(mut elements) => {
                elements.push(text!("]"));
                Elements::Multiple(elements)
            }
            Elements::None => Elements::Multiple(vec![text!("]")]),
        }
    } else {
        output
    };

    success_elements_with_paragraph_safety(
        paragraph_safe || (flag_score && parser.settings().layout.legacy()),
        output,
        errors,
    )
}

fn is_ascii_separator(ch: char) -> bool {
    matches!(ch, ' ' | '\t')
}

fn follows_separator_or_line_start(before: &str) -> bool {
    match before.chars().next_back() {
        None | Some('\n') => true,
        Some(ch) if is_ascii_separator(ch) => true,
        Some(']') => before
            .strip_suffix("]]")
            .and_then(|head| head.rfind("[[/").map(|start| &head[..start]))
            .and_then(|body| body.chars().next_back())
            .is_some_and(is_ascii_separator),
        Some(_) => false,
    }
}

fn trim_leading_separator(elements: &mut Vec<Element<'_>>) {
    while let Some(Element::Text(text)) = elements.first_mut() {
        let trimmed = text.trim_start_matches(is_ascii_separator).len();
        if trimmed == text.len() {
            return;
        }
        if trimmed == 0 {
            elements.remove(0);
        } else {
            let start = text.len() - trimmed;
            text.to_mut().drain(..start);
            return;
        }
    }
}

fn trim_trailing_separator(elements: &mut Vec<Element<'_>>) {
    while let Some(Element::Text(text)) = elements.last_mut() {
        let trimmed = text.trim_end_matches(is_ascii_separator).len();
        if trimmed == text.len() {
            return;
        }
        if trimmed == 0 {
            elements.pop();
        } else {
            text.to_mut().truncate(trimmed);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};

    fn render_with_layout(source: &str, layout: Layout) -> String {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
        let tokenization = crate::tokenize(source);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        HtmlRender.render(&tree, &page_info, &settings).body
    }

    fn render(source: &str) -> String {
        render_with_layout(source, Layout::Wikidot)
    }

    #[test]
    fn wikidot_anchor_preserves_relative_href() {
        assert_eq!(
            render(r#"[[a href="some-page"]]click[[/a]]"#),
            r#"<p><a href="some-page">click</a></p>"#,
        );
    }

    #[test]
    fn wikidot_anchor_moves_authored_boundary_whitespace_outside() {
        let cases = [
            (
                "leading space inside second anchor",
                "[[a]]one[[/a]][[a]] two[[/a]]",
                "<p><a href>one</a> <a href>two</a></p>",
            ),
            (
                "leading space after text",
                "x[[a]] one[[/a]]y",
                "<p>x <a href>one</a>y</p>",
            ),
            (
                "leading space merges with authored space",
                "x [[a]]  two[[/a]]",
                "<p>x <a href>two</a></p>",
            ),
            (
                "leading space at line start is dropped",
                "[[a]] one [[/a]]",
                "<p><a href>one</a></p>",
            ),
            (
                "trailing space before text",
                "x [[a]]one [[/a]]y",
                "<p>x <a href>one</a> y</p>",
            ),
            (
                "repeated trailing whitespace collapses once",
                "x [[a]]one \t[[/a]]y",
                "<p>x <a href>one</a> y</p>",
            ),
            (
                "trailing space merges with authored space",
                "x [[a]]one [[/a]] y",
                "<p>x <a href>one</a> y</p>",
            ),
            (
                "trailing space at line end is dropped",
                "x [[a]]one [[/a]]\ny",
                "<p>x <a href>one</a><br>\ny</p>",
            ),
            (
                "adjacent anchors share one separator",
                "[[a]]x[[/a]][[a]] one [[/a]][[a]] two [[/a]]",
                "<p><a href>x</a> <a href>one</a> <a href>two</a></p>",
            ),
            (
                "space between anchors",
                "[[a]]one[[/a]] [[a]]two[[/a]]",
                "<p><a href>one</a> <a href>two</a></p>",
            ),
            (
                "truly adjacent anchors",
                "[[a]]one[[/a]][[a]]two[[/a]]",
                "<p><a href>one</a><a href>two</a></p>",
            ),
            (
                "formatting before trailing space",
                "x [[a]]**one** [[/a]]y",
                "<p>x <a href><strong>one</strong></a> y</p>",
            ),
            (
                "nested formatting after leading space",
                "x[[a]] **two**[[/a]]",
                "<p>x <a href><strong>two</strong></a></p>",
            ),
            (
                "CJK label after leading space",
                "x[[a]] 日本語[[/a]]",
                "<p>x <a href>日本語</a></p>",
            ),
            (
                "whitespace-only body",
                "x [[a]] [[/a]]y",
                "<p>x <a href></a>y</p>",
            ),
            (
                "nonbreaking spaces stay inside",
                "x [[a]]\u{a0}two\u{a0}[[/a]]y",
                "<p>x <a href>\u{a0}two\u{a0}</a>y</p>",
            ),
            ("tab", "x[[a]]\ttwo\t[[/a]]y", "<p>x <a href>two</a> y</p>"),
        ];

        for (case, source, expected) in cases {
            assert_eq!(render(source), expected, "{case}");
        }
    }

    #[test]
    fn wikidot_anchor_discards_title_attribute() {
        let source = r#"[[a href="https://example.com/" title="Label"]]Label[[/a]]"#;
        assert_eq!(
            render(source),
            r#"<p><a href="https://example.com/">Label</a></p>"#,
        );
        assert_eq!(
            render_with_layout(source, Layout::Wikijump),
            r#"<p><a class="wj-anchor" href="https://example.com/" title="Label">Label</a></p>"#,
        );
    }

    #[test]
    fn wikijump_anchor_alias_remains_active() {
        assert_eq!(
            render_with_layout(
                r#"[[anchor href="some-page"]]click[[/anchor]]"#,
                Layout::Wikijump,
            ),
            r#"<p><a class="wj-anchor" href="/some-page">click</a></p>"#,
        );
        assert_eq!(
            render_with_layout(
                r#"[[a href="some-page"]]click[[/anchor]]"#,
                Layout::Wikijump,
            ),
            r#"<p><a class="wj-anchor" href="/some-page">click</a></p>"#,
        );
        assert_eq!(
            render_with_layout(
                r#"[[anchor href="some-page"]]click[[/a]]"#,
                Layout::Wikijump,
            ),
            r#"<p><a class="wj-anchor" href="/some-page">click</a></p>"#,
        );
    }

    #[test]
    fn wikidot_canonical_anchor_rejects_alias_closer() {
        assert_eq!(
            render(r#"[[a href="https://example.com"]]Canonical opener[[/anchor]]"#),
            concat!(
                r#"<p>[[a href=&quot;<a href="https://example.com">https://example.com</a>"#,
                r#"&quot;]]Canonical opener[[/anchor]]</p>"#,
            ),
        );
    }

    #[test]
    fn wikidot_star_anchor_stays_literal() {
        let html = render(r#"[[*a href="some-page"]]click[[/a]]"#);
        assert!(
            html.contains("[[*a href=&quot;some-page&quot;]]click[[/a]]"),
            "{html}"
        );
    }

    #[test]
    fn wikidot_anchor_preserves_live_dangerous_href_normalization() {
        for (href, expected) in [
            ("javascript:alert(1)", "javascript:alert-1"),
            ("javascript: alert(1)", "javascript:alert-1"),
            ("javascript : alert(1)", "javascript:alert-1"),
            ("java script:alert(1)", "java-script:alert-1"),
        ] {
            assert_eq!(
                render(&format!(r#"[[a href="{href}"]]click[[/a]]"#)),
                format!(r#"<p><a href="{expected}">click</a></p>"#),
            );
        }
    }

    #[test]
    fn wikidot_score_anchor_consumes_following_newlines() {
        let html = render(concat!(
            "EMPTY [[a_]][[/a]]\n",
            "BASIC [[a_ href=\"some-page\"]]\nclick\n[[/a]]\n",
            "ATTRS [[a_ href=\"https://example.com\"]]my link[[/a]]\n\n",
            "**BOTH**",
        ));
        assert_eq!(
            html,
            concat!(
                "<p>EMPTY <a href></a>",
                "BASIC <a href=\"some-page\">click</a>",
                "ATTRS <a href=\"https://example.com\">my link</a>",
                "<strong>BOTH</strong></p>",
            ),
        );
    }
}
