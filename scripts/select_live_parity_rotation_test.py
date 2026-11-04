import json
import tempfile
import unittest
from pathlib import Path

from scripts.select_live_parity_rotation import load_candidates, select_rotation, syntax_case


def write_case(root: Path, case_id: str, source: str) -> None:
    directory = root.joinpath(*case_id.split("--"))
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "input.ftml").write_text(source, encoding="utf-8")


def write_bindings(root: Path, rows: list[dict]) -> Path:
    bindings = root / "bindings.json"
    bindings.write_text(json.dumps({"bindings": rows}), encoding="utf-8")
    return bindings


class LiveParityRotationTest(unittest.TestCase):
    def test_load_candidates_uses_only_exact_matches_with_live_fixtures(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            write_case(root, "test--b", "B")
            write_case(root, "test--a", "A")
            write_case(root, "test--c", "C")
            write_case(root, "test--d", "D")
            bindings = write_bindings(
                root,
                [
                    {"case_id": "test--a", "status": "match"},
                    {"case_id": "test--b", "status": "match"},
                    {"case_id": "test--d", "status": "mismatch"},
                    {"case_id": "test--stale", "status": "match"},
                ],
            )
            self.assertEqual(
                [row["case_id"] for row in load_candidates(bindings, root)],
                ["test--a", "test--b"],
            )

    def test_rotation_wraps_and_advances_without_reordering(self):
        candidates = [{"case_id": str(index)} for index in range(5)]
        self.assertEqual(
            [row["case_id"] for row in select_rotation(candidates, 0, 3)],
            ["0", "1", "2"],
        )
        self.assertEqual(
            [row["case_id"] for row in select_rotation(candidates, 1, 3)],
            ["3", "4", "0"],
        )

    def test_syntax_case_contains_no_execution_classification(self):
        row = syntax_case({"case_id": "x", "source": "[[span]]x[[/span]]"})
        self.assertEqual(row["schema"], "wikijump_syntax_differential.syntax_case.v1")
        self.assertEqual(row["case_id"], "x")
        self.assertNotIn("execution_class", row)

    def test_corpus_sources_preserve_unicode_line_separators(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = "before\u2028after\u2029end"
            write_case(root, "test--unicode-separators", source)
            bindings = write_bindings(
                root,
                [{"case_id": "test--unicode-separators", "status": "match"}],
            )
            loaded = load_candidates(bindings, root)
            self.assertEqual(loaded[0]["source"], source)


if __name__ == "__main__":
    unittest.main()
