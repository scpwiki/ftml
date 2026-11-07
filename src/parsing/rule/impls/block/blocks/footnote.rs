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

use super::append_wikidot_footnote_trailing_list_break;
use super::prelude::*;
use crate::parsing::parse_boolean;
use crate::tree::AttributeMap;
use crate::parsing::rule::impls::comment_closer_token_count;
use std::borrow::Cow;
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
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing footnote ref block (in-head {in_head})");

    if parser.settings().layout.legacy() && name != "footnote" {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if parser.in_footnote() {
        return Err(parser.make_err(if parser.settings().layout.legacy() {
            ParseErrorKind::RuleFailed
        } else {
            ParseErrorKind::FootnotesNested
        }));
    }

    let parser = &mut ParserWrap::new(parser);

    assert!(!flag_star, "Footnote reference doesn't allow star flag");
    assert!(!flag_score, "Footnote reference doesn't allow score flag");
    assert_block_name(&BLOCK_FOOTNOTE, name);

    parser.get_head_none(&BLOCK_FOOTNOTE, in_head)?;

    if parser.settings().layout.legacy() {
        parser.arm_footnote_zone();
    }

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
    if parser.settings().layout.legacy()
        && matches!(elements.last(), Some(Element::LineBreak))
    {
        elements.pop();
    }
    if parser.settings().layout.legacy() {
        let repaired =
            crate::parsing::footnote::repair_wikidot_footnote_list_chrome(&mut elements);
        let note_took_the_list_break = parser.take_suppress_footnote_list_break();
        let closer_marker = wikidot_footnote_closer_line_list_marker(&parser);
        if let Some(marker) = closer_marker
            && matches!(
                elements.last(),
                Some(Element::List {
                    wikidot_explicit_block: true,
                    ..
                })
            )
        {
            elements.push(Element::Container(Container::new(
                ContainerType::Paragraph,
                vec![text!(marker)],
                AttributeMap::new(),
            )));
        } else if !repaired && !note_took_the_list_break {
            append_wikidot_footnote_trailing_list_break(&mut elements);
        }
    }

    let index = parser.push_footnote(elements);

    if fused_close {
        ok!(true; vec![Element::Footnote(index.into()), text!("]")], errors)
    } else {
        ok!(Element::Footnote(index.into()), errors)
    }
}

fn wikidot_footnote_closer_line_list_marker(
    parser: &Parser<'_, '_>,
) -> Option<&'static str> {
    let source = parser.full_text().inner();
    let mut end = parser.current().span.start.min(source.len());
    while end > 0 && matches!(source.as_bytes()[end - 1], b' ' | b'\t') {
        end -= 1;
    }
    if end < 2 || &source[end - 2..end] != "]]" {
        return None;
    }
    let lowered = source[..end].to_ascii_lowercase();
    let opener = lowered.rfind("[[/footnote")?;
    let line_start = source[..opener].rfind('\n').map_or(0, |index| index + 1);
    match source[line_start..opener].trim() {
        "*" => Some("*"),
        "#" => Some("#"),
        _ => None,
    }
}

fn parse_footnote_block<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
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
        return Err(parser.make_err(if parser.settings().layout.legacy() {
            ParseErrorKind::RuleFailed
        } else {
            ParseErrorKind::FootnotesNested
        }));
    }

    if parser.settings().layout.legacy() && parser.has_footnote_block() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let mut arguments = parser.get_head_map_wikidot(&BLOCK_FOOTNOTE_BLOCK, in_head)?;

    let title_explicit_empty = arguments.is_explicit_empty("title");
    let title = arguments.get("title").map(|title| {
        if title_explicit_empty {
            Cow::Borrowed(crate::tree::WIKIDOT_EXPLICIT_EMPTY_LABEL_MARKER)
        } else {
            title
        }
    });
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

    let inline_suffix = footnote_block_has_inline_suffix(parser);
    let inline_prefix =
        footnote_block_line_prefix(parser, opener_start).is_inline_prose();
    let inline_prose = inline_prefix || inline_suffix;

    parser.set_footnote_block();

    ok!(inline_prose; Element::FootnoteBlock {
        title,
        hide,
        wikidot_wrappers: Vec::new(),
        wikidot_inline_suffix: inline_suffix,
        wikidot_inline_prefix: inline_prefix,
    })
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

fn footnote_block_line_prefix(
    parser: &Parser<'_, '_>,
    opener_start: usize,
) -> FootnoteBlockLinePrefix {
    if parser.settings().layout.legacy()
        && wikidot_footnote_block_has_inline_prefix(parser, opener_start)
    {
        FootnoteBlockLinePrefix::InlineProse
    } else {
        FootnoteBlockLinePrefix::OwnLine
    }
}

fn footnote_block_has_inline_suffix(parser: &Parser<'_, '_>) -> bool {
    if !parser.settings().layout.legacy() {
        return false;
    }

    let current = parser.current();
    if matches!(
        current.token,
        Token::InputEnd | Token::LineBreak | Token::ParagraphBreak
    ) {
        return false;
    }

    let source = parser.full_text().inner();
    let whitespace_run = source[..current.span.start]
        .bytes()
        .rev()
        .take_while(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
        .collect::<Vec<_>>();
    if whitespace_run.contains(&b'\n') {
        return false;
    }
    if matches!(current.token, Token::Whitespace) {
        return match source[current.span.start..]
            .chars()
            .find(|character| !matches!(character, ' ' | '\t'))
        {
            Some(character) => !matches!(character, '\n' | '\r'),
            None => false,
        };
    }
    true
}

fn wikidot_footnote_block_has_inline_prefix(
    parser: &Parser<'_, '_>,
    opener_start: usize,
) -> bool {
    let source = parser.full_text().inner();
    let line_start = source[..opener_start]
        .rfind('\n')
        .map_or(0, |index| index + 1);

    source[line_start..opener_start]
        .chars()
        .any(|character| !matches!(character, ' ' | '\t' | '\0'))
}

#[derive(Debug)]
struct ParserWrap<'p, 'r, 't> {
    parser: &'p mut Parser<'r, 't>,
    zone_restore: Option<Option<usize>>,
    list_depth_restore: Option<Option<usize>>,
}

impl<'p, 'r, 't> ParserWrap<'p, 'r, 't> {
    #[inline]
    fn new(parser: &'p mut Parser<'r, 't>) -> Self {
        parser.set_footnote_flag(true);

        ParserWrap {
            parser,
            zone_restore: None,
            list_depth_restore: None,
        }
    }

    fn arm_footnote_zone(&mut self) {
        let until = wikidot_footnote_zone_end(self.parser);
        self.zone_restore = Some(self.parser.wikidot_footnote_zone_until());
        self.parser.set_wikidot_footnote_zone_until(Some(until));
        self.list_depth_restore = Some(self.parser.list_depth_at_footnote());
        self.parser
            .set_list_depth_at_footnote(Some(self.parser.wikidot_li_pairing_depth()));
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
        if let Some(previous) = self.zone_restore {
            self.parser.set_wikidot_footnote_zone_until(previous);
        }
        if let Some(previous) = self.list_depth_restore {
            self.parser.set_list_depth_at_footnote(previous);
        }
    }
}

fn wikidot_footnote_zone_end(parser: &Parser<'_, '_>) -> usize {
    let mut scan = parser.clone();
    let mut line_has_content = false;
    let mut in_comment = false;

    loop {
        if in_comment && let Some(count) = comment_closer_token_count(&scan) {
            if scan.step_n(count).is_err() {
                return usize::MAX;
            }
            in_comment = false;
            continue;
        }

        let token = scan.current();
        match token.token {
            Token::InputEnd => return usize::MAX,
            Token::LineBreak | Token::ParagraphBreak => {
                if !in_comment && line_has_content {
                    return token.span.end;
                }
            }
            Token::Whitespace | Token::DiscardedControl => {}
            Token::LeftComment => {
                in_comment = true;
            }
            Token::LeftBlockEnd => {
                if !in_comment
                    && scan.look_ahead(0).is_some_and(|next| {
                        next.token == Token::Identifier
                            && next.slice.eq_ignore_ascii_case("footnote")
                    })
                {
                    return usize::MAX;
                }
                if !in_comment {
                    line_has_content = true;
                }
            }
            _ => {
                if !in_comment {
                    let slice = token.slice;
                    if slice.contains('\n') {
                        let mut offset = 0usize;
                        let mut segments = slice.split('\n').peekable();
                        while let Some(line) = segments.next() {
                            let line_content =
                                line.chars().any(|character| !character.is_whitespace());
                            if segments.peek().is_some() {
                                if line_has_content || line_content {
                                    return token.span.start + offset + line.len() + 1;
                                }
                                offset += line.len() + 1;
                            } else if line_content {
                                line_has_content = true;
                            }
                        }
                    } else {
                        line_has_content = true;
                    }
                }
            }
        }

        if scan.step().is_err() {
            return usize::MAX;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::Element;

    fn render_wikidot(source: &str) -> (String, Vec<ParseError>) {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        (HtmlRender.render(&tree, &page_info, &settings).body, errors)
    }

    #[test]
    fn footnote_rejects_nested_footnote_blocks() {
        let page_info = PageInfo::dummy();
        let wikidot = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[footnote]][[footnoteblock]][[/footnote]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &wikidot).into();

        assert!(
            errors
                .iter()
                .any(|error| error.kind() == ParseErrorKind::RuleFailed),
            "Wikidot: {errors:#?}",
        );

        let html = HtmlRender.render(&tree, &page_info, &wikidot).body;
        assert!(html.contains("footnoteblock"), "Wikidot: {html}");

        let wikijump = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let (tree_j, errors_j) =
            crate::parse(&tokenization, &page_info, &wikijump).into();
        assert!(
            errors_j
                .iter()
                .any(|error| error.kind() == ParseErrorKind::FootnotesNested),
            "Wikijump: {errors_j:#?}",
        );
        let html_j = HtmlRender.render(&tree_j, &page_info, &wikijump).body;
        assert!(html_j.contains("footnoteblock"), "Wikijump: {html_j}");
    }

    #[test]
    fn footnote_module_first_content_stays_literal() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[footnote]][[module Rate]][[/footnote]]");
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert_eq!(tree.footnotes.len(), 1);
        let body_text: String = tree.footnotes[0]
            .iter()
            .map(|element| match element {
                Element::Text(text) => text.as_ref(),
                _ => panic!("unexpected footnote body element: {element:?}"),
            })
            .collect();
        assert_eq!(body_text, "[[module Rate]]");
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
    fn wikidot_footnote_leaves_mid_line_math_opener_literal() {
        let (html, _) = render_wikidot(
            "Text[[footnote]]note [[math]]\nx^2\n[[/math]] end[[/footnote]] done.\n\n[[footnoteblock]]",
        );

        assert!(!html.contains("class=\"math-equation\""), "{html}");
        assert!(html.contains("note [[math]]"), "{html}");
        assert!(html.contains("x^2"), "{html}");
        assert!(html.contains("[[/math]] end"), "{html}");
    }

    #[test]
    fn wikidot_footnote_leaves_single_line_math_block_literal() {
        let (html, _) = render_wikidot(
            "Text[[footnote]]See [[math]] x^2 [[/math]] here[[/footnote]]\n\n[[footnoteblock]]",
        );

        assert!(!html.contains("class=\"math-equation\""), "{html}");
        assert!(html.contains("See [[math]] x^2 [[/math]] here"), "{html}");
    }

    #[test]
    fn wikidot_footnote_from_table_header_renders_math_block() {
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
