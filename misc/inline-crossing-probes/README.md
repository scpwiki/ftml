# Inline-crossing live probes

Evidence corpus for crossed inline constructs in the Wikidot layout.
`probes.json` holds 122 sources; `live-results.json` holds the live Wikidot
bytes for each, captured twice. See `docs/WikidotInlineNesting.md` for the rule
these captures pin.

The first 63 are the crossed-shell shape set. `m1`-`m8` probe the monospace rule
directly: they ask when `{{`/`}}` pairs at all, including across an enclosing
strike or color shell. The 14 `*-p2` probes add a paragraph boundary. The 37
`odm-*` probes were predicted before capture, from Text_Wiki and libtidy, and
all 37 predictions held.

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
- The `m1`-`m8` monospace probes and the 14 `-p2` paragraph probes were
  captured later the same day through the same anonymous route and are also
  byte-identical across their two runs.
- The 37 `odm-*` probes were captured on 2026-11-05 through the same route.
- These captures are PagePreview only. Saved pages `muse-probe-odm-*` on the same
  site render the same bytes for the 12 shapes checked.

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

Original 85 probes (63 crossed shapes, `m1`-`m8`, 14 `*-p2`):

- General deferred-strike shell repair (#1003): 40/63 on the crossed set.
- Stolen-marker re-pairing (#1010): 61/63, `m1`-`m8` 5/8, `*-p2` 14/14.
- Token-level pairing and the exact inline stack: 85/85.

Pre-registered `odm-*` set: 37/37 with the token-level pairing.
