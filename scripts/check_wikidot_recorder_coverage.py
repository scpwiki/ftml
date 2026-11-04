#!/usr/bin/env python3

import argparse
import hashlib
import json
from collections import Counter, defaultdict
from pathlib import Path

from wikidot_parity_fingerprint import fingerprint, primary_family

try:
    from corpus_cases import load_corpus_cases
except ImportError:
    from scripts.corpus_cases import load_corpus_cases

ROOT = Path(__file__).resolve().parents[1]


PAGE_PREVIEW_CLASSES = {"saved-page-batch", "page-preview-isolated"}


def read_jsonl(path: Path) -> list[dict]:
    return [json.loads(line) for line in path.read_text().split("\n") if line.strip()]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--recorded-cases", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--require-no-zero-modules", action="store_true")
    args = parser.parse_args()

    recorded = read_jsonl(args.recorded_cases)
    corpus = load_corpus_cases(ROOT)
    frozen_hashes = {case["source_sha256"] for case in corpus.values()}

    module_hashes: dict[str, set[str]] = defaultdict(set)
    module_preview_hashes: dict[str, set[str]] = defaultdict(set)
    module_frozen_hashes: dict[str, set[str]] = defaultdict(set)
    fingerprint_hashes: dict[str, set[str]] = defaultdict(set)
    covered_fingerprints: set[str] = set()
    primary_by_fingerprint: dict[str, str] = {}
    preview_hashes: set[str] = set()

    for row in recorded:
        source_hash = row["source_sha256"]
        preview_compatible = row["execution_class"] in PAGE_PREVIEW_CLASSES
        source_fingerprint = fingerprint(row["source"]) if preview_compatible else None

        if preview_compatible:
            preview_hashes.add(source_hash)
            assert source_fingerprint is not None
            fingerprint_hashes[source_fingerprint].add(source_hash)
            primary_by_fingerprint.setdefault(source_fingerprint, primary_family(row["source"]))
            if source_hash in frozen_hashes:
                covered_fingerprints.add(source_fingerprint)

        for origin in row.get("record_origins", []):
            test_name = origin.get("test_name", "")
            module = test_name.split("::", 1)[0] if test_name else ""
            if not module:
                continue
            module_hashes[module].add(source_hash)
            if preview_compatible:
                module_preview_hashes[module].add(source_hash)
                if source_hash in frozen_hashes:
                    module_frozen_hashes[module].add(source_hash)

    preview_modules = sorted(module for module, hashes in module_preview_hashes.items() if hashes)
    zero_modules = sorted(module for module in preview_modules if not module_frozen_hashes[module])
    uncovered_fingerprints = sorted(set(fingerprint_hashes) - covered_fingerprints)
    uncovered_by_primary = Counter(primary_by_fingerprint[value] for value in uncovered_fingerprints)
    unobserved_preview_hashes = preview_hashes - frozen_hashes

    output = {
        "schema": "ftml.wikidot_parity.recorder_coverage.v1",
        "recorded_cases_sha256": hashlib.sha256(args.recorded_cases.read_bytes()).hexdigest(),
        "recorded_case_count": len(recorded),
        "corpus_case_count": len(corpus),
        "page_preview_source_hash_count": len(preview_hashes),
        "unobserved_page_preview_source_hash_count": len(unobserved_preview_hashes),
        "module_count": len(module_hashes),
        "page_preview_module_count": len(preview_modules),
        "zero_frozen_live_modules": zero_modules,
        "fingerprint_count": len(fingerprint_hashes),
        "covered_fingerprint_count": len(covered_fingerprints),
        "uncovered_fingerprint_count": len(uncovered_fingerprints),
        "uncovered_fingerprints_by_primary": dict(sorted(uncovered_by_primary.items())),
        "uncovered_fingerprints": uncovered_fingerprints,
        "modules": {
            module: {
                "recorded_source_hashes": len(module_hashes[module]),
                "page_preview_source_hashes": len(module_preview_hashes[module]),
                "frozen_source_hashes": len(module_frozen_hashes[module]),
            }
            for module in sorted(module_hashes)
        },
    }
    args.output.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n")

    if args.require_no_zero_modules and zero_modules:
        raise SystemExit(f"PagePreview-compatible modules without frozen live witnesses: {zero_modules}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
