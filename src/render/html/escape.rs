/*
 * render/html/escape.rs
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

pub fn escape_wikidot_raw(buffer: &mut String, s: &str) {
    let mut start = 0;

    for (index, byte) in s.bytes().enumerate() {
        let escaped = match byte {
            b' ' => "&#32;",
            b'>' => "&gt;",
            b'<' => "&lt;",
            b'&' => "&amp;",
            b'\'' => "&#39;",
            b'"' => "&quot;",
            b'\0' => " ",
            _ => continue,
        };

        if start < index {
            buffer.push_str(&s[start..index]);
        }

        buffer.push_str(escaped);
        start = index + 1;
    }

    if start < s.len() {
        buffer.push_str(&s[start..]);
    }
}

pub fn escape(buffer: &mut String, s: &str) {
    let mut start = 0;

    for (index, byte) in s.bytes().enumerate() {
        let escaped = match byte {
            b'>' => "&gt;",
            b'<' => "&lt;",
            b'&' => "&amp;",
            b'\'' => "&#39;",
            b'"' => "&quot;",
            b'\0' => " ",
            _ => continue,
        };

        if start < index {
            buffer.push_str(&s[start..index]);
        }

        buffer.push_str(escaped);
        start = index + 1;
    }

    if start < s.len() {
        buffer.push_str(&s[start..]);
    }
}

#[test]
fn test() {
    macro_rules! test {
        ($input:expr, $expected:expr $(,)?) => {{
            let mut buffer = String::new();
            escape(&mut buffer, $input);

            assert_eq!(&buffer, $expected, "Escaped HTML doesn't match expected");
        }};
    }

    test!("", "");
    test!("Hello, world!", "Hello, world!");
    test!("x + 3 > 19, solve for x", "x + 3 &gt; 19, solve for x");
    test!(
        "<script>alert('test');</script>",
        "&lt;script&gt;alert(&#39;test&#39;);&lt;/script&gt;",
    );
    test!(
        "S & C Plastic's location",
        "S &amp; C Plastic&#39;s location",
    );
    test!("null\0byte", "null byte");
    test!("日本語 < α & β", "日本語 &lt; α &amp; β");
    macro_rules! test_raw {
        ($input:expr, $expected:expr $(,)?) => {{
            let mut buffer = String::new();
            escape_wikidot_raw(&mut buffer, $input);

            assert_eq!(&buffer, $expected, "Escaped HTML doesn't match expected");
        }};
    }

    test_raw!("a 0 2px", "a&#32;0&#32;2px");
    test_raw!(" a ", "&#32;a&#32;");
    test_raw!("a&b <c>", "a&amp;b&#32;&lt;c&gt;");
}
