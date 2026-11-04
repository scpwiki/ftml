use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::Render;
use ftml::render::html::HtmlRender;
use ftml::render::text::TextRender;
use ftml::settings::{WikitextMode, WikitextSettings};
use proptest::prelude::*;
use regex::Regex;
use serde_json::Value;
use std::borrow::Cow;
use std::fs;
use std::path::Path;

const ROBUSTNESS_MATRIX_SCHEMA: &str = "ftml.wikidot_parity.robustness_matrix.v1";

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("unicode-robustness"),
        category: None,
        site: Cow::Borrowed("sandbox"),
        title: Cow::Borrowed("Unicode robustness"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn structured_source() -> impl Strategy<Value = String> {
    let text = proptest::collection::vec(
        prop_oneof![
            Just('A'),
            Just('é'),
            Just('漢'),
            Just('🦀'),
            Just('\u{0301}'),
            Just('\u{00A0}'),
            Just('\u{2007}'),
            Just('\u{200D}'),
            Just('\u{FE0F}'),
            Just('\u{2028}'),
            Just('\u{2029}'),
            Just('\u{000B}'),
            Just('\u{000C}'),
            Just('\0'),
            Just('['),
            Just(']'),
            Just('|'),
            Just('>'),
            Just('<'),
            Just('='),
            Just('"'),
            Just('\''),
            Just('\\'),
            Just(' '),
            Just('\t'),
        ],
        0..40,
    )
    .prop_map(|characters| characters.into_iter().collect::<String>());
    let eol = prop_oneof![
        Just("\n".to_owned()),
        Just("\r\n".to_owned()),
        Just("\r".to_owned()),
    ];
    let trailing = prop_oneof![
        Just(String::new()),
        Just(" ".to_owned()),
        Just("\t".to_owned()),
        Just(" \t ".to_owned()),
    ];

    (0_u8..18, text, eol, trailing).prop_map(|(shape, text, eol, trailing)| {
        match shape {
            0 => format!(
                "[[collapsible show=\"report\"]]{eol}> **Author:** {text}{eol}> End log.[[/collapsible]]{trailing}{eol}after"
            ),
            1 => format!(
                "[[collapsible show=\"report\"]]{eol}> {text}{eol}> still quoted{eol}[[/collapsible]]"
            ),
            2 => format!("[[div class=\"probe\"]]{eol}{text}{eol}[[/div]]"),
            3 => format!("[[span data-x=\"{text}\"]]body[[/span]]"),
            4 => format!("[[[target|{text}]]]"),
            5 => format!("[https://example.com/{text} label]"),
            6 => format!("[[image https://example.com/a.png link=\"https://example.com/{text}\"]]"),
            7 => format!("[[code]]{eol}{text}{eol}[[/code]]"),
            8 => format!("@@{text}@@"),
            9 => format!("[[math]]{eol}{text}{eol}[[/math]]"),
            10 => format!("[[#if 1 | {text} | fallback ]]"),
            11 => format!("[[#expr 1+2]] {text}"),
            12 => format!("|| {text} ||{eol}|| next ||"),
            13 => format!("> {text}{eol}>> nested{eol}outside"),
            14 => format!("* {text}{eol}** nested{eol}after"),
            15 => format!("[[span da[!--x--]ta-owned=\"yes\"]]{text}[[/span]]"),
            16 => format!("**bold //italic {text}** tail//"),
            _ => format!("A{eol}===={eol}{text}{eol}B"),
        }
    })
}

fn exercise_public_pipeline(source: &str, layout: Layout) {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut preprocessed = source.to_owned();
    ftml::preprocess_for_layout(&mut preprocessed, layout);
    let tokenization = ftml::tokenize(&preprocessed);
    let (tree, _errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let _html = HtmlRender.render(&tree, &page_info, &settings);
    let _text = TextRender.render(&tree, &page_info, &settings);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1_024))]

    #[test]
    fn structured_unicode_sources_never_panic_in_the_public_pipeline(
        source in structured_source(),
    ) {
        exercise_public_pipeline(&source, Layout::Wikidot);
        exercise_public_pipeline(&source, Layout::Wikijump);
    }
}

#[test]
fn root_collapsible_multibyte_boundary_matrix_never_panics() {
    for body in [
        "ASCII",
        "San José Public Library",
        "漢字の図書館",
        "crab 🦀 library",
        "e\u{301} combining",
    ] {
        for trailing in ["", " ", "\t", " \t "] {
            for eol in ["\n", "\r\n", "\r"] {
                let source = format!(
                    "[[collapsible show=\"report\"]]{eol}> **Author:** {body}{eol}> End log.[[/collapsible]]{trailing}{eol}after"
                );
                exercise_public_pipeline(&source, Layout::Wikidot);
            }
        }
    }
}

#[test]
fn malformed_getattrs_delimiter_after_multibyte_value_never_panics() {
    for value in ["é==", "漢==", "🦀==", "e\u{301}=="] {
        let source = format!(r#"[[span data-x="{value}"]]body[[/span]]"#);
        exercise_public_pipeline(&source, Layout::Wikidot);
        exercise_public_pipeline(&source, Layout::Wikijump);
    }
}

#[test]
fn stable_corpus_sources_survive_rotating_unicode_boundary_mutations() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut paths = Vec::new();
    for tree in ["test", "tests/fixtures"] {
        collect_ftml_sources(&root.join(tree), &mut paths);
    }
    paths.sort();

    let mut exercised = 0;
    for (index, path) in paths.iter().enumerate() {
        let source = fs::read_to_string(path).expect("stable corpus source");
        let mutated = mutate_stable_source(&source, index);
        exercise_public_pipeline(&mutated, Layout::Wikidot);
        exercise_public_pipeline(&mutated, Layout::Wikijump);
        exercised += 1;
    }

    assert!(exercised > 0, "fixture corpus must not be empty");
}

fn collect_ftml_sources(dir: &Path, paths: &mut Vec<std::path::PathBuf>) {
    let entries = fs::read_dir(dir).expect("read corpus directory");
    for entry in entries {
        let path = entry.expect("read corpus entry").path();
        if path.is_dir() {
            collect_ftml_sources(&path, paths);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "ftml")
        {
            paths.push(path);
        }
    }
}

#[test]
fn declared_robustness_matrix_survives_the_public_pipeline() {
    let matrix = load_robustness_matrix();
    assert_eq!(matrix["schema"], ROBUSTNESS_MATRIX_SCHEMA);
    assert_eq!(
        matrix["layouts"],
        serde_json::json!(["wikidot", "wikijump"]),
        "both public layouts must remain in the robustness contract"
    );
    assert_eq!(
        matrix["pipeline_stages"],
        serde_json::json!([
            "preprocess",
            "tokenize",
            "parse",
            "html-render",
            "text-render"
        ]),
        "the robustness contract must cover the complete public pipeline"
    );
    assert_eq!(
        matrix["stable_source_mutations"],
        serde_json::json!([
            "unicode-prefix",
            "unicode-suffix",
            "unicode-midpoint",
            "crlf"
        ]),
        "stable-corpus mutation classes changed without updating the test"
    );

    let tokens = matrix["unicode_tokens"]
        .as_array()
        .expect("robustness unicode tokens");
    let shapes = matrix["syntax_shapes"]
        .as_array()
        .expect("robustness syntax shapes");
    assert!(tokens.len() >= 13, "Unicode robustness token set regressed");
    assert!(shapes.len() >= 20, "syntax-shape robustness set regressed");

    for shape in shapes {
        let id = shape["id"].as_str().expect("robustness shape id");
        let template = shape["source"].as_str().expect("robustness shape source");
        assert!(template.contains("{u}"), "{id}: missing Unicode slot");
        for token in tokens {
            let token = token.as_str().expect("robustness Unicode token");
            let source = template.replace("{u}", token);
            exercise_public_pipeline(&source, Layout::Wikidot);
            exercise_public_pipeline(&source, Layout::Wikijump);
        }
    }
}

fn load_robustness_matrix() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wikidot-parity/robustness-matrix.json");
    serde_json::from_slice(&fs::read(&path).expect("read robustness matrix"))
        .expect("valid robustness matrix JSON")
}

fn mutate_stable_source(source: &str, index: usize) -> String {
    match index % 4 {
        0 => format!("é{source}"),
        1 => format!("{source}🦀"),
        2 => {
            let midpoint = source.len() / 2;
            let boundary = source
                .char_indices()
                .map(|(offset, _)| offset)
                .find(|offset| *offset >= midpoint)
                .unwrap_or(source.len());
            let mut mutated = source.to_owned();
            mutated.insert(boundary, '漢');
            mutated
        }
        _ => source.replace('\n', "\r\n"),
    }
}

#[test]
fn unchecked_len_derived_utf8_slices_are_absent() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let patterns = [
        Regex::new(
            r"(?:[A-Za-z_][A-Za-z0-9_]*|\))\[[^\[\]]{0,240}\.len\(\)\s*-\s*[^\[\]]{0,240}\]",
        )
            .expect("valid len-subtraction slice regex"),
        Regex::new(r"(?:[A-Za-z_][A-Za-z0-9_]*|\))\[[^\[\]]{0,240}saturating_sub\([^\[\]]{0,120}\)[^\[\]]{0,240}\]")
        .expect("valid saturating-sub slice regex"),
    ];
    let mut violations = Vec::new();
    scan_rust_sources(&root, &mut |path, source| {
        let flattened = source.replace('\n', " ");
        for pattern in &patterns {
            for found in pattern.find_iter(&flattened) {
                violations.push(format!(
                    "{}: {}",
                    path.display(),
                    found
                        .as_str()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            }
        }
    });

    assert!(
        violations.is_empty(),
        "unchecked len-derived indexing can split a UTF-8 code point; use strip_prefix/strip_suffix/str::get or a proven character boundary:\n{}",
        violations.join("\n")
    );
}

fn scan_rust_sources(root: &Path, visit: &mut impl FnMut(&Path, &str)) {
    for entry in fs::read_dir(root)
        .unwrap_or_else(|error| panic!("read {}: {error}", root.display()))
    {
        let entry = entry.expect("read source directory entry");
        let path = entry.path();
        if path.is_dir() {
            scan_rust_sources(&path, visit);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            visit(&path, &source);
        }
    }
}
