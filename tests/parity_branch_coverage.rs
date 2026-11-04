use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

const REFERENCE_FILES: &[&str] = &[
    "tests/fixtures/wikidot-parity/references-20260815-40.jsonl",
    "tests/fixtures/wikidot-parity/references-20260815-41.jsonl",
    "tests/fixtures/wikidot-parity/references-20260815-42.jsonl",
    "tests/fixtures/wikidot-parity/references-20260815-43.jsonl",
    "tests/fixtures/wikidot-parity/references-20260815-44.jsonl",
    "tests/fixtures/wikidot-parity/references-20260815-45.jsonl",
    "tests/fixtures/wikidot-parity/references-20260815-46.jsonl",
    "tests/fixtures/wikidot-parity/references-20260815-47.jsonl",
    "tests/fixtures/wikidot-parity/references-20260815-48.jsonl",
    "tests/fixtures/wikidot-parity/references-20260816-01.jsonl",
    "tests/fixtures/wikidot-parity/references-20260816-02.jsonl",
    "tests/fixtures/wikidot-parity/references-20260816-03.jsonl",
];

const REVIEWED_BRANCH_WITNESSES: &[&str] = &[
    "tests--fixtures--parity-gaps--comment-anchor-label-owner",
    "tests--fixtures--parity-gaps--comment-close-exact",
    "tests--fixtures--parity-gaps--comment-close-extended",
    "tests--fixtures--parity-gaps--comment-nonadjacent-close",
    "tests--fixtures--parity-gaps--comment-nested-first-close",
    "tests--fixtures--parity-gaps--comment-unclosed",
    "tests--fixtures--parity-gaps--comment-unmatched-close",
    "tests--fixtures--parity-gaps--comment-block-name-owner",
    "tests--fixtures--parity-gaps--comment-block-include-name",
    "tests--fixtures--parity-gaps--comment-block-closer-name-owner",
    "tests--fixtures--parity-gaps--comment-attribute-key-owner",
    "tests--fixtures--parity-gaps--comment-attribute-cross-owner",
    "tests--fixtures--parity-gaps--comment-span-value",
    "tests--fixtures--parity-gaps--comment-attribute-span-edge-pair",
    "tests--fixtures--parity-gaps--comment-attribute-span-title",
    "tests--fixtures--parity-gaps--comment-attribute-code-type",
    "tests--fixtures--parity-gaps--comment-attribute-collapsible-show",
    "tests--fixtures--parity-gaps--comment-attribute-div-class",
    "tests--fixtures--parity-gaps--comment-attribute-image-link",
    "tests--fixtures--parity-gaps--comment-date-value-owner",
    "tests--fixtures--parity-gaps--comment-iframe-url-owner",
    "tests--fixtures--parity-gaps--comment-code-body-owner",
    "tests--fixtures--parity-gaps--comment-raw-owner",
    "tests--fixtures--parity-gaps--comment-html-body-owner",
    "tests--fixtures--parity-gaps--comment-hidden-block-owner",
    "tests--fixtures--parity-gaps--comment-style-javascript-seam",
    "tests--fixtures--parity-gaps--comment-style-unicode-javascript-seam",
    "tests--fixtures--parity-gaps--comment-parserfn-name-owner",
    "tests--fixtures--parity-gaps--comment-parserfn-ifexpr-name-owner",
    "tests--fixtures--parity-gaps--comment-parserfn-expr-name-owner",
    "tests--fixtures--parity-gaps--comment-parserfn-condition-owner",
    "tests--fixtures--parity-gaps--comment-parserfn-true-branch-owner",
    "tests--fixtures--parity-gaps--comment-parserfn-opener-owner",
    "tests--fixtures--parity-gaps--comment-footnote-prefix-owner",
    "tests--fixtures--parity-gaps--comment-line-owner-code-prefix",
    "tests--fixtures--parity-gaps--comment-math-close-join",
    "tests--fixtures--parity-gaps--comment-separator-join",
    "tests--fixtures--parity-gaps--comment-variable-token",
    "tests--fixtures--parity-gaps--comment-variable-name-owner",
    "tests--fixtures--parity-gaps--comment-autourl-joined",
    "tests--fixtures--parity-gaps--comment-autourl-split-scheme",
    "tests--fixtures--parity-gaps--comment-autourl-split-ftp-scheme",
    "tests--fixtures--parity-gaps--comment-autourl-split-mailto-scheme",
    "tests--fixtures--parity-gaps--comment-split-safe-scheme",
    "tests--fixtures--parity-gaps--comment-split-javascript-scheme",
    "tests--fixtures--parity-gaps--comment-single-link-data-scheme",
    "tests--fixtures--parity-gaps--comment-image-source",
    "tests--fixtures--parity-gaps--comment-image-safe-scheme",
    "tests--fixtures--parity-gaps--comment-image-scheme-owner",
    "tests--fixtures--parity-gaps--comment-triple-safe-scheme",
    "tests--fixtures--parity-gaps--comment-triple-fields-owner",
    "tests--fixtures--parity-gaps--comment-email-token",
    "tests--fixtures--parity-gaps--comment-delimiter-block-open",
    "tests--fixtures--parity-gaps--comment-delimiter-block-close-open",
    "tests--fixtures--parity-gaps--comment-delimiter-block-star",
    "tests--fixtures--parity-gaps--comment-delimiter-bold",
    "tests--fixtures--parity-gaps--comment-delimiter-italics",
    "tests--fixtures--parity-gaps--comment-delimiter-underline",
    "tests--fixtures--parity-gaps--comment-delimiter-subscript",
    "tests--fixtures--parity-gaps--comment-delimiter-superscript",
    "tests--fixtures--parity-gaps--comment-delimiter-monospace",
    "tests--fixtures--parity-gaps--comment-delimiter-color",
    "tests--fixtures--parity-gaps--comment-delimiter-double-dash",
    "tests--fixtures--parity-gaps--comment-delimiter-triple-dash",
    "tests--fixtures--parity-gaps--comment-delimiter-raw",
    "tests--fixtures--parity-gaps--comment-delimiter-alt-raw-open",
    "tests--fixtures--parity-gaps--comment-delimiter-math-open",
    "tests--fixtures--parity-gaps--comment-delimiter-heading",
    "tests--fixtures--parity-gaps--comment-delimiter-heading-six",
    "tests--fixtures--parity-gaps--comment-delimiter-heading-seven",
    "tests--fixtures--parity-gaps--comment-delimiter-table",
    "tests--fixtures--parity-gaps--comment-delimiter-clearfloat",
    "tests--fixtures--parity-gaps--comment-delimiter-horizontal-rule",
    "tests--fixtures--parity-gaps--comment-delimiter-quote-run",
    "tests--fixtures--parity-gaps--comment-join-paragraph-break",
    "tests--fixtures--parity-gaps--comment-join-line-continuation",
    "tests--fixtures--parity-gaps--comment-crlf-join",
    "tests--fixtures--parity-gaps--comment-link-tab-separator-owner",
    "tests--fixtures--parity-gaps--comment-list-indent-space-owner",
    "tests--fixtures--parity-gaps--comment-list-indent-tab-owner",
    "tests--fixtures--parity-gaps--comment-prefix-heading",
    "tests--fixtures--parity-gaps--comment-prefix-blockquote",
    "tests--fixtures--parity-gaps--comment-prefix-list-bullet",
    "tests--fixtures--parity-gaps--comment-prefix-list-numbered",
    "tests--fixtures--parity-gaps--comment-prefix-table",
    "tests--fixtures--parity-gaps--comment-prefix-div",
    "tests--fixtures--parity-gaps--comment-prefix-center",
    "tests--fixtures--parity-gaps--comment-prefix-definition-list",
    "tests--fixtures--parity-gaps--comment-prefix-clearfloat",
    "tests--fixtures--parity-gaps--comment-table-row-owner",
    "tests--fixtures--parity-gaps--comment-table-space-trim",
    "tests--fixtures--parity-gaps--content-separator-under-threshold",
    "tests--fixtures--parity-gaps--content-separator-run-five",
    "tests--fixtures--parity-gaps--content-separator-run-six",
    "tests--fixtures--parity-gaps--content-separator-run-nine",
    "tests--fixtures--parity-gaps--content-separator-internal-run-five",
    "tests--fixtures--parity-gaps--content-separator-ascii-padding",
    "tests--fixtures--parity-gaps--content-separator-internal-leading-space",
    "tests--fixtures--parity-gaps--content-separator-internal-leading-tab",
    "tests--fixtures--parity-gaps--content-separator-leading-nbsp",
    "tests--fixtures--parity-gaps--content-separator-leading-figure-space",
    "tests--fixtures--parity-gaps--content-separator-trailing-space",
    "tests--fixtures--parity-gaps--content-separator-trailing-tab",
    "tests--fixtures--parity-gaps--content-separator-trailing-nbsp",
    "tests--fixtures--parity-gaps--content-separator-trailing-em-space",
    "tests--fixtures--parity-gaps--content-separator-trailing-form-feed",
    "tests--fixtures--parity-gaps--content-separator-discarded-control",
    "tests--fixtures--parity-gaps--content-separator-code-owner",
    "tests--fixtures--parity-gaps--content-separator-raw-owner",
    "tests--fixtures--parity-gaps--content-separator-list-owner",
    "tests--fixtures--parity-gaps--content-separator-quote-owner",
    "tests--fixtures--parity-gaps--content-separator-table-owner",
    "tests--fixtures--parity-gaps--content-separator-span-crossing",
    "tests--fixtures--parity-gaps--content-separator-escaped",
];

const REVIEWED_BRANCH_FAMILIES: &[&str] = &[
    "comment-anchor",
    "comment-attribute",
    "comment-autourl",
    "comment-block",
    "comment-close",
    "comment-code",
    "comment-crlf",
    "comment-date",
    "comment-delimiter",
    "comment-email",
    "comment-footnote",
    "comment-hidden",
    "comment-html",
    "comment-iframe",
    "comment-image",
    "comment-join",
    "comment-line",
    "comment-link",
    "comment-list",
    "comment-math",
    "comment-nested",
    "comment-nonadjacent",
    "comment-parserfn",
    "comment-prefix",
    "comment-raw",
    "comment-separator",
    "comment-single",
    "comment-span",
    "comment-split",
    "comment-style",
    "comment-table",
    "comment-triple",
    "comment-unclosed",
    "comment-unmatched",
    "comment-variable",
    "content-separator",
];

fn branch_family(case_id: &str) -> &str {
    let slug = case_id.rsplit("--").next().unwrap_or(case_id);
    match slug.match_indices('-').nth(1) {
        Some((index, _)) => &slug[..index],
        None => slug,
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_jsonl(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("valid JSONL row"))
        .collect()
}

fn snapshot_sha256(path: &Path) -> String {
    let mut bytes = fs::read(path).expect("reviewed match snapshot");
    if bytes.ends_with(b"\n") {
        bytes.pop();
    }
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(&mut output, "{byte:02x}").expect("write SHA-256");
            output
        })
}

#[test]
fn high_risk_comment_and_separator_branches_have_reviewed_live_evidence() {
    let root = root();
    let cases = read_jsonl(&root.join("tests/fixtures/wikidot-parity/cases.jsonl"));
    let cases_by_id: HashMap<&str, &Value> = cases
        .iter()
        .map(|case| (case["case_id"].as_str().expect("case id"), case))
        .collect();
    let campaign_ids: HashSet<&str> = cases
        .iter()
        .filter_map(|case| {
            let path = case["source_origin"]["path"].as_str()?;
            (path.starts_with("tests/fixtures/parity-gaps/comment-")
                || path.starts_with("tests/fixtures/parity-gaps/content-separator-"))
            .then(|| case["case_id"].as_str().expect("campaign case id"))
        })
        .collect();
    assert_eq!(campaign_ids.len(), 153, "reviewed campaign case count");

    let bindings: Value = serde_json::from_str(
        &fs::read_to_string(root.join("tests/fixtures/wikidot-parity/bindings.json"))
            .expect("binding ledger"),
    )
    .expect("valid binding ledger");
    let bindings_by_id: HashMap<&str, &Value> = bindings["bindings"]
        .as_array()
        .expect("binding rows")
        .iter()
        .map(|binding| {
            (
                binding["case_id"].as_str().expect("binding case id"),
                binding,
            )
        })
        .collect();

    let mut current_refs: HashMap<String, Value> = HashMap::new();
    for relative in REFERENCE_FILES {
        for reference in read_jsonl(&root.join(relative)) {
            let case_id = reference["syntax_case"]["case_id"]
                .as_str()
                .expect("reference case id")
                .to_owned();
            if !campaign_ids.contains(case_id.as_str()) {
                continue;
            }
            let case = cases_by_id[case_id.as_str()];
            if reference["source_sha256"] != case["source_sha256"] {
                continue;
            }
            assert!(
                current_refs.insert(case_id.clone(), reference).is_none(),
                "duplicate current reference for {case_id}"
            );
        }
    }
    assert_eq!(
        current_refs.len(),
        campaign_ids.len(),
        "every campaign case needs one current frozen reference"
    );
    for case_id in &campaign_ids {
        assert!(
            current_refs.contains_key(*case_id),
            "missing current live evidence for campaign case {case_id}"
        );
    }
    for case_id in REVIEWED_BRANCH_WITNESSES {
        assert!(
            campaign_ids.contains(case_id),
            "reviewed branch witness is not a campaign case: {case_id}"
        );
        assert!(
            current_refs.contains_key(*case_id),
            "reviewed parser/preprocessor branch lost its live witness: {case_id}"
        );
    }
    for case_id in &campaign_ids {
        let family = branch_family(case_id);
        assert!(
            REVIEWED_BRANCH_FAMILIES.contains(&family),
            "campaign case belongs to an unreviewed branch family: {case_id} ({family})"
        );
    }
    for family in REVIEWED_BRANCH_FAMILIES {
        assert!(
            campaign_ids
                .iter()
                .any(|case_id| branch_family(case_id) == *family),
            "reviewed branch family has no campaign cases: {family}"
        );
        assert!(
            REVIEWED_BRANCH_WITNESSES
                .iter()
                .any(|case_id| branch_family(case_id) == *family),
            "reviewed branch family has no live witness: {family}"
        );
    }

    let mut matches = 0;
    let mut mismatches = 0;
    let mut unresolved = 0;
    let mut caller_runtime = 0;
    let mut comparison_normalization = 0;
    for (case_id, reference) in &current_refs {
        let case = cases_by_id[case_id.as_str()];
        let binding = bindings_by_id
            .get(case_id.as_str())
            .unwrap_or_else(|| panic!("missing reviewed binding for {case_id}"));
        assert_eq!(binding["source_sha256"], case["source_sha256"]);
        assert_eq!(binding["source_sha256"], reference["source_sha256"]);
        assert_eq!(binding["wikidot_html_sha256"], reference["raw_html_sha256"]);

        let source_path = case["source_origin"]["path"].as_str().expect("source path");
        let snapshot = root
            .join(source_path)
            .parent()
            .expect("fixture parent")
            .join("wikidot.html");
        match binding["status"].as_str().expect("binding status") {
            "match" => {
                matches += 1;
                assert!(snapshot.is_file(), "reviewed match needs {snapshot:?}");
                assert_eq!(
                    snapshot_sha256(&snapshot),
                    binding["ftml_html_sha256"]
                        .as_str()
                        .expect("FTML HTML hash"),
                    "derived snapshot differs from the bound FTML output for {case_id}"
                );
            }
            "mismatch" => {
                mismatches += 1;
                assert!(
                    !snapshot.exists(),
                    "mismatch must not gain a passing snapshot: {case_id}"
                );
                match binding["disposition"].as_str() {
                    Some("unresolved") => unresolved += 1,
                    Some("caller-runtime") => caller_runtime += 1,
                    Some("comparison-normalization") => comparison_normalization += 1,
                    disposition => panic!(
                        "new mismatch lacks a reviewed disposition for {case_id}: {disposition:?}"
                    ),
                }
            }
            status => panic!("unexpected binding status {status} for {case_id}"),
        }
    }

    assert_eq!(matches, 150, "reviewed matching campaign cases");
    assert_eq!(mismatches, 3, "reviewed mismatching campaign cases");
    assert_eq!(unresolved, 0, "active FTML behavior investigations");
    assert_eq!(caller_runtime, 3, "caller-owned campaign differences");
    assert_eq!(
        comparison_normalization, 0,
        "campaign normalization differences"
    );
}
