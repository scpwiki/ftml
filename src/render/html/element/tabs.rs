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
use crate::tree::{AttributeMap, ContainerType, TabViewItem};
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
    let (items, trailing) = split_bibliography_split_tail(items);
    let tail = tabview_wrapper_tail(items);
    let items = if tail.is_some() {
        items.split_last().map(|(_, head)| head).unwrap_or(items)
    } else {
        items
    };
    let id = ctx.random().generate_wikidot_tabview_id();
    ctx.require_wikidot_tab_view(id.clone());

    ctx.html()
        .div()
        .attr(attr!("id" => &id, "class" => "yui-navset"))
        .inner(|ctx| {
            ctx.html()
                .ul()
                .attr(attr!("class" => "yui-nav"))
                .inner(|ctx| {
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
            ctx.html()
                .div()
                .attr(attr!("class" => "yui-content"))
                .inner(|ctx| {
                    if tab_count == 0 {
                        render_wikidot_tabview_content_only(ctx, items);
                        return;
                    }
                    let mut tab_index = 0usize;
                    for item in items {
                        match item {
                            TabViewItem::Tab(tab) => {
                                let tab_id = format!("wiki-tab-0-{tab_index}");
                                // Live Wikidot hides every panel after the first inline.
                                ctx.html()
                                    .div()
                                    .attr(attr!(
                                        "id" => &tab_id,
                                        "style" => "display:none"; if tab_index > 0,
                                    ))
                                    .contents(&tab.elements);
                                tab_index += 1;
                            }
                            // The panel is rendered as a bare div in a later
                            // quote segment; only the nav entry lives here.
                            TabViewItem::Remote(_) => {
                                tab_index += 1;
                            }
                            TabViewItem::Content(elements) => {
                                let flat: Vec<&Element> = elements.iter().collect();
                                if wikidot_tabview_content_has_literal_tabview_closer(
                                    &flat,
                                ) {
                                    render_wikidot_tabview_content_stream(
                                        ctx,
                                        &flat,
                                        tab_count > 1,
                                    );
                                } else {
                                    render_elements(ctx, elements);
                                }
                            }
                        }
                    }
                });
            if let Some(tail) = tail {
                render_elements(ctx, tail);
            }
            for element in trailing {
                render_element(ctx, element);
            }
        });
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
