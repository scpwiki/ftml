/*
 * render/html/element/bibliography.rs
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
use crate::tree::{Bibliography, BibliographyScopeWrapper, Container};
use std::borrow::Cow;

pub fn render_bibcite(ctx: &mut HtmlContext, label: &str, brackets: bool, inline: bool) {
    debug!("Rendering bibliography citation (label {label}, brackets {brackets})");

    if !ctx.enter_bibliography_ref(label) {
        warn!("Recursive bibliography citation detected for label {label}");
        render_missing_bibcite(ctx, label, brackets, inline);
        return;
    }

    match ctx.get_bibliography_ref(label) {
        Some((index, contents)) => {
            if ctx.layout().legacy() {
                render_wikidot_bibcite(ctx, index, brackets);
                ctx.exit_bibliography_ref(label);
                return;
            }

            let reference_string = ctx
                .handle()
                .get_message(ctx.language(), "bibliography-reference");
            let label = format!("{reference_string} {index}.");

            ctx.html()
                .span()
                .attr(attr!("class" => "wj-bibliography-ref"))
                .inner(|ctx| {
                    let id = str!(index);

                    if brackets {
                        ctx.push_raw('[');
                    }

                    ctx.html()
                        .element("wj-bibliography-ref-marker")
                        .attr(attr!(
                            "class" => "wj-bibliography-ref-marker",
                            "role" => "link",
                            "aria-label" => &label,
                            "data-id" => &id,
                        ))
                        .contents(&id);

                    if brackets {
                        ctx.push_raw(']');
                    }

                    ctx.html()
                        .span()
                        .attr(attr!(
                            "class" => "wj-bibliography-ref-tooltip",
                            "aria-hidden" => "true",
                        ))
                        .inner(|ctx| {
                            ctx.html()
                                .span()
                                .attr(
                                    attr!("class" => "wj-bibliography-ref-tooltip-label"),
                                )
                                .contents(&label);

                            ctx.html()
                                .span()
                                .attr(attr!("class" => "wj-bibliography-ref-contents"))
                                .contents(contents);
                        });
                });
        }
        None => {
            render_missing_bibcite(ctx, label, brackets, inline);
        }
    }

    ctx.exit_bibliography_ref(label);
}

fn render_wikidot_bibcite(ctx: &mut HtmlContext, index: usize, brackets: bool) {
    if brackets {
        ctx.push_raw('[');
    }

    let id = ctx.random().generate_wikidot_bibcite_id(index);
    let onclick = format!("WIKIDOT.page.utils.scrollToReference('bibitem-{index}')",);
    ctx.html()
        .a()
        .attr(attr!(
            "class" => "bibcite",
            "href" => "javascript:;",
            "id" => &id,
            "onclick" => &onclick,
        ))
        .inner(|ctx| str_write!(ctx, "{index}"));

    if brackets {
        ctx.push_raw(']');
    }
}

fn render_missing_bibcite(
    ctx: &mut HtmlContext,
    label: &str,
    brackets: bool,
    inline: bool,
) {
    if ctx.layout().legacy() {
        if inline {
            ctx.html()
                .span()
                .attr(attr!("class" => "error-inline"))
                .inner(|ctx| {
                    ctx.push_escaped("Bibliography item ");
                    ctx.html().em().contents(label);
                    ctx.push_escaped(" not found.");
                });
            return;
        } else if brackets {
            ctx.push_escaped("[[bibcite ");
        } else {
            ctx.push_escaped("[[bibcite_ ");
        }
        ctx.push_escaped(label);
        ctx.push_escaped("]]");
        return;
    }

    let message_pre = ctx
        .handle()
        .get_message(ctx.language(), "bibliography-cite-not-found-pre");
    let message_post = ctx
        .handle()
        .get_message(ctx.language(), "bibliography-cite-not-found-post");

    ctx.html()
        .span()
        .attr(attr!("class" => "wj-error-inline"))
        .inner(|ctx| {
            ctx.push_escaped(message_pre);
            ctx.html().em().contents(label);
            ctx.push_escaped(message_post);
        });
}

fn wrap_bibliography_entry_prefix<'t>(
    elements: &[Element<'t>],
    wrappers: &[BibliographyScopeWrapper<'t>],
    prefix: String,
) -> Vec<Element<'t>> {
    let output = apply_bibliography_wrappers(elements, wrappers, Some(prefix));
    merge_bibliography_wrappers(output, wrappers)
}

fn apply_bibliography_wrappers<'t>(
    elements: &[Element<'t>],
    wrappers: &[BibliographyScopeWrapper<'t>],
    prefix: Option<String>,
) -> Vec<Element<'t>> {
    if wrappers.is_empty() {
        let mut output = Vec::with_capacity(elements.len() + 1);
        if let Some(prefix) = prefix {
            output.push(Element::Text(Cow::Owned(prefix)));
        }
        output.extend(elements.iter().cloned());
        return output;
    }
    let (first, rest) = wrappers.split_first().expect("wrappers non-empty");
    if let Some(index) = elements
        .iter()
        .position(|element| bibliography_element_matches_wrapper(element, first))
    {
        let mut output = Vec::new();
        let mut inside: Vec<Element<'t>> = Vec::new();
        if let Some(prefix) = prefix {
            inside.push(Element::Text(Cow::Owned(prefix)));
        }
        inside.extend(elements[..index].iter().cloned());
        if !inside.is_empty() {
            output.push(bibliography_wrap_chain(wrappers, inside));
        }
        let children = apply_bibliography_wrappers(
            bibliography_scope_children(&elements[index]),
            rest,
            None,
        );
        output.push(bibliography_build_wrapper(first, children));
        output.extend(elements[index + 1..].iter().cloned());
        return output;
    }
    match prefix {
        Some(prefix) => {
            let mut output = vec![bibliography_wrap_chain(
                wrappers,
                vec![Element::Text(Cow::Owned(prefix.trim_end().to_owned()))],
            )];
            let mut rest_elements: Vec<Element<'t>> = elements.to_vec();
            if let Some(Element::Text(text)) = rest_elements.first_mut() {
                let trimmed = text.trim_start_matches(|c: char| c.is_whitespace());
                if trimmed.len() != text.len() {
                    *text = Cow::Owned(trimmed.to_owned());
                }
            }
            if rest_elements.first().is_some_and(
                |element| matches!(element, Element::Text(text) if text.is_empty()),
            ) {
                rest_elements.remove(0);
            }
            if !rest_elements.is_empty() {
                output.push(Element::Text(Cow::Borrowed(" ")));
                output.extend(rest_elements);
            }
            output
        }
        None => elements.to_vec(),
    }
}

fn bibliography_wrap_chain<'t>(
    wrappers: &[BibliographyScopeWrapper<'t>],
    content: Vec<Element<'t>>,
) -> Element<'t> {
    let mut current = content;
    for wrapper in wrappers.iter().rev() {
        current = vec![bibliography_build_wrapper(wrapper, current)];
    }
    current.into_iter().next().expect("wrappers non-empty")
}

fn bibliography_build_wrapper<'t>(
    wrapper: &BibliographyScopeWrapper<'t>,
    children: Vec<Element<'t>>,
) -> Element<'t> {
    match wrapper {
        BibliographyScopeWrapper::Container { ctype, attributes } => {
            Element::Container(Container::new(*ctype, children, attributes.clone()))
        }
        BibliographyScopeWrapper::Color { color, background } => Element::Color {
            color: color.clone(),
            background: *background,
            elements: children,
        },
    }
}

fn bibliography_element_matches_wrapper(
    element: &Element<'_>,
    wrapper: &BibliographyScopeWrapper<'_>,
) -> bool {
    match (element, wrapper) {
        (
            Element::Container(container),
            BibliographyScopeWrapper::Container { ctype, attributes },
        ) => container.ctype() == *ctype && container.attributes() == attributes,
        (
            Element::Color {
                color, background, ..
            },
            BibliographyScopeWrapper::Color {
                color: w_color,
                background: w_background,
            },
        ) => color == w_color && background == w_background,
        _ => false,
    }
}

fn bibliography_scope_children<'a, 't>(element: &'a Element<'t>) -> &'a [Element<'t>] {
    match element {
        Element::Container(container) => container.elements(),
        Element::Color { elements, .. } => elements,
        _ => &[],
    }
}

fn merge_bibliography_wrappers<'t>(
    elements: Vec<Element<'t>>,
    wrappers: &[BibliographyScopeWrapper<'t>],
) -> Vec<Element<'t>> {
    let mut output: Vec<Element<'t>> = Vec::with_capacity(elements.len());
    for element in elements {
        let merged = match output.last_mut() {
            Some(previous) => {
                bibliography_merge_into(previous, element.clone(), wrappers)
            }
            None => false,
        };
        if !merged {
            output.push(element);
        }
    }
    output
}

fn bibliography_merge_into<'t>(
    previous: &mut Element<'t>,
    next: Element<'t>,
    wrappers: &[BibliographyScopeWrapper<'t>],
) -> bool {
    let shared = wrappers.iter().any(|wrapper| {
        bibliography_element_matches_wrapper(previous, wrapper)
            && bibliography_element_matches_wrapper(&next, wrapper)
    });
    if !shared {
        return false;
    }
    match (previous, next) {
        (Element::Container(previous), Element::Container(next)) => {
            previous.elements_mut().extend(Vec::from(next));
            true
        }
        (
            Element::Color {
                elements: previous_elements,
                ..
            },
            Element::Color {
                elements: next_elements,
                ..
            },
        ) => {
            previous_elements.extend(next_elements);
            true
        }
        _ => false,
    }
}

fn render_wikidot_bibliography(
    ctx: &mut HtmlContext,
    title: &str,
    bibliography: &Bibliography,
) {
    ctx.html()
        .div()
        .attr(attr!("class" => "bibitems"))
        .inner(|ctx| {
            let title_wrappers = bibliography.title_wrappers();
            if title_wrappers.is_empty() {
                ctx.html()
                    .div()
                    .attr(attr!("class" => "title"))
                    .contents(title);
            } else {
                let wrapped = [bibliography_wrap_chain(
                    title_wrappers,
                    vec![Element::Text(Cow::Borrowed(title))],
                )];
                ctx.html()
                    .div()
                    .attr(attr!("class" => "title"))
                    .inner(|ctx| render_elements(ctx, &wrapped));
            }

            let mut entry_index = 0;
            for (position, (label, elements)) in bibliography.slice().iter().enumerate() {
                if Bibliography::is_residual(label) {
                    ctx.push_raw('\n');
                    render_elements(ctx, elements);
                    if bibliography.slice()[position + 1..]
                        .iter()
                        .any(|(label, _)| !Bibliography::is_residual(label))
                    {
                        ctx.push_raw('\n');
                    }
                    continue;
                }

                entry_index += 1;
                let id = format!("bibitem-{entry_index}");
                ctx.html()
                    .div()
                    .attr(attr!("class" => "bibitem", "id" => &id))
                    .inner(|ctx| {
                        let wrappers = bibliography.reopened_wrappers(position);
                        if wrappers.is_empty() {
                            if elements.is_empty() {
                                str_write!(ctx, "{entry_index}.");
                            } else {
                                str_write!(ctx, "{entry_index}. ");
                            }
                            render_elements(ctx, elements);
                        } else {
                            let prefix = if elements.is_empty() {
                                format!("{entry_index}.")
                            } else {
                                format!("{entry_index}. ")
                            };
                            let wrapped = wrap_bibliography_entry_prefix(
                                elements, wrappers, prefix,
                            );
                            render_elements(ctx, &wrapped);
                        }
                    });
            }
        });
}

pub fn render_bibliography(
    ctx: &mut HtmlContext,
    title: Option<&str>,
    bibliography_index: usize,
    bibliography: &Bibliography,
) {
    debug!(
        "Rendering bibliography block (title {}, items {})",
        title.unwrap_or("<default>"),
        bibliography.slice().len(),
    );

    let title_default;
    let title: &str = match title {
        Some(title) => title,
        None => {
            title_default = ctx
                .handle()
                .get_message(ctx.language(), "bibliography-block-title");

            title_default
        }
    };

    if ctx.layout().legacy() {
        render_wikidot_bibliography(ctx, title, bibliography);
        return;
    }

    ctx.html()
        .div()
        .attr(attr!("class" => "wj-bibliography bibitems"))
        .inner(|ctx| {
            ctx.html()
                .div()
                .attr(attr!("class" => "wj-bibliography-title title"))
                .contents(title);

            let mut id = String::new();
            let mut class = String::new();
            for (entry_index, (_, elements)) in bibliography.slice().iter().enumerate() {
                let bibliography_index = bibliography_index + 1;
                let entry_index = entry_index + 1;

                id.clear();
                str_write!(
                    id,
                    "wj-bibliography-item-{bibliography_index}-{entry_index}",
                );
                class.clear();
                str_write!(
                    class,
                    "wj-bibliography-item bibitem bibitem-{bibliography_index}-{entry_index}",
                );

                ctx.html()
                    .div()
                    .attr(attr!("class" => &class, "id" => &id))
                    .inner(|ctx| {
                        ctx.html()
                            .element("wj-bibliography-item-marker")
                            .attr(attr!(
                                "class" => "wj-bibliography-item-marker",
                                "type" => "button",
                                "role" => "link",
                            ))
                            .inner(|ctx| {
                                str_write!(ctx, "{entry_index}");

                                ctx.html()
                                    .span()
                                    .attr(attr!("class" => "wj-bibliography-sep"))
                                    .inner(|ctx| ctx.push_raw('.'));
                            });

                        render_elements(ctx, elements);
                    });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::super::super::context::HtmlContext;
    use super::super::super::output::HtmlOutput;
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Handle;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::{BibliographyList, Element};

    #[test]
    fn bibliography_rendering_covers_missing_and_block_variants() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let footnotes: [Vec<Element<'static>>; 0] = [];

        let mut bibliography = Bibliography::new();
        bibliography.add(cow!("alpha"), vec![text!("Alpha & source")]);

        let mut bibliographies = BibliographyList::new();
        bibliographies.push(bibliography);
        let bibliography_ref = bibliographies.get_bibliography(0);

        let mut ctx = HtmlContext::new(
            &page_info,
            &Handle,
            &settings,
            &[],
            &footnotes,
            &bibliographies,
            0,
        );

        render_bibcite(&mut ctx, "missing", false, false);
        render_bibcite(&mut ctx, "alpha", false, false);
        render_bibliography(&mut ctx, Some("Works Cited"), 0, bibliography_ref);

        let output = HtmlOutput::from(ctx);
        assert!(output.body.contains(r#"<span class="wj-error-inline">"#));
        assert!(
            output
                .body
                .contains("Bibliography item <em>missing</em> not found.")
        );
        assert!(
            output.body.contains(
                r#"<div class="wj-bibliography-title title">Works Cited</div>"#
            )
        );
        assert!(
            output
                .body
                .contains(r#"<span class="wj-bibliography-sep">.</span>"#)
        );
        assert!(output.body.contains("Alpha &amp; source"));
    }
}
