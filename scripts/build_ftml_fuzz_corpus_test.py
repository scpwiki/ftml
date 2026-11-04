import json
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/build_ftml_fuzz_corpus.py"


def corpus_sources() -> set[str]:
    files = set((ROOT / "test").rglob("input.ftml"))
    files.update((ROOT / "tests" / "fixtures").rglob("*.ftml"))
    return {path.read_bytes().decode("utf-8") for path in files}


class BuildFtmlFuzzCorpusTest(unittest.TestCase):
    def test_corpus_is_deduplicated_by_source_hash(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "corpus"
            completed = subprocess.run(
                ["python3", str(SCRIPT), "--output", str(output)],
                cwd=ROOT,
                check=True,
                text=True,
                capture_output=True,
            )
            summary = json.loads(completed.stdout)
            self.assertGreater(summary["corpus_cases_added"], 0)
            self.assertEqual(summary["wikijump_seeds_added"], 0)
            self.assertEqual(summary["unique_seed_files"], len(list(output.iterdir())))
            self.assertEqual(summary["unique_seed_files"], len(corpus_sources()))


if __name__ == "__main__":
    unittest.main()
