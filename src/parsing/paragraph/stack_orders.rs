/*
 * parsing/paragraph/stack_orders.rs
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

use super::ParagraphStack;
use crate::delayed::DelayedElement;
use crate::tree::Alignment;
use crate::tree::FloatAlignment;
use crate::tree::{
    AttributeMap, Element, FileSource, ImageSource, Table, TableCell, TableRow, TableType,
};
use std::cell::RefCell;
use std::num::NonZeroU32;
use std::panic::{AssertUnwindSafe, catch_unwind};

const OPS: &[&str] = &[
    "push-text",
    "push-line-break",
    "push-block(hr)",
    "push-footnote",
    "push-simple-table",
    "push-aligned-image",
    "push-plain-image",
    "defer-seam",
    "end-paragraph",
    "end-paragraph-at-break",
    "pop-line-break",
    "merge-adjacent-tables",
    "mark-current-unwrapped",
    "mark-next-unwrapped",
    "mark-terminal-backslash",
    "ensure-trailing-line-break",
    "mark-discarded-control",
];

fn image(alignment: Option<FloatAlignment>) -> Element<'static> {
    Element::Image {
        source: ImageSource::Direct(FileSource::Url("https://example.test/a.png".into())),
        link: None,
        alignment,
        attributes: AttributeMap::new(),
        target: None,
    }
}

fn simple_table() -> Element<'static> {
    Element::Table(Table {
        table_type: TableType::Simple,
        attributes: AttributeMap::new(),
        rows: vec![TableRow {
            attributes: AttributeMap::new(),
            cells: vec![TableCell {
                header: false,
                column_span: NonZeroU32::new(1).unwrap(),
                align: None,
                attributes: AttributeMap::new(),
                elements: vec![text!("c")],
            }],
        }],
    })
}

fn apply(stack: &mut ParagraphStack<'static>, op: usize) {
    match op {
        0 => stack.push_element(text!("t"), true),
        1 => stack.push_element(Element::LineBreak, true),
        2 => stack.push_element(Element::HorizontalRule, false),
        3 => stack.push_element(Element::Footnote(0), true),
        4 => stack.push_element(simple_table(), false),
        5 => stack.push_element(
            image(Some(FloatAlignment {
                align: Alignment::Left,
                float: false,
            })),
            true,
        ),
        6 => stack.push_element(image(None), true),
        7 => stack.defer_suppression_seam(Element::Delayed(
            DelayedElement::typography_boundary(),
        )),
        8 => stack.end_paragraph(),
        9 => stack.end_paragraph_at_break(),
        10 => stack.pop_line_break(),
        11 => stack.merge_wikidot_adjacent_simple_tables(),
        12 => stack.mark_current_unwrapped(),
        13 => stack.mark_next_unwrapped(),
        14 => stack.mark_wikidot_terminal_backslash(),
        15 => stack.ensure_wikidot_trailing_line_break(),
        16 => stack.mark_discarded_control(),
        _ => unreachable!("operation table and apply() disagree"),
    }
}

thread_local! {
    static LAST_PANIC: RefCell<String> = const { RefCell::new(String::new()) };
}

fn replay(wikidot: bool, ops: &[usize]) -> Option<String> {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut stack = if wikidot {
            ParagraphStack::new_wikidot()
        } else {
            ParagraphStack::new()
        };
        for &op in ops {
            apply(&mut stack, op);
        }
        let _ = stack.into_elements();
    }));
    result
        .err()
        .map(|_| LAST_PANIC.with(|m| m.borrow().clone()))
}

fn explore(
    wikidot: bool,
    prefix: &mut Vec<usize>,
    max_len: usize,
    found: &mut Vec<String>,
) {
    if prefix.len() == max_len {
        return;
    }
    for op in 0..OPS.len() {
        prefix.push(op);
        if let Some(message) = replay(wikidot, prefix) {
            let names: Vec<&str> = prefix.iter().map(|&i| OPS[i]).collect();
            found.push(format!(
                "MINIMAL-FAILURE wikidot={wikidot} ops=[{}] panic={message}",
                names.join(", ")
            ));
        } else {
            explore(wikidot, prefix, max_len, found);
        }
        prefix.pop();
    }
}

#[test]
#[ignore = "run through verification/paragraph-stack/run.sh"]
fn no_operation_order_panics_the_paragraph_stack() {
    let max_len: usize = std::env::var("FTML_STACK_OPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);

    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        LAST_PANIC.with(|m| *m.borrow_mut() = format!("{payload} at {location}"));
    }));

    let mut found = Vec::new();
    for wikidot in [false, true] {
        explore(wikidot, &mut Vec::new(), max_len, &mut found);
    }
    let _ = std::panic::take_hook();

    for line in &found {
        println!("{line}");
    }
    println!("ORDERS-BOUND max_ops={max_len} operations={}", OPS.len());
    assert!(
        found.is_empty(),
        "{} shortest failing operation orders (listed above)",
        found.len()
    );
}
