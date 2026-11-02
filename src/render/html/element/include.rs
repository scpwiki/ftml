/*
 * render/html/element/include.rs
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
use crate::data::PageRef;
use crate::tree::VariableMap;

pub fn render_include(
    ctx: &mut HtmlContext,
    location: &PageRef,
    variables: &VariableMap,
    elements: &[Element],
) {
    debug!("Rendering include (location {location:?})");
    ctx.variables_mut().push_scope(variables);
    render_elements(ctx, elements);
    ctx.variables_mut().pop_scope();
}

pub fn render_variable(ctx: &mut HtmlContext, name: &str) {
    let value = ctx.variables().get(name);
    debug!(
        "Rendering variable (name '{}', value '{}'",
        name,
        value.unwrap_or("<none>"),
    );

    let value = match value {
        Some(value) => str!(value),

        None => format!("{{${name}}}"),
    };

    ctx.push_escaped(&value);
}

#[test]
fn html_include_variables_are_scoped_and_escaped() {
    use crate::data::{PageInfo, PageRef};
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::SyntaxTree;

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let mut variables = VariableMap::new();
    variables.insert(cow!("name"), cow!("Alice & Bob"));

    let tree = SyntaxTree {
        elements: vec![
            Element::Include {
                paragraph_safe: true,
                variables,
                location: PageRef::page_only(cow!("component:html")),
                elements: vec![
                    Element::Text(cow!("inside ")),
                    Element::Variable(cow!("name")),
                    Element::Text(cow!(" / ")),
                    Element::Variable(cow!("missing")),
                ],
            },
            Element::Text(cow!(" after ")),
            Element::Variable(cow!("name")),
        ],
        ..SyntaxTree::default()
    };

    let output = HtmlRender.render(&tree, &page_info, &settings);

    assert_eq!(
        output.body,
        "inside Alice &amp; Bob / {$missing} after {$name}",
    );
}
