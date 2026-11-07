/*
 * parsing/rule/impls/block/blocks/toc.rs
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
use crate::tree::{ClearFloat, FloatAlignment};

pub const BLOCK_TABLE_OF_CONTENTS: BlockRule = BlockRule {
    name: "block-toc",
    accepts_names: &["toc", "f<toc", "f>toc"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn,
};

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing table-of-contents block (name '{name}', in-head {in_head})");
    parser.check_page_syntax()?;
    if parser.wikidot_footnote_zone_blocks(opener_start) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    assert!(!flag_star, "Table of Contents doesn't allow star flag");
    assert!(!flag_score, "Table of Contents doesn't allow score flag");
    assert_block_name(&BLOCK_TABLE_OF_CONTENTS, name);
    if parser.settings().layout.legacy() && !matches!(name, "toc" | "f<toc" | "f>toc") {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let arguments = parser.get_head_map_wikidot(&BLOCK_TABLE_OF_CONTENTS, in_head)?;
    let attributes = arguments.to_attribute_map(parser.settings());
    let align = FloatAlignment::parse(name).map(|float| float.align);
    let element = Element::TableOfContents { align, attributes };
    if parser.settings().layout.legacy()
        && let Some(clear_float) = legacy_toc_suffix_clear_float(parser)
    {
        return ok!(vec![element, clear_float]);
    }
    success_elements_with_paragraph_safety(false, element, Vec::new())
}

fn legacy_toc_suffix_clear_float<'t>(parser: &mut Parser<'_, 't>) -> Option<Element<'t>> {
    let token = parser.current();
    let clear_float = match token.token {
        Token::ClearFloatBoth => ClearFloat::Both,
        Token::ClearFloatLeft => ClearFloat::Left,
        Token::ClearFloatRight => ClearFloat::Right,
        _ => return None,
    };
    let source = parser.full_text().inner();
    let line_end = source[token.span.end..]
        .find(['\r', '\n'])
        .map_or(source.len(), |offset| token.span.end + offset);
    if token.span.end != line_end {
        return None;
    }
    parser.step().ok()?;
    Some(Element::ClearFloat(clear_float))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::Alignment;

    #[test]
    fn table_of_contents_block_carries_title_attribute() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for (source, expected_title) in [
            (r#"[[toc title="Contents"]]"#, Some("Contents")),
            (r#"[[toc title=""]]"#, Some("")),
            ("[[toc]]", None),
        ] {
            let tokenization = crate::tokenize(source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();

            assert!(errors.is_empty(), "{source}: {errors:?}");
            let [Element::TableOfContents { attributes, .. }] = tree.elements.as_slice()
            else {
                panic!("expected one table of contents for {source:?}");
            };

            assert_eq!(
                attributes.get().get("title").map(|value| value.as_ref()),
                expected_title,
                "{source}",
            );
        }
    }

    #[test]
    fn table_of_contents_block_parses_float_alignment_and_attributes() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(r#"[[f>toc id="contents"]]"#);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        let [Element::TableOfContents { align, attributes }] = tree.elements.as_slice()
        else {
            panic!("expected one table of contents, got {:?}", tree.elements);
        };

        assert_eq!(*align, Some(Alignment::Right));
        assert_eq!(
            attributes.get().get("id").map(|value| value.as_ref()),
            Some("u-contents")
        );
    }
}
