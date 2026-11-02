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
use crate::tree::Tab;
use std::iter;

pub fn render_tabview(ctx: &mut HtmlContext, tabs: &[Tab]) {
    debug!("Rendering tabview (tabs {})", tabs.len());

    if ctx.layout().legacy() {
        render_wikidot_tabview(ctx, tabs);
        return;
    }

    let button_ids = generate_ids(ctx.random(), tabs.len());
    let tab_ids = generate_ids(ctx.random(), tabs.len());

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
                    for (i, tab) in tabs.iter().enumerate() {
                        let (tab_selected, tab_index) = if i == 0 {
                            ("true", "0")
                        } else {
                            ("false", "-1")
                        };

                        ctx.html()
                            .element("wj-tabs-button")
                            .attr(attr!(
                                "class" => "wj-tabs-button",
                                "id" => &button_ids[i],
                                "role" => "tab",
                                "aria-label" => &tab.label,
                                "aria-selected" => tab_selected,
                                "aria-controls" => &tab_ids[i],
                                "tabindex" => tab_index,
                            ))
                            .contents(&tab.label);
                    }
                });

            ctx.html()
                .div()
                .attr(attr!(
                    "class" => "wj-tabs-panel-list",
                ))
                .inner(|ctx| {
                    for (i, tab) in tabs.iter().enumerate() {
                        ctx.html()
                            .div()
                            .attr(attr!(
                                "class" => "wj-tabs-panel",
                                "id" => &tab_ids[i],
                                "role" => "tabpanel",
                                "aria-labelledby" => &button_ids[i],
                                "tabindex" => "0",
                                "hidden"; if i > 0,
                            ))
                            .contents(&tab.elements);
                    }
                });
        });
}

fn render_wikidot_tabview(ctx: &mut HtmlContext, tabs: &[Tab]) {
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
                    for (index, tab) in tabs.iter().enumerate() {
                        ctx.html()
                            .li()
                            .attr(attr!(
                                "class" => "selected"; if index == 0,
                            ))
                            .inner(|ctx| {
                                ctx.html()
                                    .a()
                                    .attr(attr!("href" => "javascript:;"))
                                    .inner(|ctx| {
                                        ctx.html().em().contents(&tab.label);
                                    });
                            });
                        ctx.push_raw('\n');
                    }
                });

            ctx.html()
                .div()
                .attr(attr!("class" => "yui-content"))
                .inner(|ctx| {
                    for (index, tab) in tabs.iter().enumerate() {
                        let tab_id = format!("wiki-tab-0-{index}");
                        ctx.html()
                            .div()
                            .attr(attr!("id" => &tab_id))
                            .contents(&tab.elements);
                    }
                });
        });
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

    #[test]
    fn wikidot_tabview_uses_yui_dom_and_separates_labels() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tree = SyntaxTree {
            elements: vec![Element::TabView(vec![
                Tab {
                    label: cow!("Apple"),
                    elements: vec![text!("one")],
                },
                Tab {
                    label: cow!("Banana"),
                    elements: vec![text!("two")],
                },
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
        assert!(!html.contains("display:"), "{html}");
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
                Element::TabView(vec![Tab {
                    label: cow!("A"),
                    elements: vec![text!("one")],
                }]),
                Element::TabView(vec![Tab {
                    label: cow!("B"),
                    elements: vec![text!("two")],
                }]),
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
            elements: vec![Element::TabView(vec![Tab {
                label: cow!("</em><script>alert(1)</script>"),
                elements: vec![text!(attack)],
            }])],
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
            elements: vec![Element::TabView(vec![Tab {
                label: cow!("Empty"),
                elements: Vec::new(),
            }])],
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
