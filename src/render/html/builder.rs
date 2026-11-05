/*
 * render/html/builder.rs
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

use super::attributes::AddedAttributes;
use super::context::HtmlContext;
use super::render::ItemRender;
use std::collections::HashSet;

macro_rules! tag_method {
    ($tag:tt) => {
        pub fn $tag(self) -> HtmlBuilderTag<'c, 'i, 'h, 'e, 't> {
            self.tag(stringify!($tag))
        }
    };
}

#[derive(Debug)]
pub struct HtmlBuilder<'c, 'i, 'h, 'e, 't>
where
    'e: 't,
{
    ctx: &'c mut HtmlContext<'i, 'h, 'e, 't>,
}

impl<'c, 'i, 'h, 'e, 't> HtmlBuilder<'c, 'i, 'h, 'e, 't>
where
    'e: 't,
{
    #[inline]
    pub fn new(ctx: &'c mut HtmlContext<'i, 'h, 'e, 't>) -> Self {
        HtmlBuilder { ctx }
    }

    #[inline]
    pub fn tag(self, tag: &'t str) -> HtmlBuilderTag<'c, 'i, 'h, 'e, 't> {
        debug_assert!(is_alphanumeric(tag));

        let HtmlBuilder { ctx } = self;
        HtmlBuilderTag::new(ctx, tag)
    }

    #[inline]
    pub fn element(self, tag: &'t str) -> HtmlBuilderTag<'c, 'i, 'h, 'e, 't> {
        debug_assert!(tag.starts_with("wj-"));

        self.tag(tag)
    }

    #[inline]
    pub fn table_cell(self, header: bool) -> HtmlBuilderTag<'c, 'i, 'h, 'e, 't> {
        if header {
            self.tag("th")
        } else {
            self.tag("td")
        }
    }

    pub fn sprite(self, id: &'t str) {
        let viewbox = if id == "wj-karma" {
            "0 0 64 114"
        } else {
            "0 0 24 24"
        };

        let class = format!("wj-sprite sprite-{id}");
        let href = format!("/files--static/media/ui.svg#{id}");

        self.tag("svg")
            .attr(attr!(
                "class" => &class,
                "viewBox" => viewbox,
            ))
            .inner(|ctx| {
                ctx.html().tag("use").attr(attr!("href" => &href));
            });
    }

    tag_method!(a);
    tag_method!(article);
    tag_method!(br);
    tag_method!(code);
    tag_method!(dd);
    tag_method!(details);
    tag_method!(div);
    tag_method!(dl);
    tag_method!(dt);
    tag_method!(em);
    tag_method!(hr);
    tag_method!(iframe);
    tag_method!(img);
    tag_method!(input);
    tag_method!(li);
    tag_method!(nav);
    tag_method!(ol);
    tag_method!(pre);
    tag_method!(rp);
    tag_method!(script);
    tag_method!(style);
    tag_method!(span);
    tag_method!(sub);
    tag_method!(sup);
    tag_method!(summary);
    tag_method!(table);
    tag_method!(tbody);
    tag_method!(tr);
    tag_method!(ul);

    #[inline]
    pub fn text(&mut self, text: &str) {
        self.ctx.push_escaped(text);
    }
}

#[derive(Debug)]
pub struct HtmlBuilderTag<'c, 'i, 'h, 'e, 't>
where
    'e: 't,
{
    ctx: &'c mut HtmlContext<'i, 'h, 'e, 't>,
    tag: &'t str,
    in_tag: bool,
    in_contents: bool,
    omit_close: bool,
}

impl<'c, 'i, 'h, 'e, 't> HtmlBuilderTag<'c, 'i, 'h, 'e, 't> {
    pub fn new(ctx: &'c mut HtmlContext<'i, 'h, 'e, 't>, tag: &'t str) -> Self {
        ctx.push_raw('<');
        ctx.push_raw_str(tag);

        HtmlBuilderTag {
            ctx,
            tag,
            in_tag: true,
            in_contents: false,
            omit_close: false,
        }
    }

    pub fn omit_close(&mut self) -> &mut Self {
        self.omit_close = true;
        self.chain()
    }

    fn chain(&mut self) -> &mut Self {
        std::convert::identity(self)
    }

    fn attr_key(&mut self, key: &str, has_value: bool) {
        debug_assert!(is_alphanumeric(key));
        debug_assert!(self.in_tag);

        self.ctx.push_raw(' ');
        self.ctx.push_escaped(key);

        if has_value {
            self.ctx.push_raw('=');
        }
    }

    fn attr_value(&mut self, value_parts: &[&str]) {
        self.ctx.push_raw('"');

        for part in value_parts {
            self.ctx.push_escaped(part);
        }

        self.ctx.push_raw('"');
    }

    pub fn attr_quoted(&mut self, key: &str, value: &str) -> &mut Self {
        debug_assert!(is_alphanumeric(key));
        debug_assert!(self.in_tag);

        self.ctx.push_raw(' ');
        self.ctx.push_escaped(key);
        self.ctx.push_raw('=');
        self.ctx.push_raw('"');
        self.ctx.push_escaped(value);
        self.ctx.push_raw('"');

        self.chain()
    }

    pub fn attr_single(&mut self, key: &str, value_parts: &[&str]) -> &mut Self {
        let has_value = !value_parts.iter().all(|s| s.is_empty());

        self.attr_key(key, has_value);

        if has_value {
            self.attr_value(value_parts);
        }

        self.chain()
    }

    pub fn attr_single_quoted(&mut self, key: &str, value_parts: &[&str]) -> &mut Self {
        self.attr_key(key, true);
        self.attr_value(value_parts);
        self.chain()
    }

    pub fn attr(&mut self, attributes: AddedAttributes) -> &mut Self {
        let Some(attribute_map) = attributes.map else {
            for (key, value_parts) in filter_attribute_entries(&attributes) {
                self.attr_single(key, value_parts);
            }

            return self.chain();
        };
        let attribute_map = attribute_map.get();

        if attributes.entries.is_empty() {
            for (key, value) in attribute_map {
                if omit_empty_user_class(key, value) {
                    continue;
                }
                self.attr_single(key, &[value]);
            }

            return self.chain();
        }

        let mut merged = HashSet::new();
        let mut merged_value = Vec::new();

        for (key, value_parts) in filter_attribute_entries(&attributes) {
            if let Some(map_value) = attribute_map.get(&cow!(key)) {
                if omit_empty_user_class(key, map_value) {
                    self.attr_single(key, value_parts);
                    merged.insert(key);
                    continue;
                }

                merged_value.clear();
                merged_value.extend(value_parts);
                merged_value.push(" ");
                merged_value.push(map_value);

                self.attr_single(key, &merged_value);
                merged.insert(key);
            }
        }

        for (key, value_parts) in filter_attribute_entries(&attributes) {
            if !merged.contains(key) {
                self.attr_single(key, value_parts);
            }
        }

        for (key, value) in attribute_map {
            if !merged.contains(key.as_ref()) && !omit_empty_user_class(key, value) {
                self.attr_single(key, &[value]);
            }
        }

        self.chain()
    }

    fn content_start(&mut self) {
        if self.in_tag {
            self.ctx.push_raw('>');
            self.in_tag = false;
        }

        assert!(!self.in_contents, "Already in tag contents");
        self.in_contents = true;
    }

    #[inline]
    pub fn contents<R: ItemRender>(&mut self, item: R) -> &mut Self {
        self.content_start();
        item.render(self.ctx);
        self.chain()
    }

    pub fn inner<F>(&mut self, mut f: F) -> &mut Self
    where
        F: FnMut(&mut HtmlContext),
    {
        self.content_start();
        f(self.ctx);
        self.chain()
    }
}

impl Drop for HtmlBuilderTag<'_, '_, '_, '_, '_> {
    fn drop(&mut self) {
        if self.in_tag && !self.in_contents {
            self.ctx.push_raw('>');
        }

        if should_close_tag(self.tag) && !self.omit_close {
            self.ctx.push_raw_str("</");
            self.ctx.push_raw_str(self.tag);
            self.ctx.push_raw('>');
        }
    }
}

fn is_alphanumeric(value: &str) -> bool {
    value
        .chars()
        .all(|c| c.is_ascii_alphabetic() || c.is_ascii_digit() || c == '-')
}

fn filter_attribute_entries<'a>(
    attributes: &AddedAttributes<'a>,
) -> impl Iterator<Item = (&'a str, &'a [&'a str])> {
    let entries = attributes.entries.iter();
    entries.filter_map(|(item, accept)| if *accept { Some(*item) } else { None })
}

fn omit_empty_user_class(key: &str, value: &str) -> bool {
    key == "class" && value.is_empty()
}

#[inline]
fn should_close_tag(tag: &str) -> bool {
    !is_solo_html_tag(tag)
}

fn is_solo_html_tag(tag: &str) -> bool {
    matches!(
        tag,
        "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input"
    ) || matches!(tag, "link" | "meta" | "param" | "source" | "track" | "wbr")
}

#[cfg(test)]
mod tests {
    use super::super::output::HtmlOutput;
    use super::super::test_utils::context;
    use super::*;
    use crate::data::PageInfo;
    use crate::tree::AttributeMap;

    #[test]
    fn html_builder_renders_core_tag_shapes_and_escaped_text() {
        let info = PageInfo::dummy();
        let mut ctx = context(&info);

        ctx.html().element("wj-box").contents("A&B");
        ctx.html().table_cell(true).contents("Head");
        ctx.html().table_cell(false).contents("Cell");
        ctx.html().br();
        ctx.html().input().attr(attr!(
            "checked",
            "disabled" => "",
            "ignored"; if false,
        ));

        let mut builder = ctx.html();
        builder.text("x < y");

        let output = HtmlOutput::from(ctx);
        assert_eq!(
            output.body,
            concat!(
                "<wj-box>A&amp;B</wj-box>",
                "<th>Head</th>",
                "<td>Cell</td>",
                "<br>",
                "<input checked disabled>",
                "x &lt; y",
            ),
        );
    }

    #[test]
    fn html_builder_merges_renderer_and_user_attributes() {
        let info = PageInfo::dummy();
        let mut ctx = context(&info);
        let mut attributes = AttributeMap::new();
        assert!(attributes.insert("class", cow!("from-map")));
        assert!(attributes.insert("data-extra", cow!("1&2")));

        ctx.html()
            .div()
            .attr(attr!(
                "class" => "base",
                "data-skip" => "no"; if false;;
                attributes
            ))
            .contents("body");

        let output = HtmlOutput::from(ctx);
        assert_eq!(
            output.body,
            r#"<div class="base from-map" data-extra="1&amp;2">body</div>"#,
        );
    }

    #[test]
    fn html_builder_appends_user_attributes_without_renderer_attrs() {
        let info = PageInfo::dummy();
        let mut ctx = context(&info);
        let mut attributes = AttributeMap::new();
        assert!(attributes.insert("data-extra", cow!("1&2")));

        ctx.html().div().attr(attr!(;; attributes)).contents("body");

        let output = HtmlOutput::from(ctx);
        assert_eq!(output.body, r#"<div data-extra="1&amp;2">body</div>"#);
    }

    #[test]
    fn html_builder_omits_an_empty_user_class_attribute() {
        let info = PageInfo::dummy();
        let mut ctx = context(&info);
        let mut attributes = AttributeMap::new();
        assert!(attributes.insert("class", cow!("")));

        ctx.html().div().attr(attr!(;; attributes)).contents("body");
        ctx.html()
            .div()
            .attr(attr!("class" => "base";; attributes))
            .contents("merged");

        let output = HtmlOutput::from(ctx);
        assert_eq!(
            output.body,
            r#"<div>body</div><div class="base">merged</div>"#,
        );
    }

    #[test]
    fn html_builder_keeps_solo_tags_unclosed() {
        let info = PageInfo::dummy();
        let mut ctx = context(&info);

        for tag in [
            "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
            "param", "source", "track", "wbr",
        ] {
            ctx.html().tag(tag);
        }

        let output = HtmlOutput::from(ctx);
        assert_eq!(
            output.body,
            concat!(
                "<area><base><br><col><embed><hr><img><input><link><meta><param>",
                "<source><track><wbr>",
            ),
        );
    }

    #[test]
    fn html_builder_renders_sprite_viewboxes() {
        let info = PageInfo::dummy();
        let mut ctx = context(&info);

        ctx.html().sprite("wj-karma");
        ctx.html().sprite("wj-clipboard");

        let output = HtmlOutput::from(ctx);
        assert_eq!(
            output.body,
            concat!(
                r#"<svg class="wj-sprite sprite-wj-karma" "#,
                r#"viewBox="0 0 64 114">"#,
                r##"<use href="/files--static/media/ui.svg#wj-karma"></use></svg>"##,
                r#"<svg class="wj-sprite sprite-wj-clipboard" "#,
                r#"viewBox="0 0 24 24">"#,
                r##"<use href="/files--static/media/ui.svg#wj-clipboard"></use></svg>"##,
            ),
        );
    }
}
