use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;

fn render(source: &str) -> (String, Vec<ftml::parsing::ParseError>) {
    let page_info = PageInfo {
        page: Cow::Borrowed("cross-owner-boundary"),
        category: Some(Cow::Borrowed("_default")),
        site: Cow::Borrowed("sandbox-for-codex"),
        title: Cow::Borrowed("Cross owner boundary"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    };
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    (HtmlRender.render(&tree, &page_info, &settings).body, errors)
}

#[test]
fn crossed_div_and_collapsible_explicit_lists_match_live_owner_order() {
    for (source, expected) in [
        (
            "[[div]]\n[[ul]]\nX\n[[/div]]\n[[/ul]]\n",
            concat!(
                "<div><ul>\n<li style=\"list-style: none\">X</li>\n</ul></div>",
                "<br>\n",
            ),
        ),
        (
            "[[div]]\n[[ol]]\nX\n[[/div]]\n[[/ol]]\n",
            concat!(
                "<div><ol>\n<li style=\"list-style: none\">X</li>\n</ol></div>",
                "<ul>\n<li style=\"list-style: none\"><br>\n</li>\n</ul>",
            ),
        ),
        (
            "[[collapsible]]\n[[ul]]\nX\n[[/collapsible]]\n[[/ul]]\n",
            concat!(
                "<div class=\"collapsible-block\"><div class=\"collapsible-block-folded\">",
                "<a class=\"collapsible-block-link\" href=\"javascript:;\">+&nbsp;show&nbsp;block</a></div>",
                "<div class=\"collapsible-block-unfolded\" style=\"display:none\">",
                "<div class=\"collapsible-block-unfolded-link\"><a class=\"collapsible-block-link\" href=\"javascript:;\">",
                "–&nbsp;hide&nbsp;block</a></div><div class=\"collapsible-block-content\">",
                "<ul>\n<li style=\"list-style: none\">X</li>\n</ul></div></div></div><br>\n",
            ),
        ),
        (
            "[[collapsible]]\n[[ol]]\nX\n[[/collapsible]]\n[[/ol]]\n",
            concat!(
                "<div class=\"collapsible-block\"><div class=\"collapsible-block-folded\">",
                "<a class=\"collapsible-block-link\" href=\"javascript:;\">+&nbsp;show&nbsp;block</a></div>",
                "<div class=\"collapsible-block-unfolded\" style=\"display:none\">",
                "<div class=\"collapsible-block-unfolded-link\"><a class=\"collapsible-block-link\" href=\"javascript:;\">",
                "–&nbsp;hide&nbsp;block</a></div><div class=\"collapsible-block-content\">",
                "<ol>\n<li style=\"list-style: none\">X</li>\n</ol></div></div></div>",
                "<ul>\n<li style=\"list-style: none\"><br>\n</li>\n</ul>",
            ),
        ),
    ] {
        let (html, errors) = render(source);
        assert!(errors.is_empty(), "{source:?}: {errors:#?}");
        assert_eq!(html, expected, "{source:?}");
    }
}

#[test]
fn rejected_table_before_explicit_list_keeps_live_root_whitespace() {
    for (name, source, list_tag) in [
        ("ul", "[[table]]\n[[ul]]\nX\n[[/table]]\n[[/ul]]\n", "ul"),
        ("ol", "[[table]]\n[[ol]]\nX\n[[/table]]\n[[/ol]]\n", "ol"),
    ] {
        let (html, errors) = render(source);
        assert!(
            !errors.is_empty(),
            "{name}: rejected table must remain observable"
        );
        assert_eq!(
            html,
            format!(
                "\n\n[[table]]<br>\n<{list_tag}>\n<li style=\"list-style: none\">X<br>\n[[/table]]</li>\n</{list_tag}><br>\n"
            ),
            "{name}",
        );
    }
}

#[test]
fn inline_scope_list_line_break_ownership_matches_live_proper_and_crossed_closes() {
    for (source, expected) in [
        (
            "[[span class=\"probe\"]]\n[[ul]]\nX\n[[/ul]]\n[[/span]]\n",
            concat!(
                "<p><span class=\"probe\"><br></span></p>",
                "<ul>\n<li style=\"list-style: none\"><span class=\"probe\">X</span></li>\n</ul>",
            ),
        ),
        (
            "[[size 120%]]\n[[ol]]\nX\n[[/ol]]\n[[/size]]\n",
            concat!(
                "<p><span style=\"font-size:120%;\"><br>\n</span></p>",
                "<ol>\n<li style=\"list-style: none\"><span style=\"font-size:120%;\">X</span></li>\n</ol>",
            ),
        ),
        (
            "[[span class=\"probe\"]]\n[[ul]]\nX\n[[/span]]\n[[/ul]]\n",
            concat!(
                "<span class=\"probe\"><br></span>",
                "<ul>\n<li style=\"list-style: none\"><span class=\"probe\">X<br></span></li>\n</ul><br>\n",
            ),
        ),
        (
            "[[size 120%]]\n[[ul]]\nX\n[[/size]]\n[[/ul]]\n",
            concat!(
                "<span style=\"font-size:120%;\"><br>\n</span>",
                "<ul>\n<li style=\"list-style: none\"><span style=\"font-size:120%;\">X<br>\n</span></li>\n</ul><br>\n",
            ),
        ),
    ] {
        let (html, errors) = render(source);
        assert!(errors.is_empty(), "{source:?}: {errors:#?}");
        assert_eq!(html, expected, "{source:?}");
    }
}

#[test]
fn crossed_inline_scope_wraps_wikidot_collapsible_handles() {
    let source = concat!(
        "[[span class=\"probe\"]]\n",
        "[[collapsible]]\n",
        "X\n",
        "[[/span]]\n",
        "[[/collapsible]]\n",
    );
    let (html, errors) = render(source);
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(
        html.contains(
            "<div class=\"collapsible-block-folded\"><span class=\"probe\"><a class=\"collapsible-block-link\""
        ),
        "{html}",
    );
    assert!(
        html.contains(
            "<div class=\"collapsible-block-unfolded-link\"><span class=\"probe\"><a class=\"collapsible-block-link\""
        ),
        "{html}",
    );
    assert!(
        html.contains("<div class=\"collapsible-block-content\"><p><span class=\"probe\">X<br></span></p></div>"),
        "{html}",
    );
}
