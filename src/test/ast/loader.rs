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
        panic!("Unable to read file '{}': {}", path.display(), error,);
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
            panic!("Unable to parse JSON file '{}': {}", path.display(), error,);
        }
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
    pub fn load(test_dir: &Path) -> Self {
        todo!()
    }
}

impl Test {
    /// Constructs a particular test case.
    pub fn load(test_dir: &Path) -> Self {
        let name = test_dir
            .file_name()
            .expect("No basename for test directory")
            .to_str()
            .expect("Basename is not valid UTF-8");

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

        let test = Test {
            name: str!(name),
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
            name,
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
