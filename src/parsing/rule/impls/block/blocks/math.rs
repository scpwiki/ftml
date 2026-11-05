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
    opener_start: usize,
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

    if parser.settings().layout.legacy()
        && parser.in_footnote()
        && !math_opener_starts_line(parser, opener_start)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let head = get_math_head(parser, in_head)?;
    if parser.settings().layout.legacy()
        && head
            .name
            .as_deref()
            .is_some_and(|name| !wikidot_math_name(name))
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
        name: head.name,
        latex_source,
        environment: head.environment,
        wikidot_block_open_elements: Vec::new(),
        wikidot_label_spans: Vec::new(),
    };

    success_elements(element)
}

fn math_opener_starts_line(parser: &Parser<'_, '_>, opener_start: usize) -> bool {
    opener_start == 0
        || parser.full_text().inner().as_bytes().get(opener_start - 1) == Some(&b'\n')
}

struct MathHead<'t> {
    name: Option<Cow<'t, str>>,
    environment: Option<Cow<'t, str>>,
}

fn get_math_head<'r, 't>(
    parser: &mut Parser<'r, 't>,
    in_head: bool,
) -> Result<MathHead<'t>, ParseError>
where
    'r: 't,
{
    if !parser.settings().layout.legacy() {
        let name = parser.get_head_value_resolved(&BLOCK_MATH, in_head, |_, value| {
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
        })?;
        return Ok(MathHead {
            name,
            environment: None,
        });
    }

    if !in_head {
        return Ok(MathHead {
            name: None,
            environment: None,
        });
    }

    parser.get_head_value_resolved(&BLOCK_MATH, in_head, |_, value| {
        let Some(source) = value else {
            return Ok(MathHead {
                name: None,
                environment: None,
            });
        };
        Ok(match source {
            Cow::Borrowed(text) => match parse_wikidot_math_head(text.trim()) {
                Some((name, environment)) => MathHead {
                    name: name.map(Cow::Borrowed),
                    environment: environment.map(Cow::Borrowed),
                },
                None => MathHead {
                    name: Some(Cow::Borrowed(text.trim())),
                    environment: None,
                },
            },
            Cow::Owned(text) => match parse_wikidot_math_head(text.trim()) {
                Some((name, environment)) => MathHead {
                    name: name.map(|name| Cow::Owned(name.to_owned())),
                    environment: environment
                        .map(|environment| Cow::Owned(environment.to_owned())),
                },
                None => MathHead {
                    name: Some(Cow::Owned(text.trim().to_owned())),
                    environment: None,
                },
            },
        })
    })
}

/// Parse a resolved legacy `[[math ...]]` head.
///
/// The head holds an optional bare equation name followed by
/// double-quoted `key="value"` arguments separated by spaces or tabs.
/// Unknown arguments are ignored. The `type` argument selects the math
/// environment only when it is the last item in the head. Anything else
/// (a second bare word, a quoted or unquoted name, spaces around `=`,
/// single-quoted or bare values) leaves the whole opener literal, so the
/// caller is told the raw text and rejects it as a name instead.
fn parse_wikidot_math_head(text: &str) -> Option<(Option<&str>, Option<&str>)> {
    let text = text.trim();
    if text.is_empty() {
        return Some((None, None));
    }

    let mut rest = text;
    let mut name = None;
    let mut last_key = None;
    let mut last_value = None;

    if rest
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        let length = rest
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            .count();
        let word = &rest[..length];
        let after = &rest[length..];
        if after.is_empty() {
            return Some((Some(word), None));
        }
        if after.starts_with(' ') || after.starts_with('\t') {
            name = Some(word);
            rest = after;
        }
    }

    loop {
        rest = rest.trim_start_matches([' ', '\t']);
        if rest.is_empty() {
            break;
        }
        let key_length = rest
            .bytes()
            .take_while(|byte| {
                byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'-'
            })
            .count();
        if key_length == 0 {
            return None;
        }
        let key = &rest[..key_length];
        rest = &rest[key_length..];
        if !rest.starts_with('=') {
            return None;
        }
        rest = &rest[1..];
        if !rest.starts_with('"') {
            return None;
        }
        rest = &rest[1..];
        let Some(end) = rest.find('"') else {
            return None;
        };
        let value = &rest[..end];
        rest = &rest[end + 1..];
        if !rest.is_empty() && !(rest.starts_with(' ') || rest.starts_with('\t')) {
            return None;
        }
        last_key = Some(key);
        last_value = Some(value);
    }

    let environment = match (last_key, last_value) {
        (Some("type"), Some(value)) => Some(value),
        _ => None,
    };
    Some((name, environment))
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
mod head_tests {
    use super::parse_wikidot_math_head;

    #[test]
    fn math_head_grammar() {
        assert_eq!(parse_wikidot_math_head(""), Some((None, None)));
        assert_eq!(parse_wikidot_math_head("   "), Some((None, None)));
        assert_eq!(parse_wikidot_math_head("a"), Some((Some("a"), None)));
        assert_eq!(
            parse_wikidot_math_head("valid_name"),
            Some((Some("valid_name"), None)),
        );
        assert_eq!(parse_wikidot_math_head("type"), Some((Some("type"), None)),);
        assert_eq!(
            parse_wikidot_math_head("type=\"align\""),
            Some((None, Some("align"))),
        );
        assert_eq!(
            parse_wikidot_math_head("a type=\"align\""),
            Some((Some("a"), Some("align"))),
        );
        assert_eq!(
            parse_wikidot_math_head("  a   type=\"align\"  "),
            Some((Some("a"), Some("align"))),
        );
        assert_eq!(
            parse_wikidot_math_head("type=\"align\" type=\"gather\""),
            Some((None, Some("gather"))),
        );
        assert_eq!(
            parse_wikidot_math_head("a foo=\"bar\""),
            Some((Some("a"), None)),
        );
        assert_eq!(
            parse_wikidot_math_head("foo=\"bar\" type=\"gather\""),
            Some((None, Some("gather"))),
        );
        assert_eq!(
            parse_wikidot_math_head("type=\"gather\" foo=\"bar\""),
            Some((None, None)),
        );
        assert_eq!(
            parse_wikidot_math_head("type=\"gather\" label=\"x\""),
            Some((None, None)),
        );
        assert_eq!(
            parse_wikidot_math_head("TYPE=\"align\""),
            Some((None, None)),
        );
        assert_eq!(parse_wikidot_math_head("type=\"\""), Some((None, Some(""))));
        // Anything else leaves the whole opener literal.
        assert_eq!(parse_wikidot_math_head("a b"), None);
        assert_eq!(parse_wikidot_math_head("type=\"align\" a"), None);
        assert_eq!(parse_wikidot_math_head("a type=\"align\" b"), None);
        assert_eq!(parse_wikidot_math_head("\"a\" type=\"align\""), None);
        assert_eq!(parse_wikidot_math_head("type=align"), None);
        assert_eq!(parse_wikidot_math_head("type='align'"), None);
        assert_eq!(parse_wikidot_math_head("type = \"align\""), None);
        assert_eq!(parse_wikidot_math_head("type\t=\t\"align\""), None);
        assert_eq!(parse_wikidot_math_head("type=\"unterminated"), None);
        assert_eq!(parse_wikidot_math_head("navier-stokes"), None);
        assert_eq!(parse_wikidot_math_head("a.b"), None);
        assert_eq!(parse_wikidot_math_head("a\tb"), None);
    }
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
        assert_eq!(html.matches("math-equation").count(), 1, "{html}");
        assert!(
            html.contains("\\begin{equation} x \\end{equation}"),
            "{html}"
        );
        assert!(html.contains("<p>y</p>"), "{html}");
        assert!(html.contains("<p>z</p>"), "{html}");
    }

    #[test]
    fn wikidot_math_environment_selection() {
        let cases = [
            ("{x}", "equation"),
            ("x_{2}", "equation"),
            ("x^2", "equation"),
            ("x   y", "equation"),
            ("a&b", "align"),
            ("a&lt;b", "align"),
            ("\\alpha", "align"),
            ("\\", "align"),
            ("\\begin{matrix}a\\end{matrix}", "matrix"),
            ("\\begin{equation}x\\end{equation}", "equation"),
        ];
        for (body, environment) in cases {
            let source = format!("[[math]]\n{body}\n[[/math]]");
            let (html, errors) = render_wikidot(&source);
            assert!(errors.is_empty(), "{body}: {errors:#?}");
            assert!(
                html.contains(&format!("\\begin{{{environment}}}")),
                "{body}: {html}"
            );
        }

        let (html, errors) =
            render_wikidot("[[math]]\n\\begin{foo}x\\end{foo}\n[[/math]]");
        assert!(errors.is_empty(), "{errors:#?}");
        assert!(
            html.contains(
                "<div class=\"error-block\">Unsupported math environment \"foo\"</div>"
            ),
            "{html}",
        );

        let (html, errors) = render_wikidot("[[math type=\"align\"]]\nx\n[[/math]]");
        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("\\begin{align} x \\end{align}"), "{html}");

        let (html, errors) =
            render_wikidot("[[math a type=\"gather\"]]\nx\n[[/math]]\n[[eref a]]");
        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("\\begin{gather} x \\end{gather}"), "{html}");
        assert!(html.contains(">1</a>"), "{html}");

        let (html, errors) = render_wikidot("[[math type=\"equation\"]]\nA&B\n[[/math]]");
        assert!(errors.is_empty(), "{errors:#?}");
        assert!(
            html.contains("\\begin{align} A&amp;B \\end{align}"),
            "{html}"
        );

        let (html, errors) = render_wikidot("[[math type=\"foo\"]]\nx\n[[/math]]");
        assert!(errors.is_empty(), "{errors:#?}");
        assert!(
            html.contains(
                "<div class=\"error-block\">Unsupported math environment \"foo\"</div>"
            ),
            "{html}",
        );
        assert!(!html.contains("math-equation"), "{html}");

        let (html, errors) = render_wikidot("[[eref a]]\n[[math a]]\nx\n[[/math]]");
        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains(">1</a>"), "{html}");
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
