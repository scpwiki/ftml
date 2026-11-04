#!/usr/bin/env python3

import argparse
import hashlib
import json
from pathlib import Path

try:
    from corpus_cases import corpus_source_hashes
except ImportError:
    from scripts.corpus_cases import corpus_source_hashes

ROOT = Path(__file__).resolve().parents[1]


OWNERS = {
    "plain": lambda body: f"{body}\n",
    "list": lambda body: f"* {body}\n",
    "quote": lambda body: f"> {body}\n",
    "table": lambda body: f"|| {body} ||\n",
    "heading": lambda body: f"+ {body}\n",
    "leading-space": lambda body: f" {body}\n",
}


INNER = [
    ("bold", "**INNER**"),
    ("link", "[https://example.com INNER]"),
    ("footnote", "A[[footnote]]N[[/footnote]]B"),
    ("math", "[[$x+1$]]"),
    ("span", "[[span class=\"inner\"]]INNER[[/span]]"),
]


FAILURES = {
    "same-line-div": lambda inner: f"[[div class=\"outer\"]]{inner}[[/div]]",
    "unknown-block": lambda inner: f"[[unknown]]{inner}[[/unknown]]",
    "orphan-span-close": lambda inner: f"[[/span]]{inner}",
    "unclosed-span": lambda inner: f"[[span]]{inner}",
    "rejected-parser-function": lambda inner: f"[[#unknown 1|YES|NO]]{inner}",
    "unclosed-single-link": lambda inner: f"[https://example.com {inner}",
    "orphan-div-close": lambda inner: f"[[/div]]{inner}",
    "extra-div-close": lambda inner: f"{inner}[[/div]]",
}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--fixture-root",
        type=Path,
        default=Path("target/wikidot-parity-probes/recovery"),
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        default=Path("target/wikidot-parity-probes/recovery.json"),
    )
    args = parser.parse_args()

    existing_hashes = corpus_source_hashes(ROOT)

    probes = []
    skipped_existing = []
    index = 1
    for owner_index, (owner_name, apply_owner) in enumerate(OWNERS.items()):
        for failure_index, (failure_name, apply_failure) in enumerate(FAILURES.items()):
            inner_name, inner = INNER[(owner_index + failure_index) % len(INNER)]
            source = apply_owner(apply_failure(inner))
            source_hash = hashlib.sha256(source.encode()).hexdigest()
            if source_hash in existing_hashes:
                skipped_existing.append(
                    {
                        "owner": owner_name,
                        "failure": failure_name,
                        "inner": inner_name,
                        "source_sha256": source_hash,
                    }
                )
                continue

            fixture = f"recovery-{index:03d}-{owner_name}-{failure_name}-{inner_name}"
            directory = args.fixture_root / fixture
            directory.mkdir(parents=True, exist_ok=True)
            input_path = directory / "input.ftml"
            if input_path.exists():
                assert input_path.read_text() == source, f"recovery probe drift: {input_path}"
            else:
                input_path.write_text(source)
            probes.append(
                {
                    "fixture": fixture,
                    "owner": owner_name,
                    "failure": failure_name,
                    "inner": inner_name,
                    "source_sha256": source_hash,
                }
            )
            index += 1

    args.manifest.parent.mkdir(parents=True, exist_ok=True)
    args.manifest.write_text(
        json.dumps(
            {
                "schema": "ftml.wikidot_parity.recovery_probes.v1",
                "owners": sorted(OWNERS),
                "failures": sorted(FAILURES),
                "inner": [name for name, _ in INNER],
                "probe_count": len(probes),
                "skipped_existing_count": len(skipped_existing),
                "skipped_existing": skipped_existing,
                "probes": probes,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
