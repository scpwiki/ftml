/*
 * preproc/test.rs
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

use super::{preprocess, preprocess_for_layout};
use crate::layout::Layout;
use proptest::prelude::*;

pub fn test_substitution<F>(filter_name: &str, mut substitute: F, tests: &[(&str, &str)])
where
    F: FnMut(&mut String),
{
    let mut string = String::new();

    for (input, expected) in tests {
        string.clear();
        string.push_str(input);

        debug!("Testing {filter_name} substitution");

        substitute(&mut string);

        assert_eq!(
            &string, expected,
            "Output of {filter_name} substitution test didn't match",
        );
    }
}

const PREFILTER_TEST_CASES: [(&str, &str); 10] = [
    ("", ""),
    ("tab\ttest", "tab    test"),
    (
        "fn main() {\n\tprintln!();\n\tlet _ = ();\n}",
        "fn main() {\n    println!();\n    let _ = ();\n}",
    ),
    ("newlines:\r\nA\rB\nC\nD\n\rE", "newlines:\nA\nB\nC\nD\n\nE"),
    (
        "compress:\nA\n\nB\n\n\nC\n\n\n\nD\n\n\n\n\nE\n\n\n\n\n\n",
        "compress:\nA\n\nB\n\nC\n\nD\n\nE",
    ),
    (
        "concat:\nApple Banana \\\nCherry\\\nPineapple \\ Grape\nBlueberry\n",
        "concat:\nApple Banana CherryPineapple \\ Grape\nBlueberry",
    ),
    ("[\n  \n    \n       \n  \n      \n \n   \n]", "[\n\n]"),
    (
        "SCP-4455-Ω said, ``It was a dark and stormy night. I looked down on my arch-nemesis, the Streamliner.''",
        "SCP-4455-Ω said, “It was a dark and stormy night. I looked down on my arch-nemesis, the Streamliner.”",
    ),
    (
        ",,あんたはばかです！''\n``Ehh?''\n,,ほんと！''",
        "„あんたはばかです！”\n“Ehh?”\n„ほんと！”",
    ),
    (
        " . . . I'm not sure about this,",
        "… I'm not sure about this,",
    ),
];

#[test]
fn prefilter() {
    test_substitution("prefilter", preprocess, &PREFILTER_TEST_CASES);
}

const PREFILTER_PROPTEST_CASES: u32 = 128;
const PREFILTER_PROPTEST_MAX_CHARS: usize = 96;
const STRUCTURAL_PROPTEST_CASES: u32 = 512;

fn prefilter_input() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            Just('a'),
            Just(' '),
            Just('Ω'),
            Just('あ'),
            Just('🦀'),
            Just('\r'),
            Just('\n'),
            Just('\t'),
            Just('\0'),
            Just('\\'),
            Just('.'),
            Just('\''),
            Just('`'),
            Just(','),
        ],
        0..PREFILTER_PROPTEST_MAX_CHARS,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

fn wikidot_structural_input() -> impl Strategy<Value = String> {
    let text = proptest::collection::vec(
        prop_oneof![
            Just('a'),
            Just('é'),
            Just('漢'),
            Just('🦀'),
            Just('\u{0301}'),
            Just('\u{00A0}'),
            Just('\u{2007}'),
            Just('\u{2028}'),
            Just('\u{2029}'),
            Just('\u{000B}'),
            Just('\u{000C}'),
            Just('\0'),
            Just('['),
            Just(']'),
            Just('>'),
            Just('<'),
            Just('|'),
            Just('='),
            Just('"'),
            Just('\''),
            Just('\\'),
            Just(' '),
            Just('\t'),
        ],
        0..32,
    )
    .prop_map(|characters| characters.into_iter().collect::<String>());
    let line_ending = prop_oneof![
        Just("\n".to_owned()),
        Just("\r\n".to_owned()),
        Just("\r".to_owned()),
    ];
    let trailing = prop_oneof![
        Just(String::new()),
        Just(" ".to_owned()),
        Just("\t".to_owned()),
        Just(" \t ".to_owned()),
    ];

    (0_u8..14, text, line_ending, trailing).prop_map(
        |(shape, text, eol, trailing)| match shape {
            0 => format!(
                "[[collapsible show=\"report\"]]{eol}> {text}{eol}> End log.[[/collapsible]]{trailing}{eol}after"
            ),
            1 => format!(
                "[[COLLAPSIBLE show=\"report\"]]{eol}> {text}{eol}> End log.[[/COLLAPSIBLE]]{trailing}{eol}after"
            ),
            2 => format!("[[div class=\"probe\"]]{eol}{text}{eol}[[/div]]"),
            3 => format!("[[span data-probe=\"{text}\"]]body[[/span]]"),
            4 => format!("[[[target|{text}]]]"),
            5 => format!("[https://example.com/{text} label]"),
            6 => format!("[[code]]{eol}{text}{eol}[[/code]]"),
            7 => format!("@@{text}@@"),
            8 => format!("[[#if 1 | {text} | fallback ]]"),
            9 => format!("|| {text} ||{eol}|| next ||"),
            10 => format!("> {text}{eol}>> nested{eol}outside"),
            11 => format!("[[span da[!--x--]ta-owned=\"yes\"]]{text}[[/span]]"),
            12 => format!("**bold //italic {text}** tail//"),
            _ => format!("[[math]]{eol}{text}{eol}[[/math]]"),
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(PREFILTER_PROPTEST_CASES))]

    #[test]
    fn prefilter_prop(mut s in prefilter_input()) {
        crate::preprocess(&mut s);

        const INVALID_SUBSTRINGS: [&str; 4] = [
            "\r\n",
            "\r",
            "\t",
            "\0",
        ];

        for substring in &INVALID_SUBSTRINGS {
            assert!(!s.contains(substring));
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(STRUCTURAL_PROPTEST_CASES))]

    #[test]
    fn structural_preprocessing_accepts_arbitrary_utf8_without_panicking(
        source in wikidot_structural_input(),
    ) {
        for layout in [Layout::Wikidot, Layout::Wikijump] {
            let mut text = source.clone();
            preprocess_for_layout(&mut text, layout);
        }
    }
}
