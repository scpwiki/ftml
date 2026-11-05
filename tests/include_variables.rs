use ftml::data::{PageInfo, PageRef, ScoreValue};
use ftml::includes::{FetchedPage, IncludeRef, Includer};
use ftml::layout::Layout;
use ftml::render::Render;
use ftml::render::html::HtmlRender;
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::collections::HashMap;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("include-variables"),
        category: None,
        site: Cow::Borrowed("test"),
        title: Cow::Borrowed("Include Variables"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![],
        language: Cow::Borrowed("default"),
    }
}

fn page_settings() -> WikitextSettings {
    WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot)
}

#[derive(Debug)]
struct StaticIncluder {
    pages: HashMap<&'static str, &'static str>,
}

impl StaticIncluder {
    fn new(pages: impl IntoIterator<Item = (&'static str, &'static str)>) -> Self {
        Self {
            pages: pages.into_iter().collect(),
        }
    }
}

impl<'t> Includer<'t> for StaticIncluder {
    type Error = String;

    fn include_pages(
        &mut self,
        includes: &[IncludeRef<'t>],
    ) -> Result<Vec<FetchedPage<'t>>, Self::Error> {
        Ok(includes
            .iter()
            .map(|include| {
                let page_ref = include.page_ref().clone();
                let key = page_ref.to_string();
                let content = self
                    .pages
                    .get(key.as_str())
                    .map(|content| Cow::Borrowed(*content));

                FetchedPage { page_ref, content }
            })
            .collect())
    }

    fn no_such_include(
        &mut self,
        page_ref: &PageRef,
    ) -> Result<Cow<'t, str>, Self::Error> {
        Err(format!("missing include fixture for {page_ref}"))
    }
}

fn expand(
    source: &str,
    pages: impl IntoIterator<Item = (&'static str, &'static str)>,
) -> String {
    let (expanded, included_pages) = expand_pages(source, pages);

    assert_eq!(included_pages, vec![PageRef::page_only("component:card")]);
    expanded
}

fn expand_pages(
    source: &str,
    pages: impl IntoIterator<Item = (&'static str, &'static str)>,
) -> (String, Vec<PageRef>) {
    ftml::include(source, &page_settings(), StaticIncluder::new(pages), || {
        "invalid include result".to_owned()
    })
    .expect("include expansion should succeed")
}

fn render_html(source: &str) -> String {
    let mut text = source.to_owned();
    let page_info = page_info();
    let settings = page_settings();
    ftml::preprocess_for_layout(&mut text, settings.layout);
    let tokens = ftml::tokenize(&text);
    let result = ftml::parse(&tokens, &page_info, &settings);
    let (tree, errors) = result.into();

    assert!(errors.is_empty(), "{errors:?}");
    HtmlRender.render(&tree, &page_info, &settings).body
}

fn render_text(source: &str) -> String {
    let mut text = source.to_owned();
    let page_info = page_info();
    let settings = page_settings();
    ftml::preprocess_for_layout(&mut text, settings.layout);
    let tokens = ftml::tokenize(&text);
    let result = ftml::parse(&tokens, &page_info, &settings);
    let (tree, errors) = result.into();

    assert!(errors.is_empty(), "{errors:?}");
    ftml::render::text::TextRender.render(&tree, &page_info, &settings)
}

#[test]
fn legacy_include_variables_expand_inside_quoted_div_attributes() {
    let expanded = expand(
        "[[include component:card class=highlight|data_value=42]]",
        [(
            "component:card",
            "[[div class=\"card {$class}\" data-value=\"{$data_value}\"]]\nBody\n[[/div]]",
        )],
    );

    assert!(expanded.contains(r#"class="card highlight""#), "{expanded}");
    assert!(expanded.contains(r#"data-value="42""#), "{expanded}");

    let html = render_html(&expanded);

    assert!(
        html.contains(r#"<div class="card highlight" data-value="42">"#),
        "{html}"
    );
}

#[test]
fn include_argument_variable_pass_through_uses_fallback_value() {
    let expanded = expand(
        "[[include component:card class={$class}|class=default]]",
        [("component:card", "{$class}")],
    );

    assert_eq!(expanded, "default");
}

#[test]
fn include_argument_first_concrete_value_wins_over_fallback() {
    let expanded = expand(
        "[[include component:card class=selected|class=default]]",
        [("component:card", "{$class}")],
    );

    assert_eq!(expanded, "selected");
}

#[test]
fn snippets_redirect_dynamic_fallback_arguments_select_the_default_backend() {
    let (expanded, included_pages) = expand_pages(
        concat!(
            "[[include :snippets:redirect-backend\n",
            "|mode={$mode_{$mode}}\n",
            "|mode_advanced=advanced | mode=default\n",
            "|mode_{$mode}={$mode_default} | mode_default=default\n",
            "|url=http://example.test/target\n",
            "]]",
        ),
        [(":snippets:redirect-backend", "MODE={$mode}|URL={$url}")],
    );

    assert_eq!(
        included_pages,
        vec![PageRef::page_and_site("snippets", "redirect-backend")],
    );
    assert_eq!(expanded, "MODE=default|URL=http://example.test/target",);
}

#[test]
fn scp_6823_empty_include_argument_leaves_target_variable_unresolved() {
    let expanded = expand(
        "[[include component:card\n|secondary-class= \n|other=value\n]]",
        [("component:card", "{$secondary-class}")],
    );

    assert_eq!(expanded, "{$secondary-class}");
    assert_eq!(render_text(&expanded), "{$secondary-class}");
}

#[test]
fn inert_and_empty_include_segments_preserve_live_variable_state() {
    let component = [("component:card", "X={$x}\nN={$n}\nZ={$z}")];

    for (arguments, expected) in [
        ("|FLAG|x=1|n=2|z=3", "X=1\nN=2\nZ=3"),
        ("|x=|n=2|z=3", "X={$x}\nN=2\nZ=3"),
        ("|x=|x=2|n=3|z=4", "X=2\nN=3\nZ=4"),
        ("|x|x=2|n=3|z=4", "X=2\nN=3\nZ=4"),
        ("|日本語=1|x=2|n=3|z=4", "X=2\nN=3\nZ=4"),
        ("|x=\"A|B\"|n=2|z=3", "X=\"A\nN=2\nZ=3"),
    ] {
        let source = format!("[[include component:card {arguments}]]");
        let expanded = expand(&source, component);
        assert_eq!(expanded, expected, "{arguments}");
    }
}

#[test]
fn nested_include_passes_recover_inert_arguments_locally() {
    let (outer, outer_pages) = expand_pages(
        "[[include component:outer]]",
        [(
            "component:outer",
            "[[include component:card |FLAG|x=1|n=2|z=3]]",
        )],
    );
    assert_eq!(outer_pages, vec![PageRef::page_only("component:outer")]);

    let (expanded, inner_pages) =
        expand_pages(&outer, [("component:card", "X={$x}\nN={$n}\nZ={$z}")]);
    assert_eq!(inner_pages, vec![PageRef::page_only("component:card")]);
    assert_eq!(expanded, "X=1\nN=2\nZ=3");
}

#[test]
fn include_expansion_separates_caller_and_target_paragraphs_like_wikidot() {
    let expanded = expand(
        "CALLER_BEFORE\n[[include component:card]]\nCALLER_AFTER",
        [("component:card", "TARGET_FIRST\nTARGET_SECOND")],
    );

    assert_eq!(
        expanded,
        "CALLER_BEFORE\n\nTARGET_FIRST\nTARGET_SECOND\n\nCALLER_AFTER",
    );

    let html = render_html(&expanded);
    assert!(html.contains("<p>CALLER_BEFORE</p>"), "{html}");
    assert!(
        html.contains("<p>TARGET_FIRST<br>\nTARGET_SECOND</p>"),
        "{html}",
    );
    assert!(html.contains("<p>CALLER_AFTER</p>"), "{html}");
}

#[test]
fn include_expansion_adds_only_the_needed_document_edge_separators() {
    for (source, expected) in [
        (
            "[[include component:card]]\nCALLER_AFTER",
            "TARGET\n\nCALLER_AFTER",
        ),
        (
            "CALLER_BEFORE\n[[include component:card]]",
            "CALLER_BEFORE\n\nTARGET",
        ),
        (
            "[[include component:card]]\r\nCALLER_AFTER",
            "TARGET\r\n\r\nCALLER_AFTER",
        ),
        (
            "CALLER_BEFORE\r\n[[include component:card]]",
            "CALLER_BEFORE\r\n\r\nTARGET",
        ),
        ("[[include component:card]]", "TARGET"),
    ] {
        let expanded = expand(source, [("component:card", "TARGET")]);
        assert_eq!(expanded, expected, "{source:?}");
    }
}

#[test]
fn empty_include_target_keeps_only_the_callers_existing_line_endings() {
    let expanded = expand(
        "CALLER_BEFORE\n[[include component:card]]\nCALLER_AFTER",
        [("component:card", "")],
    );

    assert_eq!(expanded, "CALLER_BEFORE\n\nCALLER_AFTER");
}

#[test]
fn adjacent_include_expansions_remain_separate_blocks() {
    let (expanded, included_pages) = expand_pages(
        "CALLER_BEFORE\n[[include component:card]]\n[[include component:other]]\nCALLER_AFTER",
        [
            ("component:card", "TARGET_ONE"),
            ("component:other", "TARGET_TWO_FIRST\nTARGET_TWO_SECOND"),
        ],
    );

    assert_eq!(
        included_pages,
        vec![
            PageRef::page_only("component:card"),
            PageRef::page_only("component:other"),
        ],
    );
    let html = render_html(&expanded);
    for paragraph in [
        "<p>CALLER_BEFORE</p>",
        "<p>TARGET_ONE</p>",
        "<p>TARGET_TWO_FIRST<br>\nTARGET_TWO_SECOND</p>",
        "<p>CALLER_AFTER</p>",
    ] {
        assert!(html.contains(paragraph), "missing {paragraph}: {html}");
    }
}

#[test]
fn included_and_caller_quote_runs_remain_siblings() {
    let expanded = expand(
        "CALLER_BEFORE\n[[include component:card]]\n> CALLER_QUOTE\nCALLER_AFTER",
        [("component:card", "> TARGET_QUOTE")],
    );
    let html = render_html(&expanded);

    assert_eq!(html.matches("<blockquote>").count(), 2, "{html}");
    assert_eq!(html.matches("</blockquote>").count(), 2, "{html}");
    assert!(
        html.contains("<blockquote><p>TARGET_QUOTE</p></blockquote>"),
        "{html}",
    );
    assert!(
        html.contains("<blockquote><p>CALLER_QUOTE</p></blockquote>"),
        "{html}",
    );
}

#[test]
fn comment_branch_include_variables_do_not_truncate_following_body() {
    let source = r#"before
[[include component:branch selected=--]]]
middle
[[include component:branch selected=--]]]
after
"#;

    let (expanded, included_pages) = ftml::include(
        source,
        &page_settings(),
        StaticIncluder::new([(
            "component:branch",
            r#"[!-- {$selected}
branch body
[!----]
"#,
        )]),
        || "invalid include result".to_owned(),
    )
    .expect("include expansion should succeed");

    assert_eq!(
        included_pages,
        vec![
            PageRef::page_only("component:branch"),
            PageRef::page_only("component:branch"),
        ],
    );
    assert_eq!(expanded.matches("branch body").count(), 2, "{expanded}");

    let rendered = render_text(&expanded);

    assert!(rendered.contains("before"), "{rendered}");
    assert_eq!(rendered.matches("branch body").count(), 2, "{rendered}");
    assert!(rendered.contains("middle"), "{rendered}");
    assert!(rendered.contains("after"), "{rendered}");
}

#[test]
fn comment_branch_include_after_false_iftags_does_not_truncate_following_body() {
    let source = r#"before
[[include component:card
|inc-selected= --]]]
after
"#;

    let expanded = expand(
        source,
        [(
            "component:card",
            r#"[[iftags +theme]]
component documentation
> {{@@[[include component:card |inc-selected= --@@]]]}}
[[/iftags]]

[!-- {$inc-selected}
selected branch
[!----]

[!-- {$inc-other}
hidden branch
[!----]
"#,
        )],
    );

    let rendered = render_text(&expanded);

    assert!(rendered.contains("before"), "{rendered}");
    assert!(!rendered.contains("component documentation"), "{rendered}");
    assert!(rendered.contains("selected branch"), "{rendered}");
    assert!(!rendered.contains("hidden branch"), "{rendered}");
    assert!(rendered.contains("after"), "{rendered}");
}

#[test]
fn comment_branch_include_after_false_iftags_with_unclosed_block_does_not_truncate_following_body()
 {
    let source = r#"before
[[include component:theme]]
[[include component:card
|inc-selected= --]]]
after
"#;

    let (expanded, included_pages) = ftml::include(
        source,
        &page_settings(),
        StaticIncluder::new([
            (
                "component:theme",
                r#"[[iftags +theme]]
[[span]]
hidden documentation block
[[/iftags]]
"#,
            ),
            (
                "component:card",
                r#"
[!-- {$inc-selected}
selected branch
[!----]

[!-- {$inc-other}
hidden branch
[!----]
"#,
            ),
        ]),
        || "invalid include result".to_owned(),
    )
    .expect("include expansion should succeed");

    assert_eq!(
        included_pages,
        vec![
            PageRef::page_only("component:theme"),
            PageRef::page_only("component:card"),
        ],
    );

    let rendered = render_text(&expanded);

    assert!(rendered.contains("before"), "{rendered}");
    assert!(
        !rendered.contains("hidden documentation block"),
        "{rendered}"
    );
    assert!(rendered.contains("selected branch"), "{rendered}");
    assert!(!rendered.contains("hidden branch"), "{rendered}");
    assert!(rendered.contains("after"), "{rendered}");
}

#[test]
fn rendered_div_and_span_apply_their_element_specific_attribute_contracts() {
    let html = render_html(
        r#"[[div class="card" id="panel" style="color: red" data-value="42" aria-label="Panel" onclick="alert(1)" data-="bad"]]
[[span class="badge" id="tag" style="font-weight: bold" data-kind="status" aria-live="polite" onmouseover="alert(1)" aria-="bad"]]Open[[/span]]
[[/div]]"#,
    );

    for expected in [
        r#"class="card""#,
        r#"id="u-panel""#,
        r#"style="color: red""#,
        r#"data-value="42""#,
        r#"class="badge""#,
        r#"id="u-tag""#,
        r#"style="font-weight: bold""#,
        r#"data-kind="status""#,
        r#"data-="bad""#,
    ] {
        assert!(html.contains(expected), "missing {expected} in {html}");
    }

    for forbidden in [
        "onclick",
        "onmouseover",
        r#"aria-="bad""#,
        r#"aria-label="Panel""#,
        r#"aria-live="polite""#,
        "alert(1)",
    ] {
        assert!(!html.contains(forbidden), "found {forbidden} in {html}");
    }
}
