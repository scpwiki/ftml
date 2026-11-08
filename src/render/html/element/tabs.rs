/*
 * render/html/element/tabs.rs
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
use crate::tree::{
    AttributeMap, ContainerType, ListItem, PartialElement, SyntaxTree, TabViewItem,
};
use std::iter;

pub fn render_tabview(ctx: &mut HtmlContext, items: &[TabViewItem]) {
    debug!("Rendering tabview (items {})", items.len());

    if ctx.layout().legacy() {
        render_wikidot_tabview(ctx, items);
        return;
    }

    let tab_count = items
        .iter()
        .filter(|item| matches!(item, TabViewItem::Tab(_) | TabViewItem::Remote(_)))
        .count();
    let button_ids = generate_ids(ctx.random(), tab_count);
    let tab_ids = generate_ids(ctx.random(), tab_count);

    ctx.html()
        .element("wj-tabs")
        .attr(attr!(
            "class" => "wj-tabs",
        ))
        .inner(|ctx| {
            ctx.html()
                .div()
                .attr(attr!(
                    "class" => "wj-tabs-button-list",
                    "role" => "tablist",
                ))
                .inner(|ctx| {
                    let mut tab_index = 0usize;
                    for item in items {
                        let (TabViewItem::Tab(tab) | TabViewItem::Remote(tab)) = item
                        else {
                            continue;
                        };
                        let (tab_selected, tab_position) = if tab_index == 0 {
                            ("true", "0")
                        } else {
                            ("false", "-1")
                        };

                        ctx.html()
                            .element("wj-tabs-button")
                            .attr(attr!(
                                "class" => "wj-tabs-button",
                                "id" => &button_ids[tab_index],
                                "role" => "tab",
                                "aria-label" => &tab.label,
                                "aria-selected" => tab_selected,
                                "aria-controls" => &tab_ids[tab_index],
                                "tabindex" => tab_position,
                            ))
                            .contents(&tab.label);
                        tab_index += 1;
                    }
                });

            ctx.html()
                .div()
                .attr(attr!(
                    "class" => "wj-tabs-panel-list",
                ))
                .inner(|ctx| {
                    let mut tab_index = 0usize;
                    for item in items {
                        match item {
                            TabViewItem::Tab(tab) | TabViewItem::Remote(tab) => {
                                ctx.html()
                                    .div()
                                    .attr(attr!(
                                        "class" => "wj-tabs-panel",
                                        "id" => &tab_ids[tab_index],
                                        "role" => "tabpanel",
                                        "aria-labelledby" => &button_ids[tab_index],
                                        "tabindex" => "0",
                                        "hidden"; if tab_index > 0,
                                    ))
                                    .contents(&tab.elements);
                                tab_index += 1;
                            }
                            TabViewItem::Content(elements) => {
                                ctx.html()
                                    .div()
                                    .attr(attr!(
                                        "class" => "wj-tabs-content",
                                    ))
                                    .contents(elements);
                            }
                        }
                    }
                });
        });
}

/// A marker in the document-order tab/view sequence of one outermost
/// tabview subtree. Labels are identified by document-order tab index so the
/// flat Text_Wiki lists can be mapped back to hierarchical tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavMarker {
    ViewOpen(usize),
    ViewClose,
    TabOpen(usize),
    TabClose,
}

/// Compute Wikidot's flat nav lists for cleanly nested tabviews.
///
/// Live Wikidot's `Text_Wiki_Parse_Tabview` matches a view with the regex
/// `^[[tabview]] (([[tab]]...[[/tab]])+) [[/tabview]]` (tab bodies lazily to
/// the first `[[/tab]]`) and records the matched tabs as that view's nav.
/// Consuming the first descendant view's closer as the outer view's closer
/// and then re-matching the nested open on the remainder permutes the nav
/// labels across views while the `tabStart`/`tabEnd` tokens (panels) stay in
/// hierarchical order. This marker simulation reproduces those flat lists
/// (verified against saved-page probes `muse-probe-tvp-o3-*`); the renderer
/// then emits panels hierarchically and navs from the simulation.
///
/// Only tab-clean subtrees participate: every descendant view must consist of
/// `Tab` items only (no `Remote` split tabs), and every view must match.
/// `Content` runs (stray/detached closers, prose, literal marker text)
/// inside the subtree are ignored when building markers, exactly as the
/// saved-page recovery shapes (`muse-probe-tfp-w1/w8/w9/w10` and
/// `muse-probe-tfa-v*`) absorb with the permutation applied to the
/// post-recovery tab set; live does not let such runs stop the flat match.
/// Anything else keeps the hierarchical navs, preserving the stray/failed
/// pins.
pub(crate) fn preregister_wikidot_tabview_navs<'t>(
    ctx: &mut HtmlContext<'_, '_, '_, 't>,
    tree: &SyntaxTree<'t>,
) {
    let mut roots: Vec<&[Element<'_>]> = vec![&tree.elements];
    for footnote in ctx.footnotes() {
        roots.push(footnote.as_slice());
    }
    for bibliography in tree.bibliographies.slice() {
        for (_, elements) in bibliography.slice() {
            roots.push(elements.as_slice());
        }
    }
    for root in roots {
        let mut stack: Vec<(&[Element<'_>], usize)> = vec![(root, 0)];
        let mut visited_footnotes: Vec<usize> = Vec::new();
        while let Some((elements, position)) = stack.pop() {
            let Some(element) = elements.get(position) else {
                continue;
            };
            stack.push((elements, position + 1));
            match element {
                Element::TabView(tabs) => {
                    preregister_one_tabview(ctx, tabs);
                }
                Element::Container(container) => {
                    stack.push((container.elements(), 0));
                }
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
                Element::Anchor { elements, .. } => stack.push((elements.as_slice(), 0)),
                Element::Collapsible {
                    elements,
                    wikidot_block_open_elements,
                    unfolded_tail_start,
                    ..
                } => {
                    let (elements, unfolded_tail) = unfolded_tail_start
                        .map(|start| elements.split_at(start.min(elements.len())))
                        .unwrap_or((elements.as_slice(), &[]));
                    stack.push((unfolded_tail, 0));
                    stack.push((elements, 0));
                    stack.push((wikidot_block_open_elements.as_slice(), 0));
                }
                Element::Color { elements, .. } => stack.push((elements.as_slice(), 0)),
                Element::Include { elements, .. } => stack.push((elements.as_slice(), 0)),
                Element::WikidotBlockFrame { children, .. } => {
                    stack.push((children.as_slice(), 0))
                }
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
                Element::Footnote(reference) => {
                    let index = reference.index;
                    if !visited_footnotes.contains(&index)
                        && let Some(body) = ctx.footnotes().get(index.wrapping_sub(1))
                    {
                        visited_footnotes.push(index);
                        stack.push((body.as_slice(), 0));
                    }
                }
                Element::Partial(partial) => match partial {
                    PartialElement::Tab(tab) => stack.push((&tab.elements, 0)),
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
                    PartialElement::TableCell(cell) => stack.push((&cell.elements, 0)),
                    PartialElement::WikidotFoster(elements) => {
                        stack.push((elements.as_slice(), 0))
                    }
                    PartialElement::WikidotCollapsibleTail(elements) => {
                        stack.push((elements.as_slice(), 0))
                    }
                    PartialElement::RubyText(ruby) => stack.push((&ruby.elements, 0)),
                    _ => {}
                },
                _ => {}
            }
        }
    }
}

fn preregister_one_tabview<'t>(
    ctx: &mut HtmlContext<'_, '_, '_, 't>,
    tabs: &[TabViewItem<'t>],
) {
    // Only nested subtrees need a permutation; a view with no descendant
    // TabView has identical flat and hierarchical navs.
    if !tabview_subtree_has_nested_tabview(tabs) {
        return;
    }
    let mut views: Vec<usize> = vec![tabs.as_ptr() as usize];
    let mut labels: Vec<(String, Vec<AttributeMap<'static>>)> = Vec::new();
    let mut events: Vec<NavMarker> = vec![NavMarker::ViewOpen(0)];
    let mut footnote_stack: Vec<usize> = Vec::new();
    if !collect_tabview_items(
        ctx,
        tabs,
        &mut views,
        &mut labels,
        &mut events,
        &mut footnote_stack,
    ) {
        return;
    }
    events.push(NavMarker::ViewClose);
    let Some(assignments) = simulate_tabview_navs(&events, views.len()) else {
        return;
    };
    for (view_index, tab_indices) in assignments.iter().enumerate() {
        let view_ptr = views[view_index];
        let nav: Vec<(String, Vec<AttributeMap<'static>>)> = tab_indices
            .iter()
            .filter_map(|tab_index| labels.get(*tab_index).cloned())
            .collect();
        if nav.len() != tab_indices.len() {
            return;
        }
        ctx.assign_wikidot_tabview_nav(view_ptr, nav);
    }
}

fn tabview_subtree_has_nested_tabview(tabs: &[TabViewItem<'_>]) -> bool {
    fn elements_have_nested(elements: &[Element<'_>]) -> bool {
        elements.iter().any(|element| match element {
            Element::TabView(_) => true,
            Element::Container(container) => elements_have_nested(container.elements()),
            Element::Footnote(_) => false,
            Element::Table(table) => table.rows.iter().any(|row| {
                row.cells
                    .iter()
                    .any(|cell| elements_have_nested(&cell.elements))
            }),
            Element::Anchor { elements, .. }
            | Element::Collapsible { elements, .. }
            | Element::Color { elements, .. }
            | Element::Include { elements, .. }
            | Element::WikidotBlockFrame {
                children: elements, ..
            } => elements_have_nested(elements),
            Element::List { items, .. } => items.iter().any(|item| match item {
                ListItem::Elements { elements, .. } => elements_have_nested(elements),
                ListItem::SubList { element } => {
                    elements_have_nested(std::slice::from_ref(&**element))
                }
            }),
            Element::DefinitionList(items) => items.iter().any(|item| {
                elements_have_nested(&item.key_elements)
                    || elements_have_nested(&item.value_elements)
            }),
            Element::Partial(partial) => match partial {
                PartialElement::Tab(tab) => elements_have_nested(&tab.elements),
                PartialElement::ListItem(ListItem::Elements { elements, .. }) => {
                    elements_have_nested(elements)
                }
                PartialElement::ListItem(ListItem::SubList { element }) => {
                    elements_have_nested(std::slice::from_ref(&**element))
                }
                PartialElement::TableRow(row) => row
                    .cells
                    .iter()
                    .any(|cell| elements_have_nested(&cell.elements)),
                PartialElement::TableCell(cell) => elements_have_nested(&cell.elements),
                PartialElement::WikidotFoster(elements)
                | PartialElement::WikidotCollapsibleTail(elements) => {
                    elements_have_nested(elements)
                }
                PartialElement::RubyText(ruby) => elements_have_nested(&ruby.elements),
                _ => false,
            },
            _ => false,
        })
    }
    tabs.iter()
        .any(|item| elements_have_nested(item.elements()))
}

fn collect_tabview_items<'t>(
    ctx: &HtmlContext<'_, '_, '_, 't>,
    items: &[TabViewItem<'t>],
    views: &mut Vec<usize>,
    labels: &mut Vec<(String, Vec<AttributeMap<'static>>)>,
    events: &mut Vec<NavMarker>,
    footnote_stack: &mut Vec<usize>,
) -> bool {
    for item in items {
        match item {
            TabViewItem::Tab(tab) => {
                let tab_index = labels.len();
                labels.push((
                    tab.label.to_string(),
                    tab.wikidot_label_spans
                        .iter()
                        .map(AttributeMap::to_owned)
                        .collect(),
                ));
                events.push(NavMarker::TabOpen(tab_index));
                if !collect_elements(
                    ctx,
                    &tab.elements,
                    views,
                    labels,
                    events,
                    footnote_stack,
                ) {
                    return false;
                }
                events.push(NavMarker::TabClose);
            }
            TabViewItem::Remote(_) => return false,
            // Content runs carry no tab markers: live's flat match scans
            // straight past stray/prose text between (or after) the tabs
            // it collects (verified against the recovery saved pages).
            TabViewItem::Content(_) => {}
        }
    }
    true
}

#[allow(clippy::collapsible_match)]
fn collect_elements<'t>(
    ctx: &HtmlContext<'_, '_, '_, 't>,
    elements: &[Element<'t>],
    views: &mut Vec<usize>,
    labels: &mut Vec<(String, Vec<AttributeMap<'static>>)>,
    events: &mut Vec<NavMarker>,
    footnote_stack: &mut Vec<usize>,
) -> bool {
    for element in elements {
        match element {
            Element::Container(container) => {
                if !collect_elements(
                    ctx,
                    container.elements(),
                    views,
                    labels,
                    events,
                    footnote_stack,
                ) {
                    return false;
                }
            }
            Element::TabView(tabs) => {
                let view_index = views.len();
                views.push(tabs.as_ptr() as usize);
                events.push(NavMarker::ViewOpen(view_index));
                if !collect_tabview_items(
                    ctx,
                    tabs,
                    views,
                    labels,
                    events,
                    footnote_stack,
                ) {
                    return false;
                }
                events.push(NavMarker::ViewClose);
            }
            Element::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        if !collect_elements(
                            ctx,
                            &cell.elements,
                            views,
                            labels,
                            events,
                            footnote_stack,
                        ) {
                            return false;
                        }
                    }
                }
            }
            Element::Anchor { elements, .. } => {
                if !collect_elements(ctx, elements, views, labels, events, footnote_stack)
                {
                    return false;
                }
            }
            Element::Collapsible {
                elements,
                wikidot_block_open_elements,
                unfolded_tail_start,
                ..
            } => {
                if !collect_elements(
                    ctx,
                    wikidot_block_open_elements,
                    views,
                    labels,
                    events,
                    footnote_stack,
                ) {
                    return false;
                }
                let (elements, unfolded_tail) = unfolded_tail_start
                    .map(|start| elements.split_at(start.min(elements.len())))
                    .unwrap_or((elements.as_slice(), &[][..]));
                if !collect_elements(ctx, elements, views, labels, events, footnote_stack)
                {
                    return false;
                }
                if !collect_elements(
                    ctx,
                    unfolded_tail,
                    views,
                    labels,
                    events,
                    footnote_stack,
                ) {
                    return false;
                }
            }
            Element::Color { elements, .. }
            | Element::Include { elements, .. }
            | Element::WikidotBlockFrame {
                children: elements, ..
            } => {
                if !collect_elements(ctx, elements, views, labels, events, footnote_stack)
                {
                    return false;
                }
            }
            Element::List { items, .. } => {
                for item in items {
                    match item {
                        ListItem::Elements { elements, .. } => {
                            if !collect_elements(
                                ctx,
                                elements,
                                views,
                                labels,
                                events,
                                footnote_stack,
                            ) {
                                return false;
                            }
                        }
                        ListItem::SubList { element } => {
                            if !collect_elements(
                                ctx,
                                std::slice::from_ref(&**element),
                                views,
                                labels,
                                events,
                                footnote_stack,
                            ) {
                                return false;
                            }
                        }
                    }
                }
            }
            Element::DefinitionList(items) => {
                for item in items {
                    if !collect_elements(
                        ctx,
                        &item.key_elements,
                        views,
                        labels,
                        events,
                        footnote_stack,
                    ) {
                        return false;
                    }
                    if !collect_elements(
                        ctx,
                        &item.value_elements,
                        views,
                        labels,
                        events,
                        footnote_stack,
                    ) {
                        return false;
                    }
                }
            }
            Element::Footnote(reference) => {
                let index = reference.index;
                if footnote_stack.contains(&index) {
                    continue;
                }
                let Some(body) = ctx.footnotes().get(index.wrapping_sub(1)) else {
                    continue;
                };
                footnote_stack.push(index);
                let ok =
                    collect_elements(ctx, body, views, labels, events, footnote_stack);
                footnote_stack.pop();
                if !ok {
                    return false;
                }
            }
            Element::Partial(partial) => match partial {
                PartialElement::Tab(tab) => {
                    if !collect_elements(
                        ctx,
                        &tab.elements,
                        views,
                        labels,
                        events,
                        footnote_stack,
                    ) {
                        return false;
                    }
                }
                PartialElement::ListItem(ListItem::Elements { elements, .. }) => {
                    if !collect_elements(
                        ctx,
                        elements,
                        views,
                        labels,
                        events,
                        footnote_stack,
                    ) {
                        return false;
                    }
                }
                PartialElement::ListItem(ListItem::SubList { element }) => {
                    if !collect_elements(
                        ctx,
                        std::slice::from_ref(&**element),
                        views,
                        labels,
                        events,
                        footnote_stack,
                    ) {
                        return false;
                    }
                }
                PartialElement::TableRow(row) => {
                    for cell in &row.cells {
                        if !collect_elements(
                            ctx,
                            &cell.elements,
                            views,
                            labels,
                            events,
                            footnote_stack,
                        ) {
                            return false;
                        }
                    }
                }
                PartialElement::TableCell(cell) => {
                    if !collect_elements(
                        ctx,
                        &cell.elements,
                        views,
                        labels,
                        events,
                        footnote_stack,
                    ) {
                        return false;
                    }
                }
                PartialElement::WikidotFoster(elements)
                | PartialElement::WikidotCollapsibleTail(elements) => {
                    if !collect_elements(
                        ctx,
                        elements,
                        views,
                        labels,
                        events,
                        footnote_stack,
                    ) {
                        return false;
                    }
                }
                PartialElement::RubyText(ruby) => {
                    if !collect_elements(
                        ctx,
                        &ruby.elements,
                        views,
                        labels,
                        events,
                        footnote_stack,
                    ) {
                        return false;
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    true
}

fn simulate_tabview_navs(
    events: &[NavMarker],
    view_count: usize,
) -> Option<Vec<Vec<usize>>> {
    let mut consumed = vec![false; events.len()];
    let mut assignments: Vec<Option<Vec<usize>>> = vec![None; view_count];
    // Each pass mirrors one Text_Wiki `preg_replace_callback` run: matches
    // are non-overlapping left-to-right, and a matched view's span is
    // skipped for the rest of the pass, so nested opens match next pass.
    loop {
        let mut changed = false;
        let mut scan = 0usize;
        while scan < events.len() {
            let NavMarker::ViewOpen(view_index) = events[scan] else {
                scan += 1;
                continue;
            };
            if consumed[scan] {
                scan += 1;
                continue;
            }
            let mut pos = scan + 1;
            let mut flat: Vec<(usize, usize)> = Vec::new();
            let mut close_pos = None;
            while let NavMarker::TabOpen(_) =
                events.get(pos).copied().unwrap_or(NavMarker::ViewClose)
            {
                if consumed[pos] {
                    break;
                }
                let mut close = pos + 1;
                while close < events.len()
                    && !(events[close] == NavMarker::TabClose && !consumed[close])
                {
                    close += 1;
                }
                if close >= events.len() {
                    break;
                }
                flat.push((pos, close));
                pos = close + 1;
                if events.get(pos).is_some_and(|marker| {
                    matches!(marker, NavMarker::TabOpen(_)) && !consumed[pos]
                }) {
                    continue;
                }
                if events.get(pos) == Some(&NavMarker::ViewClose) && !consumed[pos] {
                    close_pos = Some(pos);
                }
                break;
            }
            let Some(close_pos) = close_pos else {
                scan += 1;
                continue;
            };
            if flat.is_empty() {
                scan += 1;
                continue;
            }
            consumed[scan] = true;
            consumed[close_pos] = true;
            for (open, close) in &flat {
                consumed[*open] = true;
                consumed[*close] = true;
            }
            let tab_indices = flat
                .iter()
                .filter_map(|(open, _)| match events[*open] {
                    NavMarker::TabOpen(index) => Some(index),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if assignments[view_index].is_some() {
                return None;
            }
            assignments[view_index] = Some(tab_indices);
            changed = true;
            scan = close_pos + 1;
        }
        if !changed {
            break;
        }
    }
    let mut result = Vec::with_capacity(view_count);
    for assignment in assignments {
        result.push(assignment?);
    }
    Some(result)
}

fn render_wikidot_tab_anchor(
    ctx: &mut HtmlContext,
    label: &str,
    label_spans: &[AttributeMap],
) {
    fn render_wrapped(ctx: &mut HtmlContext, label: &str, label_spans: &[AttributeMap]) {
        let Some((attributes, remaining)) = label_spans.split_first() else {
            ctx.html()
                .a()
                .attr(attr!("href" => "javascript:;"))
                .inner(|ctx| {
                    // Live emits an empty anchor, not an
                    // empty <em> wrapper, when the title
                    // resolves to nothing.
                    if !label.is_empty() {
                        ctx.html().em().contents(label);
                    }
                });
            return;
        };
        ctx.html().span().attr(attr!(;; attributes)).inner(|ctx| {
            render_wrapped(ctx, label, remaining);
        });
    }
    render_wrapped(ctx, label, label_spans);
}

fn tabview_wrapper_tail<'t>(items: &'t [TabViewItem<'t>]) -> Option<&'t [Element<'t>]> {
    let TabViewItem::Content(elements) = items.last()? else {
        return None;
    };
    let [Element::Container(container)] = elements.as_slice() else {
        return None;
    };
    container
        .attributes()
        .get()
        .contains_key(crate::tree::WIKIDOT_TABVIEW_TAIL_MARKER)
        .then_some(container.elements())
}

fn render_wikidot_tabview(ctx: &mut HtmlContext, items: &[TabViewItem]) {
    let depth = ctx.wikidot_tabview_depth();
    ctx.set_wikidot_tabview_depth(depth + 1);
    let (items, trailing) = split_bibliography_split_tail(items);
    let tail = tabview_wrapper_tail(items);
    let items = if tail.is_some() {
        items.split_last().map(|(_, head)| head).unwrap_or(items)
    } else {
        items
    };
    let id = ctx.random().generate_wikidot_tabview_id();
    ctx.require_wikidot_tab_view(id.clone());

    let nav_override = ctx.wikidot_tabview_nav(items.as_ptr() as usize).cloned();
    ctx.html()
        .div()
        .attr(attr!("id" => &id, "class" => "yui-navset"))
        .inner(|ctx| {
            ctx.html()
                .ul()
                .attr(attr!("class" => "yui-nav"))
                .inner(|ctx| {
                    if let Some(nav) = &nav_override {
                        for (tab_index, (label, spans)) in nav.iter().enumerate() {
                            ctx.html()
                                .li()
                                .attr(attr!(
                                    "class" => "selected"; if tab_index == 0,
                                ))
                                .inner(|ctx| {
                                    render_wikidot_tab_anchor(ctx, label, spans);
                                });
                            ctx.push_raw('\n');
                        }
                        return;
                    }
                    let mut tab_index = 0usize;
                    for item in items {
                        let (TabViewItem::Tab(tab) | TabViewItem::Remote(tab)) = item
                        else {
                            continue;
                        };
                        ctx.html()
                            .li()
                            .attr(attr!(
                                "class" => "selected"; if tab_index == 0,
                            ))
                            .inner(|ctx| {
                                render_wikidot_tab_anchor(
                                    ctx,
                                    &tab.label,
                                    &tab.wikidot_label_spans,
                                );
                            });
                        ctx.push_raw('\n');
                        tab_index += 1;
                    }
                });

            let tab_count = items
                .iter()
                .filter(|item| {
                    matches!(item, TabViewItem::Tab(_) | TabViewItem::Remote(_))
                })
                .count();
            // A stray escaped `[[/tab]]` inside a nested view's Content run
            // ends that view's `yui-content` at the marker (live leaves the
            // marker in the outermost view's content; see
            // `find_wikidot_tabview_break`). The rendering below emits the
            // prefix items into `yui-content` and the rest into the spill
            // region after it, both inside this navset.
            let break_point = if depth >= 1 && tab_count > 0 {
                find_wikidot_tabview_break(ctx, items)
            } else {
                None
            };
            let multi_tab_recovery = tab_count > 1;
            let mut tab_index = 0usize;
            let mut deferred_lift_break = false;
            ctx.html()
                .div()
                .attr(attr!("class" => "yui-content"))
                .inner(|ctx| {
                    if tab_count == 0 {
                        render_wikidot_tabview_content_only(ctx, items);
                        return;
                    }
                    let prefix_end = break_point
                        .as_ref()
                        .map_or(items.len(), |point| point.item_index + 1);
                    for (position, item) in items[..prefix_end].iter().enumerate() {
                        if depth == 0 {
                            if deferred_lift_break {
                                ctx.push_raw_str("<br>");
                                deferred_lift_break = false;
                            }
                            // The first stray in an outermost item lifts
                            // (probes `muse-probe-tte-*`); later strays in
                            // the same outermost item stay literal.
                            ctx.arm_wikidot_tabview_lift();
                        }
                        if let Some(point) = &break_point
                            && position == point.item_index
                            && let Some((pre_end, _)) = point.run_split
                            && let TabViewItem::Content(elements) = item
                        {
                            render_wikidot_tabview_yui_item(
                                ctx,
                                item,
                                &mut tab_index,
                                multi_tab_recovery,
                                Some(&elements[..pre_end]),
                                depth,
                            );
                        } else {
                            render_wikidot_tabview_yui_item(
                                ctx,
                                item,
                                &mut tab_index,
                                multi_tab_recovery,
                                None,
                                depth,
                            );
                        }
                        if depth == 0 {
                            deferred_lift_break =
                                flush_wikidot_tabview_lifts(ctx, deferred_lift_break);
                        }
                    }
                });
            if let Some(point) = &break_point {
                if let (Some((_, post_start)), Some(item)) =
                    (point.run_split, items.get(point.item_index))
                    && let TabViewItem::Content(elements) = item
                {
                    render_wikidot_tabview_yui_item(
                        ctx,
                        item,
                        &mut tab_index,
                        multi_tab_recovery,
                        Some(&elements[post_start..]),
                        depth,
                    );
                }
                for item in &items[point.item_index + 1..] {
                    render_wikidot_tabview_yui_item(
                        ctx,
                        item,
                        &mut tab_index,
                        multi_tab_recovery,
                        None,
                        depth,
                    );
                }
            }
            if let Some(tail) = tail {
                let tail = match tail {
                    [head @ .., Element::LineBreak] => head,
                    tail => tail,
                };
                render_elements(ctx, tail);
            }
            for element in trailing {
                render_element(ctx, element);
            }
        });
    ctx.set_wikidot_tabview_depth(depth);
}

/// Where a nested legacy tabview's `yui-content` stops because of a stray
/// escaped `[[/tab]]` in a Content run: the last prefix item, plus the split
/// of that item's run when the stray is in this view's own Content item.
struct WikidotTabviewBreak {
    item_index: usize,
    run_split: Option<(usize, usize)>,
}

/// Find the first stray escaped `[[/tab]]` in document order and, when it is
/// in this view's own Content run, queue its lift to the outermost view.
/// When the stray is in a descendant view, this view only breaks after the
/// item containing it; the descendant queues the lift when it renders.
fn find_wikidot_tabview_break(
    ctx: &mut HtmlContext,
    items: &[TabViewItem],
) -> Option<WikidotTabviewBreak> {
    if !ctx.wikidot_tabview_lift_armed() {
        return None;
    }
    for (index, item) in items.iter().enumerate() {
        match item {
            TabViewItem::Content(elements) => {
                if let Some(split) = wikidot_tabview_stray_split(elements) {
                    ctx.queue_wikidot_tabview_lift();
                    return Some(WikidotTabviewBreak {
                        item_index: index,
                        run_split: Some(split),
                    });
                }
            }
            TabViewItem::Tab(tab) | TabViewItem::Remote(tab) => {
                if elements_contain_stray_tab_close(&tab.elements) {
                    return Some(WikidotTabviewBreak {
                        item_index: index,
                        run_split: None,
                    });
                }
            }
        }
    }
    None
}

/// Split a Content run at its first literal `[[/tab]]` text run, returning
/// the pre-marker end (with trailing line breaks dropped: live ends the
/// content before the marker without them) and the post-marker start (with
/// one leading line break dropped: the marker line's own break is consumed
/// by the lift, whose `<br>[[/tab]]<br>` carries it).
fn wikidot_tabview_stray_split(elements: &[Element]) -> Option<(usize, usize)> {
    let flat: Vec<&Element> = elements.iter().collect();
    for start in 0..flat.len() {
        if let Some(end) = match_wikidot_literal_text_run(&flat, start, "[[/tab]]") {
            let mut pre_end = start;
            while pre_end > 0
                && matches!(
                    flat[pre_end - 1],
                    Element::LineBreak | Element::LineBreaks { .. }
                )
            {
                pre_end -= 1;
            }
            let mut post_start = end;
            if matches!(
                flat.get(post_start),
                Some(Element::LineBreak | Element::LineBreaks { .. })
            ) {
                post_start += 1;
            }
            return Some((pre_end, post_start));
        }
    }
    None
}

/// Whether any descendant tabview in these elements has a stray escaped
/// `[[/tab]]` in one of its Content runs (so an enclosing view's content
/// breaks after the item holding it).
fn elements_contain_stray_tab_close(elements: &[Element]) -> bool {
    elements.iter().any(|element| match element {
        Element::TabView(items) => items.iter().any(|item| match item {
            TabViewItem::Content(elements) => {
                wikidot_tabview_stray_split(elements).is_some()
            }
            TabViewItem::Tab(tab) | TabViewItem::Remote(tab) => {
                elements_contain_stray_tab_close(&tab.elements)
            }
        }),
        Element::Container(container) => {
            elements_contain_stray_tab_close(container.elements())
        }
        Element::Table(table) => table.rows.iter().any(|row| {
            row.cells
                .iter()
                .any(|cell| elements_contain_stray_tab_close(&cell.elements))
        }),
        Element::Anchor { elements, .. }
        | Element::Color { elements, .. }
        | Element::Include { elements, .. }
        | Element::WikidotBlockFrame {
            children: elements, ..
        } => elements_contain_stray_tab_close(elements),
        Element::Collapsible {
            elements,
            wikidot_block_open_elements,
            unfolded_tail_start,
            ..
        } => {
            let (elements, unfolded_tail) = unfolded_tail_start
                .map(|start| elements.split_at(start.min(elements.len())))
                .unwrap_or((elements.as_slice(), &[][..]));
            elements_contain_stray_tab_close(elements)
                || elements_contain_stray_tab_close(unfolded_tail)
                || elements_contain_stray_tab_close(wikidot_block_open_elements)
        }
        Element::List { items, .. } => items.iter().any(|item| match item {
            ListItem::Elements { elements, .. } => {
                elements_contain_stray_tab_close(elements)
            }
            ListItem::SubList { element } => {
                elements_contain_stray_tab_close(std::slice::from_ref(&**element))
            }
        }),
        Element::DefinitionList(items) => items.iter().any(|item| {
            elements_contain_stray_tab_close(&item.key_elements)
                || elements_contain_stray_tab_close(&item.value_elements)
        }),
        Element::Partial(partial) => match partial {
            PartialElement::Tab(tab) => elements_contain_stray_tab_close(&tab.elements),
            PartialElement::ListItem(ListItem::Elements { elements, .. }) => {
                elements_contain_stray_tab_close(elements)
            }
            PartialElement::ListItem(ListItem::SubList { element }) => {
                elements_contain_stray_tab_close(std::slice::from_ref(&**element))
            }
            PartialElement::TableRow(row) => row
                .cells
                .iter()
                .any(|cell| elements_contain_stray_tab_close(&cell.elements)),
            PartialElement::TableCell(cell) => {
                elements_contain_stray_tab_close(&cell.elements)
            }
            PartialElement::WikidotFoster(elements)
            | PartialElement::WikidotCollapsibleTail(elements) => {
                elements_contain_stray_tab_close(elements)
            }
            PartialElement::RubyText(ruby) => {
                elements_contain_stray_tab_close(&ruby.elements)
            }
            _ => false,
        },
        _ => false,
    })
}

/// Render one tabview item into `yui-content` (or the spill region after it).
/// `content_run`, when set, renders that slice of a Content run instead of
/// the whole run (the stray-break prefix/post parts).
fn render_wikidot_tabview_yui_item(
    ctx: &mut HtmlContext,
    item: &TabViewItem,
    tab_index: &mut usize,
    multi_tab_recovery: bool,
    content_run: Option<&[Element]>,
    depth: usize,
) {
    match item {
        TabViewItem::Tab(tab) => {
            let tab_id = format!("wiki-tab-0-{tab_index}");
            let index = *tab_index;
            *tab_index += 1;
            // Live closes an outermost panel right after a strayed inner
            // view: everything after it in the panel spills into the
            // outermost content (probes `muse-probe-tte-m1/m2`).
            let panel_break = if depth == 0 {
                tab.elements
                    .iter()
                    .enumerate()
                    .find(|(_, element)| {
                        elements_contain_stray_tab_close(std::slice::from_ref(element))
                    })
                    .map(|(position, _)| position)
                    .filter(|position| *position + 1 < tab.elements.len())
            } else {
                None
            };
            if let Some(break_at) = panel_break {
                if index > 0 {
                    ctx.push_raw_str(&format!(
                        "<div id=\"{tab_id}\" style=\"display:none\">"
                    ));
                } else {
                    ctx.push_raw_str(&format!("<div id=\"{tab_id}\">"));
                }
                render_elements(ctx, &tab.elements[..=break_at]);
                ctx.push_raw_str("</div>");
                render_elements(ctx, &tab.elements[break_at + 1..]);
            } else {
                // Live Wikidot hides every panel after the first inline.
                ctx.html()
                    .div()
                    .attr(attr!(
                        "id" => &tab_id,
                        "style" => "display:none"; if index > 0,
                    ))
                    .contents(&tab.elements);
            }
        }
        // The panel is rendered as a bare div in a later
        // quote segment; only the nav entry lives here.
        TabViewItem::Remote(_) => {
            *tab_index += 1;
        }
        TabViewItem::Content(elements) => {
            let run = content_run.unwrap_or(elements);
            let flat: Vec<&Element> = run.iter().collect();
            if wikidot_tabview_content_has_literal_tabview_closer(&flat) {
                render_wikidot_tabview_content_stream(ctx, &flat, multi_tab_recovery);
            } else {
                render_elements(ctx, run);
            }
        }
    }
}

/// Emit queued lifted `[[/tab]]` markers in the outermost view's content:
/// each as `<br>[[/tab]]`, with the trailing `<br>` deferred until the next
/// item (live drops it when the marker ends the content). Returns whether a
/// deferred break is pending.
fn flush_wikidot_tabview_lifts(ctx: &mut HtmlContext, deferred_break: bool) -> bool {
    let mut lifts = ctx.take_wikidot_tabview_lifts();
    let mut deferred = deferred_break;
    while lifts > 0 {
        ctx.push_raw_str("<br>[[/tab]]");
        lifts -= 1;
        deferred = true;
    }
    deferred
}

fn split_bibliography_split_tail<'a, 't>(
    items: &'a [TabViewItem<'t>],
) -> (&'a [TabViewItem<'t>], &'a [Element<'t>]) {
    let Some((last, head)) = items.split_last() else {
        return (items, &[]);
    };
    let TabViewItem::Content(elements) = last else {
        return (items, &[]);
    };
    if elements.is_empty()
        || !elements.iter().all(|element| {
            let Element::Container(container) = element else {
                return false;
            };
            container.ctype() == ContainerType::Div
                && container
                    .attributes()
                    .get()
                    .get("class")
                    .is_some_and(|class| {
                        class.split_whitespace().any(|part| part == "bibitem")
                    })
        })
    {
        return (items, &[]);
    }
    (head, elements)
}

fn wikidot_tabview_content_has_literal_tabview_closer(elements: &[&Element]) -> bool {
    let mut buf = String::new();
    for element in elements {
        if let Element::Text(text) = element {
            buf.push_str(text.as_ref());
            if buf.contains("[[/tabview]]") || buf.contains("[[/tabs]]") {
                return true;
            }
            if buf.len() > 32 {
                buf.clear();
            }
        }
    }
    false
}

fn match_wikidot_literal_text_run<'t>(
    elements: &[&'t Element<'t>],
    start: usize,
    pattern: &str,
) -> Option<usize> {
    let mut i = start;
    let mut need = pattern;
    while !need.is_empty() {
        let Element::Text(text) = elements.get(i)? else {
            return None;
        };
        let frag = text.as_ref();
        if frag.is_empty() {
            i += 1;
            continue;
        }
        if need.starts_with(frag) {
            need = &need[frag.len()..];
            i += 1;
        } else {
            return None;
        }
    }
    Some(i)
}

fn compact_wikidot_literal_adjacent_tabview_recovery<'t>(
    elements: &[&'t Element<'t>],
    multi_tab_recovery: bool,
) -> Vec<&'t Element<'t>> {
    if !multi_tab_recovery {
        return elements.to_vec();
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < elements.len() {
        let compact_range = (|| {
            let close_end = match_wikidot_literal_text_run(elements, i, "[[/tabview]]")?;
            if close_end >= elements.len() {
                return None;
            }
            if !matches!(
                *elements[close_end],
                Element::LineBreak | Element::LineBreaks { .. }
            ) {
                return None;
            }
            let open_end =
                match_wikidot_literal_text_run(elements, close_end + 1, "[[tabview]]")?;
            Some((close_end, open_end))
        })();
        if let Some((close_end, open_end)) = compact_range {
            out.extend_from_slice(&elements[i..close_end]);
            out.extend_from_slice(&elements[(close_end + 1)..open_end]);
            i = open_end;
        } else {
            out.push(elements[i]);
            i += 1;
        }
    }
    out
}

fn render_wikidot_tabview_content_group(
    ctx: &mut HtmlContext,
    group: &[&Element],
    paragraph: bool,
) {
    if group.is_empty() {
        return;
    }
    if paragraph {
        ctx.html().tag("p").inner(|ctx| {
            for element in group {
                render_element(ctx, element);
            }
        });
    } else {
        for element in group {
            render_element(ctx, element);
        }
    }
}

fn render_wikidot_tabview_content_stream<'t>(
    ctx: &mut HtmlContext,
    elements: &[&'t Element<'t>],
    multi_tab_recovery: bool,
) {
    let flat =
        compact_wikidot_literal_adjacent_tabview_recovery(elements, multi_tab_recovery);
    for element in flat {
        render_element(ctx, element);
    }
}

fn render_wikidot_tabview_content_only(ctx: &mut HtmlContext, items: &[TabViewItem]) {
    let mut flat: Vec<&Element> = Vec::new();
    for item in items {
        if let TabViewItem::Content(elements) = item {
            flat.extend(elements.iter());
        }
    }
    if flat.iter().all(|element| element.paragraph_safe()) {
        ctx.html().tag("p").inner(|ctx| {
            for element in &flat {
                render_element(ctx, element);
            }
        });
        return;
    }
    let mut group: Vec<&Element> = Vec::new();
    let mut first_group = true;
    for element in &flat {
        if element.paragraph_safe() {
            group.push(*element);
            continue;
        }
        let mut stripped = false;
        while matches!(group.last(), Some(Element::LineBreak)) {
            group.pop();
            stripped = true;
        }
        let trailing_space = match group.last() {
            Some(Element::Text(text)) => text.ends_with([' ', '\t']),
            _ => false,
        };
        let paragraph = first_group && (stripped || !trailing_space);
        first_group = false;
        render_wikidot_tabview_content_group(ctx, &group, paragraph);
        group.clear();
        render_element(ctx, element);
    }
    while matches!(group.last(), Some(Element::LineBreak)) {
        group.pop();
    }
    render_wikidot_tabview_content_group(ctx, &group, false);
}

fn generate_ids(random: &mut Random, len: usize) -> Vec<String> {
    iter::repeat_n((), len)
        .map(|_| random.generate_html_id())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::SyntaxTree;
    use crate::tree::Tab;

    #[test]
    fn wikidot_tabview_uses_yui_dom_and_separates_labels() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tree = SyntaxTree {
            elements: vec![Element::TabView(vec![
                TabViewItem::Tab(Tab {
                    label: cow!("Apple"),
                    elements: vec![text!("one")],
                    wikidot_label_spans: Vec::new(),
                }),
                TabViewItem::Tab(Tab {
                    label: cow!("Banana"),
                    elements: vec![text!("two")],
                    wikidot_label_spans: Vec::new(),
                }),
            ])],
            ..SyntaxTree::default()
        };

        let output = HtmlRender.render(&tree, &page_info, &settings);
        let html = &output.body;
        assert!(html.contains("class=\"yui-navset\""), "{html}");
        assert!(!html.contains("yui-navset-top"), "{html}");
        assert!(!html.contains("tabview-min.js"), "{html}");
        assert!(!html.contains("title=\"active\""), "{html}");
        assert!(html.contains("<em>Apple</em></a></li>\n<li>"), "{html}");
        assert!(
            html.contains("<div id=\"wiki-tab-0-0\">one</div>"),
            "{html}",
        );
        assert!(html.contains("id=\"wiki-tab-0-1\""), "{html}");
        assert!(
            html.contains("<div id=\"wiki-tab-0-1\" style=\"display:none\">two</div>"),
            "{html}",
        );
        assert_eq!(html.matches("display:none").count(), 1, "{html}");
        assert!(!html.contains("<wj-tabs"), "{html}");
        assert!(!html.contains("new YAHOO.widget.TabView"), "{html}");
        assert!(!html.contains("<script"), "{html}");

        let [resource] = output.resource_requirements.as_slice() else {
            panic!(
                "expected one tabview requirement, got {:?}",
                output.resource_requirements
            );
        };
        let requirement = resource
            .wikidot_tab_view_requirement()
            .expect("tabview renderer emits a tabview requirement");
        assert!(
            html.contains(&format!("id=\"{}\"", requirement.id())),
            "{html}",
        );
    }

    #[test]
    fn multiple_wikidot_tabviews_emit_distinct_typed_requirements() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tree = SyntaxTree {
            elements: vec![
                Element::TabView(vec![TabViewItem::Tab(Tab {
                    label: cow!("A"),
                    elements: vec![text!("one")],
                    wikidot_label_spans: Vec::new(),
                })]),
                Element::TabView(vec![TabViewItem::Tab(Tab {
                    label: cow!("B"),
                    elements: vec![text!("two")],
                    wikidot_label_spans: Vec::new(),
                })]),
            ],
            ..SyntaxTree::default()
        };

        let output = HtmlRender.render(&tree, &page_info, &settings);
        assert_eq!(output.resource_requirements.len(), 2);
        let ids = output
            .resource_requirements
            .iter()
            .map(|resource| {
                resource
                    .wikidot_tab_view_requirement()
                    .expect("only tabview requirements are emitted")
                    .id()
            })
            .collect::<Vec<_>>();
        assert_ne!(ids[0], ids[1]);
        for id in ids {
            assert!(output.body.contains(&format!("id=\"{id}\"")));
        }
        assert!(!output.body.contains("<script"));
        assert!(!output.body.contains("tabview-min.js"));
        assert!(!output.body.contains("YAHOO.widget.TabView"));
    }

    #[test]
    fn authored_tab_data_cannot_inject_resource_authority() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let attack = "wiki-tabview-x');alert(1)//";
        let tree = SyntaxTree {
            elements: vec![Element::TabView(vec![TabViewItem::Tab(Tab {
                label: cow!("</em><script>alert(1)</script>"),
                elements: vec![text!(attack)],
                wikidot_label_spans: Vec::new(),
            })])],
            ..SyntaxTree::default()
        };

        let output = HtmlRender.render(&tree, &page_info, &settings);
        let [resource] = output.resource_requirements.as_slice() else {
            panic!("expected one resource requirement");
        };
        let id = resource
            .wikidot_tab_view_requirement()
            .expect("tabview requirement")
            .id();
        assert_ne!(id, attack);
        assert!(id.starts_with("wiki-tabview-"));
        assert!(output.body.contains("&lt;/em&gt;&lt;script&gt;"));
        assert!(!output.body.contains("<script"));
        assert!(!output.body.contains("alert(1)// = new"));
    }

    #[test]
    fn empty_wikidot_tab_body_keeps_static_dom_and_requirement() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tree = SyntaxTree {
            elements: vec![Element::TabView(vec![TabViewItem::Tab(Tab {
                label: cow!("Empty"),
                elements: Vec::new(),
                wikidot_label_spans: Vec::new(),
            })])],
            ..SyntaxTree::default()
        };

        let output = HtmlRender.render(&tree, &page_info, &settings);
        assert!(
            output.body.contains("<div id=\"wiki-tab-0-0\"></div>"),
            "{}",
            output.body,
        );
        assert_eq!(output.resource_requirements.len(), 1);
    }
}
