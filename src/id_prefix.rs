/*
 * id_prefix.rs
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

//! Utility to prefix HTML IDs for isolation.
//!
//! This adds `u-` to the ID value (if non-empty)
//! to ensure non-collision with generated elements.
//!
//! However it is intelligent, and doesn't add the `u-` if
//! it's already prefixed with that.

pub fn isolate_ids(id_string: &str) -> String {
    const PREFIX: &str = "u-";
    if id_string.is_empty() {
        return String::new();
    }

    let mut collapsed = String::with_capacity(id_string.len());
    let mut pending_space = false;
    let mut started = false;
    for ch in id_string.chars() {
        if ch.is_whitespace() {
            if started {
                pending_space = true;
            }
            continue;
        }
        if pending_space {
            collapsed.push(' ');
            pending_space = false;
        }
        started = true;
        collapsed.push(ch);
    }
    if collapsed.is_empty() {
        return PREFIX.to_owned();
    }
    // A leading space means the value does not already start with `u-`.
    // ` u-a` becomes `u- u-a`. `u- a` already starts with the prefix.
    if id_string.starts_with(PREFIX) {
        return collapsed;
    }

    let mut isolated_ids = String::with_capacity(collapsed.len() + PREFIX.len() + 1);
    isolated_ids.push_str(PREFIX);
    if id_string.starts_with(char::is_whitespace) {
        isolated_ids.push(' ');
    }
    isolated_ids.push_str(&collapsed);
    isolated_ids
}

#[test]
fn test_isolate_ids() {
    macro_rules! test {
        ($input:expr, $expected:expr) => {
            assert_eq!(
                isolate_ids($input),
                $expected,
                "Actual isolated ID string doesn't match expected",
            );
        };
    }

    test!("", "");
    test!("  ", "u-");
    test!(" ", "u-");
    test!("\ta", "u- a");
    test!("  a", "u- a");
    test!(" a ", "u- a");
    test!(" a b", "u- a b");
    test!("u- a", "u- a");
    test!("apple", "u-apple");
    test!("apple banana", "u-apple banana");
    test!("apple  banana", "u-apple banana");
    test!(" apple  banana", "u- apple banana");
    test!(" apple   banana ", "u- apple banana");
    test!("apple banana cherry", "u-apple banana cherry");
    test!("  apple  banana\tcherry", "u- apple banana cherry");
    test!("u-apple", "u-apple");
    test!("u-apple banana cherry", "u-apple banana cherry");
    test!("u-apple u-banana cherry", "u-apple u-banana cherry");
    test!("u-apple u-banana u-cherry", "u-apple u-banana u-cherry");
    test!("apple u-banana cherry", "u-apple u-banana cherry");
    test!("u-", "u-");
    test!("u-u-apple", "u-u-apple");
    test!(" u-a", "u- u-a");
}
