/*
 * parsing/rule/impls/block/blocks/bibliography.rs
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
use crate::parsing::consume::wikidot_flat_close_kind_at_byte;
use crate::parsing::rule::impls::block::parser::parse_wikidot_attributes;
use crate::tree::{
    AcceptsPartial, AttributeMap, Bibliography, Container, ContainerType,
    DefinitionListItem, Element, LinkLabel, LinkLocation, LinkType, PartialElement,
    WikidotBlockCloseKind,
};
use std::borrow::Cow;

use super::collapsible::parse_hide_location;

pub const BLOCK_BIBLIOGRAPHY: BlockRule = BlockRule {
    name: "block-bibliography",
    accepts_names: &["bibliography"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
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
    debug!("Parsing bibliography block {name}, in-head {in_head}, score {flag_score}");
    assert!(!flag_star, "Bibliography doesn't allow star flag");
    assert!(!flag_score, "Bibliography doesn't allow score flag");
    assert_block_name(&BLOCK_BIBLIOGRAPHY, name);
    if parser.settings().layout.legacy() && name != "bibliography" {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.settings().layout.legacy()
        && !parser.discarding_hidden_body()
        && parser.has_bibliography()
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let wikidot_candidate =
        parser.settings().layout.legacy() && !parser.discarding_hidden_body();
    let nested_wikidot_opener =
        wikidot_candidate && parser.in_wikidot_bibliography_body();
    let source = parser.full_text().inner();

    let (mut arguments, _) =
        parser.get_head_map_with_body_start_wikidot(&BLOCK_BIBLIOGRAPHY, in_head)?;
    let opener_end = opener_start
        + source[opener_start..]
            .find("]]")
            .expect("parsed bibliography opener has closing brackets")
        + "]]".len();

    if nested_wikidot_opener {
        let opener = &source[opener_start..opener_end];
        if opener != "[[bibliography]]" {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }

        let mut elements = Vec::new();
        if source[..opener_start].ends_with('\n') {
            elements.push(text!(" "));
        }
        elements.push(Element::Text(cow!(opener)));
        if &source[opener_end..parser.current().span.start] == "\n" {
            elements.push(text!(" "));
        }
        return success_elements(elements);
    }

    let title = arguments.get("title");
    let hide = arguments.get_bool(parser, "hide")?.unwrap_or(false);

    if wikidot_candidate && !parser.has_body_end_block(&BLOCK_BIBLIOGRAPHY) {
        return Err(parser.make_end_of_input_err());
    }

    let body_start = parser.current().span.start;
    if wikidot_candidate {
        parser.enter_wikidot_bibliography_body();
    }
    parser.enter_bibliography_parse_body();
    let body = parser.get_body_elements(&BLOCK_BIBLIOGRAPHY, false);
    parser.leave_bibliography_parse_body();
    if wikidot_candidate {
        parser.leave_wikidot_bibliography_body();
    }
    let body = body?;
    if !parser.discarding_hidden_body() {
        wikidot_skip_orphan_closers_after_bibliography_body(parser);
    }
    let (elements, errors, _) = body.into();
    let crossed_residual = wikidot_candidate
        .then(|| {
            wikidot_crossed_bibliography_residual(
                source,
                body_start,
                parser.current().span.start,
            )
        })
        .flatten();

    let mut bibliography = Bibliography::new();
    let mut in_residual = false;
    let mut seen_definition = false;
    let mut crossed_residual_inserted = false;
    let mut crossed_residual_done = false;

    for element in elements {
        match element {
            Element::DefinitionList(items) => {
                if seen_definition && crossed_residual_inserted {
                    crossed_residual_done = true;
                }
                for item in items {
                    bibliography.add(item.key_string, item.value_elements);
                }
                seen_definition = true;
                in_residual = false;
            }

            _ if crossed_residual.is_some()
                && seen_definition
                && !crossed_residual_done =>
            {
                if !crossed_residual_inserted {
                    for residual in crossed_residual.as_ref().unwrap() {
                        bibliography.add_residual(residual.clone());
                    }
                    crossed_residual_inserted = true;
                }
                in_residual = true;
            }

            _ if element.is_whitespace() && !in_residual => continue,

            mut element => {
                if wikidot_candidate && matches!(element, Element::LineBreak) {
                    element = text!(" ");
                }
                if wikidot_candidate {
                    convert_nested_bibliography_items(&mut element, &mut bibliography);
                }
                bibliography.add_residual(element);
                in_residual = true;
            }
        }
    }

    if wikidot_candidate {
        for (_, elements) in bibliography.slice_mut() {
            for element in elements.iter_mut() {
                let Element::Partial(PartialElement::WikidotBlockClose(
                    WikidotBlockCloseKind::Collapsible { head },
                )) = element
                else {
                    continue;
                };
                if *head >= body_start {
                    continue;
                }
                let Some(hide_text) =
                    wikidot_crossed_collapsible_bottom(source, parser, *head)
                else {
                    continue;
                };
                *element = Element::Partial(PartialElement::WikidotBlockClose(
                    WikidotBlockCloseKind::CollapsibleCrossedBottom {
                        head: *head,
                        hide_text,
                    },
                ));
            }
        }
    }

    let index = parser.push_bibliography(bibliography);
    let block = Element::BibliographyBlock { index, title, hide };
    let frames = if wikidot_candidate {
        wikidot_crossed_frames(parser, body_start)
    } else {
        0
    };
    if frames == 0 {
        return ok!(block, errors);
    }
    let close = WikidotBlockCloseKind::BibliographyCrossed {
        head: opener_start,
        frames,
    };
    ok!(
        vec![
            block,
            Element::Partial(PartialElement::WikidotBlockClose(close))
        ],
        errors
    )
}

/// Live Bibitem (position 22) runs on the whole body text before the later line
/// rules, so a `: label : text` line becomes a bibitem token even when a table,
/// list or collapsible later wraps the line. Such an item is rendered from its
/// nested tree position: replace the nested definition list with the bibitem
/// element live produces and number it in source order.
pub(crate) fn wikidot_split_tail_bibitems<'r, 't>(
    parser: &mut Parser<'r, 't>,
    items: Vec<DefinitionListItem<'t>>,
) -> Vec<Element<'t>>
where
    'r: 't,
{
    items
        .into_iter()
        .map(|item| {
            let citation = parser.allocate_bibliography_split_citation();
            let mut contents = Vec::with_capacity(item.value_elements.len() + 1);
            if item.value_elements.is_empty() {
                contents.push(Element::Text(Cow::Owned(format!("{citation}."))));
            } else {
                contents.push(Element::Text(Cow::Owned(format!("{citation}. "))));
                contents.extend(item.value_elements);
            }
            let mut attributes = AttributeMap::new();
            attributes.insert("class", Cow::Borrowed("bibitem"));
            attributes.insert("id", Cow::Owned(format!("bibitem-{citation}")));
            Element::Container(Container::new(ContainerType::Div, contents, attributes))
        })
        .collect()
}

fn convert_nested_bibliography_items<'t>(
    element: &mut Element<'t>,
    bibliography: &mut Bibliography<'t>,
) {
    crate::parsing::inline_scope::visit_children_mut(element, &mut |children| {
        let mut output = Vec::with_capacity(children.len());
        for child in children.drain(..) {
            let items = match child {
                Element::DefinitionList(items) => items,
                mut other => {
                    convert_nested_bibliography_items(&mut other, bibliography);
                    output.push(other);
                    continue;
                }
            };
            for item in items {
                let citation = bibliography.add_nested(item.key_string);
                let mut contents = Vec::with_capacity(item.value_elements.len() + 1);
                if item.value_elements.is_empty() {
                    contents.push(Element::Text(Cow::Owned(format!("{citation}."))));
                } else {
                    contents.push(Element::Text(Cow::Owned(format!("{citation}. "))));
                    contents.extend(item.value_elements);
                }
                let mut attributes = AttributeMap::new();
                attributes.insert("class", Cow::Borrowed("bibitem"));
                attributes.insert("id", Cow::Owned(format!("bibitem-{citation}")));
                output.push(Element::Container(Container::new(
                    ContainerType::Div,
                    contents,
                    attributes,
                )));
            }
        }
        *children = output;
    });
}

fn wikidot_crossed_frames<'r, 't>(parser: &Parser<'r, 't>, body_start: usize) -> usize
where
    'r: 't,
{
    parser
        .wikidot_block_pairs_closed_between(body_start, parser.current().span.start)
        .iter()
        .filter(|(_, pair)| pair.head_byte < body_start)
        .filter_map(|(closer, _)| wikidot_flat_close_kind_at_byte(parser, *closer))
        .map(|kind| kind.rendered_frames())
        .sum()
}

fn wikidot_crossed_collapsible_bottom<'r, 't>(
    source: &'t str,
    parser: &Parser<'r, 't>,
    head_byte: usize,
) -> Option<Option<Cow<'t, str>>>
where
    'r: 't,
{
    let rest = source.get(head_byte..)?.strip_prefix("[[")?;
    let head_end = rest.find("]]")?;
    let head_text = &rest[..head_end];
    if head_text.contains("[!--") || head_text.contains("[[") {
        return None;
    }
    let (name, field) = head_text
        .find([' ', '\t', '\n', '\r'])
        .map(|split| head_text.split_at(split))
        .unwrap_or((head_text, ""));
    if !name.eq_ignore_ascii_case("collapsible") {
        return None;
    }
    let mut arguments = parse_wikidot_attributes(field);
    let (_, show_bottom) = arguments
        .get("hideLocation")
        .map(|value| parse_hide_location(&value, parser))
        .transpose()
        .ok()?
        .unwrap_or((true, false));
    if !show_bottom {
        return None;
    }
    Some(arguments.get("hide"))
}

fn wikidot_crossed_bibliography_residual<'t>(
    source: &'t str,
    body_start: usize,
    owner_end: usize,
) -> Option<Vec<Element<'t>>> {
    let body = source.get(body_start..owner_end)?;
    let close = find_ascii_case_insensitive(body, "[[/bibliography]]")?;
    let first_close_end = close + "[[/bibliography]]".len();
    let second = &body[first_close_end..];
    if !second.starts_with("[[/") {
        return None;
    }
    let second_close = second.find("]]")? + 2;
    let target = &body[close + 2..first_close_end + second_close];
    let after_second = &body[first_close_end + second_close..];
    let nested_relative = find_ascii_case_insensitive(after_second, "[[bibliography")?;
    let nested_start = first_close_end + second_close + nested_relative;
    let nested_end = nested_start + body[nested_start..].find("]]")?;
    let label = &body[nested_start..nested_end];
    if label.eq_ignore_ascii_case("[[bibliography") {
        return None;
    }

    Some(vec![
        text!("[ "),
        Element::Link {
            ltype: LinkType::Direct,
            link: LinkLocation::Url(Cow::Borrowed(target)),
            label: LinkLabel::Text(Cow::Borrowed(label)),
            target: None,
        },
        text!("]"),
    ])
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn wikidot_skip_orphan_closers_after_bibliography_body<'r, 't>(
    parser: &mut Parser<'r, 't>,
) where
    'r: 't,
{
    if parser.discarding_hidden_body() {
        return;
    }
    if matches!(
        parser.accepts_partial(),
        AcceptsPartial::TableRow | AcceptsPartial::TableCell | AcceptsPartial::ListItem,
    ) {
        return;
    }
    loop {
        while matches!(
            parser.current().token,
            Token::LineBreak | Token::ParagraphBreak | Token::Whitespace
        ) && parser.step().is_ok()
        {}
        if parser.current().token != Token::LeftBlockEnd {
            return;
        }
        let mut probe = parser.clone();
        let Ok(name) = probe.get_end_block() else {
            return;
        };
        let name = name.strip_suffix('_').unwrap_or(&name);
        if !name.eq_ignore_ascii_case("cell")
            && !name.eq_ignore_ascii_case("hcell")
            && !name.eq_ignore_ascii_case("row")
            && !name.eq_ignore_ascii_case("table")
            && !name.eq_ignore_ascii_case("li")
            && !name.eq_ignore_ascii_case("ul")
            && !name.eq_ignore_ascii_case("ol")
        {
            return;
        }
        parser.update(&probe);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn wikidot_bibliography_closer_accepts_trailing_line_blanks() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        for blanks in ["", " ", "\t", "  ", " \t"] {
            let source = format!(
                "[[bibliography]]\n: alpha : Alpha reference\n[[/bibliography]]{blanks}\n\nafter"
            );
            let tokenization = crate::tokenize(&source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            assert!(errors.is_empty(), "{blanks:?}: {errors:?}");
            let html = HtmlRender.render(&tree, &page_info, &settings).body;
            assert_eq!(html.matches("bibitems").count(), 1, "{blanks:?}: {html}");
            assert!(!html.contains("[[bibliography"), "{blanks:?}: {html}");
        }
    }

    #[test]
    fn wikidot_bibliography_closes_after_nested_flat_block_markers() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = concat!(
            "[[bibliography]]\n",
            ": alpha : Alpha reference\n",
            "[[table]]\n",
            "[[row]]\n",
            "[[cell]]x[[/cell]]\n",
            "[[/row]]\n",
            "[[/table]]\n",
            "[[/bibliography]]",
        );
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:?}");
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(html.matches("bibitems").count(), 1, "{html}");
        assert!(!html.contains("[[/bibliography"), "{html}");
    }

    #[test]
    fn wikidot_bibliography_closes_before_table_cell_closer() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = concat!(
            "[[bibliography]]\n",
            ": a : A\n",
            "[[table]]\n",
            "[[row]]\n",
            "[[cell]]\n",
            ": b : B\n",
            "[[/bibliography]]\n",
            "[[/cell]]\n",
            "[[/row]]\n",
            "[[/table]]\n",
            "after",
        );
        let mut prepped = source.to_owned();
        crate::preprocess_for_layout(&mut prepped, Layout::Wikidot);
        let tokenization = crate::tokenize(&prepped);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:?}");
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(!html.contains("[[/bibliography]]"), "{html}");
        assert!(html.contains("after"), "{html}");
    }

    #[test]
    fn bibliography_block_collects_definition_items_and_options() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(
            "[[bibliography title=\"Works\" hide=\"true\"]]\n: alpha : Alpha reference\n[[/bibliography]]",
        );
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty());
        match tree.elements.as_slice() {
            [Element::BibliographyBlock { index, title, hide }] => {
                assert_eq!(*index, 0);
                assert_eq!(title.as_deref(), Some("Works"));
                assert!(*hide);
            }
            other => panic!("expected bibliography block, got {other:?}"),
        }

        let (reference_index, reference_elements) = tree
            .bibliographies
            .get_reference("alpha")
            .expect("bibliography reference should be stored");
        assert_eq!(reference_index, 1);
        assert_eq!(
            reference_elements,
            [text!("Alpha"), text!(" "), text!("reference")]
        );
    }

    #[test]
    fn bibliography_block_preserves_non_definition_body() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization =
            crate::tokenize("[[bibliography]]\nnot a definition\n[[/bibliography]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:#?}");
        let bibliography = tree.bibliographies.get_bibliography(0);
        assert_eq!(bibliography.slice().len(), 1, "{:#?}", bibliography.slice(),);
        assert!(Bibliography::is_residual(&bibliography.slice()[0].0));
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(
            html,
            "<div class=\"bibitems\"><div class=\"title\">Bibliography</div>\nnot a definition</div>",
        );
    }

    #[test]
    fn wikidot_bibliography_renders_legacy_dom_and_self_citations() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = concat!(
            "[[bibliography title=\"Works\" hide=\"true\"]]\n",
            ": alpha : ((bibcite alpha))\n",
            "[[/bibliography]]\n",
            "((bibcite alpha))",
        )
        .to_owned();
        crate::preprocess(&mut source);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains(r#"<div class="bibitems">"#), "{html}");
        assert!(html.contains(r#"<div class="title">Works</div>"#), "{html}");
        assert!(
            html.contains(r#"<div class="bibitem" id="bibitem-1">1. "#),
            "{html}",
        );
        assert_eq!(html.matches(r#"class="bibcite""#).count(), 2, "{html}");
        assert!(!html.contains("wj-bibliography"), "{html}");
        assert!(!html.contains("error-inline"), "{html}");
    }

    #[test]
    fn wikidot_unclosed_bibliography_opener_remains_literal() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[bibliography]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(
            errors.iter().any(|error| {
                error.rule() == "block-bibliography"
                    && error.kind() == ParseErrorKind::EndOfInput
            }),
            "{errors:#?}",
        );
        assert_eq!(html, "<p>[[bibliography]]</p>");
    }

    #[test]
    fn wikidot_unclosed_bibliography_after_footnote_remains_literal() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(
            "A claim[[footnote]]with a note[[/footnote]].\n\n[[bibliography]]",
        );
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(
            errors.iter().any(|error| {
                error.rule() == "block-bibliography"
                    && error.kind() == ParseErrorKind::EndOfInput
            }),
            "{errors:#?}",
        );
        assert_eq!(
            html,
            "<p>A claim<sup class=\"footnoteref\"><a id=\"footnoteref-1\" href=\"javascript:;\" class=\"footnoteref\" onclick=\"WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)\">1</a></sup>.</p><p>[[bibliography]]</p><div class=\"footnotes-footer\"><div class=\"title\">Footnotes</div><div class=\"footnote-footer\" id=\"footnote-1\"><a href=\"javascript:;\" onclick=\"WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)\">1</a>. with a note</div></div>",
        );
    }
}
