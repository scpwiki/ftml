/*
 * parsing/rule/impls/definition_list.rs
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
use crate::parsing::ParseSuccess;
use crate::parsing::collect::collect_consume_keep_split;

type SplitEscapeLayout<'t> = (Vec<Element<'t>>, Vec<(usize, bool)>, bool);
use crate::parsing::{Token, strip_whitespace};
use crate::tree::DefinitionListItem;

pub const RULE_DEFINITION_LIST: Rule = Rule {
    name: "definition-list",
    position: LineRequirement::StartOfLine,
    try_consume_fn: parse_definition_list,
};

pub const RULE_DEFINITION_LIST_SKIP_NEWLINE: Rule = Rule {
    name: "definition-list-skip-newline",
    position: LineRequirement::Any,
    try_consume_fn: skip_newline,
};

fn skip_newline<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Seeing if we skip due to an upcoming definition list");

    if parser.next_three_tokens()
        == (
            Token::LineBreak,
            Some(Token::Colon),
            Some(Token::Whitespace),
        )
    {
        let mut next_line = parser.clone();
        next_line.step()?;
        if has_definition_separator(&next_line) {
            return ok!(Elements::None);
        }
    }

    Err(parser.make_err(ParseErrorKind::RuleFailed))
}

fn parse_definition_list<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Trying to create a definition list");

    let mut items = Vec::new();
    let mut errors = Vec::new();
    let mut paragraph_safe = false;
    let mut escape: Option<SplitEscapeLayout<'t>> = None;

    loop {
        let next =
            scan_item(parser, &mut escape)?.chain(&mut errors, &mut paragraph_safe);
        match next {
            Some((item, _)) => {
                items.push(item);
                if escape.is_some() || !starts_definition_item(parser) {
                    break;
                }
            }
            None => break,
        }
    }

    if items.is_empty() {
        ok!(false; Elements::None, errors)
    } else if let Some((flat, entries, _escape_safe)) = escape {
        let mut elements = vec![Element::DefinitionList(items)];
        elements.extend(flat);
        let mut result = ParseSuccess::new(Elements::Multiple(elements), errors, false);
        let mut full = vec![(1, false)];
        full.extend(entries);
        result.paragraph_split = Some(full);
        Ok(result)
    } else {
        ok!(Element::DefinitionList(items))
    }
}

fn scan_item<'r, 't>(
    parser: &mut Parser<'r, 't>,
    escape: &mut Option<SplitEscapeLayout<'t>>,
) -> ParseResult<'r, 't, Option<(DefinitionListItem<'t>, bool)>>
where
    'r: 't,
{
    trace!("Trying to parse a definition list item pair");

    let mut errors = Vec::new();
    let mut paragraph_safe = false;

    if !parser.start_of_line() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if !starts_definition_item(parser) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    parser.step()?;
    let prefix_ws_extra = parser.current().slice.chars().count() > 1;
    parser.step()?;

    let restore_line = if parser.settings().layout.legacy() {
        let previous = parser.in_wikidot_definition_list_line();
        parser.set_in_wikidot_definition_list_line(true);
        Some(previous)
    } else {
        None
    };
    let scan =
        collect_key_scanning(parser, prefix_ws_extra, &mut errors, &mut paragraph_safe);
    let parts = match scan {
        Ok(KeyScan::Separator {
            key_elements,
            key_string,
        }) => {
            let (value_elements, last, value_escape) =
                collect_value(parser, &mut errors, &mut paragraph_safe)?;
            if let Some(value_escape) = value_escape {
                *escape = Some(value_escape.into_layout());
            }
            Some((key_elements, key_string, value_elements, last))
        }
        Ok(KeyScan::Residue) => None,
        Err(error) => {
            if let Some(previous) = restore_line {
                parser.set_in_wikidot_definition_list_line(previous);
            }
            return Err(error);
        }
    };
    if let Some(previous) = restore_line {
        parser.set_in_wikidot_definition_list_line(previous);
    }
    let Some((key_elements, key_string, value_elements, last)) = parts else {
        return ok!(false; None, errors);
    };

    let should_break = matches!(last.token, Token::ParagraphBreak | Token::InputEnd);

    let item = DefinitionListItem {
        key_string,
        key_elements,
        value_elements,
    };

    ok!(false; Some((item, should_break)), errors)
}

#[cfg(test)]
fn parse_item<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, (DefinitionListItem<'t>, bool)> {
    let mut errors = Vec::new();
    let mut paragraph_safe = false;

    let item = scan_item(parser, &mut None)?.chain(&mut errors, &mut paragraph_safe);
    match item {
        Some(pair) => ok!(false; pair, errors),
        None => Err(parser.make_err(ParseErrorKind::RuleFailed)),
    }
}

fn starts_definition_item<'r, 't>(parser: &Parser<'r, 't>) -> bool {
    parser.next_two_tokens() == (Token::Colon, Some(Token::Whitespace))
}

fn has_definition_separator<'r, 't>(parser: &Parser<'r, 't>) -> bool {
    if !starts_definition_item(parser) {
        return false;
    }

    let mut cursor = parser.clone();
    if cursor.step_n(2).is_err() {
        return false;
    }
    if cursor.current().token == Token::Colon {
        if cursor.step().is_err() || cursor.get_optional_space().is_err() {
            return false;
        }
        return !matches!(
            cursor.current().token,
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
        );
    }
    loop {
        if cursor.next_two_tokens() == (Token::Whitespace, Some(Token::Colon)) {
            return cursor.step_n(2).is_ok()
                && !matches!(
                    cursor.current().token,
                    Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
                );
        }
        if matches!(
            cursor.current().token,
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
        ) || cursor.step().is_err()
        {
            return false;
        }
    }
}

enum KeyScan<'t> {
    Separator {
        key_elements: Vec<Element<'t>>,
        key_string: std::borrow::Cow<'t, str>,
    },
    Residue,
}

fn collect_key_scanning<'r, 't>(
    parser: &mut Parser<'r, 't>,
    prefix_ws_extra: bool,
    errors: &mut Vec<ParseError>,
    paragraph_safe: &mut bool,
) -> Result<KeyScan<'t>, ParseError>
where
    'r: 't,
{
    let start_token = parser.current();
    let mut key_elements = Vec::new();
    let mut skipped_breaks: Vec<(usize, usize)> = Vec::new();
    let mut crossed = false;

    if prefix_ws_extra && parser.current().token == Token::Colon {
        let mut peek = parser.clone();
        peek.step()?;
        if peek.current().token == Token::Whitespace {
            parser.step()?;
            return Ok(KeyScan::Separator {
                key_elements: Vec::new(),
                key_string: std::borrow::Cow::Borrowed(""),
            });
        }
    }

    loop {
        let closes = [
            ParseCondition::token_pair(Token::Whitespace, Token::Colon),
            ParseCondition::current(Token::LineBreak),
            ParseCondition::current(Token::ParagraphBreak),
            ParseCondition::current(Token::InputEnd),
        ];
        let collected =
            collect_consume_before(parser, RULE_DEFINITION_LIST, &closes, &[], None)?;
        let (chunk, last) = collected.chain(errors, paragraph_safe);
        key_elements.extend(chunk);

        match last.token {
            Token::Whitespace => {
                let after_colon = {
                    let mut peek = parser.clone();
                    peek.step_n(2)?;
                    peek.current().token
                };
                if after_colon == Token::Whitespace {
                    let key_string = build_key_string(
                        parser,
                        start_token,
                        parser.current(),
                        &skipped_breaks,
                        crossed,
                    );
                    trim_key_elements(parser, &mut key_elements, crossed);
                    parser.step_n(2)?;
                    return Ok(KeyScan::Separator {
                        key_elements,
                        key_string,
                    });
                }

                let (ws_chunk, _) = collect_consume_before(
                    parser,
                    RULE_DEFINITION_LIST,
                    &[ParseCondition::current(Token::Colon)],
                    &[],
                    None,
                )?
                .chain(errors, paragraph_safe);
                key_elements.extend(ws_chunk);
            }
            Token::LineBreak | Token::ParagraphBreak => {
                let mut peek = parser.clone();
                peek.step()?;
                if !starts_definition_item(&peek) {
                    if last.token == Token::LineBreak {
                        parser.step()?;
                    }
                    return Ok(KeyScan::Residue);
                }

                crossed = true;
                if last.token == Token::LineBreak {
                    key_elements.push(Element::LineBreak);
                } else {
                    skipped_breaks
                        .push((parser.current().span.start, parser.current().span.end));
                }
                parser.step()?;
                key_elements.push(Element::Text(parser.current().slice.into()));
                parser.step()?;
            }
            Token::InputEnd => return Ok(KeyScan::Residue),
            _ => return Err(parser.make_err(ParseErrorKind::RuleFailed)),
        }
    }
}

fn build_key_string<'t>(
    parser: &Parser<'_, 't>,
    start_token: &ExtractedToken<'t>,
    end_token: &ExtractedToken<'t>,
    skipped_breaks: &[(usize, usize)],
    crossed: bool,
) -> std::borrow::Cow<'t, str> {
    let raw = parser.full_text().slice_partial(start_token, end_token);
    if skipped_breaks.is_empty() {
        let trimmed = if crossed || !parser.settings().layout.legacy() {
            raw.trim()
        } else {
            raw.trim_matches([' ', '\t'])
        };
        return std::borrow::Cow::Borrowed(trimmed);
    }

    let base = start_token.span.start;
    let mut built = String::new();
    let mut cursor = 0;
    for &(span_start, span_end) in skipped_breaks {
        built.push_str(&raw[cursor..span_start - base]);
        cursor = span_end - base;
    }
    built.push_str(&raw[cursor..]);
    std::borrow::Cow::Owned(built.trim().to_owned())
}

fn trim_key_elements(
    parser: &Parser<'_, '_>,
    elements: &mut Vec<Element<'_>>,
    crossed: bool,
) {
    fn strip(parser: &Parser<'_, '_>, elements: &mut Vec<Element<'_>>) {
        if parser.settings().layout.legacy() {
            strip_wikidot_ascii_padding(elements);
        } else {
            strip_whitespace(elements);
        }
    }

    strip(parser, elements);
    if crossed {
        while matches!(elements.first(), Some(Element::LineBreak)) {
            elements.remove(0);
        }
        while matches!(elements.last(), Some(Element::LineBreak)) {
            elements.pop();
        }
        strip(parser, elements);
    }
}

fn collect_value<'r, 't>(
    parser: &mut Parser<'r, 't>,
    errors: &mut Vec<ParseError>,
    paragraph_safe: &mut bool,
) -> Result<
    (
        Vec<Element<'t>>,
        &'r ExtractedToken<'t>,
        Option<crate::parsing::collect::CollectSplitEscape<'t>>,
    ),
    ParseError,
>
where
    'r: 't,
{
    let close = [
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::current(Token::LineBreak),
        ParseCondition::current(Token::InputEnd),
    ];
    let rule = RULE_DEFINITION_LIST;

    let collected = collect_consume_keep_split(parser, rule, &close, &[], None)?;
    let (mut value_elements, last, escape) = collected.chain(errors, paragraph_safe);

    if parser.settings().layout.legacy() {
        strip_wikidot_ascii_padding(&mut value_elements);
    } else {
        strip_whitespace(&mut value_elements);
    }
    Ok((value_elements, last, escape))
}

fn strip_wikidot_ascii_padding(elements: &mut Vec<Element<'_>>) {
    while matches!(elements.first(), Some(Element::Text(text)) if text.chars().all(|ch| matches!(ch, ' ' | '\t')))
    {
        elements.remove(0);
    }
    while matches!(elements.last(), Some(Element::Text(text)) if text.chars().all(|ch| matches!(ch, ' ' | '\t')))
    {
        elements.pop();
    }
}

#[test]
fn wikidot_definition_lists_leave_adjacent_prose_unwrapped() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    let source = "ALPHA\n: Key : Value\n: **Apple** : //Banana// split\n\nBETA\n: Pineapple : Very long";
    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize(source);
    let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;

    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(
        html,
        "ALPHA<br>\n<dl>\n<dt>Key</dt>\n<dd>Value</dd>\n<dt><strong>Apple</strong></dt>\n<dd><em>Banana</em> split</dd>\n</dl>\nBETA<br>\n<dl>\n<dt>Pineapple</dt>\n<dd>Very long</dd>\n</dl>\n",
    );
}

#[cfg(test)]
#[derive(Debug)]
struct TestLogger;

#[cfg(test)]
impl log::Log for TestLogger {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, _record: &log::Record<'_>) {}

    fn flush(&self) {}
}

#[cfg(test)]
static TEST_LOGGER: TestLogger = TestLogger;

#[cfg(test)]
static TEST_LOGGER_INIT: std::sync::Once = std::sync::Once::new();

#[cfg(test)]
fn enable_test_logging() {
    TEST_LOGGER_INIT.call_once(|| {
        let _ = log::set_logger(&TEST_LOGGER);
        log::set_max_level(log::LevelFilter::Trace);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn definition_list_stops_before_following_paragraph() {
        enable_test_logging();

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(": Key : Value\nPlain paragraph");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty());
        assert_eq!(tree.elements.len(), 2);

        match &tree.elements[0] {
            Element::DefinitionList(items) => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].key_string, "Key");
                assert_eq!(items[0].key_elements, [text!("Key")]);
                assert_eq!(items[0].value_elements, [text!("Value")]);
            }
            other => panic!("expected definition list, got {other:?}"),
        }

        match &tree.elements[1] {
            Element::Container(container) => {
                assert_eq!(
                    container.elements(),
                    &[text!("Plain"), text!(" "), text!("paragraph")],
                );
            }
            other => panic!("expected following paragraph, got {other:?}"),
        }
    }

    #[test]
    fn definition_list_collects_multiple_items_through_input_end() {
        enable_test_logging();

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(": Alpha : One\n: Beta : Two");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty());
        assert_eq!(tree.elements.len(), 1);

        match &tree.elements[0] {
            Element::DefinitionList(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].key_string, "Alpha");
                assert_eq!(items[0].key_elements, [text!("Alpha")]);
                assert_eq!(items[0].value_elements, [text!("One")]);
                assert_eq!(items[1].key_string, "Beta");
                assert_eq!(items[1].key_elements, [text!("Beta")]);
                assert_eq!(items[1].value_elements, [text!("Two")]);
            }
            other => panic!("expected definition list, got {other:?}"),
        }
    }

    #[test]
    fn wikidot_definition_list_keeps_an_empty_term() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(":  : Value");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:#?}");
        let [Element::DefinitionList(items)] = tree.elements.as_slice() else {
            panic!("expected one definition list, got {:#?}", tree.elements);
        };
        assert_eq!(items.len(), 1);
        assert!(items[0].key_string.is_empty());
        assert!(items[0].key_elements.is_empty());
        assert_eq!(items[0].value_elements, [text!("Value")]);
    }

    #[test]
    fn malformed_definition_item_suppresses_the_preceding_paragraph_wrapper() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("alpha\n: term");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(tree.elements, [text!("alpha"), Element::LineBreak]);
    }

    #[test]
    fn parse_item_rejects_non_definition_starts() {
        enable_test_logging();

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        let tokenization = crate::tokenize("alpha beta");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().expect("identifier should follow input start");
        parser.step().expect("whitespace should follow identifier");

        let error = parse_item(&mut parser)
            .expect_err("mid-line whitespace should not start an item");
        assert_eq!(error.kind(), ParseErrorKind::RuleFailed);

        let tokenization = crate::tokenize(":Key:Value");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().expect("colon should follow input start");
        parser.set_rule(RULE_DEFINITION_LIST);

        let error =
            parse_item(&mut parser).expect_err("missing space should reject the item");
        assert_eq!(error.kind(), ParseErrorKind::RuleFailed);
    }

    #[test]
    fn malformed_definition_lines_are_discarded_like_wikidot() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for source in [": term", ": term :", ": term:value"] {
            let tokenization = crate::tokenize(source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            assert!(errors.is_empty(), "{source:?}: {errors:?}");
            assert!(tree.elements.is_empty(), "{source:?}: {:?}", tree.elements);
        }
    }

    #[test]
    fn wikidot_definition_list_empty_value_item_is_dropped() {
        use crate::render::{Render, html::HtmlRender};

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = ": 用語 : 定義\n: 空 :\n本文\n".to_owned();
        crate::preprocess_for_layout(&mut source, settings.layout);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            html,
            "<dl>\n<dt>用語</dt>\n<dd>定義</dd>\n</dl>\n<p>本文</p>",
        );
    }

    #[test]
    fn wikidot_definition_list_value_formatting_stops_at_line_end() {
        use crate::render::{Render, html::HtmlRender};

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source =
            ": @<a:b>@ : value\n: **key : value\n: key : __value\nnext__\n".to_owned();
        crate::preprocess_for_layout(&mut source, settings.layout);
        let tokenization = crate::tokenize(&source);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert_eq!(
            html,
            concat!(
                "<dl>\n",
                "<dt><span style=\"white-space: pre-wrap;\">a:b</span></dt>\n",
                "<dd>value</dd>\n",
                "<dt>**key</dt>\n",
                "<dd>value</dd>\n",
                "<dt>key</dt>\n",
                "<dd>__value</dd>\n",
                "</dl>\n",
                "<p>next__</p>",
            ),
        );
    }
}
