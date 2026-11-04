/*
 * parsing/rule/impls/block/blocks/hidden.rs
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

pub const BLOCK_HIDDEN: BlockRule = BlockRule {
    name: "block-hidden",
    accepts_names: &["hidden"],
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
    debug!("Parsing hidden block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Hidden doesn't allow star flag");
    assert!(!flag_score, "Hidden doesn't allow score flag");
    assert_block_name(&BLOCK_HIDDEN, name);

    let arguments = parser.get_head_map(&BLOCK_HIDDEN, in_head)?;

    let attributes = arguments.to_attribute_map(parser.settings());
    parser.get_body_elements_inline_container(
        &BLOCK_HIDDEN,
        ContainerType::Hidden,
        attributes,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn hidden_block_wraps_multiline_body() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tokenization = crate::tokenize("[[hidden]]\nhidden text\n[[/hidden]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        let [Element::Container(paragraph)] = tree.elements.as_slice() else {
            panic!("expected one paragraph, got {:?}", tree.elements);
        };
        assert_eq!(paragraph.ctype(), ContainerType::Paragraph);
        let [Element::Container(container)] = paragraph.elements() else {
            panic!(
                "expected one hidden container, got {:?}",
                paragraph.elements()
            );
        };

        assert_eq!(container.ctype(), ContainerType::Hidden);
        assert!(container.attributes().get().is_empty());
        let body_text = container
            .elements()
            .iter()
            .map(|element| match element {
                Element::Text(text) => text.as_ref(),
                other => panic!("expected only text body elements, got {other:?}"),
            })
            .collect::<String>();
        assert_eq!(body_text, "hidden text");
    }
}
