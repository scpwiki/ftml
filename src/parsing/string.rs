/*
 * parsing/string.rs
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

use crate::parsing::check_step::check_step;
use crate::parsing::collect::collect_text;
use crate::parsing::condition::ParseCondition;
use crate::parsing::rule::Rule;
use crate::parsing::{ParseError, ParseErrorKind, Parser, Token};
use std::borrow::Cow;

impl<'r, 't> Parser<'r, 't>
where
    'r: 't,
{
    pub fn get_quoted_string(&mut self, rule: Rule) -> Result<Cow<'t, str>, ParseError> {
        let escaped = self.get_quoted_string_escaped(rule)?;
        let value = parse_string(escaped);
        Ok(value)
    }

    pub fn get_quoted_string_escaped(
        &mut self,
        rule: Rule,
    ) -> Result<&'t str, ParseError> {
        let malformed = ParseErrorKind::BlockMalformedArguments;
        check_step(self, Token::DoubleQuote, malformed)?;

        let close = [ParseCondition::current(Token::DoubleQuote)];
        let invalid = [
            ParseCondition::current(Token::LineBreak),
            ParseCondition::current(Token::ParagraphBreak),
            ParseCondition::current(Token::InputEnd),
        ];
        collect_text(self, rule, &close, &invalid, Some(malformed))
    }
}

/// Parses a double-quoted string.
///
/// Takes inputs starting and ending with `"`
/// and containing characters, or any of these
/// escapes:
/// * `\\`
/// * `\"`
/// * `\'`
/// * `\r`
/// * `\n`
/// * `\t`
///
/// If in invalid escape is found, the input
/// is returned. So for `\$`, it will emit a
/// `\` followed by a `$`.
pub fn parse_string(input: &str) -> Cow<'_, str> {
    let input = input
        .strip_prefix('"')
        .and_then(|stripped| stripped.strip_suffix('"'))
        .unwrap_or(input);

    if !input.contains('\\') {
        trace!("No escapes, returning as-is: {:?}", input);
        return Cow::Borrowed(input);
    }

    let mut output = String::new();
    let mut wants_escape = false;

    for ch in input.chars() {
        if wants_escape {
            match escape_char(ch) {
                Some(replacement) => {
                    trace!("Replacing backslash escape: \\{ch}");
                    output.push(replacement);
                }
                None => {
                    warn!("Invalid backslash escape found, ignoring: \\{ch}");
                    output.push('\\');
                    output.push(ch);
                }
            }

            wants_escape = false;
        } else if ch == '\\' {
            wants_escape = true;
        } else {
            output.push(ch);
        }
    }

    Cow::Owned(output)
}

fn escape_char(ch: char) -> Option<char> {
    let escaped = match ch {
        '\\' => '\\',
        '\"' => '\"',
        '\'' => '\'',
        'r' => '\r',
        'n' => '\n',
        't' => '\t',
        _ => return None,
    };

    Some(escaped)
}

#[test]
fn quoted_string_escaped() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::rule::impls::RULE_LIST;
    use crate::settings::{WikitextMode, WikitextSettings};

    macro_rules! test {
        ($steps:expr, $wikitext:expr, $expected:expr) => {{
            let page_info = PageInfo::dummy();
            let settings =
                WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
            let tokenization = crate::tokenize($wikitext);
            let mut parser = Parser::new(&tokenization, &page_info, &settings);

            parser.step_n($steps + 1).expect("Unable to step");

            let actual = parser
                .get_quoted_string(RULE_LIST)
                .expect("Unable to get string value");

            assert_eq!(
                actual, $expected,
                "Extracted string value doesn't match actual",
            );
        }};
    }

    test!(0, "\"\"", "");
    test!(0, "\"alpha\"", "alpha");
    test!(1, "beta\"gamma\"", "gamma");
    test!(1, "beta\"A B C\"delta", "A B C");
    test!(2, "gamma \"\" epsilon", "");
    test!(2, "gamma \"foo\\nbar\\txyz\"", "foo\nbar\txyz");
}

#[test]
fn test_parse_string() {
    macro_rules! test {
        ($input:expr, $expected:expr, $variant:tt $(,)?) => {{
            let actual = parse_string($input);

            assert_eq!(
                &actual, $expected,
                "Actual string (left) doesn't match expected (right)"
            );

            assert!(
                matches!(actual, Cow::$variant(_)),
                "Outputted string of the incorrect variant",
            );
        }};
    }

    test!("", "", Borrowed);
    test!("!", "!", Borrowed);
    test!(r#""""#, "", Borrowed);
    test!(r#""!""#, "!", Borrowed);
    test!(r#""apple banana""#, "apple banana", Borrowed);
    test!(r#""\n def""#, "\n def", Owned);
    test!(r#"\""#, "\"", Owned);
    test!(r#"\'"#, "\'", Owned);
    test!(r"apple banana", "apple banana", Borrowed);
    test!(r"abc \\", "abc \\", Owned);
    test!(r"\n def", "\n def", Owned);
    test!(
        r"abc \t (\\\t) \r (\\\r) def",
        "abc \t (\\\t) \r (\\\r) def",
        Owned,
    );
    test!(r"abc \t \x \y \z \n \0", "abc \t \\x \\y \\z \n \\0", Owned);
}
