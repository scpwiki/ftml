use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::parsing::ParseErrorKind;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("underclosed-owner"),
        category: Some(Cow::Borrowed("_default")),
        site: Cow::Borrowed("coverage"),
        title: Cow::Borrowed("Underclosed owner"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![Cow::Borrowed("test")],
        language: Cow::Borrowed("en"),
    }
}

#[test]
fn underclosed_wikijump_body_owner_families_reuse_deterministic_failures() {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let owners = [
        ("align", "[[=]]", "[[/=]]"),
        ("anchor", "[[a href=\"/x\"]]", "[[/a]]"),
        ("bibliography", "[[bibliography]]", "[[/bibliography]]"),
        ("blockquote", "[[blockquote]]", "[[/blockquote]]"),
        ("bold", "[[bold]]", "[[/bold]]"),
        ("collapsible", "[[collapsible]]", "[[/collapsible]]"),
        ("deletion", "[[del]]", "[[/del]]"),
        ("hidden", "[[hidden]]", "[[/hidden]]"),
        ("ifcategory", "[[ifcategory _default]]", "[[/ifcategory]]"),
        ("insertion", "[[ins]]", "[[/ins]]"),
        ("invisible", "[[invisible]]", "[[/invisible]]"),
        ("italics", "[[italics]]", "[[/italics]]"),
        ("mark", "[[mark]]", "[[/mark]]"),
        ("monospace", "[[monospace]]", "[[/monospace]]"),
        ("ordered-list", "[[ol]]", "[[/ol]]"),
        ("paragraph", "[[paragraph]]", "[[/paragraph]]"),
        ("ruby", "[[ruby]]", "[[/ruby]]"),
        ("size", "[[size 100%]]", "[[/size]]"),
        ("strikethrough", "[[strikethrough]]", "[[/strikethrough]]"),
        ("subscript", "[[subscript]]", "[[/subscript]]"),
        ("superscript", "[[superscript]]", "[[/superscript]]"),
        ("tabview", "[[tabview]]", "[[/tabview]]"),
        ("underline", "[[underline]]", "[[/underline]]"),
        ("unordered-list", "[[ul]]", "[[/ul]]"),
    ];

    for (owner, opener, closer) in owners {
        for (openers, closers) in [(32, 0), (64, 32), (128, 64)] {
            let source =
                format!("{}X{}", opener.repeat(openers), closer.repeat(closers),);
            let tokenization = ftml::tokenize(&source);
            let started = Instant::now();
            let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();

            assert!(
                started.elapsed() < Duration::from_secs(2),
                "{owner}: {openers} openers / {closers} closers took {:?}",
                started.elapsed(),
            );
            if owner == "size" && closers > 0 {
                assert!(
                    errors.is_empty(),
                    "{owner}: designated underclosed nests must parse cleanly: {errors:?}"
                );
            } else {
                assert!(
                    !errors.is_empty(),
                    "{owner}: underclosed source must report errors"
                );
            }
            assert!(
                !tree.elements.is_empty(),
                "{owner}: fallback must preserve observable source"
            );
        }
    }
}

#[test]
fn parentless_partial_blocks_reuse_exact_rejection_failures() {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let cases = [
        (
            "list-item",
            "[[li]]X[[/li]]",
            ParseErrorKind::ListItemOutsideList,
        ),
        (
            "table-row",
            "[[row]][[cell]]X[[/cell]][[/row]]",
            ParseErrorKind::TableRowOutsideTable,
        ),
        (
            "table-cell",
            "[[cell]]X[[/cell]]",
            ParseErrorKind::TableCellOutsideTable,
        ),
        (
            "table-header-cell",
            "[[hcell]]X[[/hcell]]",
            ParseErrorKind::TableCellOutsideTable,
        ),
        (
            "tab",
            "[[tab Title]]X[[/tab]]",
            ParseErrorKind::TabOutsideTabView,
        ),
        (
            "ruby-text",
            "[[rt]]X[[/rt]]",
            ParseErrorKind::RubyTextOutsideRuby,
        ),
    ];

    for (name, source, expected) in cases {
        let tokenization = ftml::tokenize(source);
        let started = Instant::now();
        let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(started.elapsed() < Duration::from_millis(250), "{name}");
        let first = errors
            .first()
            .unwrap_or_else(|| panic!("{name}: missing error"));
        assert_eq!(first.rule(), "page", "{name}: caller rule changed");
        assert_eq!(first.kind(), expected, "{name}: wrong partial error");
        assert!(
            html.contains(source),
            "{name}: source did not stay literal: {html}"
        );
    }

    for (name, opener, closer) in [
        ("list-item", "[[li]]", "[[/li]]"),
        ("table-row", "[[row]]", "[[/row]]"),
        ("table-cell", "[[cell]]", "[[/cell]]"),
        ("tab", "[[tab Title]]", "[[/tab]]"),
        ("ruby-text", "[[rt]]", "[[/rt]]"),
    ] {
        let source = format!("{}X{}", opener.repeat(128), closer.repeat(64));
        let tokenization = ftml::tokenize(&source);
        let started = Instant::now();
        let (_tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{name}: parentless partial retry took {:?}",
            started.elapsed(),
        );
        assert!(!errors.is_empty());
    }
}

#[test]
fn unclosed_strong_openers_finish_in_pagenav_and_draft() {
    let page_info = page_info();
    let source = format!("{}x", "[[strong]]".repeat(22));

    for mode in [WikitextMode::PageNav, WikitextMode::Draft] {
        let settings = WikitextSettings::from_mode(mode, Layout::Wikijump);
        let tokenization = ftml::tokenize(&source);
        let started = Instant::now();
        let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();

        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{mode:?} took {:?}",
            started.elapsed(),
        );
        assert!(
            !errors.is_empty(),
            "{mode:?}: unclosed strong openers must report errors"
        );
        assert!(
            !tree.elements.is_empty(),
            "{mode:?}: fallback must preserve observable source"
        );
    }
}

#[test]
fn quoted_unclosed_ruby_lines_finish_in_list_mode() {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikijump);
    let source = "> [[collapsible show=\"+[[ruby]]\" 100%]]\n".repeat(64);
    let tokenization = ftml::tokenize(&source);
    let started = Instant::now();
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();

    assert!(
        started.elapsed() < Duration::from_secs(2),
        "quoted ruby lines took {:?}",
        started.elapsed(),
    );
    assert!(!errors.is_empty(), "unclosed ruby lines must report errors");
    assert!(
        !tree.elements.is_empty(),
        "fallback must preserve observable source"
    );
}

#[test]
fn unmatched_monospace_beside_spans_stays_within_budget() {
    let page_info = page_info();
    let source = "{{[[span]]\n".repeat(384);
    let closed = "{{x}}";

    for mode in [
        WikitextMode::Page,
        WikitextMode::PageNav,
        WikitextMode::Draft,
        WikitextMode::ForumPost,
    ] {
        let settings = WikitextSettings::from_mode(mode, Layout::Wikijump);
        let tokenization = ftml::tokenize(&source);
        let started = Instant::now();
        let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{mode:?}: unmatched monospace beside spans took {:?}",
            started.elapsed(),
        );
        assert!(
            !errors.is_empty(),
            "{mode:?}: unmatched monospace must report errors"
        );
        assert!(
            !tree.elements.is_empty(),
            "{mode:?}: fallback must preserve observable source"
        );

        let tokenization = ftml::tokenize(closed);
        let (tree, _errors) = ftml::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(
            html.contains("wj-monospace"),
            "{mode:?}: a closed monospace must still render, got {html:?}",
        );
    }
}

#[test]
fn nested_unclosed_tab_pairs_stay_within_budget() {
    let page_info = page_info();
    let pairs: String = (0..64)
        .map(|index| format!("[[tabview]]\t\n[[tab t{index}]]\t\n"))
        .collect();
    let closed = "[[tabview]]\n[[tab a]]x[[/tab]]\n[[/tabview]]";

    for mode in [WikitextMode::Page, WikitextMode::PageNav] {
        let settings = WikitextSettings::from_mode(mode, Layout::Wikijump);
        let tokenization = ftml::tokenize(&pairs);
        let started = Instant::now();
        let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{mode:?}: 64 unclosed tab pairs took {:?}",
            started.elapsed(),
        );
        assert!(
            !errors.is_empty(),
            "{mode:?}: unclosed tab pairs must report errors"
        );
        assert!(
            !tree.elements.is_empty(),
            "{mode:?}: fallback must preserve observable source"
        );

        let tokenization = ftml::tokenize(closed);
        let started = Instant::now();
        let (tree, _errors) = ftml::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{mode:?}: closed tabview took {:?}",
            started.elapsed(),
        );
        assert!(
            !html.contains("[[tabview]]"),
            "{mode:?}: a closed tabview must still form a block, got {html:?}",
        );
    }
}
