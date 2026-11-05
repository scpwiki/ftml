//! Timing regressions for parsed shapes that once grew super-linearly
//! (quadratically) or exponentially in the input size.
//!
//! Every input here is a bounded, deterministic wikitext document; the
//! assertion is only that the full preprocess + tokenize + parse + render
//! pipeline stays inside a generous wall-clock budget. Before the matching
//! parser fix each case took tens of seconds (or did not finish at all) at
//! these sizes, so a regression is unambiguous even on a loaded machine.
//!
//! Budgets are sized for unoptimized `cargo test` builds.

use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::Render;
use ftml::render::html::HtmlRender;
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("super-linear-families"),
        category: None,
        site: Cow::Borrowed("test"),
        title: Cow::Borrowed("Super linear families"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![],
        language: Cow::Borrowed("default"),
    }
}

fn render_timed(input: &str) -> (String, Duration) {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let started = Instant::now();
    let mut text = input.to_owned();
    ftml::preprocess_for_layout(&mut text, Layout::Wikidot);
    let tokens = ftml::tokenize(&text);
    let (tree, _errors) = ftml::parse(&tokens, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;
    (html, started.elapsed())
}

#[test]
fn repeated_unclosed_body_block_openers_stay_bounded() {
    // A body-consuming block with no matching `[[/name]]` after it used to
    // parse the rest of the document as its body and then fail, once per
    // opener: quadratic in the opener count. Unclosed blocks still render
    // literally, exactly as before.
    let units = [
        "[[note]]\nx\n",
        "[[html]]\n",
        "[[math]]\n",
        "[[footnote]]x\n",
        "[[collapsible]]\nx\n",
        "[[div]]\n",
        "[[bibliography]]\n",
        "[[raw]]\n",
        "[[code]]\n",
    ];
    for unit in units {
        let count = 512;
        let input = unit.repeat(count);
        let (html, elapsed) = render_timed(&input);
        assert!(
            elapsed < Duration::from_secs(5),
            "{unit:?} x {count} took {elapsed:?}",
        );
        assert!(
            html.contains(unit.split('\n').next().unwrap()),
            "{unit:?} x {count} no longer renders literally",
        );
    }
}

#[test]
fn unclosed_body_blocks_render_literally_without_closers() {
    for input in [
        "[[note]]\nx",
        "[[html]]\nx",
        "[[math]]\nx",
        "[[footnote]]x",
        "[[collapsible]]\nx",
        "[[div]]\nx",
        "[[bibliography]]\nx",
        "[[raw]]\nx",
        "[[code]]\nx",
        "[[tabview]]\n[[tab a]]x",
        "[[ul]][[li]]x",
        "[[quote]]\nx",
    ] {
        let (html, elapsed) = render_timed(input);
        assert!(
            elapsed < Duration::from_secs(5),
            "{input:?} took {elapsed:?}"
        );
        let head = &input[..input.find("]]").unwrap() + 2];
        assert!(
            html.contains(head),
            "{input:?} should render literally, got {html:?}",
        );
    }
}

#[test]
fn closed_body_blocks_are_unchanged() {
    // The unclosed-body preflight must never fire for a body that really
    // closes: these all formed blocks before the fix.
    let cases = [
        "[[note]]\nx\n[[/note]]",
        "[[div]]\nx\n[[/div]]",
        "[[collapsible show=\"+\" hide=\"-\"]]\nx\n[[/collapsible]]",
        "[[tabview]]\n[[tab a]]x[[/tab]]\n[[/tabview]]",
    ];
    for input in cases {
        let opener = &input[..input.find("]]").unwrap() + 2];
        let (html, elapsed) = render_timed(input);
        assert!(
            elapsed < Duration::from_secs(5),
            "{input:?} took {elapsed:?}"
        );
        assert!(
            !html.contains(opener),
            "{input:?} should have formed a block, got {html:?}",
        );
    }
}

#[test]
fn div_line_scans_stay_linear_in_opener_count() {
    // Each `[[div]]` opener asks whether a `[[/div]]` starts on its physical
    // line. With no line break in the document that scan used to run to the
    // end of the input for every opener.
    for count in [512usize, 1024, 2048] {
        let input = "[[div]][[/div]]".repeat(count);
        let (_, elapsed) = render_timed(&input);
        assert!(
            elapsed < Duration::from_secs(5),
            "{count} inline div pairs took {elapsed:?}",
        );
    }
}

#[test]
fn triple_link_runs_keep_the_local_link_scan_linear() {
    // Every `[` candidate that does not open a `[[` block asked for the end of
    // its physical line by scanning the rest of the document for a newline.
    // A single-line document has none, so that scan ran to end-of-input once
    // per candidate: quadratic in the number of triple links. Typography
    // output (and therefore the rendered HTML) must stay unchanged.
    for count in [4096usize, 8192, 16384] {
        let input = "[[[x]]] ".repeat(count);
        let (html, elapsed) = render_timed(&input);
        assert!(
            elapsed < Duration::from_secs(5),
            "{count} triple links took {elapsed:?}",
        );
        assert!(
            html.matches("href=\"/x\"").count() >= count / 2,
            "{count} triple links no longer render as links",
        );
    }

    // A document that does contain a `[/` still takes the scan (the
    // document-level necessary-condition gate cannot skip it), so the carried
    // line-end cursor has to keep this shape linear too.
    for count in [4096usize, 8192, 16384] {
        let input = "[//a] ".repeat(count);
        let (_, elapsed) = render_timed(&input);
        assert!(
            elapsed < Duration::from_secs(5),
            "{count} local-link candidates took {elapsed:?}",
        );
    }
}

#[test]
fn triple_links_still_render_as_links() {
    let (html, elapsed) = render_timed("[[[x]]] [[[some-page|Label]]]");
    assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
    assert!(
        html.contains("href=\"/x\""),
        "triple link target lost, got {html:?}",
    );
    assert!(
        html.contains(">Label</a>"),
        "triple link label lost, got {html:?}",
    );
}
