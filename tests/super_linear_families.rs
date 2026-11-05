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
fn unmatched_inline_openers_do_not_rescan_the_document() {
    // Every `{{` opener asks the delimiter preflight whether a `}}` is
    // reachable, and every `[[$` opener asks the Wikidot math collector for
    // its `$]]` closer. With no closer in the document each scan used to walk
    // to the end of the input once per opener: quadratic in the opener count.
    // `parsing/parser/closer_scan.rs` memoizes the proven absence, so the
    // document is only scanned once. The unit test next to that memo asserts
    // the token-visit count directly; this one guards the whole pipeline.
    for (unit, opener) in [("{", "{"), ("[[$ a ", "[[$")] {
        for count in [4096usize, 8192] {
            let input = unit.repeat(count);
            let (html, elapsed) = render_timed(&input);
            assert!(
                elapsed < Duration::from_secs(5),
                "{unit:?} x {count} took {elapsed:?}",
            );
            assert_eq!(
                html.matches(opener).count(),
                count,
                "{unit:?} x {count} no longer renders literally",
            );
        }
    }
}

#[test]
fn closed_inline_delimiters_and_math_render_unchanged() {
    // Positive controls for the memo: a reachable closer must still take the
    // real walk and still render (shapes pinned by test/monospace* and
    // test/math/inline).
    let cases = [
        ("{{x}}", "<p><tt>x</tt></p>"),
        (
            "before {{   a  **b**  c   }} after",
            "<p>before <tt>a <strong>b</strong> c</tt> after</p>",
        ),
        ("{{x}}}}tail", "<p><tt>x}}</tt>tail</p>"),
        (
            "Apple [[$ a^2 $]] Banana",
            "<p>Apple <span class=\"math-inline\">$a^2$</span> Banana</p>",
        ),
        (
            "[[$ x $]]{{y}}",
            "<p><span class=\"math-inline\">$x$</span><tt>y</tt></p>",
        ),
    ];
    for (input, expected) in cases {
        let (html, elapsed) = render_timed(input);
        assert!(
            elapsed < Duration::from_secs(5),
            "{input:?} took {elapsed:?}"
        );
        assert_eq!(html, expected, "{input:?}");
    }
}
