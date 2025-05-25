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
    pub fn run(&self) -> TestResult {
        println!("+ {}", self.name);
        // TODO
        todo!()
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
*/
