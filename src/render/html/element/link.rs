/*
 * render/html/element/link.rs
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
    AnchorTarget, AttributeMap, Element, LinkLabel, LinkLocation, LinkType,
};
use crate::url::{HrefKind, classify_href, normalize_link};
use std::borrow::Cow;

pub fn render_anchor(
    ctx: &mut HtmlContext,
    elements: &[Element],
    attributes: &AttributeMap,
    target: Option<AnchorTarget>,
) {
    debug!("Rendering anchor");

    let layout = ctx.layout();
    if layout == Layout::Wikidot {
        render_wikidot_anchor(ctx, elements, attributes, target);
        return;
    }

    let target_value = match target {
        Some(target) => target.html_attr(),
        None => "",
    };
    let opens_new_tab = target == Some(AnchorTarget::NewTab);

    ctx.html()
        .a()
        .attr(attr!(
            "class" => "wj-anchor",
            "target" => target_value; if target.is_some(),
            "rel" => "noopener noreferrer"; if opens_new_tab;;
            attributes,
        ))
        .contents(elements);
}

fn render_wikidot_anchor(
    ctx: &mut HtmlContext,
    elements: &[Element],
    attributes: &AttributeMap,
    target: Option<AnchorTarget>,
) {
    let map = attributes.get();
    let mut tag = ctx.html().a();

    for key in ["id", "class", "style", "role", "type"] {
        if let Some(value) = map.get(key) {
            tag.attr_quoted(key, value);
        }
    }

    let mut ordered = Vec::new();
    for key in attributes.ordered_keys() {
        if key.starts_with("data-")
            && let Some(value) = map.get(key.as_ref())
        {
            tag.attr_quoted(key, value);
            ordered.push(key.as_ref());
        }
    }
    for (key, value) in map {
        if key.starts_with("data-") && !ordered.contains(&key.as_ref()) {
            tag.attr_quoted(key, value);
        }
    }

    let href = map.get("href").map(Cow::as_ref).unwrap_or("");
    tag.attr_quoted("href", href);

    let target = map
        .get("target")
        .map(Cow::as_ref)
        .or_else(|| target.map(|target| target.html_attr()));
    if let Some(value) = target {
        tag.attr_quoted("target", value);
    }

    tag.contents(elements);
}

pub fn render_anchor_target(ctx: &mut HtmlContext, target: &str) {
    debug!("Rendering anchor target");

    match ctx.layout() {
        Layout::Wikidot => {
            ctx.html().a().attr(attr!("name" => target));
        }
        Layout::Wikijump => {
            ctx.html().a().attr(attr!(
                "class" => "wj-anchor-target",
                "id" => target,
            ));
        }
    }
}

pub fn render_link(
    ctx: &mut HtmlContext,
    link: &LinkLocation,
    label: &LinkLabel,
    target: Option<AnchorTarget>,
    ltype: LinkType,
) {
    debug!("Rendering link '{:?}' (type {})", link, ltype.name());
    let layout = ctx.layout();
    let handle = ctx.handle();

    ctx.add_link(link);

    let site = ctx.info().site.as_ref().to_string();
    let url = normalize_link(link, ctx.handle());

    let target_value = match target {
        Some(target) => target.html_attr(),
        None => "",
    };
    let opens_new_tab =
        target == Some(AnchorTarget::NewTab) && layout == Layout::Wikijump;

    let titled;
    let label = match (label, link) {
        (LinkLabel::Title(_), LinkLocation::Page(page))
            if !page.page().is_empty() && ctx.page_exists(page) =>
        {
            match ctx
                .page_title(page)
                .filter(|title| !title.trim().is_empty())
            {
                Some(title) => {
                    titled = LinkLabel::Text(Cow::Owned(title));
                    &titled
                }
                None => label,
            }
        }
        _ => label,
    };

    macro_rules! write_a {
        ($attr:expr) => {{
            let mut tag = ctx.html().a();
            tag.attr($attr);
            handle.get_link_label(&site, link, label, |label| {
                tag.contents(label);
            });
        }};
    }

    match layout {
        Layout::Wikidot => match link {
            LinkLocation::Url(_) => {
                if ltype == LinkType::Interwiki {
                    // Live renders a resolved interwiki link in a new window
                    // through an inline handler on the anchor, not a target.
                    write_a!(attr!(
                        "href" => &url,
                        "onclick" => "window.open(this.href, '_blank'); return false;",
                        "target" => target_value; if target.is_some(),
                        "rel" => "noopener noreferrer"; if opens_new_tab,
                    ));
                } else {
                    write_a!(attr!(
                        "href" => &url,
                        "target" => target_value; if target.is_some(),
                        "rel" => "noopener noreferrer"; if opens_new_tab,
                    ));
                }
            }
            LinkLocation::Page(page) => {
                if !page.page().is_empty() && ctx.page_exists(page) {
                    write_a!(attr!(
                        "href" => &url,
                        "target" => target_value; if target.is_some(),
                        "rel" => "noopener noreferrer"; if opens_new_tab,
                    ));
                } else {
                    write_a!(attr!(
                        "class" => "newpage",
                        "href" => &url,
                        "target" => target_value; if target.is_some(),
                        "rel" => "noopener noreferrer"; if opens_new_tab,
                    ));
                }
            }
        },
        Layout::Wikijump => {
            let css_class = match link {
                LinkLocation::Url(url) => match classify_href(url) {
                    HrefKind::NoOp | HrefKind::Invalid | HrefKind::Anchor => {
                        "wj-link-anchor"
                    }
                    HrefKind::External => "wj-link-external",
                    HrefKind::AbsolutePath | HrefKind::Relative => "wj-link-internal",
                },
                LinkLocation::Page(page) => {
                    if ctx.page_exists(page) {
                        "wj-link-internal"
                    } else {
                        "wj-link-internal wj-link-missing"
                    }
                }
            };

            let interwiki_class = if ltype == LinkType::Interwiki {
                " wj-link-interwiki"
            } else {
                ""
            };

            write_a!(attr!(
                "class" => "wj-link " css_class interwiki_class,
                "data-link-type" => ltype.name(),
                "href" => &url,
                "target" => target_value; if target.is_some(),
                "rel" => "noopener noreferrer"; if opens_new_tab,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::data::{PageInfo, PageRef};
    use crate::layout::Layout;
    use crate::render::html::HtmlRender;
    use crate::render::{PageExistenceResolver, Render};
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::{
        AnchorTarget, Element, LinkLabel, LinkLocation, LinkType, SyntaxTree,
    };

    #[test]
    fn wikijump_interwiki_links_include_interwiki_class() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tree = SyntaxTree {
            elements: vec![Element::Link {
                ltype: LinkType::Interwiki,
                link: LinkLocation::Url(cow!("https://example.com/wiki")),
                label: LinkLabel::Text(cow!("Example Wiki")),
                target: None,
            }],
            ..SyntaxTree::default()
        };

        let output = HtmlRender.render(&tree, &page_info, &settings);

        assert!(output.body.contains("wj-link-interwiki"));
        assert!(output.body.contains(r#"data-link-type="interwiki""#));
        assert!(output.body.contains(">Example Wiki</a>"));
    }

    #[test]
    fn wikidot_url_links_use_normalized_href() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tree = SyntaxTree {
            elements: vec![Element::Link {
                ltype: LinkType::Direct,
                link: LinkLocation::Url(cow!("javascript:alert(1)")),
                label: LinkLabel::Text(cow!("click")),
                target: None,
            }],
            ..SyntaxTree::default()
        };

        let output = HtmlRender.render(&tree, &page_info, &settings);

        assert!(output.body.contains(r##"href="#invalid-url""##));
        assert!(!output.body.contains("javascript:alert"));
        assert!(output.backlinks.external_links.is_empty());
        assert!(output.backlinks.internal_links.is_empty());
    }

    #[test]
    fn new_tab_link_rel_is_layout_specific() {
        let page_info = PageInfo::dummy();

        for layout in [Layout::Wikidot, Layout::Wikijump] {
            let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
            let tree = SyntaxTree {
                elements: vec![Element::Link {
                    ltype: LinkType::Direct,
                    link: LinkLocation::Url(cow!("https://example.com/")),
                    label: LinkLabel::Text(cow!("example")),
                    target: Some(AnchorTarget::NewTab),
                }],
                ..SyntaxTree::default()
            };
            let output = HtmlRender.render(&tree, &page_info, &settings).body;

            assert!(
                output.contains(r#"target="_blank""#),
                "{layout:?}: {output}"
            );
            assert_eq!(
                output.contains(r#"rel="noopener noreferrer""#),
                layout == Layout::Wikijump,
                "{layout:?}: {output}",
            );
        }
    }

    #[test]
    fn non_new_tab_targets_do_not_gain_a_rel_attribute() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for target in [None, Some(AnchorTarget::Same), Some(AnchorTarget::Parent)] {
            let tree = SyntaxTree {
                elements: vec![Element::Link {
                    ltype: LinkType::Direct,
                    link: LinkLocation::Url(cow!("https://example.com/")),
                    label: LinkLabel::Text(cow!("example")),
                    target,
                }],
                ..SyntaxTree::default()
            };
            let output = HtmlRender.render(&tree, &page_info, &settings).body;

            assert!(!output.contains(" rel="), "{target:?}: {output}");
        }
    }

    #[derive(Debug)]
    struct FixturePageExistence;

    impl PageExistenceResolver for FixturePageExistence {
        fn page_exists(&self, _site: &str, page: &str) -> bool {
            page == "present"
        }
    }

    fn page_link(page: &str) -> Element<'static> {
        Element::Link {
            ltype: LinkType::Page,
            link: LinkLocation::Page(PageRef::page_only(page)),
            label: LinkLabel::Slug(std::borrow::Cow::Owned(page.to_owned())),
            target: None,
        }
    }

    #[test]
    fn wikidot_page_classes_follow_injected_page_existence() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tree = SyntaxTree {
            elements: vec![page_link("present"), page_link("missing-page")],
            ..SyntaxTree::default()
        };

        let output = HtmlRender.render_with_page_existence(
            &tree,
            &page_info,
            &settings,
            &FixturePageExistence,
        );

        assert_eq!(
            output.body,
            concat!(
                r#"<a href="/present">present</a>"#,
                r#"<a class="newpage" href="/missing-page">missing-page</a>"#,
            ),
        );
        assert!(!output.body.contains(r#"class="active""#));
    }

    #[test]
    fn wikijump_missing_page_class_still_uses_injected_page_existence() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tree = SyntaxTree {
            elements: vec![page_link("present"), page_link("missing-page")],
            ..SyntaxTree::default()
        };

        let output = HtmlRender.render_with_page_existence(
            &tree,
            &page_info,
            &settings,
            &FixturePageExistence,
        );

        assert!(
            output
                .body
                .contains(r#"class="wj-link wj-link-internal" data-link-type="page""#),
        );
        assert!(output.body.contains(
            r#"class="wj-link wj-link-internal wj-link-missing" data-link-type="page""#,
        ));
    }
}
