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


PATTERNS = {
    "crossed-three-delimiters": "**A //B __C** X// Y__",
    "link-label-parser-function": "[https://example.com [[#if 1 | YES | NO ]]]",
    "link-label-footnote": "[https://example.com A[[footnote]]N[[/footnote]]B]",
    "same-line-div-with-link": "[[div class=\"x\"]][https://example.com label][[/div]]",
    "same-line-div-with-format": "[[div class=\"x\"]]**B**[[/div]]",
    "unknown-block-with-format": "[[unknown]]**B**[[/unknown]]",
    "raw-around-parser-function": "@@[[#if 1 | YES | NO ]]@@",
    "comment-joins-link-target": "[https://exam[!--join--]ple.com label]",
}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--fixture-root",
        type=Path,
        default=Path("target/wikidot-parity-probes/interaction"),
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        default=Path("target/wikidot-parity-probes/interaction.json"),
    )
    args = parser.parse_args()

    existing_hashes = corpus_source_hashes(ROOT)

    probes = []
    skipped_existing = []
    index = 1
    for owner_name, apply_owner in OWNERS.items():
        for pattern_name, body in PATTERNS.items():
            source = apply_owner(body)
            source_hash = hashlib.sha256(source.encode()).hexdigest()
            if source_hash in existing_hashes:
                skipped_existing.append(
                    {
                        "owner": owner_name,
                        "pattern": pattern_name,
                        "source_sha256": source_hash,
                    }
                )
                continue
            fixture = f"interaction-{index:03d}-{owner_name}-{pattern_name}"
            directory = args.fixture_root / fixture
            directory.mkdir(parents=True, exist_ok=True)
            input_path = directory / "input.ftml"
            if input_path.exists():
                assert input_path.read_text() == source, f"interaction probe drift: {input_path}"
            else:
                input_path.write_text(source)
            probes.append(
                {
                    "fixture": fixture,
                    "owner": owner_name,
                    "pattern": pattern_name,
                    "source_sha256": source_hash,
                }
            )
            index += 1

    args.manifest.parent.mkdir(parents=True, exist_ok=True)
    args.manifest.write_text(
        json.dumps(
            {
                "schema": "ftml.wikidot_parity.interaction_probes.v1",
                "owners": sorted(OWNERS),
                "patterns": sorted(PATTERNS),
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
