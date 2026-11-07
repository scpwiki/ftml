/*
 * parsing/rule/impls/block/blocks/button.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2026 Wikijump Team
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

use super::super::parser::wikidot_trim_argument_fragment;
use super::prelude::*;
use crate::parsing::collect::{
    collect_text_resolved_head, consume_valid_comment, iftags_segment_fused,
};
use crate::parsing::discard_wikidot_controls;
use crate::parsing::rule::impls::block::parser::parse_wikidot_attributes;
use crate::tree::{
    AttributeMap, StandaloneButton, StandaloneButtonAction, TagAlteration,
    is_safe_standalone_button_style,
};
use std::borrow::Cow;

const MAX_BUTTON_HEAD_BYTES: usize = 1024 * 1024;

pub const BLOCK_BUTTON: BlockRule = BlockRule {
    name: "block-button",
    accepts_names: &["button"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: false,
    parse_fn,
};

enum ParsedButton<'t> {
    Active(StandaloneButton<'t>),
    Unknown(&'t str),
    MissingSetTagsText,
}

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing standalone button block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Button doesn't allow star flag");
    assert!(!flag_score, "Button doesn't allow score flag");
    assert_block_name(&BLOCK_BUTTON, name);

    parser.check_page_syntax()?;

    if !in_head {
        return Err(parser.make_err(ParseErrorKind::BlockMissingArguments));
    }

    let start = parser.current().span.start;
    let mut head_probe = parser.clone();
    let mut saw_seam = false;
    let mut fused_bracket_close = false;
    let mut has_generated_input = false;
    loop {
        let current = parser.current();
        if current.token == Token::LeftComment {
            let mut probe = parser.clone();
            if let Ok(range) = consume_valid_comment(&mut probe) {
                saw_seam = true;
                while parser.current().span.start < range.end
                    && parser.current().token != Token::InputEnd
                {
                    parser.step()?;
                }
                continue;
            }
        }
        if current.token == Token::LeftBlock
            && let Some(segment) = iftags_segment_fused(parser)
        {
            saw_seam = true;
            while parser.current().span.end <= segment.range.end
                && parser.current().token != Token::InputEnd
            {
                if matches!(
                    parser.current().token,
                    Token::RuntimeText
                        | Token::GeneratedPageLink
                        | Token::GeneratedTagLinks
                ) {
                    has_generated_input = true;
                }
                parser.step()?;
            }
            if segment.fused_closer {
                let completes = {
                    let current = parser.current();
                    current.span.start < segment.range.end
                        && current.span.end > segment.range.end
                        && matches!(parser.remaining().first(), Some(next) if next.token == Token::RightBracket && next.span.start == segment.range.end + 1)
                };
                if completes {
                    parser.step()?;
                    fused_bracket_close = true;
                }
            }
            continue;
        }
        let fused_bracket_close = std::mem::take(&mut fused_bracket_close)
            && current.token == Token::RightBracket;
        let trailing_bracket = if fused_bracket_close {
            false
        } else {
            match current.token {
                Token::RightBlock => false,
                Token::RightLink if current.slice == "]]]" => true,
                Token::RuntimeText
                | Token::GeneratedPageLink
                | Token::GeneratedTagLinks => {
                    has_generated_input = true;
                    parser.step()?;
                    continue;
                }
                Token::LineBreak | Token::ParagraphBreak => {
                    let head = &parser.full_text().inner()[start..current.span.start];
                    if parser.settings().layout.legacy() && head_prefix_is_set_tags(head)
                    {
                        parser.step()?;
                        continue;
                    }
                    return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
                }
                Token::InputEnd => {
                    return Err(parser.make_err(ParseErrorKind::EndOfInput));
                }
                _ => {
                    parser.step()?;
                    continue;
                }
            }
        };

        let head = &parser.full_text().inner()[start..current.span.start];
        if has_generated_input || head.len() > MAX_BUTTON_HEAD_BYTES {
            return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
        }

        let head = if saw_seam {
            collect_text_resolved_head(&mut head_probe, parser.rule(), current.span.start)
                .unwrap_or(Cow::Borrowed(head))
        } else {
            Cow::Borrowed(head)
        };

        let legacy = parser.settings().layout.legacy();
        let element = match head {
            Cow::Borrowed(head) => match parse_button_head(head, legacy) {
                Some(ParsedButton::Active(button)) => Element::StandaloneButton(button),
                Some(ParsedButton::Unknown(action)) => {
                    unknown_button_error(Cow::Borrowed(action))
                }
                Some(ParsedButton::MissingSetTagsText) => missing_set_tags_text_error(),
                None => {
                    return Err(parser.make_err(ParseErrorKind::BlockMissingArguments));
                }
            },
            Cow::Owned(head) => match parse_button_head(&head, legacy) {
                Some(ParsedButton::Active(button)) => {
                    Element::StandaloneButton(button.to_owned())
                }
                Some(ParsedButton::Unknown(action)) => {
                    unknown_button_error(Cow::Owned(action.to_owned()))
                }
                Some(ParsedButton::MissingSetTagsText) => missing_set_tags_text_error(),
                None => {
                    return Err(parser.make_err(ParseErrorKind::BlockMissingArguments));
                }
            },
        };
        parser.step()?;

        return if trailing_bracket {
            ok!(Elements::Multiple(vec![element, text!("]")]))
        } else {
            success_elements(element)
        };
    }
}

fn parse_button_head(head: &str, wikidot: bool) -> Option<ParsedButton<'_>> {
    let head = wikidot_trim_argument_fragment(head);
    if head.is_empty() {
        return None;
    }

    let action_end = if wikidot {
        head.find(is_wikidot_set_tags_separator)
    } else {
        head.find([' ', '\t'])
    }
    .unwrap_or(head.len());
    let action = &head[..action_end];
    let tail = wikidot_trim_argument_fragment(&head[action_end..]);

    if action.eq_ignore_ascii_case("set-tags") {
        return Some(parse_set_tags(tail, wikidot));
    }

    let mut arguments = parse_wikidot_attributes(tail);
    let label = nonempty(arguments.get("text").map(|value| {
        if wikidot {
            discard_wikidot_controls(value)
        } else {
            value
        }
    }))
    .unwrap_or_else(|| {
        Cow::Borrowed(if action.eq_ignore_ascii_case("source") {
            "view source"
        } else if action.eq_ignore_ascii_case("edit") {
            "edit"
        } else if action.eq_ignore_ascii_case("history") {
            "history"
        } else if action.eq_ignore_ascii_case("print") {
            "print"
        } else {
            ""
        })
    });
    let class = nonempty(arguments.get("class"));
    let style = nonempty(arguments.get("style"))
        .filter(|value| is_safe_standalone_button_style(value));

    let action = if action.eq_ignore_ascii_case("edit") {
        StandaloneButtonAction::Edit
    } else if action.eq_ignore_ascii_case("history") {
        StandaloneButtonAction::History
    } else if action.eq_ignore_ascii_case("source") {
        StandaloneButtonAction::Source
    } else if action.eq_ignore_ascii_case("print") {
        StandaloneButtonAction::Print
    } else {
        return Some(ParsedButton::Unknown(action));
    };

    Some(ParsedButton::Active(StandaloneButton {
        action,
        label,
        class,
        style,
    }))
}

fn parse_set_tags(mut tail: &str, wikidot: bool) -> ParsedButton<'_> {
    let mut alterations = Vec::new();
    loop {
        tail = if wikidot {
            tail.trim_start_matches(is_wikidot_set_tags_separator)
        } else {
            wikidot_trim_argument_fragment(tail)
        };
        let end = if wikidot {
            tail.find(is_wikidot_set_tags_separator)
        } else {
            tail.find([' ', '\t'])
        }
        .unwrap_or(tail.len());
        let token = &tail[..end];
        if wikidot && token.contains(['\'', '"', '=', '&', '<', '>']) {
            break;
        }
        let alteration = match token {
            "-*" => Some(TagAlteration::ClearVisible),
            "-_*" => Some(TagAlteration::ClearHidden),
            token if token.len() > 1 && token.starts_with('+') => {
                set_tags_value(token, wikidot).map(TagAlteration::Add)
            }
            token if token.len() > 1 && token.starts_with('-') => {
                set_tags_value(token, wikidot).map(TagAlteration::Remove)
            }
            _ => None,
        };
        let Some(alteration) = alteration else {
            break;
        };
        alterations.push(alteration);
        tail = &tail[end..];
    }

    let mut arguments = parse_wikidot_attributes(tail);
    let Some(label) = nonempty(arguments.get("text").map(|value| {
        if wikidot {
            discard_wikidot_controls(value)
        } else {
            value
        }
    })) else {
        return ParsedButton::MissingSetTagsText;
    };
    let class = nonempty(arguments.get("class"));
    let style = nonempty(arguments.get("style"))
        .filter(|value| is_safe_standalone_button_style(value));
    ParsedButton::Active(StandaloneButton {
        action: StandaloneButtonAction::SetTags(alterations),
        label,
        class,
        style,
    })
}

fn head_prefix_is_set_tags(head: &str) -> bool {
    let head = head.trim_start_matches(is_wikidot_set_tags_separator);
    let action_end = head
        .find(is_wikidot_set_tags_separator)
        .unwrap_or(head.len());
    head[..action_end].eq_ignore_ascii_case("set-tags")
}

fn is_wikidot_set_tags_separator(character: char) -> bool {
    matches!(
        character,
        ' ' | '\t' | '\r' | '\n' | '\u{000b}' | '\u{000c}'
    )
}

fn set_tags_value<'t>(token: &'t str, wikidot: bool) -> Option<Cow<'t, str>> {
    let value = &token[1..];
    if wikidot && value.contains('\0') {
        let value = value.replace('\0', "");
        (!value.is_empty()).then_some(Cow::Owned(value))
    } else {
        Some(Cow::Borrowed(value))
    }
}

fn nonempty(value: Option<Cow<'_, str>>) -> Option<Cow<'_, str>> {
    value.filter(|value| !value.is_empty())
}

fn unknown_button_error<'t>(action: Cow<'t, str>) -> Element<'t> {
    let emphasized = Element::Container(Container::new(
        ContainerType::Italics,
        vec![Element::Text(action)],
        AttributeMap::new(),
    ));
    error_block(vec![emphasized, text!(" is not a valid button type")])
}

fn missing_set_tags_text_error<'t>() -> Element<'t> {
    error_block(vec![text!("You need to set text for set-tags button.")])
}

fn error_block<'t>(elements: Vec<Element<'t>>) -> Element<'t> {
    let mut attributes = AttributeMap::new();
    attributes.insert("class", cow!("error-block"));
    Element::Container(Container::new(ContainerType::Div, elements, attributes))
}
