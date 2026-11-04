#!/usr/bin/env python3

import argparse
import json
from pathlib import Path

try:
    from corpus_cases import load_corpus_cases
except ImportError:
    from scripts.corpus_cases import load_corpus_cases

ROOT = Path(__file__).resolve().parents[1]
BINDINGS = ROOT / "tests/fixtures/wikidot-parity/bindings.json"
SYNTAX_CASE_SCHEMA = "wikijump_syntax_differential.syntax_case.v1"


def load_candidates(bindings_path: Path = BINDINGS, root: Path = ROOT) -> list[dict]:
    bindings = json.loads(bindings_path.read_text(encoding="utf-8"))["bindings"]
    cases = load_corpus_cases(root)
    candidates = []
    for binding in bindings:
        case = cases.get(binding["case_id"])
        if binding["status"] == "match" and case is not None:
            candidates.append({"case_id": binding["case_id"], "source": case["source"]})
    candidates.sort(key=lambda case: case["case_id"])
    return candidates


def select_rotation(candidates: list[dict], slot: int, count: int) -> list[dict]:
    if not candidates:
        raise ValueError("no exact-match preview candidates")
    if count <= 0:
        raise ValueError("count must be positive")
    count = min(count, len(candidates))
    start = (slot * count) % len(candidates)
    return [candidates[(start + offset) % len(candidates)] for offset in range(count)]


def syntax_case(case: dict) -> dict:
    return {
        "schema": SYNTAX_CASE_SCHEMA,
        "case_id": case["case_id"],
        "source": case["source"],
        "title": case["case_id"],
        "wikidot_observation_tier": "page-preview",
        "local_execution_tier": "ftml",
    }


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Select a deterministic rotating sample of exact-match Wikidot parity cases."
    )
    parser.add_argument("--slot", type=int, required=True)
    parser.add_argument("--count", type=int, default=8)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    selected = select_rotation(load_candidates(), args.slot, args.count)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", encoding="utf-8") as output:
        for case in selected:
            output.write(json.dumps(syntax_case(case), ensure_ascii=False, separators=(",", ":")))
            output.write("\n")
    print(
        json.dumps(
            {
                "candidate_count": len(load_candidates()),
                "selected": len(selected),
                "slot": args.slot,
                "status": "pass",
            },
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
