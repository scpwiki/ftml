/*
 * parsing/footnote.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2026 Wikijump Team
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

use crate::tree::{AttributeMap, Container, ContainerType, Element};
use std::borrow::Cow;
use std::vec::IntoIter;

pub(crate) fn normalize_wikidot_blocks(
    elements: &mut Vec<Element<'_>>,
    footnotes_empty: bool,
) {
    let root = std::mem::take(elements);
    let mut stack = vec![ElementFrame::new(None, root)];

    while let Some(frame) = stack.last_mut() {
        if let Some(element) = frame.input.next() {
            let Element::Container(mut container) = element else {
                frame.output.push(element);
                continue;
            };

            if container.ctype() == ContainerType::Paragraph {
                normalize_paragraph(&mut frame.output, container, footnotes_empty);
                continue;
            }

            let children = std::mem::take(container.elements_mut());
            stack.push(ElementFrame::new(Some(container), children));
            continue;
        }

        let ElementFrame {
            parent, mut output, ..
        } = stack.pop().expect("normalization stack was not empty");
        if !footnotes_empty {
            normalize_block_sequence(&mut output);
        }
        let Some(mut parent) = parent else {
            *elements = output;
            break;
        };
        *parent.elements_mut() = output;
        stack
            .last_mut()
            .expect("nested normalization frame has a parent")
            .output
            .push(Element::Container(parent));
    }
}

struct ElementFrame<'t> {
    parent: Option<Container<'t>>,
    input: IntoIter<Element<'t>>,
    output: Vec<Element<'t>>,
}

impl<'t> ElementFrame<'t> {
    fn new(parent: Option<Container<'t>>, input: Vec<Element<'t>>) -> Self {
        let capacity = input.len();
        Self {
            parent,
            input: input.into_iter(),
            output: Vec::with_capacity(capacity),
        }
    }
}

fn normalize_paragraph<'t>(
    output: &mut Vec<Element<'t>>,
    container: Container<'t>,
    footnotes_empty: bool,
) {
    if footnotes_empty {
        let mut container = container;
        remove_empty_inline_blocks(container.elements_mut());
        output.push(Element::Container(container));
        return;
    }

    let attributes = container.attributes().clone();
    let children: Vec<Element<'t>> = container.into();
    let children = dissolve_footnote_block_containers(children);
    if !children
        .iter()
        .any(|element| matches!(element, Element::FootnoteBlock { .. }))
    {
        output.push(Element::Container(Container::new(
            ContainerType::Paragraph,
            children,
            attributes,
        )));
        return;
    }

    let mut fragment = Vec::new();
    let mut after_suffix_inline = false;
    for child in children {
        if let Element::FootnoteBlock {
            wikidot_inline_suffix,
            wikidot_inline_prefix,
            ..
        } = child
        {
            push_before_fragment(
                output,
                &mut fragment,
                &attributes,
                wikidot_inline_prefix,
            );
            output.push(child);
            after_suffix_inline = wikidot_inline_suffix;
        } else {
            fragment.push(child);
        }
    }
    push_after_fragment(output, &mut fragment, &attributes, after_suffix_inline);
}

fn push_before_fragment<'t>(
    output: &mut Vec<Element<'t>>,
    fragment: &mut Vec<Element<'t>>,
    attributes: &AttributeMap<'t>,
    prefix_inline: bool,
) {
    if fragment.is_empty() {
        return;
    }

    if !prefix_inline {
        if matches!(fragment.last(), Some(Element::LineBreak)) {
            fragment.pop();
        }
        push_paragraph(output, fragment, attributes);
        return;
    }

    output.append(fragment);
    output.push(Element::LineBreak);
}

fn push_after_fragment<'t>(
    output: &mut Vec<Element<'t>>,
    fragment: &mut Vec<Element<'t>>,
    attributes: &AttributeMap<'t>,
    suffix_inline: bool,
) {
    if fragment.is_empty() {
        return;
    }

    if !suffix_inline {
        if matches!(fragment.first(), Some(Element::LineBreak)) {
            fragment.remove(0);
        }
        push_paragraph(output, fragment, attributes);
        return;
    }

    output.push(Element::LineBreak);
    output.append(fragment);
}

fn subtree_contains_footnote_block(element: &Element<'_>) -> bool {
    let mut stack = vec![element];
    while let Some(element) = stack.pop() {
        match element {
            Element::FootnoteBlock { .. } => return true,
            Element::Container(container) => stack.extend(container.elements().iter()),
            Element::Color { elements, .. } => stack.extend(elements.iter()),
            _ => {}
        }
    }
    false
}

fn delimiter_markers(ctype: ContainerType) -> Option<(&'static str, &'static str)> {
    match ctype {
        ContainerType::Bold => Some(("**", "**")),
        ContainerType::Italics => Some(("//", "//")),
        ContainerType::Underline => Some(("__", "__")),
        ContainerType::Superscript => Some(("^^", "^^")),
        ContainerType::Subscript => Some((",,", ",,")),
        ContainerType::Strikethrough => Some(("--", "--")),
        ContainerType::Monospace => Some(("{{", "}}")),
        _ => None,
    }
}

enum DissolveStep<'t> {
    Process(Element<'t>),
    PushText(Cow<'t, str>),
    FinishContainer(ContainerType, AttributeMap<'t>),
}

fn dissolve_footnote_block_containers<'t>(
    elements: Vec<Element<'t>>,
) -> Vec<Element<'t>> {
    let mut out_stack: Vec<Vec<Element<'t>>> = vec![Vec::with_capacity(elements.len())];
    let mut steps: Vec<DissolveStep<'t>> = elements
        .into_iter()
        .rev()
        .map(DissolveStep::Process)
        .collect();
    while let Some(step) = steps.pop() {
        match step {
            DissolveStep::PushText(text) => {
                out_stack
                    .last_mut()
                    .expect("dissolve output stack was empty")
                    .push(Element::Text(text));
            }
            DissolveStep::FinishContainer(ctype, attributes) => {
                let children = out_stack.pop().expect("dissolve child output missing");
                out_stack
                    .last_mut()
                    .expect("dissolve output stack was empty")
                    .push(Element::Container(Container::new(
                        ctype, children, attributes,
                    )));
            }
            DissolveStep::Process(element) => {
                if !subtree_contains_footnote_block(&element) {
                    out_stack
                        .last_mut()
                        .expect("dissolve output stack was empty")
                        .push(element);
                    continue;
                }

                match element {
                    Element::Container(container) => {
                        match delimiter_markers(container.ctype()) {
                            Some((open, close)) => {
                                let children: Vec<Element<'t>> = container.into();
                                steps.push(DissolveStep::PushText(Cow::Borrowed(close)));
                                steps.extend(
                                    children.into_iter().rev().map(DissolveStep::Process),
                                );
                                steps.push(DissolveStep::PushText(Cow::Borrowed(open)));
                            }
                            None => {
                                let (ctype, attributes) =
                                    (container.ctype(), container.attributes().clone());
                                let children: Vec<Element<'t>> = container.into();
                                steps.push(DissolveStep::FinishContainer(
                                    ctype, attributes,
                                ));
                                out_stack.push(Vec::new());
                                steps.extend(
                                    children.into_iter().rev().map(DissolveStep::Process),
                                );
                            }
                        }
                    }
                    Element::Color {
                        color,
                        background,
                        elements: children,
                    } => {
                        let open = if background {
                            format!("##|{color}|")
                        } else {
                            format!("##{color}|")
                        };
                        steps.push(DissolveStep::PushText(Cow::Borrowed("##")));
                        steps.extend(
                            children.into_iter().rev().map(DissolveStep::Process),
                        );
                        steps.push(DissolveStep::PushText(Cow::Owned(open)));
                    }
                    element => {
                        out_stack
                            .last_mut()
                            .expect("dissolve output stack was empty")
                            .push(element);
                    }
                }
            }
        }
    }
    debug_assert_eq!(out_stack.len(), 1);
    out_stack.pop().expect("dissolve output missing")
}

fn push_paragraph<'t>(
    output: &mut Vec<Element<'t>>,
    paragraph: &mut Vec<Element<'t>>,
    attributes: &AttributeMap<'t>,
) {
    if paragraph.is_empty() {
        return;
    }
    output.push(Element::Container(Container::new(
        ContainerType::Paragraph,
        std::mem::take(paragraph),
        attributes.clone(),
    )));
}

fn normalize_block_sequence(elements: &mut Vec<Element<'_>>) {
    if !elements.iter().any(subtree_contains_footnote_block) {
        return;
    }

    let dissolved = dissolve_footnote_block_containers(std::mem::take(elements));
    let mut output = Vec::with_capacity(dissolved.len());
    let mut iter = dissolved.into_iter().peekable();
    while let Some(element) = iter.next() {
        let Element::FootnoteBlock {
            wikidot_inline_prefix,
            wikidot_inline_suffix,
            ..
        } = element
        else {
            output.push(element);
            continue;
        };

        if wikidot_inline_prefix
            && !output.is_empty()
            && !matches!(output.last(), Some(Element::LineBreak))
        {
            output.push(Element::LineBreak);
        }
        output.push(element);
        if wikidot_inline_suffix
            && iter
                .peek()
                .is_some_and(|next| !matches!(next, Element::LineBreak))
        {
            output.push(Element::LineBreak);
        }
    }
    *elements = output;
}

fn remove_empty_inline_blocks(elements: &mut Vec<Element<'_>>) {
    let mut output = Vec::with_capacity(elements.len());
    let mut footnote_after_hyphen = false;

    for mut element in elements.drain(..) {
        if matches!(element, Element::FootnoteBlock { .. }) {
            footnote_after_hyphen =
                matches!(output.last(), Some(Element::Text(text)) if text.ends_with('-'));
            continue;
        }

        if footnote_after_hyphen {
            let rest = match &element {
                Element::Text(text) => text.strip_prefix('-').map(str::to_owned),
                _ => None,
            };
            if let Some(rest) = rest {
                let Some(Element::Text(text)) = output.last_mut() else {
                    unreachable!("last element was checked above");
                };
                let mut value = text.to_string();
                value.pop();
                value.push('\u{2014}');
                *text = Cow::Owned(value);
                element = Element::Text(Cow::Owned(rest));
            }
            footnote_after_hyphen = false;
        }
        output.push(element);
    }

    *elements = output;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_block_normalization_handles_deep_container_trees_iteratively() {
        const DEPTH: usize = 2_048;

        let paragraph = Container::new(
            ContainerType::Paragraph,
            vec![
                Element::Text(Cow::Borrowed("before")),
                Element::FootnoteBlock {
                    title: None,
                    hide: false,
                    wikidot_wrappers: Vec::new(),
                    wikidot_inline_suffix: false,
                    wikidot_inline_prefix: false,
                },
                Element::Text(Cow::Borrowed("after")),
            ],
            AttributeMap::new(),
        );
        let mut elements = vec![Element::Container(paragraph)];
        for _ in 0..DEPTH {
            elements = vec![Element::Container(Container::new(
                ContainerType::Div,
                elements,
                AttributeMap::new(),
            ))];
        }

        normalize_wikidot_blocks(&mut elements, false);

        for _ in 0..DEPTH {
            let Element::Container(container) = elements.pop().expect("nested div")
            else {
                panic!("expected nested div");
            };
            assert_eq!(container.ctype(), ContainerType::Div);
            elements = container.into();
        }
        assert_eq!(elements.len(), 3);
        assert!(matches!(elements[0], Element::Container(_)));
        assert!(matches!(elements[1], Element::FootnoteBlock { .. }));
        assert!(matches!(elements[2], Element::Container(_)));
    }
}
