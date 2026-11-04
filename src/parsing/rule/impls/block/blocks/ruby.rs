/*
 * parsing/rule/impls/block/blocks/ruby.rs
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
use crate::parsing::{ParserWrap, strip_whitespace};
use crate::tree::{AcceptsPartial, AttributeMap, PartialElement, RubyText};
use std::mem;

pub const BLOCK_RUBY: BlockRule = BlockRule {
    name: "block-ruby",
    accepts_names: &["ruby"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn: parse_block,
};

pub const BLOCK_RT: BlockRule = BlockRule {
    name: "block-ruby-text",
    accepts_names: &["rt", "rubytext"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn: parse_text,
};

pub const BLOCK_RB: BlockRule = BlockRule {
    name: "block-ruby-short",
    accepts_names: &["rb", "ruby2"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: false,
    parse_fn: parse_shortcut,
};

fn take_ruby_text<'t>(
    ruby_text: &mut RubyText<'t>,
) -> (AttributeMap<'t>, Vec<Element<'t>>) {
    let taken = mem::take(ruby_text);
    (taken.attributes, taken.elements)
}

fn ruby_text_container<'t>(
    elements: Vec<Element<'t>>,
    attributes: AttributeMap<'t>,
) -> Element<'t> {
    let container = Container::new(ContainerType::RubyText, elements, attributes);
    Element::Container(container)
}

fn ruby_container<'t>(
    elements: Vec<Element<'t>>,
    attributes: AttributeMap<'t>,
) -> Element<'t> {
    let container = Container::new(ContainerType::Ruby, elements, attributes);
    Element::Container(container)
}

fn parse_shortcut_head<'r, 't>(
    parser: &Parser<'r, 't>,
    value: Option<&'t str>,
) -> Result<(&'t str, &'t str), ParseError> {
    let Some(value) = value else {
        return Err(parser.make_err(ParseErrorKind::BlockMissingArguments));
    };

    let parts = value.split('|').collect::<Vec<_>>();
    match parts.as_slice() {
        [base, ruby] => {
            let base = base.trim();
            let ruby = ruby.trim();
            if base.is_empty() || ruby.is_empty() {
                Err(parser.make_err(ParseErrorKind::BlockMalformedArguments))
            } else {
                Ok((base, ruby))
            }
        }
        _ => Err(parser.make_err(ParseErrorKind::BlockMalformedArguments)),
    }
}

fn parse_block<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing ruby block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Ruby doesn't allow star flag");
    assert!(!flag_score, "Ruby doesn't allow score flag");
    assert_block_name(&BLOCK_RUBY, name);

    let parser = &mut ParserWrap::new(parser, AcceptsPartial::Ruby);
    let arguments = parser.get_head_map(&BLOCK_RUBY, in_head)?;

    let body = parser.get_body_elements(&BLOCK_RUBY, false)?;
    let (mut elements, errors, paragraph_safe) = body.into();

    for element in &mut elements {
        if let Element::Partial(PartialElement::RubyText(ruby_text)) = element {
            let (attributes, elements) = take_ruby_text(ruby_text);
            *element = ruby_text_container(elements, attributes);
        }
    }

    #[cfg(debug_assertions)]
    assert_no_partials_after_conversion(&elements);

    strip_whitespace(&mut elements);

    let attributes = arguments.to_attribute_map(parser.settings());
    let element = ruby_container(elements, attributes);

    ok!(paragraph_safe; element, errors)
}

#[cfg(debug_assertions)]
fn assert_no_partials_after_conversion(elements: &[Element]) {
    for element in elements {
        let is_partial = matches!(element, Element::Partial(_));
        debug_assert!(!is_partial, "partial after conversion");
    }
}

fn parse_text<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing ruby text block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Ruby text doesn't allow star flag");
    assert!(!flag_score, "Ruby text doesn't allow score flag");
    assert_block_name(&BLOCK_RT, name);

    let arguments = parser.get_head_map(&BLOCK_RT, in_head)?;

    let body = parser.get_body_elements(&BLOCK_RT, false)?;
    let (mut elements, errors, paragraph_safe) = body.into();

    strip_whitespace(&mut elements);

    let attributes = arguments.to_attribute_map(parser.settings());
    let ruby_text = RubyText {
        elements,
        attributes,
    };
    let element = Element::Partial(PartialElement::RubyText(ruby_text));

    ok!(paragraph_safe; element, errors)
}

fn parse_shortcut<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing ruby shortcut block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Ruby shortcut doesn't allow star flag");
    assert!(!flag_score, "Ruby shortcut doesn't allow score flag");
    assert_block_name(&BLOCK_RB, name);

    let rule = &BLOCK_RB;
    let head = parser.get_head_value(rule, in_head, parse_shortcut_head)?;
    let (base_text, ruby_text) = head;

    let ruby_text = Element::Container(Container::new(
        ContainerType::RubyText,
        vec![text!(ruby_text)],
        AttributeMap::new(),
    ));

    let ruby = Element::Container(Container::new(
        ContainerType::Ruby,
        vec![text!(base_text), ruby_text],
        AttributeMap::new(),
    ));

    success_elements(ruby)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn ruby_block_converts_text_partials_to_containers() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tokenization = crate::tokenize(
            "[[ruby]]base[[rt class=\"annotation\"]]reading[[/rt]][[/ruby]]",
        );
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        match tree.elements.as_slice() {
            [Element::Container(paragraph)] => {
                let container = paragraph
                    .elements()
                    .iter()
                    .find_map(|element| match element {
                        Element::Container(container)
                            if container.ctype() == ContainerType::Ruby =>
                        {
                            Some(container)
                        }
                        _ => None,
                    })
                    .expect("paragraph should contain a ruby container");
                assert!(
                    container
                        .elements()
                        .iter()
                        .any(|element| matches!(element, Element::Container(inner) if inner.ctype() == ContainerType::RubyText))
                );
            }
            other => panic!("expected ruby container, got {other:?}"),
        }
    }

    #[test]
    fn ruby_shortcut_parses_and_rejects_bad_arguments() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);

        let tokenization = crate::tokenize("[[rb base | reading]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:?}");
        match tree.elements.as_slice() {
            [Element::Container(paragraph)] => assert!(paragraph
                .elements()
                .iter()
                .any(|element| matches!(
                    element,
                    Element::Container(container) if container.ctype() == ContainerType::Ruby
                ))),
            other => panic!("expected paragraph containing ruby shortcut, got {other:?}"),
        }

        for input in [
            "[[rb]]",
            "[[rb base]]",
            "[[rb a|b|c]]",
            "[[rb |reading]]",
            "[[rb base| ]]",
        ] {
            let tokenization = crate::tokenize(input);
            let (_tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();

            assert!(
                errors.iter().any(|error| matches!(
                    error.kind(),
                    ParseErrorKind::BlockMissingArguments
                        | ParseErrorKind::BlockMalformedArguments
                )),
                "{input} should report a ruby shortcut argument error: {errors:?}",
            );
        }
    }
}
