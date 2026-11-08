/*
 * parsing/rule/impls/monospace.rs
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

use super::inline_delimiter::{
    ReachableCloseMode, container_closes_at_line_start, validate_reachable_close,
};
use super::prelude::*;
use crate::parsing::inline_format::{element_contains_link, monospace_close_split};
use crate::tree::{Container, WIKIDOT_WS_MONO_VANISH_MARKER};
use std::borrow::Cow;

pub const RULE_MONOSPACE: Rule = Rule {
    name: "monospace",
    position: LineRequirement::Any,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Trying to create monospace container");
    validate_reachable_close(
        parser,
        Token::RightMonospace,
        None,
        ReachableCloseMode::BlockBarrier,
    )?;
    let opening_start = parser.current().span.start;
    assert_step(parser, Token::LeftMonospace)?;

    let mut leading_padding = false;
    if is_wikidot_edge_padding(parser.current()) {
        let is_padding_only = parser.next_two_tokens().1 == Some(Token::RightMonospace);
        let keep_for_dash = parser.settings().layout.legacy()
            && !is_padding_only
            && parser.next_two_tokens().1 == Some(Token::DoubleDash);
        if !keep_for_dash {
            assert_step(parser, Token::Whitespace)?;

            if is_padding_only {
                assert_step(parser, Token::RightMonospace)?;
                if parser.settings().layout.legacy()
                    && wikidot_padding_only_space_drops(parser)
                {
                    let mut attributes = AttributeMap::new();
                    attributes.insert(WIKIDOT_WS_MONO_VANISH_MARKER, cow!(""));
                    return success_elements(Element::Container(Container::new(
                        ContainerType::Monospace,
                        Vec::new(),
                        attributes,
                    )));
                }
                return success_elements(text!(" "));
            }
            leading_padding = true;
        }
    }

    let close = [ParseCondition::current(Token::RightMonospace)];
    let invalid = [ParseCondition::current(Token::ParagraphBreak)];
    let preserve_empty_lists = parser.settings().layout.legacy();
    let previous_preserve = if preserve_empty_lists {
        parser.set_wikidot_preserve_empty_lists(true)
    } else {
        false
    };
    let collected = collect_consume_keep(parser, RULE_MONOSPACE, &close, &invalid, None);
    if preserve_empty_lists {
        parser.set_wikidot_preserve_empty_lists(previous_preserve);
    }
    let collected = collected?;
    let ((mut elements, terminator), errors, paragraph_safe) = collected.into();

    if parser.settings().layout.legacy() && terminator.token == Token::RightMonospace {
        if let Some((shed_index, prefix, suffix, trailing_space)) =
            wikidot_shed_close(&elements)
        {
            let mut container_elements: Vec<Element<'t>> =
                elements.drain(..shed_index).collect();
            if !prefix.is_empty() {
                container_elements.push(Element::Text(Cow::Owned(prefix)));
            }
            let container = Element::Container(Container::new(
                ContainerType::Monospace,
                container_elements,
                AttributeMap::new(),
            ));
            let mut output = Vec::with_capacity(4);
            let leading_separator = leading_padding
                && parser
                    .full_text()
                    .inner()
                    .as_bytes()
                    .get(opening_start.wrapping_sub(1))
                    .is_some_and(|byte| !byte.is_ascii_whitespace());
            if leading_separator {
                output.push(text!(" "));
            }
            output.push(container);
            if !suffix.is_empty() {
                output.push(Element::Text(Cow::Owned(suffix)));
            }
            if trailing_space {
                output.push(text!(" "));
            }
            output.push(text!("}}"));
            return ok!(paragraph_safe; output, errors);
        }
    }

    let unformed_dash = parser.settings().layout.legacy()
        && terminator.token == Token::RightMonospace
        && unform_strike_ate_close(&mut elements);

    let mut trailing_padding = if parser.current().token == Token::RightMonospace {
        elements.push(text!("}}"));
        while parser.next_two_tokens()
            == (Token::RightMonospace, Some(Token::RightMonospace))
        {
            elements.push(text!("}}"));
            parser.step()?;
        }
        assert_step(parser, Token::RightMonospace)?;
        false
    } else if parser.current().token == Token::Other && parser.current().slice == "}" {
        elements.push(text!("}"));
        parser.step()?;
        false
    } else {
        let has_padding = parser
            .full_text()
            .inner()
            .as_bytes()
            .get(terminator.span.start.wrapping_sub(1))
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'));
        if has_padding
            && matches!(elements.last(), Some(Element::Text(value)) if value.as_ref() == " ")
        {
            elements.pop();
        }
        has_padding
    };

    if terminator.token == Token::RightMonospace
        && parser.current().token == Token::Other
        && parser.current().slice == "}"
    {
        elements.push(text!("}"));
        parser.step()?;
    }

    if parser.settings().layout.legacy() && terminator.token == Token::RightMonospace {
        if strip_seam_edge_spaces(&mut elements, true) {
            leading_padding = true;
        }
        if strip_seam_edge_spaces(&mut elements, false) {
            trailing_padding = true;
        }
    }

    if parser.settings().layout.legacy()
        && !leading_padding
        && !trailing_padding
        && (elements.is_empty()
            || matches!(elements.as_slice(), [Element::Text(value)] if value.as_ref() == "0"))
    {
        return ok!(paragraph_safe; Elements::None, errors);
    }

    if parser.settings().layout.legacy()
        && matches!(elements.as_slice(), [Element::Table(_)])
    {
        let opening = Element::Container(Container::new(
            ContainerType::Paragraph,
            vec![text!("{{")],
            AttributeMap::new(),
        ));
        let closing = Element::Container(Container::new(
            ContainerType::Paragraph,
            vec![text!("}}")],
            AttributeMap::new(),
        ));
        let table = elements.pop().expect("matched one table element");
        return ok!(false; vec![opening, table, closing], errors);
    }

    if parser.settings().layout.legacy()
        && super::inline_delimiter::code_preceded_by_content_in_elements(&elements)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let element = Element::Container(Container::new(
        ContainerType::Monospace,
        elements,
        AttributeMap::new(),
    ));
    let mut output = Vec::with_capacity(3);
    let leading_separator = leading_padding
        && parser
            .full_text()
            .inner()
            .as_bytes()
            .get(opening_start.wrapping_sub(1))
            .is_some_and(|byte| !byte.is_ascii_whitespace());
    if leading_separator {
        output.push(text!(" "));
    }
    output.push(element);
    if parser.settings().layout.legacy()
        && output.last().is_some_and(container_closes_at_line_start)
    {
        output.push(text!(" "));
    }
    let trailing_separator = trailing_padding
        && !matches!(
            parser.current().token,
            Token::Whitespace
                | Token::LineBreak
                | Token::ParagraphBreak
                | Token::InputEnd
        )
        && !(parser.current().token == Token::LeftMonospace
            && parser.look_ahead(0).is_some_and(is_wikidot_edge_padding));
    if trailing_separator {
        output.push(text!(" "));
    }
    if unformed_dash {
        output.push(text!("--"));
        output.push(text!("}}"));
    }
    let elements =
        super::bold::mark_format_stop_at_list(parser, Elements::Multiple(output));
    ok!(paragraph_safe; elements, errors)
}

fn wikidot_padding_only_space_drops(parser: &Parser<'_, '_>) -> bool {
    parser.full_text().inner()[parser.current().span.start..].starts_with("[[/")
}

fn unform_strike_ate_close<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let ate_close = matches!(
        elements.last(),
        Some(Element::Container(container))
            if container.ctype() == ContainerType::Strikethrough
                && container.attributes().get().is_empty()
                && matches!(
                    container.elements().last(),
                    Some(Element::Text(text)) if text.as_ref() == "}}",
                )
    );
    if !ate_close {
        return false;
    }
    let Some(Element::Container(strike)) = elements.pop() else {
        return false;
    };
    let mut children: Vec<Element<'t>> = strike.into();
    children.pop();
    elements.push(text!("--"));
    elements.extend(children);
    true
}

fn wikidot_shed_close(elements: &[Element<'_>]) -> Option<(usize, String, String, bool)> {
    let (shed_index, trailing_space) = match elements.last() {
        Some(Element::Text(value)) if value.as_ref() == " " => {
            (elements.len().checked_sub(2)?, true)
        }
        Some(_) => (elements.len() - 1, false),
        None => return None,
    };
    let Element::Text(value) = &elements[shed_index] else {
        return None;
    };
    let (prefix, suffix) = monospace_close_split(value.as_ref())?;
    if !elements[..shed_index].iter().any(element_contains_link) {
        return None;
    }
    Some((
        shed_index,
        prefix.to_string(),
        suffix.to_string(),
        trailing_space,
    ))
}

fn strip_seam_edge_spaces(elements: &mut Vec<Element<'_>>, leading: bool) -> bool {
    if leading {
        let mut index = 0usize;
        let mut saw_hidden = false;
        let mut spaces = Vec::new();
        while index < elements.len() {
            if matches!(&elements[index], Element::Delayed(delayed) if delayed.is_suppression_seam())
            {
                saw_hidden = saw_hidden
                    || matches!(&elements[index], Element::Delayed(delayed) if !delayed.is_typography_boundary());
                index += 1;
            } else if matches!(&elements[index], Element::Text(text) if !text.is_empty() && text.bytes().all(|byte| byte == b' ' || byte == b'\t'))
            {
                spaces.push(index);
                index += 1;
            } else {
                break;
            }
        }
        if saw_hidden && !spaces.is_empty() {
            for at in spaces.into_iter().rev() {
                elements.remove(at);
            }
            return true;
        }
        return false;
    }
    let mut index = elements.len();
    let mut saw_hidden = false;
    let mut spaces = Vec::new();
    while index > 0 {
        if matches!(&elements[index - 1], Element::Delayed(delayed) if delayed.is_suppression_seam())
        {
            saw_hidden = saw_hidden
                || matches!(&elements[index - 1], Element::Delayed(delayed) if !delayed.is_typography_boundary());
            index -= 1;
        } else if matches!(&elements[index - 1], Element::Text(text) if !text.is_empty() && text.bytes().all(|byte| byte == b' ' || byte == b'\t'))
        {
            spaces.push(index - 1);
            index -= 1;
        } else {
            break;
        }
    }
    if saw_hidden && !spaces.is_empty() {
        for at in spaces {
            elements.remove(at);
        }
        return true;
    }
    false
}

fn is_wikidot_edge_padding(token: &ExtractedToken<'_>) -> bool {
    token.token == Token::Whitespace
        && !token.slice.is_empty()
        && token.slice.bytes().all(|byte| matches!(byte, b' ' | b'\t'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::token::ExtractedToken;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};
    use std::time::{Duration, Instant};

    fn render(input: &str) -> (String, Vec<ParseError>) {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        (html, errors)
    }

    #[test]
    fn monospace_trims_evidenced_ascii_space_padding() {
        for input in ["{{text}}", "{{ text}}", "{{text }}", "{{ text }}"] {
            let (html, errors) = render(input);
            assert!(errors.is_empty(), "{input:?}: {errors:?}");
            assert!(html.contains("<tt>text</tt>"), "{input:?}: {html}",);
        }
    }

    #[test]
    fn wikidot_suppresses_the_exact_monospace_zero_value() {
        for (input, expected) in [
            ("{{0}}", ""),
            ("A{{0}}B", "<p>AB</p>"),
            ("A {{0}} {{****}} {{****}} B", "<p>A B</p>"),
            ("{{****}}", ""),
            ("{{00}}", "<p><tt>00</tt></p>"),
            ("{{-0}}", "<p><tt>-0</tt></p>"),
            ("{{0.0}}", "<p><tt>0.0</tt></p>"),
        ] {
            let (html, errors) = render(input);
            assert!(errors.is_empty(), "{input:?}: {errors:?}");
            assert_eq!(html, expected, "{input:?}");
        }
    }

    #[test]
    fn wikidot_collapses_suppressed_monospace_spaces_inside_color_and_size() {
        let (html, errors) = render(concat!(
            "##grey|**{{[+0](+0/-0)}}** **{{0}}**",
            "[[size 0.9em]]A {{****}} {{****}} B[[/size]]##",
        ));

        assert!(errors.is_empty(), "{errors:?}");
        assert!(
            html.contains(r#"<span style="font-size:0.9em;">A B</span>"#)
                && !html.contains("A   B"),
            "{html}",
        );
    }

    #[test]
    fn scuttle_padded_monospace_matches_the_public_wikidot_text() {
        let input = "**Affected Sites:** {{ Output Error: List object exceeds 10,000 characters. }}";
        let (html, errors) = render(input);

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            html,
            concat!(
                "<p><strong>Affected Sites:</strong> ",
                "<tt>",
                "Output Error: List object exceeds 10,000 characters.",
                "</tt></p>",
            ),
        );
    }

    #[test]
    fn monospace_collapses_space_runs_and_preserves_markup() {
        let (html, errors) = render("before {{   a  **b**  c   }} after");
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(html, "<p>before <tt>a <strong>b</strong> c</tt> after</p>",);
    }

    #[test]
    fn monospace_space_only_body_preserves_one_visible_space_without_a_container() {
        let (html, errors) = render("before{{   }}after");
        assert!(errors.is_empty(), "{errors:?}");
        assert!(html.contains("before after"), "{html}");
        assert!(!html.contains("<tt>"), "{html}");
    }

    #[test]
    fn monospace_uses_the_last_pair_in_a_closing_brace_run() {
        for (input, expected) in [
            ("{{x}}}}tail", "<p><tt>x}}</tt>tail</p>"),
            ("{{x }}}}tail", "<p><tt>x }}</tt>tail</p>"),
            ("{{x}}}}}}tail", "<p><tt>x}}}}</tt>tail</p>"),
        ] {
            let (html, errors) = render(input);
            assert!(errors.is_empty(), "{input:?}: {errors:?}");
            assert_eq!(html, expected, "{input:?}");
        }
    }

    #[test]
    fn monospace_moves_edge_padding_outside_the_container() {
        for (input, expected) in [
            ("{{x }}{{ y}}", "<p><tt>x</tt> <tt>y</tt></p>"),
            ("{{x }}after", "<p><tt>x</tt> after</p>"),
            ("before{{ x}}", "<p>before <tt>x</tt></p>"),
            ("{{x   }}{{   y}}", "<p><tt>x</tt> <tt>y</tt></p>"),
        ] {
            let (html, errors) = render(input);
            assert!(errors.is_empty(), "{input:?}: {errors:?}");
            assert_eq!(html, expected, "{input:?}");
        }
    }

    #[test]
    fn monospace_padding_failure_rolls_back_without_losing_source() {
        for input in ["{{ x", "{{x ", "prefix {{ x", "{{ x }", "{{ x\n\n y }}"] {
            let (html, _errors) = render(input);
            assert!(!html.contains("<tt>"), "{input:?}: {html}");
            assert!(html.contains("{{"), "{input:?}: {html}");
        }
    }

    #[test]
    fn monospace_keeps_nested_opener_literal_and_closes_at_the_first_terminator() {
        let input = "{{outer {{ inner }} tail}}";
        let (html, _errors) = render(input);
        assert_eq!(html, "<p><tt>outer {{ inner</tt> tail}}</p>");
    }

    #[test]
    fn monospace_normalizes_and_trims_edge_tabs_like_wikidot() {
        let (leading_html, leading_errors) = render("{{\ttext}}");
        assert!(leading_errors.is_empty(), "{leading_errors:?}");
        assert_eq!(leading_html, "<p><tt>text</tt></p>");

        let (trailing_html, trailing_errors) = render("{{text\t}}");
        assert!(trailing_errors.is_empty(), "{trailing_errors:?}");
        assert_eq!(trailing_html, "<p><tt>text</tt></p>");
    }

    #[test]
    fn repeated_quoted_monospace_emails_stay_bounded() {
        let mut input = String::new();
        for _ in 0..32 {
            input.push_str("> FROM: <{{person@scp.foundation}}>\n> \n");
        }

        let started = Instant::now();
        let (html, errors) = render(&input);
        let elapsed = started.elapsed();

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("<tt>").count(), 32, "{html}");
        assert!(
            elapsed < Duration::from_millis(500),
            "repeated monospace email parse took {elapsed:?}",
        );
    }

    #[test]
    fn edge_padding_accepts_spaces_and_tabs_only() {
        let spaces = ExtractedToken {
            token: Token::Whitespace,
            slice: "   ",
            span: 0..3,
        };
        let tab = ExtractedToken {
            token: Token::Whitespace,
            slice: "\t",
            span: 0..1,
        };
        let other = ExtractedToken {
            token: Token::Identifier,
            slice: " ",
            span: 0..1,
        };

        assert!(is_wikidot_edge_padding(&spaces));
        assert!(is_wikidot_edge_padding(&tab));
        assert!(!is_wikidot_edge_padding(&other));
    }
}
