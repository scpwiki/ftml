use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use regex::Regex;
use serde_json::Value;
use std::borrow::Cow;

const FIXTURE: &str =
    include_str!("fixtures/wikidot-image-attachment-default-20261011.json");

fn render(source: &str) -> String {
    let page_info = PageInfo {
        page: Cow::Borrowed(""),
        category: None,
        site: Cow::Borrowed("scp-wiki"),
        title: Cow::Borrowed("probe"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    };
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, _errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    HtmlRender.render(&tree, &page_info, &settings).body
}

fn image_shape(html: &str) -> (Option<String>, String, String) {
    let image =
        Regex::new(r#"(?:<a href="([^"]*)">)?<img src="([^"]*)"[^>]*? alt="([^"]*)""#)
            .expect("valid image regex");
    let captures = image
        .captures(html)
        .unwrap_or_else(|| panic!("rendered HTML has no image: {html}"));
    let host =
        |value: &str| value.replace("scp-wiki.wjfiles.com", "scp-wiki.wdfiles.com");
    (
        captures.get(1).map(|value| host(value.as_str())),
        host(&captures[2]),
        captures[3].to_owned(),
    )
}

#[test]
fn implicit_attachment_display_matches_live_file_type_default() {
    let fixture: Value = serde_json::from_str(FIXTURE).expect("valid fixture JSON");
    let observations = fixture["observations"]
        .as_array()
        .expect("fixture observations");
    assert!(observations.len() >= 40, "fixture lost observations");

    let mut compared = 0;
    for observation in observations {
        let source = observation["source"].as_str().expect("source");
        let expected = (
            observation["anchor_href"].as_str().map(str::to_owned),
            observation["img_src"].as_str().expect("img_src").to_owned(),
            observation["alt"].as_str().expect("alt").to_owned(),
        );
        let html = render(source);
        assert_eq!(image_shape(&html), expected, "{source}\n{html}");
        compared += 1;
    }
    assert_eq!(compared, observations.len());
}

#[test]
fn page_qualified_attachments_follow_the_same_file_type_default() {
    for (source, resized) in [
        ("[[image some-page/a.png]]", false),
        ("[[image some-page/a.JPG]]", false),
        ("[[image some-page/a.gif]]", false),
        ("[[image some-page/a.jpeg]]", true),
        ("[[image some-page/a.webp]]", true),
        ("[[image some-page/a.svg]]", true),
        ("[[image some-page/noext]]", true),
    ] {
        let html = render(source);
        assert_eq!(
            html.contains("/local--resized-images/some-page/"),
            resized,
            "{source}: {html}"
        );
        assert_eq!(html.contains("<a href="), resized, "{source}: {html}");
        if resized {
            assert!(
                html.contains("/local--files/some-page/"),
                "{source}: {html}"
            );
        }
    }

    let html = render(r#"[[image some-page/a.png size="small"]]"#);
    assert!(
        html.contains("/local--resized-images/some-page/a.png/small.jpg"),
        "{html}"
    );
}

#[test]
fn original_display_extensions_are_exact_and_case_insensitive() {
    for (source, resized) in [
        ("[[image a.png]]", false),
        ("[[image a.JPG]]", false),
        ("[[image a.GiF]]", false),
        ("[[image .png]]", false),
        ("[[image a.jpeg.png]]", false),
        ("[[image a.jpeg]]", true),
        ("[[image a.jpe]]", true),
        ("[[image a.apng]]", true),
        ("[[image a.png?v=1]]", true),
        ("[[image a.jpg.jpeg]]", true),
        ("[[image noext]]", true),
    ] {
        let html = render(source);
        assert_eq!(
            html.contains("/local--resized-images//"),
            resized,
            "{source}: {html}"
        );
        assert_eq!(html.contains("<a href="), resized, "{source}: {html}");
    }
}
