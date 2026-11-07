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
use crate::tree::{AnchorTarget, AttributeMap, ListItem, PartialElement, SyntaxTree};
use cfg_if::cfg_if;
use std::num::NonZeroUsize;

cfg_if! {
    if #[cfg(feature = "mathml")] {
        use crate::render::html::escape::escape;
        use latex2mathml::{latex_to_mathml, DisplayStyle};
    } else {
        #[derive(Debug, Copy, Clone, PartialEq, Eq)]
        enum DisplayStyle {
            Block,
            Inline,
        }
    }
}

pub fn render_math_block(
    ctx: &mut HtmlContext,
    assigned_index: Option<NonZeroUsize>,
    name: Option<&str>,
    latex_source: &str,
    environment: Option<&str>,
    open_elements: &[Element],
    label_spans: &[AttributeMap],
) {
    debug!(
        "Rendering math block (name '{}', source '{}')",
        name.unwrap_or("<none>"),
        latex_source,
    );

    let index = assigned_index.unwrap_or_else(|| ctx.next_equation_index());
    if ctx.layout().legacy() {
        if let Some(name) = name {
            ctx.register_named_equation(name, index);
        }
        render_wikidot_math_block(
            ctx,
            index,
            environment,
            latex_source,
            open_elements,
            label_spans,
        );
        return;
    }

    render_latex(ctx, name, Some(index), latex_source, DisplayStyle::Block);
}

/// Math environments Wikidot accepts in `[[math type="..."]]` and in a
/// whole-body `\begin{...}...\end{...}`. `equation` is valid in the body
/// but an explicit `type="equation"` falls back to auto-detection.
const WIKIDOT_MATH_ENVIRONMENTS: &[&str] = &[
    "eqnarray",
    "align",
    "equation",
    "gather",
    "multline",
    "matrix",
    "pmatrix",
    "bmatrix",
    "Bmatrix",
    "vmatrix",
    "Vmatrix",
    "smallmatrix",
    "cases",
    "split",
    "gathered",
    "aligned",
    "alignat",
    "array",
];

/// The environment for one Wikidot math block: either the valid `Ok` name
/// with its unwrapped body, or the rejected `Err` name shown in the error
/// block.
fn resolve_wikidot_math_environment(
    explicit: Option<&str>,
    source: &str,
) -> Result<(String, String), String> {
    if let Some((name, inner)) = whole_body_math_environment(source) {
        let canonical = name.replace('*', "");
        if WIKIDOT_MATH_ENVIRONMENTS.contains(&canonical.as_str()) {
            return Ok((canonical, inner.trim().to_owned()));
        }
        return Err(canonical);
    }
    if let Some(explicit) = explicit {
        let canonical = explicit.replace('*', "");
        if canonical == "equation" {
            // fall through to auto-detection below
        } else if WIKIDOT_MATH_ENVIRONMENTS.contains(&canonical.as_str()) {
            return Ok((canonical, source.to_owned()));
        } else {
            return Err(canonical);
        }
    }
    if source.contains('&') || source.contains('\\') {
        Ok((str!("align"), source.to_owned()))
    } else {
        Ok((str!("equation"), source.to_owned()))
    }
}

/// The environment and inner body of a source that is exactly one
/// `\begin{X}...\end{X}` span, or `None` when the body does not have that
/// shape. `X` must be a bare alphanumeric-or-star name; anything else
/// falls back to the explicit type or auto-detection. The match is greedy:
/// with two same-name spans the inner body keeps the middle markers.
fn whole_body_math_environment(source: &str) -> Option<(&str, &str)> {
    let rest = source.strip_prefix(r"\begin{")?;
    let brace = rest.find('}')?;
    let name = &rest[..brace];
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'*')
    {
        return None;
    }
    rest[brace + 1..]
        .strip_suffix(&format!("\\end{{{name}}}"))
        .map(|inner| (name, inner))
}

/// Join math body lines the way Wikidot does: every run of spaces, tabs
/// and newlines becomes one space. Other characters (including the
/// non-breaking space) are kept verbatim.
fn join_wikidot_math_source(source: &str) -> String {
    source
        .split([' ', '\t', '\n', '\r'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn render_wikidot_math_block(
    ctx: &mut HtmlContext,
    index: NonZeroUsize,
    environment: Option<&str>,
    latex_source: &str,
    open_elements: &[Element],
    label_spans: &[AttributeMap],
) {
    let source = join_wikidot_math_source(latex_source);
    match resolve_wikidot_math_environment(environment, &source) {
        Err(bad) => {
            // Live Wikidot escapes `&`, `<` and `>` in the rejected name
            // but leaves both quote characters verbatim, so the message is
            // assembled with raw pushes instead of escaped contents.
            ctx.html()
                .div()
                .attr(attr!("class" => "error-block"))
                .inner(|ctx| {
                    ctx.push_raw_str("Unsupported math environment \"");
                    for character in bad.chars() {
                        match character {
                            '&' => ctx.push_raw_str("&amp;"),
                            '<' => ctx.push_raw_str("&lt;"),
                            '>' => ctx.push_raw_str("&gt;"),
                            _ => ctx.push_raw(character),
                        }
                    }
                    ctx.push_raw('"');
                });
        }
        Ok((environment, source)) => {
            let wrapped = if source.is_empty() {
                format!("\\begin{{{environment}}} \\end{{{environment}}}")
            } else {
                format!("\\begin{{{environment}}} {source} \\end{{{environment}}}")
            };
            let id = format!("equation-{index}");

            render_wikidot_equation_number_with_spans(
                ctx,
                index,
                open_elements,
                label_spans,
            );
            ctx.push_raw('\n');
            ctx.html()
                .div()
                .attr(attr!("class" => "math-equation", "id" => &id))
                .contents(&wrapped);
        }
    }
}

fn render_wikidot_equation_number_with_spans(
    ctx: &mut HtmlContext,
    index: NonZeroUsize,
    open_elements: &[Element],
    label_spans: &[AttributeMap],
) {
    let Some((attributes, remaining)) = label_spans.split_first() else {
        render_wikidot_equation_number(ctx, index, open_elements);
        return;
    };
    ctx.html().span().attr(attr!(;; attributes)).inner(|ctx| {
        render_wikidot_equation_number_with_spans(ctx, index, open_elements, remaining);
    });
}

fn render_wikidot_equation_number(
    ctx: &mut HtmlContext,
    index: NonZeroUsize,
    open_elements: &[Element],
) {
    let mut anchors = Vec::new();
    for element in open_elements {
        match element {
            Element::Anchor {
                elements,
                attributes,
                target,
            } => {
                debug_assert!(elements.iter().all(|element| matches!(
                    element,
                    Element::Partial(PartialElement::WikidotEmptyInlineOwner)
                )));
                anchors.push((attributes, target));
            }
            other => render_elements(ctx, std::slice::from_ref(other)),
        }
    }
    render_wikidot_equation_number_anchors(ctx, index, &anchors);
}

fn render_wikidot_equation_number_anchors(
    ctx: &mut HtmlContext,
    index: NonZeroUsize,
    anchors: &[(&AttributeMap, &Option<AnchorTarget>)],
) {
    let Some((&(attributes, target), rest)) = anchors.split_first() else {
        ctx.html()
            .span()
            .attr(attr!("class" => "equation-number"))
            .inner(|ctx| str_write!(ctx, "({index})"));
        return;
    };
    let target_value = match target {
        Some(target) => target.html_attr(),
        None => "",
    };
    ctx.html()
        .a()
        .attr(attr!(
            "href" => ""; if !attributes.get().contains_key("href"),
            "target" => target_value; if target.is_some();;
            attributes,
        ))
        .inner(|ctx| {
            render_wikidot_equation_number_anchors(ctx, index, rest);
        });
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

/// Pre-register every named Wikidot equation in render order.
///
/// Rendering numbers equations as they are visited, so an `[[eref]]`
/// placed before its `[[math]]` cannot resolve in a single pass. This
/// walk visits the tree in exactly the order the renderer does (including
/// footnote and bibliography bodies at their blocks) and records each
/// name, so forward references resolve to the same numbers the renderer
/// then assigns. Later rendering re-registers the same values.
/// Live Wikidot numbers `[[math]]` blocks in source order: its math rule runs
/// before the footnote rule moves footnote bodies to the footer, so a body's
/// equations take their numbers at the footnote reference.
pub fn preregister_wikidot_equations(ctx: &mut HtmlContext, tree: &SyntaxTree) {
    let mut index = NonZeroUsize::new(1).unwrap();
    let mut footnote_walked = vec![false; ctx.footnotes().len()];
    preregister_wikidot_equation_slices(
        ctx,
        tree,
        &[&tree.elements],
        &mut index,
        &mut footnote_walked,
    );
    if tree.needs_footnote_block {
        let bodies: Vec<&[Element]> = ctx
            .footnotes()
            .iter()
            .zip(&footnote_walked)
            .filter(|(_, walked)| !**walked)
            .map(|(body, _)| body.as_slice())
            .collect();
        preregister_wikidot_equation_slices(
            ctx,
            tree,
            &bodies,
            &mut index,
            &mut footnote_walked,
        );
    }
    ctx.continue_equation_index_at(index);
}

fn preregister_wikidot_equation_slices(
    ctx: &mut HtmlContext,
    tree: &SyntaxTree,
    initial: &[&[Element<'_>]],
    index: &mut NonZeroUsize,
    footnote_walked: &mut [bool],
) {
    let mut stack: Vec<(&[Element], usize)> =
        initial.iter().rev().map(|slice| (*slice, 0)).collect();
    while let Some((elements, position)) = stack.pop() {
        let Some(element) = elements.get(position) else {
            continue;
        };
        stack.push((elements, position + 1));
        match element {
            Element::Container(container) => stack.push((container.elements(), 0)),
            Element::Table(table) => {
                let mut cells = Vec::new();
                for row in &table.rows {
                    for cell in &row.cells {
                        cells.push(cell.elements.as_slice());
                    }
                }
                for slice in cells.into_iter().rev() {
                    stack.push((slice, 0));
                }
            }
            Element::TabView(tabs) => {
                for tab in tabs.iter().rev() {
                    stack.push((tab.elements(), 0));
                }
            }
            Element::Anchor { elements, .. } => stack.push((elements.as_slice(), 0)),
            Element::List { items, .. } => {
                for item in items.iter().rev() {
                    match item {
                        ListItem::Elements { elements, .. } => {
                            stack.push((elements.as_slice(), 0));
                        }
                        ListItem::SubList { element } => {
                            stack.push((std::slice::from_ref(&**element), 0));
                        }
                    }
                }
            }
            Element::DefinitionList(items) => {
                for item in items.iter().rev() {
                    stack.push((item.value_elements.as_slice(), 0));
                    stack.push((item.key_elements.as_slice(), 0));
                }
            }
            Element::Collapsible {
                elements,
                unfolded_tail_start,
                wikidot_block_open_elements,
                ..
            } => {
                let (elements, unfolded_tail) = unfolded_tail_start
                    .map(|start| elements.split_at(start.min(elements.len())))
                    .unwrap_or((elements.as_slice(), &[]));
                stack.push((unfolded_tail, 0));
                stack.push((elements, 0));
                stack.push((wikidot_block_open_elements.as_slice(), 0));
            }
            Element::WikidotBlockFrame { children, .. } => {
                stack.push((children.as_slice(), 0))
            }
            Element::Color { elements, .. } => stack.push((elements.as_slice(), 0)),
            Element::Include { elements, .. } => stack.push((elements.as_slice(), 0)),
            Element::Footnote(reference) => {
                let number = reference.index;
                if let Some(walked) = number
                    .checked_sub(1)
                    .and_then(|slot| footnote_walked.get_mut(slot))
                    && !*walked
                {
                    *walked = true;
                    stack.push((ctx.footnotes()[number - 1].as_slice(), 0));
                }
            }
            Element::FootnoteBlock { .. } => {
                for (body, walked) in
                    ctx.footnotes().iter().zip(footnote_walked.iter_mut()).rev()
                {
                    if !*walked {
                        *walked = true;
                        stack.push((body.as_slice(), 0));
                    }
                }
            }
            Element::BibliographyBlock {
                index: block_index, ..
            } => {
                if let Some(bibliography) = tree.bibliographies.slice().get(*block_index)
                {
                    for (_, elements) in bibliography.slice().iter().rev() {
                        stack.push((elements.as_slice(), 0));
                    }
                }
            }
            Element::Math { name, .. } => {
                if let Some(name) = name {
                    ctx.register_named_equation(name, *index);
                }
                ctx.assign_wikidot_equation_number(
                    std::ptr::from_ref(element) as usize,
                    *index,
                );
                *index = NonZeroUsize::new(index.get() + 1).unwrap();
            }
            Element::Partial(partial) => match partial {
                PartialElement::ListItem(ListItem::Elements { elements, .. }) => {
                    stack.push((elements.as_slice(), 0));
                }
                PartialElement::ListItem(ListItem::SubList { element }) => {
                    stack.push((std::slice::from_ref(&**element), 0));
                }
                PartialElement::TableRow(row) => {
                    let mut cells = Vec::new();
                    for cell in &row.cells {
                        cells.push(cell.elements.as_slice());
                    }
                    for slice in cells.into_iter().rev() {
                        stack.push((slice, 0));
                    }
                }
                PartialElement::TableCell(cell) => {
                    stack.push((cell.elements.as_slice(), 0))
                }
                PartialElement::WikidotFoster(elements) => {
                    stack.push((elements.as_slice(), 0))
                }
                PartialElement::Tab(tab) => stack.push((tab.elements.as_slice(), 0)),
                PartialElement::RubyText(ruby_text) => {
                    stack.push((ruby_text.elements.as_slice(), 0));
                }
                _ => {}
            },
            _ => {}
        }
    }
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
