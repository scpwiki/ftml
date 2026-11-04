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

use super::super::mapping::get_block_rule_with_name_for_layout;
use super::prelude::*;
use crate::parsing::ParserWrap;
use crate::parsing::collect::{
    consume_iftags_segment_fused, consume_valid_comment, iftags_segment_fused,
};
use crate::parsing::consume::consume;
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

fn wikidot_quote_opener_at_row_start(source: &str, opener_start: usize) -> bool {
    let line_start = source[..opener_start]
        .rfind('\n')
        .map_or(0, |offset| offset + 1);
    let mut prefix = &source[line_start..opener_start];
    let mut saw_quote = false;
    while let Some(rest) = prefix.strip_prefix('>') {
        saw_quote = true;
        prefix = rest.strip_prefix(' ').unwrap_or(rest);
    }
    saw_quote && prefix.is_empty()
}

fn parse_tabview<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    if parser.settings().layout.legacy()
        && parser.native_blockquote_depth().is_some()
        && !wikidot_quote_opener_at_row_start(parser.full_text().inner(), opener_start)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if parser.wikidot_footnote_zone_blocks(opener_start) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if parser.settings().layout.legacy() && parser.in_wikidot_bibliography_body() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let parser = &mut ParserWrap::new(parser, AcceptsPartial::Tab);

    debug!("Parsing tabview block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Tabview doesn't allow star flag");
    assert!(!flag_score, "Tabview doesn't allow score flag");
    assert_block_name(&BLOCK_TABVIEW, name);

    let (tabview_name, body_start) = parser.get_head_value_resolved_with_body_start(
        &BLOCK_TABVIEW,
        in_head,
        |_, value| Ok(value),
    )?;

    let (elements, errors, _) = parser
        .get_body_elements_with_context(&BLOCK_TABVIEW, false, body_start)?
        .into();

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

fn tab_label_finish<'t>(
    source: &'t str,
    start: usize,
    output: Option<String>,
    copied_until: usize,
    end: usize,
    saw_unit: bool,
) -> std::borrow::Cow<'t, str> {
    if saw_unit {
        return std::borrow::Cow::Borrowed("");
    }
    match output {
        Some(mut output) => {
            if end >= copied_until {
                output.push_str(&source[copied_until..end]);
            }
            std::borrow::Cow::Owned(output)
        }
        None => std::borrow::Cow::Borrowed(&source[start..end]),
    }
}

fn tab_title_unit_end<'r, 't>(
    parser: &Parser<'r, 't>,
    probe: &Parser<'r, 't>,
    closer_name: Option<&str>,
) -> bool {
    if probe.current().token == Token::InputEnd {
        return false;
    }
    let start = parser.current().span.start;
    let end = probe.current().span.start;
    let layout = parser.settings().layout;
    let mut terminator = false;
    let tokens: Vec<&crate::parsing::ExtractedToken<'t>> =
        std::iter::once(parser.current())
            .chain(parser.remaining())
            .take_while(|token| token.span.start < end)
            .collect();
    let _ = start;
    for (index, token) in tokens.iter().enumerate() {
        match token.token {
            Token::LineBreak | Token::ParagraphBreak => return false,
            Token::RightBlock | Token::RightLink => {
                if closer_name.is_none() {
                    terminator = true;
                }
            }
            Token::LeftBlockEnd => {
                let name = tokens.get(index + 1).map_or("", |next| next.slice);
                if name.eq_ignore_ascii_case("tab")
                    || name.eq_ignore_ascii_case("tabview")
                    || name.eq_ignore_ascii_case("tabs")
                {
                    return false;
                }
                if let Some(closer) = closer_name
                    && name.eq_ignore_ascii_case(closer)
                {
                    terminator = true;
                }
            }
            _ => {}
        }
    }
    let _ = layout;
    terminator
}

fn tab_title_unit_fused_end<'r, 't>(
    parser: &Parser<'r, 't>,
    probe: &Parser<'r, 't>,
) -> bool {
    let current = probe.current();
    if current.token != Token::RightBracket {
        return false;
    }
    let end = current.span.start;
    let last = std::iter::once(parser.current())
        .chain(parser.remaining())
        .take_while(|token| token.span.start < end)
        .last();
    matches!(last, Some(token) if token.token == Token::RightLink && token.span.end == end)
}

fn tab_title_block_unit<'r, 't>(parser: &Parser<'r, 't>) -> Option<(Parser<'r, 't>, bool)>
where
    'r: 't,
{
    let mut name_probe = parser.clone();
    let (name, _, _) = name_probe.get_block_name(false).ok()?;
    let rule = get_block_rule_with_name_for_layout(&name, parser.settings().layout)?;
    let (closer_name, head_only) = match rule.name {
        "block-span" => (Some("span"), false),
        "block-size" => (Some("size"), false),
        "block-collapsible" => (Some("collapsible"), false),
        "block-anchor" => (Some("a"), false),
        "block-user" => (None, true),
        "block-date" => (None, true),
        "block-equation-ref" => (None, true),
        _ => return None,
    };
    let mut probe = parser.clone();
    let success = match consume(&mut probe) {
        Ok(success) => success,
        Err(_) => {
            if head_only {
                return tab_title_head_only_fused_unit(parser);
            }
            let closer = closer_name?;
            return tab_title_scoped_block_unit(parser, rule.name, closer);
        }
    };
    if matches!(success.item, Elements::None) {
        return None;
    }
    let fused_end = tab_title_unit_fused_end(parser, &probe);
    if head_only {
        if tab_title_unit_end(parser, &probe, None) {
            return Some((probe, fused_end));
        }
        return tab_title_head_only_fused_unit(parser);
    }
    if tab_title_unit_end(parser, &probe, closer_name) {
        return Some((probe, fused_end));
    }
    let closer = closer_name?;
    tab_title_scoped_block_unit(parser, rule.name, closer)
}

fn tab_title_token_block_name(
    tokens: &[&crate::parsing::ExtractedToken],
    index: usize,
    layout: crate::layout::Layout,
) -> Option<&'static str> {
    let mut cursor = index + 1;
    if tokens.get(cursor).map(|token| token.token) == Some(Token::Whitespace) {
        cursor += 1;
    }
    let raw = tokens.get(cursor)?.slice;
    let name = raw.strip_suffix('_').unwrap_or(raw);
    get_block_rule_with_name_for_layout(name, layout).map(|rule| rule.name)
}

fn tab_title_scoped_block_unit<'r, 't>(
    parser: &Parser<'r, 't>,
    rule_name: &'static str,
    _closer_name: &str,
) -> Option<(Parser<'r, 't>, bool)>
where
    'r: 't,
{
    let layout = parser.settings().layout;
    let tokens: Vec<&crate::parsing::ExtractedToken<'t>> =
        std::iter::once(parser.current())
            .chain(parser.remaining())
            .collect();
    if tab_title_token_block_name(&tokens, 0, layout) != Some(rule_name) {
        return None;
    }
    let mut depth = 0_usize;
    let mut end_close = None;
    let mut index = 0_usize;
    while index < tokens.len() {
        let token = tokens[index];
        match token.token {
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                return None;
            }
            Token::LeftBlock | Token::LeftBlockStar => {
                if tab_title_token_block_name(&tokens, index, layout) == Some(rule_name) {
                    depth += 1;
                }
            }
            Token::LeftBlockEnd => {
                let name = tab_title_token_block_name(&tokens, index, layout);
                if matches!(name, Some("block-tab") | Some("block-tabview")) {
                    return None;
                }
                if name == Some(rule_name) {
                    if depth <= 1 {
                        let mut close_index = index + 1;
                        let close = loop {
                            let close_token = tokens.get(close_index)?;
                            match close_token.token {
                                Token::RightBlock | Token::RightLink => {
                                    break *close_token;
                                }
                                Token::LineBreak
                                | Token::ParagraphBreak
                                | Token::InputEnd => {
                                    return None;
                                }
                                _ => {
                                    close_index += 1;
                                }
                            }
                        };
                        end_close = Some(close);
                        break;
                    }
                    depth -= 1;
                }
            }
            _ => {}
        }
        index += 1;
    }
    let close = end_close?;
    let start = parser.current().span.start;
    let slice = &parser.full_text().inner()[start..close.span.start + 2];
    let tokenization = crate::tokenize(slice);
    let outcome =
        crate::parsing::parse(&tokenization, parser.page_info(), parser.settings());
    let (tree, _) = outcome.into();
    if !tab_title_single_construct(&tree.elements) {
        return None;
    }
    let mut probe = parser.clone();
    while probe.current().span.start < close.span.end {
        probe.step().ok()?;
    }
    let fused_end = tab_title_unit_fused_end(parser, &probe);
    Some((probe, fused_end))
}

fn tab_title_single_construct(elements: &[Element]) -> bool {
    match elements {
        [Element::Container(container)]
            if container.ctype() == crate::tree::ContainerType::Paragraph =>
        {
            tab_title_single_construct(container.elements())
        }
        [element] => !matches!(element, Element::Text(_)),
        _ => false,
    }
}

fn tab_title_head_only_fused_unit<'r, 't>(
    parser: &Parser<'r, 't>,
) -> Option<(Parser<'r, 't>, bool)>
where
    'r: 't,
{
    let start = parser.current().span.start;
    let tokens: Vec<&crate::parsing::ExtractedToken<'t>> =
        std::iter::once(parser.current())
            .chain(parser.remaining())
            .collect();
    let mut close = None;
    for token in &tokens {
        match token.token {
            Token::RightBlock | Token::RightLink => {
                close = Some(*token);
                break;
            }
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                return None;
            }
            _ => {}
        }
    }
    let close = close?;
    let slice = &parser.full_text().inner()[start..close.span.start + 2];
    let tokenization = crate::tokenize(slice);
    let outcome =
        crate::parsing::parse(&tokenization, parser.page_info(), parser.settings());
    let (tree, _) = outcome.into();
    if !tab_title_single_construct(&tree.elements) {
        return None;
    }
    let mut probe = parser.clone();
    while probe.current().span.start < close.span.end {
        probe.step().ok()?;
    }
    let fused_end = tab_title_unit_fused_end(parser, &probe);
    Some((probe, fused_end))
}

fn tab_title_link_unit<'r, 't>(parser: &Parser<'r, 't>) -> Option<(Parser<'r, 't>, bool)>
where
    'r: 't,
{
    let mut probe = parser.clone();
    let success = consume(&mut probe).ok()?;
    if matches!(success.item, Elements::None) {
        return None;
    }
    let end = probe.current().span.start;
    let mut terminator = false;
    for token in std::iter::once(parser.current())
        .chain(parser.remaining())
        .take_while(|token| token.span.start < end)
    {
        match token.token {
            Token::LineBreak | Token::ParagraphBreak => return None,
            Token::RightLink => terminator = true,
            Token::LeftBlockEnd => {
                return None;
            }
            _ => {}
        }
    }
    let fused_end = tab_title_unit_fused_end(parser, &probe);
    terminator.then_some((probe, fused_end))
}

fn advance_parser_to<'r, 't>(
    parser: &mut Parser<'r, 't>,
    target: &Parser<'r, 't>,
) -> Result<(), ParseError> {
    let end = target.current().span.start;
    while parser.current().span.start < end {
        parser.step()?;
    }
    Ok(())
}

enum TabTitleFootnote<'r, 't> {
    NotFootnote,
    Break,
    Unit(Parser<'r, 't>, bool),
    Elided(Parser<'r, 't>, bool),
}

fn tab_title_footnote_construct<'r, 't>(
    parser: &Parser<'r, 't>,
    title_has_content: bool,
) -> TabTitleFootnote<'r, 't>
where
    'r: 't,
{
    let layout = parser.settings().layout;
    let tokens: Vec<&crate::parsing::ExtractedToken<'t>> =
        std::iter::once(parser.current())
            .chain(parser.remaining())
            .collect();
    match tab_title_token_block_name(&tokens, 0, layout) {
        Some("block-footnote") => {
            if !tab_title_footnote_closes(&tokens, layout) {
                return TabTitleFootnote::NotFootnote;
            }
            let footnote_count = parser.footnote_count();
            let mut probe = parser.clone();
            let consumed = match consume(&mut probe) {
                Ok(success) => {
                    success
                        .item
                        .as_ref()
                        .iter()
                        .any(|element| matches!(element, Element::Footnote(_)))
                        && probe.footnote_count() == footnote_count + 1
                }
                Err(_) => false,
            };
            if !consumed {
                probe.truncate_footnotes(footnote_count);
                return TabTitleFootnote::NotFootnote;
            }
            let end = probe.current().span.start;
            let crossed_break = std::iter::once(parser.current())
                .chain(parser.remaining())
                .take_while(|token| token.span.start < end)
                .any(|token| {
                    matches!(token.token, Token::LineBreak | Token::ParagraphBreak)
                });
            if crossed_break {
                probe.truncate_footnotes(footnote_count);
                return TabTitleFootnote::NotFootnote;
            }
            if !title_has_content {
                probe.truncate_footnotes(footnote_count);
                return TabTitleFootnote::Break;
            }
            let fused_end = tab_title_unit_fused_end(parser, &probe);
            TabTitleFootnote::Unit(probe, fused_end)
        }
        Some("block-footnote-block") => {
            match tab_title_footnoteblock_probe(parser, &tokens) {
                Some(probe) => {
                    let fused_end = tab_title_unit_fused_end(parser, &probe);
                    TabTitleFootnote::Elided(probe, fused_end)
                }
                None => TabTitleFootnote::NotFootnote,
            }
        }
        _ => TabTitleFootnote::NotFootnote,
    }
}

fn tab_title_footnoteblock_probe<'r, 't>(
    parser: &Parser<'r, 't>,
    tokens: &[&crate::parsing::ExtractedToken<'t>],
) -> Option<Parser<'r, 't>>
where
    'r: 't,
{
    let mut close = None;
    for token in tokens.iter().skip(1) {
        match token.token {
            Token::RightBlock | Token::RightLink => {
                close = Some(*token);
                break;
            }
            Token::LeftBlock
            | Token::LeftBlockEnd
            | Token::LineBreak
            | Token::ParagraphBreak
            | Token::InputEnd => {
                return None;
            }
            _ => {}
        }
    }
    let close = close?;
    let start = parser.current().span.start;
    let slice = &parser.full_text().inner()[start..close.span.start + 2];
    let tokenization = crate::tokenize(slice);
    let outcome =
        crate::parsing::parse(&tokenization, parser.page_info(), parser.settings());
    let (tree, _) = outcome.into();
    if !tab_title_single_footnoteblock(&tree.elements) {
        return None;
    }
    let mut probe = parser.clone();
    while probe.current().span.start < close.span.end {
        probe.step().ok()?;
    }
    Some(probe)
}

fn tab_title_single_footnoteblock(elements: &[Element]) -> bool {
    match elements {
        [Element::Container(container)]
            if container.ctype() == crate::tree::ContainerType::Paragraph =>
        {
            tab_title_single_footnoteblock(container.elements())
        }
        [Element::FootnoteBlock { .. }] => true,
        _ => false,
    }
}

fn tab_title_remainder_empty<'r, 't>(parser: &Parser<'r, 't>) -> bool
where
    'r: 't,
{
    let mut probe = parser.clone();
    loop {
        match probe.current().token {
            Token::RightBlock | Token::RightLink => return true,
            Token::LeftComment => {
                if consume_valid_comment(&mut probe).is_err() {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

fn tab_title_footnote_closes(
    tokens: &[&crate::parsing::ExtractedToken],
    layout: crate::layout::Layout,
) -> bool {
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.token {
            Token::LeftBlock => match tab_title_token_block_name(tokens, index, layout) {
                Some("block-footnote") => depth += 1,
                Some("block-tab") | Some("block-tabview") => return false,
                _ => {}
            },
            Token::LeftBlockEnd => {
                match tab_title_token_block_name(tokens, index, layout) {
                    Some("block-footnote") => {
                        if depth <= 1 {
                            return true;
                        }
                        depth -= 1;
                    }
                    Some("block-tab") | Some("block-tabview") => return false,
                    _ => {}
                }
            }
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                return false;
            }
            _ => {}
        }
    }
    false
}

fn collect_tab_label<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<(std::borrow::Cow<'t, str>, Option<&'t str>), ParseError>
where
    'r: 't,
{
    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let mut copied_until = start;
    let mut output = None::<String>;
    let mut saw_unit = false;
    let mut link_unit_end = None::<usize>;
    let mut elided_footnoteblock = false;

    loop {
        let current = parser.current();
        let pending_link_end = link_unit_end.take();
        match current.token {
            Token::GeneratedPageLink | Token::GeneratedTagLinks => {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            Token::InputEnd => {
                return Err(parser.make_err(ParseErrorKind::EndOfInput));
            }
            Token::LineBreak | Token::ParagraphBreak => {
                return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
            }
            Token::RightBlock => {
                let text = tab_label_finish(
                    source,
                    start,
                    output,
                    copied_until,
                    current.span.start,
                    saw_unit,
                );
                if elided_footnoteblock && !saw_unit && text.is_empty() {
                    return Err(parser.make_err(ParseErrorKind::RuleFailed));
                }
                parser.step()?;
                return Ok((text, None));
            }
            Token::RightLink => {
                let text = tab_label_finish(
                    source,
                    start,
                    output,
                    copied_until,
                    current.span.start,
                    saw_unit,
                );
                if elided_footnoteblock && !saw_unit && text.is_empty() {
                    return Err(parser.make_err(ParseErrorKind::RuleFailed));
                }
                let leftover = &source[current.span.start + 2..current.span.end];
                parser.step()?;
                return Ok((text, Some(leftover)));
            }
            Token::LeftComment => {
                let range = consume_valid_comment(parser)?;
                let output = output.get_or_insert_with(String::new);
                output.push_str(&source[copied_until..range.start]);
                copied_until = range.end;
            }
            Token::LeftBlock => {
                let title_has_content = saw_unit
                    || output
                        .as_deref()
                        .is_some_and(|text| text.chars().any(|c| !c.is_whitespace()))
                    || source[copied_until..current.span.start]
                        .chars()
                        .any(|c| !c.is_whitespace());
                match tab_title_footnote_construct(parser, title_has_content) {
                    TabTitleFootnote::Break => {
                        return Err(parser.make_err(ParseErrorKind::RuleFailed));
                    }
                    TabTitleFootnote::Unit(unit, fused_end) => {
                        saw_unit = true;
                        output = Some(String::new());
                        advance_parser_to(parser, &unit)?;
                        copied_until = parser.current().span.start;
                        if fused_end {
                            parser.step()?;
                            return Ok((std::borrow::Cow::Borrowed(""), None));
                        }
                        continue;
                    }
                    TabTitleFootnote::Elided(elided, fused_end) => {
                        let title_so_far_empty = !saw_unit
                            && output.as_deref().is_none_or(|text| text.is_empty())
                            && copied_until == start
                            && current.span.start == start;
                        if title_so_far_empty
                            && (fused_end || tab_title_remainder_empty(&elided))
                        {
                            return Err(parser.make_err(ParseErrorKind::RuleFailed));
                        }
                        let construct_start = current.span.start;
                        advance_parser_to(parser, &elided)?;
                        let out_text = output.get_or_insert_with(String::new);
                        out_text.push_str(&source[copied_until..construct_start]);
                        copied_until = parser.current().span.start;
                        if title_so_far_empty {
                            elided_footnoteblock = true;
                        }
                        if fused_end {
                            let text = tab_label_finish(
                                source,
                                start,
                                output,
                                copied_until,
                                parser.current().span.start,
                                saw_unit,
                            );
                            if elided_footnoteblock && !saw_unit && text.is_empty() {
                                return Err(parser.make_err(ParseErrorKind::RuleFailed));
                            }
                            parser.step()?;
                            return Ok((text, None));
                        }
                        continue;
                    }
                    TabTitleFootnote::NotFootnote => {}
                }
                if iftags_segment_fused(parser).is_some() {
                    let segment = consume_iftags_segment_fused(parser)?
                        .expect("segment presence was just checked");
                    {
                        let output = output.get_or_insert_with(String::new);
                        output.push_str(&source[copied_until..segment.range.start]);
                        if segment.include_body {
                            output.push_str(&source[segment.body.clone()]);
                        }
                    }
                    copied_until = segment.range.end;
                    if segment.fused_closer {
                        let next = parser.current();
                        if next.token == Token::RightBracket
                            && next.span.start == segment.range.end + 1
                        {
                            let text = tab_label_finish(
                                source,
                                start,
                                output,
                                copied_until,
                                segment.range.end,
                                saw_unit,
                            );
                            parser.step()?;
                            return Ok((text, None));
                        }
                    }
                    continue;
                }
                if let Some((unit, fused_end)) = tab_title_block_unit(parser) {
                    saw_unit = true;
                    output = Some(String::new());
                    parser.update(&unit);
                    copied_until = parser.current().span.start;
                    if fused_end {
                        parser.step()?;
                        return Ok((std::borrow::Cow::Borrowed(""), None));
                    }
                    continue;
                }
                return finish_tab_label_at_construct(
                    parser,
                    source,
                    start,
                    output,
                    copied_until,
                    saw_unit,
                );
            }
            Token::RightBracket => {
                if pending_link_end == Some(current.span.start)
                    && let Some(next) = parser.remaining().first()
                    && next.token == Token::RightBracket
                    && next.span.start == current.span.end
                {
                    parser.step()?;
                    parser.step()?;
                    return Ok((std::borrow::Cow::Borrowed(""), None));
                }
                parser.step()?;
            }
            Token::LeftLink => {
                if let Some((unit, _fused_end)) = tab_title_link_unit(parser) {
                    saw_unit = true;
                    output = Some(String::new());
                    parser.update(&unit);
                    copied_until = parser.current().span.start;
                    link_unit_end = Some(copied_until);
                    continue;
                }
                return finish_tab_label_at_construct(
                    parser,
                    source,
                    start,
                    output,
                    copied_until,
                    saw_unit,
                );
            }
            _ => {
                parser.step()?;
            }
        }
    }
}

fn finish_tab_label_at_construct<'r, 't>(
    parser: &mut Parser<'r, 't>,
    source: &'t str,
    start: usize,
    output: Option<String>,
    copied_until: usize,
    saw_unit: bool,
) -> Result<(std::borrow::Cow<'t, str>, Option<&'t str>), ParseError> {
    let tokens: Vec<&crate::parsing::ExtractedToken<'t>> =
        std::iter::once(parser.current())
            .chain(parser.remaining())
            .collect();
    for token in &tokens {
        match token.token {
            Token::RightBlock => {
                let text = tab_label_finish(
                    source,
                    start,
                    output,
                    copied_until,
                    token.span.start,
                    saw_unit,
                );
                while parser.current().span.start < token.span.end {
                    parser.step()?;
                }
                return Ok((text, None));
            }
            Token::RightLink => {
                let text = tab_label_finish(
                    source,
                    start,
                    output,
                    copied_until,
                    token.span.start,
                    saw_unit,
                );
                let leftover = &source[token.span.start + 2..token.span.end];
                while parser.current().span.start < token.span.end {
                    parser.step()?;
                }
                return Ok((text, Some(leftover)));
            }
            Token::LineBreak | Token::ParagraphBreak => {
                return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
            }
            Token::InputEnd => {
                return Err(parser.make_err(ParseErrorKind::EndOfInput));
            }
            _ => {}
        }
    }
    Err(parser.make_err(ParseErrorKind::EndOfInput))
}

fn parse_tab<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing tab block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Tab doesn't allow star flag");
    assert!(!flag_score, "Tab doesn't allow score flag");
    assert_block_name(&BLOCK_TAB, name);

    let (label, leftover) = if in_head {
        let (label, leftover) = collect_tab_label(parser)?;
        if leftover.is_none() && parser.current().token == Token::LineBreak {
            parser.step()?;
        }
        (Some(label), leftover)
    } else {
        (None, None)
    };

    let label = if parser.settings().layout.legacy() {
        label.unwrap_or(std::borrow::Cow::Borrowed("untitled"))
    } else {
        match label {
            Some(value) => value,
            None => {
                return Err(parser.make_err(ParseErrorKind::BlockMissingArguments));
            }
        }
    };

    let (mut elements, errors, _) = parser.get_body_elements(&BLOCK_TAB, true)?.into();
    if let Some(leftover) = leftover {
        elements.insert(0, Element::Text(std::borrow::Cow::Borrowed(leftover)));
    }

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
