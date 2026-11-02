use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::Render;
use ftml::render::html::HtmlRender;
use ftml::settings::{WikitextMode, WikitextSettings};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use time::UtcOffset;
use time::format_description::well_known::Rfc3339;

const CASE_SCHEMA: &str = "wikijump_syntax_differential.live_case.v1";
const REFERENCE_SCHEMA: &str = "wikijump_syntax_differential.wikidot_reference.v1";
const SYNTAX_CASE_SCHEMA: &str = "wikijump_syntax_differential.syntax_case.v1";
const BINDINGS_SCHEMA: &str = "ftml.wikidot_parity.bindings.v1";
const CLASSIFICATION_OVERRIDES_SCHEMA: &str =
    "ftml.wikidot_parity.classification_overrides.v1";
const CALLER_RUNTIME_CONTRACTS_SCHEMA: &str =
    "ftml.wikidot_parity.caller_runtime_contracts.v1";
const COMPARISON_NORMALIZATION_CONTRACTS_SCHEMA: &str =
    "ftml.wikidot_parity.comparison_normalization_contracts.v1";
const WIKIJUMP_FEATURE_CONTRACTS_SCHEMA: &str = "ftml.wikijump_feature_contracts.v2";
const DOWNSTREAM_PAGES_SCHEMA: &str = "ftml.wikijump_downstream_pages.v1";
const ACTIVE_INVESTIGATION_REASON_PREFIX: &str =
    "Active functional investigation: issue #";
const REVIEWED_CONTEXT_FREE_REASON_PREFIX: &str = "reviewed-context-free-";

#[derive(Debug, Deserialize)]
struct LiveCase {
    schema: String,
    case_id: String,
    source: String,
    source_sha256: String,
    source_origin: SourceOrigin,
    execution_class: ExecutionClass,
    page_scope: String,
    reasons: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SourceOrigin {
    repository: String,
    path: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum ExecutionClass {
    SavedPageBatch,
    PagePreviewIsolated,
    WikijumpRuntime,
    NotApplicable,
}

impl ExecutionClass {
    fn is_preview_compatible(self) -> bool {
        matches!(self, Self::SavedPageBatch | Self::PagePreviewIsolated)
    }
}

#[derive(Debug, Deserialize)]
struct WikidotReference {
    schema: String,
    syntax_case: SyntaxCase,
    source_sha256: String,
    captured_at: String,
    provenance: Provenance,
    raw_html: String,
    raw_html_sha256: String,
}

#[derive(Debug, Deserialize)]
struct SyntaxCase {
    schema: String,
    case_id: String,
    source: String,
    title: String,
    wikidot_observation_tier: String,
    local_execution_tier: String,
}

#[derive(Debug, Deserialize)]
struct Provenance {
    site: String,
    site_domain: String,
    module: String,
    wikidot_py_version: String,
    wikidot_py_commit: String,
    requirements_sha256: String,
    authenticated: bool,
    mutated: bool,
}

#[derive(Debug, Deserialize)]
struct BindingsManifest {
    schema: String,
    bindings: Vec<Binding>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassificationOverridesManifest {
    schema: String,
    overrides: Vec<ClassificationOverride>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClassificationOverride {
    path: String,
    source_sha256: String,
    execution_class: ExecutionClass,
    page_scope: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CallerRuntimeContractsManifest {
    schema: String,
    contracts: Vec<CallerRuntimeContract>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CallerRuntimeContract {
    id: String,
    wikijump_test: String,
    cases: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComparisonNormalizationContractsManifest {
    schema: String,
    contracts: Vec<ComparisonNormalizationContract>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComparisonNormalizationContract {
    case_id: String,
    difference_class: String,
    wikijump_feature_id: String,
    review: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WikijumpFeatureContractsManifest {
    schema: String,
    contracts: Vec<WikijumpFeatureContract>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WikijumpFeatureContract {
    id: String,
    wikijump_feature_id: String,
    cases: Vec<String>,
    property_owners: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DownstreamPagesManifest {
    schema: String,
    pages: Vec<DownstreamPage>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DownstreamPage {
    id: String,
    path: String,
    sha256: String,
    site: String,
    page: String,
    title: String,
}

#[derive(Debug, Deserialize)]
struct Binding {
    case_id: String,
    source_sha256: String,
    wikidot_html_sha256: String,
    ftml_html_sha256: String,
    status: BindingStatus,
    checks: BindingChecks,
    disposition: Option<BindingDisposition>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingChecks {
    dom_signature: CheckStatus,
    dom_tree: CheckStatus,
    visible_text: CheckStatus,
}

impl BindingChecks {
    fn all_match(&self) -> bool {
        [self.dom_signature, self.dom_tree, self.visible_text]
            .into_iter()
            .all(|status| status == CheckStatus::Match)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum CheckStatus {
    Match,
    Mismatch,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum BindingStatus {
    Match,
    Mismatch,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum BindingDisposition {
    Unresolved,
    CallerRuntime,
    ComparisonNormalization,
    SecurityBoundary,
}

fn sha256(bytes: impl AsRef<[u8]>) -> String {
    use std::fmt::Write;

    Sha256::digest(bytes.as_ref()).iter().fold(
        String::with_capacity(64),
        |mut output, byte| {
            write!(&mut output, "{byte:02x}").expect("write to String");
            output
        },
    )
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn has_positive_issue_number(reason: &str) -> bool {
    let Some(number) = reason
        .strip_prefix(ACTIVE_INVESTIGATION_REASON_PREFIX)
        .and_then(|number| number.strip_suffix('.'))
    else {
        return false;
    };
    number
        .as_bytes()
        .first()
        .is_some_and(|first| (b'1'..=b'9').contains(first))
        && number.bytes().all(|byte| byte.is_ascii_digit())
}

fn read_jsonl<T: DeserializeOwned>(path: &Path) -> Vec<T> {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str(line).unwrap_or_else(|error| {
                panic!("{}:{}: invalid JSON: {error}", path.display(), index + 1)
            })
        })
        .collect()
}

fn collect_files(
    path: &Path,
    keep: impl Copy + Fn(&Path) -> bool,
    files: &mut Vec<PathBuf>,
) {
    for entry in fs::read_dir(path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
    {
        let path = entry.expect("read directory entry").path();
        if path.is_dir() {
            collect_files(&path, keep, files);
        } else if keep(&path) {
            files.push(path);
        }
    }
}

fn fixture_sources(root: &Path) -> BTreeMap<String, String> {
    let mut files = Vec::new();
    collect_files(
        &root.join("test"),
        |path| path.file_name().is_some_and(|name| name == "input.ftml"),
        &mut files,
    );
    collect_files(
        &root.join("tests/fixtures"),
        |path| {
            path.extension()
                .is_some_and(|extension| extension == "ftml")
        },
        &mut files,
    );
    files
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(root)
                .expect("fixture is inside repository")
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            (relative, source)
        })
        .collect()
}

fn expected_case_id(path: &str) -> String {
    let flattened = path.replace('/', "--");
    flattened
        .strip_suffix("--input.ftml")
        .or_else(|| flattened.strip_suffix("input.ftml"))
        .or_else(|| flattened.strip_suffix(".ftml"))
        .expect("fixture path ends in .ftml")
        .to_owned()
}

fn render(case: &LiveCase, reference: &WikidotReference) -> String {
    let page_info = PageInfo {
        page: Cow::Borrowed(""),
        category: None,
        site: Cow::Borrowed(&reference.provenance.site),
        title: Cow::Borrowed(&case.case_id),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    };
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = case.source.clone();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, _) = ftml::parse(&tokenization, &page_info, &settings).into();
    HtmlRender.render(&tree, &page_info, &settings).body
}

#[test]
fn frozen_wikidot_parity_artifacts_are_complete_and_current() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let artifact_dir = root.join("tests/fixtures/wikidot-parity");
    let cases_path = artifact_dir.join("cases.jsonl");
    let bindings_path = artifact_dir.join("bindings.json");
    let classification_overrides_path =
        artifact_dir.join("classification-overrides.json");
    let caller_runtime_contracts_path =
        artifact_dir.join("caller-runtime-contracts.json");
    let comparison_normalization_contracts_path =
        artifact_dir.join("comparison-normalization-contracts.json");
    let wikijump_feature_contracts_path =
        artifact_dir.join("wikijump-feature-contracts.json");
    let downstream_pages_path = artifact_dir.join("downstream-pages.json");
    let cases: Vec<LiveCase> = read_jsonl(&cases_path);
    let manifest: BindingsManifest =
        serde_json::from_slice(&fs::read(&bindings_path).expect("read parity bindings"))
            .expect("bindings.json has valid schema fields");
    let classification_overrides: ClassificationOverridesManifest =
        serde_json::from_slice(
            &fs::read(&classification_overrides_path)
                .expect("read classification overrides"),
        )
        .expect("classification-overrides.json has valid schema fields");
    let caller_runtime_contracts: CallerRuntimeContractsManifest =
        serde_json::from_slice(
            &fs::read(&caller_runtime_contracts_path)
                .expect("read caller-runtime contracts"),
        )
        .expect("caller-runtime-contracts.json has valid schema fields");
    let comparison_normalization_contracts: ComparisonNormalizationContractsManifest =
        serde_json::from_slice(
            &fs::read(&comparison_normalization_contracts_path)
                .expect("read comparison-normalization contracts"),
        )
        .expect("comparison-normalization-contracts.json has valid schema fields");
    let wikijump_feature_contracts: WikijumpFeatureContractsManifest =
        serde_json::from_slice(
            &fs::read(&wikijump_feature_contracts_path)
                .expect("read Wikijump feature contracts"),
        )
        .expect("wikijump-feature-contracts.json has valid schema fields");
    let downstream_pages: DownstreamPagesManifest = serde_json::from_slice(
        &fs::read(&downstream_pages_path).expect("read downstream page contracts"),
    )
    .expect("downstream-pages.json has valid schema fields");

    assert_eq!(manifest.schema, BINDINGS_SCHEMA);
    assert_eq!(
        classification_overrides.schema,
        CLASSIFICATION_OVERRIDES_SCHEMA
    );
    assert_eq!(
        caller_runtime_contracts.schema,
        CALLER_RUNTIME_CONTRACTS_SCHEMA
    );
    assert_eq!(
        comparison_normalization_contracts.schema,
        COMPARISON_NORMALIZATION_CONTRACTS_SCHEMA
    );
    assert_eq!(
        wikijump_feature_contracts.schema,
        WIKIJUMP_FEATURE_CONTRACTS_SCHEMA
    );
    assert_eq!(downstream_pages.schema, DOWNSTREAM_PAGES_SCHEMA);
    assert!(!cases.is_empty(), "parity case population is empty");
    assert_eq!(
        cases
            .iter()
            .map(|case| case.case_id.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        cases.len(),
        "cases.jsonl must have unique case IDs"
    );
    assert!(
        manifest
            .bindings
            .windows(2)
            .all(|pair| pair[0].case_id < pair[1].case_id),
        "bindings must have unique, sorted case IDs"
    );
    assert!(
        classification_overrides
            .overrides
            .windows(2)
            .all(|pair| pair[0].path < pair[1].path),
        "classification overrides must have unique, sorted paths"
    );

    let cases_by_path: BTreeMap<_, _> = cases
        .iter()
        .map(|case| (case.source_origin.path.as_str(), case))
        .collect();
    let override_paths: BTreeSet<_> = classification_overrides
        .overrides
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    for entry in &classification_overrides.overrides {
        assert!(
            entry
                .reason
                .starts_with(REVIEWED_CONTEXT_FREE_REASON_PREFIX),
            "{}: classification override reason must be reviewed context-free",
            entry.path
        );
        let case = cases_by_path.get(entry.path.as_str()).unwrap_or_else(|| {
            panic!("{}: classification override has no case", entry.path)
        });
        assert_eq!(
            entry.source_sha256, case.source_sha256,
            "{}: source hash",
            entry.path
        );
        assert_eq!(
            entry.execution_class, case.execution_class,
            "{}: execution class",
            entry.path
        );
        assert_eq!(
            entry.page_scope, case.page_scope,
            "{}: page scope",
            entry.path
        );
        assert_eq!(
            case.reasons.as_slice(),
            std::slice::from_ref(&entry.reason),
            "{}: reason",
            entry.path
        );
    }
    for case in &cases {
        if case
            .reasons
            .iter()
            .any(|reason| reason.starts_with(REVIEWED_CONTEXT_FREE_REASON_PREFIX))
        {
            assert!(
                override_paths.contains(case.source_origin.path.as_str()),
                "{}: reviewed context-free case needs a classification override",
                case.case_id
            );
        }
    }
    let execution_counts = cases.iter().fold([0; 4], |mut counts, case| {
        counts[match case.execution_class {
            ExecutionClass::SavedPageBatch => 0,
            ExecutionClass::PagePreviewIsolated => 1,
            ExecutionClass::WikijumpRuntime => 2,
            ExecutionClass::NotApplicable => 3,
        }] += 1;
        counts
    });
    assert_eq!(
        execution_counts[0] + execution_counts[1],
        manifest.bindings.len(),
        "every preview-compatible case must have one frozen binding"
    );
    let binding_counts = manifest
        .bindings
        .iter()
        .fold([0; 2], |mut counts, binding| {
            counts[match binding.status {
                BindingStatus::Match => 0,
                BindingStatus::Mismatch => 1,
            }] += 1;
            counts
        });
    assert_eq!(
        binding_counts[0] + binding_counts[1],
        manifest.bindings.len(),
        "binding status counts must cover the manifest exactly"
    );

    let disposition_counts =
        manifest
            .bindings
            .iter()
            .fold([0; 4], |mut counts, binding| {
                if let Some(disposition) = binding.disposition {
                    counts[match disposition {
                        BindingDisposition::Unresolved => 0,
                        BindingDisposition::CallerRuntime => 1,
                        BindingDisposition::ComparisonNormalization => 2,
                        BindingDisposition::SecurityBoundary => 3,
                    }] += 1;
                }
                counts
            });
    assert_eq!(
        disposition_counts.iter().sum::<usize>(),
        binding_counts[1],
        "every mismatch must have exactly one disposition"
    );

    assert!(
        caller_runtime_contracts
            .contracts
            .windows(2)
            .all(|pair| pair[0].id < pair[1].id),
        "caller-runtime contract IDs must be unique and sorted"
    );
    let caller_runtime_cases: BTreeSet<_> = manifest
        .bindings
        .iter()
        .filter(|binding| binding.disposition == Some(BindingDisposition::CallerRuntime))
        .map(|binding| binding.case_id.as_str())
        .collect();
    let mut contracted_cases = BTreeSet::new();
    for contract in &caller_runtime_contracts.contracts {
        assert!(
            contract.wikijump_test.starts_with("services::"),
            "{}: Wikijump test must name a concrete service test",
            contract.id
        );
        assert!(
            !contract.cases.is_empty(),
            "{}: caller-runtime contract must own at least one case",
            contract.id
        );
        assert!(
            contract.cases.windows(2).all(|pair| pair[0] < pair[1]),
            "{}: caller-runtime case IDs must be unique and sorted",
            contract.id
        );
        for case_id in &contract.cases {
            assert!(
                contracted_cases.insert(case_id.as_str()),
                "{case_id}: caller-runtime case is mapped by more than one contract"
            );
        }
    }
    assert_eq!(
        contracted_cases, caller_runtime_cases,
        "every caller-runtime binding must map to exactly one concrete Wikijump regression contract"
    );

    let comparison_normalization_cases: BTreeSet<_> = manifest
        .bindings
        .iter()
        .filter(|binding| {
            binding.disposition == Some(BindingDisposition::ComparisonNormalization)
        })
        .map(|binding| binding.case_id.as_str())
        .collect();
    assert!(
        comparison_normalization_contracts
            .contracts
            .windows(2)
            .all(|pair| pair[0].case_id < pair[1].case_id),
        "comparison-normalization contracts must have unique sorted case IDs"
    );
    let contracted_normalizations: BTreeSet<_> = comparison_normalization_contracts
        .contracts
        .iter()
        .map(|contract| {
            assert!(
                matches!(
                    contract.difference_class.as_str(),
                    "page-preview-root-whitespace"
                        | "html-serialization"
                        | "browser-rendering-whitespace"
                ),
                "{}: comparison-normalization may only cover browser-verified rendering/serialization whitespace differences",
                contract.case_id
            );
            assert!(
                contract.wikijump_feature_id.starts_with("syntax-"),
                "{}: normalization contract must name a syntax feature",
                contract.case_id
            );
            assert!(
                !contract.review.trim().is_empty(),
                "{}: normalization contract needs explicit review",
                contract.case_id
            );
            contract.case_id.as_str()
        })
        .collect();
    assert_eq!(
        contracted_normalizations, comparison_normalization_cases,
        "every comparison-normalization mismatch needs exactly one explicit review contract"
    );

    let case_ids: BTreeSet<_> = cases.iter().map(|case| case.case_id.as_str()).collect();
    assert!(
        wikijump_feature_contracts
            .contracts
            .windows(2)
            .all(|pair| pair[0].id < pair[1].id),
        "Wikijump feature contract IDs must be unique and sorted"
    );
    assert_eq!(
        wikijump_feature_contracts.contracts.len(),
        37,
        "every Wikijump syntax-* feature must have one FTML ownership contract"
    );
    let mut contracted_features = BTreeSet::new();
    for contract in &wikijump_feature_contracts.contracts {
        assert!(
            contract.wikijump_feature_id.starts_with("syntax-"),
            "{}: Wikijump feature contract must name a syntax feature",
            contract.id
        );
        assert!(
            contracted_features.insert(contract.wikijump_feature_id.as_str()),
            "{}: duplicate Wikijump syntax feature ownership",
            contract.wikijump_feature_id
        );
        assert!(
            !contract.cases.is_empty(),
            "{}: Wikijump feature contract needs representative FTML cases",
            contract.id
        );
        for case_id in &contract.cases {
            assert!(
                case_ids.contains(case_id.as_str()),
                "{}: Wikijump feature contract references missing case {case_id}",
                contract.id
            );
        }
        let expected_axes = (1..=8)
            .map(|index| format!("P{index}"))
            .collect::<BTreeSet<_>>();
        assert_eq!(
            contract
                .property_owners
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>(),
            expected_axes,
            "{}: property ownership must cover exactly P1-P8",
            contract.id
        );
        for owner in contract.property_owners.values() {
            assert!(
                matches!(
                    owner.as_str(),
                    "ftml" | "wikijump" | "shared" | "not-applicable"
                ),
                "{}: invalid property owner {owner}",
                contract.id
            );
        }
    }

    assert!(
        downstream_pages
            .pages
            .windows(2)
            .all(|pair| pair[0].id < pair[1].id),
        "downstream page IDs must be unique and sorted"
    );
    assert!(
        downstream_pages
            .pages
            .iter()
            .any(|page| page.id == "scp-9506"),
        "SCP-9506 must remain a downstream canary"
    );
    let mut downstream_paths = BTreeSet::new();
    for page in &downstream_pages.pages {
        assert!(
            downstream_paths.insert(page.path.as_str()),
            "{}: duplicate downstream page path {}",
            page.id,
            page.path
        );
        assert!(
            is_lower_hex(&page.sha256, 64),
            "{}: downstream source hash",
            page.id
        );
        assert!(!page.site.is_empty(), "{}: downstream site", page.id);
        assert!(!page.page.is_empty(), "{}: downstream page", page.id);
        assert!(!page.title.is_empty(), "{}: downstream title", page.id);
    }

    let parity_docs = fs::read_to_string(root.join("docs/ParityTests.md"))
        .expect("read docs/ParityTests.md");
    assert!(
        parity_docs.contains(
            "The current inventory and binding totals are derived from the committed machine-readable artifacts"
        ),
        "ParityTests.md must treat the machine-readable artifacts as the changing denominator authority"
    );
    assert!(
        parity_docs.contains("EXPECTED_PREVIEW=$(jq -c"),
        "ParityTests.md capture instructions must derive the preview denominator from cases.jsonl"
    );

    let fixture_sources = fixture_sources(root);
    let case_paths: BTreeSet<_> = cases
        .iter()
        .map(|case| case.source_origin.path.as_str())
        .collect();
    assert_eq!(
        case_paths,
        fixture_sources
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        "cases.jsonl must exactly cover the stable FTML fixture population"
    );

    let mut cases_by_id = BTreeMap::new();
    for case in &cases {
        assert_eq!(case.schema, CASE_SCHEMA, "{}: case schema", case.case_id);
        assert_eq!(
            case.source_origin.repository, "Rokurolize/ftml",
            "{}: source repository",
            case.case_id
        );
        let fixture_source = &fixture_sources[&case.source_origin.path];
        assert_eq!(
            &case.source, fixture_source,
            "{}: fixture source",
            case.case_id
        );
        assert_eq!(
            case.case_id,
            expected_case_id(&case.source_origin.path),
            "{}: stable case identity",
            case.case_id
        );
        assert_eq!(
            case.source_sha256,
            sha256(case.source.as_bytes()),
            "{}: source hash",
            case.case_id
        );
        assert!(
            matches!(case.page_scope.as_str(), "batch-safe" | "isolated"),
            "{}: page scope",
            case.case_id
        );
        assert!(
            !case.reasons.is_empty()
                && case.reasons.iter().all(|reason| !reason.is_empty()),
            "{}: execution classification needs explicit reasons",
            case.case_id
        );
        assert!(
            cases_by_id.insert(case.case_id.as_str(), case).is_none(),
            "{}: duplicate case ID",
            case.case_id
        );
    }

    let mut reference_files = fs::read_dir(&artifact_dir)
        .expect("read parity artifact directory")
        .map(|entry| entry.expect("read parity artifact entry").path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("references-") && name.ends_with(".jsonl")
                })
        })
        .collect::<Vec<_>>();
    reference_files.sort();
    assert!(!reference_files.is_empty(), "no current Wikidot references");

    let mut references = BTreeMap::new();
    for path in reference_files {
        for reference in read_jsonl::<WikidotReference>(&path) {
            let id = reference.syntax_case.case_id.clone();
            let case = cases_by_id
                .get(id.as_str())
                .unwrap_or_else(|| panic!("{}: reference has no live case", id));
            assert_eq!(
                reference.schema, REFERENCE_SCHEMA,
                "{}: reference schema",
                id
            );
            assert_eq!(
                reference.syntax_case.schema, SYNTAX_CASE_SCHEMA,
                "{}: syntax-case schema",
                id
            );
            assert_eq!(reference.syntax_case.title, id, "{}: preview title", id);
            assert_eq!(
                reference.syntax_case.wikidot_observation_tier, "page-preview",
                "{}: Wikidot tier",
                id
            );
            assert_eq!(
                reference.syntax_case.local_execution_tier, "ftml",
                "{}: local tier",
                id
            );
            assert_eq!(
                reference.source_sha256,
                sha256(reference.syntax_case.source.as_bytes()),
                "{}: embedded source hash",
                id
            );
            assert_eq!(
                reference.raw_html_sha256,
                sha256(reference.raw_html.as_bytes()),
                "{}: Wikidot HTML hash",
                id
            );
            assert_eq!(
                reference.provenance.module, "edit/PagePreviewModule",
                "{}: reference module",
                id
            );
            assert!(
                !reference.provenance.authenticated,
                "{}: preview must be anonymous",
                id
            );
            assert!(
                !reference.provenance.mutated,
                "{}: preview must not mutate Wikidot",
                id
            );
            assert!(
                !reference.provenance.wikidot_py_version.is_empty(),
                "{}: wikidot.py version",
                id
            );
            assert_eq!(
                reference.provenance.site, "sandbox-for-codex",
                "{}: reference site",
                id
            );
            assert_eq!(
                reference.provenance.site_domain, "sandbox-for-codex.wikidot.com",
                "{}: reference site domain",
                id
            );
            assert!(
                is_lower_hex(&reference.provenance.wikidot_py_commit, 40),
                "{}: wikidot.py commit",
                id
            );
            assert!(
                is_lower_hex(&reference.provenance.requirements_sha256, 64),
                "{}: requirements hash",
                id
            );
            let captured_at =
                time::OffsetDateTime::parse(&reference.captured_at, &Rfc3339)
                    .unwrap_or_else(|error| panic!("{}: capture timestamp: {error}", id));
            assert_eq!(
                captured_at.offset(),
                UtcOffset::UTC,
                "{}: capture offset",
                id
            );
            if reference.source_sha256 != case.source_sha256 {
                continue;
            }
            assert!(
                case.execution_class.is_preview_compatible(),
                "{}: non-preview case has a current reference",
                id
            );
            assert_eq!(
                reference.syntax_case.source, case.source,
                "{}: current reference source",
                id
            );
            assert!(
                references.insert(id.clone(), reference).is_none(),
                "{}: duplicate current reference",
                id
            );
        }
    }

    let bindings: BTreeMap<_, _> = manifest
        .bindings
        .iter()
        .map(|binding| (binding.case_id.as_str(), binding))
        .collect();
    let preview_ids: BTreeSet<_> = cases
        .iter()
        .filter(|case| case.execution_class.is_preview_compatible())
        .map(|case| case.case_id.as_str())
        .collect();
    assert_eq!(
        references
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        preview_ids,
        "every preview-compatible case needs exactly one reference"
    );
    assert_eq!(
        bindings.keys().copied().collect::<BTreeSet<_>>(),
        preview_ids,
        "every preview-compatible case needs exactly one binding"
    );
    assert_eq!(
        bindings.len(),
        manifest.bindings.len(),
        "duplicate binding case IDs"
    );

    for id in preview_ids {
        let case = cases_by_id[id];
        let reference = &references[id];
        let binding = bindings[id];
        assert_eq!(
            binding.source_sha256, case.source_sha256,
            "{}: binding source identity",
            id
        );
        assert_eq!(
            binding.wikidot_html_sha256, reference.raw_html_sha256,
            "{}: binding Wikidot identity",
            id
        );
        assert!(
            is_lower_hex(&binding.ftml_html_sha256, 64),
            "{}: binding FTML HTML hash",
            id
        );
        let source = case.source.to_ascii_lowercase();
        if ![
            "[[bibliography",
            "[[ bibliography",
            "[[embedvideo",
            "[[ embedvideo",
            "[[gallery",
            "[[ gallery",
            "[[social",
            "[[ social",
        ]
        .iter()
        .any(|marker| source.contains(marker))
        {
            assert_eq!(
                binding.ftml_html_sha256,
                sha256(render(case, reference).as_bytes()),
                "{}: current FTML HTML hash",
                id
            );
        }
        match binding.status {
            BindingStatus::Match => {
                assert!(
                    binding.checks.all_match(),
                    "{}: match has a failed check",
                    id
                );
                assert!(
                    binding.disposition.is_none(),
                    "{}: match has a disposition",
                    id
                );
                assert!(binding.reason.is_none(), "{}: match has a reason", id);
            }
            BindingStatus::Mismatch => {
                assert!(
                    !binding.checks.all_match(),
                    "{}: mismatch has no failed check",
                    id
                );
                assert!(
                    binding.disposition.is_some(),
                    "{}: mismatch needs a disposition",
                    id
                );
                assert!(
                    binding
                        .reason
                        .as_ref()
                        .is_some_and(|value| !value.is_empty()),
                    "{}: mismatch needs a reason",
                    id
                );
                if binding.disposition == Some(BindingDisposition::Unresolved) {
                    assert!(
                        binding
                            .reason
                            .as_ref()
                            .is_some_and(|reason| has_positive_issue_number(reason)),
                        "{}: unresolved mismatch needs an active functional issue",
                        id
                    );
                }
            }
        }
    }
}
