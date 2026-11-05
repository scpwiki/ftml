/*
 * parsing/rule/impls/block/blocks/module/mod.rs
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

mod mapping;
mod modules;
mod output;
mod parser;
mod rule;

use super::prelude;
use crate::parsing::rule::impls::block::Arguments;
use crate::parsing::rule::{LineRequirement, Rule};
use crate::parsing::{ParseResult, Parser};
use crate::tree::Elements;
use std::fmt::{self, Debug};

pub use self::output::ModuleParseOutput;
pub use self::rule::BLOCK_MODULE;

#[derive(Clone)]
pub struct ModuleRule {
    name: &'static str,

    accepts_names: &'static [&'static str],

    pub(crate) reads_body: bool,

    pub(crate) owns_opener_line: bool,

    parse_fn: ModuleParseFn,
}

impl ModuleRule {
    #[cold]
    pub fn rule(&self) -> Rule {
        fn try_consume_fn<'r, 't>(
            _: &mut Parser<'r, 't>,
        ) -> ParseResult<'r, 't, Elements<'t>> {
            panic!("Pseudo rule for this module should not be executed directly!");
        }

        Rule {
            name: self.name,
            position: LineRequirement::Any,
            try_consume_fn,
        }
    }
}

impl Debug for ModuleRule {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ModuleRule")
            .field("name", &self.name)
            .field("accepts_names", &self.accepts_names)
            .field("parse_fn", &(self.parse_fn as *const ()))
            .finish()
    }
}

pub type ModuleParseFn = for<'r, 't> fn(
    &mut Parser<'r, 't>,
    &'t str,
    Arguments<'t>,
    usize,
) -> ParseResult<'r, 't, ModuleParseOutput<'t>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn module_rule_metadata_helpers_cover_debug_and_rule() {
        let rule = modules::MODULE_RATE.rule();
        assert_eq!(rule.name(), "module-rate");

        let debug = format!("{:?}", modules::MODULE_RATE);
        assert!(debug.contains("ModuleRule"));
        assert!(debug.contains("module-rate"));
        assert!(debug.contains("accepts_names"));
        assert!(debug.contains("parse_fn"));
    }

    #[test]
    #[should_panic(
        expected = "Pseudo rule for this module should not be executed directly"
    )]
    fn module_rule_pseudo_rule_panics_when_executed() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[module Rate]]");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);

        let _ = modules::MODULE_RATE.rule().try_consume(&mut parser);
    }
}
