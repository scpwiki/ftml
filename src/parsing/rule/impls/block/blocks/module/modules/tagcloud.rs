/*
 * parsing/rule/impls/block/blocks/module/modules/tagcloud.rs
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

pub const MODULE_TAGCLOUD: ModuleRule = ModuleRule {
    name: "module-tagcloud",
    accepts_names: &["TagCloud"],
    parse_fn,
};

fn parse_fn<'r, 't>(
    _parser: &mut Parser<'r, 't>,
    name: &'t str,
    arguments: Arguments<'t>,
) -> ParseResult<'r, 't, ModuleParseOutput<'t>> {
    debug!("Parsing tagcloud module");
    assert_module_name(&MODULE_TAGCLOUD, name);

    ok!(false; Module::TagCloud {
        arguments: arguments.into_raw_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::Module;

    fn parse_tagcloud(input: &str) -> Vec<(String, String)> {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{input:?}: {errors:#?}");
        let [Element::Module(Module::TagCloud { arguments })] = tree.elements.as_slice()
        else {
            panic!("{input:?}: {tree:#?}");
        };

        arguments
            .iter()
            .map(|argument| {
                (
                    argument.name.as_ref().to_owned(),
                    argument.value.as_ref().to_owned(),
                )
            })
            .collect()
    }

    #[test]
    fn tagcloud_preserves_documented_attributes_verbatim() {
        assert_eq!(
            parse_tagcloud("[[module TagCloud]]"),
            Vec::<(String, String)>::new(),
        );
        assert_eq!(
            parse_tagcloud("[[module TagCloud limit=\"2\" category=\"news\"]]"),
            [
                ("limit".to_owned(), "2".to_owned()),
                ("category".to_owned(), "news".to_owned()),
            ],
        );
        assert_eq!(
            parse_tagcloud(
                "[[module TagCloud minFontSize=\"1em\" maxFontSize=\"2em\" maxColor=\"8,8,8\" minColor=\"200,200,228\" category=\"news\"]]"
            ),
            [
                ("minFontSize".to_owned(), "1em".to_owned()),
                ("maxFontSize".to_owned(), "2em".to_owned()),
                ("maxColor".to_owned(), "8,8,8".to_owned()),
                ("minColor".to_owned(), "200,200,228".to_owned()),
                ("category".to_owned(), "news".to_owned()),
            ],
        );
    }

    #[test]
    fn tagcloud_preserves_link_and_display_combinations() {
        assert_eq!(
            parse_tagcloud(
                "[[module TagCloud target=\"my-target\" urlAttrPrefix=\"lp\" skipCategoryFromUrl=\"true\" category=\"news\"]]"
            ),
            [
                ("target".to_owned(), "my-target".to_owned()),
                ("urlAttrPrefix".to_owned(), "lp".to_owned()),
                ("skipCategoryFromUrl".to_owned(), "true".to_owned()),
                ("category".to_owned(), "news".to_owned()),
            ],
        );
        assert_eq!(
            parse_tagcloud("[[module TagCloud mode=\"3d\" width=\"123\" height=\"77\"]]"),
            [
                ("mode".to_owned(), "3d".to_owned()),
                ("width".to_owned(), "123".to_owned()),
                ("height".to_owned(), "77".to_owned()),
            ],
        );
        assert_eq!(
            parse_tagcloud(
                "[[module TagCloud limit=\"0\" showHidden=\"\" minFontSize=\"1em\"]]"
            ),
            [
                ("limit".to_owned(), "0".to_owned()),
                ("showHidden".to_owned(), String::new()),
                ("minFontSize".to_owned(), "1em".to_owned()),
            ],
        );
    }

    #[test]
    fn tagcloud_preserves_duplicates_unknowns_and_empty_values() {
        assert_eq!(
            parse_tagcloud(
                "[[module TagCloud limit=\"2\" limit=\"5\" unknown=\"kept\" showHidden=\"\"]]"
            ),
            [
                ("limit".to_owned(), "2".to_owned()),
                ("limit".to_owned(), "5".to_owned()),
                ("unknown".to_owned(), "kept".to_owned()),
                ("showHidden".to_owned(), String::new()),
            ],
        );
    }

    #[test]
    fn tagcloud_module_name_is_case_insensitive() {
        for input in [
            "[[module tagcloud limit=\"2\"]]",
            "[[module TAGCLOUD limit=\"2\"]]",
            "[[module TagCloud limit=\"2\"]]",
        ] {
            assert_eq!(
                parse_tagcloud(input),
                [("limit".to_owned(), "2".to_owned())],
                "{input:?}",
            );
        }
    }
}
