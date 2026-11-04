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
