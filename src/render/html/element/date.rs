/*
 * render/html/element/date.rs
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
use crate::tree::DateItem;

pub fn render_date(
    ctx: &mut HtmlContext,
    date: DateItem,
    date_format: Option<&str>,
    hover: bool,
) {
    let (display_format, agohover) = split_agohover_format(date_format);
    let formatted_datetime = date.format_or_default(display_format, ctx.language());

    match ctx.layout() {
        Layout::Wikidot => {
            render_date_wikidot(ctx, date, display_format, agohover, &formatted_datetime)
        }
        Layout::Wikijump => {
            render_date_wikijump(ctx, date, display_format, hover, &formatted_datetime)
        }
    }
}

fn render_date_wikidot(
    ctx: &mut HtmlContext,
    date: DateItem,
    date_format: Option<&str>,
    agohover: bool,
    formatted_datetime: &str,
) {
    let timestamp = date.timestamp();
    let mut class = format!("odate time_{timestamp}");
    push_date_format_class(&mut class, date_format, agohover);
    let style = if agohover {
        "cursor: help; display: inline;"
    } else {
        "display: inline;"
    };

    ctx.html()
        .span()
        .attr(attr!(
            "class" => &class,
            "style" => style,
        ))
        .contents(formatted_datetime);
}

fn split_agohover_format(date_format: Option<&str>) -> (Option<&str>, bool) {
    match date_format {
        Some(date_format) => match date_format.strip_suffix("|agohover") {
            Some(display_format) => (Some(display_format), true),
            None => (Some(date_format), false),
        },
        None => (None, false),
    }
}

fn render_date_wikijump(
    ctx: &mut HtmlContext,
    date: DateItem,
    date_format: Option<&str>,
    hover: bool,
    formatted_datetime: &str,
) {
    let timestamp = str!(date.timestamp());
    let delta = str!(date.time_since());
    let mut class = str!("wj-date");

    if hover {
        class.push_str(" wj-date-hover");
    }

    push_date_format_class(&mut class, date_format, false);

    ctx.html()
        .span()
        .attr(attr!(
            "class" => &class,
            "data-timestamp" => &timestamp,
            "data-delta" => &delta,
        ))
        .contents(formatted_datetime);
}

fn push_date_format_class(
    class: &mut String,
    date_format: Option<&str>,
    append_agohover: bool,
) {
    if let Some(date_format) = date_format {
        class.push_str(" format_");
        class.push_str(&encode_date_format(date_format));

        if append_agohover {
            class.push_str("%7Cagohover");
        }
    } else if append_agohover {
        class.push_str(" format_");
        class.push_str("%7Cagohover");
    }
}

fn encode_date_format(date_format: &str) -> String {
    let mut encoded = String::new();
    for byte in date_format.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => {
                encoded.push('%');
                str_write!(&mut encoded, "{byte:02X}");
            }
        }
    }

    encoded
}

#[test]
fn date_format_encoding() {
    assert_eq!(encode_date_format("%d. %m. %Y"), "%25d.%20%25m.%20%25Y");
}

#[test]
fn split_agohover_format_removes_suffix_from_display_format() {
    assert_eq!(
        split_agohover_format(Some("%d. %m. %Y|agohover")),
        (Some("%d. %m. %Y"), true)
    );
}

#[test]
fn split_agohover_format_leaves_normal_format_unchanged() {
    assert_eq!(
        split_agohover_format(Some("%d. %m. %Y")),
        (Some("%d. %m. %Y"), false)
    );
}

#[test]
fn wikidot_date_class_includes_format() {
    let mut class = str!("odate time_1216153821");
    push_date_format_class(&mut class, Some("%d. %m. %Y"), false);

    assert_eq!(class, "odate time_1216153821 format_%25d.%20%25m.%20%25Y");
}

#[test]
fn wikidot_date_class_includes_agohover() {
    let mut class = str!("odate time_1216153821");
    push_date_format_class(&mut class, Some("%d. %m. %Y"), true);

    assert_eq!(
        class,
        "odate time_1216153821 format_%25d.%20%25m.%20%25Y%7Cagohover"
    );
}

#[test]
fn wikidot_date_class_allows_agohover_without_format() {
    let mut class = str!("odate time_1216153821");
    push_date_format_class(&mut class, None, true);

    assert_eq!(class, "odate time_1216153821 format_%7Cagohover");
}

#[test]
fn wikijump_date_class_includes_format() {
    let mut class = str!("wj-date");
    push_date_format_class(&mut class, Some("%d. %m. %Y"), false);

    assert_eq!(class, "wj-date format_%25d.%20%25m.%20%25Y");
}
