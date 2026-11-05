/*
 * tree/traversal.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2026 Wikijump Team
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

use super::{Element, ListItem, PartialElement, SyntaxTree};
#[cfg(not(target_arch = "wasm32"))]
use std::cell::Cell;

#[cfg(not(target_arch = "wasm32"))]
const DEEP_TREE_STACK_BYTES: usize = 64 * 1024 * 1024;
const BOUNDED_TREE_DEPTH_THRESHOLD: usize = 64;

#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    static ON_BOUNDED_TREE_STACK: Cell<bool> = const { Cell::new(false) };
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn on_bounded_tree_stack() -> bool {
    ON_BOUNDED_TREE_STACK.with(Cell::get)
}

#[cfg(target_arch = "wasm32")]
pub(crate) const fn on_bounded_tree_stack() -> bool {
    false
}

pub(crate) fn element_requires_bounded_tree_stack(element: &Element<'_>) -> bool {
    if on_bounded_tree_stack() {
        return false;
    }
    let mut stack = vec![(element, 1usize)];
    tree_stack_depth_reaches_threshold(&mut stack)
}

pub(crate) fn elements_require_bounded_tree_stack(elements: &[Element<'_>]) -> bool {
    if on_bounded_tree_stack() {
        return false;
    }
    let mut stack = Vec::with_capacity(BOUNDED_TREE_DEPTH_THRESHOLD);
    stack.extend(elements.iter().map(|element| (element, 1usize)));
    tree_stack_depth_reaches_threshold(&mut stack)
}

pub(crate) fn tree_requires_bounded_tree_stack(tree: &SyntaxTree<'_>) -> bool {
    if on_bounded_tree_stack() {
        return false;
    }
    let mut stack = Vec::with_capacity(BOUNDED_TREE_DEPTH_THRESHOLD);
    stack.extend(tree.elements.iter().map(|element| (element, 1usize)));
    stack.extend(
        tree.table_of_contents
            .iter()
            .map(|element| (element, 1usize)),
    );
    for footnote in &tree.footnotes {
        stack.extend(footnote.iter().map(|element| (element, 1usize)));
    }
    for bibliography in tree.bibliographies.slice() {
        for (_, elements) in bibliography.slice() {
            stack.extend(elements.iter().map(|element| (element, 1usize)));
        }
    }
    tree_stack_depth_reaches_threshold(&mut stack)
}

fn push_tree_elements<'a>(
    stack: &mut Vec<(&'a Element<'a>, usize)>,
    elements: &'a [Element<'a>],
    depth: usize,
) {
    stack.extend(elements.iter().map(|element| (element, depth)));
}

fn tree_stack_depth_reaches_threshold<'a>(
    stack: &mut Vec<(&'a Element<'a>, usize)>,
) -> bool {
    while let Some((element, depth)) = stack.pop() {
        if depth >= BOUNDED_TREE_DEPTH_THRESHOLD {
            return true;
        }
        let child_depth = depth + 1;

        match element {
            Element::Container(container) => {
                push_tree_elements(stack, container.elements(), child_depth);
            }
            Element::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        push_tree_elements(stack, &cell.elements, child_depth);
                    }
                }
            }
            Element::TabView(tabs) => {
                for item in tabs {
                    push_tree_elements(stack, item.elements(), child_depth);
                }
            }
            Element::Anchor { elements, .. }
            | Element::Collapsible { elements, .. }
            | Element::Color { elements, .. }
            | Element::Include { elements, .. } => {
                push_tree_elements(stack, elements, child_depth)
            }
            Element::WikidotBlockFrame { children, .. } => {
                push_tree_elements(stack, children, child_depth)
            }
            Element::List { items, .. } => {
                for item in items {
                    match item {
                        ListItem::Elements { elements, .. } => {
                            push_tree_elements(stack, elements, child_depth)
                        }
                        ListItem::SubList { element } => {
                            stack.push((element, child_depth));
                        }
                    }
                }
            }
            Element::DefinitionList(items) => {
                for item in items {
                    push_tree_elements(stack, &item.key_elements, child_depth);
                    push_tree_elements(stack, &item.value_elements, child_depth);
                }
            }
            Element::Partial(partial) => match partial {
                PartialElement::ListItem(ListItem::Elements { elements, .. }) => {
                    push_tree_elements(stack, elements, child_depth);
                }
                PartialElement::ListItem(ListItem::SubList { element }) => {
                    stack.push((element, child_depth));
                }
                PartialElement::TableRow(row) => {
                    for cell in &row.cells {
                        push_tree_elements(stack, &cell.elements, child_depth);
                    }
                }
                PartialElement::TableCell(cell) => {
                    push_tree_elements(stack, &cell.elements, child_depth)
                }
                PartialElement::WikidotFoster(elements) => {
                    push_tree_elements(stack, elements, child_depth)
                }
                PartialElement::WikidotCollapsibleTail(elements) => {
                    push_tree_elements(stack, elements, child_depth)
                }
                PartialElement::WikidotCollapsibleHeadingEnd => {}
                PartialElement::Tab(tab) => {
                    push_tree_elements(stack, &tab.elements, child_depth)
                }
                PartialElement::RubyText(ruby) => {
                    push_tree_elements(stack, &ruby.elements, child_depth)
                }
                PartialElement::WikidotEmptyInlineOwner
                | PartialElement::WikidotBlockOpen(_)
                | PartialElement::WikidotBlockClose(_)
                | PartialElement::InlineSizeOpen(..)
                | PartialElement::InlineSizeClose(_)
                | PartialElement::InlineSpanOpen(..)
                | PartialElement::InlineSpanClose(_)
                | PartialElement::InlineFormatOpen(_)
                | PartialElement::InlineFormatClose(_)
                | PartialElement::InlineColorOpen(..)
                | PartialElement::InlineColorClose
                | PartialElement::InlineAnchorOpen(..)
                | PartialElement::InlineAnchorClose(_) => {}
            },
            _ => {}
        }
    }
    false
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn run_on_bounded_tree_stack<T, F>(name: &str, operation: F) -> T
where
    T: Send,
    F: FnOnce() -> T + Send,
{
    if on_bounded_tree_stack() {
        return operation();
    }
    std::thread::scope(|scope| {
        let handle = std::thread::Builder::new()
            .name(name.to_owned())
            .stack_size(DEEP_TREE_STACK_BYTES)
            .spawn_scoped(scope, || {
                ON_BOUNDED_TREE_STACK.with(|marker| {
                    debug_assert!(!marker.get());
                    marker.set(true);
                    let output = operation();
                    marker.set(false);
                    output
                })
            })
            .unwrap_or_else(|error| {
                panic!("unable to start bounded FTML tree worker {name}: {error}")
            });
        match handle.join() {
            Ok(output) => output,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    })
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn run_on_bounded_tree_stack<T, F>(_name: &str, operation: F) -> T
where
    F: FnOnce() -> T,
{
    operation()
}
