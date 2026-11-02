/*
 * test/ast/mod.rs
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

mod loader;
mod runner;

use crate::parsing::ParseError;
use crate::tree::{Element, ListItem, PartialElement, RubyText, SyntaxTree};
use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use std::sync::LazyLock;

const SKIP_TESTS: &[&str] = &[];

const ONLY_TESTS: &[&str] = &[];

const UPDATE_TESTS: bool = false;

static TEST_DIRECTORY: LazyLock<PathBuf> = LazyLock::new(|| {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("test");
    path
});

#[derive(Debug, Copy, Clone)]
pub enum TestResult {
    Pass,
    Fail,
    Skip,
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct TestStats {
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
}

impl TestStats {
    #[inline]
    pub fn new() -> TestStats {
        TestStats::default()
    }

    pub fn add(&mut self, result: TestResult) {
        match result {
            TestResult::Pass => self.passed += 1,
            TestResult::Fail => self.failed += 1,
            TestResult::Skip => self.skipped += 1,
        }
    }

    pub fn print(self) {
        let total = self.passed + self.failed + self.skipped;

        if self.failed + self.skipped == 0 {
            println!("Ran a total of {total} tests, all of which passed.");
        } else {
            let percent = |value| (value as f32) / (total as f32) * 100.0;
            println!("Ran a total of {total} tests. Of these:");
            println!("* {} passed ({:.1}%)", self.passed, percent(self.passed));

            if self.failed != 0 {
                println!("* {} failed ({:.1}%)", self.failed, percent(self.failed));
            }

            if self.skipped != 0 {
                println!("* {} skipped ({:.1}%)", self.skipped, percent(self.skipped));
            }
        }
    }

    pub fn exit_code(self) -> i32 {
        (self.failed + self.skipped).try_into().ok().unwrap_or(-1)
    }

    pub fn assert_success(self) {
        assert_eq!(
            self.exit_code(),
            0,
            "AST fixture run had {} failed and {} skipped tests",
            self.failed,
            self.skipped,
        );
    }
}

#[derive(Debug)]
pub struct Test {
    pub name: String,

    pub input: String,

    pub tree: Option<SyntaxTree<'static>>,

    pub errors: Option<Vec<ParseError>>,

    pub wikidot_output: Option<String>,

    pub html_output: Option<String>,

    pub text_output: Option<String>,

    pub locale: Option<String>,
}

#[derive(Debug)]
pub struct TestUniverse {
    pub tests: BTreeMap<String, Test>,
}

static TEST_UNIVERSE: LazyLock<TestUniverse> =
    LazyLock::new(|| TestUniverse::load(&TEST_DIRECTORY));

fn test_universe() -> &'static TestUniverse {
    &TEST_UNIVERSE
}

fn env_update_tests() -> bool {
    match env::var("FTML_UPDATE_TESTS").ok() {
        Some(value) => matches!(value.as_str(), "true" | "1"),
        _ => false,
    }
}

#[test]
fn ast() {
    if UPDATE_TESTS || env_update_tests() {
        let tests = TestUniverse::load_permissive(&TEST_DIRECTORY);

        println!("=========");
        println!(" WARNING ");
        println!("=========");
        println!();
        println!("You are running in UPDATE MODE!");
        println!();
        println!(
            "This will run tests and save whatever results as the new \"expected\" value."
        );
        println!("Carefully inspect the diff and only save changes that are correct.");
        println!();

        tests.update(&TEST_DIRECTORY, SKIP_TESTS, ONLY_TESTS);

        println!();
        println!("Failing test, you must unset update mode to let CI pass");
        println!("This is either:");
        println!("* The constant UPDATE_TESTS");
        println!("* The environment variable FTML_UPDATE_TESTS");
        panic!("FTML_UPDATE_TESTS/UPDATE_TESTS update mode must not pass CI");
    }

    let tests = test_universe();

    #[allow(clippy::const_is_empty)]
    if !SKIP_TESTS.is_empty() {
        println!("=========");
        println!(" WARNING ");
        println!("=========");
        println!();
        println!("Tests matching the following are being SKIPPED:");

        for test in SKIP_TESTS {
            println!("- {}", test);
        }

        println!();
    }

    #[allow(clippy::const_is_empty)]
    if !ONLY_TESTS.is_empty() {
        println!("=========");
        println!(" WARNING ");
        println!("=========");
        println!();
        println!("Only tests matching the following will being run.");
        println!("All others are being SKIPPED!");

        for test in ONLY_TESTS {
            println!("> {}", test);
        }

        println!();
    }

    let stats = tests.run(SKIP_TESTS, ONLY_TESTS);
    stats.print();
    stats.assert_success();
}

#[test]
fn ast_elements_exercise_surface_helpers() {
    let tests = test_universe();
    let mut element_count = 0;

    for test in tests.tests.values() {
        if let Some(tree) = &test.tree {
            let _owned_tree = tree.to_owned();
            element_count += exercise_syntax_tree_helpers(tree);
        }
    }

    assert!(
        element_count > 0,
        "AST fixtures did not contain any elements"
    );
}

#[test]
fn partial_element_surface_helpers() {
    let partial = Element::Partial(PartialElement::RubyText(RubyText::default()));

    assert_eq!(partial.name(), "RubyText");
    let _owned_partial = partial.to_owned();
}

#[test]
fn structural_partial_element_is_paragraph_unsafe() {
    let partial = Element::Partial(PartialElement::RubyText(RubyText::default()));

    assert!(!partial.paragraph_safe());
}

fn exercise_syntax_tree_helpers(tree: &SyntaxTree<'_>) -> usize {
    let mut count = 0;

    count += exercise_elements_helpers(&tree.elements);
    count += exercise_elements_helpers(&tree.table_of_contents);

    for footnote in &tree.footnotes {
        count += exercise_elements_helpers(footnote);
    }

    for index in 0..tree.bibliographies.next_index() {
        for (_, elements) in tree.bibliographies.get_bibliography(index).slice() {
            count += exercise_elements_helpers(elements);
        }
    }

    count
}

fn exercise_elements_helpers(elements: &[Element<'_>]) -> usize {
    elements.iter().map(exercise_element_helpers).sum()
}

fn exercise_element_helpers(element: &Element<'_>) -> usize {
    let _name = element.name();
    let _owned = element.to_owned();

    if !matches!(element, Element::Partial(_)) {
        let _paragraph_safe = element.paragraph_safe();
    }

    let nested_count = match element {
        Element::Container(container) => exercise_elements_helpers(container.elements()),
        Element::Table(table) => table
            .rows
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| exercise_elements_helpers(&cell.elements))
                    .sum::<usize>()
            })
            .sum(),
        Element::TabView(tabs) => tabs
            .iter()
            .map(|tab| exercise_elements_helpers(&tab.elements))
            .sum(),
        Element::Anchor { elements, .. } => exercise_elements_helpers(elements),
        Element::List { items, .. } => items.iter().map(exercise_list_item_helpers).sum(),
        Element::DefinitionList(items) => items
            .iter()
            .map(|item| {
                exercise_elements_helpers(&item.key_elements)
                    + exercise_elements_helpers(&item.value_elements)
            })
            .sum(),
        Element::Collapsible { elements, .. } => exercise_elements_helpers(elements),
        Element::Color { elements, .. } => exercise_elements_helpers(elements),
        Element::Include { elements, .. } => exercise_elements_helpers(elements),
        Element::Partial(partial) => exercise_partial_element_helpers(partial),
        _ => 0,
    };

    nested_count + 1
}

fn exercise_list_item_helpers(item: &ListItem<'_>) -> usize {
    match item {
        ListItem::Elements { elements, .. } => exercise_elements_helpers(elements),
        ListItem::SubList { element } => exercise_element_helpers(element),
    }
}

fn exercise_partial_element_helpers(partial: &PartialElement<'_>) -> usize {
    let _name = partial.name();
    let _error_kind = partial.parse_error_kind();
    let _owned = partial.to_owned();

    match partial {
        PartialElement::WikidotEmptyInlineOwner
        | PartialElement::InlineSizeOpen(_)
        | PartialElement::InlineSizeClose(_)
        | PartialElement::InlineSpanOpen(_)
        | PartialElement::InlineSpanClose(_) => 0,
        PartialElement::ListItem(item) => exercise_list_item_helpers(item),
        PartialElement::TableRow(row) => row
            .cells
            .iter()
            .map(|cell| exercise_elements_helpers(&cell.elements))
            .sum(),
        PartialElement::TableCell(cell) => exercise_elements_helpers(&cell.elements),
        PartialElement::Tab(tab) => exercise_elements_helpers(&tab.elements),
        PartialElement::RubyText(text) => exercise_elements_helpers(&text.elements),
    }
}
