/*
 * parsing/rule/impls/block/blocks/span.rs
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
use crate::delayed::DelayedElement;
use crate::parsing::inline_scope::WIKIDOT_SCORE_SPAN_UNWRAPPED_ATTRIBUTE;
use crate::parsing::strip_newlines;
use crate::tree::{PartialElement, WIKIDOT_GENERATED_EMPTY_CLASS_MARKER};
use std::borrow::Cow;

pub const BLOCK_SPAN: BlockRule = BlockRule {
    name: "block-span",
    accepts_names: &["span"],
    accepts_star: false,
    accepts_score: true,
    accepts_newlines_outside_body: false,
    parse_fn,
};

pub(super) fn wikidot_literal_with_empty_scored_spans_elided<'t>(
    source: &str,
) -> Option<Elements<'t>> {
    const EMPTY_SCORED_SPAN: &str = "[[span_]][[/span]]";

    let lowercase = source.to_ascii_lowercase();
    let mut cursor = 0;
    let mut output = String::with_capacity(source.len());
    let mut found = false;
    while let Some(relative) = lowercase[cursor..].find(EMPTY_SCORED_SPAN) {
        let start = cursor + relative;
        output.push_str(&source[cursor..start]);
        cursor = start + EMPTY_SCORED_SPAN.len();
        if source[cursor..].starts_with("\r\n") {
            cursor += 2;
        } else if source[cursor..].starts_with('\n') {
            cursor += 1;
        }
        found = true;
    }
    if !found {
        return None;
    }
    output.push_str(&source[cursor..]);

    let mut elements = Vec::new();
    for chunk in output.split_inclusive('\n') {
        let has_newline = chunk.ends_with('\n');
        let line = chunk
            .strip_suffix('\n')
            .unwrap_or(chunk)
            .strip_suffix('\r')
            .unwrap_or_else(|| chunk.strip_suffix('\n').unwrap_or(chunk));
        if !line.is_empty() {
            elements.push(Element::Text(Cow::Owned(line.to_owned())));
        }
        if has_newline {
            elements.push(Element::LineBreak);
        }
    }
    Some(Elements::Multiple(elements))
}

struct SpeculativeSpanFailureCache {
    enabled: bool,
    owner_start: usize,
}

impl SpeculativeSpanFailureCache {
    fn new(parser: &Parser<'_, '_>, owner_start: usize) -> Self {
        SpeculativeSpanFailureCache {
            enabled: !parser.settings().layout.legacy(),
            owner_start,
        }
    }

    fn replays_cached_failure(&self, parser: &Parser<'_, '_>) -> bool {
        self.enabled
            && parser.underclosed_block_failure_cached(BLOCK_SPAN.name, self.owner_start)
    }

    fn record_failure(&self, parser: &mut Parser<'_, '_>, error: &ParseError) {
        if self.enabled && error.kind() == ParseErrorKind::EndOfInput {
            parser.cache_underclosed_block_failure(BLOCK_SPAN.name, self.owner_start);
        }
    }
}

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing span block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Span doesn't allow star flag");
    assert_block_name(&BLOCK_SPAN, name);

    let speculative_failure_cache =
        SpeculativeSpanFailureCache::new(parser, opener_start);
    if speculative_failure_cache.replays_cached_failure(parser) {
        return Err(parser.make_end_of_input_err());
    }
    let generated = parser.generated_until_right_block();
    let arguments = parser.get_head_map_wikidot(&BLOCK_SPAN, in_head)?;
    let head_source = if wikidot_head_source(parser, opener_start).is_empty() {
        Cow::Borrowed("")
    } else {
        Cow::Borrowed(
            &parser.full_text().inner()[opener_start..parser.current().span.start],
        )
    };
    let scored_starts_next_physical_line = flag_score
        && matches!(
            parser.current().token,
            Token::LineBreak | Token::ParagraphBreak
        );
    if parser.settings().layout.legacy()
        && flag_score
        && !parser.wikidot_alias_has_compatible_close(&BLOCK_SPAN, opener_start)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if generated
        .iter()
        .any(|slot| slot.kind == crate::delayed::GeneratedKind::TagLinks)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let generated_attributes = generated
        .iter()
        .map(|slot| delayed_attribute_key(parser.full_text().inner(), slot))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| parser.make_err(ParseErrorKind::RuleFailed))?;
    if generated_attributes
        .iter()
        .any(|key| !matches!(*key, "class" | "title"))
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let has_close = parser.has_body_end_block(&BLOCK_SPAN)
        || parser.settings().layout.legacy()
            && has_wikidot_composite_span_close_on_line(parser);

    if parser.settings().layout.legacy() {
        if !has_close {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
        let mut attributes = arguments.to_attribute_map(parser.settings());
        super::super::arguments::retain_wikidot_html_attributes(&mut attributes);
        arguments.insert_explicit_empty_marker(&mut attributes);
        for key in generated_attributes {
            match key {
                "title" => {
                    attributes.remove("title");
                }
                "class" => {
                    attributes.insert("class", cow!(""));
                    attributes.insert(WIKIDOT_GENERATED_EMPTY_CLASS_MARKER, cow!(""));
                }
                _ => unreachable!(),
            }
        }
        if flag_score {
            attributes.insert("data-ftml-score-span", cow!(""));
            if scored_starts_next_physical_line || arguments.has_empty_key() {
                attributes.insert(WIKIDOT_SCORE_SPAN_UNWRAPPED_ATTRIBUTE, cow!(""));
            }
        }
        let leading_separator = !in_head
            && !flag_score
            && wikidot_body_leading_separator(parser, opener_start);
        let span = Element::Partial(PartialElement::InlineSpanOpen(
            attributes,
            head_source,
            leading_separator,
        ));
        if flag_score && in_head && parser.current().token == Token::Whitespace {
            parser.step()?;
        }
        parser.enter_wikidot_span_body(flag_score);
        return if generated.is_empty() {
            ok!(span)
        } else {
            ok!(Elements::Multiple(vec![
                span,
                Element::Delayed(DelayedElement::suppressed(&generated)),
            ]))
        };
    }

    let body = parser.get_body_element_segments(&BLOCK_SPAN);
    let (mut segments, errors, paragraph_safe) = match body {
        Ok(body) => body.into(),
        Err(error) => {
            speculative_failure_cache.record_failure(parser, &error);
            return Err(error);
        }
    };

    if flag_score {
        for segment in &mut segments {
            strip_newlines(segment);
        }
    }

    let (elements, paragraph_split) = parser.wrap_body_segments(
        segments,
        ContainerType::Span,
        arguments.to_attribute_map(parser.settings()),
        paragraph_safe,
    );

    let mut result =
        success_elements_with_paragraph_safety(paragraph_safe, elements, errors);
    if let Ok(success) = result.as_mut() {
        success.paragraph_split = paragraph_split;
    }
    result
}

fn wikidot_span_close_then_simple_table_delimiter_on_same_line(
    close: &mut Parser,
) -> bool {
    let text = close.full_text().inner();
    let rest_start = close.current().span.start;
    let line_end = text[rest_start..]
        .find(['\r', '\n'])
        .map(|offset| rest_start + offset)
        .unwrap_or(text.len());
    let mut index = rest_start;
    while index < line_end && text.as_bytes()[index].is_ascii_whitespace() {
        index += 1;
    }
    text.get(index..line_end)
        .is_some_and(|suffix| suffix.starts_with("||"))
}

fn has_wikidot_composite_span_close_on_line<'r, 't>(parser: &Parser<'r, 't>) -> bool
where
    'r: 't,
{
    let mut scan = parser.clone();
    let mut traversed_token_starts = Vec::new();
    loop {
        let token_start = scan.current().span.start;
        if let Some(outcome) = parser.wikidot_composite_span_close_outcome(token_start) {
            parser.cache_wikidot_composite_span_close_outcomes(
                &traversed_token_starts,
                outcome,
            );
            return outcome;
        }
        traversed_token_starts.push(token_start);
        #[cfg(test)]
        parser.increment_composite_span_close_scan_token_visits();
        if scan.skip_discarded_iftags_segment(true) {
            continue;
        }
        if scan.skip_raw_segment() {
            continue;
        }
        match scan.current().token {
            Token::LeftBlockEnd => {
                let mut close = scan.clone();
                if let Ok((name, residual)) = close.get_wikidot_end_block_with_residual()
                    && name.eq_ignore_ascii_case("span")
                    && (residual
                        || wikidot_span_close_then_simple_table_delimiter_on_same_line(
                            &mut close,
                        ))
                {
                    parser.cache_wikidot_composite_span_close_outcomes(
                        &traversed_token_starts,
                        true,
                    );
                    return true;
                }
            }
            Token::ParagraphBreak | Token::InputEnd => {
                parser.cache_wikidot_composite_span_close_outcomes(
                    &traversed_token_starts,
                    false,
                );
                return false;
            }
            Token::LineBreak => {}
            _ => {}
        }
        if scan.step().is_err() {
            parser.cache_wikidot_composite_span_close_outcomes(
                &traversed_token_starts,
                false,
            );
            return false;
        }
    }
}

fn delayed_attribute_key<'a>(
    source: &'a str,
    slot: &crate::delayed::GeneratedInput,
) -> Option<&'a str> {
    let prefix = &source[..slot.source_range.start];
    let equals = prefix.rfind('=')?;
    if !prefix[equals + 1..]
        .chars()
        .all(|character| matches!(character, '"' | '\''))
    {
        return None;
    }
    let key_start = prefix[..equals]
        .rfind(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        })
        .map_or(0, |position| position + 1);
    let key = &prefix[key_start..equals];
    (!key.is_empty()).then_some(key)
}

fn wikidot_body_leading_separator(parser: &Parser<'_, '_>, opener_start: usize) -> bool {
    if wikidot_separator_hits_segment_edge(parser) {
        return false;
    }
    let source = parser.full_text().inner();
    source[..parser.current().span.start]
        .rfind("]]")
        .map(|close| close + 2)
        .is_some_and(|body_start| {
            source[body_start..].starts_with(is_ascii_separator)
                && !follows_separator_or_line_start(&source[..opener_start])
        })
}

fn wikidot_separator_hits_segment_edge(parser: &Parser<'_, '_>) -> bool {
    match parser.current().token {
        Token::ParagraphBreak => true,
        Token::LineBreak => matches!(
            parser.look_ahead(0).map(|token| token.token),
            Some(
                Token::BulletItem
                    | Token::NumberedItem
                    | Token::TableColumn
                    | Token::TableColumnRight
                    | Token::TableColumnCenter
                    | Token::TableColumnTitle
                    | Token::Heading
                    | Token::Quote
            )
        ),
        _ => false,
    }
}

fn is_ascii_separator(ch: char) -> bool {
    matches!(ch, ' ' | '\t')
}

fn follows_separator_or_line_start(before: &str) -> bool {
    match before.chars().next_back() {
        None | Some('\n') => true,
        Some(ch) if is_ascii_separator(ch) => true,
        Some(']') => before
            .strip_suffix("]]")
            .and_then(|head| head.rfind("[[/").map(|start| &head[..start]))
            .and_then(|body| body.chars().next_back())
            .is_some_and(is_ascii_separator),
        Some(_) => false,
    }
}

pub(super) fn wikidot_head_source<'r, 't>(
    parser: &Parser<'r, 't>,
    opener_start: usize,
) -> Cow<'t, str> {
    let source = parser.full_text().inner();
    let end = parser.current().span.start;
    if opener_start > end {
        return Cow::Borrowed("");
    }
    let slice = source[opener_start..end].trim_end_matches([' ', '\t']);
    if slice.ends_with("]]") {
        Cow::Borrowed(slice)
    } else {
        Cow::Borrowed("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::Parser;
    use crate::parsing::token::Token;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn composite_span_close_scan_visits_scale_with_input_tokens() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for depth in [32, 64, 128, 256, 512] {
            let input = "[[span]]".repeat(depth);
            let tokenization = crate::tokenize(&input);
            let mut parser = Parser::new(&tokenization, &page_info, &settings);
            while parser.current().token != Token::InputEnd {
                if parser.current().token == Token::LeftBlock {
                    assert!(!has_wikidot_composite_span_close_on_line(&parser));
                }
                parser.step().expect("input end must remain available");
            }
            assert!(
                parser.composite_span_close_scan_token_visits()
                    <= tokenization.tokens().len() * 2,
                "{depth} unclosed openers visited {} tokens for {} input tokens",
                parser.composite_span_close_scan_token_visits(),
                tokenization.tokens().len(),
            );
        }
    }

    #[test]
    fn wikidot_span_close_after_deep_nesting_stays_structural() {
        let mut source = "[[span]]".repeat(300);
        source.push_str("x");
        source.push_str(&"[[/span]]".repeat(260));
        source.push_str("[[/span]] label ]");
        source.push_str(&"[[/span]]".repeat(40));
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(html.contains("[[/span]] label ]"), "{html}");
        assert!(!html.contains("<a href=\"/span"), "{html}");
    }

    #[test]
    fn wikidot_span_unmatched_close_after_full_nesting_becomes_link() {
        let mut source = "[[span]]".repeat(300);
        source.push_str("x");
        source.push_str(&"[[/span]]".repeat(300));
        source.push_str(" [[/span]] label ]");
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(html.contains("<a href=\"/span]]\">label</a>"), "{html}");
        assert!(!html.contains("[[/span</a>"), "{html}");
    }

    #[test]
    fn wikidot_span_keeps_unmatched_end_blocks_literal() {
        for (source, expected) in [
            (
                "x [[span]]y [[/foo]] w[[/span]]",
                "<p>x <span>y [[/foo]] w</span></p>",
            ),
            (
                "x [[span]]y [[/div]] w[[/span]]",
                "<p>x <span>y [[/div]] w</span></p>",
            ),
            (
                "x [[span]]y [[html]]z[[/html]] w[[/span]]",
                "<p>x <span>y [[html]]z[[/html]] w</span></p>",
            ),
        ] {
            let page_info = PageInfo::dummy();
            let settings =
                WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
            let tokenization = crate::tokenize(source);
            let (tree, _errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;
            assert_eq!(html, expected, "{source}");
        }
    }

    #[test]
    fn wikidot_span_bold_markers_pair_across_span_boundary() {
        let source = "[[span class=\"a\"]]**X[[/span]][[span class=\"b\"]]:** [[/span]]";
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(
            html,
            "<p><span class=\"a\"><strong>X</strong></span><strong><span class=\"b\">:</span></strong></p>",
        );
    }

    #[test]
    fn wikidot_span_space_before_final_close_is_dropped() {
        let source = "[[span]]x [[/span]]";
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(html, "<p><span>x</span></p>");
    }

    #[test]
    fn wikidot_span_space_before_mid_text_close_is_kept() {
        let source = "[[span]]x [[/span]] y";
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(html, "<p><span>x</span> y</p>");
    }

    #[test]
    fn wikidot_span_bold_open_inside_close_outside_pairs() {
        let source = "[[span]]**X[[/span]]Y**";
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(
            html,
            "<p><span><strong>X</strong></span><strong>Y</strong></p>"
        );
    }

    #[test]
    fn wikidot_span_unmatched_bold_marker_stays_literal() {
        let source = "[[span]]**X[[/span]]";
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(html, "<p><span>**X</span></p>");
    }

    #[test]
    fn score_span_strips_line_breaks_from_body() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization =
            crate::tokenize("[[span_ class=\"compact\"]]\nalpha\n[[/span]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        let [Element::Container(span)] = tree.elements.as_slice() else {
            panic!("expected one scored span, got {:?}", tree.elements);
        };
        assert_eq!(span.ctype(), ContainerType::Span);

        assert_eq!(span.elements(), &[text!("alpha")]);
        assert_eq!(
            span.attributes()
                .get()
                .get("class")
                .map(|value| value.as_ref()),
            Some("compact"),
        );
    }

    #[test]
    fn wikidot_score_span_joins_only_its_adjacent_paragraph_boundaries() {
        let source = "PREVIOUS\n\nEMPTY [[span_]][[/span]]\nBASIC [[span_]]durian[[/span]]\n\nNEWLINES\n\n[[span_]]eggplant\nrafflesia[[/span]]\n\n[[span_]]\nparagraph\n\nin span\n[[/span]]\n\n[[span_ id=\"this-thing\"]]span2[[/span]]";
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(
            html,
            "<p>PREVIOUS</p><p>EMPTY BASIC <span>durian</span>NEWLINES<span>eggplant<br>\nrafflesia</span><span>paragraph</span></p><span>in span</span><span id=\"u-this-thing\">span2</span>",
        );
    }

    #[test]
    fn wikidot_span_does_not_duplicate_space_across_its_close_boundary() {
        let source = "[[span]]literal [[/span]] tail";
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(html, "<p><span>literal</span> tail</p>");
    }

    #[test]
    fn wikidot_span_wraps_continuation_after_paragraph_break() {
        let source = "[[span]]mango\n\npineapple\n[[/span]]";
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(
            html,
            "<p><span>mango</span></p><p><span>pineapple<br></span></p>",
        );
    }

    #[test]
    fn wikidot_empty_span_removes_its_preceding_space() {
        let source = "EMPTY [[span]][[/span]]\nBASIC [[span]]apple[[/span]]";
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(html, "<p>EMPTY<br>\nBASIC <span>apple</span></p>");
    }

    #[test]
    fn wikidot_span_scope_resumes_without_a_paragraph_after_div() {
        let source = "[[span style=\"color: rgb(1, 2, 3);\"]]SPAN_BEFORE\n[[div class=\"span-scope-block\"]]\nSPAN_INSIDE\n[[/div]]\nSPAN_AFTER[[/span]]";
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(
            html,
            "<p><span style=\"color: rgb(1, 2, 3);\">SPAN_BEFORE</span></p><div class=\"span-scope-block\"><p><span style=\"color: rgb(1, 2, 3);\">SPAN_INSIDE</span></p></div><span style=\"color: rgb(1, 2, 3);\"><br>\nSPAN_AFTER</span>",
        );
    }

    #[test]
    fn wikidot_span_scope_stays_open_across_unsupported_inline_aliases() {
        let source = "[[b]]Nested [[span]]blocks [[bold]][[span]]even[[strong]]more[[/strong]][[/span]][[/bold]][[/span]][[/b]]";
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(
            html,
            "<p>[[b]]Nested <span>blocks [[bold]]<span>even[[strong]]more[[/strong]]</span>[[/bold]]</span>[[/b]]</p>",
        );
    }

    #[test]
    fn wikijump_underclosed_nested_spans_reuse_failure_cache() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);

        let semantic_control = "[[span]][[span]]X[[/span]]";
        let tokenization = crate::tokenize(semantic_control);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(html, "<p><span>[[span]]X</span></p>");
        assert_eq!(
            errors
                .iter()
                .map(|error| (error.rule(), error.kind()))
                .collect::<Vec<_>>(),
            Vec::new(),
        );

        for (openers, closers) in [(32, 0), (64, 32), (128, 64)] {
            let source = format!(
                "{}X{}",
                "[[span]]".repeat(openers),
                "[[/span]]".repeat(closers),
            );
            let tokenization = crate::tokenize(&source);
            let started = std::time::Instant::now();
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();

            assert!(
                started.elapsed() < std::time::Duration::from_secs(2),
                "{openers} openers / {closers} closers took {:?}",
                started.elapsed(),
            );
            assert!(!tree.elements.is_empty());
            if closers == 0 {
                assert!(
                    errors
                        .iter()
                        .any(|error| error.kind() == ParseErrorKind::EndOfInput),
                    "{errors:#?}",
                );
            } else {
                assert!(errors.is_empty(), "{errors:#?}");
            }
        }
    }

    #[test]
    fn wikidot_adjacent_span_scopes_remain_distinct_elements() {
        let source = "[[span]]one[[/span]][[span]]two[[/span]]";
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(html, "<p><span>one</span><span>two</span></p>");
    }
}
