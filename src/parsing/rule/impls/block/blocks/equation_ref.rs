/*
 * parsing/rule/impls/block/blocks/equation_ref.rs
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

pub const BLOCK_EQUATION_REF: BlockRule = BlockRule {
    name: "block-equation-ref",
    accepts_names: &["equation", "eref", "eqref"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: false,
    parse_fn,
};

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing equation reference block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Equation reference doesn't allow start flag");
    assert!(!flag_score, "Equation reference doesn't allow score flag");
    assert_block_name(&BLOCK_EQUATION_REF, name);
    if parser.settings().layout.legacy() && name != "eref" {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let block = &BLOCK_EQUATION_REF;
    let name = parser.get_head_value(block, in_head, require_trimmed_block_argument)?;

    success_elements(Element::EquationReference(std::borrow::Cow::Borrowed(name)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn equation_reference_parses_name_and_requires_argument() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);

        let tokenization = crate::tokenize("[[eqref theorem-one]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:?}");
        assert!(matches!(
            tree.elements.as_slice(),
            [Element::Container(paragraph)]
                if paragraph.elements().iter().any(|element| matches!(
                    element,
                    Element::EquationReference(name) if name.as_ref() == "theorem-one"
                ))
        ));

        let tokenization = crate::tokenize("[[eref]]");
        let (_tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(
            errors
                .iter()
                .any(|error| error.kind() == ParseErrorKind::BlockMissingArguments)
        );
    }
}
