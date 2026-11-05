/*
 * parsing/rule/impls/color.rs
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

use super::inline_delimiter::{ReachableCloseMode, validate_reachable_close};
use super::prelude::*;
use crate::parsing::ParseSuccess;
use crate::tree::Container;
use std::borrow::Cow;

#[derive(Debug, Clone)]
struct ColorSpec<'t> {
    color: Cow<'t, str>,
    background: bool,
}

pub const RULE_COLOR: Rule = Rule {
    name: "color",
    position: LineRequirement::Any,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Trying to create color container");
    let starts_line = parser.start_of_line();
    let head_start = parser.current().span.start;
    assert_step(parser, Token::Color)?;

    let legacy = parser.settings().layout.legacy();
    let color_close = if legacy {
        &[
            ParseCondition::current(Token::Pipe),
            ParseCondition::current(Token::TableColumn),
            ParseCondition::current(Token::TableColumnTitle),
            ParseCondition::current(Token::TableColumnCenter),
            ParseCondition::current(Token::TableColumnRight),
        ][..]
    } else {
        &[ParseCondition::current(Token::Pipe)][..]
    };
    let color_invalid = [
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::current(Token::LineBreak),
    ];
    let (first_field, first_terminator) =
        collect_text_resolved(parser, RULE_COLOR, color_close, &color_invalid, None)?;
    let body_pipe_prefix = (legacy && is_table_column_token(first_terminator.token))
        .then(|| text!(&first_terminator.slice[1..]));
    let (color, background) = (first_field, false);
    let color: Cow<'t, str> = match &color {
        Cow::Borrowed(text) => {
            let text: &'t str = text;
            if legacy {
                normalize_wikidot_color(text)
                    .ok_or_else(|| parser.make_err(ParseErrorKind::RuleFailed))?
            } else {
                normalize_color(text)
            }
        }
        Cow::Owned(_) => {
            let normalized = if legacy {
                normalize_wikidot_color(&color)
                    .ok_or_else(|| parser.make_err(ParseErrorKind::RuleFailed))?
            } else {
                normalize_color(&color)
            };
            Cow::Owned(normalized.into_owned())
        }
    };
    let spec = ColorSpec { color, background };

    trace!(
        "Retrieved color descriptor, now building container ('{}')",
        spec.color
    );

    if legacy {
        let mut crossed = parser.clone();
        let leading_space = if crossed.current().token == Token::Whitespace {
            let whitespace = crossed.current();
            crossed.step()?;
            has_crossed_bold_close(&crossed)
                .then(|| text!(crossed.full_text().slice(whitespace, whitespace)))
        } else {
            None
        };
        if has_crossed_bold_close(parser) {
            return collect_crossed_bold_color(parser, spec, None);
        }
        if let Some(leading_space) = leading_space {
            parser.update(&crossed);
            return collect_crossed_bold_color(
                parser,
                spec,
                (!starts_line).then_some(leading_space),
            );
        }
    }

    let table_owned = legacy && parser.in_wikidot_simple_table_cell();
    if !table_owned {
        validate_reachable_close(
            parser,
            Token::Color,
            None,
            ReachableCloseMode::BlockBarrier,
        )?;
    }
    let close = if table_owned {
        &[
            ParseCondition::current(Token::Color),
            ParseCondition::current(Token::TableColumn),
            ParseCondition::current(Token::TableColumnTitle),
            ParseCondition::current(Token::TableColumnCenter),
            ParseCondition::current(Token::TableColumnRight),
        ][..]
    } else {
        &[ParseCondition::current(Token::Color)][..]
    };
    let invalid = [ParseCondition::current(Token::ParagraphBreak)];
    let body_start = parser.current().span.start;
    let body = if table_owned {
        collect_consume_before(parser, RULE_COLOR, close, &invalid, None)?
    } else {
        collect_consume_keep(parser, RULE_COLOR, close, &invalid, None)?
    };
    let ((mut elements, terminator), errors, paragraph_safe) = body.into();
    let unformed_dash = legacy
        && !table_owned
        && terminator.token == Token::Color
        && unform_strike_ate_close(&mut elements);
    if let Some(prefix) = body_pipe_prefix {
        elements.insert(0, prefix);
    }
    if terminator.token == Token::Color && table_owned {
        parser.step()?;
    } else if is_table_column_token(terminator.token) {
        parser.mark_wikidot_simple_table_crossed_closer(Token::Color);
    }

    let split_tail = if legacy && !table_owned && terminator.token == Token::Color {
        split_swallowed_mono_close(&mut elements)
    } else {
        None
    };

    let body_was_empty = elements.is_empty();
    let leading_space = if unformed_dash {
        None
    } else {
        take_edge_space(&mut elements, true)
    };
    let trailing_space = take_edge_space(&mut elements, false);
    if legacy
        && elements.iter().all(
            |element| matches!(element, Element::Text(text) if text.trim().is_empty()),
        )
    {
        elements.clear();
    }
    if legacy && body_was_empty {
        let body_had_tokens = terminator.span.start != body_start;
        if body_had_tokens && !table_owned && terminator.token == Token::Color {
            return ok!(paragraph_safe; Elements::None, errors);
        }
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let spaces_vanish = legacy && !body_was_empty && elements.is_empty();
    let space_survives = spaces_vanish
        && !starts_line
        && terminator.token == Token::Color
        && ws_color_space_survives(
            parser.full_text().inner(),
            head_start,
            terminator.span.end,
            table_owned,
        );
    let trailing_break_space = legacy
        && !spaces_vanish
        && terminator.token == Token::Color
        && split_tail.is_none()
        && matches!(elements.last(), Some(Element::LineBreak))
        && trailing_break_space_survives(
            parser.full_text().inner(),
            terminator.span.end,
            table_owned,
        );
    let leading_run_eaten = legacy
        && !spaces_vanish
        && !starts_line
        && leading_space.is_none()
        && !matches!(
            elements.first(),
            Some(Element::Text(text)) if text.starts_with([' ', '\t'])
        )
        && parser
            .full_text()
            .inner()
            .as_bytes()
            .get(first_terminator.span.end)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'));
    let element = color_element(spec, elements);
    let mut output = Vec::with_capacity(3);
    if !starts_line && !spaces_vanish {
        if let Some(space) = leading_space {
            output.push(space);
        } else if leading_run_eaten {
            output.push(text!(" "));
        }
    }
    if space_survives {
        output.push(text!(" "));
    }
    output.push(element);
    if !spaces_vanish {
        if let Some(space) = trailing_space {
            output.push(space);
        } else if trailing_break_space {
            output.push(text!(" "));
        }
    }
    if let Some(tail) = split_tail {
        output.extend(tail);
        output.push(text!("##"));
    }
    if unformed_dash {
        output.push(text!("--"));
        output.push(text!("##"));
    }

    ok!(paragraph_safe; Elements::Multiple(output), errors)
}

fn ws_color_space_survives(
    source: &str,
    head_start: usize,
    close_end: usize,
    table_owned: bool,
) -> bool {
    let bytes = source.as_bytes();
    if head_start == 0 || close_end >= bytes.len() {
        return false;
    }
    let prev = bytes[head_start - 1];
    if matches!(prev, b' ' | b'\t' | b'\n' | b'\r') {
        return false;
    }
    let next = bytes[close_end];
    if matches!(next, b' ' | b'\t' | b'\n' | b'\r') {
        return false;
    }
    if source[close_end..].starts_with("[[/") {
        return false;
    }
    if table_owned && next == b'|' {
        return false;
    }
    true
}

fn trailing_break_space_survives(
    source: &str,
    close_end: usize,
    table_owned: bool,
) -> bool {
    let bytes = source.as_bytes();
    if close_end >= bytes.len() {
        return false;
    }
    let next = bytes[close_end];
    if matches!(next, b' ' | b'\t' | b'\n' | b'\r') {
        return false;
    }
    if table_owned && next == b'|' {
        return false;
    }
    true
}

fn is_table_column_token(token: Token) -> bool {
    matches!(
        token,
        Token::TableColumn
            | Token::TableColumnTitle
            | Token::TableColumnCenter
            | Token::TableColumnRight
    )
}

fn split_swallowed_mono_close<'t>(
    elements: &mut Vec<Element<'t>>,
) -> Option<Vec<Element<'t>>> {
    let mono_index = elements.iter().position(|element| {
        matches!(
            element,
            Element::Container(container)
                if container.ctype() == ContainerType::Monospace
                    && container.attributes().get().is_empty()
                    && container.elements().iter().any(|child| matches!(
                        child,
                        Element::Text(text) if text.contains("##")
                    ))
        )
    })?;
    let Element::Container(mono) = elements.remove(mono_index) else {
        unreachable!("split search requires a monospace container");
    };
    let children: Vec<Element<'t>> = mono.into();
    let hash_at = children
        .iter()
        .position(|child| matches!(child, Element::Text(text) if text.contains("##")))?;
    let Element::Text(hash_text) = &children[hash_at] else {
        unreachable!("split search requires hash text");
    };
    let hash_offset = hash_text.find("##")?;
    let (before, after) = hash_text.split_at(hash_offset);
    let after = &after["##".len()..];
    let mut pre: Vec<Element<'t>> = children[..hash_at].to_vec();
    if !before.is_empty() {
        pre.push(Element::Text(Cow::Owned(before.to_owned())));
    }
    let mut post: Vec<Element<'t>> = Vec::new();
    if !after.is_empty() {
        post.push(Element::Text(Cow::Owned(after.to_owned())));
    }
    post.extend(children[hash_at + 1..].iter().cloned());
    elements.insert(
        mono_index,
        Element::Container(Container::new(
            ContainerType::Monospace,
            pre,
            AttributeMap::new(),
        )),
    );
    let mut tail: Vec<Element<'t>> = Vec::new();
    if !post.is_empty() {
        tail.push(Element::Container(Container::new(
            ContainerType::Monospace,
            post,
            AttributeMap::new(),
        )));
    }
    tail.extend(elements.drain(mono_index + 1..));
    Some(tail)
}

fn unform_strike_ate_close<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let mut path: Vec<usize> = Vec::new();
    let mut here: &[Element<'t>] = elements;
    loop {
        let Some(last) = here.last() else {
            return false;
        };
        match last {
            Element::Container(container)
                if container.ctype() == ContainerType::Strikethrough
                    && container.attributes().get().is_empty() =>
            {
                let ate = matches!(
                    container.elements().last(),
                    Some(Element::Text(text)) if text.as_ref() == "##",
                );
                if !ate {
                    return false;
                }
                break;
            }
            Element::Container(container)
                if container.attributes().get().is_empty()
                    && matches!(
                        container.ctype(),
                        ContainerType::Bold
                            | ContainerType::Italics
                            | ContainerType::Underline
                            | ContainerType::Subscript
                            | ContainerType::Superscript
                            | ContainerType::Monospace,
                    ) =>
            {
                path.push(here.len() - 1);
                here = container.elements();
            }
            _ => return false,
        }
    }
    let mut slot: &mut Vec<Element<'t>> = elements;
    for index in path {
        let Element::Container(container) = &mut slot[index] else {
            return false;
        };
        slot = container.elements_mut();
    }
    let Some(Element::Container(strike)) = slot.pop() else {
        return false;
    };
    let mut children: Vec<Element<'t>> = strike.into();
    children.pop();
    slot.push(text!("--"));
    slot.extend(children);
    true
}

fn take_edge_space<'t>(
    elements: &mut Vec<Element<'t>>,
    leading: bool,
) -> Option<Element<'t>> {
    let index = if leading {
        0
    } else {
        elements.len().checked_sub(1)?
    };
    if matches!(elements.get(index), Some(Element::Text(text)) if text.as_ref() == " ") {
        Some(elements.remove(index))
    } else {
        None
    }
}

fn color_element<'t>(spec: ColorSpec<'t>, elements: Vec<Element<'t>>) -> Element<'t> {
    Element::Color {
        color: spec.color,
        background: spec.background,
        elements,
    }
}

fn has_crossed_bold_close(parser: &Parser<'_, '_>) -> bool {
    if parser.current().token != Token::Bold {
        return false;
    }

    let tokens = parser.remaining();
    let Some(color_close) = tokens.iter().position(|token| token.token == Token::Color)
    else {
        return false;
    };
    if tokens[..color_close]
        .iter()
        .any(|token| matches!(token.token, Token::Bold | Token::ParagraphBreak))
    {
        return false;
    }

    for token in &tokens[color_close + 1..] {
        match token.token {
            Token::Bold => return true,
            Token::ParagraphBreak | Token::InputEnd => return false,
            _ => {}
        }
    }
    false
}

fn collect_crossed_bold_color<'r, 't>(
    parser: &mut Parser<'r, 't>,
    spec: ColorSpec<'t>,
    leading_space: Option<Element<'t>>,
) -> ParseResult<'r, 't, Elements<'t>> {
    assert_step(parser, Token::Bold)?;

    let invalid = [ParseCondition::current(Token::ParagraphBreak)];
    let colored = collect_consume(
        parser,
        RULE_COLOR,
        &[ParseCondition::current(Token::Color)],
        &invalid,
        None,
    )?;
    let trailing = collect_consume(
        parser,
        RULE_COLOR,
        &[ParseCondition::current(Token::Bold)],
        &invalid,
        None,
    )?;
    let (colored, mut errors, colored_safe) = colored.into();
    let (trailing, mut trailing_errors, trailing_safe) = trailing.into();
    errors.append(&mut trailing_errors);

    let colored = Element::Container(Container::new(
        ContainerType::Bold,
        colored,
        AttributeMap::new(),
    ));
    let color = color_element(spec, vec![colored]);
    let mut elements = Vec::with_capacity(3);
    if let Some(leading_space) = leading_space {
        elements.push(leading_space);
    }
    elements.push(color);
    if !trailing.is_empty() {
        let trailing = Element::Container(Container::new(
            ContainerType::Bold,
            trailing,
            AttributeMap::new(),
        ));
        elements.push(trailing);
    }

    Ok(ParseSuccess::new(
        elements.into(),
        errors,
        colored_safe && trailing_safe,
    ))
}

pub(crate) fn normalize_color(color: &str) -> Cow<'_, str> {
    if !is_safe_color(color) {
        return Cow::Borrowed("inherit");
    }

    let hex = color.strip_prefix('#').unwrap_or(color);
    if matches!(hex.len(), 3 | 6) && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Cow::Owned(format!("#{}", hex.to_ascii_lowercase()))
    } else {
        Cow::Borrowed(color)
    }
}

fn is_safe_color(color: &str) -> bool {
    color.chars().all(|ch| {
        !ch.is_control()
            && !matches!(ch, ';' | '{' | '}' | '<' | '>' | '"' | '\'' | '\\' | '&')
    })
}

pub(crate) fn normalize_wikidot_color(color: &str) -> Option<Cow<'_, str>> {
    if color.is_empty() {
        return None;
    }

    let mut collapsed = String::with_capacity(color.len());
    for part in color.split([' ', '\t']) {
        if part.is_empty() {
            continue;
        }
        if !collapsed.is_empty() {
            collapsed.push(' ');
        }
        collapsed.push_str(part);
    }
    let collapsed = collapsed.to_ascii_lowercase();

    let hex = collapsed.strip_prefix('#').unwrap_or(&collapsed);
    if matches!(hex.len(), 3 | 6) && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Some(Cow::Owned(format!("#{hex}")));
    }

    collapsed
        .bytes()
        .all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'#' | b'(' | b')' | b',' | b'-' | b' ')
        })
        .then_some(Cow::Owned(collapsed))
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
    fn color_normalization_rejects_css_declaration_breakout() {
        assert_eq!(normalize_color("abc"), "#abc");
        assert_eq!(normalize_color("ABC"), "#abc");
        assert_eq!(normalize_color("#B01"), "#b01");
        assert_eq!(normalize_color("").as_ref(), "");
        assert_eq!(normalize_color("red").as_ref(), "red");
        assert_eq!(
            normalize_color("rgb(10, 12, 14)").as_ref(),
            "rgb(10, 12, 14)"
        );
        assert_eq!(
            normalize_color("red;background:url(//x)").as_ref(),
            "inherit"
        );
        assert_eq!(
            normalize_color("red\nbackground:url(//x)").as_ref(),
            "inherit"
        );
        assert_eq!(
            normalize_color("red&#59background:url(//x)").as_ref(),
            "inherit"
        );
        assert_eq!(
            normalize_color("red&#x3bbackground:url(//x)").as_ref(),
            "inherit"
        );
    }

    #[test]
    fn wikidot_crossed_bold_color_closes_and_reopens_bold() {
        let (html, errors) = render_wikidot("##orange|**ORANGE##-PRIME:**");

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            "<p><span style=\"color: orange\"><strong>ORANGE</strong></span><strong>-PRIME:</strong></p>",
        );
    }

    #[test]
    fn ordinary_bold_inside_color_is_unchanged() {
        let (html, errors) = render_wikidot("##orange|before **bold** after##");

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            "<p><span style=\"color: orange\">before <strong>bold</strong> after</span></p>",
        );
    }

    #[test]
    fn wikidot_color_value_boundary() {
        for (input, expected) in [
            ("var(--c-keyword)", "var(--c-keyword)"),
            ("var(--missing, red)", "var(--missing, red)"),
            ("var(--a, var(--b, blue))", "var(--a, var(--b, blue))"),
            ("var(--x)", "var(--x)"),
            ("var(x)", "var(x)"),
            ("var(--)", "var(--)"),
            ("var(--x", "var(--x"),
            ("var(--x))", "var(--x))"),
            ("var()", "var()"),
            ("rgb(var(--x))", "rgb(var(--x))"),
            ("not-a-color", "not-a-color"),
            ("rgb(1, 2)", "rgb(1, 2)"),
            ("expression(alert(1))", "expression(alert(1))"),
            ("DarkRed", "darkred"),
            ("FF0000", "#ff0000"),
            ("#FFFF", "#ffff"),
            ("c00", "#c00"),
            ("rgb(255,  0,  0)", "rgb(255, 0, 0)"),
            (" ", ""),
        ] {
            assert_eq!(
                normalize_wikidot_color(input).as_deref(),
                Some(expected),
                "{input:?}",
            );
        }

        for rejected in [
            "",
            "var(--x);background:red",
            "var(--x\"y)",
            "hsl(120, 50%, 25%)",
            "rgba(10,20,30,.5)",
            "a_b",
            "url(javascript:alert(1))",
        ] {
            assert_eq!(normalize_wikidot_color(rejected), None, "{rejected:?}");
        }
    }

    #[test]
    fn wikidot_var_color_renders_as_span() {
        let (html, errors) = render_wikidot("##var(--c-keyword)|text##");

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            "<p><span style=\"color: var(--c-keyword)\">text</span></p>",
        );
    }

    #[test]
    fn wikidot_rgb_var_color_renders_as_span() {
        let (html, errors) = render_wikidot("##rgb(var(--x))|text##");

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            "<p><span style=\"color: rgb(var(--x))\">text</span></p>",
        );
    }

    #[test]
    fn wikidot_space_only_color_renders_empty_style() {
        let (html, errors) = render_wikidot("## |gold|A##");

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html, "<p><span style=\"color:\">gold|A</span></p>");
    }
}
