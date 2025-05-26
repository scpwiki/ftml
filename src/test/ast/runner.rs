/*
 * test/ast/runner.rs
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

//! Submodule responsible for defining the AST test runner.

use super::{Test, TestResult, TestStats, TestUniverse};
use crate::data::{PageInfo, ScoreValue};
use crate::layout::Layout;
use crate::parsing::ParseError;
use crate::render::html::HtmlRender;
use crate::render::text::TextRender;
use crate::render::Render;
use crate::settings::{WikitextMode, WikitextSettings};
use crate::test::includer::TestIncluder;
use crate::tree::SyntaxTree;
use std::borrow::Cow;

macro_rules! cow {
    ($value:expr $(,)?) => {
        Cow::Borrowed(&$value)
    };
}

macro_rules! settings {
    ($layout:ident $(,)?) => {
        WikitextSettings::from_mode(WikitextMode::Page, Layout::$layout)
    };
}

impl TestUniverse {
    pub fn run(&self, skip_tests: &[&str], only_tests: &[&str]) -> TestStats {
        let mut stats = TestStats::new();
        for (test_name, test) in &self.tests {
            // Either we are running all tests (is empty),
            // or it's one of the only tests we're actually running.
            if only_tests.is_empty() || test_applies(test_name, only_tests) {
                // But not a skipped test
                let result = if test_applies(test_name, skip_tests) {
                    TestResult::Skip
                } else {
                    test.run()
                };

                stats.add(result);
            }
        }
        stats
    }
}

impl Test {
    fn page_info(&self) -> PageInfo<'static> {
        let (group, unit) = self.name.split_once('/').expect("Invalid test name");

        PageInfo {
            page: Cow::Owned(format!("page-{group}-{unit}")),
            category: Some(cow!("test")),
            site: cow!("ast-test"),
            title: Cow::Owned(format!("Test {}", self.name)),
            alt_title: None,
            score: ScoreValue::Integer(10),
            tags: vec![cow!("fruit"), cow!("component")],
            language: cow!("default"),
        }
    }

    /// Runs this test, yielding its result.
    ///
    /// # Returns
    /// Either `TestResult::Pass` or `TestResult::Fail`.
    pub fn run(&self) -> TestResult {
        println!("+ {}", self.name);

        let page_info = self.page_info();
        let parse_settings = settings!(Wikijump);

        let (mut text, _pages) = crate::include(
            &self.input,
            &parse_settings,
            TestIncluder,
            || unreachable!(),
        )
        .unwrap_or_else(|x| match x {});

        crate::preprocess(&mut text);
        let tokens = crate::tokenize(&text);
        let result = crate::parse(&tokens, &page_info, &parse_settings);
        let (mut tree, actual_errors) = result.into();
        tree.wikitext_len = 0; // not stored in the JSON, need for correct eq

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

        // Check abstract syntax tree
        if let Some(expected_tree) = &self.tree {
            let actual_tree = &tree;
            if actual_tree != expected_tree {
                result = TestResult::Fail;
                eprintln!("AST did not match:");
                eprintln!("Expected: {}", json(&expected_tree));
                eprintln!("Actual:   {}", json(&actual_tree));
            }
        }

        // Check errors
        //
        // We always check this, since if there _are_ errors
        // but there is no errors.json file, then
        let expected_errors = match self.errors {
            Some(ref errors) => errors.as_slice(),
            None => &[],
        };
        if &actual_errors != expected_errors {
            result = TestResult::Fail;
            eprintln!("Parse errors did not match:");
            eprintln!("Expected: {}", json(&expected_errors));
            eprintln!("Actual:   {}", json(&actual_errors));
        }

        // Run and check wikidot render
        if let Some(expected_html) = &self.wikidot_output {
            let settings = settings!(Wikidot);
            let actual_output = HtmlRender.render(&tree, &page_info, &settings);
            eprintln!("Wikidot HTML did not match:");
            eprintln!("Expected: {}", expected_html);
            eprintln!("Actual:   {}", actual_output.body);
        }

        // Run and check wikijump render
        if let Some(expected_html) = &self.html_output {
            let settings = settings!(Wikijump);
            let actual_output = HtmlRender.render(&tree, &page_info, &settings);
            eprintln!("Wikijump HTML did not match:");
            eprintln!("Expected: {}", expected_html);
            eprintln!("Actual:   {}", actual_output.body);
        }

        // Run and check text render
        if let Some(expected_text) = &self.text_output {
            let settings = settings!(Wikijump);
            let actual_text = TextRender.render(&tree, &page_info, &settings);
            eprintln!("Text output did not match:");
            eprintln!("Expected: {}", expected_text);
            eprintln!("Actual:   {}", actual_text);
        }

        result
    }
}

// Helper functions

/// Determine if any of the given patterns apply to this test.
/// What "apply" means depends on the function:
/// * `SKIP_TESTS` &mdash; This test should be skipped.
/// * `ONLY_TESTS` &mdash; This is one of the only tests to be run.
fn test_applies(test_name: &str, patterns: &[&str]) -> bool {
    for &pattern in patterns {
        // Literal test name
        if pattern == test_name {
            return true;
        }

        // Test group, match as a prefix
        // e.g. 'underline/' to match all underline tests
        if pattern.ends_with('/') {
            if test_name.starts_with(pattern) {
                return true;
            }
        }
    }

    false
}
