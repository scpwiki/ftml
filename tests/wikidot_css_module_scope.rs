use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender, text::TextRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn render(source: &str) -> (String, Vec<String>, String) {
    let page_info = PageInfo {
        page: Cow::Borrowed("css-module-scope"),
        category: None,
        site: Cow::Borrowed("syntax-differential"),
        title: Cow::Borrowed("CSS module scope"),
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
    let output = HtmlRender.render(&tree, &page_info, &settings);
    let text = TextRender.render(&tree, &page_info, &settings);
    (output.body, output.styles, text)
}

#[test]
fn wikidot_css_module_scope_matrix_matches_live_dom() {
    for (case_id, source, expected) in [
        (
            "scout-css-scope-ownline-multiline",
            "[[module CSS]]\nx{}\n[[/module]]",
            "",
        ),
        (
            "scout-css-scope-ownline-multiline-empty",
            "[[module CSS]]\n\n[[/module]]",
            "<p>[[/module]]</p>",
        ),
        (
            "scout-css-scope-ownline-same-line",
            "[[module CSS]]x{}[[/module]]",
            "<p>[[module CSS]]x{}[[/module]]</p>",
        ),
        (
            "scout-css-scope-ownline-same-line-empty",
            "[[module CSS]][[/module]]",
            "<p>[[module CSS]][[/module]]</p>",
        ),
        (
            "scout-css-scope-prefix-same-line",
            "P[[module CSS]]x{}[[/module]]",
            "<p>P[[module CSS]]x{}[[/module]]</p>",
        ),
        (
            "scout-css-scope-suffix-same-line",
            "[[module CSS]]x{}[[/module]]S",
            "<p>[[module CSS]]x{}[[/module]]S</p>",
        ),
        (
            "scout-css-scope-both-same-line",
            "P[[module CSS]]x{}[[/module]]S",
            "<p>P[[module CSS]]x{}[[/module]]S</p>",
        ),
        (
            "scout-css-scope-prefix-line",
            "P\n[[module CSS]]x{}[[/module]]",
            "<p>P<br>\n[[module CSS]]x{}[[/module]]</p>",
        ),
        (
            "scout-css-scope-suffix-line",
            "[[module CSS]]x{}[[/module]]\nS",
            "<p>[[module CSS]]x{}[[/module]]<br>\nS</p>",
        ),
        (
            "scout-css-scope-quote-ownline",
            "> [[module CSS]]\n> x{}\n> [[/module]]",
            "<blockquote><p>[[module CSS]]<br>\nx{}<br>\n[[/module]]</p></blockquote>",
        ),
        (
            "scout-css-scope-quote-sameline",
            "> [[module CSS]]x{}[[/module]]",
            "<blockquote><p>[[module CSS]]x{}[[/module]]</p></blockquote>",
        ),
        (
            "scout-css-scope-list-ownline",
            "* [[module CSS]]\n* x{}\n* [[/module]]",
            "<ul>\n<li>[[module CSS]]</li>\n<li>x{}</li>\n<li>[[/module]]</li>\n</ul>",
        ),
        (
            "scout-css-scope-list-sameline",
            "* [[module CSS]]x{}[[/module]]",
            "<ul>\n<li>[[module CSS]]x{}[[/module]]</li>\n</ul>",
        ),
        (
            "scout-css-scope-heading-sameline",
            "+ [[module CSS]]x{}[[/module]]",
            "<h1 id=\"toc0\"><span>[[module CSS]]x{}[[/module]]</span></h1>",
        ),
        (
            "scout-css-scope-table-sameline",
            "|| [[module CSS]]x{}[[/module]] ||",
            "<table class=\"wiki-content-table\">\n<tr>\n<td>[[module CSS]]x{}[[/module]]</td>\n</tr>\n</table>",
        ),
        (
            "scout-css-scope-mono-sameline",
            "{{[[module CSS]]x{}[[/module]]}}",
            "<p><tt>[[module CSS]]x{}[[/module]]</tt></p>",
        ),
        (
            "scout-css-scope-raw-inline",
            "@@[[module CSS]]x{}[[/module]]@@",
            "<p><span style=\"white-space: pre-wrap;\">[[module&#32;CSS]]x{}[[/module]]</span></p>",
        ),
        (
            "scout-css-scope-comment",
            "[!-- [[module CSS]]x{}[[/module]] --]",
            "",
        ),
        (
            "scout-css-scope-leading-space-opener",
            " [[module CSS]]\nx{}\n[[/module]]",
            "<p>[[module CSS]]<br>\nx{}<br>\n[[/module]]</p>",
        ),
        (
            "scout-css-scope-leading-space-name",
            "[[ module CSS ]]\nx{}\n[[/module]]",
            "<p>[[ module CSS ]]<br>\nx{}<br>\n[[/module]]</p>",
        ),
        (
            "scout-css-scope-lowercase-ownline",
            "[[module css]]\nx{}\n[[/module]]",
            "",
        ),
        (
            "scout-css-scope-lowercase-sameline",
            "[[module css]]x{}[[/module]]",
            "<p>[[module css]]x{}[[/module]]</p>",
        ),
        (
            "scout-css-scope-uppercase-ownline",
            "[[module CSS]]\nx{}\n[[/MODULE]]",
            "",
        ),
        (
            "scout-css-scope-head-arg-ownline",
            "[[module CSS show=\"head\"]]\nx{}\n[[/module]]",
            "",
        ),
        (
            "scout-css-scope-head-arg-sameline",
            "[[module CSS show=\"head\"]]x{}[[/module]]",
            "",
        ),
        (
            "scout-css-scope-unclosed-ownline",
            "[[module CSS]]\nx{}",
            "<p>x{}</p>",
        ),
        (
            "scout-css-scope-unclosed-sameline",
            "[[module CSS]]x{}",
            "<p>[[module CSS]]x{}</p>",
        ),
        (
            "scout-css-scope-extra-bracket",
            "[[module CSS]]]x{}[[/module]]",
            "<p>[[module CSS]]]x{}[[/module]]</p>",
        ),
        (
            "scout-css-scope-adjacent-text",
            "A\n[[module CSS]]\nx{}\n[[/module]]\nB",
            "<p>A</p><p>B</p>",
        ),
        (
            "scout-css-scope-adjacent-text-sameline",
            "A\n[[module CSS]]x{}[[/module]]\nB",
            "<p>A<br>\n[[module CSS]]x{}[[/module]]<br>\nB</p>",
        ),
    ] {
        assert_eq!(render(source).0, expected, "{case_id}: {source:?}");
    }
}

#[test]
fn wikidot_css_recovery_preserves_style_authority_boundaries() {
    let (body, styles, text) = render("[[module CSS]]\nx{}\n[[/module]]");
    assert!(body.is_empty());
    assert_eq!(styles, ["x{}"]);
    assert!(text.is_empty());

    let (body, styles, text) = render("[[module CSS show=\"head\"]]x{}[[/module]]");
    assert!(body.is_empty());
    assert!(styles.is_empty());
    assert!(text.is_empty());

    let (body, styles, text) = render("[[module CSS]]\nx{}");
    assert_eq!(body, "<p>x{}</p>");
    assert!(styles.is_empty());
    assert_eq!(text, "x{}");

    let (body, styles, text) = render("[[module CSS]]\n\n[[/module]]");
    assert_eq!(body, "<p>[[/module]]</p>");
    assert!(styles.is_empty());
    assert_eq!(text, "[[/module]]");

    let (body, styles, text) = render("[[module CSS]]]x{}[[/module]]");
    assert_eq!(body, "<p>[[module CSS]]]x{}[[/module]]</p>");
    assert!(styles.is_empty());
    assert_eq!(text, "[[module CSS]]]x{}[[/module]]");
}

#[test]
fn repeated_literal_and_empty_css_recovery_stays_bounded() {
    let unit = concat!(
        "A\n[[module CSS]]]x{}[[/module]]\n",
        "[[module CSS]]\n\n[[/module]]\n",
        "[[module CSS]]inline{}[[/module]]\n",
    );
    let source = unit.repeat(1_024);
    let started = Instant::now();
    let (body, styles, _) = render(&source);
    let elapsed = started.elapsed();

    assert_eq!(body.matches("[[module CSS]]]x{}[[/module]]").count(), 1_024);
    assert_eq!(body.matches("[[/module]]").count(), 3_072);
    assert!(styles.is_empty());
    assert!(
        elapsed < Duration::from_secs(3),
        "CSS recovery took {elapsed:?}",
    );
}
