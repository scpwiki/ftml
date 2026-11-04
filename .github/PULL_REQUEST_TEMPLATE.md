## Classification

- [ ] Coverage improvement
- [ ] Wikidot parity fix
- [ ] Parser/rendering behavior change without a parity claim
- [ ] Documentation or workflow only

Coverage-only PRs should stay small and independently mergeable. Do not claim that a coverage PR fixes Wikidot fixture parity unless it includes direct fixture-driven syntax or rendering evidence.

## Parity Evidence

Required when "Wikidot parity fix" is selected:

- Fixture issue or PR:
- Raw Wikidot reference JSONL and case ID(s):
- Comparator verdict:
- Checked-in binding(s):
- Derived `wikidot.html` snapshot(s) or focused regression test(s):
- Concrete expected behavior:
- Concrete previous or observed failure:

Parity claims must link to raw provenance-backed Wikidot evidence. A generated `wikidot.html` snapshot is not independent evidence, and a broad assertion that behavior is "more like Wikidot" is not enough. Caller-owned runtime cases must name the Wikijump or other caller verification lane.

## Parser Behavior Impact

- [ ] No parser changes
- [ ] Parser/rendering changes with fixture regression verified
- [ ] Parser/rendering changes with explicit non-impact rationale

If a coverage refactor changes parser or renderer behavior, include either a fixture regression check or an explicit non-impact note.

## Local Validation

Run and record the checks below before opening the pull request. GitHub Actions does not repeat them.

- [ ] `python3 scripts/check_wikijump_feature_contracts.py --wikijump-root ../wikijump`
- [ ] `python3 scripts/check_wikijump_full_pages.py --wikijump-root ../wikijump --build-renderer`
- [ ] `python3 scripts/check_wikijump_candidate.py --wikijump-root ../wikijump --ftml-root . --run-tests`
- [ ] `python3 scripts/check_comparison_normalization_browser.py --chrome "$(command -v google-chrome)" --build-renderer`
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --all-features --lib`
- [ ] `cargo test --all-features --tests`
- [ ] `RUSTFLAGS="-A unused -D warnings" cargo clippy --tests --no-deps`
- [ ] Other:
