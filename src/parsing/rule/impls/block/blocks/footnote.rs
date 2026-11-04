/*
 * parsing/rule/impls/block/blocks/footnote.rs
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
use crate::parsing::parse_boolean;
use std::ops::{Deref, DerefMut};

pub const BLOCK_FOOTNOTE: BlockRule = BlockRule {
    name: "block-footnote",
    accepts_names: &["footnote"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: false,
    parse_fn: parse_footnote_ref,
};

pub const BLOCK_FOOTNOTE_BLOCK: BlockRule = BlockRule {
    name: "block-footnote-block",
    accepts_names: &["footnoteblock"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn: parse_footnote_block,
};

fn parse_footnote_ref<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing footnote ref block (in-head {in_head})");

    if parser.settings().layout.legacy() && name != "footnote" {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if parser.in_footnote() {
        return Err(parser.make_err(ParseErrorKind::FootnotesNested));
    }

    let parser = &mut ParserWrap::new(parser);

    assert!(!flag_star, "Footnote reference doesn't allow star flag");
    assert!(!flag_score, "Footnote reference doesn't allow score flag");
    assert_block_name(&BLOCK_FOOTNOTE, name);

    parser.get_head_none(&BLOCK_FOOTNOTE, in_head)?;

    let body = parser.get_body_elements(&BLOCK_FOOTNOTE, true)?;
    let fused_close = parser.wikidot_body_closed_with_fused_close(&BLOCK_FOOTNOTE);
    let (mut elements, errors, _) = body.into();

    if elements.len() == 1 {
        match elements.pop().unwrap() {
            Element::Container(container)
                if container.ctype() == ContainerType::Paragraph =>
            {
                let mut new_elements: Vec<Element> = container.into();
                elements.append(&mut new_elements);
            }

            element => elements.push(element),
        };
    } else if parser.settings().layout.legacy()
        && matches!(
            elements.first(),
            Some(Element::Container(container))
                if container.ctype() == ContainerType::Paragraph
        )
    {
        let Element::Container(container) = elements.remove(0) else {
            unreachable!("first element was checked above");
        };
        let mut first: Vec<Element> = container.into();
        first.append(&mut elements);
        elements = first;
    }

    if parser.settings().layout.legacy()
        && let Some(Element::Container(container)) = elements.last_mut()
        && container.ctype() == ContainerType::Paragraph
        && matches!(container.elements().last(), Some(Element::LineBreak))
    {
        container.elements_mut().pop();
    }
    if parser.settings().layout.legacy()
        && matches!(
            elements.as_slice(),
            [.., Element::Code(_), Element::LineBreak]
        )
    {
        elements.pop();
    }

    let index = parser.push_footnote(elements);

    if fused_close {
        ok!(true; vec![Element::Footnote(index), text!("]")], errors)
    } else {
        ok!(Element::Footnote(index), errors)
    }
}

fn parse_footnote_block<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing footnote list block (in-head {in_head})");
    assert!(!flag_star, "Footnote block doesn't allow star flag");
    assert!(!flag_score, "Footnote block doesn't allow score flag");
    assert_block_name(&BLOCK_FOOTNOTE_BLOCK, name);

    if parser.settings().layout.legacy() && name != "footnoteblock" {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if parser.in_footnote() {
        return Err(parser.make_err(ParseErrorKind::FootnotesNested));
    }

    if parser.settings().layout.legacy() && parser.has_footnote_block() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let mut arguments = parser.get_head_map_wikidot(&BLOCK_FOOTNOTE_BLOCK, in_head)?;

    let title = arguments.get("title");
    // Wikidot does not reject the block over an unrecognized `hide`
    // value; anything outside the boolean vocabulary acts as false.
    let hide = arguments
        .get("hide")
        .map(|value| parse_boolean(value.as_ref()).unwrap_or(false))
        .unwrap_or(false);

    if !arguments.is_empty() {
        warn!("Invalid argument keys found");
        if parser.settings().layout.legacy() {
            return ok!(Elements::None);
        }
        return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
    }

    let line_prefix = footnote_block_line_prefix(parser);

    parser.set_footnote_block();

    ok!(line_prefix.is_inline_prose(); Element::FootnoteBlock { title, hide })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FootnoteBlockLinePrefix {
    OwnLine,
    InlineProse,
}

impl FootnoteBlockLinePrefix {
    fn is_inline_prose(self) -> bool {
        self == FootnoteBlockLinePrefix::InlineProse
    }
}

fn footnote_block_line_prefix(parser: &Parser<'_, '_>) -> FootnoteBlockLinePrefix {
    if parser.settings().layout.legacy()
        && wikidot_footnote_block_has_inline_prefix(parser)
    {
        FootnoteBlockLinePrefix::InlineProse
    } else {
        FootnoteBlockLinePrefix::OwnLine
    }
}

fn wikidot_footnote_block_has_inline_prefix(parser: &Parser<'_, '_>) -> bool {
    let source = parser.full_text().inner();
    let head = parser.current().span.start;
    let opener = source[..head].rfind("[[").unwrap_or(head);
    let line_start = source[..opener].rfind('\n').map_or(0, |index| index + 1);

    source[line_start..opener]
        .chars()
        .any(|character| !matches!(character, ' ' | '\t' | '\0'))
}

#[derive(Debug)]
struct ParserWrap<'p, 'r, 't> {
    parser: &'p mut Parser<'r, 't>,
}

impl<'p, 'r, 't> ParserWrap<'p, 'r, 't> {
    #[inline]
    fn new(parser: &'p mut Parser<'r, 't>) -> Self {
        parser.set_footnote_flag(true);

        ParserWrap { parser }
    }
}

impl<'r, 't> Deref for ParserWrap<'_, 'r, 't> {
    type Target = Parser<'r, 't>;

    #[inline]
    fn deref(&self) -> &Parser<'r, 't> {
        self.parser
    }
}

impl<'r, 't> DerefMut for ParserWrap<'_, 'r, 't> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Parser<'r, 't> {
        self.parser
    }
}

impl Drop for ParserWrap<'_, '_, '_> {
    fn drop(&mut self) {
        self.parser.set_footnote_flag(false);
    }
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
    fn footnote_rejects_nested_footnotes() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(
            "[[footnote]]outer [[footnote]]inner[[/footnote]][[/footnote]]",
        );
        let (_tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(
            errors
                .iter()
                .any(|error| error.kind() == ParseErrorKind::FootnotesNested)
        );
    }

    #[test]
    fn footnote_rejects_nested_footnote_blocks() {
        for layout in [Layout::Wikidot, Layout::Wikijump] {
            let page_info = PageInfo::dummy();
            let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
            let tokenization =
                crate::tokenize("[[footnote]][[footnoteblock]][[/footnote]]");
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();

            assert!(
                errors
                    .iter()
                    .any(|error| error.kind() == ParseErrorKind::FootnotesNested),
                "{layout:?}: {errors:#?}",
            );

            let html = HtmlRender.render(&tree, &page_info, &settings).body;
            assert!(html.contains("footnoteblock"), "{layout:?}: {html}");
        }
    }

    #[test]
    fn footnote_keeps_single_non_paragraph_body_element() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[footnote]][[module Rate]][[/footnote]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty());
        assert_eq!(tree.footnotes.len(), 1);
        assert_eq!(tree.footnotes[0].len(), 1);
        assert!(matches!(tree.footnotes[0][0], Element::Module(_)));
    }

    #[test]
    fn wikidot_footnote_lowers_inline_span_scopes() {
        let (html, errors) = render_wikidot(
            "before[[footnote]]以[[span class=\"ruby\"]]精神-物质[[span class=\"rt\"]]灵与肉[[/span]][[/span]]后[[/footnote]]after[[footnoteblock]]",
        );

        assert!(errors.is_empty(), "{errors:?}");
        assert!(
            html.contains(
                ". 以<span class=\"ruby\">精神-物质<span class=\"rt\">灵与肉</span></span>后",
            ),
            "{html}",
        );
    }

    #[test]
    fn wikidot_footnote_block_ignores_unknown_arguments() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[footnoteblock bogus=\"true\"]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        assert!(tree.elements.is_empty());
    }

    #[test]
    fn parser_wrap_sets_and_resets_footnote_flag() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("text");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);

        {
            let mut wrapped = ParserWrap::new(&mut parser);
            assert!(wrapped.in_footnote());

            wrapped.set_footnote_block();
        }

        assert!(!parser.in_footnote());
        assert!(parser.has_footnote_block());
    }

    #[test]
    fn wikidot_second_footnote_block_remains_literal() {
        let (html, errors) = render_wikidot(
            "Alpha[[footnote]]A[[/footnote]]\n[[footnoteblock]]\n\nBeta[[footnote]]B[[/footnote]]\n[[footnoteblock]]",
        );

        assert!(!errors.is_empty());
        assert_eq!(html.matches("class=\"footnotes-footer\"").count(), 1);
        assert!(html.contains("Beta<sup class=\"footnoteref\""), "{html}");
        assert!(html.contains("<br>\n[[footnoteblock]]</p>"), "{html}");
    }

    #[test]
    fn wikidot_multiline_footnote_unwraps_its_first_paragraph() {
        let (html, errors) = render_wikidot(
            "Apple[[footnote]]\nfirst line\n\nsecond paragraph\nlast line\n[[/footnote]]",
        );

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(
            html.contains("\">1</a>. first line\n<p>second paragraph<br>\nlast line</p>"),
            "{html}",
        );
        assert!(!html.contains("\">1</a>. <p>first line</p>"), "{html}");
    }

    #[test]
    fn wikidot_footnote_closes_at_closer_on_quote_line() {
        // jp/user-component-policy's shape: a footnote body starts on one
        // line and its [[/footnote]] closer sits at the end of a following
        // `> ` quote line. Wikidot keeps the quote line as a blockquote
        // inside the footnote and closes the footnote there. Losing the
        // closer makes the body gather run to the end of the input, which
        // loses the footnote and degrades everything after it (on the
        // page, the two [[code]] blocks inside the following [[div]]).
        let source = "First[[footnote]] excerpt\n> quoted one [[/footnote]] and second[[footnote]] excerpt two\n> quoted two [[/footnote]] done.\n\n[[div]]\n[[code]]\ncode body\n[[/code]]\n[[/div]]\n\n[[footnoteblock]]";
        let (html, errors) = render_wikidot(source);

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("<blockquote>"), "{html}");
        assert!(html.contains("quoted one"), "{html}");
        assert!(html.contains("quoted two"), "{html}");
        assert!(html.contains("<div class=\"code\">"), "{html}");
        assert!(html.contains("code body"), "{html}");
        assert!(!html.contains("[[footnote]]"), "{html}");
        assert!(!html.contains("[[code]]"), "{html}");

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, _) = crate::parse(&tokenization, &page_info, &settings).into();
        assert_eq!(tree.footnotes.len(), 2);
    }

    #[test]
    fn wikidot_footnote_renders_math_block_in_body() {
        // Triage shape mx-footnote: the math opener follows body text on the
        // same line, and text follows [[/math]] on its line. Wikidot closes
        // the footnote's math at its first [[/math]] regardless.
        let (html, errors) = render_wikidot(
            "Text[[footnote]]note [[math]]\nx^2\n[[/math]] end[[/footnote]] done.\n\n[[footnoteblock]]",
        );

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("class=\"math-equation\""), "{html}");
        assert!(
            html.contains("\\begin{equation} x^2 \\end{equation}"),
            "{html}"
        );
        assert!(html.contains("<p>end</p>"), "{html}");
        assert!(!html.contains("[[math]]"), "{html}");
    }

    #[test]
    fn wikidot_footnote_renders_single_line_math_block() {
        let (html, errors) = render_wikidot(
            "Text[[footnote]]See [[math]] x^2 [[/math]] here[[/footnote]]\n\n[[footnoteblock]]",
        );

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("class=\"math-equation\""), "{html}");
        assert!(
            html.contains("\\begin{equation} x^2 \\end{equation}"),
            "{html}"
        );
        assert!(!html.contains("[[math]]"), "{html}");
    }

    #[test]
    fn wikidot_footnote_from_table_header_renders_math_block() {
        // scp-7079's shape: the footnote opens at the end of a table header
        // line, its body begins with a math block, and text trails the
        // [[/math]] closer on the same line. Live Wikidot renders the
        // equation inside the footnote, with the trailing text after it.
        let (html, errors) = render_wikidot(
            "||~ N ||~ x[[footnote]]\n[[math]]\nx^2\n[[/math]] tail\n[[/footnote]]||\n|| a ||\n\n[[footnoteblock]]",
        );

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("<th>x<sup class=\"footnoteref\""), "{html}");
        assert!(html.contains("class=\"math-equation\""), "{html}");
        assert!(
            html.contains("\\begin{equation} x^2 \\end{equation}"),
            "{html}"
        );
        assert!(html.contains("<p>tail</p>"), "{html}");
        assert!(!html.contains("[[math]]"), "{html}");
    }

    #[test]
    fn wikidot_footnote_block_ignores_hide_true() {
        let (html, errors) = render_wikidot(
            "Apple[[footnote]]Banana[[/footnote]]\n\n[[footnoteblock hide=\"true\"]]",
        );

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("class=\"footnotes-footer\""), "{html}");
        assert!(html.contains(">1</a>. Banana</div>"), "{html}");
    }
}
