/*
 * parsing/rule/impls/block/blocks/align.rs
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
use super::{consume_wikidot_flat_head_break, wikidot_flat_pair_for_head};
use crate::delayed::DelayedElement;
use crate::parsing::parser::block_pairs::WikidotBlockKind;
use crate::tree::{Alignment, AttributeMap, PartialElement, WikidotBlockShell};

macro_rules! make_align_block {
    ($block_const:ident, $block_name:expr, $symbol:expr, $align:ident) => {
        use super::align::parse_alignment_block;
        use super::prelude::*;
        use crate::tree::Alignment;

        pub const $block_const: BlockRule = BlockRule {
            name: $block_name,
            accepts_names: &[$symbol],
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
            parse_alignment_block(
                (&$block_const, Alignment::$align),
                parser,
                name,
                opener_start,
                flag_star,
                flag_score,
                in_head,
            )
        }
    };
}

pub fn parse_alignment_block<'r, 't>(
    (block_rule, alignment): (&BlockRule, Alignment),
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    let block_name = block_rule.name;
    debug!(
        "Parsing align block {name}/{block_name}/{}, in-head {in_head}",
        alignment.name(),
    );
    assert!(!flag_star, "Alignment block doesn't allow star flag");
    assert!(!flag_score, "Alignment block doesn't allow score flag");
    assert_block_name(block_rule, name);

    let source = parser.full_text().inner();
    let escaped_owner = parser.settings().layout.legacy()
        && source[..opener_start]
            .bytes()
            .rev()
            .take_while(|byte| *byte == b'\\')
            .count()
            % 2
            == 1;

    // Live Wikidot gives these blocks line-owned openers: the opener must be
    // the first thing on its line and be followed immediately by a line
    // break. A mid-line or indented opener, or trailing text after it,
    // renders literally.
    if parser.settings().layout.legacy() && !in_head {
        let at_line_start = match opener_start
            .checked_sub(1)
            .map(|index| source.as_bytes()[index])
        {
            None => true,
            Some(b'\n' | b'\r') => true,
            Some(_) => parser.in_native_blockquote_line(),
        };
        if !at_line_start {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
        let head_end = source[opener_start..]
            .find("]]")
            .map(|offset| opener_start + offset + 2)
            .unwrap_or(source.len());
        if !source[head_end..]
            .as_bytes()
            .first()
            .is_some_and(|byte| matches!(*byte, b'\n' | b'\r'))
        {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
    }

    let body_start = parser.get_head_none_with_body_start(block_rule, in_head)?;

    if let Some(pair) = wikidot_flat_pair_for_head(parser, opener_start)
        && matches!(
            pair.kind,
            WikidotBlockKind::AlignCenter
                | WikidotBlockKind::AlignRight
                | WikidotBlockKind::AlignLeft
                | WikidotBlockKind::AlignJustify
        )
    {
        consume_wikidot_flat_head_break(parser, false)?;
        let shell = WikidotBlockShell::Align {
            head: opener_start,
            alignment,
        };
        return ok!(Element::Partial(PartialElement::WikidotBlockOpen(shell)));
    }

    if !parser.has_body_end_block(block_rule) {
        return Err(parser.make_end_of_input_err());
    }

    if escaped_owner {
        let _ = parser.get_body_text(block_rule)?;
        let owner_end = parser.current().span.start;
        let generated = parser.generated_in_range(opener_start..owner_end);
        return if generated.is_empty() {
            ok!(true; literal_elements(&source[opener_start..owner_end]))
        } else {
            ok!(true; Element::Delayed(DelayedElement::shell(
                source,
                opener_start..owner_end,
                &generated,
            )))
        };
    }

    let wikidot_center =
        parser.settings().layout.legacy() && alignment == Alignment::Center;
    if wikidot_center {
        parser.enter_wikidot_center_body();
    }
    let body = parser.get_body_elements_with_context(block_rule, true, body_start);
    if wikidot_center {
        parser.leave_wikidot_center_body();
    }
    let (elements, errors, _) = body?.into();

    let element = Element::Container(Container::new(
        ContainerType::Align(alignment),
        elements,
        AttributeMap::new(),
    ));

    ok!(element, errors)
}

fn literal_elements(source: &str) -> Elements<'_> {
    let mut elements = Vec::new();
    for chunk in source.split_inclusive('\n') {
        let has_newline = chunk.ends_with('\n');
        let line = chunk.strip_suffix('\n').unwrap_or(chunk);
        let line = line.strip_suffix('\r').unwrap_or(line);
        if !line.is_empty() {
            elements.push(text!(line));
        }
        if has_newline {
            elements.push(Element::LineBreak);
        }
    }
    Elements::Multiple(elements)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};
    use std::time::{Duration, Instant};

    #[test]
    fn alignment_block_wraps_body_in_align_container() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[=]]\ncentered[[/=]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        let [Element::Container(container)] = tree.elements.as_slice() else {
            panic!("expected alignment container, got {:?}", tree.elements);
        };
        assert_eq!(container.ctype(), ContainerType::Align(Alignment::Center));
        let [Element::Container(paragraph)] = container.elements() else {
            panic!("expected paragraph body, got {:?}", container.elements());
        };
        assert_eq!(paragraph.ctype(), ContainerType::Paragraph);
        assert_eq!(paragraph.elements(), &[text!("centered")]);
    }

    #[test]
    fn wikidot_separates_adjacent_alignment_blocks_with_breaks() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(
            "[[=]]\n[[div class=\"blockquote\"]]\ncard body 0\n\n[[/div]]\n[[/=]]\n[[=]]\n[[div class=\"blockquote\"]]\ncard body 1\n\n[[/div]]\n[[/=]]",
        );
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            html,
            "<div style=\"text-align: center;\"><div class=\"blockquote\"><p>card body 0</p></div></div><br>\n<div style=\"text-align: center;\"><div class=\"blockquote\"><p>card body 1</p></div></div>",
        );
    }

    #[test]
    fn wikidot_alignment_close_unwraps_the_following_physical_line() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let render = |source| {
            let tokenization = crate::tokenize(source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            assert!(errors.is_empty(), "{errors:?}");
            HtmlRender.render(&tree, &page_info, &settings).body
        };

        assert_eq!(
            render("[[=]]\ncentered\n[[/=]]\nafter"),
            "<div style=\"text-align: center;\"><p>centered</p></div><br>\nafter",
        );
        assert_eq!(
            render("[[=]]\ncentered\n[[/=]]"),
            "<div style=\"text-align: center;\"><p>centered</p></div>",
        );
        assert_eq!(
            render("[[=]]\ncentered\n[[/=]]\n\nafter"),
            "<div style=\"text-align: center;\"><p>centered</p></div><p>after</p>",
        );
    }

    #[test]
    fn quoted_alignment_blocks_remain_native_and_bounded() {
        const BLOCK_COUNT: usize = 64;

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut input = String::new();
        for index in 0..BLOCK_COUNT {
            input.push_str("> [[>]]\n> right-");
            input.push_str(&index.to_string());
            input.push_str("\n> [[/>]]\n> after-");
            input.push_str(&index.to_string());
            input.push('\n');
        }
        input.push_str("outside-sentinel\n");
        crate::preprocess(&mut input);

        let started = Instant::now();
        let tokenization = crate::tokenize(&input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("text-align: right;").count(), BLOCK_COUNT);
        for index in 0..BLOCK_COUNT {
            assert!(html.contains(&format!("right-{index}")), "{html}");
            assert!(html.contains(&format!("after-{index}")), "{html}");
        }
        assert!(html.contains("outside-sentinel"), "{html}");
        assert!(!html.contains("[[>]]"), "{html}");
        assert!(!html.contains("[[/>]]"), "{html}");
        assert_eq!(html.matches("<br>").count(), BLOCK_COUNT * 2 - 1, "{html}");
        assert_eq!(html.matches("<p>").count(), BLOCK_COUNT + 1, "{html}");
    }

    #[test]
    fn unclosed_quoted_alignment_blocks_fail_closed_in_bounded_time() {
        const BLOCK_COUNT: usize = 512;

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut input = String::new();
        for index in 0..BLOCK_COUNT {
            input.push_str("> [[>]] malformed-");
            input.push_str(&index.to_string());
            input.push('\n');
        }
        input.push_str("outside-sentinel\n");

        let started = Instant::now();
        let tokenization = crate::tokenize(&input);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(started.elapsed() < Duration::from_secs(5));
        for index in 0..BLOCK_COUNT {
            assert!(html.contains(&format!("malformed-{index}")), "{html}");
        }
        assert!(html.contains("outside-sentinel"), "{html}");
        assert_eq!(html.matches("[[&gt;]]").count(), BLOCK_COUNT, "{html}");
        assert!(!html.contains("text-align: right;"), "{html}");
    }

    #[test]
    fn wikidot_paragraph_break_separated_alignment_blocks_have_no_break() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(
            "[[>]]\nRight\n[[/>]]\n\n[[<]]\nLeft\n[[/<]]\n\n[[=]]\nCenter\n[[/=]]\n\n[[==]]\nJustify\n[[/==]]\n\n\nDeep align blocks:\n[[=]]\nApple\n[[>]]\nBanana\n[[==]]\nCherry\n[[<]]\nBlueberry\n[[/<]]\n[[/==]]\n[[/>]]\n[[/=]]",
        );
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            html,
            "<div style=\"text-align: right;\"><p>Right</p></div><div style=\"text-align: left;\"><p>Left</p></div><div style=\"text-align: center;\"><p>Center</p></div><div style=\"text-align: justify;\"><p>Justify</p></div><p>Deep align blocks:<br>\n</p><div style=\"text-align: center;\"><p>Apple<br>\n</p><div style=\"text-align: right;\"><p>Banana<br>\n</p><div style=\"text-align: justify;\"><p>Cherry<br>\n</p><div style=\"text-align: left;\"><p>Blueberry</p></div></div></div></div>",
        );
    }

    #[test]
    fn wikidot_adjacent_line_alignment_blocks_keep_their_break() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[=]]\nA\n[[/=]]\n[[=]]\nB\n[[/=]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            html,
            "<div style=\"text-align: center;\"><p>A</p></div><br>\n<div style=\"text-align: center;\"><p>B</p></div>",
        );
    }
}
