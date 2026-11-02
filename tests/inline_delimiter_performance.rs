use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::Element;
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("inline-delimiter-performance"),
        category: Some(Cow::Borrowed("test")),
        site: Cow::Borrowed("coverage"),
        title: Cow::Borrowed("Inline delimiter performance"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![Cow::Borrowed("test")],
        language: Cow::Borrowed("en"),
    }
}

#[test]
fn padded_inline_openers_inside_list_items_stay_literal_in_bounded_time() {
    const ROW_COUNT: usize = 128;

    for marker in ["**", "//", "__", "^^", ",,"] {
        let row = format!("# [[size 0%]]{marker} [[/size]]\n");
        let input = row.repeat(ROW_COUNT);
        let page_info = page_info();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        let started = Instant::now();
        let mut input = input;
        ftml::preprocess_for_layout(&mut input, settings.layout);
        let tokenization = ftml::tokenize(&input);
        let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        let elapsed = started.elapsed();

        assert!(elapsed < Duration::from_secs(5), "{marker:?}: {elapsed:?}");
        assert_eq!(tree.elements.len(), 1, "{marker:?}: {:#?}", tree.elements);
        let Element::List { items, .. } = &tree.elements[0] else {
            panic!("{marker:?}: expected one list, got {:#?}", tree.elements);
        };
        assert_eq!(items.len(), ROW_COUNT, "{marker:?}");
        assert_eq!(html.matches(marker).count(), ROW_COUNT, "{marker:?}");
        assert!(errors.is_empty(), "{marker:?}: {errors:#?}");
    }
}

#[test]
fn repeated_underline_spacer_run_stays_linear_and_invisible() {
    const MARKER_COUNT: usize = 16_384;

    let mut input = format!("{} ", "__".repeat(MARKER_COUNT));
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let started = Instant::now();

    ftml::preprocess_for_layout(&mut input, settings.layout);
    let tokenization = ftml::tokenize(&input);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;

    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(html.is_empty(), "{html}");
}

#[test]
fn quoted_multiline_inline_delimiter_pairs_stay_bounded() {
    const PAIR_COUNT: usize = 64;

    for (marker, opening) in [
        ("**", "<strong>"),
        ("//", "<em>"),
        ("__", "<span style=\"text-decoration: underline;\">"),
        ("^^", "<sup>"),
        (",,", "<sub>"),
    ] {
        let mut input = format!("> {marker}\n> quoted text{marker}\n").repeat(PAIR_COUNT);
        let page_info = page_info();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let started = Instant::now();

        ftml::preprocess_for_layout(&mut input, settings.layout);
        let tokenization = ftml::tokenize(&input);
        let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(started.elapsed() < Duration::from_secs(5), "{marker}");
        assert!(errors.is_empty(), "{marker}: {errors:#?}");
        assert_eq!(html.matches(opening).count(), PAIR_COUNT, "{marker}");
        assert_eq!(html.matches("quoted text").count(), PAIR_COUNT);
    }
}

#[test]
fn email_after_block_closer_does_not_hide_the_closer() {
    const ROW_COUNT: usize = 128;

    let row = concat!(
        "[[div class=\"contact\"]]\n",
        "[[span style=\"color: red;\"]]name[[/span]]19@scip.net\n",
        "[[/div]]\n",
    );
    let input = row.repeat(ROW_COUNT);
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let started = Instant::now();

    let tokenization = ftml::tokenize(&input);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;

    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        html.matches("<span style=\"color: red;\">name</span>")
            .count(),
        ROW_COUNT
    );
    assert_eq!(html.matches("ten.pics|91#ten.pics|91").count(), ROW_COUNT,);
}

#[test]
fn repeated_wikidot_size_scopes_crossing_structural_blocks_stay_bounded() {
    const ROW_COUNT: usize = 128;

    let row = concat!(
        "[[size larger]]\n",
        "[[div_ class=\"colmod-block\"]]\n",
        "[[ul]][[li]][[ul]]header[[/ul]][[/li]][[/ul]]\n",
        "[[div class=\"foldable-list-container\"]]link[[/div]]\n",
        "[[/div]][[div class=\"colmod-content\"]]\n",
        "[[/size]]\n",
        "body\n",
        "[[/div]]\n",
    );
    let input = row.repeat(ROW_COUNT);
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let started = Instant::now();

    let tokenization = ftml::tokenize(&input);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;

    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(html.matches("font-size:larger;").count(), ROW_COUNT * 3);
    assert_eq!(html.matches("colmod-content").count(), ROW_COUNT);
    assert_eq!(html.matches("body").count(), ROW_COUNT);
    assert!(!html.contains("font-size: larger;\">body"));
}

#[test]
fn crossed_color_monospace_prompts_in_blocks_stay_bounded() {
    const PROMPT_COUNT: usize = 64;

    let mut input =
        "[[>]]\n##63ff54|{{@@ >  @@boot##}}\n[[/>]]\n{{TEXT}}\n".repeat(PROMPT_COUNT);
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let started = Instant::now();

    ftml::preprocess_for_layout(&mut input, settings.layout);
    let tokenization = ftml::tokenize(&input);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;

    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(
        html.matches(concat!(
            r#"<div style="text-align: right;"><p><span style="color: #63ff54"><tt>"#,
            r#"<span style="white-space: pre-wrap;"> &gt;  </span>boot</tt></span></p></div>"#,
        ))
        .count(),
        PROMPT_COUNT,
        "{html}",
    );
    assert_eq!(
        html.matches("<tt>TEXT</tt>").count(),
        PROMPT_COUNT,
        "{html}"
    );
    assert!(
        errors
            .iter()
            .all(|error| error.kind()
                != ftml::parsing::ParseErrorKind::RecursionDepthExceeded),
        "{errors:#?}",
    );
}

#[test]
fn long_block_bodies_without_closers_stay_linear() {
    const LINE_COUNT: usize = 8_000;

    let mut input = format!(
        "[[div]]\n{}[[/div]]\n",
        "plain words on a line\n".repeat(LINE_COUNT),
    );
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let started = Instant::now();

    ftml::preprocess_for_layout(&mut input, settings.layout);
    let tokenization = ftml::tokenize(&input);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;
    let elapsed = started.elapsed();

    assert!(errors.is_empty(), "{errors:#?}");
    assert!(
        html.starts_with("<div><p>plain words on a line<br>"),
        "{html}"
    );
    assert_eq!(html.matches("plain words on a line").count(), LINE_COUNT);
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
}
