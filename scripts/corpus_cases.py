#!/usr/bin/env python3
"""Derive the parity corpus from the fixture tree.

Every ``test/**/input.ftml`` and ``tests/fixtures/**/*.ftml`` source is a
case, and adding a fixture is sufficient: there is no registry to update.
A case_id is the repository-relative path with the ``.ftml`` suffix
removed, a trailing ``input`` segment dropped, and the remaining segments
joined by ``--``: ``test/align/basic/input.ftml`` is
``test--align--basic`` and ``tests/fixtures/scp8980/listpages.ftml`` is
``tests--fixtures--scp8980--listpages``.
"""

import hashlib
from pathlib import Path


def case_id_for_path(relative_path: Path) -> str:
    parts = list(relative_path.parts)
    if parts[-1] == "input.ftml":
        parts.pop()
    elif parts[-1].endswith(".ftml"):
        parts[-1] = parts[-1][: -len(".ftml")]
    return "--".join(parts)


def corpus_case_files(root: Path) -> list[Path]:
    files = set((root / "test").rglob("input.ftml"))
    files.update((root / "tests" / "fixtures").rglob("*.ftml"))
    return sorted(files)


def load_corpus_cases(root: Path) -> dict[str, dict]:
    cases = {}
    for path in corpus_case_files(root):
        relative = path.relative_to(root)
        case_id = case_id_for_path(relative)
        if case_id in cases:
            raise ValueError(f"duplicate corpus case_id derived from tree: {case_id}")
        source = path.read_bytes().decode("utf-8")
        cases[case_id] = {
            "case_id": case_id,
            "source": source,
            "source_sha256": hashlib.sha256(source.encode("utf-8")).hexdigest(),
            "path": relative.as_posix(),
        }
    return cases


def corpus_case_ids(root: Path) -> set[str]:
    return {
        case_id_for_path(path.relative_to(root)) for path in corpus_case_files(root)
    }


def corpus_source_hashes(root: Path) -> set[str]:
    return {case["source_sha256"] for case in load_corpus_cases(root).values()}
