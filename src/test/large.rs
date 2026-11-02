/*
 * test/large.rs
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

use crate::data::PageInfo;
use crate::layout::Layout;
use crate::parsing::{DEEP_MAX_RECURSION_DEPTH, ParseErrorKind, Token};
use crate::settings::{WikitextMode, WikitextSettings};
use crate::tree::{Element, SyntaxTree};
use std::time::{Duration, Instant};

fn run_on_bounded_test_stack(name: &str, test: fn()) {
    std::thread::Builder::new()
        .name(name.to_owned())
        .stack_size(16 * 1024 * 1024)
        .spawn(test)
        .expect("failed to start bounded large-parser test")
        .join()
        .expect("large-parser test panicked");
}

#[test]
fn recursion_depth() {
    run_on_bounded_test_stack(
        "ftml-recursion-depth-test",
        recursion_depth_on_bounded_test_stack,
    );
}

fn recursion_depth_on_bounded_test_stack() {
    let page_info = PageInfo::dummy();
    let wikidot = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

    let mut input = String::new();

    for _ in 0..=DEEP_MAX_RECURSION_DEPTH {
        input.push_str("[[div]]\n");
    }

    for _ in 0..=DEEP_MAX_RECURSION_DEPTH {
        input.push_str("[[/div]]\n");
    }

    crate::preprocess(&mut input);
    let tokens = crate::tokenize(&input);
    let started = Instant::now();
    let (tree, errors) = crate::parse(&tokens, &page_info, &wikidot).into();
    assert!(started.elapsed() < Duration::from_secs(20));

    let error = errors.first().expect("No errors produced");
    assert_eq!(error.token(), Token::LeftBlock);
    assert_eq!(error.rule(), "block-div");
    let error_start = DEEP_MAX_RECURSION_DEPTH * "[[div]]\n".len();
    assert_eq!(error.span(), error_start..error_start + 2);
    assert_eq!(error.kind(), ParseErrorKind::RecursionDepthExceeded);

    let SyntaxTree { elements, .. } = tree;
    assert_eq!(elements.len(), 1);

    let wikijump = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let (tree, errors) = crate::parse(&tokens, &page_info, &wikijump).into();
    assert_eq!(
        errors.first().map(|error| error.kind()),
        Some(ParseErrorKind::RecursionDepthExceeded),
    );
    let SyntaxTree { elements, .. } = tree;
    assert_eq!(elements.len(), 1);
}

#[test]
fn corpus_depth_component_tree_parses_on_bounded_stack_worker() {
    run_on_bounded_test_stack(
        "ftml-corpus-depth-test",
        corpus_depth_component_tree_on_bounded_stack,
    );
}

fn corpus_depth_component_tree_on_bounded_stack() {
    const CORPUS_BACKED_DEPTH: usize = 115;

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut input = "[[div]]\n".repeat(CORPUS_BACKED_DEPTH);
    input.push_str(&"[[/div]]\n".repeat(CORPUS_BACKED_DEPTH));

    crate::preprocess(&mut input);
    let tokens = crate::tokenize(&input);
    let (_, errors) = crate::parse(&tokens, &page_info, &settings).into();

    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn corpus_colmod_tree_parses_on_bounded_stack_worker() {
    run_on_bounded_test_stack(
        "ftml-corpus-colmod-test",
        corpus_colmod_tree_on_bounded_stack,
    );
}

fn corpus_colmod_tree_on_bounded_stack() {
    const CORPUS_BACKED_ROWS: usize = 231;

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut input = String::new();
    for _ in 0..CORPUS_BACKED_ROWS {
        input.push_str(
            "[[div_ class=\"colmod-block\"]]\n\
             [[ul]][[li class=\"folded\"]][[ul]]_[[/ul]][[div class=\"colmod-link-top\"]]\n\
             [[div_ class=\"foldable-list-container\"]]\n\
             link[[/div]][[/div]][[div class=\"colmod-content\"]]\n",
        );
    }
    for _ in 0..CORPUS_BACKED_ROWS {
        input.push_str(
            "[[/div]][[div]]\n\
             [[div_ class=\"foldable-list-container\"]]\n\
             link[[/div]][[/div]][[/li]][[/ul]][[/div]]\n",
        );
    }

    crate::preprocess(&mut input);
    let tokens = crate::tokenize(&input);
    let started = Instant::now();
    let (_, errors) = crate::parse(&tokens, &page_info, &settings).into();

    assert!(started.elapsed() < Duration::from_secs(20));
    assert!(
        !errors
            .iter()
            .any(|error| error.kind() == ParseErrorKind::RecursionDepthExceeded),
        "{errors:#?}",
    );
}

#[test]
fn nested_unclosed_divs() {
    const ITERATIONS: usize = 22;

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

    let mut input = String::new();
    for _ in 0..ITERATIONS {
        input.push_str("[[div]]\n");
    }

    crate::preprocess(&mut input);
    let tokens = crate::tokenize(&input);
    let started = Instant::now();
    let (tree, errors) = crate::parse(&tokens, &page_info, &settings).into();

    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(errors.len(), ITERATIONS * 3);
    assert_eq!(tree.elements.len(), 1);
}

#[test]
fn nested_unclosed_blocks_preserve_bibliography_indices() {
    const ITERATIONS: usize = 22;

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

    let mut input = String::new();
    for _ in 0..ITERATIONS {
        input.push_str("[[div]]\n");
    }
    input.push_str("[[bibliography]]\n: foo : bar\n[[/bibliography]]\n");

    crate::preprocess(&mut input);
    let tokens = crate::tokenize(&input);
    let (tree, errors) = crate::parse(&tokens, &page_info, &settings).into();

    assert_eq!(errors.len(), ITERATIONS * 3);
    assert_eq!(tree.elements.len(), 2);
    assert!(matches!(
        tree.elements.last(),
        Some(Element::BibliographyBlock { index: 0, .. })
    ));
    assert_eq!(tree.bibliographies.next_index(), 1);
}

#[test]
#[ignore = "slow test"]
fn large_payload() {
    const ITERATIONS: usize = 500;

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

    let mut input = String::new();

    for _ in 0..ITERATIONS {
        input.push_str("
[[div]]
Lorem ipsum dolor sit amet, consectetur adipiscing elit.
Maecenas sed risus sed ex suscipit ultricies ac quis metus.
Mauris facilisis dui quam, in mollis velit ultrices vitae. Nam pretium accumsan arcu eu ultricies. Sed viverra eleifend elit at blandit. Aenean tempor vitae ipsum vitae lacinia.
Proin eu maximus nulla, id imperdiet libero. Duis convallis posuere arcu vitae sodales. Cras porta ac ligula non porttitor.
Proin et sodales arcu. Class aptent taciti sociosqu ad litora torquent per conubia nostra, per inceptos himenaeos. Mauris eget ante maximus, tincidunt enim nec, dignissim mi.
Quisque tincidunt convallis faucibus. Praesent vel semper dolor, vel tincidunt mi.

In hac habitasse platea dictumst. Vestibulum fermentum libero nec erat porttitor fermentum. Etiam at convallis odio, gravida commodo ipsum. Phasellus consequat nisl vitae ultricies pulvinar. Integer scelerisque eget nisl id fermentum. Pellentesque pretium, enim non molestie rhoncus, dolor diam porta mauris, eu cursus dolor est condimentum nisi. Phasellus tellus est, euismod non accumsan at, congue eget erat.

% ]] ! $ * -- @< _
[[/div]]
        ");
    }

    crate::preprocess(&mut input);
    let tokens = crate::tokenize(&input);
    let (_tree, errors) = crate::parse(&tokens, &page_info, &settings).into();

    assert_eq!(errors.len(), ITERATIONS * 3);
}
