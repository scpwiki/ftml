#!/usr/bin/env python3

import argparse
import json
from pathlib import Path

try:
    from corpus_cases import corpus_case_ids
except ImportError:
    from scripts.corpus_cases import corpus_case_ids

ROOT = Path(__file__).resolve().parents[1]
CONTRACTS = ROOT / "tests/fixtures/wikidot-parity/wikijump-feature-contracts.json"
SCHEMA = "ftml.wikijump_feature_contracts.v2"
PROPERTY_AXES = {f"P{index}" for index in range(1, 9)}
PROPERTY_OWNERS = {"ftml", "wikijump", "shared", "not-applicable"}


def load_case_ids() -> set[str]:
    return corpus_case_ids(ROOT)


def check(wikijump_root: Path, contracts_path: Path = CONTRACTS) -> list[str]:
    ledger_path = wikijump_root / "docs/wikidot-specifications/implementation-ledger.json"
    if not ledger_path.is_file():
        return [f"missing Wikijump implementation ledger: {ledger_path}"]
    ledger = json.loads(ledger_path.read_text(encoding="utf-8"))
    features = ledger["features"]
    syntax_features = {feature for feature in features if feature.startswith("syntax-")}
    cases = load_case_ids()
    manifest = json.loads(contracts_path.read_text(encoding="utf-8"))
    contracts = manifest.get("contracts", [])
    errors = []
    if manifest.get("schema") != SCHEMA:
        errors.append(f"invalid feature contract schema: {manifest.get('schema')!r}")
    seen = set()
    seen_features = set()
    for contract in contracts:
        cid = contract.get("id", "<missing-id>")
        if cid in seen:
            errors.append(f"duplicate feature contract id: {cid}")
        seen.add(cid)
        feature = contract.get("wikijump_feature_id", "")
        if feature in seen_features:
            errors.append(f"duplicate Wikijump syntax feature contract: {feature}")
        seen_features.add(feature)
        if feature not in features:
            errors.append(f"{cid}: Wikijump feature {feature} is absent from the ledger")
        case_ids = contract.get("cases", [])
        if not case_ids:
            errors.append(f"{cid}: feature contract needs at least one stable FTML case")
        for case_id in case_ids:
            if case_id not in cases:
                errors.append(f"{cid}: FTML parity case does not exist: {case_id}")
        owners = contract.get("property_owners")
        if not isinstance(owners, dict) or set(owners) != PROPERTY_AXES:
            errors.append(f"{cid}: property_owners must cover exactly P1-P8")
        elif any(owner not in PROPERTY_OWNERS for owner in owners.values()):
            errors.append(f"{cid}: property owner must be ftml/wikijump/shared/not-applicable")
    missing = sorted(syntax_features - seen_features)
    extra = sorted(seen_features - syntax_features)
    if missing:
        errors.append(f"missing Wikijump syntax feature contracts: {','.join(missing)}")
    if extra:
        errors.append(f"non-syntax or stale Wikijump feature contracts: {','.join(extra)}")
    return errors


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Verify FTML parity property families against the adjacent Wikijump compatibility ledger."
    )
    parser.add_argument("--wikijump-root", required=True, type=Path)
    args = parser.parse_args()
    errors = check(args.wikijump_root.resolve())
    if errors:
        raise SystemExit("\n".join(errors))
    contracts = json.loads(CONTRACTS.read_text(encoding="utf-8"))["contracts"]
    print(json.dumps({"contracts": len(contracts), "status": "pass"}, sort_keys=True))


if __name__ == "__main__":
    main()
