/*
 * parsing/rule/impls/block/blocks/paragraph.rs
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
use crate::tree::ContainerType;

pub const BLOCK_PARAGRAPH: BlockRule = BlockRule {
    name: "block-paragraph",
    accepts_names: &["p", "paragraph"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines: true,
    parse_fn,
};

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing paragraph block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Paragraph doesn't allow star flag");
    assert!(!flag_score, "Paragraph doesn't allow score flag");
    assert_block_name(&BLOCK_PARAGRAPH, name);

    let arguments = parser.get_head_map(&BLOCK_PARAGRAPH, in_head)?;
    let attributes = arguments.to_attribute_map(parser.settings());
    let body = parser.get_body_elements(&BLOCK_PARAGRAPH, true)?;
    let (mut elements, errors, _) = body.into();

    for element in &mut elements {
        if let Element::Container(container) = element
            && container.ctype() == ContainerType::Paragraph
        {
            container.attributes_mut().clone_from(&attributes);
        }
    }

    let element = Elements::Multiple(elements);
    ok!(element, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn paragraph_block_applies_attributes_to_body_paragraph() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tokenization = crate::tokenize(
            r#"[[p class="lead"]]
paragraph text
[[/p]]"#,
        );
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        let [Element::Container(paragraph)] = tree.elements.as_slice() else {
            panic!("expected one paragraph, got {:?}", tree.elements);
        };

        assert_eq!(paragraph.ctype(), ContainerType::Paragraph);
        assert_eq!(
            paragraph
                .attributes()
                .get()
                .get("class")
                .map(|value| value.as_ref()),
            Some("lead")
        );
        let body_text = paragraph
            .elements()
            .iter()
            .map(|element| match element {
                Element::Text(text) => text.as_ref(),
                other => panic!("expected only paragraph text, got {other:?}"),
            })
            .collect::<String>();
        assert_eq!(body_text, "paragraph text");
    }
}
