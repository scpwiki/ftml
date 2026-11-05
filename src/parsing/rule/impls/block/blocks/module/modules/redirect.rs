/*
 * parsing/rule/impls/block/blocks/module/modules/redirect.rs
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

pub const MODULE_REDIRECT: ModuleRule = ModuleRule {
    name: "module-redirect",
    accepts_names: &["Redirect"],
    parse_fn,
    reads_body: false,
    owns_opener_line: false,
};

fn parse_fn<'r, 't>(
    _parser: &mut Parser<'r, 't>,
    name: &'t str,
    mut arguments: Arguments<'t>,
    _opener_start: usize,
) -> ParseResult<'r, 't, ModuleParseOutput<'t>> {
    debug!("Parsing redirect module");
    assert_module_name(&MODULE_REDIRECT, name);

    let destination = arguments.get("destination");
    ok!(false; Module::Redirect { destination })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::Module;

    fn parse_redirect(input: &str) -> Option<String> {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{input:?}: {errors:#?}");
        let [Element::Module(Module::Redirect { destination })] =
            tree.elements.as_slice()
        else {
            panic!("{input:?}: {tree:#?}");
        };

        destination.as_deref().map(str::to_owned)
    }

    #[test]
    fn redirect_preserves_page_and_url_destinations_verbatim() {
        for (input, expected) in [
            (
                "[[module Redirect destination=\"start\"]]",
                Some("start".to_owned()),
            ),
            (
                "[[module Redirect destination=\"http://slashdot.org\"]]",
                Some("http://slashdot.org".to_owned()),
            ),
            (
                "[[module Redirect destination=\"http://www.example.com/base/\"]]",
                Some("http://www.example.com/base/".to_owned()),
            ),
            ("[[module Redirect]]", None),
            ("[[module Redirect destination=\"\"]]", Some(String::new())),
        ] {
            assert_eq!(parse_redirect(input), expected, "{input:?}");
        }
    }

    #[test]
    fn redirect_argument_names_are_case_insensitive() {
        assert_eq!(
            parse_redirect("[[module Redirect Destination=\"start\"]]"),
            Some("start".to_owned())
        );
        assert_eq!(
            parse_redirect("[[module Redirect DESTINATION=\"start\"]]"),
            Some("start".to_owned())
        );
    }

    #[test]
    fn redirect_ignores_unknown_arguments_like_backlinks() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(
            "[[module Redirect destination=\"start\" invalid=\"argument\"]]",
        );
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(matches!(
            tree.elements.as_slice(),
            [Element::Module(Module::Redirect {
                destination: Some(destination),
            })] if destination == "start",
        ));
    }

    #[test]
    fn redirect_module_name_is_case_insensitive() {
        for input in [
            "[[module redirect destination=\"start\"]]",
            "[[module REDIRECT destination=\"start\"]]",
            "[[module ReDiReCt destination=\"start\"]]",
        ] {
            assert_eq!(parse_redirect(input), Some("start".to_owned()), "{input:?}",);
        }
    }
}
