use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{PageExistenceResolver, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;

struct MissingPages;

impl PageExistenceResolver for MissingPages {
    fn page_exists(&self, _site: &str, _page: &str) -> bool {
        false
    }
}

fn render_with_layout(source: &str, layout: Layout) -> String {
    let page_info = PageInfo {
        page: Cow::Borrowed("variable-ownership"),
        category: None,
        site: Cow::Borrowed("syntax-differential"),
        title: Cow::Borrowed("Variable ownership"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    };
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokens = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokens, &page_info, &settings).into();
    assert!(errors.is_empty(), "{source:?}: {errors:#?}");
    HtmlRender
        .render_with_page_existence(&tree, &page_info, &settings, &MissingPages)
        .body
}

fn render(source: &str) -> String {
    render_with_layout(source, Layout::Wikidot)
}

#[test]
fn wikidot_variables_keep_text_authority_and_literal_attribute_values() {
    assert_eq!(
        render(r#"[[span title="{$x}"]]A[[/span]]"#),
        "<p><span>A</span></p>",
    );
    assert_eq!(
        render(r#"[[span class="{$x}"]]A[[/span]]"#),
        r#"<p><span class="{$x}">A</span></p>"#,
    );
    assert_eq!(render("A{$x}B"), "<p>A{$x}B</p>");
    assert_eq!(
        render("[[[scp-002|{$x}]]]"),
        "<p><a class=\"newpage\" href=\"/scp-002\">{$x}</a></p>"
    );
    assert_eq!(
        render("[[[{$target}|A]]]"),
        "<p><a class=\"newpage\" href=\"/target\">A</a></p>"
    );
    assert_eq!(render("{{$x}}"), "<p><tt>$x</tt></p>");
    assert_eq!(render(r"\{$x}"), r"<p>\{$x}</p>");
    assert_eq!(render("{$foo-bar}"), "<p>{$foo-bar}</p>");
    assert_eq!(render("{$foo_bar}"), "<p>{$foo_bar}</p>");
}

#[test]
fn wikidot_div_preserves_literal_variable_class_around_an_explicit_list() {
    let source = concat!(
        "[[div id=\"fruit\" class=\"{$class}\"]]\n",
        "[[ul]]\n",
        "[[li]] 1 [[/li]]\n",
        "[[/ul]]\n",
        "[[/div]]",
    );

    assert_eq!(
        render(source),
        "<div class=\"{$class}\" id=\"u-fruit\"><ul>\n<li>1</li>\n</ul></div>",
    );
}

#[test]
fn wikidot_div_literal_variable_class_normalizes_whitespace() {
    let source = concat!(
        "[[div id=\"fruit\" class=\" \t  lead\n{$class}\r\ttrail\u{000B}\u{000C}tail  \"]]\n",
        "[[ul]]\n",
        "[[li]] 1 [[/li]]\n",
        "[[/ul]]\n",
        "[[/div]]",
    );

    assert_eq!(
        render(source),
        "<div class=\"lead {$class} trailtail\" id=\"u-fruit\"><ul>\n<li>1</li>\n</ul></div>",
    );
}

#[test]
fn wikidot_literal_variable_attributes_render_verbatim() {
    assert_eq!(
        render("[[div class=\"{$class}\"]]\n1\n[[/div]]"),
        r#"<div class="{$class}"><p>1</p></div>"#,
    );
    assert_eq!(
        render("[[div data-probe=\"{$class}\"]]\n1\n[[/div]]"),
        r#"<div data-probe="{$class}"><p>1</p></div>"#,
    );
    assert_eq!(
        render(r#"[[span class="{$class}"]]1[[/span]]"#),
        r#"<p><span class="{$class}">1</span></p>"#,
    );
    assert_eq!(
        render(r#"[[ul class="{$class}"]][[li]]1[[/li]][[/ul]]"#),
        "<ul class=\"{$class}\">\n<li>1</li>\n</ul><br>\n",
    );
    assert_eq!(
        render(r#"[[a href="some-page" class="{$class}"]]1[[/a]]"#),
        r#"<p><a class="{$class}" href="some-page">1</a></p>"#,
    );
    assert_eq!(
        render(
            r#"[[table class="{$class}"]][[row]][[cell]]1[[/cell]][[/row]][[/table]]"#,
        ),
        "<table class=\"{$class}\">\n<tr>\n<td>1</td>\n</tr>\n</table>",
    );

    for source in [
        "[[div CLASS=\"{$class}\"]]\n1\n[[/div]]",
        "[[div onclick=\"alert(1)\"]]\n1\n[[/div]]",
    ] {
        let html = render(source);
        assert!(!html.contains("class=\"{$class}\""), "{source}: {html}");
        assert!(!html.contains("<div onclick="), "{source}: {html}");
    }

    assert!(
        render_with_layout(r#"[[div class="{$class}"]]1[[/div]]"#, Layout::Wikijump)
            .contains(r#"class="{$class}"#),
    );
}
