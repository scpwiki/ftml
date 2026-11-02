use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender, text::TextRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::collections::HashSet;

#[test]
fn parser_function_openers_follow_the_live_case_and_spacing_grammar() {
    for (source, expected) in [
        ("[[#if 1 | YES | NO ]]", "YES"),
        ("[[#IF 1 | YES | NO ]]", ""),
        ("[[#If 1 | YES | NO ]]", ""),
        ("[[#if 1|YES|NO]]", ""),
        ("[[#ifexpr 1 | YES | NO ]]", "YES"),
        ("[[#IFEXPR 1 | YES | NO ]]", ""),
        ("[[#ifexpr 1|YES|NO]]", "syntax error near `|YES|NO`"),
        ("[[#expr 1+2 ]]", "3"),
        ("[[#EXPR 1+2 ]]", ""),
        ("[[#expr1+2]]", "[[#expr1+2]]"),
        ("[[ #expr 1+2 ]]", "[[ #expr 1+2 ]]"),
    ] {
        assert_eq!(
            ftml::resolve_wikidot_parser_functions(source),
            expected,
            "{source:?}",
        );
    }
}

#[test]
fn conditional_source_boundaries_match_the_live_matrix() {
    for (source, expected) in [
        ("[[#if   | YES | NO ]]", "YES"),
        ("[[#if  | YES | NO ]]", "NO"),
        ("[[#if | YES | NO ]]", "NO"),
        ("[[#if 1 | YES ]]", "YES"),
        ("[[#if 0 | YES ]]", ""),
        ("[[#if 1 ]]", ""),
        ("[[#if 1 | A|B | C ]]", "A|B"),
        ("[[#if 0 | A | B|C ]]", "B|C"),
        ("[[#if\t1\t|\tYES\t|\tNO\t]]", "\tYES\t"),
        ("[[#if\n1\n|\nYES\n|\nNO\n]]", "[[#if\n1\n|\nYES\n|\nNO\n]]"),
        ("[[#if 1 | YES | NO ]", "[1 | YES | NO "),
        ("[[#if 1 | YES | NO ]]]", "YES]"),
        (
            "[[#if 1 | [[span]]A|B[[/span]] | C ]]",
            "[[spanA|B[[/span]] | C ]]",
        ),
        (
            "[[#if 1 | [https://example.com A|B] | C ]]",
            "[https://example.com A|B]",
        ),
        ("[[#if 1 | @@A|B@@ | C ]]", "@@A|B@@"),
        ("[[#if 1 | [!--A|B--] | C ]]", "[!--A|B--]"),
        ("[[#if 1 | [[#if 0 | X | Y ]] | Z ]]", "[[#if 0 | Z ]]"),
        ("[[#ifexpr 1 | [[#expr 2+3]] | NO ]]", "[[#expr 2+3 | NO ]]"),
    ] {
        assert_eq!(
            ftml::resolve_wikidot_parser_functions(source),
            expected,
            "{source:?}",
        );
    }
}

#[test]
fn expression_failures_emit_only_the_evidenced_live_errors() {
    for (source, expected) in [
        (
            "[[#expr missing ]]",
            r#"run-time error: undefined constant "missing""#,
        ),
        (
            "[[#ifexpr missing | YES | NO ]]",
            r#"run-time error: undefined constant "missing""#,
        ),
        (
            "[[#expr 1 + ]]",
            r#"run-time error: too few parameters for operator "+" (2 -> 1)"#,
        ),
        (
            "[[#expr 1,2 ]]",
            "parser error: missing token `(` or misplaced token `,`",
        ),
        (
            "[[#expr abs(1,2) ]]",
            r#"run-time error: too many arguments for function "abs"(1 -> 2)"#,
        ),
    ] {
        assert_eq!(
            ftml::resolve_wikidot_parser_functions(source),
            expected,
            "{source:?}",
        );
    }
}

#[test]
fn unsupported_hash_functions_use_the_bounded_legacy_fallback() {
    for (source, expected) in [
        ("[[#time Y|0]]", "[[#time Y|0]]"),
        (
            "[[#switch x|x=A|#default=B]]",
            "[[#switch x|x=A|#default=B]]",
        ),
        ("[[#ifeq a|a|YES|NO]]", "[[#ifeq a|a|YES|NO]]"),
        ("[[#ifexist page|YES|NO]]", "[[#ifexist page|YES|NO]]"),
        ("[[#unknown 1|YES|NO]]", "[[#unknown 1|YES|NO]]"),
        ("[[# 1|YES|NO]]", "[[# 1|YES|NO]]"),
        ("[[#unknown payload ]]", "[[#unknown payload ]]"),
        (
            "[[#unknown <script>|YES|NO]]",
            "[[#unknown <script>|YES|NO]]",
        ),
        (
            "[[#unknown [[span]]|YES|NO]]",
            "[[#unknown [[span]]|YES|NO]]",
        ),
        ("[[#unknown 1|YES|NO]", "[[#unknown 1|YES|NO]"),
        ("[[# apple]]", "[[# apple]]"),
        ("@@[[#unknown 1|YES|NO]]@@", "@@[[#unknown 1|YES|NO]]@@"),
    ] {
        assert_eq!(
            ftml::resolve_wikidot_parser_functions(source),
            expected,
            "{source:?}",
        );
    }
}

#[test]
fn empty_hash_name_fallback_remains_literal_through_preprocessing() {
    let mut source = "BEGIN|[[# expr 1+2 ]]|END".to_owned();
    ftml::preprocess_for_layout(&mut source, Layout::Wikidot);
    assert_eq!(source, "BEGIN|[[# expr 1+2 ]]|END");
}

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("parser-functions"),
        category: Some(Cow::Borrowed("test")),
        site: Cow::Borrowed("coverage"),
        title: Cow::Borrowed("Parser Functions"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![Cow::Borrowed("test")],
        language: Cow::Borrowed("en"),
    }
}

fn render(input: &str) -> (String, String) {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = input.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokens = ftml::tokenize(&source);
    let result = ftml::parse(&tokens, &page_info, &settings);
    let (tree, errors) = result.into();
    assert!(errors.is_empty(), "{input:?}: {errors:#?}");
    (
        TextRender.render(&tree, &page_info, &settings),
        HtmlRender.render(&tree, &page_info, &settings).body,
    )
}

fn render_text_with_recovery(input: &str) -> String {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = input.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokens = ftml::tokenize(&source);
    let (tree, _) = ftml::parse(&tokens, &page_info, &settings).into();
    TextRender.render(&tree, &page_info, &settings)
}

#[test]
fn complete_live_matrix_matches_parser_function_owned_acceptance() {
    let matrix: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/parser-functions-top-level-live-20260730.json"
    ))
    .expect("live parser-function matrix is valid JSON");
    let evidence = &matrix["evidence"];
    assert_eq!(
        evidence["cases_sha256"],
        "92dca9b5e9af8535c0317e218209ad915ef139a4234b7d43dc7e7b2dc0056229"
    );
    assert_eq!(
        evidence["live_sha256"],
        "b2d92f3af0a7f23437debf457435bada673807b8239cf398267f068895f6c2a3"
    );
    assert_eq!(
        evidence["pinned_ftml_sha256"],
        "e635f983158d604f19043a9a76f5d40b8547d09b65106db9b2d3ba911c0fa7b1"
    );
    assert_eq!(
        evidence["identity_sha256"],
        "04050b04302ba65398502a1ba8773f28a4dd7b23f02276c94603b24eabdd4503"
    );

    let cases = matrix["cases"]
        .as_array()
        .expect("matrix cases are an array");
    assert_eq!(cases.len(), 162);
    let mut ids = HashSet::new();
    let mut rendered = 0usize;
    let mut external_owners = 0usize;

    for case in cases {
        let case_id = case["case_id"].as_str().expect("case has an ID");
        assert!(ids.insert(case_id), "duplicate case ID {case_id}");
        let source = case["source"].as_str().expect("case has source");
        let expected = if case_id == "scout-parserfn-top-expr-docs" {
            "BEGIN|-3.333333333333|END"
        } else if matches!(
            case_id,
            "scout-parserfn-top-context-time-html"
                | "scout-parserfn-top-context-time-span-attr"
        ) {
            source
        } else {
            case["expected"].as_str().expect("case has expected output")
        };
        for hash_key in ["source_sha256", "live_html_sha256"] {
            assert_eq!(
                case[hash_key]
                    .as_str()
                    .expect("case has an evidence hash")
                    .len(),
                64,
                "{case_id}: {hash_key}",
            );
        }

        match case["assertion"].as_str() {
            Some("rendered_text") => {
                rendered += 1;
                assert_eq!(render_text_with_recovery(source), expected, "{case_id}");
            }
            Some("preprocessed_source") => {
                external_owners += 1;
                assert!(case["remaining_owner"].is_string(), "{case_id}");
                let mut actual = source.to_owned();
                ftml::preprocess_for_layout(&mut actual, Layout::Wikidot);
                assert_eq!(actual, expected, "{case_id}");
            }
            assertion => panic!("{case_id}: unknown assertion {assertion:?}"),
        }
    }

    assert_eq!(rendered, 157);
    assert_eq!(external_owners, 5);
    assert_eq!(
        cases
            .iter()
            .filter(|case| !case["case_id"].as_str().unwrap().contains("-context-"))
            .count(),
        102,
    );
}

#[test]
fn expression_prefix_matrix_matches_saved_wikidot() {
    let (text, html) = render("OMEGA_ROOT=[[#expr 7*6 ]]\nOMEGA_AFTER");
    assert!(text.contains("OMEGA_ROOT=42"), "{text}");
    assert!(text.contains("OMEGA_AFTER"), "{text}");
    assert!(!html.contains("[[#expr"), "{html}");

    for input in [
        ">[[#expr 7*6 ]]OMEGA_TIGHT_D1\nOMEGA_AFTER",
        ">>[[#expr 7*6 ]]OMEGA_TIGHT_D2\nOMEGA_AFTER",
    ] {
        let (text, html) = render(input);
        assert!(!text.contains("OMEGA_TIGHT"), "{input:?}: {text}");
        assert_eq!(text, "OMEGA_AFTER", "{input:?}: {text}");
        assert!(!html.contains("<blockquote>"), "{input:?}: {html}");
    }

    let (text, html) = render("> [[#expr 7*6 ]] OMEGA_SPACED_D1\nOMEGA_AFTER");
    assert!(text.contains("42 OMEGA_SPACED_D1"), "{text}");
    assert!(text.contains("OMEGA_AFTER"), "{text}");
    assert_eq!(html.matches("<blockquote>").count(), 1, "{html}");

    let (text, html) = render(">> [[#expr 7*6 ]] OMEGA_SPACED_D2\nOMEGA_AFTER");
    assert!(text.contains("42 OMEGA_SPACED_D2"), "{text}");
    assert_eq!(html.matches("<blockquote>").count(), 2, "{html}");

    let (text, html) = render("> > [[#expr 7*6 ]] OMEGA_SPACED_INNER\nOMEGA_AFTER");
    assert!(text.contains("> 42 OMEGA_SPACED_INNER"), "{text}");
    assert_eq!(html.matches("<blockquote>").count(), 1, "{html}");
    assert!(html.contains("&gt; 42 OMEGA_SPACED_INNER"), "{html}");
}

#[test]
fn document_leading_indented_quote_remains_literal_after_parser_functions() {
    let (text, html) =
        render("\n\t  > [[#expr 7*6 ]] OMEGA_FIRST\n  > OMEGA_SECOND\nOMEGA_AFTER");
    assert!(text.contains("42 OMEGA_FIRST"), "{text}");
    assert!(text.contains("> OMEGA_SECOND"), "{text}");
    assert!(text.contains("OMEGA_AFTER"), "{text}");
    assert_eq!(html.matches("<blockquote>").count(), 0, "{html}");
    assert!(html.contains("&gt; 42 OMEGA_FIRST"), "{html}");
    assert!(html.contains("&gt; OMEGA_SECOND"), "{html}");

    let (text, html) = render("[!-- comment --]\n  > [[#expr 7*6 ]] OMEGA_AFTER_COMMENT");
    assert!(text.contains("> 42 OMEGA_AFTER_COMMENT"), "{text}");
    assert!(!html.contains("<blockquote>"), "{html}");
}

#[test]
fn ifexpr_prefix_matrix_selects_only_the_live_branch() {
    let (text, html) = render(concat!(
        "> [[#ifexpr 3>2 | OMEGA_TRUE | OMEGA_FALSE ]]\n",
        "> [[#ifexpr 2>3 | OMEGA_HIDDEN | OMEGA_SELECTED ]]\n",
        "OMEGA_AFTER",
    ));
    assert!(text.contains("OMEGA_TRUE"), "{text}");
    assert!(text.contains("OMEGA_SELECTED"), "{text}");
    assert!(!text.contains("OMEGA_FALSE"), "{text}");
    assert!(!text.contains("OMEGA_HIDDEN"), "{text}");
    assert!(text.contains("OMEGA_AFTER"), "{text}");
    assert_eq!(html.matches("<blockquote>").count(), 1, "{html}");

    let (text, html) =
        render(">[[#ifexpr 3>2 | OMEGA_TIGHT_TRUE | OMEGA_TIGHT_FALSE ]]\nOMEGA_AFTER");
    assert_eq!(text, "OMEGA_AFTER", "{text}");
    assert!(!html.contains("OMEGA_TIGHT"), "{html}");
}

#[test]
fn simple_if_prefix_matrix_selects_only_the_live_branch() {
    let (text, html) = render(concat!(
        "[[#if 1 | OMEGA_ROOT_TRUE | OMEGA_ROOT_FALSE ]]\n",
        "[[#if 0 | OMEGA_ZERO_TRUE | OMEGA_ZERO_FALSE ]]\n",
        "> [[#if 1 | OMEGA_QUOTED_TRUE | OMEGA_QUOTED_FALSE ]]\n",
        "OMEGA_AFTER",
    ));
    for selected in ["OMEGA_ROOT_TRUE", "OMEGA_ZERO_FALSE", "OMEGA_QUOTED_TRUE"] {
        assert!(text.contains(selected), "{selected}: {text}");
    }
    for hidden in ["OMEGA_ROOT_FALSE", "OMEGA_ZERO_TRUE", "OMEGA_QUOTED_FALSE"] {
        assert!(!text.contains(hidden), "{hidden}: {text}");
    }
    assert!(text.contains("OMEGA_AFTER"), "{text}");
    assert_eq!(html.matches("<blockquote>").count(), 1, "{html}");

    let (text, html) =
        render(">[[#if 1 | OMEGA_TIGHT_TRUE | OMEGA_TIGHT_FALSE ]]\nOMEGA_AFTER");
    assert_eq!(text, "OMEGA_AFTER", "{text}");
    assert!(!html.contains("OMEGA_TIGHT"), "{html}");
}

#[test]
fn simple_if_string_truthiness_matches_saved_wikidot() {
    let (text, html) = render(concat!(
        "[[#if aroace | OMEGA_STRING_TRUE | OMEGA_STRING_FALSE ]]\n",
        "[[#if {$code} | OMEGA_PLACEHOLDER_TRUE | OMEGA_PLACEHOLDER_FALSE ]]\n",
        "[[#if  | OMEGA_EMPTY_TRUE | OMEGA_EMPTY_FALSE ]]\n",
        "OMEGA_AFTER",
    ));

    for selected in [
        "OMEGA_STRING_TRUE",
        "OMEGA_PLACEHOLDER_TRUE",
        "OMEGA_EMPTY_FALSE",
        "OMEGA_AFTER",
    ] {
        assert!(text.contains(selected), "{selected}: {text}");
    }
    for hidden in [
        "OMEGA_STRING_FALSE",
        "OMEGA_PLACEHOLDER_FALSE",
        "OMEGA_EMPTY_TRUE",
    ] {
        assert!(!text.contains(hidden), "{hidden}: {text}");
    }
    assert!(!html.contains("[[#if"), "{html}");
}

#[test]
fn parser_functions_generate_comment_delimiters_before_comment_parsing() {
    let (text, html) = render(concat!(
        "[!-- [[#if aroace | --] |  ]]OMEGA_TRUE[!-- --]\n",
        "[!-- [[#if 0 | --] |  ]]OMEGA_FALSE[!-- --]\n",
        "[!-- [[#expr 1+1]] OMEGA_COMMENT --]\n",
        "OMEGA_AFTER",
    ));

    assert!(text.contains("OMEGA_TRUE"), "{text}");
    assert!(text.contains("OMEGA_AFTER"), "{text}");
    for hidden in ["OMEGA_FALSE", "OMEGA_COMMENT", "[[#expr", "2 OMEGA_COMMENT"] {
        assert!(!text.contains(hidden), "{hidden}: {text}");
        assert!(!html.contains(hidden), "{hidden}: {html}");
    }
}

#[test]
fn literal_and_runtime_error_outputs_survive_the_full_pipeline() {
    let (text, html) = render(concat!(
        "> @@OMEGA_RAW [[#ifexpr 3>2 | OMEGA_RAW_TRUE | OMEGA_RAW_FALSE ]]@@\n",
        "> OMEGA_BAD=[[#expr unknown(1) ]]\n",
        "> OMEGA_DIV=[[#expr 1/0 ]]\n",
        "> OMEGA_MOD=[[#expr 1%0 ]]\n",
        "OMEGA_AFTER",
    ));
    assert!(
        text.contains("[[#ifexpr 3>2 | OMEGA_RAW_TRUE | OMEGA_RAW_FALSE ]]"),
        "{text}",
    );
    assert!(
        text.contains(r#"OMEGA_BAD=run-time error: undefined function "unknown""#),
        "{text}",
    );
    assert!(
        text.contains("OMEGA_DIV=run-time error: division by zero"),
        "{text}",
    );
    assert!(
        text.contains("OMEGA_MOD=run-time error: rest-division by zero"),
        "{text}",
    );
    assert!(text.contains("OMEGA_AFTER"), "{text}");
    assert_eq!(html.matches("<blockquote>").count(), 1, "{html}");
}
