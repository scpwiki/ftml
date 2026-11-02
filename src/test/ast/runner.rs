/*
 * test/ast/runner.rs
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

use super::{Test, TestResult, TestStats, TestUniverse};
use crate::data::{PageInfo, ScoreValue};
use crate::layout::Layout;
use crate::render::Render;
use crate::render::html::HtmlRender;
use crate::render::text::TextRender;
use crate::settings::{WikitextMode, WikitextSettings};
use crate::test::includer::TestIncluder;
use std::borrow::Cow;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

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
            if only_tests.is_empty() || test_applies(test_name, only_tests) {
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

    pub fn update(&self, test_dir: &Path, skip_tests: &[&str], only_tests: &[&str]) {
        let mut path = PathBuf::from(test_dir);
        for (test_name, test) in &self.tests {
            if (only_tests.is_empty() || test_applies(test_name, only_tests))
                && !test_applies(test_name, skip_tests)
            {
                path.push(test_name);
                test.update(&path);
                path.pop();
                path.pop();
            }
        }
    }
}

impl Test {
    fn page_info(&self) -> PageInfo<'_> {
        let (group, unit) = self.name.split_once('/').expect("Invalid test name");

        PageInfo {
            page: Cow::Owned(format!("page-{group}-{unit}")),
            category: Some(cow!("test")),
            site: cow!("ast-test"),
            title: Cow::Owned(format!("Test {}", self.name)),
            alt_title: None,
            score: ScoreValue::Integer(10),
            tags: vec![cow!("fruit"), cow!("component")],
            language: match self.locale.as_deref() {
                Some(locale) => cow!(locale),
                None => cow!("default"),
            },
        }
    }

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
        tree.wikitext_len = 0;

        let mut result = TestResult::Pass;

        if let Some(expected_tree) = &self.tree {
            let actual_tree = &tree;
            if actual_tree != expected_tree {
                result = TestResult::Fail;
                eprintln!("AST did not match:");
                eprintln!("Expected: {}", json(&expected_tree));
                eprintln!("Actual:   {}", json(&actual_tree));
            }
        }

        let expected_errors = match self.errors {
            Some(ref errors) => errors.as_slice(),
            None => &[],
        };
        if actual_errors != expected_errors {
            result = TestResult::Fail;
            eprintln!("Parse errors did not match:");
            eprintln!("Expected: {}", json(&expected_errors));
            eprintln!("Actual:   {}", json(&actual_errors));
        }

        if let Some(expected_html) = &self.wikidot_output {
            let settings = settings!(Wikidot);
            let (mut wikidot_text, _pages) =
                crate::include(&self.input, &settings, TestIncluder, || unreachable!())
                    .unwrap_or_else(|x| match x {});
            crate::preprocess_for_layout(&mut wikidot_text, settings.layout);
            let wikidot_tokens = crate::tokenize(&wikidot_text);
            let (wikidot_tree, _) =
                crate::parse(&wikidot_tokens, &page_info, &settings).into();
            let actual_output = HtmlRender.render(&wikidot_tree, &page_info, &settings);
            if actual_output.body != *expected_html {
                result = TestResult::Fail;
                eprintln!("Wikidot HTML did not match:");
                eprintln!("Expected: {:?}", expected_html);
                eprintln!("Actual:   {:?}", actual_output.body);
            }
        }

        if let Some(expected_html) = &self.html_output {
            let settings = settings!(Wikijump);
            let actual_output = HtmlRender.render(&tree, &page_info, &settings);
            let expected_html = harden_expected_new_tab_links(expected_html);
            if actual_output.body != expected_html {
                result = TestResult::Fail;
                eprintln!("Wikijump HTML did not match:");
                eprintln!("Expected: {:?}", expected_html);
                eprintln!("Actual:   {:?}", actual_output.body);
            }
        }

        if let Some(expected_text) = &self.text_output {
            let settings = settings!(Wikijump);
            let actual_text = TextRender.render(&tree, &page_info, &settings);
            if &actual_text != expected_text {
                result = TestResult::Fail;
                eprintln!("Text output did not match:");
                eprintln!("Expected: {}", expected_text);
                eprintln!("Actual:   {}", actual_text);
            }
        }

        result
    }

    pub fn update(&self, directory: &Path) {
        println!("+ {}", self.name);

        let page_info = self.page_info();
        let parse_settings = settings!(Wikijump);
        let mut path = PathBuf::from(directory);

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
        let (mut tree, errors) = result.into();
        tree.wikitext_len = 0;

        macro_rules! update {
            ($write_func:ident, $object:expr, $filename:expr $(,)?) => {{
                println!("= {}/{}", self.name, $filename);
                path.push($filename);
                $write_func(&path, &$object);
                path.pop();
            }};
        }

        if let Some(expected_tree) = &self.tree {
            let actual_tree = &tree;
            if actual_tree != expected_tree {
                update!(write_json, actual_tree, "tree.json");
            }
        }

        if !errors.is_empty() {
            path.push("errors.json");
            let errors_file_exists = fs::exists(&path).ok().unwrap_or(false);
            if !errors_file_exists {
                panic!(
                    "Parser errors produced for test '{}', but no errors.json file: {}",
                    self.name,
                    json(&errors),
                );
            }
            path.pop();
        }

        let expected_errors = match self.errors {
            Some(ref errors) => errors.as_slice(),
            None => &[],
        };
        if errors != expected_errors {
            update!(write_json, errors, "errors.json");
        }

        if let Some(expected_html) = &self.wikidot_output {
            let settings = settings!(Wikidot);
            let (mut wikidot_text, _pages) =
                crate::include(&self.input, &settings, TestIncluder, || unreachable!())
                    .unwrap_or_else(|x| match x {});
            crate::preprocess_for_layout(&mut wikidot_text, settings.layout);
            let wikidot_tokens = crate::tokenize(&wikidot_text);
            let (wikidot_tree, _) =
                crate::parse(&wikidot_tokens, &page_info, &settings).into();
            let html_output = HtmlRender.render(&wikidot_tree, &page_info, &settings);
            if &html_output.body != expected_html {
                update!(write_text, html_output.body, "wikidot.html");
            }
        }

        if let Some(expected_html) = &self.html_output {
            let settings = settings!(Wikijump);
            let html_output = HtmlRender.render(&tree, &page_info, &settings);
            if &html_output.body != expected_html {
                update!(write_text, html_output.body, "output.html");
            }
        }

        if let Some(expected_text) = &self.text_output {
            let settings = settings!(Wikijump);
            let actual_text = TextRender.render(&tree, &page_info, &settings);
            if &actual_text != expected_text {
                update!(write_text, actual_text, "output.txt");
            }
        }
    }
}

fn test_applies(test_name: &str, patterns: &[&str]) -> bool {
    for &pattern in patterns {
        if pattern == test_name {
            return true;
        }

        if pattern.ends_with('/') && test_name.starts_with(pattern) {
            return true;
        }
    }

    false
}

fn harden_expected_new_tab_links(html: &str) -> Cow<'_, str> {
    const TARGET: &str = r#"target="_blank""#;
    const REL: &str = r#" rel="noopener noreferrer""#;

    let Some(first) = html.find(TARGET) else {
        return Cow::Borrowed(html);
    };

    let mut output = String::with_capacity(html.len() + REL.len());
    output.push_str(&html[..first]);
    let mut remaining = &html[first..];

    while let Some(index) = remaining.find(TARGET) {
        output.push_str(&remaining[..index + TARGET.len()]);
        remaining = &remaining[index + TARGET.len()..];
        if !remaining.starts_with(REL) {
            output.push_str(REL);
        }
    }
    output.push_str(remaining);
    Cow::Owned(output)
}

#[test]
fn ast_fixtures_require_new_tab_link_isolation() {
    assert_eq!(
        harden_expected_new_tab_links(
            r#"<a target="_blank">A</a><a target="_blank" rel="noopener noreferrer">B</a>"#,
        ),
        r#"<a target="_blank" rel="noopener noreferrer">A</a><a target="_blank" rel="noopener noreferrer">B</a>"#,
    );
}

fn json_writer<T, W>(object: &T, writer: W)
where
    T: serde::Serialize,
    W: Write,
{
    use serde_json::ser::{PrettyFormatter, Serializer};

    let fmt = PrettyFormatter::with_indent(b"    ");
    let mut ser = Serializer::with_formatter(writer, fmt);
    object
        .serialize(&mut ser)
        .expect("JSON serialization failed");
}

fn json<T>(object: &T) -> String
where
    T: serde::Serialize,
{
    let mut buffer = Vec::with_capacity(256);
    json_writer(object, &mut buffer);
    String::from_utf8(buffer).expect("JSON was not valid UTF-8")
}

fn write_json<T>(path: &Path, object: &T)
where
    T: serde::Serialize,
{
    let mut file = File::create(path).expect("Unable to create file");
    json_writer(object, &mut file);

    file.write_all(b"\n")
        .expect("Unable to write final newline to file");
}

fn write_text(path: &Path, contents: &str) {
    let mut file = File::create(path).expect("Unable to create file");

    file.write_all(contents.as_bytes())
        .expect("Unable to write bytes");

    file.write_all(b"\n")
        .expect("Unable to write final newline to file");
}
