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

use crate::tree::{
    AttributeMap, Container, ContainerType, Element, ListItem, ListType, PartialElement,
};
use std::borrow::Cow;
use std::vec::IntoIter;

pub(crate) fn repair_wikidot_footnote_list_chrome<'t>(
    elements: &mut Vec<Element<'t>>,
) -> bool {
    let mut changed = false;
    for element in elements.iter_mut() {
        changed |= repair_footnote_list_element(element);
    }
    changed |= merge_trailing_one_item_list(elements);
    changed
}

fn repair_footnote_list_element<'t>(element: &mut Element<'t>) -> bool {
    let Element::List { items, .. } = element else {
        if let Element::Container(container) = element {
            return repair_wikidot_footnote_list_chrome(container.elements_mut());
        }
        return false;
    };

    let mut changed = lift_footnote_list_items(items);
    for item in items.iter_mut() {
        match item {
            ListItem::Elements { elements, .. } => {
                changed |= repair_wikidot_footnote_list_chrome(elements);
            }
            ListItem::SubList { element } => {
                changed |= repair_footnote_list_element(element);
            }
        }
    }
    changed
}

fn lift_footnote_list_items<'t>(items: &mut Vec<ListItem<'t>>) -> bool {
    let original = std::mem::take(items);
    let mut changed = false;
    for item in original {
        let ListItem::Elements {
            mut elements,
            attributes,
        } = item
        else {
            items.push(item);
            continue;
        };
        let Some(inner) = take_wrapped_list(&mut elements) else {
            items.push(ListItem::Elements {
                elements,
                attributes,
            });
            continue;
        };
        changed = true;
        if !elements.is_empty() {
            items.push(ListItem::Elements {
                elements,
                attributes,
            });
        }
        items.extend(inner);
    }
    changed
}

fn take_wrapped_list<'t>(elements: &mut Vec<Element<'t>>) -> Option<Vec<ListItem<'t>>> {
    let rest_start = elements
        .iter()
        .position(|element| !matches!(element, Element::Text(_)))?;
    if !elements[..rest_start].iter().all(|element| match element {
        Element::Text(text) => quote_marker_residue(text) || text.trim().is_empty(),
        _ => false,
    }) {
        return None;
    }
    if elements.len() != rest_start + 1 {
        return None;
    }
    let wrapped = elements.pop()?;
    let inner = match wrapped {
        Element::List { items, .. } => items,
        Element::Partial(PartialElement::ListItem(ListItem::Elements {
            elements: mut body,
            ..
        })) if body.len() == 1 => match body.pop()? {
            Element::List { items, .. } => items,
            other => {
                body.push(other);
                elements.push(Element::Partial(PartialElement::ListItem(
                    ListItem::Elements {
                        elements: body,
                        attributes: AttributeMap::new(),
                    },
                )));
                return None;
            }
        },
        other => {
            elements.push(other);
            return None;
        }
    };
    if elements
        .iter()
        .all(|element| matches!(element, Element::Text(text) if text.trim().is_empty()))
    {
        elements.clear();
    }
    Some(inner)
}

fn quote_marker_residue(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && trimmed.bytes().all(|byte| byte == b'>')
}

fn merge_trailing_one_item_list<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let original = std::mem::take(elements);
    let mut changed = false;
    for element in original {
        let can_merge = matches!(
            (&element, elements.last()),
            (
                Element::List { items, ltype, .. },
                Some(Element::List {
                    ltype: previous,
                    ..
                })
            ) if items.len() == 1 && ltype == previous
        );
        if can_merge {
            let Element::List { items, .. } = element else {
                unreachable!("element was a one-item list");
            };
            let Some(Element::List {
                items: previous, ..
            }) = elements.last_mut()
            else {
                unreachable!("previous element was a list");
            };
            previous.extend(items);
            changed = true;
            continue;
        }
        elements.push(element);
    }
    changed
}

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
            if !footnotes_empty && needs_footnote_block_hoist(&output) {
                hoist_wikidot_footnote_blocks(&mut output);
            }
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
        } else if iter
            .peek()
            .is_some_and(|next| matches!(next, Element::Table(_)))
        {
            // Live's injection leaves a single newline between the footer and
            // the fragment after a marker authored on a table line; that
            // newline stays a break in front of the table the fragment forms.
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

fn needs_footnote_block_hoist(elements: &[Element<'_>]) -> bool {
    let mut stack: Vec<&[Element<'_>]> = vec![elements];
    while let Some(elements) = stack.pop() {
        for element in elements {
            match element {
                Element::Container(container)
                    if hoistable_line_container(container.ctype()) =>
                {
                    if container
                        .elements()
                        .iter()
                        .any(|child| matches!(child, Element::FootnoteBlock { .. }))
                    {
                        return true;
                    }
                    stack.push(container.elements());
                }
                Element::List {
                    items,
                    wikidot_explicit_block: false,
                    ..
                } => {
                    for item in items {
                        let ListItem::Elements { elements, .. } = item else {
                            continue;
                        };
                        if elements
                            .iter()
                            .any(|child| matches!(child, Element::FootnoteBlock { .. }))
                        {
                            return true;
                        }
                        stack.push(elements);
                    }
                }
                Element::Container(container) => stack.push(container.elements()),
                Element::Collapsible { elements, .. }
                | Element::Anchor { elements, .. }
                | Element::Color { elements, .. }
                | Element::Include { elements, .. } => stack.push(elements),
                Element::List { items, .. } => {
                    for item in items {
                        match item {
                            ListItem::Elements { elements, .. } => stack.push(elements),
                            ListItem::SubList { element } => {
                                stack.push(std::slice::from_ref(element.as_ref()))
                            }
                        }
                    }
                }
                Element::WikidotBlockFrame { children, .. } => stack.push(children),
                _ => {}
            }
        }
    }
    false
}

/// Live Wikidot hoists a `[[footnoteblock]]` out of the line container it
/// was authored in: a native list splits around it (same-depth remainder items
/// become a second list after the footer), and a blockquote, header or
/// definition list keeps only the content before the footer.
fn hoist_wikidot_footnote_blocks(elements: &mut Vec<Element<'_>>) {
    let taken = std::mem::take(elements);
    *elements = hoist_block_sequence(taken);
}

fn hoist_block_sequence<'t>(elements: Vec<Element<'t>>) -> Vec<Element<'t>> {
    let mut output = Vec::with_capacity(elements.len());
    for element in elements {
        match element {
            Element::Container(mut container)
                if hoistable_line_container(container.ctype()) =>
            {
                let children =
                    hoist_block_sequence(std::mem::take(container.elements_mut()));
                let mut children = children;
                let footer_position = children
                    .iter()
                    .position(|element| matches!(element, Element::FootnoteBlock { .. }));
                let Some(footer_position) = footer_position else {
                    *container.elements_mut() = children;
                    output.push(Element::Container(container));
                    continue;
                };

                let after = children.split_off(footer_position + 1);
                let footer = children
                    .pop()
                    .expect("footnote block position is the final element");
                let suffix_inline = matches!(
                    &footer,
                    Element::FootnoteBlock {
                        wikidot_inline_suffix: true,
                        ..
                    }
                );
                trim_hoisted_before(&mut children);
                let ctype = container.ctype();
                let attributes = container.attributes().clone();
                *container.elements_mut() = children;
                output.push(Element::Container(container));
                output.push(footer);
                if suffix_inline {
                    output.extend(after);
                } else if !after.is_empty() {
                    output.push(Element::Container(Container::new(
                        ctype, after, attributes,
                    )));
                }
            }

            Element::List {
                ltype,
                attributes,
                items,
                wikidot_explicit_block,
            } => {
                let (list, hoisted) = hoist_list_footnote_block(
                    ltype,
                    attributes,
                    items,
                    wikidot_explicit_block,
                );
                if let Some(list) = list {
                    output.push(list);
                }
                if let Some((footer, after)) = hoisted {
                    output.push(footer);
                    output.extend(after);
                }
            }

            other => output.push(hoist_nested_footnote_blocks(other)),
        }
    }
    output
}

fn hoistable_line_container(ctype: ContainerType) -> bool {
    matches!(ctype, ContainerType::Blockquote | ContainerType::Header(_))
}

fn hoist_nested_footnote_blocks<'t>(mut element: Element<'t>) -> Element<'t> {
    match &mut element {
        Element::Container(container) => {
            let children = hoist_block_sequence(std::mem::take(container.elements_mut()));
            *container.elements_mut() = children;
        }
        Element::Collapsible { elements, .. }
        | Element::Anchor { elements, .. }
        | Element::Color { elements, .. }
        | Element::Include { elements, .. } => {
            let children = hoist_block_sequence(std::mem::take(elements));
            *elements = children;
        }
        Element::WikidotBlockFrame { children, .. } => {
            let hoisted = hoist_block_sequence(std::mem::take(children));
            *children = hoisted;
        }
        _ => {}
    }
    element
}

fn hoist_list_footnote_block<'t>(
    ltype: ListType,
    attributes: AttributeMap<'t>,
    mut items: Vec<ListItem<'t>>,
    wikidot_explicit_block: bool,
) -> (Option<Element<'t>>, Option<(Element<'t>, Vec<Element<'t>>)>) {
    let rebuild =
        |items: Vec<ListItem<'t>>, attributes: AttributeMap<'t>| Element::List {
            ltype,
            attributes,
            items,
            wikidot_explicit_block,
        };

    if wikidot_explicit_block {
        return (Some(rebuild(items, attributes)), None);
    }

    let mut found = None;
    for index in 0..items.len() {
        let ListItem::Elements { elements, .. } = &mut items[index] else {
            continue;
        };
        let Some(position) = elements
            .iter()
            .position(|element| matches!(element, Element::FootnoteBlock { .. }))
        else {
            continue;
        };

        let after = elements.split_off(position + 1);
        let footer = elements
            .pop()
            .expect("footnote block position is the final element");
        found = Some((index, footer, after));
        break;
    }

    let Some((index, footer, after)) = found else {
        return (Some(rebuild(items, attributes)), None);
    };

    let suffix_inline = matches!(
        &footer,
        Element::FootnoteBlock {
            wikidot_inline_suffix: true,
            ..
        }
    );
    let mut after_elements = Vec::new();
    if suffix_inline && !after.is_empty() {
        after_elements.push(Element::LineBreak);
        after_elements.extend(after);
    } else {
        after_elements.extend(after);
    }

    let remainder_items = items.split_off(index + 1);
    let current_item_empty = matches!(&items[index], ListItem::Elements { elements, .. } if elements.is_empty());
    if current_item_empty {
        items.pop();
    }

    let first_list = if items.is_empty() {
        None
    } else {
        Some(rebuild(items, attributes.clone()))
    };
    if !remainder_items.is_empty() {
        after_elements.push(rebuild(remainder_items, attributes));
    }

    (first_list, Some((footer, after_elements)))
}

enum HoistTrim {
    Pop,
    TrimSpace,
    Paragraph,
    Stop,
}

fn hoist_trim_kind(element: &Element<'_>) -> HoistTrim {
    match element {
        Element::LineBreak => HoistTrim::Pop,
        Element::Container(container)
            if container.ctype() == ContainerType::Paragraph =>
        {
            HoistTrim::Paragraph
        }
        Element::Text(text) if text.chars().all(char::is_whitespace) => HoistTrim::Pop,
        Element::Text(text) if text.ends_with([' ', '\t']) => HoistTrim::TrimSpace,
        _ => HoistTrim::Stop,
    }
}

fn trim_hoisted_before(elements: &mut Vec<Element<'_>>) {
    loop {
        let kind = elements
            .last()
            .map(hoist_trim_kind)
            .unwrap_or(HoistTrim::Stop);
        match kind {
            HoistTrim::Pop => {
                elements.pop();
            }
            HoistTrim::TrimSpace => {
                if let Some(Element::Text(text)) = elements.last_mut() {
                    let trimmed = text.trim_end_matches([' ', '\t']).to_owned();
                    *text = Cow::Owned(trimmed);
                }
                break;
            }
            HoistTrim::Paragraph => {
                let Some(Element::Container(mut container)) = elements.pop() else {
                    break;
                };
                trim_hoisted_before(container.elements_mut());
                if !container.elements().is_empty() {
                    elements.push(Element::Container(container));
                }
                break;
            }
            HoistTrim::Stop => break,
        }
    }
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
