/*
 * render/html/element/table.rs
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
use crate::tree::{Table, TableType};
use std::num::NonZeroU32;

pub fn render_table(ctx: &mut HtmlContext, table: &Table) {
    debug!("Rendering table");

    let mut column_span_buf = String::new();
    let value_one = NonZeroU32::new(1).unwrap();
    let layout = ctx.layout();

    let table_class = match (layout, table.table_type) {
        (Layout::Wikidot, TableType::Simple) => "wiki-content-table",
        (Layout::Wikidot, TableType::Advanced) => "",
        (Layout::Wikijump, TableType::Simple) => "wj-table wj-table-simple",
        (Layout::Wikijump, TableType::Advanced) => "wj-table wj-table-advanced",
    };

    let table_attributes = if table_class.is_empty() {
        attr!(;; &table.attributes)
    } else {
        attr!(
            "class" => table_class;;
            &table.attributes,
        )
    };

    ctx.html().table().attr(table_attributes).inner(|ctx| {
        if layout.legacy() {
            ctx.push_raw('\n');
            render_rows(ctx, table, layout, &mut column_span_buf, value_one);
        } else {
            ctx.html().tbody().inner(|ctx| {
                render_rows(ctx, table, layout, &mut column_span_buf, value_one);
            });
        }
    });
}

fn render_rows(
    ctx: &mut HtmlContext,
    table: &Table,
    layout: Layout,
    column_span_buf: &mut String,
    value_one: NonZeroU32,
) {
    for row in &table.rows {
        ctx.html()
            .tr()
            .attr(attr!(;; &row.attributes))
            .inner(|ctx| {
                if layout.legacy() {
                    ctx.push_raw('\n');
                }
                for cell in &row.cells {
                    let elements: &[Element] = &cell.elements;
                    let has_authored_column_span =
                        cell.attributes.get().contains_key("colspan");
                    if cell.column_span > value_one && !has_authored_column_span {
                        column_span_buf.clear();
                        str_write!(column_span_buf, "{}", cell.column_span);
                    }
                    let attributes = match (cell.align, layout) {
                        (Some(align), Layout::Wikidot) => attr!(
                            "colspan" => &column_span_buf; if cell.column_span > value_one && !has_authored_column_span,
                            "style" => align.wd_html_style();;
                            &cell.attributes,
                        ),
                        (Some(align), Layout::Wikijump) => attr!(
                            "colspan" => &column_span_buf; if cell.column_span > value_one && !has_authored_column_span,
                            "class" => align.wj_html_class();;
                            &cell.attributes,
                        ),
                        (None, _) => attr!(
                            "colspan" => &column_span_buf; if cell.column_span > value_one && !has_authored_column_span;;
                            &cell.attributes,
                        ),
                    };
                    let mut table_cell = ctx.html().table_cell(cell.header);
                    table_cell.attr(attributes);
                    table_cell.contents(elements);
                    drop(table_cell);
                    if layout.legacy() {
                        ctx.push_raw('\n');
                    }
                }
            });
        if layout.legacy() {
            ctx.push_raw('\n');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::{AttributeMap, SyntaxTree, TableCell, TableRow};

    #[test]
    fn table_render_outputs_cell_contents() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let table = Table {
            table_type: TableType::Simple,
            attributes: AttributeMap::new(),
            rows: vec![TableRow {
                attributes: AttributeMap::new(),
                cells: vec![TableCell {
                    header: false,
                    column_span: NonZeroU32::new(1).unwrap(),
                    align: None,
                    attributes: AttributeMap::new(),
                    elements: vec![Element::Text(cow!("cell <value>"))],
                }],
            }],
        };
        let tree = SyntaxTree {
            elements: vec![Element::Table(table)],
            ..SyntaxTree::default()
        };

        let output = HtmlRender.render(&tree, &page_info, &settings);

        assert!(
            output
                .body
                .contains(r#"<table class="wj-table wj-table-simple">"#)
        );
        assert!(output.body.contains("<td>cell &lt;value&gt;</td>"));
    }
}
