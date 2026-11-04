/*
 * tests/wikidot_footnote_boundaries.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2026 Wikijump Team
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::parsing::ParseError;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("footnote-boundaries"),
        category: None,
        site: Cow::Borrowed("syntax-differential"),
        title: Cow::Borrowed("Footnote boundaries"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn render_wikidot(source: &str) -> (String, Vec<ParseError>) {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    (HtmlRender.render(&tree, &page_info, &settings).body, errors)
}

fn render_for_layout(source: &str, layout: Layout) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, _) = ftml::parse(&tokenization, &page_info, &settings).into();
    HtmlRender.render(&tree, &page_info, &settings).body
}

const REFERENCE_1: &str = concat!(
    r#"<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">"#,
    "1</a></sup>",
);

const REFERENCE_2: &str = concat!(
    r#"<sup class="footnoteref"><a id="footnoteref-2" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-2&#39;)">"#,
    "2</a></sup>",
);

const FOOTER_START: &str =
    r#"<div class="footnotes-footer"><div class="title">Footnotes</div>"#;

const FOOTER_1: &str = concat!(
    r#"<div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">"#,
    "1</a>. v7 body</div>",
);

const FOOTER_2: &str = concat!(
    r#"<div class="footnote-footer" id="footnote-2"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-2&#39;)">"#,
    "2</a>. end</div>",
);

fn single_footnote(body: &str) -> String {
    format!(
        "<p>{REFERENCE_1}</p>{FOOTER_START}<div class=\"footnote-footer\" id=\"footnote-1\"><a href=\"javascript:;\" onclick=\"WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)\">1</a>. {body}</div></div>",
    )
}

#[test]
fn wikidot_footnote_drops_block_owned_trailing_break_after_code() {
    let source = "A[[footnote]]\n[[code]]\nX\n[[/code]]\n[[/footnote]]\n";

    assert_eq!(
        render_wikidot("A[[footnote]]X[[/footnote]]").0,
        format!(
            "<p>A{REFERENCE_1}</p>{FOOTER_START}<div class=\"footnote-footer\" id=\"footnote-1\"><a href=\"javascript:;\" onclick=\"WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)\">1</a>. X</div></div>"
        ),
    );
    assert_eq!(
        render_for_layout("[[code]]\nX\n[[/code]]", Layout::Wikidot),
        "<div class=\"code\"><pre><code>X</code></pre></div>",
    );
    let wikijump = render_for_layout(source, Layout::Wikijump);
    assert!(wikijump.contains("<wj-code "), "{wikijump}");
    assert!(
        wikijump.contains("class=\"wj-footnote-list\""),
        "{wikijump}"
    );

    let (wikidot, errors) = render_wikidot(source);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        wikidot,
        format!(
            "<p>A{REFERENCE_1}</p>{FOOTER_START}<div class=\"footnote-footer\" id=\"footnote-1\"><a href=\"javascript:;\" onclick=\"WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)\">1</a>. <div class=\"code\"><pre><code>X</code></pre></div></div></div>"
        ),
    );
}

#[test]
fn v7_case_variants_remain_literal_and_do_not_consume_numbering() {
    let source = concat!(
        "[[FOOTNOTE]]v7 body[[/FOOTNOTE]]\n\n",
        "[[FOOTNOTEBLOCK]]\n\n",
        "lower[[footnote]]numbered[[/footnote]]",
    );
    let (html, errors) = render_wikidot(source);

    assert!(!errors.is_empty());
    assert_eq!(
        html,
        format!(
            "<p>[[FOOTNOTE]]v7 body[[/FOOTNOTE]]</p><p>[[FOOTNOTEBLOCK]]</p><p>lower{REFERENCE_1}</p>{FOOTER_START}<div class=\"footnote-footer\" id=\"footnote-1\"><a href=\"javascript:;\" onclick=\"WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)\">1</a>. numbered</div></div>",
        ),
    );
}

#[test]
fn v7_footnote_boundary_keeps_later_reference_in_the_prose_paragraph() {
    let (html, errors) = render_wikidot(
        "start-[[footnote]]v7 body[[/footnote]]-middle\n\n[[footnote]]end[[/footnote]]",
    );

    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        html,
        format!(
            "<p>start-{REFERENCE_1}-middle{REFERENCE_2}</p>{FOOTER_START}{FOOTER_1}{FOOTER_2}</div>",
        ),
    );
}

#[test]
fn v7_inline_footnote_block_disappears_without_splitting_prose() {
    let (html, errors) =
        render_wikidot("start-[[footnoteblock]]-middle\n\n[[footnoteblock]]");

    assert!(!errors.is_empty());
    assert_eq!(html, "<p>start—middle</p><p>[[footnoteblock]]</p>");
}

#[test]
fn complete_v7_footnote_family_matches_live_controls() {
    for (property, source, expected) in [
        (
            "footnote-canonical-valid",
            "[[footnote]]v7 body[[/footnote]]",
            single_footnote("v7 body"),
        ),
        (
            "footnote-incomplete-opening",
            "[[footnote",
            "<p>[[footnote</p>".to_owned(),
        ),
        (
            "footnote-case-variation-name",
            "[[FOOTNOTE]]v7 body[[/FOOTNOTE]]",
            "<p>[[FOOTNOTE]]v7 body[[/FOOTNOTE]]</p>".to_owned(),
        ),
        (
            "footnote-whitespace-control",
            "[[footnote]]alpha\tbeta\u{00a0}gamma[[/footnote]]",
            single_footnote("alpha beta\u{00a0}gamma"),
        ),
        (
            "footnote-serialization-source-preservation",
            "[[footnote]]serialized body[[/footnote]]",
            single_footnote("serialized body"),
        ),
        (
            "footnote-text-renderer-relevance",
            "[[footnote]]visible text[[/footnote]]",
            single_footnote("visible text"),
        ),
        (
            "footnote-missing-close",
            "[[footnote]]unterminated body",
            "<p>[[footnote]]unterminated body</p>".to_owned(),
        ),
        (
            "footnote-nesting-same-feature",
            "[[footnote]][[footnote]]nested[[/footnote]][[/footnote]]",
            format!(
                "<p>{REFERENCE_1}[[/footnote]]</p>{FOOTER_START}<div class=\"footnote-footer\" id=\"footnote-1\"><a href=\"javascript:;\" onclick=\"WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)\">1</a>. [[footnote]]nested</div></div>",
            ),
        ),
        (
            "footnote-nesting-different-feature",
            "[[footnote]][[bold]]nested[[/bold]][[/footnote]]",
            single_footnote("[[bold]]nested[[/bold]]"),
        ),
        (
            "footnote-invalid-overlap",
            "[[footnote]]outer [[bold]]inner[[/footnote]][[/bold]]",
            format!(
                "<p>{REFERENCE_1}[[/bold]]</p>{FOOTER_START}<div class=\"footnote-footer\" id=\"footnote-1\"><a href=\"javascript:;\" onclick=\"WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)\">1</a>. outer [[bold]]inner</div></div>",
            ),
        ),
    ] {
        let (html, _) = render_wikidot(source);
        assert_eq!(html, expected, "V7 property {property}");
    }
}

#[test]
fn complete_v7_footnote_block_family_matches_live_controls() {
    for (property, source, expected) in [
        ("footnoteblock-canonical-valid", "[[footnoteblock]]", ""),
        (
            "footnoteblock-incomplete-opening",
            "[[footnoteblock",
            "<p>[[footnoteblock</p>",
        ),
        (
            "footnoteblock-case-variation-name",
            "[[FOOTNOTEBLOCK]]",
            "<p>[[FOOTNOTEBLOCK]]</p>",
        ),
        (
            "footnoteblock-whitespace-control",
            "[[footnoteblock v7ws=\"alpha\tbeta\u{00a0}gamma\"]]",
            "",
        ),
        (
            "footnoteblock-serialization-source-preservation",
            "[[footnoteblock v7ser=\"serialized body\"]]",
            "",
        ),
        (
            "footnoteblock-text-renderer-relevance",
            "[[footnoteblock v7text=\"visible text\"]]",
            "",
        ),
        (
            "footnoteblock-duplicate-arguments",
            "[[footnoteblock v7arg=\"one\" v7arg=\"two\"]]",
            "",
        ),
        (
            "footnoteblock-empty-arguments",
            "[[footnoteblock v7arg=\"\"]]",
            "",
        ),
        (
            "footnoteblock-unknown-argument",
            "[[footnoteblock v7UnknownArgument=\"x\"]]",
            "",
        ),
        (
            "footnoteblock-quote-variation",
            "[[footnoteblock v7arg='single quoted' data-v7=unquoted]]",
            "",
        ),
    ] {
        let (html, _) = render_wikidot(source);
        assert_eq!(html, expected, "V7 property {property}");
    }
}

#[test]
fn missing_duplicate_and_malformed_blocks_recover_for_later_syntax() {
    let (missing, missing_errors) =
        render_wikidot("A[[footnote]]first[[/footnote]]B[[footnote]]second[[/footnote]]");
    assert!(missing_errors.is_empty(), "{missing_errors:#?}");
    assert_eq!(missing.matches("class=\"footnotes-footer\"").count(), 1);
    assert_eq!(missing.matches("class=\"footnote-footer\"").count(), 2);
    assert!(missing.contains(&format!("A{REFERENCE_1}B{REFERENCE_2}")));

    let (duplicate, duplicate_errors) = render_wikidot(concat!(
        "A[[footnote]]first[[/footnote]]\n[[footnoteblock]]\n\n",
        "[[footnoteblock]]\n\nB[[footnote]]second[[/footnote]]",
    ));
    assert!(!duplicate_errors.is_empty());
    assert_eq!(duplicate.matches("class=\"footnotes-footer\"").count(), 1);
    assert_eq!(duplicate.matches("class=\"footnote-footer\"").count(), 2);
    assert!(
        duplicate.contains("<p>[[footnoteblock]]</p>"),
        "{duplicate}"
    );
    assert!(duplicate.contains("id=\"footnoteref-2\""), "{duplicate}");

    let (malformed, malformed_errors) = render_wikidot(
        "[[footnote\n\n[[FOOTNOTE]]unsafe<script>[[/FOOTNOTE]]\n\nLater[[footnote]]safe[[/footnote]]",
    );
    assert!(!malformed_errors.is_empty());
    assert!(malformed.contains("<p>[[footnote</p>"), "{malformed}");
    assert!(malformed.contains("unsafe&lt;script&gt;"), "{malformed}");
    assert!(!malformed.contains("<script>"), "{malformed}");
    assert!(
        malformed.contains("Later<sup class=\"footnoteref\"><a id=\"footnoteref-1\"")
    );
}

#[test]
fn inline_explicit_block_with_multiple_references_stays_outside_paragraphs() {
    let (html, errors) = render_wikidot(concat!(
        "start[[footnote]]one[[/footnote]]-",
        "[[footnoteblock]]",
        "-middle[[footnote]]two[[/footnote]]",
    ));

    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(html.matches("class=\"footnotes-footer\"").count(), 1);
    assert_eq!(html.matches("class=\"footnote-footer\"").count(), 2);
    assert!(
        html.starts_with(&format!("start{REFERENCE_1}-<br>\n{FOOTER_START}")),
        "{html}",
    );
    assert!(
        html.ends_with(&format!("</div><br>\n-middle{REFERENCE_2}")),
        "{html}"
    );
    assert!(
        !html.contains("<p><div class=\"footnotes-footer\""),
        "{html}"
    );
}

#[test]
fn issue_326_is_wikidot_only_and_keeps_wikijump_activation() {
    let source = "[[FOOTNOTE]]body[[/FOOTNOTE]]";
    let wikidot = render_for_layout(source, Layout::Wikidot);
    let wikijump = render_for_layout(source, Layout::Wikijump);

    assert_eq!(wikidot, "<p>[[FOOTNOTE]]body[[/FOOTNOTE]]</p>");
    assert!(wikijump.contains("class=\"wj-footnote-ref\""), "{wikijump}");
}

#[test]
fn rejected_case_variants_stay_bounded_and_leave_numbering_untouched() {
    let rejected = "[[FOOTNOTE]]x[[/FOOTNOTE]]\n\n".repeat(512);
    let source = format!("{rejected}valid[[footnote]]body[[/footnote]]");
    let started = Instant::now();
    let (html, errors) = render_wikidot(&source);
    let elapsed = started.elapsed();

    assert!(!errors.is_empty());
    assert_eq!(html.matches("id=\"footnoteref-1\"").count(), 1);
    assert!(!html.contains("id=\"footnoteref-2\""), "{html}");
    assert!(
        elapsed < Duration::from_secs(2),
        "case rejection took {elapsed:?}",
    );
}
