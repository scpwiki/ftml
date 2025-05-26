/*
 * test/ast/loader.rs
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

//! Submodule responsible for defining the AST test loader system.

use super::{Test, TestUniverse};
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

// File helper functions

fn open_file(path: &Path) -> File {
    match File::open(path) {
        Ok(file) => file,
        Err(error) => {
            panic!("Unable to open file '{}': {}", path.display(), error)
        }
    }
}

fn read_text_file(path: &Path) -> String {
    let mut file = open_file(path);
    let mut contents = String::new();

    if let Err(error) = file.read_to_string(&mut contents) {
        panic!("Unable to read file '{}': {}", path.display(), error);
    }

    process_newlines(&mut contents);

    if contents.ends_with('\n') {
        contents.pop();
    }

    contents
}

fn read_json<T: DeserializeOwned>(path: &Path) -> T {
    let mut file = open_file(path);

    match serde_json::from_reader(&mut file) {
        Ok(object) => object,
        Err(error) => {
            panic!("Unable to parse JSON file '{}': {}", path.display(), error);
        }
    }
}

// String helper functions

fn convert_os_string(s: OsString) -> String {
    match s.into_string() {
        Ok(s) => s,
        Err(s) => panic!("Unable to convert OsString: {}", s.display()),
    }
}

// Windows compatibility

#[cfg(not(target_os = "windows"))]
fn process_newlines(_: &mut String) {}

#[cfg(target_os = "windows")]
fn process_newlines(text: &mut String) {
    while let Some(idx) = text.find("\r\n") {
        let range = idx..idx + 2;
        text.replace_range(range, "\n");
    }
}

// Main loader functionality

impl TestUniverse {
    /// Loads all tests from the filesystem.
    ///
    /// There is a particular directory structure to AST tests.
    /// Within `/test` in the repo, there is a set of directories,
    /// which correspond to the main "test groups", a set of tests
    /// which are related in some way (generally to a specific piece
    /// of syntax or functionality).
    ///
    /// Then within each group, is another set of directories, which
    /// are each actual test case.
    ///
    /// For instance, consider this structure:
    /// ```text
    /// test/
    /// ├── diff
    /// │   ├── alias
    /// │   ├── basic
    /// │   └── newlines
    /// └── underline
    ///     ├── basic
    ///     ├── empty
    ///     └── fail
    /// ```
    ///
    /// This will create six test cases:
    /// * `diff/alias`
    /// * `diff/basic`
    /// * `diff/newlines`
    /// * `underline/basic`
    /// * `underline/empty`
    /// * `underline/fail`
    pub fn load(test_dir: &Path) -> Self {
        let mut tests = BTreeMap::new();

        // Read all test groups
        for entry in fs::read_dir(test_dir).expect("Unable to read dir") {
            let entry = entry.expect("Unable to read dir entry");
            let metadata = entry.metadata().expect("Unable to get dir entry metadata");
            let path = entry.path();
            let test_group = convert_os_string(entry.file_name());

            if metadata.is_dir() {
                // Read all individual tests
                Self::load_group(&mut tests, &test_group, &path);
            } else {
                // TODO: Remove this branch and panic.
                //       But for now, let's ignore any of these files until they're all moved over.
                println!("+ Ignoring file: {}", path.display());
            }
        }

        TestUniverse { tests }
    }

    fn load_group(tests: &mut BTreeMap<String, Test>, test_group: &str, test_dir: &Path) {
        for entry in fs::read_dir(test_dir).expect("Unable to read dir") {
            let entry = entry.expect("Unable to read dir entry");
            let metadata = entry.metadata().expect("Unable to get dir entry metadata");
            let path = entry.path();
            let name = {
                // Write out the test name as 'group/name'
                let mut test_name = convert_os_string(entry.file_name());
                test_name.insert(0, '/');
                test_name.insert_str(0, test_group);
                test_name
            };

            if !metadata.is_dir() {
                panic!("Found a non-directory test path: {}", path.display());
            }

            // Read test object
            let test = Test::load(name.clone(), &path);
            tests.insert(name, test);
        }
    }
}

impl Test {
    /// Constructs a particular test case.
    pub fn load(name: String, test_dir: &Path) -> Self {
        let mut input = None;
        let mut tree = None;
        let mut errors = None;
        let mut wikidot_output = None;
        let mut html_output = None;
        let mut text_output = None;

        for entry in fs::read_dir(test_dir).expect("Unable to read dir") {
            let entry = entry.expect("Unable to read dir entry");
            let path = entry.path();
            let filename = path
                .file_name()
                .expect("No basename from read_dir path")
                .to_str()
                .expect("Encountered non-UTF-8 path");

            match filename {
                "input.ftml" => input = Some(read_text_file(&path)),
                "ast.json" => tree = Some(read_json(&path)),
                "errors.json" => errors = Some(read_json(&path)),
                "wikidot.html" => wikidot_output = Some(read_text_file(&path)),
                "output.html" => html_output = Some(read_text_file(&path)),
                "output.txt" => text_output = Some(read_text_file(&path)),
                _ => panic!("Unexpected file in AST test: {}", entry.path().display()),
            }
        }

        // Extract required field, with better panic message
        let input = match input {
            Some(input) => input,
            None => panic!("No wikitext file (input.ftml) found for test '{name}'!"),
        };

        // Ensure syntax tree is present for fail tests
        if errors.is_some() {
            assert!(
                tree.is_some(),
                "No syntax tree file (ast.json) found for test '{name}' with errors.json",
            );
        }

        let test = Test {
            name,
            input,
            tree,
            errors,
            wikidot_output,
            html_output,
            text_output,
        };

        assert!(
            test.has_something_to_do(),
            "Test '{}' has nothing to do! Add at least one expected output file",
            test.name,
        );

        test
    }

    #[inline]
    pub fn has_something_to_do(&self) -> bool {
        self.tree.is_some()
            || self.errors.is_some()
            || self.wikidot_output.is_some()
            || self.html_output.is_some()
            || self.text_output.is_some()
    }
}
