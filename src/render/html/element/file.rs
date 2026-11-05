/*
 * render/html/element/file.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2026 Wikijump Team
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

use super::prelude::*;
use crate::tree::FileSource;
use std::borrow::Cow;

pub fn render_file_link(ctx: &mut HtmlContext, file: &str, label: &str) {
    let url = if ctx.layout().legacy() {
        get_wikidot_file_link_url(ctx, file)
    } else {
        ctx.handle().get_file_link(
            &FileSource::File1 {
                file: Cow::Borrowed(file),
            },
            ctx.info(),
            ctx.settings(),
        )
    };
    let Some(url) = url else {
        ctx.push_escaped(label);
        return;
    };
    let url = if ctx.layout().legacy() {
        encode_wikidot_file_href(&url)
    } else {
        url
    };

    let layout = ctx.layout();
    let mut anchor = ctx.html().a();
    match layout {
        Layout::Wikidot => anchor.attr(attr!("href" => &url)),
        Layout::Wikijump => anchor.attr(attr!(
            "class" => "wj-link wj-link-internal",
            "data-link-type" => "file",
            "href" => &url,
        )),
    };
    anchor.contents(label);
}

fn get_wikidot_file_link_url<'a>(
    ctx: &HtmlContext,
    file: &'a str,
) -> Option<Cow<'a, str>> {
    let (path, is_path) = match file.strip_prefix('/') {
        Some(path) => (path, true),
        None => (file, file.contains('/')),
    };

    if is_path {
        ctx.handle()
            .get_local_file_path_link(path, ctx.info(), ctx.settings())
    } else {
        ctx.handle().get_file_link(
            &FileSource::File1 {
                file: Cow::Borrowed(path),
            },
            ctx.info(),
            ctx.settings(),
        )
    }
}

fn encode_wikidot_file_href(href: &str) -> Cow<'_, str> {
    if href.is_ascii() && !href.contains(' ') {
        return Cow::Borrowed(href);
    }

    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(href.len());
    for byte in href.bytes() {
        if byte == b' ' || !byte.is_ascii() {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        } else {
            encoded.push(char::from(byte));
        }
    }
    Cow::Owned(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wikidot_file_href_encodes_spaces_and_non_ascii_without_normalizing_path() {
        assert_eq!(
            encode_wikidot_file_href(
                "https://example.test/local--files/path with spaces/日本語.txt?x=1#y",
            ),
            "https://example.test/local--files/path%20with%20spaces/%E6%97%A5%E6%9C%AC%E8%AA%9E.txt?x=1#y",
        );
        assert_eq!(
            encode_wikidot_file_href("https://example.test/local--files/../elements.tsv",),
            "https://example.test/local--files/../elements.tsv",
        );
    }
}
