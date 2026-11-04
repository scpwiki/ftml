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
use cssparser::{Parser as CssParser, ParserInput};
use cssparser_color::Color;
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
    let mut body_pipe_prefix = (legacy && is_table_column_token(first_terminator.token))
        .then(|| text!(&first_terminator.slice[1..]));
    let (color, background) = if legacy && trim_field(&first_field, legacy).is_empty() {
        let (background, terminator) =
            collect_text_resolved(parser, RULE_COLOR, color_close, &color_invalid, None)?;
        body_pipe_prefix = is_table_column_token(terminator.token)
            .then(|| text!(&terminator.slice[1..]));
        (trim_field_cow(background, legacy), true)
    } else if legacy {
        (trim_field_cow(first_field, legacy), false)
    } else {
        (first_field, false)
    };
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
    let body = if table_owned {
        collect_consume_before(parser, RULE_COLOR, close, &invalid, None)?
    } else {
        collect_consume_keep(parser, RULE_COLOR, close, &invalid, None)?
    };
    let ((mut elements, terminator), errors, paragraph_safe) = body.into();
    if let Some(prefix) = body_pipe_prefix {
        elements.insert(0, prefix);
    }
    if terminator.token == Token::Color && table_owned {
        parser.step()?;
    } else if is_table_column_token(terminator.token) {
        parser.mark_wikidot_simple_table_crossed_closer(Token::Color);
    }

    let leading_space = take_edge_space(&mut elements, true);
    let trailing_space = take_edge_space(&mut elements, false);
    if legacy && elements.is_empty() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let element = color_element(spec, elements);
    let mut output = Vec::with_capacity(3);
    if !starts_line && let Some(space) = leading_space {
        output.push(space);
    }
    output.push(element);
    if let Some(space) = trailing_space {
        output.push(space);
    }

    ok!(paragraph_safe; Elements::Multiple(output), errors)
}

fn trim_field(value: &str, legacy: bool) -> &str {
    if legacy {
        value.trim_matches([' ', '\t'])
    } else {
        value.trim()
    }
}

fn trim_field_cow<'t>(field: Cow<'t, str>, legacy: bool) -> Cow<'t, str> {
    match field {
        Cow::Borrowed(value) => Cow::Borrowed(trim_field(value, legacy)),
        Cow::Owned(value) => {
            let trimmed = trim_field(&value, legacy);
            if trimmed.len() == value.len() {
                Cow::Owned(value)
            } else {
                Cow::Owned(trimmed.to_owned())
            }
        }
    }
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
    let color = color.trim_matches([' ', '\t']);
    if color.is_empty() {
        return None;
    }
    let (hex, has_hash) = color
        .strip_prefix('#')
        .map_or((color, false), |hex| (hex, true));
    if matches!(hex.len(), 3 | 6) && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
        || has_hash
            && matches!(hex.len(), 4 | 8)
            && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Some(Cow::Owned(format!("#{}", hex.to_ascii_lowercase())));
    }

    // Wikidot passes var() references through as the color value: live
    // pages (e.g. component:croqstyle's documentation) use
    // ##var(--custom-property)|...## for syntax highlighting, and the
    // baseline renders them as colored spans. cssparser's Color::parse
    // cannot accept var(), so validate the reference shape directly.
    if is_css_var_reference(color) {
        return Some(Cow::Borrowed(color));
    }

    let mut input = ParserInput::new(color);
    let mut parser = CssParser::new(&mut input);
    let parsed: Result<_, cssparser::ParseError<'_, ()>> =
        parser.parse_entirely(Color::parse);
    parsed.ok().map(|_| Cow::Borrowed(color))
}

fn is_css_var_reference(color: &str) -> bool {
    let Some(inner) = color
        .strip_prefix("var(")
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return false;
    };

    if !inner.chars().all(|ch| {
        !ch.is_control()
            && !matches!(ch, ';' | '{' | '}' | '<' | '>' | '"' | '\'' | '\\' | '&')
    }) {
        return false;
    }

    let mut depth = 0usize;
    for ch in inner.chars() {
        match ch {
            '(' => depth += 1,
            ')' => {
                let Some(next) = depth.checked_sub(1) else {
                    return false;
                };
                depth = next;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return false;
    }

    let name = inner.split(',').next().unwrap_or_default().trim();
    let Some(ident) = name.strip_prefix("--") else {
        return false;
    };
    !ident.is_empty()
        && ident
            .chars()
            .all(|ch| ch.is_alphanumeric() || matches!(ch, '-' | '_'))
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
    fn wikidot_color_accepts_css_var_references() {
        for accepted in [
            "var(--c-keyword)",
            "var(--missing, red)",
            "var(--a, var(--b, blue))",
            "var(--x)",
        ] {
            assert_eq!(
                normalize_wikidot_color(accepted).as_deref(),
                Some(accepted),
                "{accepted:?}",
            );
        }

        for rejected in [
            "var(x)",
            "var(--)",
            "var(--x",
            "var(--x))",
            "var(--x);background:red",
            "var(--x\"y)",
            "var()",
        ] {
            assert_eq!(normalize_wikidot_color(rejected), None, "{rejected:?}",);
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
}
