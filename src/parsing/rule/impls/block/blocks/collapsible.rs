/*
 * parsing/rule/impls/block/blocks/collapsible.rs
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
use crate::parsing::consume::consume;
use crate::parsing::rule::impls::block::parser::BlockBodyStart;
use crate::parsing::{ParseError, ParseErrorKind};
use crate::tree::{AttributeMap, ListItem, ListType};
use std::borrow::Cow;

pub const BLOCK_COLLAPSIBLE: BlockRule = BlockRule {
    name: "block-collapsible",
    accepts_names: &["collapsible"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines: true,
    parse_fn,
};

pub(crate) struct CollapsibleHead<'t> {
    attributes: AttributeMap<'t>,
    start_open: bool,
    show_text: Option<Cow<'t, str>>,
    hide_text: Option<Cow<'t, str>>,
    show_top: bool,
    show_bottom: bool,
}

impl<'t> CollapsibleHead<'t> {
    pub(crate) fn into_element(self, elements: Vec<Element<'t>>) -> Element<'t> {
        self.into_element_with_unfolded_tail(elements, None)
    }

    fn into_element_with_unfolded_tail(
        self,
        elements: Vec<Element<'t>>,
        unfolded_tail_start: Option<usize>,
    ) -> Element<'t> {
        Element::Collapsible {
            elements,
            unfolded_tail_start,
            wikidot_label_spans: Vec::new(),
            attributes: self.attributes,
            start_open: self.start_open,
            show_text: self.show_text,
            hide_text: self.hide_text,
            show_top: self.show_top,
            show_bottom: self.show_bottom,
        }
    }
}

pub(crate) fn parse_collapsible_head<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    in_head: bool,
) -> Result<(CollapsibleHead<'t>, BlockBodyStart), ParseError>
where
    'r: 't,
{
    let (mut arguments, body_start) =
        parser.get_head_map_with_body_start_wikidot(&BLOCK_COLLAPSIBLE, in_head)?;
    let show_text = arguments.get("show");
    let hide_text = arguments.get("hide");
    // Wikidot unfolds only these two exact values. Every other present value,
    // including an empty value, retains the default folded state.
    let start_open = if parser.settings().layout.legacy() {
        matches!(arguments.get("folded").as_deref(), Some("no" | "false"))
    } else {
        !arguments.get_bool(parser, "folded")?.unwrap_or(true)
    };
    let (show_top, show_bottom) = match arguments.get("hideLocation") {
        Some(value) => parse_hide_location(&value, parser)?,
        None => (true, false),
    };
    let attributes = arguments.to_attribute_map(parser.settings());
    assert_block_name(&BLOCK_COLLAPSIBLE, name);

    Ok((
        CollapsibleHead {
            attributes,
            start_open,
            show_text,
            hide_text,
            show_top,
            show_bottom,
        },
        body_start,
    ))
}

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing collapsible block (in-head {in_head})");
    assert!(!flag_star, "Collapsible doesn't allow star flag");
    assert!(!flag_score, "Collapsible doesn't allow score flag");
    assert_block_name(&BLOCK_COLLAPSIBLE, name);

    let (head, body_start) = parse_collapsible_head(parser, name, in_head)?;

    let previous_collapsible_context = parser.in_wikidot_collapsible();
    if parser.settings().layout.legacy() {
        parser.set_in_wikidot_collapsible(true);
    }
    let body = parser.get_body_elements_with_literal_quote_context(
        &BLOCK_COLLAPSIBLE,
        true,
        body_start,
    );
    let body = match body {
        Ok(body) => body,
        Err(error) => {
            parser.set_in_wikidot_collapsible(previous_collapsible_context);
            return Err(error);
        }
    };
    let (mut elements, mut errors, _) = body.into();
    let unfolded_tail_start = if parser.take_wikidot_collapsible_content_closed() {
        let tail_start = elements.len();
        let tail = parser.get_body_elements_with_literal_quote_context(
            &BLOCK_COLLAPSIBLE,
            true,
            BlockBodyStart::NextPhysicalLine,
        );
        let tail = match tail {
            Ok(tail) => tail,
            Err(error) => {
                parser.set_in_wikidot_collapsible(previous_collapsible_context);
                return Err(error);
            }
        };
        let (tail, tail_errors, _) = tail.into();
        errors.extend(tail_errors);
        elements.extend(prepare_wikidot_unfolded_tail(tail));
        Some(tail_start)
    } else {
        None
    };
    parser.set_in_wikidot_collapsible(previous_collapsible_context);
    if parser.settings().layout.legacy()
        && parser.pending_wikidot_collapsible_closer()
        && parser.current().token == Token::LineBreak
    {
        parser.step()?;
    }

    let element = head.into_element_with_unfolded_tail(elements, unfolded_tail_start);

    let mut output = vec![element];
    if parser.take_wikidot_crossed_ol_tail_sentinel()? {
        output.push(wikidot_crossed_ol_tail());
        return ok!(false; output, errors);
    }
    if parser.take_wikidot_crossed_list_break_sentinel()? {
        output.push(Element::LineBreak);
        return ok!(false; output, errors);
    }
    if parser.settings().layout.legacy()
        && parser.native_blockquote_depth().is_some()
        && !parser.start_of_line()
        && !matches!(
            parser.current().token,
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
        )
    {
        let mut paragraph_safe = true;
        while !matches!(
            parser.current().token,
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
        ) {
            let suffix = consume(parser)?.chain(&mut errors, &mut paragraph_safe);
            output.extend(suffix);
        }
        if parser.current().token == Token::LineBreak {
            parser.step()?;
        }
    }

    ok!(false; output, errors)
}

fn wikidot_crossed_ol_tail<'t>() -> Element<'t> {
    let mut item_attributes = AttributeMap::new();
    item_attributes.insert("style", cow!("list-style: none"));
    Element::List {
        ltype: ListType::Bullet,
        items: vec![ListItem::Elements {
            elements: vec![Element::LineBreak],
            attributes: item_attributes,
        }],
        attributes: AttributeMap::new(),
    }
}

fn prepare_wikidot_unfolded_tail(mut elements: Vec<Element<'_>>) -> Vec<Element<'_>> {
    if let [Element::Container(container)] = elements.as_slice()
        && container.ctype() == crate::tree::ContainerType::Paragraph
    {
        elements = container.elements().to_vec();
    }
    elements.insert(0, Element::LineBreak);
    elements
}

fn parse_hide_location(s: &str, parser: &Parser) -> Result<(bool, bool), ParseError> {
    // Wikidot treats every unrecognized or differently-cased value as `top`.
    if parser.settings().layout.legacy() {
        return Ok(match s {
            "bottom" => (false, true),
            "both" => (true, true),
            _ => (true, false),
        });
    }

    const NAMES: [(&str, (bool, bool)); 6] = [
        ("top", (true, false)),
        ("side", (true, false)),
        ("bottom", (false, true)),
        ("both", (true, true)),
        ("neither", (false, false)),
        ("none", (false, false)),
    ];

    let s = s.trim();
    for &(name, value) in &NAMES {
        if name.eq_ignore_ascii_case(s) {
            return Ok(value);
        }
    }

    warn!("Unknown hideLocation argument '{s}'");
    Err(parser.make_err(ParseErrorKind::BlockMalformedArguments))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender, text::TextRender};
    use crate::settings::{WikitextMode, WikitextSettings};
    use std::time::{Duration, Instant};

    #[test]
    fn collapsible_hide_location_controls_visible_handles() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        let tokenization = crate::tokenize(
            "[[collapsible folded=\"no\" hideLocation=\"both\"]]Body[[/collapsible]]",
        );
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:?}");
        match tree.elements.as_slice() {
            [
                Element::Collapsible {
                    start_open,
                    show_top,
                    show_bottom,
                    ..
                },
            ] => {
                assert!(*start_open);
                assert!(*show_top);
                assert!(*show_bottom);
            }
            other => panic!("expected collapsible element, got {other:?}"),
        }

        let tokenization =
            crate::tokenize("[[collapsible hideLocation=\"side\"]]Body[[/collapsible]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("+&nbsp;show&nbsp;block"), "{html}");
        assert!(html.contains("–&nbsp;hide&nbsp;block"), "{html}");

        let (html, _, errors) =
            render("[[collapsible hideLocation=\"neither\"]]Body[[/collapsible]]");
        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("–&nbsp;hide&nbsp;block"), "{html}");
    }

    fn render(input: &str) -> (String, String, Vec<ParseError>) {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        let text = TextRender.render(&tree, &page_info, &settings);
        (html, text, errors)
    }

    #[test]
    fn wikidot_collapsible_uses_legacy_labels_and_drops_outer_attributes() {
        let (html, _, errors) = render(
            r#"[[collapsible id="hider" class="custom" style="color: teal" data-hider="yes"]]Body[[/collapsible]]"#,
        );

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("+&nbsp;show&nbsp;block"), "{html}");
        assert!(html.contains("–&nbsp;hide&nbsp;block"), "{html}");
        assert!(!html.contains("custom"), "{html}");
        assert!(!html.contains("hider"), "{html}");
        assert!(!html.contains("color: teal"), "{html}");
        assert!(!html.contains("data-hider"), "{html}");

        let (html, _, errors) =
            render(r#"[[collapsible show="" hide="close"]]Body[[/collapsible]]"#);
        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("+&nbsp;show&nbsp;block"), "{html}");
    }

    #[test]
    fn quoted_multiline_collapsibles_remain_native_and_bounded() {
        let mut input = String::new();
        for index in 0..24 {
            input.push_str(&format!(
                concat!(
                    "> title-{0}\n",
                    "> [[collapsible show=\"show-{0}\" hide=\"hide-{0}\"]]\n",
                    "> \n",
                    "> body-{0}\n",
                    "> [[/collapsible]]\n",
                    "\n",
                ),
                index,
            ));
        }
        input.push_str("following-page-content\n");

        let started = Instant::now();
        let (html, text, errors) = render(&input);
        let elapsed = started.elapsed();

        assert!(elapsed < Duration::from_secs(5), "elapsed {elapsed:?}");
        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 24);
        assert_eq!(html.matches("<blockquote>").count(), 24);
        assert!(html.contains("show-0"), "{html}");
        assert!(html.contains("hide-23"), "{html}");
        assert!(html.contains("body-0"), "{html}");
        assert!(html.contains("body-23"), "{html}");
        assert!(html.contains("following-page-content"), "{html}");
        assert!(!html.contains("[[collapsible"), "{html}");
        assert!(!html.contains("[[/collapsible]]"), "{html}");
        assert!(text.contains("body-0"), "{text}");
        assert!(text.contains("body-23"), "{text}");
    }

    #[test]
    fn continuous_unclosed_quoted_collapsibles_are_bounded() {
        let mut input = String::new();
        for index in 0..1024 {
            input.push_str(&format!(
                "> [[collapsible show=\"show-{index}\"]]\n> readable-{index}\n",
            ));
        }

        let started = Instant::now();
        let (html, _, _) = render(&input);
        let elapsed = started.elapsed();

        assert!(elapsed < Duration::from_secs(5), "elapsed {elapsed:?}");
        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 0);
        assert!(html.contains("[[collapsible"), "{html}");
        assert!(html.contains("readable-0"), "{html}");
        assert!(html.contains("readable-1023"), "{html}");
    }

    #[test]
    fn quoted_collapsible_keeps_inline_body_on_generic_path() {
        let input = concat!(
            "> [[collapsible show=\"show\" hide=\"hide\"]]inline body\n",
            "[[/collapsible]]\n",
            "following content\n",
        );
        let (html, _, errors) = render(input);

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 1);
        assert!(html.contains("inline body"), "{html}");
        assert!(
            html.contains("</blockquote><br>\nfollowing content"),
            "{html}"
        );
        assert!(!html.contains("<p>following content</p>"), "{html}");
    }

    #[test]
    fn quoted_multiline_collapsible_allows_immediate_close_and_spaces() {
        let input = concat!(
            "> [[collapsible show=\"show\" hide=\"hide\"]]\n",
            "> [[/collapsible]]   \n",
            "> following quote\n",
        );
        let (html, _, errors) = render(input);

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 1);
        assert_eq!(html.matches("<blockquote>").count(), 1, "{html}");
        assert!(html.contains("following quote"), "{html}");
        assert!(!html.contains("[[/collapsible]]"), "{html}");
    }

    #[test]
    fn quoted_midline_close_stays_literal_until_standalone_close() {
        let input = concat!(
            "> [[collapsible show=\"show\"]]\n",
            "> before [[/collapsible]] stray\n",
            "> body after false close\n",
            "> [[/collapsible]]\n",
            "> following quote\n",
        );
        let (html, text, _errors) = render(input);

        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 1);
        assert_eq!(html.matches("<blockquote>").count(), 1, "{html}");
        assert!(
            html.contains(
                r#"<div class="collapsible-block-content"><p>before</p></div>"#
            ),
            "{html}"
        );
        assert!(html.contains("stray<br>\nbody after false close"), "{html}");
        assert!(html.contains("[[/collapsible]]<br>"), "{html}");
        assert!(html.contains("body after false close"), "{html}");
        assert!(html.contains("following quote"), "{html}");
        assert!(!text.contains("[[/collapsible]] stray"), "{text}");
        assert!(text.contains("[[/collapsible]]"), "{text}");
        assert!(text.contains("body after false close"), "{text}");
    }

    #[test]
    fn quoted_close_with_trailing_text_closes_before_the_remaining_line() {
        let input = concat!(
            "> [[collapsible show=\"show\" hide=\"hide\"]]\n",
            "> body\n",
            "> [[/collapsible]] still quoted\n",
            "following page\n",
        );
        let (html, _, _) = render(input);

        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 1);
        assert!(!html.contains("[[collapsible"), "{html}");
        assert!(!html.contains("[[/collapsible]]"), "{html}");
        let quoted = html.find("still quoted").expect("quoted text missing");
        let blockquote_end = html
            .find("</blockquote>")
            .expect("blockquote close missing");
        let following = html.find("following page").expect("following text missing");
        assert!(quoted < blockquote_end, "{html}");
        assert!(blockquote_end < following, "{html}");
    }

    #[test]
    fn quoted_collapsible_ends_at_quote_boundary_before_a_later_close() {
        let (html, _, errors) = render(concat!(
            "> [[collapsible show=\"show\"]]\n",
            "unquoted body\n",
            "[[/collapsible]]\n",
            "following page\n",
        ));
        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 1);
        assert!(
            html.contains("</blockquote><p>unquoted body</p><br>\nfollowing page"),
            "{html}"
        );
        assert!(!html.contains("[[collapsible"), "{html}");

        let (html, _, errors) = render(concat!(
            "> [[collapsible show=\"show\"]]\n",
            "> body\n",
            "\n",
            "> [[/collapsible]]\n",
            "following page\n",
        ));
        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 1);
        assert!(
            html.contains("<div class=\"collapsible-block-content\"><p>body</p></div>"),
            "{html}"
        );
        assert!(html.contains("</blockquote><p>following page"), "{html}");
        assert!(!html.contains("[[collapsible"), "{html}");
    }

    #[test]
    fn nested_span_does_not_hide_an_earlier_collapsible_close() {
        let input = concat!(
            "> [[collapsible show=\"show\"]]\n",
            "> [[span]]\n",
            "> [[/collapsible]]\n",
            "> [[/span]]\n",
            "\n",
            "> escaped page content\n",
            "> [[/collapsible]]\n",
            "following page\n",
        );
        let (html, _, _) = render(input);

        // Live Wikidot closes the collapsible at the first closer even though
        // the span remains active until the following quoted line.
        assert_eq!(
            html.matches("class=\"collapsible-block\"").count(),
            1,
            "{html}"
        );
        assert!(!html.contains("[[collapsible show"), "{html}");
        assert!(html.contains("escaped page content"), "{html}");
        assert!(html.contains("following page"), "{html}");
    }

    #[test]
    fn nested_multiline_inline_collector_prepares_each_quote_prefix() {
        let input = concat!(
            "> [[collapsible show=\"show\"]]\n",
            "> **alpha\n",
            "> beta**\n",
            "> [[/collapsible]]\n",
            "> following quote\n",
        );
        let (html, _, errors) = render(input);

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html.matches("class=\"collapsible-block\"").count(),
            1,
            "{html}"
        );
        assert!(html.contains("<strong>alpha<br>\nbeta</strong>"), "{html}");
        assert!(html.contains("following quote"), "{html}");
        assert!(!html.contains("&gt; beta"), "{html}");
    }

    #[test]
    fn quoted_collapsible_reuses_block_level_paragraph_semantics() {
        let input = concat!(
            "> [[collapsible show=\"show\"]]\n",
            "> text before rule\n",
            "> ----\n",
            "> text after rule\n",
            "> [[/collapsible]]\n",
        );
        let (html, _, errors) = render(input);

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("<p>text before rule</p><hr>"), "{html}");
        assert!(html.contains("<p>text after rule</p>"), "{html}");
        assert!(!html.contains("text before rule<br>"), "{html}");
    }

    #[test]
    fn quoted_collapsible_preserves_nested_native_quote_depths() {
        let input = concat!(
            "> [[collapsible show=\"show\"]]\n",
            ">> nested one\n",
            ">>> nested two\n",
            "> [[/collapsible]]\n",
            "> following quote\n",
            "following page\n",
        );
        let (html, _, errors) = render(input);

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 1);
        assert_eq!(html.matches("<blockquote>").count(), 3, "{html}");
        assert!(html.contains("nested one"), "{html}");
        assert!(html.contains("nested two"), "{html}");
        assert!(html.contains("following quote"), "{html}");
        assert!(html.contains("following page"), "{html}");
        assert!(!html.contains("[[collapsible"), "{html}");
    }

    #[test]
    fn nested_quoted_collapsibles_restore_outer_quote_cursor() {
        let input = concat!(
            "> [[collapsible show=\"outer\"]]\n",
            ">> [[collapsible show=\"inner\"]]\n",
            ">> inner body\n",
            ">> [[/collapsible]]\n",
            "> outer body\n",
            "> [[/collapsible]]\n",
            "> following quote\n",
        );
        let (html, _, errors) = render(input);

        assert!(!errors.is_empty());
        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 1);
        assert_eq!(html.matches("<blockquote>").count(), 2, "{html}");
        assert!(
            html.contains("[[collapsible show=&quot;inner&quot;]]"),
            "{html}"
        );
        assert!(html.contains("inner body"), "{html}");
        assert!(html.contains("outer body"), "{html}");
        assert!(html.contains("following quote"), "{html}");
        assert!(
            html.contains(
                "</blockquote><p>outer body<br>\n[[/collapsible]]<br>\nfollowing quote</p>"
            ),
            "{html}"
        );
    }

    #[test]
    fn nested_spaced_quote_collapsible_uses_absolute_depth() {
        let input = concat!(
            "> [[collapsible show=\"outer\"]]\n",
            "> > [[collapsible show=\"inner\"]]\n",
            "> > inner body\n",
            "> > [[/collapsible]]\n",
            "> outer body\n",
            "> [[/collapsible]]\n",
            "> following quote\n",
        );
        let (html, _, errors) = render(input);

        assert!(!errors.is_empty());
        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 1);
        assert_eq!(html.matches("<blockquote>").count(), 1, "{html}");
        assert!(
            html.contains("&gt; [[collapsible show=&quot;inner&quot;]]"),
            "{html}"
        );
        assert!(html.contains("inner body"), "{html}");
        assert!(html.contains("outer body"), "{html}");
        assert!(html.contains("following quote"), "{html}");
        assert!(html.contains("[[/collapsible]]"), "{html}");
        assert!(!html.contains("<p>outer body"), "{html}");
    }

    #[test]
    fn quoted_collapsible_scopes_close_policy_away_from_inline_blocks() {
        let input = concat!(
            "> [[collapsible show=\"show\"]]\n",
            "> [[span]]inline span[[/span]]\n",
            "> [[/collapsible]]\n",
        );
        let (html, _, errors) = render(input);

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 1);
        assert!(html.contains("<span>inline span</span>"), "{html}");
        assert!(!html.contains("[[span]]"), "{html}");
    }

    #[test]
    fn quoted_collapsible_keeps_nested_raw_text_block_markers_literal() {
        let input = concat!(
            "> [[collapsible show=\"show\"]]\n",
            "> [[code]]\n",
            "> hello code\n",
            "> [[/collapsible]] literal code text\n",
            "> [[/code]]\n",
            "> [[raw]]\n",
            "> **raw text**\n",
            "> [[/raw]]\n",
            "> [[/collapsible]]\n",
            "> following quote\n",
        );
        let (html, _, _errors) = render(input);

        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 1);
        assert_eq!(html.matches("<blockquote>").count(), 1, "{html}");
        assert!(html.contains("hello code"), "{html}");
        assert!(
            !html.contains("[[/collapsible]] literal code text"),
            "{html}"
        );
        assert!(html.contains("literal code text"), "{html}");
        assert!(html.contains("<strong>raw text</strong>"), "{html}");
        assert!(html.contains("[[code]]"), "{html}");
        assert!(html.contains("[[/code]]"), "{html}");
        assert!(html.contains("[[raw]]"), "{html}");
        assert!(html.contains("[[/raw]]"), "{html}");
        assert!(!html.contains("&gt; hello code"), "{html}");
        assert!(html.contains("following quote"), "{html}");
    }

    #[test]
    fn quoted_collapsible_with_trailing_opener_content_closes_at_its_quoted_closer() {
        // en/nexus-series (and jp/nexus-series): the table of contents is a
        // quoted collapsible whose opener line carries more content (`[[/size]]`
        // after the opener). Live Wikidot closes it at the quoted
        // `> [[/collapsible]]`: the retained html has three collapsible blocks
        // and all four code blocks on the full page. Before this change the
        // quoted closer stayed invisible behind its `> ` prefix, the first
        // collapsible swallowed the page up to the next unquoted closer, and
        // the later collapsible and its code blocks degraded to literal text.
        let source = concat!(
            "[[# toc]]\n",
            "> [[size larger]] [[collapsible show=\"Table of Contents\" hide=\"Fold\"]] [[/size]]\n",
            "> = **Table of Contents**\n",
            "> @@ @@[#nexus List of Nexuses]\n",
            "> @@    @@[#esterberg FP-120 | Esterberg]\n",
            "> [[/collapsible]]\n",
            "\n",
            "> Nexus\n",
            "> /ˈnɛksəs/\n",
            "\n",
            "-----\n",
            "\n",
            "[[collapsible show=\"Adding a Nexus to the List?\" hide=\"I am a block of cheese in a Nexus\"]]\n",
            "\n",
            "**Table of Contents**\n",
            "\n",
            "[[code]]\n",
            "> @@    @@[#name Nx-##| Name]\n",
            "[[/code]]\n",
            "-----\n",
            "\n",
            "**For Nexuses:**\n",
            "\n",
            "[[code]]\n",
            "\n",
            "[[# name]]\n",
            "**Area Class:** Insert Area Class\n",
            "[[/code]]\n",
            "\n",
            "-----\n",
            "\n",
            "**For Free Ports:**\n",
            "\n",
            "[[code]]\n",
            "[[# name]]\n",
            "**Area Class:** ##purple|**Free Port**##\n",
            "[[/code]]\n",
            "\n",
            "[[/collapsible]]\n",
            "\n",
            "[[size 200%]]**Documentation of a Nexus**[[/size]]\n",
        );
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, _) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert_eq!(
            html.matches("class=\"collapsible-block\"").count(),
            2,
            "{html}"
        );
        assert_eq!(tree.code_blocks.len(), 3, "{:?}", tree.code_blocks);
        assert_eq!(html.matches("<div class=\"code\">").count(), 3, "{html}");
        assert!(!html.contains("[[collapsible"), "{html}");
        assert!(!html.contains("[[/collapsible]]"), "{html}");
        assert!(!html.contains("[[code]]"), "{html}");
    }

    #[test]
    fn quoted_collapsible_opener_tails_do_not_hide_the_quoted_closer() {
        for tail in [" ", "\t", " x", " [[/size]]"] {
            let source = format!(
                "> [[collapsible show=\"T\" hide=\"F\"]]{tail}\n> body\n> [[/collapsible]]\n\nB\n\n[[collapsible show=\"A\" hide=\"a\"]]\n[[code]]\nx\n[[/code]]\n[[/collapsible]]\n"
            );
            let (html, _, _) = render(&source);
            assert_eq!(
                html.matches("class=\"collapsible-block\"").count(),
                2,
                "{tail:?}: {html}"
            );
            assert_eq!(
                html.matches("<div class=\"code\">").count(),
                1,
                "{tail:?}: {html}"
            );
        }
    }

    #[test]
    fn wikidot_spaced_and_nested_collapsible_openers_remain_literal() {
        let (html, _, errors) = render(concat!(
            "[[ collapsible hideLocation=\"both\" ]]\n",
            "spaced body\n",
            "[[/collapsible]]\n",
            "[[collapsible show=\"outer\"]]\n",
            "outer body\n",
            "[[collapsible show=\"inner\"]]\n",
            "inner body\n",
            "[[/collapsible]]\n",
            "[[/collapsible]]",
        ));

        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 1);
        assert!(html.contains("[[ collapsible hideLocation="), "{html}");
        assert!(html.contains("spaced body"), "{html}");
        assert!(
            html.contains("[[collapsible show=&quot;inner&quot;]]"),
            "{html}"
        );
        assert!(html.contains("inner body"), "{html}");
        assert!(html.contains("[[/collapsible]]"), "{html}");
        assert!(!errors.is_empty());
    }

    #[test]
    fn quoted_raw_markers_retain_physical_line_breaks_in_collapsible_body() {
        let (html, _, _errors) = render(concat!(
            "> [[collapsible]]\n",
            "> [[raw]]\n",
            "> raw body\n",
            "> [[/raw]]\n",
            "> [[/collapsible]]\n",
        ));

        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 1);
        assert!(
            html.contains("<p>[[raw]]<br>\nraw body<br>\n[[/raw]]</p>"),
            "{html}"
        );
    }

    #[test]
    fn repeated_unclosed_quoted_code_blocks_are_bounded() {
        let mut input = String::from("> [[collapsible show=\"show\"]]\n");
        for index in 0..512 {
            input.push_str(&format!("> [[code]]\n> readable-code-{index}\n",));
        }
        input.push_str("> [[/collapsible]]\n");

        let started = Instant::now();
        let (html, _, _errors) = render(&input);
        let elapsed = started.elapsed();

        assert!(elapsed < Duration::from_secs(5), "elapsed {elapsed:?}");
        assert_eq!(html.matches("class=\"collapsible-block\"").count(), 1);
        assert!(html.contains("[[code]]"), "{html}");
        assert!(html.contains("readable-code-0"), "{html}");
        assert!(html.contains("readable-code-511"), "{html}");
    }
}
