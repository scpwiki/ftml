/*
 * parsing/rule/impls/block/blocks/module/rule.rs
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

use super::mapping::get_module_rule_with_name;
use super::prelude::*;
use crate::parsing::rule::impls::block::parser::BlockBodyStart;
use crate::tree::{
    AnchorTarget, AttributeMap, LinkLabel, LinkLocation, LinkType, Module,
};

pub const BLOCK_MODULE: BlockRule = BlockRule {
    name: "block-module",
    accepts_names: &["module", "module654"],
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
    debug!("Parsing module block (in-head {in_head})");
    parser.check_page_syntax()?;
    assert!(!flag_star, "Module doesn't allow star flag");
    assert!(!flag_score, "Module doesn't allow score flag");
    assert_block_name(&BLOCK_MODULE, name);

    if parser.native_blockquote_depth().is_some() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.wikidot_footnote_zone_blocks(opener_start) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if parser.settings().layout.legacy() && module_opener_has_leading_space(parser) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let (subname, arguments, body_start) = parser
        .get_module_head_name_map_with_body_start_wikidot(&BLOCK_MODULE, in_head)?;

    if parser.settings().layout.legacy()
        && let Some(literal) = wikidot_extra_bracket_css_literal(parser, opener_start)
    {
        return ok!(true; text!(literal));
    }

    if parser.settings().layout.legacy() && !wikidot_valid_module_name(subname) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let resolved_module = get_module_rule_with_name(subname);
    let owns_opener_line = resolved_module.is_some_and(|rule| rule.owns_opener_line);

    let mut same_line_close_consumed = false;
    if parser.settings().layout.legacy()
        && !owns_opener_line
        && body_start == BlockBodyStart::Inline
        && !parser.wikidot_module_head_rest_is_line_end()?
    {
        let has_separator =
            parser.wikidot_module_head_has_separator(opener_start, name, subname);
        if has_separator
            && parser.consume_wikidot_module_same_line_close(&BLOCK_MODULE)?
        {
            same_line_close_consumed = true;
        } else {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
    }

    let module_rule = match resolved_module {
        Some(rule) => rule,
        None if parser.settings().layout.legacy() => {
            let has_body_end = parser.has_body_end_block(&BLOCK_MODULE);
            if arguments.is_empty()
                && body_start == BlockBodyStart::Inline
                && !same_line_close_consumed
                && parser.current().token != Token::InputEnd
            {
                let source = parser.full_text().inner();
                let owner_end = if has_body_end {
                    let _ = parser.get_body_text(&BLOCK_MODULE)?;
                    parser.current().span.start
                } else {
                    parser.current().span.start
                };
                return ok!(true; text!(&source[opener_start..owner_end]));
            }
            if has_body_end && body_start == BlockBodyStart::NextPhysicalLine {
                let body = parser.get_body_text(&BLOCK_MODULE)?;
                return ok!(false; Element::Module(Module::Runtime {
                    name: cow!(subname),
                    arguments: arguments.into_raw_vec(),
                    body,
                }));
            }
            if has_body_end {
                let _ = parser.get_body_text(&BLOCK_MODULE)?;
            }
            return ok!(false; wikidot_unknown_module_error(subname));
        }
        None => return Err(parser.make_err(ParseErrorKind::NoSuchModule)),
    };

    if same_line_close_consumed {
        parser.mark_wikidot_module_empty_body();
    } else if parser.settings().layout.legacy() && !module_rule.reads_body {
        parser.skip_wikidot_module_body(&BLOCK_MODULE);
    }

    parser.set_module(module_rule);

    let output = (module_rule.parse_fn)(parser, subname, arguments, opener_start)?;
    let (elements, errors, paragraph_safe) = output.into();
    let _ = parser.take_wikidot_module_empty_body();

    success_elements_with_paragraph_safety(paragraph_safe, elements, errors)
}

fn wikidot_valid_module_name(name: &str) -> bool {
    let mut chars = name.bytes();
    chars.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && chars.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn wikidot_extra_bracket_css_literal<'t>(
    parser: &Parser<'_, 't>,
    opener_start: usize,
) -> Option<&'t str> {
    let source = parser.full_text().inner();
    let prefix = source[opener_start..].get(.."[[module CSS]]]".len())?;
    if !prefix.eq_ignore_ascii_case("[[module CSS]]]") {
        return None;
    }

    source.get(opener_start..parser.current().span.start)
}

fn wikidot_unknown_module_error<'t>(name: &'t str) -> Element<'t> {
    let mut attributes = AttributeMap::new();
    attributes.insert("class", cow!("error-block"));
    let emphasized_name = Element::Container(Container::new(
        ContainerType::Italics,
        vec![text!(name)],
        AttributeMap::new(),
    ));
    let documentation_link = Element::Link {
        ltype: LinkType::Direct,
        link: LinkLocation::Url(cow!("https://www.wikidot.com/doc:modules")),
        label: LinkLabel::Text(cow!("check available modules")),
        target: Some(AnchorTarget::NewTab),
    };
    Element::Container(Container::new(
        ContainerType::Div,
        vec![
            text!("[[module "),
            emphasized_name,
            text!("]] No such module, please "),
            documentation_link,
            text!(" and fix this page."),
        ],
        attributes,
    ))
}

fn module_opener_has_leading_space(parser: &Parser<'_, '_>) -> bool {
    let head = &parser.full_text().inner()[..parser.current().span.start];
    head.rfind("[[").is_some_and(|start| {
        head[start + 2..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
    })
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::ParseErrorKind;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::{Element, Module};

    #[test]
    fn quoted_module_markers_remain_literal_like_wikidot() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        for input in [
            "> [[module CSS]]\n> .OMEGA_CSS_DEPTH_ONE { color: red; }\n> [[/module]]",
            ">> [[module CSS]]\n>> .OMEGA_CSS_DEPTH_TWO { color: red; }\n>> [[/module]]",
            "> > [[module CSS]]\n> > .OMEGA_CSS_SPACED_INNER { color: red; }\n> > [[/module]]",
            ">> [[module CSS]]\n>> .OMEGA_CSS_SHALLOW_CLOSE { color: red; }\n> [[/module]]\n> OMEGA_AFTER_SHALLOW",
            "> [[module CSS]]\n> .OMEGA_CSS_DEEP_CLOSE { color: red; }\n>> [[/module]]\n> OMEGA_AFTER_DEEP",
            "> [[module CSS]]\n> .OMEGA_CSS_UNCLOSED { color: red; }\n> OMEGA_QUOTED_AFTER_UNCLOSED\nOMEGA_OUTSIDE_AFTER_UNCLOSED",
            "> [[module Rate show=\"OMEGA_RATE_DEPTH_ONE\"]]",
            ">> [[module Rate show=\"OMEGA_RATE_DEPTH_TWO\"]]",
            "> [[module CountPages category=\"OMEGA_COUNT_DEPTH_ONE\"]]",
        ] {
            let tokenization = crate::tokenize(input);
            let (tree, _errors) =
                crate::parse(&tokenization, &page_info, &settings).into();

            let debug = format!("{tree:?}");
            assert!(!debug.contains("Style("), "{input:?}: {debug}");
            assert!(!debug.contains("Module("), "{input:?}: {debug}");
            assert!(debug.contains("Text(\"module\")"), "{input:?}: {debug}");
        }
    }

    #[test]
    fn wikidot_leading_space_module_openers_remain_literal() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        for input in [
            "[[ module BACKlinks ]]",
            "[[ module Rate ]]",
            "[[ module Categories ]]",
            "[[ module Join ]]",
            "[[ module PageTree ]]",
            "[[ module CSS ]]",
        ] {
            let tokenization = crate::tokenize(input);
            let (tree, _errors) =
                crate::parse(&tokenization, &page_info, &settings).into();

            let debug = format!("{tree:?}");
            assert!(!debug.contains("Module("), "{input:?}: {debug}");
            assert!(debug.contains("Text(\"module\")"), "{input:?}: {debug}");
        }
    }

    #[test]
    fn wikijump_layout_keeps_leading_space_module_extension() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tokenization = crate::tokenize("[[ module Rate ]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(format!("{tree:?}").contains("Module(Rate)"));
    }

    #[test]
    fn wikidot_unknown_module_renders_the_live_error_block() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[module NoSuchModuleWithThisName]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            HtmlRender.render(&tree, &page_info, &settings).body,
            concat!(
                r#"<div class="error-block">[[module <em>NoSuchModuleWithThisName</em>]] No such module, please "#,
                r#"<a href="https://www.wikidot.com/doc:modules" target="_blank">check available modules</a>"#,
                " and fix this page.</div>",
            ),
        );
    }

    #[test]
    fn wikidot_unknown_inline_body_remains_literal() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization =
            crate::tokenize("[[module UnknownOracleModule]]preserved body[[/module]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(
            errors
                .iter()
                .any(|error| error.kind() == ParseErrorKind::RuleFailed),
            "{errors:#?}",
        );
        assert_eq!(
            HtmlRender.render(&tree, &page_info, &settings).body,
            "<p>[[module UnknownOracleModule]]preserved body[[/module]]</p>",
        );
    }

    #[test]
    fn wikidot_unknown_module_boundary_matches_live_source_shapes() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        for (input, literal) in [
            ("[[module UnknownOracleModule]]TAIL", true),
            ("[[module UnknownOracleModule]][[/module]]", true),
            ("[[module UnknownOracleModule]]\nbody\n[[/module]]", false),
            ("[[module UnknownOracleModule foo=\"bar\"]]", false),
        ] {
            let tokenization = crate::tokenize(input);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();

            let body = HtmlRender.render(&tree, &page_info, &settings).body;
            if literal {
                assert_eq!(body, format!("<p>{input}</p>"), "{input:?}");
                assert!(
                    errors
                        .iter()
                        .any(|error| error.kind() == ParseErrorKind::RuleFailed),
                    "{input:?}: {errors:#?}",
                );
            } else {
                assert!(errors.is_empty(), "{input:?}: {errors:#?}");
                assert!(
                    body.starts_with(r#"<div class="error-block">[[module <em>UnknownOracleModule</em>]] No such module"#),
                    "{input:?}: {body}",
                );
            }
        }
    }

    #[test]
    fn wikidot_unknown_module_body_ownership_matches_live_boundaries() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let inline_tail =
            crate::tokenize("[[module UnknownOracleModule]]TAIL\n[[module RandomPage]]");
        let (tree, errors) = crate::parse(&inline_tail, &page_info, &settings).into();
        assert!(
            errors
                .iter()
                .any(|error| error.kind() == ParseErrorKind::RuleFailed),
            "{errors:#?}",
        );
        assert!(
            HtmlRender
                .render(&tree, &page_info, &settings)
                .body
                .contains("[[module UnknownOracleModule]]TAIL</p>")
        );
        assert!(
            HtmlRender
                .render(&tree, &page_info, &settings)
                .body
                .contains("[[module <em>RandomPage</em>]] No such module")
        );

        let newline_unclosed = crate::tokenize(
            "[[module UnknownOracleModule]]\nbody\n[[module RandomPage]]",
        );
        let (tree, errors) =
            crate::parse(&newline_unclosed, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            HtmlRender.render(&tree, &page_info, &settings).body,
            concat!(
                r#"<div class="error-block">[[module <em>UnknownOracleModule</em>]] No such module, please "#,
                r#"<a href="https://www.wikidot.com/doc:modules" target="_blank">check available modules</a>"#,
                " and fix this page.</div>",
                "<p>body</p>",
                r#"<div class="error-block">[[module <em>RandomPage</em>]] No such module, please "#,
                r#"<a href="https://www.wikidot.com/doc:modules" target="_blank">check available modules</a>"#,
                " and fix this page.</div>",
            ),
        );
    }

    #[test]
    fn wikidot_invalid_module_heads_remain_literal() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        for (input, expected_body) in [
            (
                r#"[[module foo="bar"]]"#,
                r#"<p>[[module foo=&quot;bar&quot;]]</p>"#,
            ),
            (
                "[[module https://example.com]]",
                r#"<p>[[module <a href="https://example.com">https://example.com</a>]]</p>"#,
            ),
        ] {
            let tokenization = crate::tokenize(input);
            let (tree, _errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            assert_eq!(
                HtmlRender.render(&tree, &page_info, &settings).body,
                expected_body
            );
        }
    }

    #[test]
    fn wikidot_module654_keeps_the_live_name_boundary() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let valid_unknown = crate::tokenize("[[module654 UnknownOracleModule]]");
        let (tree, errors) = crate::parse(&valid_unknown, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:#?}");
        assert!(HtmlRender.render(&tree, &page_info, &settings)
            .body
            .starts_with(
                r#"<div class="error-block">[[module <em>UnknownOracleModule</em>]] No such module"#,
            ));

        let invalid =
            crate::tokenize("[[module654 class=\"\"]]\nv7 body\n[[/module654]]");
        let (tree, _errors) = crate::parse(&invalid, &page_info, &settings).into();
        assert_eq!(
            HtmlRender.render(&tree, &page_info, &settings).body,
            "<p>[[module654 class=&quot;&quot;]]<br>\nv7 body<br>\n[[/module654]]</p>",
        );
    }

    #[test]
    fn wikidot_module_alias_closers_do_not_cross() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        for opener in ["module", "module654"] {
            let source = format!(
                "Before\n[[{opener} CSS]]\n.parity-gap {{ color: red; }}\n[[/module654]]\nAfter\n",
            );
            let tokenization = crate::tokenize(&source);
            let (tree, _errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            assert_eq!(
                HtmlRender.render(&tree, &page_info, &settings).body,
                concat!(
                    "<p>Before</p>",
                    "<p>.parity-gap { color: red; }<br>\n",
                    "[[/module654]]<br>\n",
                    "After</p>",
                ),
                "{opener}",
            );
        }
    }

    #[test]
    fn module654_closer_policy_is_layout_specific() {
        let page_info = PageInfo::dummy();
        for (layout, closer) in
            [(Layout::Wikidot, "module"), (Layout::Wikijump, "module654")]
        {
            let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
            let source = format!(
                "[[module654 CSS]]\n.parity-gap {{ color: blue; }}\n[[/{closer}]]",
            );
            let tokenization = crate::tokenize(&source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();

            assert!(errors.is_empty(), "{layout:?}: {errors:#?}");
            assert!(
                tree.elements
                    .iter()
                    .any(|element| matches!(element, Element::Style(_))),
                "{layout:?}: {tree:#?}",
            );
        }
    }

    #[test]
    fn wikijump_unknown_module_remains_literal() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tokenization = crate::tokenize("[[module NoSuchModuleWithThisName]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(!errors.is_empty());
        assert_eq!(
            HtmlRender.render(&tree, &page_info, &settings).body,
            "<p>[[module NoSuchModuleWithThisName]]</p>",
        );
    }

    #[test]
    fn wikidot_module_failure_fixture_recovers_without_changing_backlinks() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = concat!(
            "[[module NoSuchModuleWithThisName]]\n\n",
            "[[module backlinks invalid=\"argument\"]]\n\n",
            "[[module CSS]]",
        );
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(tree.elements.len(), 2);
        assert!(matches!(
            &tree.elements[1],
            Element::Module(Module::Backlinks { page: None }),
        ));
    }
}
