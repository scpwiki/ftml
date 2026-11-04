[<< Return to the README](../README.md)

## Wikidot Parity Evidence

Raw, provenance-backed Wikidot reference JSONL is the independent oracle. A tree-test `wikidot.html` file is derived FTML output for a local `Layout::Wikidot` regression; it must never be generated and then cited as evidence for itself.

The regression corpus is the fixture tree itself: every `test/**/input.ftml` and `tests/fixtures/**/*.ftml` source is a case, and no registration step exists — adding a fixture is sufficient. The dated `tests/fixtures/wikidot-parity/references-YYYYMMDD-NN.jsonl` files preserve the raw Wikidot responses and capture provenance; they are the frozen evidence the corpus was judged against. `tests/fixtures/wikidot-parity/bindings.json` records the comparator outcome (match or mismatch, with hashes) for the captured cases and is consumed by the live-rotation selector and the comparison-normalization browser check; it is an output of the comparison tooling, not a gate: no test requires it to be complete or current.

A frozen mismatch stays visible as a mismatch in the reference record and in `bindings.json`; it does not become a match. The dispositions used by the comparison tooling are `caller-runtime` (a frozen output difference whose missing inputs belong to the caller runtime rather than FTML), `comparison-normalization` (a frozen raw-DOM difference with no demonstrated functional behavior difference), `security-boundary` (the live output would cross an explicit FTML safety boundary and is intentionally denied rather than activated; narrow: it requires a concrete security contract and focused regression, not a general compatibility escape hatch), and `unresolved` (an active functional investigation).

`tests/fixtures/wikidot-parity/caller-runtime-contracts.json` names the concrete Wikijump regression responsible for each caller-runtime boundary. When the adjacent Wikijump checkout is available, verify and execute all named runtime contracts with `python3 scripts/check_wikijump_runtime_contracts.py --wikijump-root ../wikijump --run-tests`. Omit `--run-tests` when only checking cross-repository test identity.

Ordinary FTML tests are offline. The corpus AST tests pin each fixture's parse tree and both layouts' rendered output, `tests/canonical_wikidot_source.rs` compares current FTML output against the frozen references, and the Unicode robustness tests drive the fixture corpus through the public pipeline under mutation:

```sh
cargo test --lib
cargo test --test integration
```

Exact local FTML output hashing is skipped for sources containing `bibliography`, `embedvideo`, or `gallery` blocks because production rendering can generate random IDs. Their frozen Wikidot hashes and functional DOM and text checks remain enforced. The bibliography comparator canonicalizes only its generated suffix; caller-runtime mismatches remain mismatches and receive no snapshot.

## Adding evidence

New captures are added as new dated `references-YYYYMMDD-NN.jsonl` files and are never edited or replaced afterwards: if a fixture source changes, keep its old rows and add a new capture. Capture runs through the Wikijump verification tooling against `sandbox-for-codex`:

```sh
../wikijump/install/local/wikidot-verification/.venv/bin/python \
  ../wikijump/install/local/wikidot-verification/scripts/capture_wikidot_preview_references.py \
  --cases <batch.jsonl> \
  --output "tests/fixtures/wikidot-parity/references-$(date -u +%Y%m%d)-01.jsonl" \
  --site sandbox-for-codex \
  --batch-size 4 \
  --live-execution-class saved-page-batch \
  --live-execution-class page-preview-isolated
```

Here `<batch.jsonl>` is an ad-hoc JSONL of syntax cases for the sources being captured. Both `--live-execution-class` options are required; otherwise the capture command defaults to only `page-preview-isolated`. The command is anonymous, read-only, and external to normal FTML tests. Never replace or edit a raw reference file to make a comparison pass.

Comparisons run the streaming renderer against a reference batch with the Wikijump differential runner (`run-syntax-differential.mjs` in the same verification tooling); it compares parsed DOM, DOM signature, and visible text and exits nonzero for a mismatch or runner error. Historical reference files remain valid inputs for their own source rows. A mismatch is an explicit result, not permission to hide or normalize the difference.

### UTF-8 and panic-safety gate

Wikidot compatibility fixes must remain safe for arbitrary valid UTF-8, not only ASCII witnesses. The ordinary integration suite includes structured Unicode mutation tests that drive both layouts through preprocess, tokenize, parse, HTML render, and text render. It also rejects unchecked Rust slice expressions whose bounds subtract from `str::len()` or use `saturating_sub` inside an index; those forms are easy to make invalid at a multibyte character boundary. Prefer `strip_prefix`, `strip_suffix`, `str::get`, or offsets proved by a character-boundary-producing API.

For longer-running discovery, `fuzz/fuzz_targets/public_pipeline.rs` exercises the same Page-mode public pipeline, while `fuzz/fuzz_targets/delayed_pipeline.rs` drives the List-mode delayed-input constructor, parser, atomic binding, and HTML render path across authored, runtime-scalar, runtime-literal, single-slot, and multi-slot segmentations. Build a seed corpus from the fixture corpus, optionally including Wikijump's seeded full pages, with:

```sh
FUZZ_CORPUS=$(mktemp -d)
python3 scripts/build_ftml_fuzz_corpus.py \
  --output "$FUZZ_CORPUS" \
  --wikijump-root ../wikijump
cargo +nightly fuzz run --fuzz-dir fuzz public_pipeline "$FUZZ_CORPUS"
cargo +nightly fuzz run --fuzz-dir fuzz delayed_pipeline "$FUZZ_CORPUS"
```

The fuzz corpus is generated outside the repository and is not compatibility evidence. Any interesting crash or novel parity case must be minimized into a stable regression and, where behavior is observable on Wikidot, captured through the normal provenance-backed reference path.

The scheduled robustness workflow keeps both libFuzzer artifact directories and writes target-specific manifests with the FTML commit, artifact SHA-256, byte length, UTF-8 validity, and exact one-input reproduction command. Reproduce a saved artifact locally with the exact target rather than relying on the original wall-clock timeout:

```sh
cargo +nightly fuzz run --fuzz-dir fuzz public_pipeline -- \
  -runs=1 -timeout=10 fuzz/artifacts/public_pipeline/<artifact>
cargo +nightly fuzz run --fuzz-dir fuzz delayed_pipeline -- \
  -runs=1 -timeout=10 fuzz/artifacts/delayed_pipeline/<artifact>
```

### Wikijump full-page downstream gate

Reduced parity fixtures are not sufficient for integration safety. `tests/fixtures/wikidot-parity/downstream-pages.json` pins six full Wikijump seed sources, including `scp-9506`, the two large `scp-8980` fragments, and the Basalt theme, by exact SHA-256. Run each source through the public FTML pipeline in both layouts before merging parser or preprocessor changes:

```sh
python3 scripts/check_wikijump_full_pages.py \
  --wikijump-root ../wikijump \
  --build-renderer
```

The command uses one `render_html_jsonl` worker for all pages, so a panic, abort, timeout, missing output row, or source drift is a gate failure. Parse diagnostics are allowed because some full pages intentionally contain caller-owned or unsupported runtime constructs; the gate protects process safety and whole-page interactions rather than pretending the FTML-only lane owns caller runtime. The nightly robustness workflow repeats this check against Wikijump `develop` and then includes the same full pages in the fuzz seed corpus.

The FTML-only full-page pass is not enough to prove that the same candidate dependency compiles inside Deepwell. Run the candidate-integration lane against a clean Wikijump checkout as well:

```sh
python3 scripts/check_wikijump_candidate.py \
  --wikijump-root ../wikijump \
  --ftml-root . \
  --run-tests
```

The checker temporarily replaces only Deepwell's `ftml` dependency with the local checkout, verifies Cargo metadata resolves that path, runs Wikijump's full SCP-9506 canary plus every caller-runtime contract named by `caller-runtime-contracts.json`, then restores both `Cargo.toml` and `Cargo.lock` even on failure. Missing named tests are themselves a gate failure. This is the required downstream lane for parser/preprocessor changes that can affect Wikijump runtime behavior.

### Operation-count performance gate

High-risk parser scanners expose test-only token-visit counters. Their tests run input-size series rather than relying only on wall-clock thresholds: quote close scans stay within twice the token population, generic body-end suffix scans reuse cached unmatched suffixes within a linear token budget, nested two-close scans stay within the token population for the initial scan, lost-owner lookahead stays within the token population, and underline fast-path work has an exact marker-count relation. Wall-clock tests remain outer watchdogs; deterministic operation counts are the regression authority for these scanners.

### Comparison-normalization review

`comparison-normalization` is not a general escape hatch. Every such binding must appear exactly once in `tests/fixtures/wikidot-parity/comparison-normalization-contracts.json`, with an explicit difference class, Wikijump feature ID, and review. Only browser-reviewed root/serialization/rendering-whitespace classes are permitted. Semantic codepoint differences, element/attribute differences, interaction-hook differences, or caller-state differences must be fixed or classified elsewhere. This rule promoted the former `test--math--inline` U+0020/U+00A0 difference back to an ordinary exact match instead of preserving it as normalization.

The eight current normalization cases are also verified by a real Chrome DOM pass:

```sh
python3 scripts/check_comparison_normalization_browser.py \
  --chrome "$(command -v google-chrome)" \
  --build-renderer
```

`page-preview-root-whitespace` and `html-serialization` cases must become the exact same parsed DOM after trimming only ASCII whitespace at the fragment boundaries. `browser-rendering-whitespace` cases may retain formatting whitespace text nodes only when Chrome reports identical element/attribute topology, per-element direct text after normal HTML whitespace collapsing, rendered `innerText`, and exact preformatted/textarea text. The current set has five strict root-equivalent cases and three browser-render-equivalent formatting-whitespace cases. Any future case that fails those checks is not eligible for normalization.

### Standing live differential rotation

Committed references remain the merge authority, but they cannot by themselves detect a later Wikidot behavior change or an FTML regression outside a touched fixture. `.github/workflows/wikidot-live-rotation.yml` therefore selects eight currently exact-match, preview-compatible cases per run with `scripts/select_live_parity_rotation.py`, captures fresh anonymous read-only `edit/PagePreviewModule` observations, and compares the current FTML renderer against them. The deterministic rotation advances through the exact-match population instead of repeatedly sampling the same popular features. Fresh cases, references, and verdicts are uploaded as temporary CI evidence only; they never rewrite committed references automatically.

### Wikijump feature-ledger linkage

`tests/fixtures/wikidot-parity/wikijump-feature-contracts.json` accounts for every canonical `syntax-*` feature in Wikijump's `docs/wikidot-specifications/implementation-ledger.json`. Each feature names representative stable FTML cases and assigns every compatibility property axis P1-P8 to `ftml`, `wikijump`, `shared`, or `not-applicable`. Verify the adjacent ledger with:

```sh
python3 scripts/check_wikijump_feature_contracts.py --wikijump-root ../wikijump
```

The checker fails on any missing or extra `syntax-*` feature, missing P1-P8 owner, invalid owner, or stale case ID. The cited cases are still representative rather than a claim that a small example set exhausts a feature; the contract prevents an unowned feature/property from hiding behind an aggregate passing case count.

For selective mutation testing of the two high-risk compatibility scanners that previously produced false confidence, run:

```sh
cargo mutants \
  --file src/preproc/compatibility.rs \
  --file src/parsing/rule/impls/block/parser.rs \
  -F 'canonicalize_root_collapsible_inline_quoted_closers|parse_wikidot_attribute_field|wikidot_attribute_key'
```

Any surviving mutant is a test-coverage defect until either a focused control kills it or the mutation is documented as semantically equivalent. Do not broaden the command to unrelated renderer code merely to make a global mutation score look better.

The same targeted command runs weekly in `.github/workflows/wikidot-mutation.yml`, with `mutants.out/` retained as a CI artifact. Mutation testing is therefore a standing ratchet rather than a documentation-only review step.

### Live note evidence

On 2026-08-15, anonymous `edit/PagePreviewModule` on `sandbox-for-codex.wikidot.com` was queried with `[[note class="custom"]]\nLive note probe.\n[[/note]]`. Wikidot returned literal markup:

```html
<p>[[note class=&quot;custom&quot;]]<br />
Live note probe.<br />
[[/note]]</p>
```

The response SHA-256 was `363e4f6b6139f479eb0b1fc80b0ccd4e8a2bafbbc681b77e912f94a9ea8c6d98`. The focused `test/note/wikidot-literal` case now compares this response's DOM tree, DOM signature, and visible text successfully. FTML therefore keeps the Wikijump-only `wj-note` feature while leaving the marker literal in `Layout::Wikidot`; issue #500 is resolved by this change.

### Live module alias evidence

On 2026-08-15, reference batches `references-20260815-11.jsonl` and `references-20260815-12.jsonl` captured seven anonymous `edit/PagePreviewModule` observations on `sandbox-for-codex.wikidot.com`. The Wikidot module closer rule refers to the observed behavior in which `module654` may open a module body but only `module` may close it. Wikidot rendered `[[/module654]]` and the remaining CSS body as page text for both `[[module CSS]]` and `[[module654 CSS]]`, while `[[module654 CSS]]` with `[[/module]]` consumed the CSS body normally.

Before the fix, the two alias-closer cases were confirmed parity mismatches across DOM tree, DOM signature, and visible text. Issue #503 records the FTML behavior defect. After the shared close-name fix, all seven observations in the two batches match across all three checks. Their exact source hashes, raw response hashes, timestamps, anonymous acquisition fields, and `wikidot.py` provenance remain frozen in the reference files.

### Registered block-name evidence

Registered block-name coverage refers to the state in which every configured block name and alias appears in the parity corpus. Reference batch `references-20260815-13.jsonl` closes the final three name gaps with `button`, `embedvideo`, and `gallery` sources. The invalid `[[button]]` case matches Wikidot across DOM tree, DOM signature, and visible text.

The other two rows are caller-runtime mismatches and remain visible as mismatches. Wikidot returns a provider no-match error for the inert `embedvideo` payload, while FTML emits a typed placeholder for caller resolution; issue #506 tracks that result. Wikidot gallery selection uses current-page and file-service state and returns a selection error for the empty case, while FTML emits a typed gallery request; issue #507 tracks that result. These rows are not normalized into matches.

Configured block-facet coverage refers to the state in which every configured star, score, and body-close branch appears in the parity corpus. Reference batch `references-20260815-14.jsonl` closes the four previously missing star branches for `anchor`, `checkbox`, `radio`, and `user`. Live Wikidot and `Layout::Wikidot` both keep all four starred forms literal, so every DOM tree, DOM signature, and visible text check matches. The parity report now fails locally if a configured name, star branch, score branch, or body-close branch lacks a stable source.

Ordered alias-pair coverage refers to the state in which a configured opener and closer name pair for a body-owning block appears together in a stable source. Batch `references-20260815-15.jsonl` added the two anchor cross-pairs and exposed issue #510. Batches `references-20260815-16.jsonl` through `references-20260815-18.jsonl` captured the remaining 48 pairs. All 48 match across DOM tree, DOM signature, and visible text, so the local report now lists no missing ordered alias pair.

Reference batch `references-20260815-19.jsonl` freezes the 12 context-free argument-grammar sources asserted by `tests/wikidot_block_argument_grammar.rs` that previously lacked per-source evidence. All 12 match across DOM tree, DOM signature, and visible text. The remaining whitespace-name `user` source is recorded as caller runtime because reproducing its missing-user result requires the caller-owned user resolver used by that test.

Reference batch `references-20260815-20.jsonl` tests column-zero, space-prefixed, tab-prefixed, and text-prefixed ownership for `bibliography`, `code`, `gallery`, `math`, `module CSS`, and `toc`. It exposed issues #514 and #515: FTML accepted whitespace-prefixed physical-line blocks and joined a rejected gallery closer to the next line. After both fixes, five cases match all three comparison surfaces. Gallery line ownership and visible text also match; its remaining DOM mismatch is the caller-owned page/file rendering tracked by issue #507.

Configured head-shape coverage refers to the state in which every head shape required by a block's effective parser appears in the parity corpus. The local ratchet requires empty and non-assignment value tails for no-head and value-head blocks; empty, assignment-map, and non-assignment value tails for map-head blocks; and empty, value, map, and value-plus-map tails for value-plus-map blocks. It applies the effective `html` map, `tabview` value, and `raw` custom-head overrides. The parity report now lists `missing-head-shape 0`.

Reference batches `references-20260815-21.jsonl` through `references-20260815-24.jsonl` freeze 61 anonymous PagePreview head-shape observations; the remaining `file:none` and `module:map` sources are recorded as caller runtime. The live comparisons exposed three FTML defects. Issue #517 covered standalone buttons missing Wikidot click handlers and incorrectly receiving a generated `id`; the fixed Wikidot output emits the live handlers without that ID. Issue #518 covered the localized host default for `[[date 0]]`; the fixed Wikidot default is the deterministic `01 Jan 1970 00:00`. Issue #519 covered a non-assignment gallery tail remaining literal instead of creating a gallery requirement. After the fixes, 59 of these 61 PagePreview cases match all three comparison surfaces. The `gallery:map` and `gallery:value` cases reach the correct gallery requirement but remain visible caller-runtime mismatches because current-page and file-service selection belongs to issue #507.

Reference batch `references-20260815-25.jsonl` adds the security-sensitive invalid apostrophe tag token for `set-tags`. Issue #520 recorded that Wikidot rejects the token with its missing-text error while FTML previously emitted a functional button. FTML now rejects the token while retaining JavaScript-string escaping as defense in depth, and the frozen case matches all three comparison surfaces.

Reference batches `references-20260815-26.jsonl` and `references-20260815-27.jsonl` freeze 25 button and date sources that were either missing live evidence for local regressions or added for the token boundaries in issue #521. The fixed Wikidot parser rejects quote, equals, ampersand, entity, and angle-bracket artifacts; treats ASCII tab, carriage return, line feed, vertical tab, and form feed as separators; removes NUL; and retains the observed broad punctuation and Unicode acceptance. All 25 now match all three comparison surfaces, including Wikidot's live-accepted backslash in the generated `set-tags` handler. The compatibility hazard is documented in `WikidotCompatibilityHazards.md`.

Batch `references-20260815-27.jsonl` also exposed issue #522. For `[[date 0 format="%Y"]]`, Wikidot records `format_%25Y` in the class but renders the default `01 Jan 1970 00:00` text rather than `1970`. `Layout::Wikidot` now matches that behavior, while `Layout::Wikijump` retains explicit-format rendering.

Reference batches `references-20260815-28.jsonl` through `references-20260815-32.jsonl` freeze 76 anonymous PagePreview observations from a 77-source interaction set. The sources are grouped by shared parser branch rather than a Cartesian product: 8 lexical and malformed-block recovery cases, 12 quote-ownership cases, 9 native-list-ownership cases, 16 inline-delimiter cases, 16 math-boundary cases, and 16 table-ownership cases. The remaining `interaction-triple-link-span-close` source is recorded as caller runtime, so all 77 remain accounted for.

These batches exposed three FTML defects. Issue #527 removed the functional trailing line break after a code block at the end of a footnote; its remaining root formatting whitespace differs only in the raw DOM, while DOM signature and visible text match, so the binding is `comparison-normalization`. Issue #528 made a malformed scored span opener with a residual `]` use Wikidot's root ownership. Issue #529 made a residual gallery opener bracket and the following line share Wikidot's paragraph ownership. The embedvideo residual-opener row remains a `caller-runtime` mismatch under issue #506 because provider matching belongs to the caller, and the gallery residual-opener row remains a `caller-runtime` mismatch under issue #507 because current-page file selection belongs to the caller.

Reference batch `references-20260815-33.jsonl` freezes 16 anonymous, non-mutating PagePreview observations for top-level parser functions and preprocessor ownership seams. Ten sources cover `#if`, `#ifexpr`, and `#expr` truth, error, case, newline, and residual-close branches. Six sources cover quote removal, crossed center/collapsible ownership, crossed center/div ownership, inline bold/size correction, and orphan quoted tab closers. The crossed quoted center/collapsible source exposed issue #534: FTML incorrectly kept text after the collapsible close inside the unfolded body. The fixed `Layout::Wikidot` renderer matches all 16 sources across DOM tree, DOM signature, and visible text; `Layout::Wikijump` keeps its existing crossed-closer behavior.

Reference batch `references-20260815-35.jsonl` freezes 13 anonymous PagePreview branch witnesses selected from the remaining interaction backlog. Two crossed Color/Container directions exercise both shared-span shell roles. Three inline triples, together with the existing no-equality triple, exercise each FIFO tag-equality relation. Four quote cases exercise depth decrease, blank and empty quoted-row ownership, and virtual heading start. Four list cases exercise indented-first rejection, depth one, a skipped depth, and tab expansion to depth four. All 13 match Wikidot across DOM tree, DOM signature, and visible text. These witnesses close the relevant parser branches; equivalent ordered combinations do not need Cartesian capture.

Reference batch `references-20260815-34.jsonl` freezes 16 anonymous, non-mutating PagePreview observations for autolink termination and ownership seams: punctuation, whitespace, Unicode, email, `mailto:`, protocol-relative text, external triple links, code, raw text, comments, and tables. The current `Layout::Wikidot` renderer at engine commit `bd1e6bfb6859011a62b2cf2173c0764b279abf00` matches all 16 across DOM tree, DOM signature, and visible text, so this batch exposed no functional discrepancy.

Reference batch `references-20260815-36.jsonl` freezes 10 anonymous, non-mutating PagePreview observations for advanced-table closer selection, residual closers, casing, malformed names, and heading, quote, and list ownership. Nine cases match Wikidot across DOM tree, DOM signature, and visible text and have reviewed local snapshots. In `table-residual-hcell-closer`, PagePreview's standard leading root `\n\n` is merged into the residual text node; DOM signature and visible text match. Issue #537 reviewed this as `comparison-normalization`, not FTML behavior, so that row remains a visible classified mismatch and has no snapshot.

Reference batches `references-20260815-37.jsonl` and `references-20260815-38.jsonl` freeze 17 anonymous, non-mutating PagePreview observations for the remaining context-free automatic-link and single-link recovery branches. The witnesses cover paired wrapper removal, terminal-period splitting, bare `www.` rejection, single-link target and label comment elision, empty and invalid target dispositions, empty-label and end-of-input recovery, residual closers, raw, span, footnote, and bibliography ownership, and case-sensitive and security-sensitive scheme handling. All 17 match Wikidot across DOM tree, DOM signature, and visible text on FTML engine commit `81d60e5db932b83c5101442e4534bf5ef13212f7`.

Reference batch `references-20260815-39.jsonl` freezes 16 anonymous, non-mutating PagePreview observations for heading content and recovery. The witnesses cover empty TOC and no-TOC headings, comment, raw, inline-format, span, Color, footnote-first and next-line footnote ownership, adjacent heading retries, nested line ownership, and the trailing-underscore guard. All 16 match Wikidot across DOM tree, DOM signature, and visible text on FTML engine commit `926303b2931c4420c1ed3557399f74cc4375ef67`.

Reference batches `references-20260815-40.jsonl` through `references-20260815-48.jsonl` and `references-20260816-01.jsonl` through `references-20260816-03.jsonl` add 153 current-source branch witnesses for the high-risk comment and content-separator seams. Comment recovery exposed issues #544, #545, and #548-#562 for attribute, parser-function, URL, delimiter, line-ownership, and comment-formed separator behavior. The two dangerous CSS seam cases now reproduce Wikidot rather than remaining security divergences. The internal triple-link target/label behavior is correct; only the page-existence-dependent `newpage` class differs, so that row is `caller-runtime` under issue #550.

The content-separator witnesses establish that unpadded runs of four or more `=` remain separators, including five-, six-, and nine-character runs and an internal five-character run. Leading or trailing ASCII SP/TAB prevents separator ownership, tracked by issue #546, while leading U+00A0 NBSP or U+2007 FIGURE SPACE must remain authored text, tracked by issue #547. Trailing NBSP, trailing U+2003 EM SPACE, and discarded trailing form feed already match FTML as non-separators. The 153 comment and content-separator witnesses remain frozen in these batches; matching rows carry local snapshots and mismatching rows carry their recorded dispositions.

Reference batches `references-20260816-04.jsonl` through `references-20260816-20.jsonl` and `references-20260817-01.jsonl`, `references-20260817-02.jsonl`, and `references-20260817-04.jsonl` add 250 recorder-derived and boundary witnesses for advanced-table recovery, parser-function errors, malformed attribute heads, crossed ownership, rollback, and implementation limits. One hundred ninety-five cases match, three are reviewed comparison-normalization differences, and 52 remain unresolved FTML behavior investigations under issues #564-#580. The current source hashes and bindings cover these observations without asserting that the recorder or limit matrix is exhaustive.

Reference batch `references-20260817-12.jsonl` freezes the literal-body typography family. Live applies the number-space rule to prose and monospace but preserves ASCII inside `[[code]]` and `@@...@@` bodies, while `Layout::Wikidot` transforms those literal bodies too; the code case also changes visible text. The prose, monospace, and code-ellipsis controls match (proving the rule fires in live prose and that the ellipsis literal-region protection is correct) and have reviewed snapshots. The code and raw number-space rows are unresolved FTML-owned mismatches under issue #618.

Reference batch `references-20260817-13.jsonl` adds the unit-space code witness and two monospace controls. Live also preserves `41 px` inside `[[code]]` while `Layout::Wikidot` emits U+00A0 (visible text differs); this row extends issue #618 to `replace_unit_spaces`. Monospace `<<A>>` and `x....x` both match live, confirming that live treats monospace as prose for all typography rules; both controls have reviewed snapshots.

Reference batch `references-20260817-14.jsonl` freezes the URL-target ellipsis witnesses. Live preserves dot runs inside autolink and single-link targets, while `Layout::Wikidot` replaces them with U+2026, corrupting the rendered `href` (`https://ex%E2%80%A6.ample.com`); the autolink case also changes visible text. Both rows are unresolved FTML-owned mismatches under issue #582.

Reference batch `references-20260817-15.jsonl` adds the quote-rule code witness. Live preserves backtick-quote pairs inside `[[code]]` bodies while `Layout::Wikidot` emits curly quotes (visible text differs); this row extends issue #618 to `replace_within_paragraphs`.

Reference batch `references-20260817-16.jsonl` freezes the body-tab whitespace family. Prose, `[[code]]`, `{{...}}`, and code-NBSP tab behavior all match live (prose and monospace collapse to one space; code expands to four columns and converts NBSP). Live expands tabs inside `@@...@@` raw bodies to four columns while `Layout::Wikidot` collapses to one; that row is an unresolved FTML-owned mismatch under issue #619. The four matching controls have reviewed snapshots.

Reference batch `references-20260817-17.jsonl` freezes the replacement-marker and terminal-backslash family. Authored U+FFFD mid-line (dropped) and on its own line (marker `2`) match live, as do single and double terminal backslashes at true document end. An authored U+FFFD at document end renders as marker `2` in live but marker `1` in `Layout::Wikidot`; that row is an unresolved FTML-owned mismatch under issue #620. The four matching controls have reviewed snapshots.

Reference batch `references-20260817-18.jsonl` freezes the line-continuation family. Plain continuation joins and a span opener after a continuation both match live, while the div-opener case validates FTML's standalone-div emulation structurally; its only DOM difference is PagePreview's standard leading root whitespace merging into the residual text node, reviewed as `comparison-normalization` under the same class as issue #537. The two matching controls have reviewed snapshots.

Reference batch `references-20260817-19.jsonl` freezes the final whitespace controls. A document-leading indented quote stays literal in both engines (validating the indentation barrier), literal NUL bytes drop in both, CRLF in `[[code]]` bodies normalizes in both, and bare CR Mac newlines normalize in both. All four match and have reviewed snapshots.

Reference batch `references-20260817-20.jsonl` freezes the double-bracket autolink boundary. `[[http://example.com]]` renders as literal brackets around an autolinked URL in both engines, matching live and closing the single-bracket lookbehind question; the control has a reviewed snapshot.

Reference batch `references-20260817-21.jsonl` freezes the module-case and code-raw marker controls. Uppercase `[[MODULE]]` keywords and uppercase `[[/MODULE]]` closers both dispatch identically to live (case-insensitive), and `@@` markers inside `[[code]]` bodies stay literal in both engines. All three match and have reviewed snapshots.

Reference batch `references-20260817-22.jsonl` freezes the autolink `>@` termination seam. Live's automatic-link scan continues through `>@` (rendering `href="https://example.com/a&gt;@b"`), while `Layout::Wikidot` truncates the target at `>`; that row is an unresolved FTML-owned mismatch under issue #621. The alt-raw body with a URL (`@<https://example.com/raw>@`) matches live and has a reviewed snapshot.

Reference batch `references-20260817-23.jsonl` freezes the unsafe triple-link scheme controls. Live renders the same sanitized relative hrefs as `Layout::Wikidot` for `javascript:` and `data:` triple-link targets (the scheme neutralization is live-verified and identical); the only difference is live's `newpage` class, which depends on page-existence state. Both rows are `caller-runtime` mismatches under issue #550.

Reference batch `references-20260817-24.jsonl` freezes the bare-angle autolink control. Live and FTML both continue the automatic-link target through a bare `>` in `https://example.com/a>b`, confirming that the `>@` mismatch in issue #621 is a narrower delimiter condition; the control has a reviewed snapshot.

Reference batch `references-20260817-25.jsonl` freezes the greedy ellipsis controls. Five-dot runs and mixed continuous/spaced groups match live, validating the documented greedy replacement and overlap behavior; both controls have reviewed snapshots.

Reference batches `references-20260817-05.jsonl`, `references-20260817-06.jsonl`, `references-20260817-10.jsonl`, and `references-20260817-11.jsonl` freeze the focused `getAttrs`-versus-generic-attribute and single-link-label typography family. The `div` legacy-`getAttrs` numeric-space control and the link-label angle-quote control match and have reviewed snapshots. Wikidot preserves authored ASCII inside generic `span`, legacy `getAttrs`, and external single-link label values, while `Layout::Wikidot` applies prose typography there. The number-space witnesses (`style="padding: 0 2px;"`, `data-x="0 2"` in a link label), the unit-space witness (`data-x="41 px"`), the ellipsis witnesses (`data-x="x....x"` on span, div, and in a link label), and the angle-quote witnesses (`data-x="<<A>>"` on span and div) differ in the DOM tree while DOM signature and, except for the link-label ellipsis case, visible text match. The link-label ellipsis witness also changes visible text. The number-space and unit-space rows are unresolved FTML-owned mismatches under issue #581; the ellipsis and angle-quote rows (generic, `getAttrs`, and link-label surfaces) are unresolved under issue #582. None of the mismatch rows has a passing snapshot.

### Promote matched cases

For a new case, add `test/<group>/<case>/wikidot.html` only when a current capture of that source matches FTML across DOM tree, DOM signature, and visible text. Create the empty file, run the existing FTML update path, and inspect every generated byte:

```sh
touch test/<group>/<case>/wikidot.html
FTML_UPDATE_TESTS=1 cargo test --lib test::ast::ast -- --exact --nocapture
git diff --no-index -- /dev/null test/<group>/<case>/wikidot.html || true
```

Update mode deliberately fails after writing. It generates a derived local snapshot; by itself it is never evidence. Existing snapshots for sources whose current captures mismatch remain visible historical regressions, but a new or changed snapshot must not be promoted from a mismatch.
