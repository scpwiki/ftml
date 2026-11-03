/*
 * parsing/rule/impls/block/blocks/bibcite.rs
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

pub const BLOCK_BIBCITE: BlockRule = BlockRule {
    name: "block-bibcite",
    accepts_names: &["bibcite"],
    accepts_star: false,
    accepts_score: true,
    accepts_newlines_outside_body: false,
    parse_fn,
};

fn require_label<'r, 't>(
    parser: &Parser<'r, 't>,
    value: Option<&'t str>,
) -> Result<&'t str, ParseError> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => Ok(value),
        None => {
            warn!("No label provided in [[bibcite]], failing rule");
            Err(parser.make_err(ParseErrorKind::BlockMissingArguments))
        }
    }
}

fn bibliography_cite(label: &str, brackets: bool) -> Element<'_> {
    Element::BibliographyCite {
        label: cow!(label),
        brackets,
        inline: false,
    }
}

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing bibcite block (name {name}, in-head {in_head}, score {flag_score})");
    assert!(!flag_star, "Bibcite doesn't allow star flag");
    assert_block_name(&BLOCK_BIBCITE, name);

    let label = parser.get_head_value(&BLOCK_BIBCITE, in_head, require_label)?;

    let brackets = !flag_score;

    ok!(bibliography_cite(label, brackets))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn block_bibcite_requires_label() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        for input in ["[[bibcite]]", "[[bibcite   ]]"] {
            let tokenization = crate::tokenize(input);
            let (_tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();

            assert!(
                errors
                    .iter()
                    .any(|error| error.kind() == ParseErrorKind::BlockMissingArguments),
                "{input} should require a non-empty label: {errors:?}",
            );
        }
    }

    #[test]
    fn block_bibcite_parses_bracketed_and_bare_variants() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tokenization = crate::tokenize("[[bibcite alpha]]\n[[bibcite_ beta]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty());
        match tree.elements.as_slice() {
            [Element::Container(container)] => match container.elements() {
                [
                    Element::BibliographyCite {
                        label: first_label,
                        brackets: first_brackets,
                        ..
                    },
                    Element::LineBreak,
                    Element::BibliographyCite {
                        label: second_label,
                        brackets: second_brackets,
                        ..
                    },
                ] => {
                    assert_eq!(first_label, "alpha");
                    assert!(*first_brackets);
                    assert_eq!(second_label, "beta");
                    assert!(!second_brackets);
                }
                other => panic!("expected two bibliography cites, got {other:?}"),
            },
            other => panic!("expected citation paragraph, got {other:?}"),
        }
    }
}
