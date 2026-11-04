/*
 * parsing/rule/impls/block/blocks/html.rs
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

use super::gallery::authored_gallery_line_offset;
use super::prelude::*;
use super::span::wikidot_literal_with_empty_scored_spans_elided;
use crate::parsing::rule::impls::block::parser::BlockBodyStart;
use std::borrow::Cow;

pub const BLOCK_HTML: BlockRule = BlockRule {
    name: "block-html",
    accepts_names: &["html"],
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
    debug!("Parsing HTML block (in-head {in_head})");
    assert!(!flag_star, "HTML doesn't allow star flag");
    assert!(!flag_score, "HTML doesn't allow score flag");
    assert_block_name(&BLOCK_HTML, name);

    if parser.settings().layout.legacy() && !parser.discarding_hidden_body() {
        if in_head {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
        let source = parser.full_text().inner();
        if opener_start > 0 && !source[..opener_start].ends_with('\n') {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
    }

    let (arguments, body_start) =
        parser.get_head_map_with_body_start(&BLOCK_HTML, in_head)?;
    if !parser.settings().enable_html_blocks {
        if parser.native_blockquote_depth().is_some() {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
        let source = parser.full_text().inner();
        let mut owner = parser.clone();
        let _ = owner.get_body_text(&BLOCK_HTML)?;
        let owner_end = owner.current().span.start;
        let generated = owner.generated_in_range(opener_start..owner_end);
        let has_runtime = std::iter::once(parser.current())
            .chain(parser.remaining())
            .take_while(|token| token.span.start < owner_end)
            .any(|token| token.token == Token::RuntimeText);
        if generated.is_empty()
            && !has_runtime
            && authored_gallery_line_offset(
                &source[opener_start..owner_end],
                parser.settings().layout.legacy(),
            )
            .is_some()
        {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
        let mut elements = if generated.is_empty() && !has_runtime {
            wikidot_literal_with_empty_scored_spans_elided(
                &source[opener_start..owner_end],
            )
            .unwrap_or_else(|| literal_elements(&source[opener_start..owner_end]))
        } else {
            literal_elements(&source[opener_start..owner_end])
        };
        parser.update(&owner);
        if source[..opener_start].ends_with('\n')
            && let Elements::Multiple(elements) = &mut elements
        {
            elements.insert(0, Element::LineBreak);
        }
        return success_elements(elements);
    }

    let body_content_start = parser.current().span.start;
    let html = parser.get_body_text(&BLOCK_HTML)?;
    let stored_html = if parser.settings().layout.legacy() {
        let leading_newline =
            body_start == BlockBodyStart::NextPhysicalLine && !html.starts_with('\n');
        let trailing_newline = matches!(html, Cow::Borrowed(_))
            && parser.full_text().inner()[body_content_start + html.len()..]
                .starts_with('\n');
        if leading_newline || trailing_newline {
            let mut stored = String::with_capacity(
                html.len() + usize::from(leading_newline) + usize::from(trailing_newline),
            );
            if leading_newline {
                stored.push('\n');
            }
            stored.push_str(&html);
            if trailing_newline {
                stored.push('\n');
            }
            Cow::Owned(stored)
        } else {
            html.clone()
        }
    } else {
        html.clone()
    };
    let element = Element::Html {
        contents: html,
        attributes: arguments.to_attribute_map(parser.settings()),
    };
    parser.push_html_block(stored_html);
    ok!(parser.settings().layout.legacy(); element)
}

fn literal_elements<'t>(source: &'t str) -> Elements<'t> {
    let mut elements = Vec::new();
    for chunk in source.split_inclusive('\n') {
        let has_newline = chunk.ends_with('\n');
        let line = chunk;
        let line = line.strip_suffix('\n').unwrap_or(line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        if !line.is_empty() {
            elements.push(text!(line));
        }
        if has_newline {
            elements.push(Element::LineBreak);
        }
    }
    if elements.is_empty() {
        elements.push(text!(source));
    }
    Elements::Multiple(elements)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn html_block_tracks_body_and_element_contents_in_wikijump() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tokenization = crate::tokenize("[[html]]\n<strong>raw</strong>\n[[/html]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        let [
            Element::Html {
                contents,
                attributes,
            },
        ] = tree.elements.as_slice()
        else {
            panic!("expected one HTML block element, got {:?}", tree.elements);
        };
        let [tracked_html] = tree.html_blocks.as_slice() else {
            panic!(
                "expected one tracked HTML block, got {:?}",
                tree.html_blocks
            );
        };

        assert_eq!(contents, "<strong>raw</strong>");
        assert!(attributes.get().is_empty());
        assert_eq!(tracked_html, "<strong>raw</strong>");
    }

    #[test]
    fn wikidot_html_block_tracks_body_for_hosted_iframe() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization =
            crate::tokenize("[[html]]\n<strong>isolated</strong>\n[[/html]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(tree.html_blocks, ["\n<strong>isolated</strong>\n"]);

        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;
        assert_eq!(
            html,
            r#"<p><iframe src="https://example.com/" allowtransparency="true" frameborder="0" class="html-block-iframe"></iframe></p>"#,
        );
    }

    #[test]
    fn wikidot_inline_html_block_does_not_gain_boundary_newlines() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[html]]<strong>inline</strong>[[/html]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(tree.html_blocks, ["<strong>inline</strong>"]);
    }

    #[test]
    fn wikidot_html_block_with_arguments_remains_literal() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[html garbage]]x[[/html]]");
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(tree.html_blocks.is_empty(), "{tree:#?}");
        assert!(!format!("{tree:?}").contains("Html {"), "{tree:#?}");
    }

    #[test]
    fn wikidot_html_blocks_can_be_kept_literal_by_the_caller() {
        let page_info = PageInfo::dummy();
        let mut settings =
            WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        settings.enable_html_blocks = false;
        let tokenization =
            crate::tokenize("[[html]]\n<strong>isolated</strong>\n[[/html]]");
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(tree.html_blocks.is_empty(), "{tree:#?}");
        assert_eq!(
            crate::render::html::HtmlRender
                .render(&tree, &page_info, &settings)
                .body,
            "<p>[[html]]<br>\n&lt;strong&gt;isolated&lt;/strong&gt;<br>\n[[/html]]</p>",
        );
    }

    #[test]
    fn disabled_wikidot_html_blocks_keep_preview_shapes_literal() {
        let page_info = PageInfo::dummy();
        let mut settings =
            WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        settings.enable_html_blocks = false;

        for (source, expected) in [
            (
                "[[html]]<b>X</b>[[/html]]",
                "<p>[[html]]&lt;b&gt;X&lt;/b&gt;[[/html]]</p>",
            ),
            (
                "BEFORE|[[html]]\n<b>X</b>\n[[/html]]|AFTER",
                "<p>BEFORE|[[html]]<br>\n&lt;b&gt;X&lt;/b&gt;<br>\n[[/html]]|AFTER</p>",
            ),
            (
                " [[html]]\n<b>X</b>\n[[/html]]",
                "<p>[[html]]<br>\n&lt;b&gt;X&lt;/b&gt;<br>\n[[/html]]</p>",
            ),
            (
                "> [[html]]\n> <b>X</b>\n> [[/html]]",
                "<blockquote><p>[[html]]<br>\n&lt;b&gt;X&lt;/b&gt;<br>\n[[/html]]</p></blockquote>",
            ),
            (
                "[[html class=\"probe\"]]\n<b>X</b>\n[[/html]]",
                "<p>[[html class=&quot;probe&quot;]]<br>\n&lt;b&gt;X&lt;/b&gt;<br>\n[[/html]]</p>",
            ),
            (
                "[[html]]\n<b>unclosed</b>",
                "<p>[[html]]<br>\n&lt;b&gt;unclosed&lt;/b&gt;</p>",
            ),
        ] {
            let tokenization = crate::tokenize(source);
            let (tree, _errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = crate::render::html::HtmlRender
                .render(&tree, &page_info, &settings)
                .body;

            assert!(tree.html_blocks.is_empty(), "{source:?}: {tree:#?}");
            assert_eq!(html, expected, "{source:?}");
        }
    }

    #[test]
    fn disabled_wikidot_html_blocks_with_unicode_spacing_remain_literal() {
        let page_info = PageInfo::dummy();
        let mut settings =
            WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        settings.enable_html_blocks = false;

        for spacing in ["\u{a0}", "\u{2003}", "\u{3000}", "\u{a0}\u{2003}\u{3000}"] {
            let source = format!("[[{spacing}html]]x[[/html]]");
            let tokenization = crate::tokenize(&source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();

            assert!(tree.html_blocks.is_empty(), "{source:?}: {tree:#?}");
            assert!(
                errors.iter().all(|error| error.rule() != BLOCK_HTML.name),
                "{source:?}: {errors:#?}",
            );
            assert_eq!(
                crate::render::html::HtmlRender
                    .render(&tree, &page_info, &settings)
                    .body,
                format!("<p>{source}</p>"),
                "{source:?}",
            );
        }
    }

    #[test]
    fn disabled_unicode_spaced_html_blocks_keep_crlf_and_later_blocks_independent() {
        let page_info = PageInfo::dummy();
        let mut settings =
            WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        settings.enable_html_blocks = false;
        let source = concat!(
            "prefix [[ unrelated\r\n",
            "[[\u{2003}html]]\r\n",
            "<b>X</b>\r\n",
            "[[/html]]\r\n",
            "[[div class=\"later\"]]\r\n",
            "Y\r\n",
            "[[/div]]",
        );
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        assert!(tree.html_blocks.is_empty(), "{tree:#?}");
        assert!(
            errors.iter().all(|error| error.rule() != BLOCK_HTML.name),
            "{errors:#?}",
        );
        assert!(
            html.contains("[[\u{2003}html]]<br>\n&lt;b&gt;X&lt;/b&gt;<br>\n[[/html]]"),
            "{html}",
        );
        assert!(
            html.contains(r#"<div class="later"><p>Y</p></div>"#),
            "{html}",
        );
    }

    #[test]
    fn disabled_wikidot_html_blocks_keep_nested_modules_literal() {
        let page_info = PageInfo::dummy();
        let mut settings =
            WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        settings.enable_html_blocks = false;
        let source = concat!(
            "[[html]]\n",
            "[[module ListPages]]\n",
            "%%title%%\n",
            "[[/module]]\n",
            "[[/html]]",
        );
        let tokenization = crate::tokenize(source);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        assert!(tree.html_blocks.is_empty(), "{tree:#?}");
        assert_eq!(
            html,
            "<p>[[html]]<br>\n[[module ListPages]]<br>\n%%title%%<br>\n[[/module]]<br>\n[[/html]]</p>",
        );
    }

    #[test]
    fn disabled_wikidot_html_block_keeps_the_preceding_line_break() {
        let page_info = PageInfo::dummy();
        let mut settings =
            WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        settings.enable_html_blocks = false;
        let tokenization = crate::tokenize("before\n[[html]]\n<b>X</b>\n[[/html]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(tree.html_blocks.is_empty(), "{tree:#?}");
        assert!(
            errors.iter().all(|error| error.rule() != BLOCK_HTML.name),
            "{errors:#?}",
        );
        assert_eq!(
            crate::render::html::HtmlRender
                .render(&tree, &page_info, &settings)
                .body,
            "<p>before<br>\n[[html]]<br>\n&lt;b&gt;X&lt;/b&gt;<br>\n[[/html]]</p>",
        );
    }

    #[test]
    fn wikidot_midline_html_markers_stay_literal_inside_span() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization =
            crate::tokenize("prefix [[span]] [[html]] [[/span]] [[/html]]");
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(tree.html_blocks.is_empty(), "{tree:#?}");
        assert_eq!(
            crate::render::html::HtmlRender
                .render(&tree, &page_info, &settings)
                .body,
            "<p>prefix <span>[[html]]</span> [[/html]]</p>",
        );
    }

    #[test]
    fn wikidot_midline_html_markers_stay_literal() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for (source, expected) in [
            (
                "prefix [[html]] [[/html]]",
                "<p>prefix [[html]] [[/html]]</p>",
            ),
            (
                "prefix [[html]]x[[/html]] tail",
                "<p>prefix [[html]]x[[/html]] tail</p>",
            ),
        ] {
            let tokenization = crate::tokenize(source);
            let (tree, _errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = crate::render::html::HtmlRender
                .render(&tree, &page_info, &settings)
                .body;

            assert!(tree.html_blocks.is_empty(), "{source:?}: {tree:#?}");
            assert_eq!(html, expected, "{source:?}");
        }
    }

    #[test]
    fn wikidot_html_block_at_line_start_after_text_still_parses() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("prefix\n[[html]]\nx\n[[/html]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(tree.html_blocks.len(), 1, "{tree:#?}");
        assert!(
            crate::render::html::HtmlRender
                .render(&tree, &page_info, &settings)
                .body
                .contains("html-block-iframe"),
        );
    }
}
