/*
 * includes/test.rs
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

use super::{DebugIncluder, FetchedPage, IncludeRef, Includer, PageRef, include};
use crate::layout::Layout;
use crate::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;

#[test]
fn multiline_self_referencing_argument_uses_later_fallback() {
    let source = concat!(
        "[[include component:backend\n",
        "ifprot={$ifprot}\n",
        "|ifprot=0\n",
        "|nohide=true\n",
        "|nohide=0\n",
        "]]",
    );
    let (include, end) =
        super::parse::parse_include_block(source, 0).expect("valid multiline include");

    assert_eq!(end, source.len());
    assert_eq!(
        include
            .variables()
            .get("ifprot")
            .map(|value| value.trim_end()),
        Some("0"),
    );
    assert_eq!(
        include
            .variables()
            .get("nohide")
            .map(|value| value.trim_end()),
        Some("true"),
    );
}

#[test]
fn self_referential_argument_prefix_is_not_reexpanded() {
    let source = "[[include component:earthworm | class=earthworm--anth24 {$class}]]";
    let (include, end) =
        super::parse::parse_include_block(source, 0).expect("valid include");

    assert_eq!(end, source.len());
    assert_eq!(
        include
            .variables()
            .get("class")
            .map(|value| value.trim_end()),
        Some("earthworm--anth24 {$class}"),
    );
}

#[test]
fn includes() {
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

    macro_rules! test {
        ($text:expr, $expected:expr $(,)?) => {{
            let mut text = str!($text);
            let result = include(
                &mut text,
                &settings,
                DebugIncluder::first_missing(),
                || panic!(),
            );
            let (output, actual) = result.expect("Fetching pages failed");
            let expected = $expected;

            println!("Input:  '{}'", $text);
            println!("Output: '{}'", &output);
            println!("Pages (actual):   {:?}", &actual);
            println!("Pages (expected): {:?}", &expected);
            println!();

            assert_eq!(
                &actual, &expected,
                "Actual pages to include doesn't match expected"
            );
        }};
    }

    test!("", vec![]);
    test!("[[include page]]", vec![PageRef::page_only("page")]);
    test!("[[include page ]]", vec![PageRef::page_only("page")]);
    test!("[[include page ]]", vec![PageRef::page_only("page")]);
    test!("[[ include page ]]", vec![PageRef::page_only("page")]);
    test!("[[include page |]]", vec![PageRef::page_only("page")]);
    test!("[[include page | ]]", vec![PageRef::page_only("page")]);
    test!("[[include page ||]]", vec![PageRef::page_only("page")]);
    test!("[[include page || ]]", vec![PageRef::page_only("page")]);

    test!("[[include PAGE]]", vec![PageRef::page_only("PAGE")]);
    test!("[[include PAGE ]]", vec![PageRef::page_only("PAGE")]);
    test!("[[include PAGE ]]", vec![PageRef::page_only("PAGE")]);
    test!("[[ include PAGE ]]", vec![PageRef::page_only("PAGE")]);

    test!("[[include apple a =1]]", vec![PageRef::page_only("apple")]);
    test!("[[include apple a= 1]]", vec![PageRef::page_only("apple")]);
    test!("[[include apple a = 1]]", vec![PageRef::page_only("apple")]);
    test!(
        "[[include apple a = 1 ]]",
        vec![PageRef::page_only("apple")],
    );
    test!(
        "[[include apple  a = 1 ]]",
        vec![PageRef::page_only("apple")],
    );

    test!("[[include banana a=1]]", vec![PageRef::page_only("banana")]);
    test!(
        "[[include banana a=1|]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana a=1||]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana a=1| |]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana a=1|||]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana a=1| |  |]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana a=1 |]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana a=1 ||]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana a=1 | |]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana |a=1]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana ||a=1]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana | |a=1]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana | a=1]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana || a=1]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana |a=1|]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana ||a=1|]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana | a=1|]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana || a=1|]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana |a=1 |]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana |a=1 ||]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana ||a=1 |]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana ||a=1 ||]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana | a=1 |]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana | a=1 ||]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana || a=1 |]]",
        vec![PageRef::page_only("banana")],
    );
    test!(
        "[[include banana || a=1 ||]]",
        vec![PageRef::page_only("banana")],
    );

    test!(
        "[[include cherry a=1|b=2]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry a=1||b=2]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry a=1|b=2|]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry a=1|b=2||]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry a=1||b=2|]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry a=1||b=2||]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry a=1 |b=2 |]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry a=1 |b=2 ||]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry a=1 ||b=2 |]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry |a=1|b=2]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry |a=1||b=2]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry ||a=1|b=2]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry | a=1| b=2]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry | a=1|| b=2]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry || a=1| b=2]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry |a=1|b=2|]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry |a=1|b=2|]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry |a=1|b=2||]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry |a=1||b=2|]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry ||a=1|b=2|]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry ||a=1||b=2||]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry | a=1| b=2|]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry |a=1 |b=2 |]]",
        vec![PageRef::page_only("cherry")],
    );
    test!(
        "[[include cherry | a=1 | b=2 |]]",
        vec![PageRef::page_only("cherry")],
    );

    test!(
        "[[include durian a=1|b=2|C=**]]",
        vec![PageRef::page_only("durian")],
    );
    test!(
        "[[include durian a=1|b=2|C=**|]]",
        vec![PageRef::page_only("durian")],
    );
    test!(
        "[[include durian a=1 |b=2 |C=** |]]",
        vec![PageRef::page_only("durian")],
    );
    test!(
        "[[include durian |a=1|b=2|C=**]]",
        vec![PageRef::page_only("durian")],
    );
    test!(
        "[[include durian | a=1| b=2| C=**]]",
        vec![PageRef::page_only("durian")],
    );
    test!(
        "[[include durian |a=1|b=2|C=**|]]",
        vec![PageRef::page_only("durian")],
    );
    test!(
        "[[include durian | a=1| b=2| C=**|]]",
        vec![PageRef::page_only("durian")],
    );
    test!(
        "[[include durian |a=1 |b=2 |C=** |]]",
        vec![PageRef::page_only("durian")],
    );
    test!(
        "[[include durian | a=1 | b=2 | C=** ]]",
        vec![PageRef::page_only("durian")],
    );

    test!(
        "[[include component:my-thing]]",
        vec![PageRef::page_only("component:my-thing")],
    );
    test!(
        "[[include :scp-wiki:main]]",
        vec![PageRef::page_and_site("scp-wiki", "main")],
    );
    test!(
        "[[include :scp-wiki:component:my-thing]]",
        vec![PageRef::page_and_site("scp-wiki", "component:my-thing")],
    );
    test!(
        "[[include :scp-wiki:deleted:protected:component:magic]]",
        vec![PageRef::page_and_site(
            "scp-wiki",
            "deleted:protected:component:magic"
        )],
    );

    test!(
        "A\n[[include B]]\nC\n[[include D]]\nE\n[[include F]]\nG",
        vec![
            PageRef::page_only("B"),
            PageRef::page_only("D"),
            PageRef::page_only("F"),
        ],
    );
    test!(
        "[[include my-page]]\n[[include :scp-wiki:theme:black-highlighter-theme]]\n",
        vec![
            PageRef::page_only("my-page"),
            PageRef::page_and_site("scp-wiki", "theme:black-highlighter-theme"),
        ],
    );

    test!("[[include page\n]]", vec![PageRef::page_only("page")]);
    test!(
        "[[include component:multi-line | contents= \nSome content here \nMore stuff]]",
        vec![PageRef::page_only("component:multi-line")],
    );
    test!(
        "[[include component:multi-line argument=x | contents= \nSome content here \nMore stuff \n|]]",
        vec![PageRef::page_only("component:multi-line")],
    );
    test!(
        "[[include component:multi-line | contents= \nSome content here\nMore stuff\n]]",
        vec![PageRef::page_only("component:multi-line")],
    );
    test!(
        "[[include component:multi-line | contents=\nSome content here\nMore stuff\n]]",
        vec![PageRef::page_only("component:multi-line")],
    );
    test!(
        "My wonderful page!\n\n[[include component:info-ayers\n\tlang=en |\n\tpage=scp-xxxx |\n\tauthorPage=http://scpwiki.com/main |\n\tcomments=\n**SCP-XXXX:** My amazing skip \n**Author:** [[*user Username]] \n]]",
        vec![PageRef::page_only("component:info-ayers")],
    );
    test!(
        "My other wonderful page!\n\n[[include component:info-ayers\n\t|lang=en\n\t|page=scp-xxxx\n\t|authorPage=http://scpwiki.com/main\n\t|comments=\n**SCP-XXXX:** My amazing skip \n**Author:** [[*user Username]] \n]]",
        vec![PageRef::page_only("component:info-ayers")],
    );

    test!("other text", vec![]);
    test!("include]]", vec![]);
    test!("[[include", vec![]);
    test!("[[include]]", vec![]);
    test!("[[include ]]", vec![]);
    test!("[[ include]]", vec![]);
    test!("[[include ::page]]", vec![]);

    test!(
        "[[include component:multi-line | contents= \nSome content here \nMore stuff",
        vec![],
    );
}

#[derive(Debug)]
struct PanicIncluder;

impl<'t> Includer<'t> for PanicIncluder {
    type Error = &'static str;

    fn include_pages(
        &mut self,
        _includes: &[IncludeRef<'t>],
    ) -> Result<Vec<FetchedPage<'t>>, Self::Error> {
        panic!("includer should not be called when page syntax is disabled");
    }

    fn no_such_include(
        &mut self,
        _page_ref: &PageRef,
    ) -> Result<Cow<'t, str>, Self::Error> {
        panic!("includer should not be called when page syntax is disabled");
    }
}

#[test]
fn include_is_noop_when_page_syntax_is_disabled() {
    let settings = WikitextSettings::from_mode(WikitextMode::ForumPost, Layout::Wikidot);
    let input = "A [[include component:box name=Alice]] B";

    let (output, pages) = include(
        input,
        &settings,
        PanicIncluder,
        || "invalid include response",
    )
    .expect("include should not fail when page syntax is disabled");

    assert_eq!(output, input);
    assert!(pages.is_empty());
}

#[derive(Debug)]
struct EmptyResponseIncluder;

impl<'t> Includer<'t> for EmptyResponseIncluder {
    type Error = &'static str;

    fn include_pages(
        &mut self,
        _includes: &[IncludeRef<'t>],
    ) -> Result<Vec<FetchedPage<'t>>, Self::Error> {
        Ok(Vec::new())
    }

    fn no_such_include(
        &mut self,
        _page_ref: &PageRef,
    ) -> Result<Cow<'t, str>, Self::Error> {
        panic!("missing page rendering is not reached for response count mismatches");
    }
}

#[test]
fn include_rejects_includer_result_count_mismatch() {
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let result = include(
        "[[include page]]",
        &settings,
        EmptyResponseIncluder,
        || "invalid include response",
    );

    assert_eq!(result.unwrap_err(), "invalid include response");
}

#[derive(Debug)]
struct MismatchedPageIncluder;

impl<'t> Includer<'t> for MismatchedPageIncluder {
    type Error = &'static str;

    fn include_pages(
        &mut self,
        _includes: &[IncludeRef<'t>],
    ) -> Result<Vec<FetchedPage<'t>>, Self::Error> {
        Ok(vec![FetchedPage {
            page_ref: PageRef::page_only("other"),
            content: Some(Cow::Borrowed("ignored")),
        }])
    }

    fn no_such_include(
        &mut self,
        _page_ref: &PageRef,
    ) -> Result<Cow<'t, str>, Self::Error> {
        panic!("missing page rendering is not reached for page ref mismatches");
    }
}

#[test]
fn include_rejects_includer_page_ref_mismatch() {
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let result = include(
        "[[include page]]",
        &settings,
        MismatchedPageIncluder,
        || "invalid include response",
    );

    assert_eq!(result.unwrap_err(), "invalid include response");
}

#[test]
fn include_swallowed_by_multiline_argument_does_not_overlap() {
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let text = "[[include component:info\n\
                |comments=some text\n\
                ----\n\
                [[include :scp-wiki:more-by:someone]]\n\
                trailing text after the block\n";

    let (output, pages) = include(
        text,
        &settings,
        DebugIncluder::first_missing(),
        || unreachable!(),
    )
    .expect("include failed");

    assert_eq!(pages, vec![PageRef::page_only("component:info")]);
    assert!(
        output.contains("trailing text after the block"),
        "text after the block must survive substitution: {output}"
    );
    assert!(
        !output.contains("[[include component:info"),
        "outer include block must be substituted: {output}"
    );
}

#[test]
fn tight_quoted_multiline_include_is_consumed_without_resolving_its_target() {
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let input = concat!(
        "before\n",
        ">[[include :scp-wiki:component:author-label-source start=—\n",
        ">|name=toadking07]]\n",
        "after\n",
    );

    let (output, pages) = include(
        input,
        &settings,
        DebugIncluder::first_missing(),
        || unreachable!(),
    )
    .expect("tight quoted include should be consumed");

    assert!(pages.is_empty());
    assert_eq!(output, "before\n\nafter\n");
}

#[test]
fn spaced_quoted_include_markers_remain_literal_like_wikidot() {
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let nested = "  > > [[include component:box |name=x]]\n";
    let (output, pages) = include(
        nested,
        &settings,
        DebugIncluder::first_missing(),
        || unreachable!(),
    )
    .expect("spaced quoted include should remain literal");

    assert!(pages.is_empty());
    assert_eq!(output, nested);

    let escaped = "> [[include component:box\n|name=unquoted]]\n";
    let (output, pages) = include(
        escaped,
        &settings,
        DebugIncluder::first_missing(),
        || unreachable!(),
    )
    .expect("malformed quoted include should remain literal");
    assert!(pages.is_empty());
    assert_eq!(output, escaped);
}

#[test]
fn spaced_quoted_include_with_crlf_remains_literal() {
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let input = "> [[include component:box |name=x]]\r\n";

    let (output, pages) = include(
        input,
        &settings,
        DebugIncluder::first_missing(),
        || unreachable!(),
    )
    .expect("spaced quoted CRLF include should remain literal");

    assert!(pages.is_empty());
    assert_eq!(output, input);
}

#[test]
fn svg_animation_spaced_self_include_examples_remain_literal() {
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let input = concat!(
        "> [[include :scp-wiki:svg-animation |oneiroi=true|width=100%]]\n",
        "> [[include :scp-wiki:svg-animation |gocLogo=true|width=100%]]\n",
    );

    let (output, pages) = include(
        input,
        &settings,
        DebugIncluder::first_missing(),
        || unreachable!(),
    )
    .expect("spaced self-include examples should remain literal");

    assert!(pages.is_empty());
    assert_eq!(output, input);
}

#[test]
fn quoted_include_scanner_handles_many_one_line_includes_in_bounded_time() {
    const INCLUDE_LINES: usize = 4_096;

    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut input = String::new();
    for _ in 0..INCLUDE_LINES {
        input.push_str(">[[include component:box]]\n");
    }

    let started = std::time::Instant::now();
    let (output, pages) = include(
        &input,
        &settings,
        DebugIncluder::first_missing(),
        || unreachable!(),
    )
    .expect("tight quoted includes should be consumed");

    assert!(pages.is_empty());
    assert_eq!(output, "\n".repeat(INCLUDE_LINES));
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "quoted one-line include scan took {:?}",
        started.elapsed(),
    );
}

#[test]
fn include_tolerates_stray_open_brackets_before_arguments() {
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let text = "[[include :scp-wiki:component:anomaly-class-bar-source\n\
                [[\n\
                \n\
                |item-number= 9507-D\n\
                \n\
                |clearance= 3\n\
                \n\
                ]]\n";

    let (output, pages) = include(
        text,
        &settings,
        DebugIncluder::first_missing(),
        || unreachable!(),
    )
    .expect("include failed");

    assert_eq!(
        pages,
        vec![PageRef::page_and_site(
            "scp-wiki",
            "component:anomaly-class-bar-source",
        )],
    );
    assert!(
        !output.contains("[[include"),
        "the directive must not survive as literal text: {output}"
    );
    for expected in ["item-number", "9507-D", "clearance"] {
        assert!(
            output.contains(expected),
            "argument {expected} must reach the included page: {output}"
        );
    }
    assert!(
        !output.contains('['),
        "the stray bracket must not survive into the output: {output}"
    );
}

#[test]
fn include_without_stray_brackets_is_unchanged() {
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let text = "[[include component:info\nitem-number= PLAIN\n|clearance= 1\n]]\n";

    let (output, pages) = include(
        text,
        &settings,
        DebugIncluder::first_missing(),
        || unreachable!(),
    )
    .expect("include failed");

    assert_eq!(pages, vec![PageRef::page_only("component:info")]);
    for expected in ["item-number", "PLAIN", "clearance"] {
        assert!(
            output.contains(expected),
            "argument {expected} must still be parsed: {output}"
        );
    }
}
