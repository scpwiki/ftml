use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender, text::TextRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::SyntaxTree;
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("code-blocks"),
        category: None,
        site: Cow::Borrowed("compatibility"),
        title: Cow::Borrowed("Code blocks"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![],
        language: Cow::Borrowed("en"),
    }
}

fn parse(source: &str) -> (SyntaxTree<'_>, Vec<ftml::parsing::ParseError>) {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    (tree.to_owned(), errors)
}

fn render(source: &str) -> (String, String, SyntaxTree<'static>) {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, _) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = HtmlRender.render(&tree, &page_info, &settings).body;
    let text = TextRender.render(&tree, &page_info, &settings);
    (html, text, tree.to_owned())
}

#[test]
fn complete_v7_code_family_matches_live_page_preview() {
    let cases = [
        (
            "code-canonical-valid",
            "[[code]]\nv7 body\n[[/code]]",
            "<div class=\"code\"><pre><code>v7 body</code></pre></div>",
            "v7 body",
            1,
        ),
        (
            "code-incomplete-opening",
            "[[code",
            "<p>[[code</p>",
            "[[code",
            0,
        ),
        (
            "code-case-variation-name",
            "[[CODE]]\nv7 body\n[[/CODE]]",
            "<div class=\"code\"><pre><code>v7 body</code></pre></div>",
            "v7 body",
            1,
        ),
        (
            "code-whitespace-control",
            "[[code]]\nalpha\tbeta\u{00a0}gamma\n[[/code]]",
            "<div class=\"code\"><pre><code>alpha    beta gamma</code></pre></div>",
            "alpha    beta gamma",
            1,
        ),
        (
            "code-boundary",
            "start-[[code]]\nv7 body\n[[/code]]-middle\n\n[[code]]\nend\n[[/code]]",
            concat!(
                "<p>start-[[code]]<br>\nv7 body<br>\n[[/code]]-middle</p>",
                "<div class=\"code\"><pre><code>end</code></pre></div>",
            ),
            "start-[[code]]\nv7 body\n[[/code]]-middle\n\nend",
            1,
        ),
        (
            "code-serialization-source-preservation",
            "[[code]]\nserialized body\n[[/code]]",
            "<div class=\"code\"><pre><code>serialized body</code></pre></div>",
            "serialized body",
            1,
        ),
        (
            "code-text-renderer-relevance",
            "[[code]]\nvisible text\n[[/code]]",
            "<div class=\"code\"><pre><code>visible text</code></pre></div>",
            "visible text",
            1,
        ),
        (
            "code-missing-close",
            "[[code]]\nunterminated body",
            "<p>[[code]]<br>\nunterminated body</p>",
            "[[code]]\nunterminated body",
            0,
        ),
        (
            "code-nesting-same-feature",
            "[[code]]\n[[code]]\nnested\n[[/code]]\n[[/code]]",
            concat!(
                "<div class=\"code\"><pre><code>",
                "[[code]]\nnested\n[[/code]]",
                "</code></pre></div>",
            ),
            "[[code]]\nnested\n[[/code]]",
            1,
        ),
        (
            "code-nesting-different-feature",
            "[[code]]\n[[bold]]nested[[/bold]]\n[[/code]]",
            concat!(
                "<div class=\"code\"><pre><code>",
                "[[bold]]nested[[/bold]]",
                "</code></pre></div>",
            ),
            "[[bold]]nested[[/bold]]",
            1,
        ),
        (
            "code-invalid-overlap",
            "[[code]]\nouter [[bold]]inner\n[[/code]][[/bold]]",
            concat!(
                "<p>[[code]]<br>\n",
                "outer [[bold]]inner<br>\n",
                "[[/code]][[/bold]]</p>",
            ),
            "[[code]]\nouter [[bold]]inner\n[[/code]][[/bold]]",
            0,
        ),
        (
            "code-duplicate-arguments",
            "[[code type=\"one\" type=\"two\"]]\nv7 body\n[[/code]]",
            "<div class=\"code\"><pre><code>v7 body</code></pre></div>",
            "v7 body",
            1,
        ),
        (
            "code-empty-arguments",
            "[[code type=\"\"]]\nv7 body\n[[/code]]",
            "<div class=\"code\"><pre><code>v7 body</code></pre></div>",
            "v7 body",
            1,
        ),
        (
            "code-unknown-argument",
            "[[code v7UnknownArgument=\"x\"]]\nv7 body\n[[/code]]",
            "<div class=\"code\"><pre><code>v7 body</code></pre></div>",
            "v7 body",
            1,
        ),
        (
            "code-quote-variation",
            "[[code type='single quoted' data-v7=unquoted]]\nv7 body\n[[/code]]",
            "<div class=\"code\"><pre><code>v7 body</code></pre></div>",
            "v7 body",
            1,
        ),
    ];

    let mut failures = Vec::new();
    for (name, source, expected_html, expected_text, expected_blocks) in cases {
        let (html, text, tree) = render(source);
        if html != expected_html {
            failures.push(format!(
                "{name} HTML:\n  actual: {html:?}\nexpected: {expected_html:?}",
            ));
        }
        if text != expected_text {
            failures.push(format!(
                "{name} text:\n  actual: {text:?}\nexpected: {expected_text:?}",
            ));
        }
        if tree.code_blocks.len() != expected_blocks {
            failures.push(format!(
                "{name} blocks:\n  actual: {}\nexpected: {expected_blocks}\ntree: {tree:#?}",
                tree.code_blocks.len(),
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn active_code_normalizes_only_live_body_bytes() {
    let source = concat!(
        "[[code type=\"rust\" name=\"Sample Heading\"]]\r\n",
        "alpha\tbeta\u{00a0}gamma  \r\n",
        "  leading\u{00a0}and\tinternal\t  \r\n",
        "[[/code]]",
    );
    let (_, _, tree) = render(source);

    let [code] = tree.code_blocks.as_slice() else {
        panic!("expected one code block, got {tree:#?}");
    };
    // Live Wikidot (PagePreview, captured twice): tabs expand to four
    // spaces, NBSP is served as a space, interior trailing spaces are kept,
    // and the body's trailing ASCII whitespace is trimmed (disc4 F2).
    assert_eq!(
        code.contents,
        "alpha    beta gamma  \n  leading and    internal",
    );
    assert_eq!(code.language.as_deref(), Some("rust"));
    assert_eq!(code.name.as_deref(), Some("sample-heading"));
}

#[test]
fn active_code_keeps_wiki_shaped_content_inert_and_escaped() {
    let source = concat!(
        "[[code]]\n",
        "[[code]]\nnested\n[[/code]]\n",
        "[[#expr 1 + 1]]\n",
        "[[module CSS]]<script>alert(1)</script>[[/module]]\n",
        "[[include component:secret]] **bold** [javascript:alert(1) x]\n",
        "[[/code]]",
    );
    let (html, _, tree) = render(source);

    assert_eq!(tree.code_blocks.len(), 1, "{tree:#?}");
    assert_eq!(
        tree.code_blocks[0].contents,
        concat!(
            "[[code]]\nnested\n[[/code]]\n",
            "[[#expr 1 + 1]]\n",
            "[[module CSS]]<script>alert(1)</script>[[/module]]\n",
            "[[include component:secret]] **bold** [javascript:alert(1) x]",
        ),
    );
    assert!(html.contains("[[#expr 1 + 1]]"), "{html}");
    assert!(html.contains("[[module CSS]]&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!html.contains("<script>"), "{html}");
    assert!(!html.contains("<strong>"), "{html}");
    assert!(!html.contains("href="), "{html}");
}

#[test]
fn rejected_code_candidates_do_not_gain_code_whitespace_rules() {
    for source in [
        "prefix[[code]]\nalpha\tbeta\u{00a0}gamma\n[[/code]]",
        "[[code]]\nalpha\tbeta\u{00a0}gamma",
        "[[code]]\nouter [[bold]]inner\t\u{00a0}\n[[/code]][[/bold]]",
    ] {
        let (_, _, tree) = render(source);
        assert!(tree.code_blocks.is_empty(), "{source:?}: {tree:#?}");
    }
}

#[test]
fn dense_and_unclosed_code_candidates_stay_bounded() {
    let mut dense = String::from("[[code]]\n");
    for index in 0..4_096 {
        dense.push_str(&format!(
            "[[bold]]literal-{index}[[/bold]] [[module CSS]]x{{}}[[/module]]\n",
        ));
    }
    dense.push_str("[[/code]]");

    let started = Instant::now();
    let (tree, errors) = parse(&dense);
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "dense parse took {elapsed:?}"
    );
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(tree.code_blocks.len(), 1, "{tree:#?}");
    assert!(tree.code_blocks[0].contents.contains("literal-4095"));

    let mut unclosed = String::new();
    for index in 0..512 {
        unclosed.push_str(&format!("[[code]]\nunclosed-{index}\n"));
    }
    let started = Instant::now();
    let (tree, _) = parse(&unclosed);
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "unclosed parse took {elapsed:?}",
    );
    assert!(tree.code_blocks.is_empty(), "{tree:#?}");
}

#[test]
fn protected_closers_fallback_pairing_matches_live_page_preview() {
    let cases = [
        (
            "code-protected-close",
            concat!(
                "[[code]]\n",
                "[[code type=\"php\"]]\n",
                "日本語...\n",
                "line\\\n",
                "next\n",
                "[[/code]]\n",
                "@<[[code]]>@\n",
                "@<[[/code]]>@\n",
                "@@[[/code]]@@\n",
                "tail...\n",
                "[[/code ]]\n",
                "\n",
                "outside...",
            ),
            concat!(
                "<div class=\"code\"><pre><code>[[code type=&quot;php&quot;]]\n",
                "日本語...\n",
                "line\\\n",
                "next</code></pre></div><br>\n",
                "<span style=\"white-space: pre-wrap;\">[[code]]</span><br>\n",
                "<span style=\"white-space: pre-wrap;\">[[/code]]</span><br>\n",
                "<span style=\"white-space: pre-wrap;\">[[/code]]</span><br>\n",
                "tail…<br>\n",
                "[[/code ]]\n",
                "<p>outside…</p>",
            ),
            Some("[[code type=\"php\"]]\n日本語...\nline\\\nnext"),
        ),
        (
            "code-typed-inner-two-closers",
            "[[code]]\n[[code type=\"php\"]]\ninner\n[[/code]]\nafter\n[[/code]]",
            concat!(
                "<div class=\"code\"><pre><code>[[code type=&quot;php&quot;]]\n",
                "inner\n",
                "[[/code]]\n",
                "after</code></pre></div>",
            ),
            Some("[[code type=\"php\"]]\ninner\n[[/code]]\nafter"),
        ),
        (
            "code-bare-inner-spaced-outer-closer",
            "[[code]]\n[[code]]\ninner\n[[/code]]\nafter\n[[/code ]]",
            concat!(
                "<div class=\"code\"><pre><code>[[code]]\n",
                "inner</code></pre></div><br>\n",
                "after<br>\n",
                "[[/code ]]",
            ),
            Some("[[code]]\ninner"),
        ),
        (
            "code-bare-inner-outer-fallback-with-whitespace",
            "[[code]]\n[[code]]\nalpha\tbeta\u{00a0}gamma\n[[/code]]",
            "<div class=\"code\"><pre><code>[[code]]\nalpha    beta gamma</code></pre></div>",
            Some("[[code]]\nalpha    beta gamma"),
        ),
        (
            "code-spaced-closer-stays-literal",
            "[[code]]\nbody\n[[/code ]]\nafter",
            "<p>[[code]]<br>\nbody<br>\n[[/code ]]<br>\nafter</p>",
            None,
        ),
    ];

    let mut failures = Vec::new();
    for (name, source, expected_html, expected_contents) in cases {
        let (html, _text, tree) = render(source);
        if html != expected_html {
            failures.push(format!(
                "{name} HTML:\n  actual: {html:?}\nexpected: {expected_html:?}",
            ));
        }
        let contents = tree.code_blocks.first().map(|block| &*block.contents);
        if contents != expected_contents {
            failures.push(format!(
                "{name} contents:\n  actual: {contents:?}\nexpected: {expected_contents:?}",
            ));
        }
        let expected_blocks = expected_contents.is_some() as usize;
        if tree.code_blocks.len() != expected_blocks {
            failures.push(format!(
                "{name} blocks:\n  actual: {}\nexpected: {expected_blocks}",
                tree.code_blocks.len(),
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
