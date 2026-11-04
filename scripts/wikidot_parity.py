#!/usr/bin/env python3
"""Bind and summarize Wikijump syntax-differential evidence."""

import argparse
import hashlib
import json
import re
import sys
import tomllib
from collections import Counter
from datetime import datetime
from pathlib import Path

try:
    from corpus_cases import load_corpus_cases
except ImportError:
    from scripts.corpus_cases import load_corpus_cases


ROOT = Path(__file__).resolve().parents[1]
LIVE_CASE_SCHEMA = "wikijump_syntax_differential.live_case.v1"
CASE_SCHEMA = "wikijump_syntax_differential.syntax_case.v1"
REFERENCE_SCHEMA = "wikijump_syntax_differential.wikidot_reference.v1"
VERDICT_SCHEMA = "wikijump_syntax_differential.verdict.v1"
COMPARISON_SCHEMA = "wikijump_syntax_differential.syntax_comparison.v1"
BINDINGS_SCHEMA = "ftml.wikidot_parity.bindings.v1"
CHECKS = ("dom_tree", "dom_signature", "visible_text")
STATUSES = {"match", "mismatch"}
DISPOSITIONS = {
    "unresolved",
    "caller-runtime",
    "comparison-normalization",
    "security-boundary",
}
ACTIVE_INVESTIGATION_REASON = re.compile(r"Active functional investigation: issue #[1-9][0-9]*\.")


def fail(message):
    raise ValueError(message)


def sha256(value):
    return hashlib.sha256(value.encode()).hexdigest()


def is_sha256(value):
    return isinstance(value, str) and len(value) == 64 and all(
        character in "0123456789abcdef" for character in value)


def validate_provenance(reference, case_id):
    captured_at = reference.get("captured_at")
    try:
        captured = datetime.fromisoformat(captured_at.replace("Z", "+00:00"))
    except (AttributeError, ValueError):
        fail(f"invalid capture time for {case_id}")
    if (not re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})", captured_at) or
            captured.tzinfo is None):
        fail(f"invalid capture time for {case_id}")
    provenance = reference.get("provenance")
    if (not isinstance(provenance, dict) or
            provenance.get("module") != "edit/PagePreviewModule" or
            provenance.get("authenticated") is not False or
            provenance.get("mutated") is not False or
            any(not isinstance(provenance.get(name), str) or not provenance[name]
                for name in ("site", "site_domain", "wikidot_py_version")) or
            not isinstance(provenance.get("wikidot_py_commit"), str) or
            not re.fullmatch(r"[0-9a-f]{40}", provenance["wikidot_py_commit"]) or
            not is_sha256(provenance.get("requirements_sha256"))):
        fail(f"invalid acquisition provenance for {case_id}")


def records(path):
    try:
        text = Path(path).read_text(encoding="utf-8")
    except OSError as error:
        fail(f"{path}: {error.strerror}")
    try:
        value = json.loads(text)
        return value if isinstance(value, list) else [value]
    except json.JSONDecodeError:
        try:
            # JSON Lines records are delimited by LF. `str.splitlines()` also
            # treats authored NEL, U+2028, and U+2029 inside JSON strings as
            # record boundaries, which corrupts valid parity sources.
            return [json.loads(line) for line in text.split("\n") if line.strip()]
        except json.JSONDecodeError as error:
            fail(f"{path}: invalid JSON: {error}")


def unique(values, label):
    result = {}
    for value in values:
        case_id = value.get("case_id") if isinstance(value, dict) else None
        if not isinstance(case_id, str) or not case_id or case_id in result:
            fail(f"{label} has an invalid or duplicate case_id")
        result[case_id] = value
    return result


def load_cases(root):
    cases = {}
    for case in load_corpus_cases(Path(root)).values():
        cases[case["case_id"]] = {
            "schema": LIVE_CASE_SCHEMA,
            "case_id": case["case_id"],
            "source": case["source"],
            "source_sha256": case["source_sha256"],
            "source_origin": {"repository": "Rokurolize/ftml", "path": case["path"]},
        }
    if not cases:
        fail(f"{root}: no corpus cases")
    return cases


def projected_case(case):
    return {
        "schema": CASE_SCHEMA,
        "case_id": case["case_id"],
        "source": case["source"],
        "title": case["case_id"],
        "wikidot_observation_tier": "page-preview",
        "local_execution_tier": "ftml",
    }


def load_references(paths, cases):
    values = [value for path in paths for value in records(path)]
    current = []
    for reference in values:
        syntax_case = reference.get("syntax_case") if isinstance(reference, dict) else None
        case_id = syntax_case.get("case_id") if isinstance(syntax_case, dict) else None
        if (not isinstance(reference, dict) or reference.get("schema") != REFERENCE_SCHEMA or
                not isinstance(syntax_case, dict) or
                syntax_case.get("schema") != CASE_SCHEMA or
                not isinstance(case_id, str) or not case_id or
                not isinstance(syntax_case.get("source"), str) or
                syntax_case.get("title") != case_id or
                syntax_case.get("wikidot_observation_tier") != "page-preview" or
                syntax_case.get("local_execution_tier") != "ftml"):
            fail(f"invalid reference syntax case for {case_id}")
        case = cases.get(case_id)
        if case is None:
            fail(f"reference has unknown case {case_id}")
        raw_html = reference.get("raw_html")
        if reference.get("source_sha256") != sha256(syntax_case["source"]):
            fail(f"invalid source hash for {case_id}")
        if not isinstance(raw_html, str) or reference.get("raw_html_sha256") != sha256(raw_html):
            fail(f"invalid raw HTML hash for {case_id}")
        validate_provenance(reference, case_id)
        if syntax_case["source"] != case["source"]:
            continue
        if syntax_case != projected_case(case):
            fail(f"invalid current reference for {case_id}")
        current.append(dict(reference, case_id=case_id))
    return unique(current, "references")


def projected_checks(comparison, case_id):
    checks = comparison.get("checks")
    if not isinstance(checks, dict):
        fail(f"comparison checks are invalid for {case_id}")
    result = {}
    for name in CHECKS:
        status = checks.get(name, {}).get("status") if isinstance(checks.get(name), dict) else None
        if status not in STATUSES:
            fail(f"comparison check {name} is invalid for {case_id}")
        result[name] = status
    return result


def load_verdicts(paths, references):
    comparisons = []
    for path in paths:
        verdicts = records(path)
        if (len(verdicts) != 1 or not isinstance(verdicts[0], dict) or
                verdicts[0].get("schema") != VERDICT_SCHEMA):
            fail(f"{path}: invalid verdict schema")
        verdict = verdicts[0]
        current = verdict.get("comparisons")
        if not isinstance(current, list):
            fail(f"{path}: invalid comparisons")
        counts = Counter(item.get("status") for item in current if isinstance(item, dict))
        expected = {"total": len(current), **{status: counts[status] for status in
                    ("match", "mismatch", "runner-error", "not-applicable")}}
        if verdict.get("summary") != expected:
            fail(f"{path}: verdict summary is not exact")
        comparisons.extend(current)
    by_id = unique(comparisons, "verdicts")
    if by_id.keys() != references.keys():
        fail("verdicts do not account for every reference exactly")
    for case_id, comparison in by_id.items():
        reference = references[case_id]
        identities = comparison.get("identities", {})
        if (comparison.get("schema") != COMPARISON_SCHEMA or
                comparison.get("status") not in STATUSES or
                identities.get("source_sha256") != reference["source_sha256"] or
                identities.get("wikidot_html_sha256") != reference["raw_html_sha256"] or
                not is_sha256(identities.get("ftml_html_sha256"))):
            fail(f"invalid comparison identity for {case_id}")
        checks = projected_checks(comparison, case_id)
        expected_status = "match" if all(status == "match" for status in checks.values()) else "mismatch"
        if comparison["status"] != expected_status:
            fail(f"comparison status does not match checks for {case_id}")
    return by_id


def make_bindings(references, verdicts):
    bindings = []
    for case_id in sorted(references):
        comparison = verdicts[case_id]
        value = {
            "case_id": case_id,
            "source_sha256": references[case_id]["source_sha256"],
            "wikidot_html_sha256": references[case_id]["raw_html_sha256"],
            "ftml_html_sha256": comparison["identities"]["ftml_html_sha256"],
            "checks": projected_checks(comparison, case_id),
            "status": comparison["status"],
        }
        if comparison["status"] == "mismatch":
            value.update(disposition=comparison.get("disposition", "unresolved"),
                         reason=comparison.get("reason", "Unreviewed mismatch."))
            if (not isinstance(value["disposition"], str) or
                    value["disposition"] not in DISPOSITIONS or
                    not isinstance(value["reason"], str) or not value["reason"].strip()):
                fail(f"mismatch disposition is invalid for {case_id}")
        bindings.append(value)
    return {"schema": BINDINGS_SCHEMA, "bindings": bindings}


def load_bindings(path, cases, references):
    values = records(path)
    if (len(values) != 1 or not isinstance(values[0], dict) or
            values[0].get("schema") != BINDINGS_SCHEMA or
            set(values[0]) != {"schema", "bindings"} or
            not isinstance(values[0].get("bindings"), list)):
        fail(f"{path}: invalid bindings schema")
    bindings = unique(values[0]["bindings"], path)
    for case_id, binding in bindings.items():
        case = cases.get(case_id)
        reference = references.get(case_id)
        expected_fields = {"case_id", "source_sha256", "wikidot_html_sha256",
                           "ftml_html_sha256", "checks", "status"}
        if binding.get("status") == "mismatch":
            expected_fields |= {"disposition", "reason"}
        checks = binding.get("checks")
        if (case is None or set(binding) != expected_fields or
                binding.get("status") not in STATUSES or
                binding.get("source_sha256") != case["source_sha256"] or
                not is_sha256(binding.get("wikidot_html_sha256")) or
                not is_sha256(binding.get("ftml_html_sha256")) or
                not isinstance(checks, dict) or set(checks) != set(CHECKS) or
                any(status not in STATUSES for status in checks.values())):
            fail(f"invalid binding for {case_id}")
        expected_status = "match" if all(status == "match" for status in checks.values()) else "mismatch"
        if binding["status"] != expected_status:
            fail(f"binding status does not match checks for {case_id}")
        if reference and (binding["source_sha256"] != reference["source_sha256"] or
                          binding["wikidot_html_sha256"] != reference["raw_html_sha256"]):
            fail(f"binding hashes do not match reference for {case_id}")
        if (binding["status"] == "mismatch" and
                (not isinstance(binding["disposition"], str) or
                 binding["disposition"] not in DISPOSITIONS or
                 not isinstance(binding["reason"], str) or
                 not binding["reason"].strip())):
            fail(f"mismatch disposition is invalid for {case_id}")
        if (binding["status"] == "mismatch" and
                binding["disposition"] == "unresolved" and
                not ACTIVE_INVESTIGATION_REASON.fullmatch(binding["reason"])):
            fail(f"unresolved mismatch needs an active functional issue for {case_id}")
    return bindings


def snapshot_ids(cases, root):
    if not root.is_dir():
        return None
    found = set()
    for case_id, case in cases.items():
        origin = case.get("source_origin", {})
        source_path = origin.get("path") if isinstance(origin, dict) else None
        if not isinstance(source_path, str):
            continue
        path = Path(source_path)
        if path.is_absolute() or ".." in path.parts:
            fail(f"invalid source origin for {case_id}")
        if (root / path).with_name("wikidot.html").is_file():
            found.add(case_id)
    return found


def configured_blocks(root):
    try:
        with (root / "conf/blocks.toml").open("rb") as file:
            return tomllib.load(file)
    except (OSError, tomllib.TOMLDecodeError) as error:
        fail(f"cannot load configured block names: {error}")


def block_names(name, block):
    names = set() if block.get("exclude-name", False) else {name.casefold()}
    names.update(alias.casefold() for alias in block.get("aliases", []))
    return names


def configured_block_names(root):
    names = set()
    for name, block in configured_blocks(root).items():
        names.update(block_names(name, block))
    return names


def observed_block_markers(cases):
    markers = set()
    for case in cases.values():
        for match in re.finditer(r"\[\[(/?)([^\s\]]+)", case["source"]):
            token = match.group(2).casefold()
            name = token.rstrip("*_")
            suffix = token[len(name):]
            markers.add((bool(match.group(1)), name, "*" in suffix, "_" in suffix))
    return markers


def observed_block_names(cases):
    return {name for _, name, _, _ in observed_block_markers(cases)}


def missing_block_facets(root, cases):
    markers = observed_block_markers(cases)
    missing = {"star-flag": set(), "score-flag": set(), "body-close": set()}
    for name, block in configured_blocks(root).items():
        names = block_names(name, block)
        if block.get("accepts-star", False) and not any(
                not close and marker in names and star
                for close, marker, star, _ in markers):
            missing["star-flag"].add(name)
        if block.get("accepts-score", False) and not any(
                not close and marker in names and score
                for close, marker, _, score in markers):
            missing["score-flag"].add(name)
        if block.get("body", "none") != "none" and not any(
                close and marker in names for close, marker, _, _ in markers):
            missing["body-close"].add(name)
    return missing


def missing_block_head_facets(root, cases):
    effective_head_kinds = {"html": "map", "tabview": "value", "raw": "custom"}
    expected_head_shapes = {
        "none": {"none", "value"},
        "value": {"none", "value"},
        "map": {"none", "map", "value"},
        "value+map": {"none", "value", "map", "value+map"},
        "custom": {"none", "custom"},
    }
    observed = set()
    blocks = configured_blocks(root)
    for case in cases.values():
        for match in re.finditer(r"\[\[(?!/)([^\s\]]+)([^\]]*)\]\]", case["source"]):
            marker = match.group(1).rstrip("*_").casefold()
            tail = match.group(2).strip()
            for name, block in blocks.items():
                if marker not in block_names(name, block):
                    continue
                kind = effective_head_kinds.get(name, block.get("head", "none"))
                if not tail:
                    shape = "none"
                elif kind == "custom":
                    shape = "custom"
                else:
                    assignment = re.search(r'(?:^|\s)[^\s=]+\s*=\s*"', tail)
                    if assignment is None:
                        shape = "value"
                    elif tail[:assignment.start()].strip():
                        shape = "value+map"
                    else:
                        shape = "map"
                observed.add(f"{name}:{shape}")

    return {
        f"{name}:{shape}"
        for name, block in blocks.items()
        for shape in expected_head_shapes[
            effective_head_kinds.get(name, block.get("head", "none"))
        ]
        if f"{name}:{shape}" not in observed
    }


def missing_alias_pairs(root, cases):
    observed = set()
    blocks = {
        name: set(block_names(name, block))
        for name, block in configured_blocks(root).items()
        if block.get("body", "none") != "none"
    }
    for case in cases.values():
        markers = [
            (bool(match.group(1)), match.group(2).rstrip("*_").casefold())
            for match in re.finditer(r"\[\[(/?)([^\s\]]+)", case["source"])
        ]
        for block_name, names in blocks.items():
            openers = []
            for close, marker in markers:
                if marker not in names:
                    continue
                if close:
                    if openers:
                        observed.add(f"{block_name}:{openers.pop()}->{marker}")
                else:
                    openers.append(marker)

    missing = set()
    for name, names in blocks.items():
        for opener in names:
            for closer in names:
                pair = f"{name}:{opener}->{closer}"
                if pair not in observed:
                    missing.add(pair)
    return missing


def print_report(cases, references, bindings, root):
    ids = set(cases)
    missing_facets = missing_block_facets(root, cases)
    categories = {
        "missing-block-name": configured_block_names(root) - observed_block_names(cases),
        **{f"missing-{name}": values for name, values in missing_facets.items()},
        "missing-head-shape": missing_block_head_facets(root, cases),
        "missing-alias-pair": missing_alias_pairs(root, cases),
        "mismatch": {case_id for case_id, value in bindings.items() if value["status"] == "mismatch"},
        "missing-reference": ids - references.keys(),
        "missing-binding": ids - bindings.keys(),
    }
    snapshots = snapshot_ids(cases, root)
    if snapshots is not None:
        matched = {case_id for case_id, value in bindings.items() if value["status"] == "match"}
        categories["missing-snapshot"] = matched - snapshots
    print(f"total {len(ids)}")
    for name, case_ids in categories.items():
        print(f"{name} {len(case_ids)}" + (f": {','.join(sorted(case_ids))}" if case_ids else ""))


def parser():
    value = argparse.ArgumentParser()
    commands = value.add_subparsers(dest="command", required=True)
    bind = commands.add_parser("bind")
    bind.add_argument("--repo", type=Path, default=ROOT)
    bind.add_argument("--references", action="append", required=True)
    bind.add_argument("--verdict", action="append", required=True)
    report = commands.add_parser("report")
    report.add_argument("--references", action="append", required=True)
    report.add_argument("--bindings", required=True)
    report.add_argument("--repo", type=Path, default=ROOT)
    return value


def main(argv=None):
    args = parser().parse_args(argv)
    cases = load_cases(args.repo)
    references = load_references(args.references, cases)
    if args.command == "bind":
        print(json.dumps(make_bindings(references, load_verdicts(args.verdict, references)),
                         sort_keys=True, separators=(",", ":")))
    else:
        print_report(cases, references, load_bindings(args.bindings, cases, references), args.repo)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ValueError as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2)
