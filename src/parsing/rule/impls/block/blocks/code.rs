/*
 * parsing/rule/impls/block/blocks/code.rs
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
use crate::tree::CodeBlock;
use std::borrow::Cow;
use wikidot_normalize::normalize;

pub const BLOCK_CODE: BlockRule = BlockRule {
    name: "block-code",
    accepts_names: &["code"],
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
    debug!("Parsing code block (in-head {in_head})");
    assert!(!flag_star, "Code doesn't allow star flag");
    assert!(!flag_score, "Code doesn't allow score flag");
    assert_block_name(&BLOCK_CODE, name);
    let source = parser.full_text().inner();

    if parser.settings().layout.legacy() && !parser.discarding_hidden_body() {
        let head = &source[..parser.current().span.start];
        if head
            .rfind("[[")
            .and_then(|start| head[start + 2..].chars().next())
            .is_some_and(char::is_whitespace)
        {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
    }

    if parser.native_blockquote_depth().is_some() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let wikidot_candidate =
        if parser.settings().layout.legacy() && !parser.discarding_hidden_body() {
            let candidate = parser
                .wikidot_code_candidate(opener_start)
                .ok_or_else(|| parser.make_err(ParseErrorKind::RuleFailed))?;
            if !source[opener_start..candidate.owner_end].contains(['\r', '\n'])
                && source[opener_start..candidate.body.start].contains("[!--")
            {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            Some(candidate)
        } else {
            None
        };

    let mut arguments = parser.get_head_map_wikidot(&BLOCK_CODE, in_head)?;
    let body_start = parser.current().span.start;

    let mut language = arguments.get("type");
    if let Some(ref mut language) = language {
        language.to_mut().make_ascii_lowercase();
    }

    let mut name = arguments.get("name");
    if let Some(ref mut name) = name {
        normalize(name.to_mut());
    }

    let has_delayed_input = wikidot_candidate.as_ref().map_or_else(
        || {
            parser.has_runtime_literal_in_range(opener_start..parser.current().span.start)
                || parser.body_has_delayed_input(&BLOCK_CODE)
        },
        |candidate| {
            let owner_range = opener_start..candidate.owner_end;
            parser.has_generated_in_range(owner_range.clone())
                || parser.has_runtime_literal_in_range(owner_range)
        },
    );
    let skip_end_blocks = wikidot_candidate
        .as_ref()
        .map(|candidate| candidate.end_blocks_to_skip)
        .unwrap_or(0);
    let code = if skip_end_blocks > 0 {
        parser.get_body_text_after_skipping_end_blocks(&BLOCK_CODE, skip_end_blocks)?
    } else {
        parser.get_body_text(&BLOCK_CODE)?
    };

    // Wikidot trims code bodies: leading blank lines are dropped and
    // trailing whitespace is stripped, while leading spaces on the first
    // content line are kept. The token slice above can additionally eat
    // same-line spaces after the opener head (the head parser consumes
    // them), so recover exactly those before trimming; the trim itself
    // handles every trailing case. Non-legacy layouts keep the token
    // slice untouched.
    let code = match wikidot_candidate {
        Some(candidate) => {
            let eaten = &source[candidate.body.start..body_start];
            let eaten = if !eaten.is_empty()
                && eaten.bytes().all(|byte| byte == b' ' || byte == b'\t')
            {
                eaten
            } else {
                ""
            };
            Cow::Owned(wikidot_trim_code_body(&format!("{eaten}{code}")))
        }
        None => code,
    };

    if has_delayed_input {
        let owner_end = parser.current().span.start;
        let generated = parser.generated_in_range(opener_start..owner_end);
        return success_elements(Element::Delayed(DelayedElement::shell(
            source,
            opener_start..owner_end,
            &generated,
        )));
    }

    let code = if parser
        .fused_name_residual()
        .is_some_and(|position| position <= body_start)
    {
        parser.clear_fused_name_residual();
        Cow::Owned(format!("]{code}"))
    } else {
        code
    };

    let code_block = CodeBlock {
        contents: code,
        language,
        name,
        wikidot_block_open_elements: Vec::new(),
        wikidot_label_spans: Vec::new(),
    };

    let element = parser.push_code_block_element(code_block);
    ok!(element)
}

/// Trims a legacy code body the way Wikidot does: leading blank lines are
/// dropped, trailing whitespace is stripped, and everything else (including
/// leading spaces on the first content line and blank lines in the middle)
/// is kept verbatim.
fn wikidot_trim_code_body(body: &str) -> String {
    let mut trimmed = body;
    loop {
        let without_indent = trimmed.trim_start_matches([' ', '\t']);
        match without_indent.strip_prefix('\n') {
            Some(rest) => trimmed = rest,
            // Not a blank line: keep the indent, it is content.
            None => break,
        }
    }
    // NOTE: Only ASCII whitespace is stripped. Wider whitespace (such as
    // ideographic spaces) is preserved, as on live. Non-breaking spaces
    // survive the trim and are then served as regular spaces, matching the
    // live bytes for bodies like `\n\xa0x\xa0\n` (codews:19).
    trimmed
        .trim_end_matches([' ', '\t', '\n'])
        .replace('\u{a0}', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn code_blocks_with_trailing_space_closers_parse_and_track() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = concat!(
            "[[code]]\n",
            "public class Program {\n",
            "    public static main(string args) : void {\n",
            "        Console.output(\"Hello, world\"); \n",
            "    }\n",
            "}\n",
            "[[/code]] \n",
            "[[code]] \n",
            "public class Program { \n",
            "   public static main(string args) : void { \n",
            "        person.say(\"Hello\");\n",
            "   }\n",
            "}\n",
            "[[/code]] \n",
        );
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(tree.code_blocks.len(), 2, "{:?}", tree.code_blocks);
        assert!(
            tree.code_blocks[0]
                .contents
                .contains("Console.output(\"Hello, world\");"),
            "{:?}",
            tree.code_blocks[0].contents,
        );
        assert!(
            tree.code_blocks[1]
                .contents
                .contains("person.say(\"Hello\");"),
            "{:?}",
            tree.code_blocks[1].contents,
        );

        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert_eq!(html.matches("<div class=\"code\">").count(), 2, "{html}");
        assert!(!html.contains("[[code]]"), "{html}");
    }

    #[test]
    fn code_block_tracks_body_language_and_normalized_name() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(
            "[[code type=\"RUST\" name=\"Sample Heading\"]]\nfn main() {}\n[[/code]]",
        );
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        let [Element::Code(element_code)] = tree.elements.as_slice() else {
            panic!("expected one code block element, got {:?}", tree.elements);
        };
        let [tracked_code] = tree.code_blocks.as_slice() else {
            panic!(
                "expected one tracked code block, got {:?}",
                tree.code_blocks
            );
        };

        for code_block in [element_code, tracked_code] {
            assert_eq!(code_block.contents, "fn main() {}");
            assert_eq!(code_block.language.as_deref(), Some("rust"));
            assert_eq!(code_block.name.as_deref(), Some("sample-heading"));
        }
    }

    #[test]
    fn wikidot_code_uses_legacy_plain_and_highlighted_dom() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        for (source, expected) in [
            (
                "[[code]]apple[[/code]]",
                r#"<div class="code"><pre><code>apple</code></pre></div>"#,
            ),
            ("[[code]][[/code]]", r#"<div class="code"></div>"#),
            (
                "[[code type=\"rust\"]]fn main() {}[[/code]]",
                r#"<div class="code"><pre><code>fn main() {}</code></pre></div>"#,
            ),
            (
                "[[code type=\"python\"]]import antigravity[[/code]]",
                r#"<div class="code"><div class="hl-main"><pre><span class="hl-reserved">import</span><span class="hl-code"> </span><span class="hl-identifier">antigravity</span></pre></div></div>"#,
            ),
            (
                "[[code type=\"css\"]]:root {\n     --right-1: 0%;\n}[[/code]]",
                concat!(
                    r#"<div class="code"><div class="hl-main"><pre>"#,
                    r#"<span class="hl-special">:root</span>"#,
                    r#"<span class="hl-code"> </span>"#,
                    r#"<span class="hl-brackets">{</span>"#,
                    "<span class=\"hl-code\">\n     --</span>",
                    r#"<span class="hl-reserved">right-1:</span>"#,
                    r#"<span class="hl-code"> </span>"#,
                    r#"<span class="hl-number">0</span>"#,
                    r#"<span class="hl-string">%</span>"#,
                    "<span class=\"hl-code\">;\n</span>",
                    r#"<span class="hl-brackets">}</span>"#,
                    "</pre></div></div>",
                ),
            ),
        ] {
            let tokenization = crate::tokenize(source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;

            assert!(errors.is_empty(), "{source}: {errors:#?}");
            assert_eq!(html, expected, "{source}");
        }
    }

    #[test]
    fn wikidot_code_head_ends_at_an_iftags_segment_close() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        for (source, expected) in [
            (
                "[[code type=\"python\"[[iftags -zzz]][[/iftags]]]]x = 1[[/code]]",
                concat!(
                    "<div class=\"code\"><div class=\"hl-main\"><pre>",
                    "<span class=\"hl-identifier\">[[/iftags]]]]x</span>",
                    "<span class=\"hl-code\"> </span>",
                    "<span class=\"hl-identifier\">=</span>",
                    "<span class=\"hl-code\"> </span>",
                    "<span class=\"hl-identifier\">1</span>",
                    "</pre></div></div>",
                ),
            ),
            (
                "[[code [[iftags -zzz]][[/iftags]]]]x = 1[[/code]]",
                "<div class=\"code\"><pre><code>[[/iftags]]]]x = 1</code></pre></div>",
            ),
            (
                "[[code [[iftags +zzz]]zzz[[/iftags]]]]x = 1[[/code]]",
                "<div class=\"code\"><pre><code>zzz[[/iftags]]]]x = 1</code></pre></div>",
            ),
            (
                "[[code [[iftags -zzz]]type=\"python\"[[/iftags]]]]x = 1[[/code]]",
                "<div class=\"code\"><pre><code>type=&quot;python&quot;[[/iftags]]]]x = 1</code></pre></div>",
            ),
        ] {
            let tokenization = crate::tokenize(source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;

            assert!(errors.is_empty(), "{source}: {errors:#?}");
            assert_eq!(html, expected, "{source}");
        }
    }

    #[test]
    fn wikidot_code_accepts_bare_and_unbalanced_type_values() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        for source in [
            "[[code type=css]][[/code]]",
            "[[code type=\"css]][[/code]]",
            "[[code type=css\"]][[/code]]",
        ] {
            let tokenization = crate::tokenize(source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;

            assert!(errors.is_empty(), "{source}: {errors:#?}");
            assert_eq!(html, r#"<div class="code"></div>"#, "{source}");
        }
    }

    #[test]
    fn wikidot_code_followed_by_inline_raw_stays_unwrapped() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[code]]\n====\n[[/code]]\n@@====@@");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            concat!(
                "<div class=\"code\"><pre><code>====</code></pre></div>",
                "<br>\n<span style=\"white-space: pre-wrap;\">====</span>",
            ),
        );
    }

    #[test]
    fn wikidot_code_list_mode_keeps_its_existing_inline_owner() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
        let tokenization = crate::tokenize("prefix[[code]]body[[/code]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(tree.code_blocks.len(), 1, "{tree:#?}");
        assert_eq!(tree.code_blocks[0].contents, "body");
    }

    #[test]
    fn wikidot_spaced_code_opener_stays_literal_without_stealing_the_next_block() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = "[[ code ]]\nNESTED\n[[code]]\n[[/code]]";
        let tokenization = crate::tokenize(source);
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert_eq!(
            html,
            "<p>[[ code ]]<br>\nNESTED</p><div class=\"code\"></div>",
        );
    }

    #[test]
    fn quoted_code_markers_remain_literal_like_wikidot() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        for input in [
            ">> [[code]]\n>> ALPHA_CODE_DEPTH_TWO\n>> [[/code]]",
            "> > [[code]]\n> > ALPHA_CODE_SPACED_LITERAL\n> > [[/code]]",
            ">> [[code]]\n>> ALPHA_CODE_SHALLOW_CLOSE\n> [[/code]]\n> ALPHA_AFTER_SHALLOW",
            "> [[code]]\n> ALPHA_CODE_DEEP_CLOSE\n>> [[/code]]\n> ALPHA_AFTER_DEEP",
            concat!(
                "> [[collapsible]]\n",
                "> [[code]]\n",
                "> ALPHA_COLLAPSIBLE_CODE_LITERAL\n",
                "> [[/code]]\n",
                "> [[/collapsible]]\n",
            ),
        ] {
            let tokenization = crate::tokenize(input);
            let (tree, _errors) =
                crate::parse(&tokenization, &page_info, &settings).into();

            assert!(
                tree.code_blocks.is_empty(),
                "{input:?}: {:?}",
                tree.code_blocks
            );
            let debug = format!("{tree:?}");
            assert!(debug.contains("Text(\"[[\")"), "{input:?}: {debug}");
            assert!(debug.contains("Text(\"code\")"), "{input:?}: {debug}");
            assert!(debug.contains("Text(\"]]\")"), "{input:?}: {debug}");
        }
    }
}
