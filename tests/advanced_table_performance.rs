use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("advanced-table-performance"),
        category: Some(Cow::Borrowed("test")),
        site: Cow::Borrowed("coverage"),
        title: Cow::Borrowed("Advanced table performance"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![Cow::Borrowed("test")],
        language: Cow::Borrowed("en"),
    }
}

#[test]
fn repeated_wikidot_header_cell_closers_stay_bounded() {
    const TABLE_COUNT: usize = 64;

    let table = "[[table]]\n[[row]]\n[[hcell]]Heading[[/cell]]\n[[/row]]\n[[/table]]\n";
    let input = table.repeat(TABLE_COUNT);
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let started = Instant::now();

    let tokenization = ftml::tokenize(&input);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;

    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(html.matches("<table").count(), TABLE_COUNT);
    assert_eq!(html.matches("<td").count(), TABLE_COUNT);
}

#[test]
fn quoted_advanced_tables_stay_bounded() {
    const TABLE_COUNT: usize = 16;

    let table = concat!(
        "> [[table]]\n",
        "> [[row]]\n",
        "> [[cell]]Timestamp[[/cell]]\n",
        "> [[cell]]Message[[/cell]]\n",
        "> [[/row]]\n",
        "> [[/table]]\n",
    );
    let input = table.repeat(TABLE_COUNT);
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let started = Instant::now();

    let tokenization = ftml::tokenize(&input);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;

    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(html.matches("<table").count(), TABLE_COUNT);
    assert_eq!(html.matches("<td").count(), TABLE_COUNT * 2);
}

#[test]
fn crossed_and_extra_cell_closer_runs_stay_bounded() {
    const RESIDUAL_COUNT: usize = 2_048;

    let mut input = String::from("[[table]]\n[[row]]\n[[cell]]A[[/hcell]]\n");
    for index in 0..RESIDUAL_COUNT {
        input.push_str(if index % 2 == 0 {
            "[[/cell]]\n"
        } else {
            "[[/hcell]]\n"
        });
    }
    input.push_str("[[/row]]\n[[/table]]");

    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let started = Instant::now();
    let tokenization = ftml::tokenize(&input);
    let (tree, _) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;

    assert!(
        started.elapsed() < Duration::from_secs(5),
        "crossed/extra closer run took {:?}",
        started.elapsed(),
    );
    assert_eq!(html.matches("[[/cell]]").count(), RESIDUAL_COUNT / 2);
    assert_eq!(html.matches("[[/hcell]]").count(), RESIDUAL_COUNT / 2);
    assert_eq!(html.matches("<table>").count(), 1);
    assert_eq!(html.matches("<th>").count(), 1);
}

#[test]
fn large_malformed_advanced_table_body_stays_bounded() {
    const BODY_LEN: usize = 262_144;

    let input = format!(
        "[[table]]\n[[row]]\n[[cell]]{}[[/row]]\n[[/table]]",
        "A".repeat(BODY_LEN),
    );
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let started = Instant::now();
    let tokenization = ftml::tokenize(&input);
    let (tree, _) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;

    assert!(
        started.elapsed() < Duration::from_secs(5),
        "large malformed table took {:?}",
        started.elapsed(),
    );
    assert!(!html.contains("<table>"), "{html}");
    assert!(
        html.contains("[[cell]]"),
        "malformed source was not preserved"
    );
    assert!(html.len() >= BODY_LEN, "large cell body was truncated");
}

#[test]
fn deeply_unclosed_advanced_table_owners_fail_before_recursive_body_parsing() {
    const NESTING: usize = 256;

    let mut input = String::new();
    for _ in 0..NESTING {
        input.push_str("[[table]]\n[[row]]\n[[cell]]\n");
    }

    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let started = Instant::now();
    let tokenization = ftml::tokenize(&input);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;

    assert!(
        started.elapsed() < Duration::from_secs(2),
        "unclosed nested table owners took {:?}",
        started.elapsed(),
    );
    assert!(!errors.is_empty());
    assert!(!html.contains("<table>"), "{html}");
}

#[test]
fn partially_closed_wikidot_advanced_table_owners_reuse_failure_cache() {
    const NESTING: usize = 4;
    const TAIL_LINES: usize = 700;

    let mut source = String::new();
    source.push_str(&"[[table]]\n[[row]]\n[[cell]]\n".repeat(NESTING));
    source.push_str("X\n[[/table]]\n[[/cell]]\n[[/row]]\n[[/table]]\n");
    source.push_str(&"plain text line\n".repeat(TAIL_LINES));

    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let started = Instant::now();
    let tokenization = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;

    assert!(
        started.elapsed() < Duration::from_secs(2),
        "partially closed Wikidot advanced tables took {:?}",
        started.elapsed(),
    );
    assert!(!errors.is_empty());
    assert!(!tree.elements.is_empty());
    assert!(html.contains("plain text line"));
}

#[test]
fn underclosed_wikijump_advanced_table_owners_reuse_failure_cache() {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);

    let semantic_control = "[[table]][[row]][[cell]]X[[/cell]][[/row]]";
    let tokenization = ftml::tokenize(semantic_control);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;
    assert_eq!(html, "<p>[[table]][[row]][[cell]]X[[/cell]][[/row]]</p>");
    assert_eq!(
        errors.first().map(|error| error.kind()),
        Some(ftml::parsing::ParseErrorKind::EndOfInput)
    );

    const NESTING: usize = 24;
    const CLOSED: usize = NESTING / 2;
    let mut source = String::new();
    source.push_str(&"[[table]][[row]][[cell]]".repeat(NESTING));
    source.push('X');
    source.push_str(&"[[/cell]][[/row]][[/table]]".repeat(CLOSED));
    let tokenization = ftml::tokenize(&source);
    let started = Instant::now();
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;

    assert!(
        started.elapsed() < Duration::from_secs(2),
        "underclosed Wikijump advanced tables took {:?}",
        started.elapsed(),
    );
    assert!(!errors.is_empty());
    assert!(!tree.elements.is_empty());
    assert!(html.contains("[[table]]"), "{html}");
}

#[test]
fn many_paragraphs_and_nested_tables_stay_bounded() {
    const PARAGRAPH_COUNT: usize = 2_048;
    const NESTED_INTERVAL: usize = 64;

    let mut input = String::from("[[table]]\n[[row]]\n[[cell]]\n");
    for index in 0..PARAGRAPH_COUNT {
        input.push_str(&format!("P{index}\n\n"));
        if index % NESTED_INTERVAL == NESTED_INTERVAL - 1 {
            input.push_str(concat!(
                "[[table]]\n[[row]]\n[[cell]]I[[/cell]]\n",
                "[[/row]]\n[[/table]]\n\n",
            ));
        }
    }
    input.push_str("[[/cell]]\n[[/row]]\n[[/table]]");

    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let started = Instant::now();
    let tokenization = ftml::tokenize(&input);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;

    assert!(
        started.elapsed() < Duration::from_secs(5),
        "paragraph/table run took {:?}",
        started.elapsed(),
    );
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(html.matches("<p>").count(), PARAGRAPH_COUNT);
    assert_eq!(
        html.matches("<table>").count(),
        1 + PARAGRAPH_COUNT / NESTED_INTERVAL,
    );
}
