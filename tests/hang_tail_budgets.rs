use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("hang-tail"),
        category: Some(Cow::Borrowed("_default")),
        site: Cow::Borrowed("coverage"),
        title: Cow::Borrowed("Hang tail"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![Cow::Borrowed("test")],
        language: Cow::Borrowed("en"),
    }
}

#[test]
fn bracket_run_lines_do_not_reprobe_every_bracket_pair() {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Draft, Layout::Wikijump);
    let line = "[".repeat(1100);
    let source = vec![line.as_str(); 48].join("\n");
    let tokenization = ftml::tokenize(&source);
    let started = Instant::now();
    let _ = ftml::parse(&tokenization, &page_info, &settings);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "bracket-run lines took {:?}",
        started.elapsed(),
    );
}

#[test]
fn unclosed_tab_lines_with_bib_openers_stay_bounded() {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::PageNav, Layout::Wikijump);
    let source = "[[tab x]]--[[bib[[/row] x\n".repeat(700);
    let tokenization = ftml::tokenize(&source);
    let started = Instant::now();
    let _ = ftml::parse(&tokenization, &page_info, &settings);
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "unclosed tab/bib lines took {:?}",
        started.elapsed(),
    );
}
