/*
 * render/html/element/math.rs
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
use cfg_if::cfg_if;
use std::num::NonZeroUsize;

cfg_if! {
    if #[cfg(feature = "mathml")] {
        use crate::render::html::escape::escape;
        use latex2mathml::{latex_to_mathml, DisplayStyle};
    } else {
        /// Mocked version of the enum from `latex2mathml`.
        #[derive(Debug, Copy, Clone, PartialEq, Eq)]
        enum DisplayStyle {
            Block,
            Inline,
        }
    }
}

pub fn render_math_block(ctx: &mut HtmlContext, name: Option<&str>, latex_source: &str) {
    debug!(
        "Rendering math block (name '{}', source '{}')",
        name.unwrap_or("<none>"),
        latex_source,
    );

    let index = ctx.next_equation_index();
    if ctx.layout().legacy() {
        if let Some(name) = name {
            ctx.register_named_equation(name, index);
        }
        render_wikidot_math_block(ctx, index, latex_source);
        return;
    }

    render_latex(ctx, name, Some(index), latex_source, DisplayStyle::Block);
}

fn render_wikidot_math_block(
    ctx: &mut HtmlContext,
    index: NonZeroUsize,
    latex_source: &str,
) {
    let environment = if latex_source.contains('{') {
        "align"
    } else {
        "equation"
    };
    let source = latex_source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let wrapped = if source.is_empty() {
        format!("\\begin{{{environment}}} \\end{{{environment}}}")
    } else {
        format!("\\begin{{{environment}}} {source} \\end{{{environment}}}")
    };
    let id = format!("equation-{index}");

    ctx.html()
        .span()
        .attr(attr!("class" => "equation-number"))
        .inner(|ctx| str_write!(ctx, "({index})"));
    ctx.push_raw('\n');
    ctx.html()
        .div()
        .attr(attr!("class" => "math-equation", "id" => &id))
        .contents(&wrapped);
}

pub fn render_math_inline(ctx: &mut HtmlContext, latex_source: &str) {
    debug!("Rendering math inline (source '{latex_source}'");
    if ctx.layout().legacy() {
        ctx.html()
            .span()
            .attr(attr!("class" => "math-inline"))
            .inner(|ctx| {
                ctx.push_raw('$');
                ctx.push_escaped(latex_source);
                ctx.push_raw('$');
            });
        return;
    }
    render_latex(ctx, None, None, latex_source, DisplayStyle::Inline);
}

fn render_latex(
    ctx: &mut HtmlContext,
    name: Option<&str>,
    index: Option<NonZeroUsize>,
    latex_source: &str,
    display: DisplayStyle,
) {
    let (html_tag, wj_type, _error_type) = match display {
        DisplayStyle::Block => ("div", "wj-math-block", "wj-error-block"),
        DisplayStyle::Inline => ("span", "wj-math-inline", "wj-error-inline"),
    };

    ctx.html()
        .tag(html_tag)
        .attr(attr!(
            "class" => "wj-math " wj_type,
            "data-name" => name.unwrap_or(""); if name.is_some(),
        ))
        .inner(|ctx| {
            if let Some(index) = index {
                ctx.html()
                    .span()
                    .attr(attr!("class" => "wj-equation-number"))
                    .inner(|ctx| {
                        let class = "wj-equation-paren wj-equation-paren-open";
                        ctx.html()
                            .span()
                            .attr(attr!("class" => class))
                            .inner(|ctx| ctx.push_raw('('));

                        str_write!(ctx, "{index}");

                        let class = "wj-equation-paren wj-equation-paren-close";
                        ctx.html()
                            .span()
                            .attr(attr!("class" => class))
                            .inner(|ctx| ctx.push_raw(')'));
                    });
            }

            // Add LaTeX source (hidden)
            // Can't use a pre tag because that won't work for inline tags
            ctx.html()
                .code()
                .attr(attr!(
                    "class" => "wj-math-source wj-hidden",
                    "aria-hidden" => "true",
                ))
                .contents(latex_source);

            cfg_if! {
                if #[cfg(feature = "mathml")] {
                    match latex_to_mathml(latex_source, display) {
                        Ok(mathml) => {
                            debug!("Processed LaTeX -> MathML");

                            // `latex2mathml` returns markup, but some text nodes are
                            // formatted without escaping. Keep the generated MathML
                            // structure while escaping text and rejecting unknown tags.
                            let mathml = sanitize_mathml(&mathml);

                            ctx.html()
                                .element("wj-math-ml")
                                .attr(attr!("class" => "wj-math-ml"))
                                .inner(|ctx| ctx.push_raw_str(&mathml));
                        }
                        Err(error) => {
                            warn!("Error processing LaTeX -> MathML: {error}");
                            let error = str!(error);

                            ctx.html()
                                .span()
                                .attr(attr!("class" => _error_type))
                                .contents(error);
                        }
                    }
                }
            }
        });
}

#[cfg(feature = "mathml")]
fn sanitize_mathml(mathml: &str) -> String {
    let mut output = String::with_capacity(mathml.len());
    let mut stack = Vec::new();
    let mut offset = 0;

    while let Some(relative_start) = mathml[offset..].find('<') {
        let tag_start = offset + relative_start;
        escape(&mut output, &mathml[offset..tag_start]);

        let Some(relative_end) = mathml[tag_start..].find('>') else {
            escape(&mut output, &mathml[tag_start..]);
            return output;
        };

        let tag_end = tag_start + relative_end;
        let raw_tag = &mathml[tag_start + 1..tag_end];

        match parse_mathml_tag(raw_tag) {
            Some(MathmlTag::Open {
                name,
                attributes,
                self_closing,
            }) => {
                output.push('<');
                output.push_str(name);
                output.push_str(&attributes);

                if self_closing {
                    output.push_str("/>");
                    offset = tag_end + 1;
                    continue;
                }

                output.push('>');

                if mathml_text_tag(name) {
                    let close_tag = format!("</{name}>");
                    let content_start = tag_end + 1;

                    if let Some(relative_close) = mathml[content_start..].find(&close_tag)
                    {
                        let close_start = content_start + relative_close;
                        escape(&mut output, &mathml[content_start..close_start]);
                        output.push_str(&close_tag);
                        offset = close_start + close_tag.len();
                    } else {
                        escape(&mut output, &mathml[content_start..]);
                        return output;
                    }
                } else {
                    stack.push(name);
                    offset = tag_end + 1;
                }
            }
            Some(MathmlTag::Close { name }) if stack.last().copied() == Some(name) => {
                stack.pop();
                output.push_str("</");
                output.push_str(name);
                output.push('>');
                offset = tag_end + 1;
            }
            _ => {
                escape(&mut output, &mathml[tag_start..=tag_end]);
                offset = tag_end + 1;
            }
        }
    }

    escape(&mut output, &mathml[offset..]);
    output
}

#[cfg(feature = "mathml")]
enum MathmlTag {
    Open {
        name: &'static str,
        attributes: String,
        self_closing: bool,
    },
    Close {
        name: &'static str,
    },
}

#[cfg(feature = "mathml")]
fn parse_mathml_tag(raw_tag: &str) -> Option<MathmlTag> {
    let mut tag = raw_tag.trim();
    if tag.is_empty() || tag.starts_with(['!', '?']) {
        return None;
    }

    if let Some(name) = tag.strip_prefix('/') {
        let name = name.trim();
        if name.contains(char::is_whitespace) || name.ends_with('/') {
            return None;
        }

        return Some(MathmlTag::Close {
            name: known_mathml_tag(name)?,
        });
    }

    let self_closing = tag.ends_with('/');
    if let Some(without_close) = tag.strip_suffix('/') {
        tag = without_close.trim_end();
    }

    let name_end = tag.find(|ch: char| ch.is_whitespace()).unwrap_or(tag.len());
    let name = known_mathml_tag(&tag[..name_end])?;
    let mut rest = &tag[name_end..];
    let mut attributes = String::new();

    while !rest.trim_start().is_empty() {
        rest = rest.trim_start();

        let key_end = rest
            .find(|ch: char| ch == '=' || ch.is_whitespace())
            .unwrap_or(rest.len());
        if key_end == 0 {
            return None;
        }

        let key = &rest[..key_end];
        if !known_mathml_attribute(key) {
            return None;
        }

        rest = rest[key_end..].trim_start();
        if !rest.starts_with('=') {
            return None;
        }

        rest = rest[1..].trim_start();
        let (value, remaining) = parse_mathml_attribute_value(rest)?;
        attributes.push(' ');
        attributes.push_str(key);
        attributes.push_str("=\"");
        escape(&mut attributes, value);
        attributes.push('"');
        rest = remaining;
    }

    Some(MathmlTag::Open {
        name,
        attributes,
        self_closing,
    })
}

#[cfg(feature = "mathml")]
fn parse_mathml_attribute_value(value: &str) -> Option<(&str, &str)> {
    if let Some(quote) = value.chars().next().filter(|ch| *ch == '"' || *ch == '\'') {
        let value_start = quote.len_utf8();
        let close = value[value_start..].find(quote)?;
        let value_end = value_start + close;
        Some((
            &value[value_start..value_end],
            &value[value_end + quote.len_utf8()..],
        ))
    } else {
        let value_end = value.find(char::is_whitespace).unwrap_or(value.len());
        if value_end == 0 {
            None
        } else {
            Some((&value[..value_end], &value[value_end..]))
        }
    }
}

#[cfg(feature = "mathml")]
fn known_mathml_tag(tag: &str) -> Option<&'static str> {
    match tag {
        "math" => Some("math"),
        "mi" => Some("mi"),
        "mn" => Some("mn"),
        "mo" => Some("mo"),
        "mfrac" => Some("mfrac"),
        "mroot" => Some("mroot"),
        "mrow" => Some("mrow"),
        "msqrt" => Some("msqrt"),
        "mspace" => Some("mspace"),
        "mstyle" => Some("mstyle"),
        "msub" => Some("msub"),
        "msubsup" => Some("msubsup"),
        "msup" => Some("msup"),
        "mtable" => Some("mtable"),
        "mtd" => Some("mtd"),
        "mtext" => Some("mtext"),
        "mtr" => Some("mtr"),
        "munder" => Some("munder"),
        "munderover" => Some("munderover"),
        "mover" => Some("mover"),
        _ => None,
    }
}

#[cfg(feature = "mathml")]
fn known_mathml_attribute(attribute: &str) -> bool {
    matches!(
        attribute,
        "accent"
            | "columnalign"
            | "display"
            | "displaystyle"
            | "form"
            | "linethickness"
            | "mathvariant"
            | "stretchy"
            | "width"
            | "xmlns"
    )
}

#[cfg(feature = "mathml")]
fn mathml_text_tag(tag: &str) -> bool {
    matches!(tag, "mi" | "mn" | "mo" | "mtext")
}

pub fn render_equation_reference(ctx: &mut HtmlContext, name: &str) {
    debug!("Rendering equation reference (name '{name}')");

    if ctx.layout().legacy() {
        let index = ctx.get_named_equation(name);
        let target = index.map_or_else(String::new, |index| index.to_string());
        let onclick =
            format!("WIKIDOT.page.utils.scrollToReference('equation-{target}')");
        ctx.html()
            .a()
            .attr(attr!(
                "class" => "eref",
                "href" => "javascript:;",
                "onclick" => &onclick,
            ))
            .inner(|ctx| {
                if let Some(index) = index {
                    str_write!(ctx, "{index}");
                }
            });
        return;
    }

    ctx.html()
        .span()
        .attr(attr!("class" => "wj-equation-ref"))
        .inner(|ctx| {
            ctx.html()
                .element("wj-equation-ref-marker")
                .attr(attr!(
                    "class" => "wj-equation-ref-marker",
                    "type" => "button",
                    "data-name" => name,
                ))
                .contents(name);

            ctx.html().span().attr(attr!(
                "class" => "wj-equation-ref-tooltip",
                "aria-hidden" => "true",
            ));
        });
}

#[cfg(all(test, feature = "mathml"))]
mod tests {
    use super::sanitize_mathml;

    #[test]
    fn mathml_sanitizer_preserves_generated_markup_and_escapes_text() {
        let sanitized = sanitize_mathml(
            r#"<math xmlns="http://www.w3.org/1998/Math/MathML" display="inline"><msup><mi>x</mi><mn>2</mn></msup><mo><</mo><mtext><script>x</script> & y</mtext></math>"#,
        );

        assert!(sanitized.contains(
            r#"<math xmlns="http://www.w3.org/1998/Math/MathML" display="inline">"#
        ));
        assert!(sanitized.contains("<msup><mi>x</mi><mn>2</mn></msup>"));
        assert!(sanitized.contains("<mo>&lt;</mo>"));
        assert!(
            sanitized.contains("<mtext>&lt;script&gt;x&lt;/script&gt; &amp; y</mtext>")
        );
        assert!(!sanitized.contains("<script>"));
    }

    #[test]
    fn mathml_sanitizer_rejects_unknown_tags_and_attributes() {
        let sanitized = sanitize_mathml(
            r#"<math display="inline" onclick="alert(1)"><mtext>safe</mtext><script>alert(1)</script></math>"#,
        );

        assert!(!sanitized.contains(r#"<math display="inline" onclick"#));
        assert!(!sanitized.contains("<script>"));
        assert!(sanitized.contains(
            "&lt;math display=&quot;inline&quot; onclick=&quot;alert(1)&quot;&gt;"
        ));
        assert!(sanitized.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    }
}
