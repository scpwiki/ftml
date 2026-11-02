/*
 * parsing/rule/impls/block/blocks/module/output.rs
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

use crate::tree::{Element, Elements, Module};

#[derive(Debug)]
pub enum ModuleParseOutput<'t> {
    Module(Module<'t>),
    Element(Element<'t>),
    None,
}

impl<'t> From<Module<'t>> for ModuleParseOutput<'t> {
    #[inline]
    fn from(module: Module<'t>) -> ModuleParseOutput<'t> {
        ModuleParseOutput::Module(module)
    }
}

impl<'t> From<Element<'t>> for ModuleParseOutput<'t> {
    #[inline]
    fn from(element: Element<'t>) -> ModuleParseOutput<'t> {
        ModuleParseOutput::Element(element)
    }
}

impl<'t> From<Option<()>> for ModuleParseOutput<'t> {
    #[inline]
    fn from(_: Option<()>) -> ModuleParseOutput<'t> {
        ModuleParseOutput::None
    }
}

impl<'t> From<ModuleParseOutput<'t>> for Elements<'t> {
    fn from(output: ModuleParseOutput<'t>) -> Elements<'t> {
        match output {
            ModuleParseOutput::Module(module) => {
                Elements::Single(Element::Module(module))
            }
            ModuleParseOutput::Element(element) => Elements::Single(element),
            ModuleParseOutput::None => Elements::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_parse_output_converts_into_elements() {
        let module_output = ModuleParseOutput::Module(Module::Rate);
        let module_elements = Elements::from(module_output);
        assert_eq!(
            module_elements,
            Elements::Single(Element::Module(Module::Rate))
        );

        let element_output = ModuleParseOutput::Element(Element::LineBreak);
        let element_elements = Elements::from(element_output);
        assert_eq!(element_elements, Elements::Single(Element::LineBreak));

        let none_output = ModuleParseOutput::None;
        let none_elements = Elements::from(none_output);
        assert_eq!(none_elements, Elements::None);
    }

    #[test]
    fn module_parse_output_from_conversions_preserve_variants() {
        assert!(matches!(
            ModuleParseOutput::from(Module::Rate),
            ModuleParseOutput::Module(Module::Rate),
        ));

        assert!(matches!(
            ModuleParseOutput::from(Element::LineBreak),
            ModuleParseOutput::Element(Element::LineBreak),
        ));

        assert!(matches!(
            ModuleParseOutput::from(None::<()>),
            ModuleParseOutput::None,
        ));
    }
}
