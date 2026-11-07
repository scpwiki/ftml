/*
 * parsing/rule/impls/block/blocks/module/modules/css.rs
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

pub const MODULE_CSS: ModuleRule = ModuleRule {
    name: "module-css",
    accepts_names: &["CSS"],
    parse_fn: parse_with_wikidot_boundary,
    reads_body: true,
    owns_opener_line: true,
};

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    _arguments: Arguments<'t>,
) -> ParseResult<'r, 't, ModuleParseOutput<'t>> {
    debug!("Parsing categories module");
    assert_module_name(&MODULE_CSS, name);

    if parser.settings().layout.legacy() && parser.current().token == Token::InputEnd {
        return ok!(false; ModuleParseOutput::None);
    }
    let css = parser.get_body_text(&BLOCK_MODULE)?;
    let element = Element::Style(css);
    success_value(element.into(), Vec::new(), false)
}

fn parse_with_wikidot_boundary<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    arguments: Arguments<'t>,
    opener_start: usize,
) -> ParseResult<'r, 't, ModuleParseOutput<'t>> {
    assert_module_name(&MODULE_CSS, name);

    if parser.settings().layout.legacy() {
        if parser.take_wikidot_module_empty_body() {
            return ok!(false; ModuleParseOutput::None);
        }
        if parser.current().token == Token::InputEnd {
            return parse_fn(parser, name, arguments);
        }
        if !wikidot_css_body_starts_on_new_line(parser) {
            let source = parser.full_text().inner();
            let _ = parser.get_body_text(&BLOCK_MODULE)?;
            if arguments.has_source() {
                return ok!(false; ModuleParseOutput::None);
            }
            let owner_end = parser.current().span.start;
            return success_value(
                ModuleParseOutput::Element(text!(&source[opener_start..owner_end])),
                Vec::new(),
                true,
            );
        }

        let body_start = parser.current().span.start;
        let source = parser.full_text().inner();
        let mut body_probe = parser.clone();
        match body_probe.get_body_text(&BLOCK_MODULE) {
            Ok(body) => {
                if body.chars().all(char::is_whitespace) {
                    let _ = parser.get_body_text(&BLOCK_MODULE)?;
                    let owner_end = parser.current().span.start;
                    let closer = wikidot_css_closer(source, body_start, owner_end)
                        .expect("empty closed CSS body has a module closer");
                    return success_value(
                        ModuleParseOutput::Element(text!(closer)),
                        Vec::new(),
                        true,
                    );
                }

                if !wikidot_list_pages_head_offsets(&body).is_empty() {
                    match wikidot_css_list_pages_boundary(parser) {
                        Some(CssListPagesBoundary::Unclosed) => {
                            return ok!(false; ModuleParseOutput::None);
                        }
                        Some(CssListPagesBoundary::Closed { nested_end_blocks }) => {
                            let css = parser.get_body_text_after_skipping_end_blocks(
                                &BLOCK_MODULE,
                                nested_end_blocks,
                            )?;
                            return success_value(
                                Element::Style(css).into(),
                                Vec::new(),
                                false,
                            );
                        }
                        None => {}
                    }
                }
            }
            Err(error) if error.kind() == ParseErrorKind::EndOfInput => {
                return ok!(false; ModuleParseOutput::Element(text!("")));
            }
            Err(_) => {}
        }
    }

    parse_fn(parser, name, arguments)
}

fn wikidot_css_closer(source: &str, body_start: usize, owner_end: usize) -> Option<&str> {
    let body = &source[body_start..owner_end];
    let mut cursor = 0;
    while let Some(relative_start) = body[cursor..].find("[[/") {
        let closer_start = cursor + relative_start;
        let tail = &body[closer_start + 3..];
        let tail = tail.trim_start_matches([' ', '\t']);
        let name = tail.get(.."module".len())?;
        if name.eq_ignore_ascii_case("module") {
            let after_name = &tail["module".len()..];
            if after_name.starts_with("]]") || after_name.starts_with([' ', '\t']) {
                let close = body[closer_start..].find("]]")?;
                return source.get(
                    body_start + closer_start..body_start + closer_start + close + 2,
                );
            }
        }
        cursor = closer_start + 3;
    }
    None
}

fn wikidot_css_body_starts_on_new_line(parser: &Parser<'_, '_>) -> bool {
    let source = parser.full_text().inner();
    let source_before_body = &source[..parser.current().span.start];
    let Some(opener_close) = source_before_body.rfind("]]") else {
        return false;
    };
    source[opener_close + 2..].starts_with(['\r', '\n'])
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CssListPagesBoundary {
    Unclosed,
    Closed { nested_end_blocks: usize },
}

fn wikidot_css_list_pages_boundary<'r, 't>(
    parser: &Parser<'r, 't>,
) -> Option<CssListPagesBoundary>
where
    'r: 't,
{
    let mut body_probe = parser.clone();
    let mut scan = CssListPagesScan::new();

    loop {
        match scan.advance(&mut body_probe) {
            CssListPagesStep::Continue => {}
            CssListPagesStep::Boundary(boundary) => return boundary,
        }
    }
}

struct CssListPagesScan {
    first: bool,
    open_list_pages: usize,
    nested_end_blocks: usize,
}

impl CssListPagesScan {
    fn new() -> Self {
        CssListPagesScan {
            first: true,
            open_list_pages: 0,
            nested_end_blocks: 0,
        }
    }
}

enum CssListPagesStep {
    Continue,
    Boundary(Option<CssListPagesBoundary>),
}

impl CssListPagesScan {
    fn advance<'r, 't>(&mut self, body_probe: &mut Parser<'r, 't>) -> CssListPagesStep
    where
        'r: 't,
    {
        if self.nested_end_blocks > 0
            && self.open_list_pages == 0
            && wikidot_parser_starts_module(body_probe, "css")
        {
            return CssListPagesStep::Boundary(Some(CssListPagesBoundary::Unclosed));
        }

        if wikidot_parser_starts_module(body_probe, "listpages") {
            self.open_list_pages += 1;
        }

        if body_probe.consume_body_end_block(self.first, &BLOCK_MODULE) {
            if self.open_list_pages > 0 {
                self.open_list_pages -= 1;
                self.nested_end_blocks += 1;
                self.first = false;
                return CssListPagesStep::Continue;
            }
            return CssListPagesStep::Boundary(Some(CssListPagesBoundary::Closed {
                nested_end_blocks: self.nested_end_blocks,
            }));
        }

        if body_probe.step().is_err() {
            return CssListPagesStep::Boundary(
                (self.open_list_pages == 0 && self.nested_end_blocks > 0)
                    .then_some(CssListPagesBoundary::Unclosed),
            );
        }
        self.first = false;
        CssListPagesStep::Continue
    }
}

fn wikidot_parser_starts_module(parser: &Parser<'_, '_>, module_name: &str) -> bool {
    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    if start > 0 && !source[..start].ends_with('\n') {
        return false;
    }
    let line = source[start..]
        .split_once('\n')
        .map_or(&source[start..], |(line, _)| line);
    wikidot_line_starts_module(line, module_name)
}

fn wikidot_list_pages_head_offsets(body: &str) -> Vec<usize> {
    let mut offset = 0;
    let mut offsets = Vec::new();

    for line in body.split_inclusive('\n') {
        if wikidot_line_starts_module(line, "listpages") {
            offsets.push(offset);
        }
        offset += line.len();
    }

    offsets
}

fn wikidot_line_starts_module(line: &str, module_name: &str) -> bool {
    let Some(head) = line.get(.."[[module".len()) else {
        return false;
    };
    if !head.eq_ignore_ascii_case("[[module") {
        return false;
    }
    let head = &line["[[module".len()..];
    if !head.starts_with([' ', '\t']) {
        return false;
    }
    let head = head.trim_start_matches([' ', '\t']);
    let Some(name) = head.get(..module_name.len()) else {
        return false;
    };
    if !name.eq_ignore_ascii_case(module_name) {
        return false;
    }
    let rest = &head[module_name.len()..];
    rest.starts_with([']', ' ', '\t'])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::render::{html::HtmlRender, text::TextRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn css_module_body_stays_raw_and_disable_argument_is_ignored() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tokenization = crate::tokenize(
            "[[module CSS show=\"head\" disable=\"true\"]]\n.raw { --literal: \"[[*bold]] [[span]]\"; }\n[[/module]]",
        );
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            tree.elements,
            vec![Element::Style(cow!(
                ".raw { --literal: \"[[*bold]] [[span]]\"; }"
            ))],
        );

        let output = HtmlRender.render(&tree, &page_info, &settings);
        assert_eq!(output.body, "");
        assert_eq!(
            output.styles,
            vec![".raw{--literal:\"[[*bold]] [[span]]\"}".to_owned()],
        );
    }

    #[test]
    fn repeated_css_module_body_renders_like_independent_modules() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let module = "[[module CSS]]\n.same { color: red; }\n[[/module]]";
        let repeated_source = format!("{module}\n{module}");

        let tokenization = crate::tokenize(&repeated_source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:?}");
        let repeated = HtmlRender.render(&tree, &page_info, &settings);

        let tokenization = crate::tokenize(module);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:?}");
        let independent = HtmlRender.render(&tree, &page_info, &settings);

        assert_eq!(repeated.body, independent.body);
        assert_eq!(
            repeated.styles,
            vec![independent.styles[0].clone(), independent.styles[0].clone()],
        );
    }

    #[test]
    fn wikidot_unclosed_empty_css_module_is_not_displayed() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[module CSS]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let output = HtmlRender.render(&tree, &page_info, &settings);

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(tree.elements.is_empty());
        assert!(output.body.is_empty());
        assert!(output.styles.is_empty());
    }

    #[test]
    fn wikidot_unclosed_css_module_does_not_consume_following_blocks() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(concat!(
            "[[module CSS]]\n\n",
            "[[module ListPages name=\"=\"]]\n",
            "%%title%%\n",
            "[[/module]]",
        ));
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let output = HtmlRender.render(&tree, &page_info, &settings);

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            tree.elements,
            vec![Element::Module(crate::tree::Module::ListPages {
                arguments: vec![crate::tree::RawModuleArgument {
                    name: cow!("name"),
                    value: cow!("="),
                }],
                body: cow!("%%title%%"),
            })],
        );
        assert!(output.styles.is_empty());
    }

    #[test]
    fn wikidot_unclosed_css_yields_to_list_pages_after_a_css_comment_open() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(concat!(
            "[[module CSS]]\n",
            "/*\n",
            "[[module ListPages name=\"=\"]]\n",
            "*/\n",
            "[[/module]]",
        ));
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let output = HtmlRender.render(&tree, &page_info, &settings);

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(tree.elements.iter().any(|element| {
            matches!(
                element,
                Element::Module(crate::tree::Module::ListPages { body, .. })
                    if body == "*/"
            )
        }));
        assert!(output.styles.is_empty());
    }

    #[test]
    fn wikidot_unclosed_css_yields_to_multiple_complete_list_pages_modules() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(concat!(
            "[[module CSS]]\n",
            "[[module ListPages name=\"missing-one\"]]\n",
            "ONE\n",
            "[[/module]]\n",
            "[[module ListPages name=\"missing-two\"]]\n",
            "TWO\n",
            "[[/module]]",
        ));
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let output = HtmlRender.render(&tree, &page_info, &settings);

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            tree.elements
                .iter()
                .filter(|element| {
                    matches!(
                        element,
                        Element::Module(crate::tree::Module::ListPages { .. })
                    )
                })
                .count(),
            2,
        );
        assert!(output.styles.is_empty());
    }

    #[test]
    fn wikidot_repeated_unclosed_css_modules_preserve_each_list_pages() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let unit = concat!(
            "[[module CSS]]\n",
            "[[module ListPages name=\"missing\"]]\n",
            "BODY\n",
            "[[/module]]\n",
        );
        let source = unit.repeat(128);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            tree.elements
                .iter()
                .filter(|element| {
                    matches!(
                        element,
                        Element::Module(crate::tree::Module::ListPages { .. })
                    )
                })
                .count(),
            128,
        );
        assert!(
            !tree
                .elements
                .iter()
                .any(|element| matches!(element, Element::Style(_)))
        );
    }

    #[test]
    fn wikidot_unclosed_css_does_not_consume_a_live_malformed_list_pages_head() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(concat!(
            "[[module CSS]]\n",
            "[[module ListPages name=\"=\n",
            "%%title%%\n",
            "[[/module]]",
        ));
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let output = HtmlRender.render(&tree, &page_info, &settings);

        assert!(!errors.is_empty());
        assert!(
            !tree
                .elements
                .iter()
                .any(|element| matches!(element, Element::Style(_)))
        );
        assert!(output.body.contains("[[module ListPages"));
        assert!(output.styles.is_empty());
    }

    #[test]
    fn wikidot_closed_css_keeps_nested_list_pages_markers_raw() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(concat!(
            "[[module CSS]]\n",
            "[[module ListPages name=\"missing-one\"]]\n",
            "ONE\n",
            "[[/module]]\n",
            "[[module ListPages name=\"missing-two\"]]\n",
            "TWO\n",
            "[[/module]]\n",
            "[[/module]]",
        ));
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let output = HtmlRender.render(&tree, &page_info, &settings);

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(matches!(tree.elements.as_slice(), [Element::Style(_)]));
        assert!(!format!("{tree:?}").contains("Module(ListPages"));
        assert!(output.body.is_empty());
    }

    #[test]
    fn wikidot_closed_css_module_keeps_following_list_pages_independent() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(concat!(
            "[[module CSS]]\n",
            ".closed { color: red; }\n",
            "[[/module]]\n\n",
            "[[module ListPages name=\"=\"]]\n",
            "%%title%%\n",
            "[[/module]]",
        ));
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let output = HtmlRender.render(&tree, &page_info, &settings);

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(matches!(
            tree.elements.as_slice(),
            [
                Element::Style(_),
                Element::Module(crate::tree::Module::ListPages { .. })
            ],
        ));
        assert_eq!(output.styles, [".closed{color:red}"]);
    }

    #[test]
    fn wikidot_unclosed_css_recovery_requires_a_root_list_pages_head() {
        for body in ["[[module ListPages]]", "[[MODULE \t LISTPAGES name=\"=\"]]"] {
            assert_eq!(wikidot_list_pages_head_offsets(body), [0], "{body:?}");
        }
        for body in [
            " [[module ListPages]]",
            "\t[[module ListPages]]",
            "> [[module ListPages]]",
            "text [[module ListPages]]",
            "[[moduleListPages]]",
            "[[module ListPagesExample]]",
        ] {
            assert!(wikidot_list_pages_head_offsets(body).is_empty(), "{body:?}");
        }
    }

    #[test]
    fn wikidot_inline_css_remains_literal_without_nested_module_execution() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for source in [
            "[[module CSS]].inline { color: red; }[[/module]]",
            "[[module CSS]][[module ListPages name=\"=\"]]%%title%%[[/module]]",
        ] {
            let tokenization = crate::tokenize(source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let output = HtmlRender.render(&tree, &page_info, &settings);
            let text = TextRender.render(&tree, &page_info, &settings);

            assert!(errors.is_empty(), "{source:?}: {errors:#?}");
            assert_eq!(text, source, "{source:?}: {}", output.body);
            assert!(!format!("{tree:?}").contains("Style("), "{source:?}");
            assert!(!format!("{tree:?}").contains("Module("), "{source:?}");
            assert!(output.styles.is_empty(), "{source:?}");
        }
    }

    #[test]
    fn wikijump_unclosed_empty_css_module_remains_literal() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tokenization = crate::tokenize("[[module CSS]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let output = HtmlRender.render(&tree, &page_info, &settings);

        assert!(!errors.is_empty());
        assert_eq!(output.body, "<p>[[module CSS]]</p>");
        assert!(output.styles.is_empty());
    }
}
