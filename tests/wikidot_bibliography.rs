use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::SyntaxTree;
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("wikidot-bibliography"),
        category: None,
        site: Cow::Borrowed("compatibility"),
        title: Cow::Borrowed("Wikidot bibliography"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![],
        language: Cow::Borrowed("en"),
    }
}

fn parse(source: &str) -> SyntaxTree<'static> {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, _) = ftml::parse(&tokenization, &page_info, &settings).into();
    tree.to_owned()
}

fn render(source: &str) -> (String, SyntaxTree<'static>) {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, _) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;
    (html, tree.to_owned())
}

#[test]
fn complete_v7_bibliography_family_matches_live_page_preview() {
    let cases = [
        (
            "bibliography-canonical-valid",
            "[[bibliography]]\nv7 body\n[[/bibliography]]",
            "<div class=\"bibitems\"><div class=\"title\">Bibliography</div>\nv7 body</div>",
            1,
        ),
        (
            "bibliography-incomplete-opening",
            "[[bibliography",
            "<p>[[bibliography</p>",
            0,
        ),
        (
            "bibliography-case-variation-name",
            "[[BIBLIOGRAPHY]]\nv7 body\n[[/BIBLIOGRAPHY]]",
            "<p>[[BIBLIOGRAPHY]]<br>\nv7 body<br>\n[[/BIBLIOGRAPHY]]</p>",
            0,
        ),
        (
            "bibliography-whitespace-control",
            "[[bibliography]]\nalpha\tbeta\u{00a0}gamma\n[[/bibliography]]",
            "<div class=\"bibitems\"><div class=\"title\">Bibliography</div>\nalpha beta\u{00a0}gamma</div>",
            1,
        ),
        (
            "bibliography-boundary",
            concat!(
                "start-[[bibliography]]\nv7 body\n[[/bibliography]]-middle\n\n",
                "[[bibliography]]\nend\n[[/bibliography]]",
            ),
            concat!(
                "<p>start-[[bibliography]]<br>\nv7 body<br>\n",
                "[[/bibliography]]-middle</p>",
                "<div class=\"bibitems\"><div class=\"title\">",
                "Bibliography</div>\nend</div>",
            ),
            1,
        ),
        (
            "bibliography-serialization-source-preservation",
            "[[bibliography]]\nserialized body\n[[/bibliography]]",
            "<div class=\"bibitems\"><div class=\"title\">Bibliography</div>\nserialized body</div>",
            1,
        ),
        (
            "bibliography-text-renderer-relevance",
            "[[bibliography]]\nvisible text\n[[/bibliography]]",
            "<div class=\"bibitems\"><div class=\"title\">Bibliography</div>\nvisible text</div>",
            1,
        ),
        (
            "bibliography-missing-close",
            "[[bibliography]]\nunterminated body",
            "<p>[[bibliography]]<br>\nunterminated body</p>",
            0,
        ),
        (
            "bibliography-nesting-same-feature",
            concat!(
                "[[bibliography]]\n[[bibliography]]\nnested\n",
                "[[/bibliography]]\n[[/bibliography]]",
            ),
            concat!(
                "<div class=\"bibitems\"><div class=\"title\">",
                "Bibliography</div>\n[[bibliography]] nested</div>",
                "<p>[[/bibliography]]</p>",
            ),
            1,
        ),
        (
            "bibliography-nesting-different-feature",
            "[[bibliography]]\n[[bold]]nested[[/bold]]\n[[/bibliography]]",
            concat!(
                "<div class=\"bibitems\"><div class=\"title\">",
                "Bibliography</div>\n[[bold]]nested[[/bold]]</div>",
            ),
            1,
        ),
        (
            "bibliography-invalid-overlap",
            "[[bibliography]]\nouter [[bold]]inner\n[[/bibliography]][[/bold]]",
            concat!(
                "<p>[[bibliography]]<br>\nouter [[bold]]inner<br>\n",
                "[[/bibliography]][[/bold]]</p>",
            ),
            0,
        ),
        (
            "bibliography-duplicate-arguments",
            "[[bibliography v7arg=\"one\" v7arg=\"two\"]]\nv7 body\n[[/bibliography]]",
            "<div class=\"bibitems\"><div class=\"title\">Bibliography</div>\nv7 body</div>",
            1,
        ),
        (
            "bibliography-empty-arguments",
            "[[bibliography v7arg=\"\"]]\nv7 body\n[[/bibliography]]",
            "<div class=\"bibitems\"><div class=\"title\">Bibliography</div>\nv7 body</div>",
            1,
        ),
        (
            "bibliography-unknown-argument",
            "[[bibliography v7UnknownArgument=\"x\"]]\nv7 body\n[[/bibliography]]",
            "<div class=\"bibitems\"><div class=\"title\">Bibliography</div>\nv7 body</div>",
            1,
        ),
        (
            "bibliography-quote-variation",
            "[[bibliography v7arg='single quoted' data-v7=unquoted]]\nv7 body\n[[/bibliography]]",
            "<div class=\"bibitems\"><div class=\"title\">Bibliography</div>\nv7 body</div>",
            1,
        ),
    ];

    let mut failures = Vec::new();
    for (property, source, expected_html, expected_bibliographies) in cases {
        let (html, tree) = render(source);
        if html != expected_html {
            failures.push(format!(
                "{property} HTML:\n  actual: {html:?}\nexpected: {expected_html:?}",
            ));
        }
        if tree.bibliographies.next_index() != expected_bibliographies {
            failures.push(format!(
                "{property} bibliographies:\n  actual: {}\nexpected: {expected_bibliographies}\ntree: {tree:#?}",
                tree.bibliographies.next_index(),
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn crossed_definition_bibliography_commits_metadata_and_numbering() {
    let source = concat!(
        "[[bibliography]]\n: leaked : outer [[bold]]inner\n",
        "[[/bibliography]][[/bold]]\n\n",
        "[[bibliography title=\"<Works & Sources>\"]]\n",
        ": alpha : A & B\n",
        ": beta : B & C\n",
        "[[/bibliography]]\n",
        "((bibcite alpha))",
    );
    let (html, tree) = render(source);

    assert_eq!(tree.bibliographies.next_index(), 1, "{tree:#?}");
    assert_eq!(tree.bibliographies.get_reference("leaked").unwrap().0, 1);
    assert_eq!(tree.bibliographies.get_reference("alpha").unwrap().0, 2);
    assert_eq!(tree.bibliographies.get_reference("beta").unwrap().0, 3);
    assert_eq!(html.matches("class=\"bibitems\"").count(), 1, "{html}");
    assert!(html.contains(
        "<div class=\"bibitem\" id=\"bibitem-1\">1. outer [[bold]]inner</div>"
    ));
    assert!(html.contains("<div class=\"bibitem\" id=\"bibitem-2\">2. A &amp; B</div>"));
    assert!(html.contains("<div class=\"bibitem\" id=\"bibitem-3\">3. B &amp; C</div>"));
    assert!(html.contains("class=\"bibcite\""), "{html}");
    assert!(
        html.contains("scrollToReference(&#39;bibitem-2&#39;)"),
        "{html}"
    );
}

#[test]
fn dense_valid_crossed_and_unclosed_candidates_stay_bounded() {
    const ACTIVE_COUNT: usize = 1_024;
    let mut dense = String::new();
    for index in 0..ACTIVE_COUNT {
        dense.push_str(&format!(
            "[[bibliography]]\n: item-{index} : value\n[[/bibliography]]\n",
        ));
        dense.push_str(
            "[[bibliography]]\nouter [[bold]]inner\n[[/bibliography]][[/bold]]\n",
        );
    }

    let started = Instant::now();
    let tree = parse(&dense);
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "dense parse took {elapsed:?}"
    );
    assert_eq!(tree.bibliographies.next_index(), 1, "{tree:#?}");

    let mut unclosed = String::new();
    for index in 0..512 {
        unclosed.push_str(&format!("[[bibliography]]\nunterminated-{index}\n"));
    }
    let started = Instant::now();
    let tree = parse(&unclosed);
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "unclosed parse took {elapsed:?}",
    );
    assert!(tree.bibliographies.is_empty(), "{tree:#?}");
}
