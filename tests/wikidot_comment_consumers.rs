use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::parsing::ParseError;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::SyntaxTree;
use std::borrow::Cow;
use std::sync::mpsc;
use std::time::Duration;

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("comment-consumers"),
        category: None,
        site: Cow::Borrowed("sandbox-for-codex"),
        title: Cow::Borrowed("Comment consumers"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn parse(source: &str, layout: Layout) -> (SyntaxTree<'static>, Vec<ParseError>) {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    (tree.to_owned(), errors)
}

fn render(source: &str, layout: Layout) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let (tree, errors) = parse(source, layout);
    assert!(errors.is_empty(), "{source:?}: {errors:#?}");
    HtmlRender.render(&tree, &page_info, &settings).body
}

fn render_allow_errors(source: &str, layout: Layout) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let (tree, _) = parse(source, layout);
    HtmlRender.render(&tree, &page_info, &settings).body
}

fn render_wikidot(source: &str) -> String {
    render(source, Layout::Wikidot)
}

#[test]
fn evidenced_link_label_and_target_consumers_elide_comments_first() {
    for (source, expected) in [
        (
            "[[[start|A[!--x--]B]]]",
            r#"<p><a href="/start">AB</a></p>"#,
        ),
        (
            "[[[st[!--x--]art|AB]]]",
            r#"<p><a href="/start">AB</a></p>"#,
        ),
        (
            "[https://example.com A[!--x--]B]",
            r#"<p><a href="https://example.com">AB</a></p>"#,
        ),
        (
            "[https://exam[!--x--]ple.com AB]",
            r#"<p><a href="https://example.com">AB</a></p>"#,
        ),
    ] {
        assert_eq!(render_wikidot(source), expected, "{source:?}");
    }
}

#[test]
fn comment_elision_handles_field_edges_adjacency_multiline_and_unicode() {
    for (source, expected) in [
        ("[[[start|[!--x--]AB]]]", "AB"),
        ("[[[start|AB[!--x--]]]]", "AB"),
        ("[[[start|A[!--x--][!--y--]B]]]", "AB"),
        ("[[[start|A[!--x\ny--]B]]]", "AB"),
        ("[[[start|雪[!--日本語😀--]月]]]", "雪月"),
    ] {
        assert_eq!(
            render_wikidot(source),
            format!(r#"<p><a href="/start">{expected}</a></p>"#),
            "{source:?}",
        );
    }
}

#[test]
fn automatic_url_and_image_source_use_the_joined_authored_value() {
    assert_eq!(
        render_wikidot("https://example.com/A[!--x--]B"),
        concat!(
            r#"<p><a href="https://example.com/AB">"#,
            "https://example.com/AB</a></p>",
        ),
    );
    assert_eq!(
        render_wikidot("[[image http://example.com/A[!--x--]B.png]]"),
        concat!(
            r#"<img src="http://example.com/AB.png" class="image" "#,
            r#"alt="AB.png">"#,
        ),
    );
}

#[test]
fn block_attribute_values_are_elided_before_owner_filtering() {
    assert_eq!(
        render_wikidot("[[span class=\"A[!--x--]B\"]]X[[/span]]"),
        r#"<p><span class="AB">X</span></p>"#,
    );
    assert_eq!(
        render_wikidot("[[span title=\"A[!--x--]B\"]]X[[/span]]"),
        "<p><span>X</span></p>",
    );
    assert_eq!(
        render_wikidot("[[span class=\"[!--x--]AB[!--y--]\"]]X[[/span]]"),
        r#"<p><span class="AB">X</span></p>"#,
    );
    assert_eq!(
        render_wikidot("[[div class=\"A[!--x--]B\"]]\nX\n[[/div]]"),
        r#"<div class="AB"><p>X</p></div>"#,
    );

    let collapsible =
        render_wikidot("[[collapsible show=\"A[!--x--]B\"]]X[[/collapsible]]");
    assert!(collapsible.contains(">AB</a>"), "{collapsible}");
    assert!(!collapsible.contains("[!--"), "{collapsible}");

    let (tree, errors) = parse("[[code type=\"A[!--x--]B\"]]X[[/code]]", Layout::Wikidot);
    assert!(
        !errors.is_empty(),
        "commented inline code head must recover literally"
    );
    assert!(tree.code_blocks.is_empty(), "{tree:#?}");
    let html =
        render_allow_errors("[[code type=\"A[!--x--]B\"]]X[[/code]]", Layout::Wikidot);
    assert!(html.contains("[[code type=&quot;AB&quot;]]"), "{html}");
    assert!(html.contains("[[/code]]"), "{html}");

    let image = render_wikidot("[[image http://example.com/a.png link=\"A[!--x--]B\"]]");
    assert!(image.contains(r#"<a href="/AB">"#), "{image}");
    assert!(
        image.contains(r#"src="http://example.com/a.png""#),
        "{image}"
    );
}

#[test]
fn unknown_module_arguments_reach_the_normal_diagnostic_after_elision() {
    let html = render_wikidot("[[module FooBar x=\"[!--x--]\"]]");
    assert!(
        html.contains("[[module <em>FooBar</em>]] No such module"),
        "{html}"
    );
    assert!(!html.contains("[!--"), "{html}");
}

#[test]
fn malformed_and_escaped_comment_lookalikes_remain_authored_bytes() {
    for source in [
        "[[[start|A[!--x-- ]B]]]",
        "[[[start|A[!--unclosed B]]]",
        "[[[start|A\\[!--x--]B]]]",
        "[[[start|A[&#33;--x--]B]]]",
        "[[[start|A［！－－x－－］B]]]",
    ] {
        let html = render_allow_errors(source, Layout::Wikidot);
        assert_ne!(html, r#"<p><a href="/start">AB</a></p>"#, "{source:?}");
    }
}

#[test]
fn elision_preserves_regular_block_names_but_can_create_parser_function_fallbacks() {
    for source in [
        "[[mo[!--x--]dule CSS]]body{}[[/module]]",
        "[[inc[!--x--]lude secret]]",
    ] {
        let html = render_allow_errors(source, Layout::Wikidot);
        assert!(html.contains("[["), "{source:?}: {html}");
    }
    assert!(
        !render_allow_errors("[[inc[!--x--]lude secret]]", Layout::Wikidot)
            .contains("Included page"),
    );
    assert_eq!(
        render_allow_errors("[[#i[!--x--]f 1 | ACTIVE | INACTIVE ]]", Layout::Wikidot,),
        r##"<p>[<a href="#if">1 | ACTIVE | INACTIVE</a> ]</p>"##,
    );

    let html = render_wikidot(
        "[[span da[!--x--]ta-owned=\"yes\" st[!--x--]yle=\"color:red\" \
         class=[!--x--]\"joined\"]]X[[/span]]",
    );
    assert_eq!(
        html,
        r#"<p><span class="joined" data-owned="yes" style="color:red">X</span></p>"#,
    );
    let hidden_delimiter =
        render_wikidot("[[span class=\"A[!--\" data-owned=\"yes--]B\"]]X[[/span]]");
    assert_eq!(hidden_delimiter, r#"<p><span class="AB">X</span></p>"#);

    let style = render_wikidot(
        "[[span style=\"background:url(java[!--x--]script:alert(1))\"]]X[[/span]]",
    );
    assert!(style.contains("javascript:"), "{style}");
    let unicode_style = render_wikidot(
        "[[span style=\"background:雪java[!--x--]script:alert(1)\"]]X[[/span]]",
    );
    assert!(unicode_style.contains("javascript:"), "{unicode_style}");

    let image =
        render_allow_errors("[[image java[!--x--]script:alert(1)]]", Layout::Wikidot);
    assert!(!image.contains(r#"src="javascript:"#), "{image}");
}

#[test]
fn joined_url_schemes_are_classified_only_after_elision() {
    let javascript = render_wikidot("[java[!--x--]script:alert(1) SAFE]");
    assert!(javascript.contains("SAFE"), "{javascript}");
    assert!(!javascript.contains("href=\"javascript:"), "{javascript}");

    let data = render_wikidot("[da[!--x--]ta:text/html,boom SAFE]");
    assert!(!data.contains("href=\"data:"), "{data}");

    let split_safe_scheme =
        render_allow_errors("[ht[!--x--]tps://example.com SAFE]", Layout::Wikidot);
    assert!(
        split_safe_scheme.contains("href=\"https://example.com\""),
        "{split_safe_scheme}",
    );

    let internal = render_wikidot("[[[java[!--x--]script:alert(1)|SAFE]]]");
    assert!(internal.contains("SAFE"), "{internal}");
    assert!(!internal.contains("href=\"javascript:"), "{internal}");
}

#[test]
fn literal_owners_and_hidden_blocks_are_unchanged() {
    let (tree, errors) = parse("[[code]]\nA[!--x--]B\n[[/code]]", Layout::Wikidot);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(tree.code_blocks[0].contents, "A[!--x--]B");
    let (html_tree, html_errors) =
        parse("[[html]]\nA[!--x--]B\n[[/html]]", Layout::Wikidot);
    assert!(html_errors.is_empty(), "{html_errors:#?}");
    assert_eq!(html_tree.html_blocks, ["\nA[!--x--]B\n"]);
    assert_eq!(
        render_wikidot("@@A[!--x--]B@@"),
        concat!(
            r#"<p><span style="white-space: pre-wrap;">"#,
            "A[!--x--]B</span></p>",
        ),
    );

    let hidden = render_wikidot(concat!(
        "A[!--",
        "[[module CSS]]body{}[[/module]]",
        "[[include secret]]",
        "--]B",
    ));
    assert_eq!(hidden, "<p>AB</p>");

    assert_eq!(
        render("[[[start|A[!--x--]B]]]", Layout::Wikijump),
        concat!(
            r#"<p><a class="wj-link wj-link-internal" data-link-type="page" "#,
            r#"href="/start">AB</a></p>"#,
        ),
    );
}

#[test]
fn comment_heavy_fields_have_bounded_work_and_output() {
    const COMMENT_COUNT: usize = 4_096;
    const BODY_BYTES: usize = 32_768;
    let dense = format!("[[[start|A{}B]]]", "[!--x--]".repeat(COMMENT_COUNT),);
    let large = format!(
        "[[span class=\"A[!--{}--]B\"]]X[[/span]]",
        "x".repeat(BODY_BYTES),
    );
    let input_bytes = dense.len() + large.len();
    let (sender, receiver) = mpsc::channel();

    std::thread::spawn(move || {
        let dense_html = render_wikidot(&dense);
        let large_html = render_wikidot(&large);
        let _ = sender.send((dense_html, large_html));
    });

    let (dense_html, large_html) = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("comment-elided fields should remain bounded");
    assert_eq!(dense_html, r#"<p><a href="/start">AB</a></p>"#);
    assert_eq!(large_html, r#"<p><span class="AB">X</span></p>"#);
    assert!(dense_html.len() + large_html.len() < input_bytes);
}
