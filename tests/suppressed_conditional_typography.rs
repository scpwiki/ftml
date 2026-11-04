use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::Render;
use ftml::render::html::HtmlRender;
use ftml::render::text::TextRender;
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::{Element, SyntaxTree};
use std::borrow::Cow;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("issue-329"),
        category: Some(Cow::Borrowed("test")),
        site: Cow::Borrowed("syntax-differential"),
        title: Cow::Borrowed("Suppressed conditional typography"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![Cow::Borrowed("present")],
        language: Cow::Borrowed("en"),
    }
}

fn parse(source: &str, layout: Layout) -> (usize, SyntaxTree<'static>) {
    parse_as(source, layout, &page_info())
}

fn parse_as(
    source: &str,
    layout: Layout,
    page_info: &PageInfo<'static>,
) -> (usize, SyntaxTree<'static>) {
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut preprocessed = source.to_owned();
    ftml::preprocess_for_layout(&mut preprocessed, layout);
    let tokenization = ftml::tokenize(&preprocessed);
    let (tree, errors) = ftml::parse(&tokenization, page_info, &settings).into();
    assert!(errors.is_empty(), "{source:?}: {errors:#?}");
    (preprocessed.len(), tree.to_owned())
}

fn render(source: &str, layout: Layout) -> (String, String) {
    render_as(source, layout, &page_info())
}

fn render_as(
    source: &str,
    layout: Layout,
    page_info: &PageInfo<'static>,
) -> (String, String) {
    let (_, tree) = parse_as(source, layout, page_info);
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    (
        TextRender.render(&tree, page_info, &settings),
        HtmlRender.render(&tree, page_info, &settings).body,
    )
}

#[test]
fn replay_false_iftags_boundary_applies_wikidot_em_dash() {
    let source = "start-[[iftags]]\nv7 body\n[[/iftags]]-middle";
    let tagless = PageInfo {
        tags: Vec::new(),
        ..page_info()
    };
    assert_eq!(
        render_as(source, Layout::Wikidot, &tagless),
        ("start—middle".to_owned(), "<p>start—middle</p>".to_owned()),
    );
}

#[test]
fn true_and_false_iftags_apply_only_post_resolution_lexical_edits() {
    for (case_id, source, expected) in [
        (
            "false-explicit-dash",
            "left-[[iftags +missing]]\nhidden\n[[/iftags]]-right",
            "left—right",
        ),
        (
            "false-ellipsis",
            "left.[[iftags +missing]]\nhidden\n[[/iftags]]..right",
            "left…right",
        ),
        (
            "false-spaced-ellipsis",
            "left. [[iftags +missing]]\nhidden\n[[/iftags]]. . right",
            "left… right",
        ),
        (
            "false-double-quotes",
            "`[[iftags +missing]]\nhidden\n[[/iftags]]`quoted'[[iftags +missing]]\nhidden\n[[/iftags]]'",
            "“quoted”",
        ),
        (
            "true-empty-double-quotes",
            "`[[iftags -missing]]\n[[/iftags]]`quoted'[[iftags -missing]]\n[[/iftags]]'",
            "`\n`quoted'\n'",
        ),
        (
            "false-angle-quotes",
            "left<[[iftags +missing]]\nhidden\n[[/iftags]]<quoted>[[iftags +missing]]\nhidden\n[[/iftags]]>right",
            "left«quoted»right",
        ),
    ] {
        assert_eq!(
            render(source, Layout::Wikidot).0,
            expected,
            "{case_id}: {source:?}",
        );
    }

    let visible = render(
        "left-[[iftags +present]]\nvisible\n[[/iftags]]-right",
        Layout::Wikidot,
    )
    .0;
    assert_eq!(visible, "left-\nvisible\n-right");
    assert!(!visible.contains('—'));
}

#[test]
fn url_and_escape_owners_are_hard_typography_boundaries_on_both_sides() {
    let (text, html) = render(
        "https://example.com/a-[[iftags +missing]]\nhidden\n[[/iftags]]-tail",
        Layout::Wikidot,
    );
    assert_eq!(text, "https://example.com/a--tail");
    assert_eq!(
        html,
        concat!(
            "<p><a href=\"https://example.com/a-\">",
            "https://example.com/a-</a>-tail</p>",
        ),
    );

    let (text, html) = render(
        "left-[[iftags +missing]]\nhidden\n[[/iftags]]-https://example.com/tail",
        Layout::Wikidot,
    );
    assert_eq!(text, "left—https://example.com/tail");
    assert_eq!(
        html,
        concat!(
            "<p>left—<a href=\"https://example.com/tail\">",
            "https://example.com/tail</a></p>",
        ),
    );

    let (text, html) = render(
        "[https://example.com label-][[iftags +missing]]\nhidden\n[[/iftags]]-tail",
        Layout::Wikidot,
    );
    assert_eq!(text, "label--tail");
    assert!(!html.contains('—'), "{html}");

    let (text, html) = render(
        "@@left-@@[[iftags +missing]]\nhidden\n[[/iftags]]@@-right@@",
        Layout::Wikidot,
    );
    assert_eq!(text, "left--right");
    assert_eq!(html.matches("white-space: pre-wrap").count(), 2, "{html}");
    assert!(!html.contains('—'), "{html}");
}

#[test]
fn malformed_and_fail_closed_conditionals_do_not_create_a_seam() {
    let source = "left-[[iftags +missing]]\nunclosed-middle";
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut preprocessed = source.to_owned();
    ftml::preprocess_for_layout(&mut preprocessed, Layout::Wikidot);
    let tokenization = ftml::tokenize(&preprocessed);
    let (tree, _errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let text = TextRender.render(&tree, &page_info, &settings);
    assert!(text.contains("[[iftags +missing]]"), "{text}");
    assert!(!text.contains('—'), "{text}");

    let malformed_child = concat!(
        "left-[[iftags +missing]]\n",
        "[[code]]\n",
        "[[/iftags]]\n",
        "-must-stay-hidden",
    );
    let (text, _) = render(malformed_child, Layout::Wikidot);
    assert_eq!(text, "left-\n-must-stay-hidden");
    assert!(!text.contains('—'), "{text}");
}

#[test]
fn suppression_rolls_back_hidden_metadata_and_keeps_source_length() {
    let source = concat!(
        "start-[[iftags +missing]]\n",
        "[[html]]<b>hidden</b>[[/html]]\n",
        "[[code]]hidden code[[/code]]\n",
        "[[footnote]]hidden footnote[[/footnote]]\n",
        "[[bibliography]]\n:hidden:hidden reference\n[[/bibliography]]\n",
        "[[/iftags]]-middle",
    );
    let (preprocessed_len, tree) = parse(source, Layout::Wikidot);
    assert_eq!(tree.wikitext_len, preprocessed_len);
    assert!(tree.html_blocks.is_empty());
    assert!(tree.code_blocks.is_empty());
    assert!(tree.footnotes.is_empty());
    assert!(tree.bibliographies.is_empty());

    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    assert_eq!(
        HtmlRender.render(&tree, &page_info, &settings).body,
        "<p>start—middle</p>",
    );
}

#[test]
fn wikijump_layout_keeps_its_dom_while_resolving_both_conditional_variants() {
    assert_eq!(
        render(
            "left-[[iftags +missing]]\nhidden\n[[/iftags]]-right",
            Layout::Wikijump,
        )
        .1,
        "<p>left—right</p>",
    );
    assert_eq!(
        render(
            "left-[[ifcategory other]]\nhidden\n[[/ifcategory]]-right",
            Layout::Wikijump,
        )
        .1,
        "<p>left—right</p>",
    );
    let true_category = render(
        "left-[[ifcategory test]]\nvisible\n[[/ifcategory]]-right",
        Layout::Wikijump,
    )
    .0;
    assert_eq!(true_category, "left-visible-right");
}

#[test]
fn wikijump_kept_body_trailing_break_forms_no_element_before_final_suppression() {
    let source = concat!(
        "[[iftags -missing]]\n",
        "[[module css]]\n",
        "body { color: red; }\n",
        "[[/module]]\n\n",
        "[[/iftags]]\n\n",
        "[[iftags +missing]]\n",
        "hidden\n",
        "[[/iftags]]\n",
    );
    let (_, tree) = parse(source, Layout::Wikijump);

    assert!(matches!(tree.elements.as_slice(), [Element::Style(_)]));
}
