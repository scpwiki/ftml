/*
 * delayed/seam_equivalence.rs
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

use super::seam::resolve_static_suppressions;
use crate::delayed::DelayedElement;
use crate::preproc::typography::{
    substitute_wikidot, substitute_wikidot_deferring_conditionals,
};
use crate::tree::Element;
use std::borrow::Cow;

const BLOCK: &str = "[[iftags +zz-no-such-tag]]x[[/iftags]]";

const TOKENS: &[&str] = &[
    "\"", "'", "a", " ", ".", "...", "<", ">", "<<", ">>", "\u{e9}",
];

fn typography(raw: &str) -> String {
    let mut text = raw.to_owned();
    substitute_wikidot(&mut text);
    text
}

fn preprocess_pieces(source: &str) -> Vec<String> {
    let mut text = source.to_owned();
    substitute_wikidot_deferring_conditionals(&mut text);
    text.split(BLOCK).map(str::to_owned).collect()
}

fn difference_class(got: &str, expected: &str) -> String {
    let mut counts: std::collections::BTreeMap<char, i64> =
        std::collections::BTreeMap::new();
    for c in got.chars() {
        *counts.entry(c).or_default() += 1;
    }
    for c in expected.chars() {
        *counts.entry(c).or_default() -= 1;
    }
    let mut class = String::from("text-differs:");
    for (c, n) in counts {
        if n != 0 {
            class.push_str(&format!(" {c}{}", if n > 0 { "+" } else { "-" }));
        }
    }
    class
}

fn pick_has_right_angle(pick: &[usize]) -> bool {
    pick.iter()
        .any(|&p| p < TOKENS.len() && matches!(TOKENS[p], ">" | ">>"))
}

fn check(pick: &[usize]) -> Option<(String, String)> {
    check_inner(pick).map(|(class, message)| {
        if pick_has_right_angle(pick) {
            (format!("right-angle:{class}"), message)
        } else {
            (class, message)
        }
    })
}

fn check_inner(pick: &[usize]) -> Option<(String, String)> {
    let seam = TOKENS.len();
    let mut segments: Vec<String> = vec![String::new()];
    for &p in pick {
        if p == seam {
            segments.push(String::new());
        } else {
            segments.last_mut().unwrap().push_str(TOKENS[p]);
        }
    }
    let describe = || {
        segments
            .iter()
            .map(|s| format!("{s:?}"))
            .collect::<Vec<_>>()
            .join(" | ")
    };

    let with_blocks = segments.join(BLOCK);
    let pieces = preprocess_pieces(&with_blocks);
    debug_assert_eq!(pieces.len(), segments.len());

    let mut elements: Vec<Element<'static>> = Vec::new();
    for (i, piece) in pieces.iter().enumerate() {
        if i > 0 {
            elements.push(Element::Delayed(DelayedElement::typography_boundary()));
        }
        if !piece.is_empty() {
            elements.push(Element::Text(Cow::Owned(piece.clone())));
        }
    }
    let had_seam = segments.len() > 1;

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        resolve_static_suppressions(&mut elements, true);
        elements
    }));
    let Ok(elements) = outcome else {
        return Some(("panic".into(), format!("segments {}: panicked", describe())));
    };
    if elements.iter().any(|e| matches!(e, Element::Delayed(_))) {
        return Some((
            "seam-survived".into(),
            format!("segments {}: a seam survived", describe()),
        ));
    }
    let mut got = String::new();
    for element in &elements {
        match element {
            Element::Text(text) => got.push_str(text),
            other => {
                return Some((
                    "unexpected-element".into(),
                    format!("segments {}: unexpected element {other:?}", describe()),
                ));
            }
        }
    }
    let expected = typography(&segments.concat());
    if got != expected {
        let _ = had_seam;
        return Some((
            difference_class(&got, &expected),
            format!(
                "segments {}: got {got:?} but typography of the joined text is {expected:?}",
                describe()
            ),
        ));
    }
    None
}

#[test]
#[ignore = "run through verification/seam-typography/run.sh"]
fn seams_are_transparent_to_wikidot_typography() {
    let max_tokens: usize = std::env::var("FTML_SEAM_TOKENS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let choices = TOKENS.len() + 1;
    std::panic::set_hook(Box::new(|_| {}));
    let mut checked = 0u64;
    let mut shortest: std::collections::BTreeMap<String, (usize, u64, String)> =
        std::collections::BTreeMap::new();
    let mut failure_count = 0u64;
    for len in 0..=max_tokens {
        let mut pick = vec![0usize; len];
        loop {
            checked += 1;
            if let Some((class, message)) = check(&pick) {
                failure_count += 1;
                shortest.entry(class).or_insert((len, 0, message)).1 += 1;
            }
            let mut pos = 0;
            while pos < len {
                pick[pos] += 1;
                if pick[pos] < choices {
                    break;
                }
                pick[pos] = 0;
                pos += 1;
            }
            if pos == len {
                break;
            }
        }
    }
    let _ = std::panic::take_hook();
    for (class, (len, count, example)) in &shortest {
        println!("\nCLASS {class}|{len}|{count}|{example}");
    }
    println!(
        "\nSEAM-BOUND max_tokens={max_tokens} checked={checked} failures={failure_count}"
    );
    assert!(
        shortest.is_empty(),
        "{failure_count} inputs broke the property in {} classes (listed above)",
        shortest.len()
    );
}

#[test]
#[ignore = "end-to-end seam check; run explicitly with --ignored"]
fn seams_are_transparent_end_to_end() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    fn render(source: &str) -> String {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut text = source.to_owned();
        crate::preprocess_for_layout(&mut text, Layout::Wikidot);
        let tokens = crate::tokenize(&text);
        let (tree, _) = crate::parse(&tokens, &page_info, &settings).into();
        HtmlRender.render(&tree, &page_info, &settings).body
    }

    let max_tokens = 4usize;
    let choices = TOKENS.len() + 1;
    let seam = TOKENS.len();
    let mut checked = 0u64;
    let mut right_angle_failures = 0u64;
    let mut whitespace_failures = 0u64;
    let mut other_failures = 0u64;
    let mut failures: Vec<String> = Vec::new();
    for len in 0..=max_tokens {
        let mut pick = vec![0usize; len];
        loop {
            checked += 1;
            let mut segments: Vec<String> = vec![String::new()];
            for &p in &pick {
                if p == seam {
                    segments.push(String::new());
                } else {
                    segments.last_mut().unwrap().push_str(TOKENS[p]);
                }
            }
            if segments.len() > 1 {
                let with_blocks = format!("a{}", segments.join(BLOCK));
                let joined = format!("a{}", segments.concat());
                let got = render(&with_blocks);
                let expected = render(&joined);
                if got != expected {
                    if pick_has_right_angle(&pick) {
                        right_angle_failures += 1;
                    } else if got.replace(' ', "") == expected.replace(' ', "") {
                        whitespace_failures += 1;
                    } else {
                        other_failures += 1;
                        if failures.len() < 10 {
                            failures.push(format!(
                                "segments {:?}: with-block rendered {got:?} but joined rendered {expected:?}",
                                segments.join(" | ")
                            ));
                        }
                    }
                }
            }
            let mut pos = 0;
            while pos < len {
                pick[pos] += 1;
                if pick[pos] < choices {
                    break;
                }
                pick[pos] = 0;
                pos += 1;
            }
            if pos == len {
                break;
            }
        }
    }
    println!(
        "SEAM-E2E max_tokens={max_tokens} checked={checked} right_angle_failures={right_angle_failures} whitespace_failures={whitespace_failures} other_failures={other_failures}"
    );
    for failure in &failures {
        println!("E2E-FAIL {failure}");
    }
    assert!(
        other_failures == 0,
        "{other_failures} end-to-end seam mismatches outside the right-angle classes: {failures:?}"
    );
}

#[test]
fn plain_parse_hidden_iftags_typography_matches_joined_text() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    fn render(source: &str) -> String {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut text = source.to_owned();
        crate::preprocess_for_layout(&mut text, Layout::Wikidot);
        let tokens = crate::tokenize(&text);
        let (tree, _) = crate::parse(&tokens, &page_info, &settings).into();
        HtmlRender.render(&tree, &page_info, &settings).body
    }

    const HIDDEN: &str = "[[iftags +zz-no-such-tag]]x[[/iftags]]";

    assert_eq!(render(&format!("<{HIDDEN}<<<")), "<p>««</p>");
    assert_eq!(render(&format!(".{HIDDEN}...")), "<p>….</p>");
    assert_eq!(render(&format!(".{HIDDEN}.....")), "<p>……</p>");
    assert_eq!(render(&format!("\"{HIDDEN}>>>>")), "<p>&quot;»»</p>");

    assert_eq!(
        render("<[[iftags -zz-no-such-tag]]<<<[[/iftags]]"),
        "<p>««</p>",
    );
    assert_eq!(
        render("<[[iftags -zz-no-such-tag]]x[[/iftags]]<<<"),
        "<p>&lt;x«&lt;</p>",
    );
    assert_eq!(
        render(".[[iftags -zz-no-such-tag]]...[[/iftags]]"),
        "<p>….</p>",
    );
    assert_eq!(
        render(".[[iftags -zz-no-such-tag]]x[[/iftags]]..."),
        "<p>.x…</p>",
    );

    assert_eq!(
        render("<[[iftags -zz-no-such-tag]]**b**[[/iftags]]<<<"),
        "<p>&lt;<strong>b</strong>«&lt;</p>",
    );

    assert_eq!(render("[[/iftags]]..."), "<p>[[/iftags]]…</p>");
    assert_eq!(
        render("[[ifcategory x]]y[[/ifcategory]]..."),
        "<p>[[ifcategory x]]y[[/ifcategory]]…</p>",
    );
    let unclosed = render("<[[iftags -zz-no-such-tag]]<<<");
    assert!(
        !unclosed.contains("<<<"),
        "an unclosed opener must not defer the run: {unclosed}",
    );
    assert!(unclosed.contains('«'), "the run still converts: {unclosed}");

    assert_eq!(render(&format!("<\u{ab}{HIDDEN}<")), "<p>&lt;«&lt;</p>",);
}
