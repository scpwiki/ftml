/*
 * parsing/rule/impls/block/blocks/include_elements.rs
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
use crate::parsing::UnstructuredParseResult;

pub const BLOCK_INCLUDE_ELEMENTS: BlockRule = BlockRule {
    name: "block-include-elements",
    accepts_names: &["include-elements"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn,
};

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Found invalid include-elements block");
    parser.check_page_syntax()?;
    assert!(!flag_star, "Include (elements) doesn't allow star flag");
    assert!(!flag_score, "Include (elements) doesn't allow score flag");
    assert_block_name(&BLOCK_INCLUDE_ELEMENTS, name);

    let head = parser.get_head_name_map(&BLOCK_INCLUDE_ELEMENTS, in_head)?;
    let (page_name, variables) = head;

    let page_ref = match PageRef::parse_include(&page_name) {
        Ok(page_ref) => page_ref,
        Err(_) => return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments)),
    };

    let included = include_page(parser, &page_ref)?;
    let result = included.result;
    let mut html_blocks = included.html_blocks;
    let mut code_blocks = included.code_blocks;
    let mut table_of_contents_depths = included.table_of_contents_depths;
    let mut footnotes = included.footnotes;
    let has_footnote_block = included.has_footnote_block;
    let mut bibliographies = included.bibliographies;

    let result = result?;
    set_included_footnote_block(parser, has_footnote_block);
    let mut elements = result.item;
    let errors = result.errors;
    let paragraph_safe = result.paragraph_safe;

    offset_footnote_references(&mut elements, &mut footnotes, parser.footnote_count());

    let html = &mut html_blocks;
    let code = &mut code_blocks;
    let toc = &mut table_of_contents_depths;
    let notes = &mut footnotes;
    let bibs = &mut bibliographies;
    parser.append_shared_items(html, code, toc, notes, bibs);

    let variables = variables.to_hash_map();
    let element = Element::Include {
        paragraph_safe,
        variables,
        location: page_ref,
        elements,
    };

    ok!(element, errors)
}

fn offset_footnote_references(
    elements: &mut [Element<'_>],
    footnotes: &mut [Vec<Element<'_>>],
    offset: usize,
) {
    for element in elements {
        element.offset_footnote_indices(offset);
    }
    for footnote in footnotes {
        for element in footnote {
            element.offset_footnote_indices(offset);
        }
    }
}

fn set_included_footnote_block(parser: &mut Parser<'_, '_>, has_footnote_block: bool) {
    if has_footnote_block {
        parser.set_footnote_block();
    }
}

fn include_page<'r, 't>(
    parser: &Parser<'r, 't>,
    page: &PageRef,
) -> Result<UnstructuredParseResult<'r, 't>, ParseError> {
    if page.page().is_empty() {
        return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
    }

    let elements = vec![text!("<INCLUDED PAGE (ELEMENTS)>")];
    let result = Ok(ParseSuccess::new(elements, Vec::new(), false));
    let has_footnote_block = false;
    Ok(UnstructuredParseResult {
        result,
        html_blocks: Vec::new(),
        code_blocks: Vec::new(),
        table_of_contents_depths: Vec::new(),
        footnotes: Vec::new(),
        has_footnote_block: std::convert::identity(has_footnote_block),
        bibliographies: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    use super::*;

    #[test]
    fn include_elements_propagates_included_footnote_block_state() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[include-elements page]]");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);

        set_included_footnote_block(&mut parser, true);
        assert!(parser.has_footnote_block());
    }

    #[test]
    fn include_page_rejects_empty_page_references() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[include-elements page]]");
        let parser = Parser::new(&tokenization, &page_info, &settings);
        let empty = PageRef {
            site: None,
            page: String::new(),
            extra: None,
        };

        let error = include_page(&parser, &empty).expect_err("empty page should reject");
        assert_eq!(error.kind(), ParseErrorKind::BlockMalformedArguments);
    }

    #[test]
    fn included_footnote_references_are_offset_after_outer_footnotes() {
        let mut elements = vec![
            Element::Footnote(1),
            Element::Include {
                paragraph_safe: true,
                variables: Default::default(),
                location: PageRef::new(None::<String>, "nested"),
                elements: vec![Element::Footnote(2)],
            },
        ];
        let mut footnotes = vec![vec![Element::Footnote(1)]];

        offset_footnote_references(&mut elements, &mut footnotes, 3);

        assert!(matches!(elements[0], Element::Footnote(4)));
        let Element::Include { elements, .. } = &elements[1] else {
            panic!("expected nested include: {elements:#?}");
        };
        assert!(matches!(elements[0], Element::Footnote(5)));
        assert!(matches!(footnotes[0][0], Element::Footnote(4)));
    }
}
