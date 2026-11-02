use ftml::data::{PageInfo, ScoreValue, UserInfo};
use ftml::layout::Layout;
use ftml::render::html::HtmlRender;
use ftml::render::{PageExistenceResolver, Render, UserInfoResolver};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;

struct Pages;

impl PageExistenceResolver for Pages {
    fn page_exists(&self, _site: &str, page: &str) -> bool {
        matches!(
            page,
            "scp-series" | "scp-173" | "bigger-than-jesus" | "untitled" | "blank-title"
        )
    }

    fn page_title(&self, _site: &str, page: &str) -> Option<String> {
        match page {
            "scp-series" => Some("SCP Series 1".to_owned()),
            "scp-173" => Some("SCP-173".to_owned()),
            "bigger-than-jesus" => Some("Bigger Than <Jesus>".to_owned()),
            "blank-title" => Some(" ".to_owned()),
            _ => None,
        }
    }
}

impl UserInfoResolver for Pages {
    fn user_info(&self, _name: &str) -> Option<UserInfo<'static>> {
        None
    }
}

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("empty-link-label"),
        category: None,
        site: Cow::Borrowed("scp-wiki"),
        title: Cow::Borrowed("Empty link label"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn render(source: &str, resolve: bool) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, Layout::Wikidot);
    let tokenization = ftml::tokenize(&source);
    let (tree, _errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    if resolve {
        HtmlRender
            .render_with_resolvers(&tree, &page_info, &settings, &Pages, &Pages)
            .body
    } else {
        HtmlRender.render(&tree, &page_info, &settings).body
    }
}

#[test]
fn empty_labels_show_the_existing_page_title() {
    assert_eq!(
        render(
            "[[[scp-series|]]] / [[[scp-173|]]] / [[[Bigger Than Jesus|]]] / [[[missing-page|]]]",
            true,
        ),
        concat!(
            r#"<p><a href="/scp-series">SCP Series 1</a> / "#,
            r#"<a href="/scp-173">SCP-173</a> / "#,
            r#"<a href="/bigger-than-jesus">Bigger Than &lt;Jesus&gt;</a> / "#,
            r#"<a class="newpage" href="/missing-page">missing-page</a></p>"#,
        ),
    );
}

#[test]
fn empty_labels_fall_back_to_the_normalized_target() {
    assert_eq!(
        render("[[[untitled|]]]", true),
        r#"<p><a href="/untitled">untitled</a></p>"#,
    );
    assert_eq!(
        render("[[[Bigger Than Jesus|]]]", false),
        r#"<p><a href="/bigger-than-jesus">bigger-than-jesus</a></p>"#,
    );
}

#[test]
fn empty_labels_fall_back_when_the_page_has_a_blank_title() {
    assert_eq!(
        render("[[[blank-title|]]]", true),
        r#"<p><a href="/blank-title">blank-title</a></p>"#,
    );
}

#[test]
fn authored_and_url_labels_ignore_page_titles() {
    assert_eq!(
        render("[[[scp-173|scp-173]]] [[[scp-173]]]", true),
        r#"<p><a href="/scp-173">scp-173</a> <a href="/scp-173">scp-173</a></p>"#,
    );
    assert_eq!(
        render("[[[https://example.com/a|]]]", true),
        r#"<p><a href="https://example.com/a">https:example-com-a</a></p>"#,
    );
}
