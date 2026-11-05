# Inline-crossing live probes

Evidence corpus for the crossed inline construct repair in the Wikidot
layout. `probes.json` holds 63 sources; `live-results.json` holds the live
Wikidot bytes for each, captured twice.

## Provenance

- Source of the shapes: the 63-shape probe list recorded by the
  `claude/wikidot-nested-crossing-inversion-t78ibw` session report. The list
  was rebuilt from that report's shape families (double-shell pairs inside a
  deferred strike/color shell, three-shell variants, shifted/trailing text,
  missing and extra closers, no-color and no-strike variants, plain
  crossings, nested blue color).
- Captured: 2026-11-05, anonymous `edit/PagePreviewModule` requests against
  `sandbox-for-codex`, two runs per case, ~0.35 s apart. All 63 cases were
  byte-identical across their two runs.
- No saved pages were created; PagePreview only.

## Re-running the capture

The client is the local wikidot.py fork, which is only importable from its
own uv environment:

```sh
cd /home/roku/src/Rokurolize/wikidot.py
uv run python /path/to/misc/inline-crossing-probes/probe-live.py
```

`probe-live.py [batch.json]` merges into `live-results.json`, skips cached
ids whose source is unchanged, and refuses to reuse an id for a new source.

## Comparing an FTML build

Render the probe set with the `render_html_jsonl` example:

```sh
python3 - <<'PY' > /tmp/probes-input.jsonl
import json
for case_id, source in json.load(open("probes.json")).items():
    print(json.dumps({
        "schema": "wikijump_syntax_differential.syntax_case.v1",
        "case_id": case_id,
        "source": source,
        "title": "probe",
        "layout": "wikidot",
    }))
PY
cargo build --example render_html_jsonl
./target/debug/examples/render_html_jsonl < /tmp/probes-input.jsonl > /tmp/ftml.jsonl
python3 compare.py /tmp/ftml.jsonl
```

`compare.py` normalizes both sides: HTML entities are decoded, a single outer
`<p>` is unwrapped, `style` declarations are normalized, `<br>` spellings are
unified, and ASCII whitespace runs are collapsed. It prints the agreement
count and one block per disagreement, and `--json` writes a machine-readable
summary.

## Baseline

- Pristine `origin/main` (a6700b892): 7/63.
- Cloud branch's narrow two-shell fix: 27/63.
- General deferred-strike shell repair (this branch): 40/63.

The remaining 23 are listed in the pull request body with their divergence
class.
