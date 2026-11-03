use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};

fn render(source: &str, layout: Layout) -> String {
    let info = PageInfo {
        page: "bold-after-literal-bracket".into(),
        category: None,
        site: "test".into(),
        title: "Bold after literal bracket".into(),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: "en".into(),
    };
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, layout);
    let tokens = ftml::tokenize(&source);
    let (tree, _) = ftml::parse(&tokens, &info, &settings).into();
    HtmlRender.render(&tree, &info, &settings).body
}

#[test]
fn bold_forms_immediately_after_a_literal_left_bracket_wikidot() {
    for (source, expected) in [
        ("[**text**]", "<p>[<strong>text</strong>]</p>"),
        (
            "[**[https://scp-wiki.wikidot.com/art:tiredsn0w Art page]**]",
            "<p>[<strong><a href=\"https://scp-wiki.wikidot.com/art:tiredsn0w\">Art page</a></strong>]</p>",
        ),
        ("a [**b**] c", "<p>a [<strong>b</strong>] c</p>"),
        (
            "[**a** and **b**]",
            "<p>[<strong>a</strong> and <strong>b</strong>]</p>",
        ),
        ("[**//text//**]", "<p>[<strong><em>text</em></strong>]</p>"),
    ] {
        assert_eq!(render(source, Layout::Wikidot), expected, "{source}");
    }
}

#[test]
fn bold_forms_immediately_after_a_literal_left_bracket_wikijump() {
    for (source, expected) in [
        ("[**text**]", "<p>[<strong>text</strong>]</p>"),
        (
            "[**[https://scp-wiki.wikidot.com/art:tiredsn0w Art page]**]",
            "<p>[<strong><a class=\"wj-link wj-link-external\" data-link-type=\"direct\" href=\"https://scp-wiki.wikidot.com/art:tiredsn0w\">Art page</a></strong>]</p>",
        ),
        ("a [**b**] c", "<p>a [<strong>b</strong>] c</p>"),
    ] {
        assert_eq!(render(source, Layout::Wikijump), expected, "{source}");
    }
}

#[test]
fn neighboring_delimiters_after_a_literal_left_bracket_still_form() {
    for (source, expected) in [
        ("[//text//]", "<p>[<em>text</em>]</p>"),
        (
            "[__text__]",
            "<p>[<span style=\"text-decoration: underline;\">text</span>]</p>",
        ),
        (
            "[--text--]",
            "<p>[<span style=\"text-decoration: line-through;\">text</span>]</p>",
        ),
        ("[^^text^^]", "<p>[<sup>text</sup>]</p>"),
        ("[,,text,,]", "<p>[<sub>text</sub>]</p>"),
    ] {
        assert_eq!(render(source, Layout::Wikidot), expected, "{source}");
    }
}

#[test]
fn working_bold_and_link_shapes_around_brackets_are_unchanged() {
    for (source, expected) in [
        (
            "**[https://example.com Text]**",
            "<p><strong><a href=\"https://example.com\">Text</a></strong></p>",
        ),
        (
            "[ **[https://example.com Text]** ]",
            "<p>[ <strong><a href=\"https://example.com\">Text</a></strong> ]</p>",
        ),
        (
            "[*https://example.com Label]",
            "<p><a href=\"https://example.com\" target=\"_blank\">Label</a></p>",
        ),
        ("[**text**", "<p>[<strong>text</strong></p>"),
        ("[ **text** ]", "<p>[ <strong>text</strong> ]</p>"),
    ] {
        assert_eq!(render(source, Layout::Wikidot), expected, "{source}");
    }
    assert_eq!(
        render("[*https://example.com Label]", Layout::Wikijump),
        "<p><a class=\"wj-link wj-link-external\" data-link-type=\"direct\" href=\"https://example.com\" target=\"_blank\" rel=\"noopener noreferrer\">Label</a></p>",
    );
}
