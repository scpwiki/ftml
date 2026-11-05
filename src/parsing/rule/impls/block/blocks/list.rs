/*
 * parsing/rule/impls/block/blocks/list.rs
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
use crate::parsing::{ParserWrap, strip_newlines};
use crate::tree::{
    AcceptsPartial, AttributeMap, Container, ContainerType, ListItem, ListType,
    PartialElement,
};

pub const BLOCK_UL: BlockRule = BlockRule {
    name: "block-list-unordered",
    accepts_names: &["ul"],
    accepts_star: false,
    accepts_score: true,
    accepts_newlines_outside_body: true,
    parse_fn: parse_unordered_block,
};

pub const BLOCK_OL: BlockRule = BlockRule {
    name: "block-list-ordered",
    accepts_names: &["ol"],
    accepts_star: false,
    accepts_score: true,
    accepts_newlines_outside_body: true,
    parse_fn: parse_ordered_block,
};

pub const BLOCK_LI: BlockRule = BlockRule {
    name: "block-list-item",
    accepts_names: &["li"],
    accepts_star: false,
    accepts_score: true,
    accepts_newlines_outside_body: true,
    parse_fn: parse_list_item,
};

fn parse_unordered_block<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    let block = (&BLOCK_UL, ListType::Bullet);

    parse_list_block(
        block,
        parser,
        name,
        _opener_start,
        flag_star,
        flag_score,
        in_head,
    )
}

fn parse_ordered_block<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    let block = (&BLOCK_OL, ListType::Numbered);

    parse_list_block(
        block,
        parser,
        name,
        _opener_start,
        flag_star,
        flag_score,
        in_head,
    )
}

fn parse_list_block<'r, 't>(
    (block_rule, list_type): (&BlockRule, ListType),
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    let depth = parser.wikidot_li_pairing_depth();
    parser.set_wikidot_li_pairing_depth(depth + 1);
    let track_collect = parser.settings().layout.legacy();
    if track_collect {
        parser.push_wikidot_li_collect(None);
    }
    let result = parse_list_block_impl(
        (block_rule, list_type),
        parser,
        name,
        _opener_start,
        flag_star,
        flag_score,
        in_head,
    );
    if track_collect {
        parser.pop_wikidot_li_collect();
    }
    parser.set_wikidot_li_pairing_depth(depth);
    result
}

fn parse_list_block_impl<'r, 't>(
    (block_rule, list_type): (&BlockRule, ListType),
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    let rule_name = block_rule.name;
    debug!(
        "{name}/{rule_name}/{} head={in_head} score={flag_score}",
        list_type.name(),
    );

    let nested = parser.accepts_partial() == AcceptsPartial::ListItem;
    let wikidot = parser.settings().layout.legacy();
    let parser = &mut ParserWrap::new(parser, AcceptsPartial::ListItem);

    assert!(!flag_star, "List block doesn't allow star flag");
    assert_block_name(block_rule, name);

    let arguments = parser.get_head_map_wikidot(block_rule, in_head)?;
    let attributes = arguments.to_attribute_map(parser.settings());
    if wikidot && parser.wikidot_li_pairing_depth() == 1 {
        parser.compute_wikidot_li_pairing();
    }
    if parser.settings().layout.legacy() && !parser.has_body_end_block(block_rule) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let body = parser.get_body_elements(block_rule, false)?;
    let fused_close = parser.wikidot_body_closed_with_fused_close(block_rule);
    let closer_has_same_line_residual = !matches!(
        parser.current().token,
        Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
    );
    let (mut elements, errors, _) = body.into();

    if wikidot && name != block_rule.accepts_names[0] {
        strip_newlines(&mut elements);
        if fused_close {
            elements.push(text!("]"));
        }
        if !closer_has_same_line_residual {
            elements.push(Element::LineBreak);
        }
        return ok!(false; elements, errors);
    }

    let mut items = Vec::new();
    let mut literal_elements = Vec::new();

    if flag_score {
        strip_newlines(&mut elements);
    }

    if elements.is_empty() {
        if parser.settings().layout.legacy() {
            return if nested {
                ok!(false; Elements::None, errors)
            } else {
                ok!(false; Element::LineBreak, errors)
            };
        }
        return Err(parser.make_err(ParseErrorKind::ListEmpty));
    }

    let content_follows = wikidot_content_follows(parser.remaining());

    for element in elements {
        match element {
            Element::Partial(PartialElement::ListItem(list_item)) => {
                push_literal_list_item(
                    &mut items,
                    &mut literal_elements,
                    wikidot,
                    flag_score,
                    content_follows,
                );
                if !matches!(&list_item, ListItem::Elements { elements, .. } if elements.is_empty())
                {
                    items.push(list_item);
                }
            }

            element @ Element::List { .. } => {
                push_literal_list_item(
                    &mut items,
                    &mut literal_elements,
                    wikidot,
                    flag_score,
                    content_follows,
                );
                if parser.settings().layout.legacy()
                    && let Some(ListItem::Elements { elements, .. }) = items.last_mut()
                {
                    elements.push(element);
                    continue;
                }
                let element = Box::new(element);
                items.push(ListItem::SubList { element });
            }

            element if element.is_whitespace() && literal_elements.is_empty() => continue,

            element => literal_elements.push(element),
        }
    }
    push_literal_list_item(
        &mut items,
        &mut literal_elements,
        wikidot,
        flag_score,
        content_follows,
    );

    if wikidot && flag_score {
        for item in &mut items {
            if let ListItem::Elements { elements, .. } = item {
                let terminal_inline_raw_line_break = matches!(
                    elements.as_slice(),
                    [.., Element::Raw(_), Element::LineBreak]
                );
                strip_newlines(elements);
                if terminal_inline_raw_line_break {
                    elements.push(Element::LineBreak);
                }
            }
        }
    }

    if items.is_empty() {
        if parser.settings().layout.legacy() {
            return if nested {
                ok!(false; Elements::None, errors)
            } else {
                ok!(false; Element::LineBreak, errors)
            };
        }
        return Err(parser.make_err(ParseErrorKind::ListEmpty));
    }

    let element = Element::List {
        ltype: list_type,
        items,
        attributes,
        wikidot_explicit_block: parser.settings().layout.legacy(),
    };
    if parser.settings().layout.legacy()
        && !nested
        && !flag_score
        && !closer_has_same_line_residual
    {
        let mut output = vec![element];
        if fused_close {
            output.push(text!("]"));
        }
        output.push(Element::LineBreak);
        ok!(false; output, errors)
    } else if fused_close {
        ok!(false; vec![element, text!("]")], errors)
    } else {
        success_elements_with_paragraph_safety(false, element, errors)
    }
}

fn push_literal_list_item<'t>(
    items: &mut Vec<ListItem<'t>>,
    elements: &mut Vec<Element<'t>>,
    append_to_last: bool,
    outer_list_is_scored: bool,
    content_follows: bool,
) {
    while elements.last().is_some_and(Element::is_whitespace) {
        elements.pop();
    }
    if elements.is_empty() {
        return;
    }

    if append_to_last
        && let Some(ListItem::Elements {
            elements: item_elements,
            ..
        }) = items.last_mut()
    {
        item_elements.append(elements);
    } else {
        if append_to_last && !outer_list_is_scored {
            wrap_wikidot_malformed_scored_items(elements, content_follows);
        }
        let mut attributes = AttributeMap::new();
        assert!(attributes.insert("style", cow!("list-style: none")));
        items.push(ListItem::Elements {
            elements: std::mem::take(elements),
            attributes,
        });
    }
}

fn wikidot_content_follows(tokens: &[ExtractedToken<'_>]) -> bool {
    let mut in_comment = false;
    let mut in_block_end = false;
    let mut comment_tail_dashes = false;
    for extracted in tokens {
        if in_block_end {
            if extracted.token == Token::RightBlock {
                in_block_end = false;
            }
            continue;
        }
        if in_comment {
            if comment_tail_dashes && extracted.token == Token::RightBracket {
                in_comment = false;
                comment_tail_dashes = false;
            } else {
                comment_tail_dashes = extracted.slice.ends_with("--");
            }
            continue;
        }
        match extracted.token {
            Token::InputEnd => return false,
            Token::LineBreak | Token::ParagraphBreak | Token::Whitespace => {}
            Token::LeftComment => in_comment = true,
            Token::LeftBlockEnd => in_block_end = true,
            Token::Other if extracted.slice.trim().is_empty() => {}
            _ => return true,
        }
    }
    false
}

fn wrap_wikidot_malformed_scored_items(
    elements: &mut Vec<Element<'_>>,
    content_follows: bool,
) {
    let mut lines = vec![Vec::new()];
    for element in std::mem::take(elements) {
        if element == Element::LineBreak {
            lines.push(Vec::new());
        } else {
            lines.last_mut().unwrap().push(element);
        }
    }
    let line_text = |line: &[Element]| {
        line.iter()
            .filter_map(|element| match element {
                Element::Text(text) => Some(text.as_ref()),
                _ => None,
            })
            .collect::<String>()
    };
    if line_text(&lines[0]) != "[[li_]]" {
        *elements = join_lines(lines);
        return;
    }
    let Some(paragraph_start) = lines
        .iter()
        .enumerate()
        .skip(1)
        .find_map(|(index, line)| (line_text(line) == "[[li_]]").then_some(index))
    else {
        *elements = join_lines(lines);
        return;
    };

    let paragraph_lines = lines.split_off(paragraph_start);
    *elements = join_lines(lines);
    for line in paragraph_lines {
        if line.is_empty() {
            continue;
        }
        if line
            .iter()
            .all(|element| matches!(element, Element::Text(_)))
            && line_text(&line) == "[[/li]]"
            && !content_follows
        {
            elements.extend(line);
            continue;
        }
        let paragraph =
            Container::new(ContainerType::Paragraph, line, AttributeMap::new());
        elements.push(Element::Container(paragraph));
    }
}

fn join_lines<'t>(lines: Vec<Vec<Element<'t>>>) -> Vec<Element<'t>> {
    let mut elements = Vec::new();
    for (index, line) in lines.into_iter().enumerate() {
        if index > 0 {
            elements.push(Element::LineBreak);
        }
        elements.extend(line);
    }
    elements
}

fn parse_list_item<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("List item block: name={name}, in_head={in_head}, score={flag_score}");
    let wikidot = parser.settings().layout.legacy();
    assert!(!flag_star, "List item block doesn't allow star flag");
    assert_block_name(&BLOCK_LI, name);
    if flag_score && wikidot {
        return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
    }

    let arguments = parser.get_head_map_wikidot(&BLOCK_LI, in_head)?;
    let attributes = arguments.to_attribute_map(parser.settings());
    let body_start = parser.current().span.start;
    if wikidot && parser.wikidot_li_inside_item_body() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if wikidot && parser.wikidot_li_pairing_depth() > 0 {
        if !parser.wikidot_li_head_paired(opener_start) {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
    } else if wikidot && !parser.has_body_end_block(&BLOCK_LI) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if wikidot
        && parser.wikidot_li_pairing_depth() > 0
        && !parser.discarding_hidden_body()
    {
        parser.push_wikidot_li_collect(Some(opener_start));
        let body = parser.get_wikidot_li_body_segments(&BLOCK_LI);
        parser.pop_wikidot_li_collect();
        let body = body?;
        let source_end = parser.current().span.start;
        let fused_close = parser.wikidot_body_closed_with_fused_close(&BLOCK_LI);
        let (segments, errors, _) = body.into();
        let source = &parser.full_text().inner()[body_start..source_end];
        let mut elements = assemble_wikidot_li_segments(segments, source);
        if fused_close {
            elements.push(text!("]"));
        }
        let element = Element::Partial(PartialElement::ListItem(ListItem::Elements {
            elements,
            attributes,
        }));
        return success_elements_with_paragraph_safety(false, element, errors);
    }

    let body = parser.get_body_elements(&BLOCK_LI, false)?;
    let source_end = parser.current().span.start;
    let fused_close = parser.wikidot_body_closed_with_fused_close(&BLOCK_LI);
    let (mut elements, errors, _) = body.into();

    if flag_score {
        strip_newlines(&mut elements);
    }
    if wikidot {
        let source = &parser.full_text().inner()[body_start..source_end];
        let body = source
            .to_ascii_lowercase()
            .rfind("[[/li")
            .map(|index| &source[..index])
            .unwrap_or(source);
        if body.starts_with("\n\n") || body.ends_with("\n\n") {
            strip_newlines(&mut elements);
        } else if body.ends_with('\n')
            && !matches!(elements.last(), Some(Element::LineBreak))
        {
            elements.push(Element::LineBreak);
        }
        while matches!(elements.last(), Some(Element::Text(text)) if text == " ") {
            elements.pop();
        }
        trim_wikidot_block_gaps(&mut elements);
    }

    if fused_close {
        elements.push(text!("]"));
    }

    let element = Element::Partial(PartialElement::ListItem(ListItem::Elements {
        elements,
        attributes,
    }));

    success_elements_with_paragraph_safety(false, element, errors)
}

fn wikidot_li_segment_is_blank(segment: &[Element]) -> bool {
    segment.iter().all(Element::is_whitespace)
}

fn assemble_wikidot_li_segments<'t>(
    mut segments: Vec<Vec<Element<'t>>>,
    source: &str,
) -> Vec<Element<'t>> {
    let mut had_trailing_empty = false;
    while segments
        .last()
        .is_some_and(|segment| wikidot_li_segment_is_blank(segment))
    {
        segments.pop();
        had_trailing_empty = true;
    }
    let mut had_leading_empty = false;
    while segments
        .first()
        .is_some_and(|segment| wikidot_li_segment_is_blank(segment))
    {
        segments.remove(0);
        had_leading_empty = true;
    }
    for segment in &mut segments {
        let mut index = 1;
        while index < segment.len() {
            if segment[index] == Element::LineBreak
                && !segment[index - 1].paragraph_safe()
            {
                segment.remove(index);
            } else {
                index += 1;
            }
        }
    }
    if let Some(first) = segments.first_mut() {
        while matches!(first.first(), Some(Element::LineBreak)) {
            first.remove(0);
        }
    }
    let mut kept: Vec<Vec<Element<'t>>> = Vec::with_capacity(segments.len());
    let last_index = segments.len().saturating_sub(1);
    for (index, segment) in segments.into_iter().enumerate() {
        if index > 0 && index < last_index && wikidot_li_segment_is_blank(&segment) {
            continue;
        }
        kept.push(segment);
    }
    let mut joins: Vec<bool> = Vec::new();
    for pair in kept.windows(2) {
        let boundary = pair[0]
            .last()
            .is_some_and(|element| !element.paragraph_safe())
            && pair[1].first().is_some_and(Element::paragraph_safe);
        joins.push(boundary);
    }
    let mut elements: Vec<Element<'t>> = Vec::new();
    let kept_last = kept.len().saturating_sub(1);
    for (index, segment) in kept.into_iter().enumerate() {
        if index > 0 && joins[index - 1] {
            elements.push(Element::LineBreak);
        }
        if index > 0 && index < kept_last && segment.iter().all(Element::paragraph_safe) {
            elements.push(Element::Container(Container::new(
                ContainerType::Paragraph,
                segment,
                AttributeMap::new(),
            )));
        } else {
            elements.extend(segment);
        }
    }
    if had_leading_empty || had_trailing_empty {
        strip_newlines(&mut elements);
    }
    if had_trailing_empty
        && elements
            .last()
            .is_some_and(|element| !element.paragraph_safe())
    {
        elements.push(Element::LineBreak);
    } else if !had_trailing_empty {
        let body = source
            .to_ascii_lowercase()
            .rfind("[[/li")
            .map(|index| &source[..index])
            .unwrap_or(source);
        if body.ends_with('\n') && !matches!(elements.last(), Some(Element::LineBreak)) {
            elements.push(Element::LineBreak);
        }
    }
    while matches!(elements.last(), Some(Element::Text(text)) if text == " ") {
        elements.pop();
    }
    elements
}

fn trim_wikidot_block_gaps(elements: &mut Vec<Element<'_>>) {
    let mut index = 1;
    while index + 1 < elements.len() {
        if elements[index] == Element::LineBreak
            && !elements[index - 1].paragraph_safe()
            && !elements[index + 1].paragraph_safe()
        {
            elements.remove(index);
        } else {
            index += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::ParseError;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    fn with_parse<R>(
        source: &str,
        check: impl for<'t> FnOnce(Vec<Element<'t>>, Vec<ParseError>) -> R,
    ) -> R {
        with_parse_layout(source, Layout::Wikidot, check)
    }

    fn with_parse_layout<R>(
        source: &str,
        layout: Layout,
        check: impl for<'t> FnOnce(Vec<Element<'t>>, Vec<ParseError>) -> R,
    ) -> R {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        check(tree.elements, errors)
    }

    fn element_text(elements: &[Element]) -> String {
        elements
            .iter()
            .filter_map(|element| match element {
                Element::Text(text) => Some(text.as_ref()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn wikidot_top_level_list_item_remains_literal_in_its_paragraph() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[li]]\nBaz\n[[/li]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(
            errors
                .iter()
                .any(|error| error.kind() == ParseErrorKind::ListItemOutsideList),
            "{errors:?}",
        );
        assert_eq!(html, "<p>[[li]]<br>\nBaz<br>\n[[/li]]</p>");
    }

    #[test]
    fn unordered_block_list_preserves_attributes_and_items() {
        with_parse(
            r#"[[ul class="menu"]]
[[li class="first"]]Alpha[[/li]]
[[li]]Beta[[/li]]
[[/ul]]"#,
            |tree, errors| {
                assert!(errors.is_empty(), "{errors:?}");
                let [
                    Element::List {
                        ltype,
                        attributes,
                        items,
                        ..
                    },
                    Element::LineBreak,
                ] = tree.as_slice()
                else {
                    panic!("expected one unordered list, got {tree:?}");
                };

                assert_eq!(*ltype, ListType::Bullet);
                assert_eq!(
                    attributes.get().get("class").map(|value| value.as_ref()),
                    Some("menu")
                );
                assert_eq!(items.len(), 2);

                let ListItem::Elements {
                    attributes,
                    elements,
                } = &items[0]
                else {
                    panic!("expected first list item, got {:?}", items[0]);
                };
                assert_eq!(
                    attributes.get().get("class").map(|value| value.as_ref()),
                    Some("first")
                );
                assert_eq!(element_text(elements), "Alpha");

                let ListItem::Elements { elements, .. } = &items[1] else {
                    panic!("expected second list item, got {:?}", items[1]);
                };
                assert_eq!(element_text(elements), "Beta");
            },
        );
    }

    #[test]
    fn wikidot_list_item_drops_blank_line_between_adjacent_blocks() {
        let source = "[[ul]]\n[[li]]\n[[div class=\"a\"]]\nA\n[[/div]]\n\n[[div class=\"b\"]]\nB\n[[/div]]\n[[/li]]\n[[/ul]]";
        with_parse(source, |tree, errors| {
            assert!(errors.is_empty(), "{errors:?}");
            let [Element::List { items, .. }, Element::LineBreak] = tree.as_slice()
            else {
                panic!("expected Wikidot list and trailing break, got {tree:?}");
            };
            let [ListItem::Elements { elements, .. }] = items.as_slice() else {
                panic!("expected one list item, got {items:?}");
            };
            let [
                Element::Container(first),
                Element::Container(second),
                Element::LineBreak,
            ] = elements.as_slice()
            else {
                panic!(
                    "expected adjacent block elements and a terminal break, got {elements:?}"
                );
            };
            assert_eq!(first.ctype(), ContainerType::Div);
            assert_eq!(second.ctype(), ContainerType::Div);
        });

        with_parse_layout(source, Layout::Wikijump, |tree, errors| {
            assert!(errors.is_empty(), "{errors:?}");
            let [Element::List { items, .. }] = tree.as_slice() else {
                panic!("expected Wikijump list, got {tree:?}");
            };
            let [ListItem::Elements { elements, .. }] = items.as_slice() else {
                panic!("expected one Wikijump list item, got {items:?}");
            };
            assert!(matches!(
                elements.as_slice(),
                [
                    Element::Container(_),
                    Element::LineBreak,
                    Element::Container(_),
                    ..
                ]
            ));
        });
    }

    #[test]
    fn ordered_block_list_accepts_nested_sublist() {
        with_parse(
            r#"[[ol]]
[[li]]Parent[[/li]]
[[ul]]
[[li]]Child[[/li]]
[[/ul]]
[[/ol]]"#,
            |tree, errors| {
                assert!(errors.is_empty(), "{errors:?}");
                let [Element::List { ltype, items, .. }, Element::LineBreak] =
                    tree.as_slice()
                else {
                    panic!("expected one ordered list, got {tree:?}");
                };

                assert_eq!(*ltype, ListType::Numbered);
                assert_eq!(items.len(), 1);
                let ListItem::Elements { elements, .. } = &items[0] else {
                    panic!("expected parent item, got {:?}", items[0]);
                };
                assert_eq!(element_text(elements), "Parent");

                let Some(element @ Element::List { .. }) = elements.last() else {
                    panic!("expected nested sublist in parent item, got {elements:?}");
                };
                let Element::List {
                    ltype,
                    attributes,
                    items,
                    ..
                } = element
                else {
                    panic!("expected nested list element, got {element:?}");
                };

                assert_eq!(*ltype, ListType::Bullet);
                assert!(attributes.get().is_empty());
                let [ListItem::Elements { elements, .. }] = items.as_slice() else {
                    panic!("expected one nested list item, got {items:?}");
                };
                assert_eq!(element_text(elements), "Child");
            },
        );
    }

    #[test]
    fn scored_list_keeps_nested_list_attached_after_literal_recovery() {
        with_parse(
            concat!(
                "[[ul_]]\n",
                "[[li_]]\n",
                "Parent\n",
                "[[/li]]\n",
                "[[ul]]\n",
                "[[li]]Child[[/li]]\n",
                "[[/ul]]\n",
                "[[/ul_]]",
            ),
            |tree, errors| {
                assert!(!errors.is_empty());
                let [Element::List { items, .. }] = tree.as_slice() else {
                    panic!("expected one list, got {tree:?}");
                };
                let [ListItem::Elements { elements, .. }] = items.as_slice() else {
                    panic!("expected one synthetic parent item, got {items:?}");
                };
                assert!(!elements.iter().any(|element| {
                    matches!(
                        element,
                        Element::Container(container)
                            if container.ctype() == ContainerType::Paragraph
                    )
                }));
                let Some(items) = elements.iter().find_map(|element| match element {
                    Element::List { items, .. } => Some(items),
                    _ => None,
                }) else {
                    panic!(
                        "expected nested list attached to parent item, got {elements:?}"
                    );
                };
                let [ListItem::Elements { elements, .. }] = items.as_slice() else {
                    panic!("expected one nested list item, got {items:?}");
                };
                assert_eq!(element_text(elements), "Child");
            },
        );
    }

    #[test]
    fn scored_block_strips_outer_line_breaks() {
        with_parse(
            r#"[[ul_]]
[[li]]
Alpha
[[/li]]
[[/ul]]"#,
            |tree, errors| {
                assert!(errors.is_empty(), "{errors:?}");
                let [Element::List { items, .. }] = tree.as_slice() else {
                    panic!("expected one list, got {tree:?}");
                };
                let [ListItem::Elements { elements, .. }] = items.as_slice() else {
                    panic!("expected one list item, got {items:?}");
                };

                assert!(
                    !elements
                        .iter()
                        .any(|element| matches!(element, Element::LineBreak))
                );
                assert_eq!(element_text(elements), "Alpha");
            },
        );
    }

    #[test]
    fn wikidot_scored_item_preserves_a_terminal_inline_raw_line_break() {
        let source = concat!(
            "[[ul_]]\n",
            "[[li]]\n",
            "Alpha\n",
            "@@ @@\n",
            "[[/li]]\n",
            "[[/ul]]",
        );
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            html,
            concat!(
                "<ul>\n",
                "<li>Alpha<br>\n",
                r#"<span style="white-space: pre-wrap;">&#32;</span><br>"#,
                "\n</li>\n",
                "</ul>",
            ),
        );
    }

    #[test]
    fn empty_block_list_becomes_a_trailing_line_break_like_wikidot() {
        for input in [
            "[[ul]][[/ul]]",
            "[[ul]]\n[[/ul]]",
            "[[ul]]   [[/ul]]",
            "[[ul]]\n \n[[/ul]]",
        ] {
            with_parse(input, |tree, errors| {
                assert!(errors.is_empty(), "{input:?}: {errors:?}");
                assert_eq!(tree, [Element::LineBreak], "{input:?}");
            });
        }
    }

    #[test]
    fn block_list_wraps_bare_body_like_wikidot() {
        with_parse("[[ul]]_[[/ul]]", |tree, errors| {
            assert!(errors.is_empty(), "{errors:#?}");
            let [Element::List { items, .. }, Element::LineBreak] = tree.as_slice()
            else {
                panic!("expected one list, got {tree:?}");
            };
            let [
                ListItem::Elements {
                    elements,
                    attributes,
                },
            ] = items.as_slice()
            else {
                panic!("expected one synthetic item, got {items:?}");
            };
            assert_eq!(element_text(elements), "_");
            assert_eq!(
                attributes.get().get("style").map(|value| value.as_ref()),
                Some("list-style: none"),
            );
        });
    }

    #[test]
    fn wikidot_rejects_spaced_and_scored_list_closers() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        for input in [
            "[[ul]]\n[[li]]spaced[[/li ]]\n[[/ul ]]",
            "[[ul_]]\n[[li_]]\ntext\n[[/li_]]\n[[/ul_]]",
        ] {
            let tokenization = crate::tokenize(input);
            let (tree, _errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;
            assert!(!html.contains("<ul>"), "{input:?}: {html}");
            assert!(html.contains("[[ul"), "{input:?}: {html}");
        }
    }

    #[test]
    fn malformed_scored_items_are_literal_only_in_wikidot_layout() {
        let source = concat!(
            "[[ul]]\n\n",
            "[[li_]]\nALPHA\n[[/li]]\n\n",
            "[[li_]]\n\nBETA\n\n[[/li]]\n",
            "[[li]]GAMMA[[/li]]\n\n",
            "[[li_]] DELTA [[/li]]\n\n",
            "[[/ul]]",
        );

        with_parse_layout(source, Layout::Wikidot, |tree, errors| {
            assert!(!errors.is_empty());
            let [Element::List { items, .. }, Element::LineBreak] = tree.as_slice()
            else {
                panic!("expected Wikidot list and trailing break, got {tree:?}");
            };
            let [
                ListItem::Elements {
                    elements: malformed,
                    attributes,
                },
                ListItem::Elements {
                    elements: gamma, ..
                },
            ] = items.as_slice()
            else {
                panic!("expected two Wikidot items, got {items:?}");
            };
            assert_eq!(
                attributes.get().get("style").map(|value| value.as_ref()),
                Some("list-style: none"),
            );
            assert_eq!(
                malformed
                    .iter()
                    .filter(|element| {
                        matches!(
                            element,
                            Element::Container(container)
                                if container.ctype() == ContainerType::Paragraph
                        )
                    })
                    .count(),
                2,
            );
            assert_eq!(element_text(gamma), "GAMMA[[li_]] DELTA [[/li]]");
        });

        with_parse_layout(source, Layout::Wikijump, |tree, errors| {
            assert!(errors.is_empty(), "{errors:?}");
            let [Element::List { items, .. }] = tree.as_slice() else {
                panic!("expected one Wikijump list, got {tree:?}");
            };
            assert_eq!(items.len(), 4);
            let text: Vec<_> = items
                .iter()
                .map(|item| match item {
                    ListItem::Elements { elements, .. } => element_text(elements),
                    ListItem::SubList { .. } => panic!("unexpected sublist"),
                })
                .collect();
            assert_eq!(text, ["ALPHA", "BETA", "GAMMA", "DELTA "]);
        });
    }

    #[test]
    fn multiline_item_newline_policy_is_wikidot_only() {
        let source = concat!(
            "[[ol]]\n",
            "[[li]]\nALPHA\n[[/li]]\n",
            "[[li]]\n\nBETA\n\n[[/li]]\n",
            "[[/ol]]",
        );

        with_parse_layout(source, Layout::Wikidot, |tree, errors| {
            assert!(errors.is_empty(), "{errors:?}");
            let [Element::List { items, .. }, Element::LineBreak] = tree.as_slice()
            else {
                panic!("expected Wikidot list and trailing break, got {tree:?}");
            };
            let [
                ListItem::Elements {
                    elements: alpha, ..
                },
                ListItem::Elements { elements: beta, .. },
            ] = items.as_slice()
            else {
                panic!("expected two Wikidot items, got {items:?}");
            };
            assert_eq!(alpha, &[text!("ALPHA"), Element::LineBreak]);
            assert_eq!(beta, &[text!("BETA")]);
        });

        with_parse_layout(source, Layout::Wikijump, |tree, errors| {
            assert!(errors.is_empty(), "{errors:?}");
            let [Element::List { items, .. }] = tree.as_slice() else {
                panic!("expected one Wikijump list, got {tree:?}");
            };
            let [
                ListItem::Elements {
                    elements: alpha, ..
                },
                ListItem::Elements { elements: beta, .. },
            ] = items.as_slice()
            else {
                panic!("expected two Wikijump items, got {items:?}");
            };
            assert_eq!(alpha, &[text!("ALPHA")]);
            assert_eq!(
                beta,
                &[Element::LineBreak, text!("BETA"), Element::LineBreak],
            );
        });
    }

    #[test]
    fn nested_list_attachment_is_wikidot_only() {
        let source = "[[ol]][[li]]Parent[[/li]][[ul]][[li]]Child[[/li]][[/ul]][[/ol]]";

        with_parse_layout(source, Layout::Wikidot, |tree, errors| {
            assert!(errors.is_empty(), "{errors:?}");
            let [Element::List { items, .. }, Element::LineBreak] = tree.as_slice()
            else {
                panic!("expected Wikidot list and trailing break, got {tree:?}");
            };
            let [ListItem::Elements { elements, .. }] = items.as_slice() else {
                panic!("expected one Wikidot parent item, got {items:?}");
            };
            assert!(matches!(elements.last(), Some(Element::List { .. })));
        });

        with_parse_layout(source, Layout::Wikijump, |tree, errors| {
            assert!(errors.is_empty(), "{errors:?}");
            let [Element::List { items, .. }] = tree.as_slice() else {
                panic!("expected one Wikijump list, got {tree:?}");
            };
            assert!(matches!(
                items.as_slice(),
                [ListItem::Elements { .. }, ListItem::SubList { .. }]
            ));
        });
    }
}
