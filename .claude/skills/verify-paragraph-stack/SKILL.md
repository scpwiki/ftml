---
name: verify-paragraph-stack
description: Use when a change touches src/parsing/paragraph/stack.rs (ParagraphStack and its flags/indexes into `finished`) or the order in which src/parsing/paragraph/mod.rs drives it. Checks that no order of the stack's operations makes it panic.
---

# Verify `ParagraphStack` operation orders

## What is promised, and what breaks if it is not

`ParagraphStack` (`src/parsing/paragraph/stack.rs`) is a state machine of about 15 flags plus an index into `finished` (`pending_aligned_image_separator`). `gather_paragraphs` (`src/parsing/paragraph/mod.rs`) drives it by calling `push_element`, `end_paragraph`, `end_paragraph_at_break`, `defer_suppression_seam`, `pop_line_break`, the `mark_*` methods and others in an order set by the input text.

**Property.** Whatever order those operations are called in, finishing the stack never panics: no `unreachable!`, `expect`, out-of-range index or `debug_assert` failure.

**Harm.** A panic here aborts parsing a whole page from ordinary-looking wikitext. The stack's flags remember facts about `finished` that later operations can change, so each flag is a chance for a stale fact.

## Run it

```sh
verification/paragraph-stack/run.sh            # the check, ~5 s once built
verification/paragraph-stack/run.sh mutants    # can-it-fail self-check, ~4 min; expect 4 "caught:" lines
```

Both compile Rust. The check is the `#[ignore]` test `no_operation_order_panics_the_paragraph_stack` in `src/parsing/paragraph/stack_orders.rs` (compiled only under `cfg(test)`; plain `cargo test` skips it). `run.sh` runs it and compares what fails with `verification/paragraph-stack/known-findings.txt`. Exit 0 means "the failures are exactly the known ones". To widen the search: `FTML_STACK_OPS=6 verification/paragraph-stack/run.sh` (about 17x longer; the result at 6 was the same single root cause with 143 variants).

## Tool and why

Exhaustive enumeration on the real type. Every order of up to 5 operations (17 operations, including element pushes of text, line break, block, footnote, simple table, aligned image, plain image, and a suppression seam) is replayed on a fresh `ParagraphStack`, in both `new()` and `new_wikidot()` modes, with `catch_unwind`. Replay stops extending an order once it fails, so every report is a shortest failing order. TLA+ and Kani were not used: the flags are Rust-level facts about real `Element` values, and a hand model of ~15 flags would not be checkable against the code; Kani did not scale on the neighbouring recursive list code (see `verify-process-depths`).

## Current result: no known findings

`known-findings.txt` is empty, so `run.sh` passes only when no order of up to 5 operations panics (also true at `FTML_STACK_OPS=6`).

History: the check found one latent panic, now fixed. Order: `text, end_paragraph_at_break, defer_suppression_seam, end_paragraph, push_footnote` made `push_element` hit `unreachable!("footnote reopening requires a preceding paragraph")` in the Wikidot layout, because `end_paragraph` flushed the seam into `finished` but left `wikidot_reopen_for_footnote` armed. `end_paragraph` now clears that flag whenever it appends to `finished`; the regression test is `footnote_after_seam_flushed_past_a_break_does_not_reopen_the_paragraph` in `stack.rs`. The finding was never reproduced from real wikitext (84 candidate pages), so it was latent.

## Scope: what a pass covers

- Verified: no panic for any order of up to 5 of the 17 listed operations, from a fresh stack, both modes, with the listed element shapes only.
- Not covered: orders longer than 5; element shapes not in the list (other blocks, lists, images with links, delayed images, collapsibles); the parser's real call patterns (the check is stronger than reality, which is why a pass or a finding must be read against `gather_paragraphs`); whether the output tree is *right* (only that it exists).
- A pass means "matches known findings" (currently none), within those bounds.

## Coverage record and sync commit

| Checked code | Operations the check calls |
| --- | --- |
| `src/parsing/paragraph/stack.rs`: `ParagraphStack::{new, new_wikidot, push_element, defer_suppression_seam, end_paragraph, end_paragraph_at_break, pop_line_break, merge_wikidot_adjacent_simple_tables, mark_current_unwrapped, mark_next_unwrapped, mark_wikidot_terminal_backslash, ensure_wikidot_trailing_line_break, mark_discarded_control, mark_wikidot_advanced_table_cell, into_elements}` | list `OPS` / `apply()` in `stack_orders.rs` |

Not called by the check, but able to change the same flags: `mark_next_unwrapped_after_block`, `mark_wikidot_same_line_div_residual`, `mark_wikidot_literal_div_line`, `mark_wikidot_literal_iftags_line`, `mark_wikidot_simple_table_boundary`, `mark_wikidot_tabview_boundary`, `mark_wikidot_continued_block_boundary`, `mark_wikidot_invisible_line_occupancy`, `mark_wikidot_complete_raw_line_occupancy`, `mark_wikidot_empty_list_break`, `record_next_element_line_start`, `push_paragraph_safe_elements`, `push_errors`, `into_result`.

**Sync commit: `1375d8ac6`** (the commit that fixed the reopen flag and emptied the known findings; the check was confirmed against `stack.rs` and `paragraph/mod.rs` as of that commit).

Staleness test: `git diff <sync commit> -- src/parsing/paragraph/stack.rs src/parsing/paragraph/mod.rs`. If it shows a new `pub`/`pub(crate)` method, a new flag, a new `finished` mutation, or a changed caller order, add the operation to `OPS` and `apply()` in `stack_orders.rs` (keep the two in step; `apply` ends in `unreachable!` for unknown numbers), rerun both commands, and only then move the sync commit to the new `HEAD`.

## Reading a result

- `MATCHES known findings`: no change in behaviour found. Done.
- `+ <message>|<n>` not previously known: a **new finding**, or a shorter order for a known message. Run the test directly to see the full order: `cargo test --lib stack_orders -- --ignored --nocapture` and read the `MINIMAL-FAILURE` line (operation names in call order). Replay it by hand in a scratch `#[test]` to confirm. Real bug if the parser can reach that order (check `gather_paragraphs`); otherwise latent, report it and add it to `known-findings.txt` only with the user's agreement.
- `- <message>|<n>` known but gone: you fixed or masked it. Delete the line. If you changed a guard rather than fixed the cause, check the order still behaves (rerun with `FTML_STACK_OPS=6`).
- `CHECK DID NOT RUN`: build error or the test was renamed; fix that first.
- `MUTANT SURVIVED`: the check got weaker or the mutated line changed meaning; if a mutant's search text no longer occurs once, the script's assertion says so; update the text in `run.sh`.

## Mutants: proof that the check can fail

`run.sh mutants` copies the repo (minus `target/` and `.git`) to a temp dir and applies four breaks to `stack.rs`: table merge panics when the last element is not a table; `pop_line_break` removes the wrong index in `current`; footnote reopening armed with no preceding paragraph (a shorter version of the known finding); the same wrong index in `finished`. Each must produce a failure absent from `known-findings.txt`. An earlier candidate (removing the bounds guard around `pending_aligned_image_separator`) was dropped as an equivalent mutant: no reachable order of the 17 operations makes that index stale.

## Prerequisites

Rust toolchain from `rust-toolchain.toml`, Python 3 with `-I`. Uses the canonical `target/` for `run.sh`; `run.sh mutants` uses a temporary target dir and deletes it (about 3 minutes of compiling).
