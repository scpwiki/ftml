use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender, text::TextRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("orphan-tabs"),
        category: None,
        site: Cow::Borrowed("compatibility"),
        title: Cow::Borrowed("Orphan tabs"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![],
        language: Cow::Borrowed("en"),
    }
}

fn render(source: &str) -> (String, String, Vec<ftml::parsing::ParseError>) {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let text = TextRender.render(&tree, &page_info, &settings);
    let html = HtmlRender.render(&tree, &page_info, &settings).body;
    (text, html, errors)
}

#[test]
fn complete_orphan_tabs_keep_the_full_v7_literal_family() {
    for source in [
        "[[tab]]\nv7 body\n[[/tab]]",
        "[[TAB]]\nv7 body\n[[/TAB]]",
        "[[tab]]\nalpha\tbeta\u{a0}gamma\n[[/tab]]",
        "start-[[tab]]\nv7 body\n[[/tab]]-middle\n\n[[tab]]\nend\n[[/tab]]",
        "[[tab]]\nserialized body\n[[/tab]]",
        "[[tab]]\nvisible text\n[[/tab]]",
        "[[tab]]\n[[tab]]\nnested\n[[/tab]]\n[[/tab]]",
        "[[tab]]\n[[bold]]nested[[/bold]]\n[[/tab]]",
    ] {
        let expected = source.replace('\t', " ");
        let (text, html, _) = render(source);

        assert_eq!(text, expected, "{source:?}: {html}");
        assert!(!html.contains("class=\"yui-navset\""), "{source:?}: {html}");
    }
}

#[test]
fn only_direct_tabview_children_activate_as_tabs() {
    let source = concat!(
        "[[tab]]\nbefore\n[[/tab]]\n",
        "[[tabview]]\n",
        "[[tab <unsafe>]]\n",
        "outer\n[[tab Inner]]\nnested\n[[/tab]]\ntail\n",
        "[[tab Broken]\nmalformed\n",
        "[[/tab]]\n",
        "[[tab Second]]second[[/tab]]\n",
        "[[/tabview]]\n",
        "[[tab]]\nafter\n[[/tab]]",
    );
    let (text, html, errors) = render(source);

    assert!(
        !errors.is_empty(),
        "literal fallbacks should remain observable"
    );
    assert!(text.starts_with("[[tab]]\nbefore\n[[/tab]]\n"), "{text:?}");
    assert!(text.ends_with("[[tab]]\nafter\n[[/tab]]"), "{text:?}");
    // Live (saved-page probes muse-probe-tvp-o1/o1b, BRIEF-TVP (17)) ends the
    // outer tab at its FIRST closer: the orphan `[[tab Inner]]…[[/tab]]` pair
    // inside the body is literal text, and the tail, the malformed head, and
    // the stray closer that follow are recovery content, not part of the
    // outer tab. The former nested-tab veto swallowed them into the tab.
    assert!(text.contains("outer\n[[tab Inner]]\nnested"), "{text:?}");
    assert!(
        !text.contains("nested\n[[/tab]]\ntail"),
        "outer tab must not swallow the first closer: {text:?}"
    );
    assert!(
        text.contains("nested\n\ntail\n[[tab Broken]\nmalformed\n[[/tab]]"),
        "{text:?}"
    );
    assert_eq!(html.matches("class=\"yui-navset\"").count(), 1, "{html}");
    assert_eq!(html.matches("class=\"selected\"").count(), 1, "{html}");
    assert!(html.contains("&lt;unsafe&gt;"), "{html}");
    assert!(!html.contains("<unsafe>"), "{html}");
}

#[test]
fn malformed_orphan_tabs_and_closers_do_not_capture_later_tabviews() {
    for orphan in [
        "[[tab]]\nmissing closer",
        "[[tab Broken]\nbody\n[[/tab]]",
        "[[tab]]\nbody\n[[/tabs]]",
        "[[/tab]]",
    ] {
        let source =
            format!("{orphan}\n[[tabview]]\n[[tab Good]]body[[/tab]]\n[[/tabview]]");
        let (text, html, _) = render(&source);

        assert!(text.contains(orphan), "{orphan:?}: {text:?}");
        assert_eq!(html.matches("class=\"yui-navset\"").count(), 1, "{html}");
        assert!(html.contains("<em>Good</em>"), "{html}");
    }
}

#[test]
fn orphan_tab_recovery_is_bounded() {
    let source = "[[tab]]\nbody\n[[/tab]]\n".repeat(512);
    let started = Instant::now();
    let (text, html, _) = render(&source);

    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(text.matches("[[tab]]").count(), 512, "{html}");
    assert_eq!(text.matches("[[/tab]]").count(), 512, "{html}");
}
