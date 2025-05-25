/*
 * test/ast/mod.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2025 Wikijump Team
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

//! Runs AST tests, stored in `/test`, where a given input wikitext file
//! is processed and a variety of assertions can be done on its output.

mod loader;
mod runner;

use super::includer::TestIncluder;
use crate::data::{PageInfo, ScoreValue};
use crate::layout::Layout;
use crate::parsing::ParseError;
use crate::render::html::HtmlRender;
use crate::render::Render;
use crate::settings::{WikitextMode, WikitextSettings};
use crate::tree::SyntaxTree;
use once_cell::sync::Lazy;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process;

/// Temporary measure to not run certain tests.
///
/// This is meant to help with development, or in specific circumstances
/// where it is known functionality is broken while alternatives are
/// being developed.
const SKIP_TESTS: &[&str] = &[];

/// Temporary measure to only run certain tests.
///
/// This can assist with development, when you only care about specific
/// tests to check if certain functionality is working as expected.
const ONLY_TESTS: &[&str] = &[];

static TEST_DIRECTORY: Lazy<PathBuf> = Lazy::new(|| {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("test");
    path
});

macro_rules! cow {
    ($text:expr) => {
        Cow::Borrowed(&$text)
    };
}

// Structs

/// Represents a particular result from a test execution.
#[derive(Debug, Copy, Clone)]
pub enum TestResult {
    Pass,
    Fail,
    Skip,
}

/// Represents one AST unit test case.
#[derive(Debug)]
pub struct Test {
    /// The name of this test.
    /// This is composed of two parts joined with a `/`.
    /// This is unique among all AST tests in the universe.
    pub name: String,

    /// The wikitext input for this test.
    /// Read from `input.ftml`. This file is required.
    pub input: String,

    /// The abstract syntax tree to check the output against.
    /// Read from `ast.json`.
    pub tree: Option<SyntaxTree<'static>>,

    /// The list of expected errors to be produced from this input.
    /// Read from `errors.json`.
    pub errors: Option<Vec<ParseError>>,

    /// The Wikidot-layout HTML expected to be generated from this input.
    /// Read from `wikidot.html`.
    pub wikidot_output: Option<String>,

    /// The Wikijump-layout HTML expected to be generated from this input.
    /// Read from `output.html`.
    pub html_output: Option<String>,

    /// The Wikijump-layout text expected to be generated from this input.
    /// This refers to the "text renderer" present in ftml.
    /// Read from `output.txt`.
    pub text_output: Option<String>,
}

/// Represents the universe of all AST unit tests read from the filesystem.
#[derive(Debug)]
pub struct TestUniverse {
    pub tests: BTreeMap<String, Test>,
}

// Debugging execution

fn only_test_should_skip(name: &str) -> bool {
    assert!(!ONLY_TESTS.is_empty());

    for pattern in ONLY_TESTS.iter() {
        // Literal test name
        if pattern == &name {
            return false;
        }

        // Test name prefix
        if pattern.ends_with('-') {
            if name.starts_with(pattern) {
                return false;
            }
        }
    }

    true
}

// Test runner

/*
     XXX
impl Test {
    pub fn run(&self) -> TestResult {
        if SKIP_TESTS.contains(&&*self.name) {
            println!("+ {} [SKIPPED]", self.name);
            return TestResult::Skip;
        }

        if !ONLY_TESTS.is_empty() && only_test_should_skip(&&*self.name) {
            println!("+ {} [SKIPPED]", self.name);
            return TestResult::Skip;
        }

        debug!(
            "Running syntax tree test case {} on {}",
            &self.name, &self.input,
        );

        println!("+ {}", self.name);

        let page_info = PageInfo {
            page: Cow::Owned(format!("page-{}", self.name)),
            category: None,
            site: cow!("test"),
            title: cow!(self.name),
            alt_title: None,
            score: ScoreValue::Integer(0),
            tags: vec![cow!("fruit"), cow!("component")],
            language: cow!("default"),
        };

        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        let (mut text, _pages) =
            crate::include(&self.input, &settings, TestIncluder, || unreachable!())
                .unwrap_or_else(|x| match x {});

        crate::preprocess(&mut text);
        let tokens = crate::tokenize(&text);
        let result = crate::parse(&tokens, &page_info, &settings);
        let (mut tree, errors) = result.into();
        tree.wikitext_len = self.tree.wikitext_len; // not stored in the JSON
        let html_output = HtmlRender.render(&tree, &page_info, &settings);

        fn json<T>(object: &T) -> String
        where
            T: serde::Serialize,
        {
            let mut output = serde_json::to_string_pretty(object)
                .expect("Unable to serialize JSON to stdout");

            output.insert_str(0, "Generated JSON: ");
            output
        }

        let mut result = TestResult::Pass;

        if tree != self.tree {
            result = TestResult::Fail;
            eprintln!(
                "AST did not match:\nExpected: {:#?}\nActual: {:#?}\n{}\nErrors: {:#?}",
                self.tree,
                tree,
                json(&tree),
                &errors,
            );
        }

        if errors != self.errors {
            result = TestResult::Fail;
            eprintln!(
                "Errors did not match:\nExpected: {:#?}\nActual:   {:#?}\n{}\nTree (for reference): {:#?}",
                self.errors,
                errors,
                json(&errors),
                &tree,
            );
        }

        if html_output.body != self.html {
            result = TestResult::Fail;
            eprintln!(
                "HTML does not match:\nExpected: {:?}\nActual:   {:?}\n\n{}\n\nTree (for reference): {:#?}",
                self.html,
                html_output.body,
                html_output.body,
                &tree,
            );
        }

        result
    }
}

#[test]
fn ast_and_html() {
    // Warn if any test are being skipped
    if !SKIP_TESTS.is_empty() {
        println!("=========");
        println!(" WARNING ");
        println!("=========");
        println!();
        println!("The following tests are being SKIPPED:");

        for test in SKIP_TESTS {
            println!("- {}", test);
        }

        println!();
    }

    // Warn if we're only checking certain tests
    if !ONLY_TESTS.is_empty() {
        println!("=========");
        println!(" WARNING ");
        println!("=========");
        println!();
        println!("Only the following tests are being run.");
        println!("All others are being SKIPPED!");

        for test in ONLY_TESTS {
            println!("- {}", test);
        }

        println!();
    }

    // Load tests from JSON files
    let entries = fs::read_dir(&*TEST_DIRECTORY) //
        .expect("Unable to read directory");

    let tests_iter = entries.filter_map(|entry| {
        let entry = entry.expect("Unable to read directory entry");
        let ftype = entry.file_type().expect("Unable to get file type");
        if !ftype.is_file() {
            println!("Skipping non-file {}", file_name!(entry));
            return None;
        }

        let path = entry.path();
        let stem = path
            .file_stem()
            .expect("Unable to get file stem")
            .to_string_lossy();

        let extension = path.extension().map(|s| s.to_str()).flatten();
        match extension {
            // Load JSON test data
            Some("json") => Some(Test::load(&path, &stem)),

            // We expect these, don't print anything
            Some("html") => None,

            // Print for other, unexpected files
            _ => {
                println!("Skipping non-JSON file {}", file_name!(entry));
                None
            }
        }
    });

    // Sort tests by name
    let mut tests: Vec<Test> = tests_iter.collect();
    tests.sort_by(|a, b| (a.name).cmp(&b.name));

    // Run tests
    let mut failed = 0;
    let mut skipped = 0;

    println!("Running {} syntax tree tests:", tests.len());
    for test in &tests {
        match test.run() {
            TestResult::Pass => (),
            TestResult::Fail => failed += 1,
            TestResult::Skip => skipped += 1,
        }
    }

    println!();
    println!("Ran a total of {} tests", tests.len());

    if failed > 0 {
        println!("Of these, {} failed", failed);
    }

    if skipped > 0 {
        // Don't allow accidentally committing skipped tests
        println!("Additionally, {} tests are being skipped", skipped);
        println!("Remember to re-enable all tests before committing!");
    }

    process::exit(failed + skipped);
}
*/
