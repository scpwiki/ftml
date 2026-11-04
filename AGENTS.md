# FTML

## Product and documentation

- `README.md`, `docs/WikijumpBoundary.md`, the workspace manifests, and nearby parser/render fixtures.
- FTML provides deterministic parsing and rendering primitives. Behavior requiring a site, page database, actor, URL arguments, query, import state, file service, permissions, or browser runtime belongs in Wikijump or another caller.
- For Wikidot compatibility, live Wikidot evidence and provenance-backed corpus examples outrank assumptions and local Wikijump output.
- Corpus expectations are staged: `tree.json` records the parse tree only; rendering is judged from `wikidot.html` and `output.html`. Some constructs exist in only one layout (Wikijump-native blocks render literally in the Wikidot layout); judge each construct in the layout where it exists.
- Evidence from Wikidot's PagePreview route can differ from saved pages, notably for `[[iftags]]` gate evaluation; treat PagePreview-only verdicts as provisional until a saved page confirms them.

## Development

- Search existing tests and helpers before adding parser or renderer behavior. Add focused positive and negative regression fixtures for changed syntax.
- A behavior confirmed against live Wikidot is pinned once, as a corpus fixture whose directory and input carry the intent; do not duplicate the same shape as a per-shape Rust test. Adjudicate a suspected divergence against existing pins and frozen parity captures before probing live.
- `FTML_UPDATE_TESTS=1` rewrites unrelated fixtures (a known `rel="noopener"` drift); revert every rewrite outside the fixtures the change owns before committing.
- Prefer simple local invariants over one-off abstraction. Split large modules when their responsibilities no longer fit clearly in one place.
- Remove task-owned branches, worktrees, target directories, and build artifacts when they are no longer useful.
- Cargo targets: normal development uses the canonical `target/` and compact profiles in `Cargo.toml`; candidate and one-shot CI builds use revision/role-specific `CARGO_TARGET_DIR` with `CARGO_INCREMENTAL=0`. Read `docs/development/cargo-target-policy.md` before changing build or cleanup behavior.

## Code Review Rules

- Treat observable Wikidot behavior as the compatibility specification for `Layout::Wikidot` even when that behavior is insecure, unsafe, obsolete, or contrary to modern web best practices. A review objection based only on modern hardening is not a compatibility justification.
- For security-sensitive behavior, first establish what Wikidot actually does from live evidence or provenance-backed observations. When Wikidot demonstrably permits the behavior, the default compatibility disposition is to reproduce it and document the concern rather than silently harden the renderer or parser.
- Put discretionary security controls at the caller's deployment, network, privilege, or isolation boundary when possible instead of changing Wikidot-visible parsing or rendering semantics. Review FTML's Wikidot layout as a local compatibility component, not as a public Internet security boundary by default.
- Accept a deliberate divergence from evidenced Wikidot behavior only when reproducing it would violate an explicit deployment boundary or create a materially greater capability than Wikidot itself exposes. Record the evidence, the reason for divergence, and the containment boundary instead of appealing generically to "security" or "best practice".
- Treat HTTP resources, URL schemes, link attributes such as `noopener`, escaping, sanitization, and similarly security-relevant output as parity questions first: if Wikidot preserves or omits something observably, match that behavior unless the preceding divergence rule applies.
- Distinguish faithful reproduction of a Wikidot weakness from a vulnerability introduced by FTML. New capability or exposure that Wikidot does not exhibit remains an ordinary defect and should be reviewed as such.
- Compatibility covers malformed-input behavior and failure shapes too: a divergence is not excused because live's output looks broken, and it is never parked as "pathological on both sides". When live's underlying rule is not yet known, determine it from evidence and then reproduce it.

## Validation and delivery

- Choose validation in proportion to the changed surface. Typical commands are `cargo fmt --check`, focused `cargo test`, the full suite for general parser or renderer changes, and `RUSTFLAGS='-D warnings' cargo clippy --tests --no-deps`.

## Agent skills

### Issue tracker

Issues live in GitHub Issues and use the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Use the default five triage labels. See `docs/agents/triage-labels.md`.

### Domain docs

This is a single-context repo. See `docs/agents/domain.md`.

Agents run builds and tests autonomously in their own worktrees; no approval is required.
