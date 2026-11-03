/*
 * parsing/rule/impls/block/blocks/iframe.rs
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
use crate::parsing::discard_wikidot_controls;
use crate::tree::{AttributeMap, LinkLabel, LinkLocation, LinkType};
use std::borrow::Cow;

pub const BLOCK_IFRAME: BlockRule = BlockRule {
    name: "block-iframe",
    accepts_names: &["iframe"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn,
};

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing iframe block (in-head {in_head})");
    assert!(!flag_star, "iframe doesn't allow star flag");
    assert!(!flag_score, "iframe doesn't allow score flag");
    assert_block_name(&BLOCK_IFRAME, name);

    let (url, arguments) = parser.get_head_name_map_wikidot(&BLOCK_IFRAME, in_head)?;
    let url = if parser.settings().layout.legacy() {
        discard_wikidot_controls(Cow::Borrowed(url))
    } else {
        Cow::Borrowed(url)
    };
    if !(if parser.settings().layout.legacy() {
        wikidot_iframe_url(&url)
    } else {
        crate::url::is_url(&url)
    }) {
        if parser.settings().layout.legacy()
            && (url.starts_with("git://") || url.starts_with("sftp://"))
        {
            return ok!(Elements::Multiple(vec![
                text!("[[iframe "),
                Element::Link {
                    ltype: LinkType::Direct,
                    link: LinkLocation::Url(url.clone()),
                    label: LinkLabel::Url,
                    target: None,
                },
                text!("]]"),
            ]));
        }
        warn!("Iframe block references non-URL ({} bytes)", url.len());
        return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
    }

    let attributes = if parser.settings().layout.legacy() && arguments.has_spaced_equals()
    {
        AttributeMap::default()
    } else {
        arguments.to_attribute_map(parser.settings())
    };
    let element = Element::Iframe { url, attributes };

    ok!(parser.settings().layout.legacy(); element)
}

fn wikidot_iframe_url(url: &str) -> bool {
    ["http://", "https://", "ftp://", "gopher://", "mailto:"]
        .iter()
        .any(|prefix| {
            url.get(..prefix.len())
                .is_some_and(|value| value.eq_ignore_ascii_case(prefix))
        })
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn wikidot_iframe_discards_attributes_when_equals_is_spaced() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(
            r#"[[iframe https://example.com/ id = "my-example" class="iframe" width = "90%"]]"#,
        );
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            concat!(
                "<p><iframe src=\"https://example.com/\" align frameborder ",
                "height scrolling width class style></iframe></p>",
            ),
        );
    }

    #[test]
    fn wikidot_iframe_keeps_compact_attributes() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization =
            crate::tokenize(r#"[[iframe https://example.com/ frameborder="0"]]"#);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = crate::render::html::HtmlRender
            .render(&tree, &page_info, &settings)
            .body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("frameborder=\"0\""), "{html}");
    }
}
