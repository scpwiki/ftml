/*
 * parsing/rule/impls/block/blocks/math.rs
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
use std::borrow::Cow;

pub const BLOCK_MATH: BlockRule = BlockRule {
    name: "block-math",
    accepts_names: &["math"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn,
};

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing math block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "User doesn't allow star flag");
    assert!(!flag_score, "User doesn't allow score flag");
    assert_block_name(&BLOCK_MATH, name);

    if parser.settings().layout.legacy() && parser.native_blockquote_depth().is_some() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let name = get_math_head(parser, in_head)?;
    if parser.settings().layout.legacy()
        && name.as_deref().is_some_and(|name| !wikidot_math_name(name))
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let latex_source = match parser.get_body_text(&BLOCK_MATH)? {
        Cow::Borrowed(source) => Cow::Borrowed(source.trim()),
        Cow::Owned(source) => Cow::Owned(source.trim().to_owned()),
    };
    if parser.settings().layout.legacy()
        && wikidot_math_crosses_bold_owner(parser, latex_source.as_ref())
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    if latex_source.is_empty() && !parser.settings().layout.legacy() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let element = Element::Math {
        name,
        latex_source,
        wikidot_block_open_elements: Vec::new(),
    };

    success_elements(element)
}

fn get_math_head<'r, 't>(
    parser: &mut Parser<'r, 't>,
    in_head: bool,
) -> Result<Option<Cow<'t, str>>, ParseError>
where
    'r: 't,
{
    if parser.settings().layout.legacy() && in_head {
        let mut attribute_candidate = parser.clone();
        if let Ok(arguments) = attribute_candidate.get_head_map(&BLOCK_MATH, in_head)
            && arguments.has_source()
            && !arguments.has_bare_source()
        {
            parser.update(&attribute_candidate);
            return Ok(None);
        }
    }

    parser.get_head_value_resolved(&BLOCK_MATH, in_head, |_, value| {
        Ok(value.and_then(|source| match source {
            Cow::Borrowed(text) => {
                let name = text.trim();
                (!name.is_empty()).then_some(Cow::Borrowed(name))
            }
            Cow::Owned(text) => {
                let name = text.trim();
                (!name.is_empty()).then_some(Cow::Owned(name.to_owned()))
            }
        }))
    })
}

fn wikidot_math_crosses_bold_owner(parser: &Parser<'_, '_>, body: &str) -> bool {
    if wikidot_unclosed_block_depth(body, "bold") == 0 {
        return false;
    }

    let suffix = &parser.full_text().inner()[parser.current().span.start..];
    let suffix = suffix.trim_start_matches([' ', '\t', '\r', '\n', '\0']);
    let Some(close) = suffix.strip_prefix("[[/") else {
        return false;
    };
    let Some(end) = close.find("]]") else {
        return false;
    };
    close[..end].trim().eq_ignore_ascii_case("bold")
}

fn wikidot_unclosed_block_depth(source: &str, block_name: &str) -> usize {
    let mut depth = 0_usize;
    let mut remaining = source;

    while let Some(start) = remaining.find("[[") {
        remaining = &remaining[start + 2..];
        let Some(end) = remaining.find("]]") else {
            break;
        };
        let marker = remaining[..end].trim();
        let marker_name = marker.split_ascii_whitespace().next().unwrap_or_default();
        if marker_name.eq_ignore_ascii_case(block_name) {
            depth += 1;
        } else if marker_name
            .strip_prefix('/')
            .is_some_and(|name| name.eq_ignore_ascii_case(block_name))
        {
            depth = depth.saturating_sub(1);
        }
        remaining = &remaining[end + 2..];
    }

    depth
}

pub(crate) fn wikidot_math_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    fn render(layout: Layout, source: &str) -> (String, Vec<crate::parsing::ParseError>) {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
        let mut source = source.to_owned();
        crate::preprocess(&mut source);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        (HtmlRender.render(&tree, &page_info, &settings).body, errors)
    }

    #[test]
    fn quoted_math_block_trims_owned_source() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tokenization = crate::tokenize(concat!(
            "> [[collapsible]]\n",
            "> [[math]]\n",
            ">   x + y   \n",
            "> [[/math]]\n",
            "> [[/collapsible]]\n",
        ));
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        assert!(format!("{tree:?}").contains("x + y"));
    }

    #[test]
    fn wikidot_math_preserves_live_block_boundaries_and_dom() {
        let source = concat!(
            "[[math]] A [[/math]]\n\n",
            "[[math]]\nB\n[[/math]]\n\n",
            "[[math invalid-name]]\n\\frac{x}{y}\n[[/math]]\n\n",
            "[[math valid_name]]\nC\n[[/math]]\n",
            "X[[eref valid_name]]Y[[eref missing]]Z\n\n",
            "[[math]]\n[[/math]]",
        );
        let (html, errors) = render(Layout::Wikidot, source);

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains(
            r#"<span class="equation-number">(1)</span>
<div class="math-equation" id="equation-1">\begin{align} A [[/math]] [[math]] B [[/math]] [[math invalid-name]] \frac{x}{y} \end{align}</div>"#,
        ), "{html}");
        assert!(
            html.contains(
                r#"<span class="equation-number">(2)</span>
<div class="math-equation" id="equation-2">\begin{equation} C \end{equation}</div>"#,
            ),
            "{html}"
        );
        assert!(html.contains(
            r#"X<a class="eref" href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;equation-2&#39;)">2</a>Y<a class="eref" href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;equation-&#39;)"></a>Z"#,
        ), "{html}");
        assert!(
            html.contains(
                r#"<span class="equation-number">(3)</span>
<div class="math-equation" id="equation-3">\begin{equation} \end{equation}</div>"#,
            ),
            "{html}"
        );
    }

    #[test]
    fn wikidot_math_closer_accepts_trailing_line_blanks() {
        for blanks in ["", " ", "\t", "  ", " \t"] {
            let source = format!("A\n[[math]]\nx + y\n[[/math]]{blanks}\nB");
            let (html, errors) = render(Layout::Wikidot, &source);
            assert!(errors.is_empty(), "{blanks:?}: {errors:#?}");
            assert_eq!(
                html.matches("math-equation").count(),
                1,
                "{blanks:?}: {html}"
            );
            assert!(!html.contains("[[math]]"), "{blanks:?}: {html}");
            assert!(
                html.contains("\\begin{equation} x + y \\end{equation}"),
                "{blanks:?}: {html}"
            );
        }

        let (html, _) = render(Layout::Wikidot, "[[math]]\nx\n[[/math]] ");
        assert_eq!(html.matches("math-equation").count(), 1, "{html}");

        let (html, _) = render(Layout::Wikidot, "[[math]]\nx\n[[/math]] y\n\nz");
        assert_eq!(html.matches("math-equation").count(), 0, "{html}");
    }

    fn render_wikidot(source: &str) -> (String, Vec<crate::parsing::ParseError>) {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = source.to_owned();
        crate::preprocess_for_layout(&mut source, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        (HtmlRender.render(&tree, &page_info, &settings).body, errors)
    }

    #[test]
    fn wikidot_math_body_keeps_backslash_line_breaks() {
        let (html, errors) = render_wikidot(concat!(
            "[[math]]\n",
            "\\left[ \\begin{array}{cc}\n",
            "PR(p_1) \\\\\n",
            "\\vdots \\\\\n",
            "PR(p_N)\\end{array} \\right]\n",
            "[[/math]]\n",
            "Where [[$ \\ell(p_i,p_j) $]] is 0\n",
        ));

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(
            html.contains(
                r"\left[ \begin{array}{cc} PR(p_1) \\ \vdots \\ PR(p_N)\end{array} \right]"
            ),
            "{html}"
        );
        let (html, _) = render_wikidot("joined \\\nline\n");
        assert!(html.contains("joined line"), "{html}");
        assert!(!html.contains('\\'), "{html}");
    }

    #[test]
    fn wikidot_math_closer_after_a_double_backslash_line_still_closes() {
        let source = concat!(
            "[[code]]\n",
            "[[math sample]]\n",
            "\\lim_{h \\to 0} \\frac{(x+h)^{2}-x^{2}}{h} \\\n",
            "[[/math]]\n",
            "[[/code]]\n",
            "> {{@@[[eref@@ //sample//]]}}\n",
            "**出力:**\n",
            "[[math sample]]\n",
            "\\lim_{h \\to 0} \\frac{(x+h)^{2}-x^{2}}{h} \\\\\n",
            "[[/math]]\n",
            "> [[eref sample]]\n",
            "> ([[eref sample]])\n",
            "\n",
            "LaTeXの環境を指定する方法は2通りあります。\n",
            "\n",
            "[[code]]\n",
            "[[math type=\"align\"]]\n",
            "E = mc^2\n",
            "[[/math]]\n",
            "[[/code]]\n",
            "\n",
            "[[code]]\n",
            "[[math]]\n",
            "\\begin{align}\n",
            "E = mc^2\n",
            "\\end{align}\n",
            "[[/math]]\n",
            "[[/code]]\n",
        );
        let mut text = source.to_owned();
        crate::preprocess_for_layout(&mut text, Layout::Wikidot);
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(&text);
        let (tree, _) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert_eq!(tree.code_blocks.len(), 3, "{:?}", tree.code_blocks);
        assert_eq!(html.matches("<div class=\"code\">").count(), 3, "{html}");
        assert!(html.contains(r"{h} \\ \end{align}"), "{html}");
        assert!(html.contains("{h} \\\n[[/math]]</code>"), "{html}");
        assert!(!html.contains("[[code]]"), "{html}");
    }

    #[test]
    fn wikidot_math_one_line_block_merges_and_numbers_first() {
        let source = concat!(
            "[[math]] A = \\pi \\cdot r^2 [[/math]]\n\n",
            "[[math]]\nC = 2\\pi r\n[[/math]]\n\n",
            "Apple[[eref quadratic-formula]]\n\n",
            "Banana cherry[[equation quadratic-formula]] Gooseberry\n\n",
            "Pineapple[[eref navier-stokes]]Blueberry\n",
        );
        let (html, errors) = render_wikidot(source);

        assert!(
            errors.iter().all(|error| error.rule() != "block-math"),
            "{errors:#?}"
        );
        assert_eq!(html.matches("math-equation").count(), 1, "{html}");
        assert!(html.contains(
            "<span class=\"equation-number\">(1)</span>\n<div class=\"math-equation\" id=\"equation-1\">\\begin{align} A = \\pi \\cdot r^2 [[/math]] [[math]] C = 2\\pi r \\end{align}</div>",
        ), "{html}");
        assert!(html.contains(
            "<p>Apple<a class=\"eref\" href=\"javascript:;\" onclick=\"WIKIDOT.page.utils.scrollToReference(&#39;equation-&#39;)\"></a></p>",
        ), "{html}");
        assert!(
            html.contains(
                "<p>Banana cherry[[equation quadratic-formula]] Gooseberry</p>",
            ),
            "{html}"
        );
        assert!(html.contains(
            "<p>Pineapple<a class=\"eref\" href=\"javascript:;\" onclick=\"WIKIDOT.page.utils.scrollToReference(&#39;equation-&#39;)\"></a>Blueberry</p>",
        ), "{html}");
    }

    #[test]
    fn wikidot_math_one_line_opener_close_fallback_boundaries() {
        let (html, errors) =
            render_wikidot("[[math]] A [[/math]]\n\n[[math]]\nB\n[[/math]]\n");
        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains(
            "<div class=\"math-equation\" id=\"equation-1\">\\begin{equation} A [[/math]] [[math]] B \\end{equation}</div>",
        ), "{html}");

        let (html, _) = render_wikidot("[[math]] A [[/math]]\n");
        assert_eq!(html.matches("math-equation").count(), 0, "{html}");
        assert!(html.contains("<p>[[math]] A [[/math]]</p>"), "{html}");
    }

    #[test]
    fn wikidot_math_align_environment_for_backslash_bodies() {
        let (html, errors) = render_wikidot("[[math]]\n\\alpha + \\beta\n[[/math]]\n");
        assert!(errors.is_empty(), "{errors:#?}");
        assert!(
            html.contains("\\begin{align} \\alpha + \\beta \\end{align}"),
            "{html}"
        );

        let (html, errors) = render_wikidot("[[math]]\nx^2\n[[/math]]\n");
        assert!(errors.is_empty(), "{errors:#?}");
        assert!(
            html.contains("\\begin{equation} x^2 \\end{equation}"),
            "{html}"
        );
    }

    #[test]
    fn wikijump_math_rendering_is_unchanged() {
        let (html, errors) = render(
            Layout::Wikijump,
            "[[math named-equation]]\nx + y\n[[/math]]",
        );

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains(r#"class="wj-math wj-math-block""#), "{html}");
        assert!(html.contains(r#"data-name="named-equation""#), "{html}");
        assert!(!html.contains(r#"class="math-equation""#), "{html}");
    }
}
