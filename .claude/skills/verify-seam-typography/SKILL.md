---
name: verify-seam-typography
description: Use when a change touches src/delayed/seam.rs (rewrite_elements, flush_group, resolve_static_suppressions, resolve_bound_suppressions, dash and text seam edits) or wikidot_seam_edits / native_seam_edits / substitute_wikidot in src/preproc/typography.rs. Checks that removing a suppressed block between two text segments does not change the typography of the surrounding text.
---

# Verify seam transparency of Wikidot typography

## What is promised, and what breaks if it is not

No production caller typographs text segments one at a time. The preprocess path typographs the whole source at once (`substitute_wikidot_deferring_conditionals` on the Wikidot path), and Deepwell typographs the whole row template before allocating its slots. Suppressed blocks (`[[iftags]]` bodies that are hidden, comments, hidden generated values) are removed after parsing; each removal leaves a **seam**. `wikidot_seam_edits` and `collect_dash_edits` then redo the typography matches whose character run touches a seam, so that the result reads as if the suppressed block had never been in the source.

**Property, modeled on the real path.** Join the authored segments with a hidden block between neighbours, `seg0 + BLOCK + seg1 + ...` where `BLOCK` is `[[iftags +zz-no-such-tag]]x[[/iftags]]`, and run the preprocess-path typography on the whole source. Split the output on the literal block, insert a typography boundary between the pieces, and resolve the seams with `resolve_static_suppressions(.., true)`. The joined text must equal `substitute_wikidot` of the plain concatenation of the raw segments. It must also not panic and leave no seam behind. An end-to-end layer (`seams_are_transparent_end_to_end`, bound 4 with an `a` prefix, full preprocess + parse + render) covers the parser itself; the split harness alone does not.

**Harm.** Quotes, `<<` `>>`, and `...` next to a hidden block come out different from the same text without the block: a straight quote where a curly one belongs, or the reverse.

## Run it

```sh
verification/seam-typography/run.sh            # the check, ~30 s once built; "MATCHES known finding classes" is a pass
verification/seam-typography/run.sh mutants    # can-it-fail self-check, ~6 min; expect 3 "caught:" lines
```

Both compile Rust. The check is the `#[ignore]` test `seams_are_transparent_to_wikidot_typography` in `src/delayed/seam_equivalence.rs` (compiled only under `cfg(test)`). `run.sh` compares the failure classes it finds with `verification/seam-typography/known-findings.txt`. `FTML_SEAM_TOKENS=6` is a 12x larger run (about 10 minutes); under the current model it reports the same three `right-angle:` classes as bound 5. It is for investigations, not routine use.

## Tool and why

Exhaustive enumeration through the real functions. Every sequence of up to 5 items from 11 text tokens (`"` `'` `a` space `.` `...` `<` `>` `<<` `>>` `é`) and a seam is built into segments joined by the hidden block (271,453 inputs), run through the preprocess-path typography, split, and resolved, then compared with the oracle above. A failure is named by the characters on which the two strings disagree (sign only), so one root cause is one class. A failure whose input contains a `>` or `>>` token is prefixed `right-angle:`, so no other class can hide among the known right-angle ones.

## Current result: only the right-angle classes remain

The `<` and ellipsis classes are fixed (ftml #775). The fix is **in-band left-edge deferral**, and it replaces the earlier direction that typography must record which characters it produced and pass them to the seam code (a provenance API): a deferred run is left *unconverted*, and the raw run is itself the record. It is unambiguous because typography never produces `<` or `.`, so every such character in the joined text is authored text.

- `substitute_wikidot_deferring_conditionals` (crate-private, called only from `preprocess_internal`) leaves a maximal `<` run or dot-and-space run starting directly after the `]]` of an opener or closer of a balanced, non-literal `[[iftags]]` pair unconverted. Grouping is left to right, so only the run that *starts* after a seam can be misgrouped; a run ending at a seam is already grouped correctly. The public `substitute_wikidot` is unchanged and takes self-contained text; Deepwell's row path, which calls it after preprocess, converts whatever was deferred, exactly as before.
- In `wikidot_seam_edits`, a left-angle or ellipsis match is accepted when its maximal same-family run (`<`, or `.`/space) touches or contains a seam, a seam at either run end counting, and every match inside the run converts. This mirrors the `>` run expansion below.
- Two parse-level pieces make the deferral reach the seam code in plain parse: the double-angle rule fails a `<<` token under the Wikidot layout (preprocess has already converted every `<<` in ordinary text; protected owners consume theirs elsewhere), so the token falls back to raw text instead of pairing within its segment; and the paragraph stack keeps a single trailing seam of a rule result in place, so a shown body's closing boundary records the seam at the end of the body, not its start.
- Regression tests: `plain_parse_hidden_iftags_typography_matches_joined_text` in `src/delayed/seam_equivalence.rs` (issue table, shown body at both tag edges, non-text body, negatives, authored literal `«` across a seam), the delayed rows in `src/test/delayed.rs`, and the typography-level cases for the deferring entry.

The `>>` run class was fixed earlier (#777): `wikidot_seam_edits` converts every pair of a `>` run that touches a seam, because typography never produces `>`, so each `>` is authored text. Regression test: `right_angle_run_touching_a_suppressed_seam_converts_every_pair` in `src/test/delayed.rs`.

**Remaining: three `right-angle:` classes.** In the harness model the pieces are typographed with the block's `]` as left context, which makes some `>>` pairs eligible that the joined text's real left context would group differently (or leave unconverted at a run start): `">` seam `>>>` gives `">»>` where the joined text gives `"»»`, and an empty segment followed by `>>` converts where the joined text, the run sitting at the very start, does not. These are kept out of the #775 fix: there is no live evidence for them yet, and a fix needs tree left-context in `flush_group` (converting blindly would regress `**b**` + hidden block + `>>`, which is currently correct). Get live evidence first, then open the follow-up.

**Live Wikidot evidence** (`verification/seam-typography/evidence/wikidot-2026-11-01.json` and `wikidot-2026-11-02.json`, captured on http://sandbox-for-codex.wikidot.com/ with a run-owned page, since deleted): a hidden comment or false `[[iftags]]` between two segments renders exactly as if the block were absent, and a *shown* iftags body behaves as if the tags were absent too (`<[[iftags -zz-no-such-tag]]<<<[[/iftags]]` renders `««`; a non-text body keeps the runs on either side apart: `<b«<`). So the oracle is right for these classes. The 2026-11-02 file also records two block-level anomalies that are not typography cases (an iftags at a paragraph start rendering its opener literally, and an unclosed opener being dropped by Wikidot); they are out of scope here.

**Plain parse.** Plain FTML parse evaluates `[[iftags]]` during parsing (`check_iftags(parser.page_info(), ..)`) and resolves false blocks through the same `resolve_static_suppressions`. Before #775 it shared the seam limit for `<<` and ellipsis; it is fixed by the same deferral, because plain parse is the preprocess path the harness models. Hidden comments match Wikidot in every evidence case.

`,,` was dropped from the alphabet: it is subscript markup in Wikidot, so `,,` + `,,''` is not a typography case (live Wikidot and FTML both render `''` with or without a hidden block).

Do not narrow the alphabet to hide a class. When a class is fixed, delete its line from `known-findings.txt` in the same change and extend the evidence file.

## Scope: what a pass covers

- Verified: the failure classes above are the only ones for inputs of up to 5 tokens from the 11-token alphabet, `apply_typography = true` (Wikidot), `TypographyBoundary` seams, text elements only. The end-to-end layer additionally covers the parser at bound 4 (with an `a` prefix, so the block is never at a paragraph start).
- Not covered: dash runs (`--`, `---`) across a seam (they become separate dash elements, not text, so the alphabet excludes them; `collect_dash_edits` is unchecked); the native layout (`apply_typography = false`, `native_seam_edits`); digit and unit spacing (`replace_number_spaces`, `replace_unit_spaces`); newlines and paragraph boundaries; bound suppressions with generated values (`resolve_bound_suppressions` error paths); non-text elements between segments (plain-parse regression tests cover one non-text body shape); inputs longer than 5 tokens.
- Also uncovered, recorded with the #775 fix: comment gates restored after typography (`[!-- ]]`-style gates restored in the delayed pipeline, `src/delayed.rs`) keep the old limit, because no deferral ever applied to them; a protected raw run (for example an unpaired math fallback) that touches a seam can now convert, which #777 already accepted for `>`; Deepwell's ListPages row path keeps `<` seam `<<<` wrong because it strips iftags lexically after typography — that belongs to Wikijump, not to this check.
- A pass means "the failure classes are exactly the known ones", not "no differences". The known ones are parity differences with live Wikidot, so a pass does not mean the code is right.

## Coverage record and sync commit

| Checked code | Entry point |
| --- | --- |
| `src/delayed/seam.rs`: `resolve_static_suppressions`, `rewrite_elements`, `flush_group`, `emit_source_range` | `resolve_static_suppressions` |
| `src/preproc/typography.rs`: `wikidot_seam_edits`, `substitute_wikidot` (as oracle) | via `flush_group` and the oracle |
| `src/preproc/typography.rs`: `substitute_wikidot_deferring_conditionals`, `conditional_deferral_spans` | via the harness's preprocess-path pass |
| `src/parsing/rule/impls/double_angle.rs` (`<<` fallback under the Wikidot layout), `src/parsing/paragraph/mod.rs` (trailing-seam placement) | end-to-end layer only |

Not exercised: `collect_dash_edits`, `range_crosses_seam` in `seam.rs` (dash path only), `native_seam_edits`, `resolve_bound_suppressions`.

**Sync commit: `1466c1994`** (`seam.rs`, `typography.rs` and `delayed.rs`, including the `>>` fix).

Staleness test: `git diff <sync commit> -- src/delayed/seam.rs src/preproc/typography.rs src/delayed.rs src/parsing/paragraph/mod.rs src/parsing/rule/impls/double_angle.rs`. If it shows changes to the seam rules, the typography regexes, or `TypographyBoundary` handling, rerun both commands, update `known-findings.txt` only for classes you understand, and only then move the sync commit to the new `HEAD`.

## Reading a result

- `MATCHES known finding classes`: unchanged. Done.
- `+ <class>|<len>|<count>` unknown class, or a known class with a different shortest length or count: behavior of the seam code changed. Run `cargo test --lib seam_equivalence -- --ignored --nocapture` and read the `CLASS` line: it ends with the shortest example, as `segments "a" | "b"` (`|` separates segments, a hidden block sits between them), what the code produced and what the joined text produces. Decide with live Wikidot output, not by editing the oracle.
- `- <class>`: a class went away. If you meant to fix it, delete the line. If you did not, a mutation or a rule change is hiding it.
- `CHECK DID NOT RUN`: build error or the test was renamed.

## Mutants: proof that the check can fail

`run.sh mutants` copies the repo to a temp dir and breaks `src/delayed/seam.rs` three ways: seam edits never applied; text after an edit emitted from the wrong place; seam positions not sorted and de-duplicated. Each must change the class list or the failure counts. A candidate that edits `range_crosses_seam` in `seam.rs` was dropped as equivalent here (that function serves the dash path only, outside the alphabet).

## Prerequisites

Rust toolchain from `rust-toolchain.toml`, Python 3 with `-I`. Uses the canonical `target/`; `run.sh mutants` uses a temporary target and deletes it.
