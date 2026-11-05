/*
 * parsing/rule/impls/block/blocks/module/modules/page_tree.rs
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

pub const MODULE_PAGE_TREE: ModuleRule = ModuleRule {
    name: "module-page-tree",
    accepts_names: &["PageTree"],
    parse_fn,
    reads_body: false,
    owns_opener_line: false,
};

fn parse_fn<'r, 't>(
    _parser: &mut Parser<'r, 't>,
    name: &'t str,
    arguments: Arguments<'t>,
    _opener_start: usize,
) -> ParseResult<'r, 't, ModuleParseOutput<'t>> {
    debug!("Parsing PageTree module");
    assert_module_name(&MODULE_PAGE_TREE, name);

    let arguments = arguments.into_raw_vec();
    let get_argument = |name| {
        arguments
            .iter()
            .rev()
            .find(|argument| argument.name == name)
            .map(|argument| argument.value.clone())
    };

    let root = get_argument("root");
    let depth = get_argument("depth").and_then(|depth| depth.parse().ok());
    let show_root = get_argument("showRoot").is_some_and(|value| value == "true");

    let module = Module::PageTree {
        root,
        show_root,
        depth,
    };
    success_value(module.into(), Vec::new(), false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    fn parse_page_tree(input: &str) -> (Option<String>, bool, Option<u32>) {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{input:?}: {errors:#?}");
        let Some(Element::Module(Module::PageTree {
            root,
            show_root,
            depth,
        })) = tree.elements.first()
        else {
            panic!("{input:?}: {tree:#?}");
        };

        (
            root.as_deref().map(str::to_owned),
            *show_root,
            depth.map(|depth| depth.get()),
        )
    }

    #[test]
    fn page_tree_uses_case_sensitive_argument_names() {
        for (input, expected) in [
            (
                "[[module PageTree root=\"start\" showRoot=\"true\" depth=\"2\"]]",
                (Some("start".to_owned()), true, Some(2)),
            ),
            (
                "[[module PageTree Root=\"start\" showRoot=\"true\" depth=\"2\"]]",
                (None, true, Some(2)),
            ),
            (
                "[[module PageTree root=\"start\" Showroot=\"true\" depth=\"2\"]]",
                (Some("start".to_owned()), false, Some(2)),
            ),
            (
                "[[module PageTree root=\"start\" showRoot=\"true\" Depth=\"2\"]]",
                (Some("start".to_owned()), true, None),
            ),
        ] {
            assert_eq!(parse_page_tree(input), expected, "{input:?}");
        }
    }

    #[test]
    fn page_tree_show_root_only_accepts_exact_lowercase_true() {
        for (value, expected) in [
            ("true", true),
            ("false", false),
            ("yes", false),
            ("no", false),
            ("TRUE", false),
            ("1", false),
        ] {
            let input = format!("[[module PageTree showRoot=\"{value}\"]]");
            assert_eq!(parse_page_tree(&input), (None, expected, None), "{input:?}",);
        }
    }

    #[test]
    fn page_tree_depth_only_accepts_positive_integers() {
        for (value, expected) in [
            (None, None),
            (Some("1"), Some(1)),
            (Some("2"), Some(2)),
            (Some("3"), Some(3)),
            (Some("0"), None),
            (Some("many"), None),
            (Some("-1"), None),
        ] {
            let input = value.map_or_else(
                || "[[module PageTree]]".to_owned(),
                |value| format!("[[module PageTree depth=\"{value}\"]]"),
            );
            assert_eq!(
                parse_page_tree(&input),
                (None, false, expected),
                "{input:?}",
            );
        }
    }
}
