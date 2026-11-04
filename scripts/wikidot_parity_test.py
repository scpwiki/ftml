import importlib.util
import json
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("wikidot_parity", Path(__file__).with_name("wikidot_parity.py"))
parity = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(parity)


class WikidotParityTest(unittest.TestCase):
    def test_jsonl_records_preserve_authored_unicode_line_separators(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "records.jsonl"
            values = [
                {"case_id": "nel", "source": "A\u0085B"},
                {"case_id": "line-separator", "source": "A\u2028B"},
                {"case_id": "paragraph-separator", "source": "A\u2029B"},
            ]
            path.write_text(
                "\n".join(json.dumps(value, ensure_ascii=False) for value in values) + "\n",
                encoding="utf-8",
            )

            self.assertEqual(parity.records(path), values)

    def test_registered_block_names_are_observed_from_stable_sources(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "conf").mkdir()
            (root / "conf/blocks.toml").write_text(
                '[alpha]\naliases = ["a"]\naccepts-star = true\nbody = "elements"\n'
                '[hidden-name]\nexclude-name = true\naliases = ["visible-alias"]\naccepts-score = true\n'
            )
            cases = {
                "one": {"source": "[[alpha*]] [[alpha key=\"value\"]] [[alpha stray]] [[/a]] [[a]][[/alpha]] [[alpha]][[/alpha]] [[a]][[/a]]"},
                "two": {"source": "[[visible-alias_]] [[visible-alias stray]]"},
            }

            self.assertEqual(
                parity.configured_block_names(root),
                {"alpha", "a", "visible-alias"},
            )
            self.assertEqual(
                parity.observed_block_names(cases),
                {"alpha", "a", "visible-alias"},
            )
            self.assertEqual(
                parity.missing_block_facets(root, cases),
                {"star-flag": set(), "score-flag": set(), "body-close": set()},
            )
            self.assertEqual(parity.missing_block_head_facets(root, cases), set())
            self.assertEqual(parity.missing_alias_pairs(root, cases), set())
            self.assertEqual(
                parity.missing_alias_pairs(
                    root,
                    {"separate": {"source": "[[alpha]][[/alpha]] [[a]][[/a]]"}},
                ),
                {"alpha:alpha->a", "alpha:a->alpha"},
            )

        corpus_cases = parity.load_cases(ROOT)
        self.assertEqual(
            parity.configured_block_names(ROOT)
            - parity.observed_block_names(corpus_cases),
            set(),
        )
        self.assertEqual(
            parity.missing_block_facets(ROOT, corpus_cases),
            {"star-flag": set(), "score-flag": set(), "body-close": set()},
        )
        self.assertEqual(parity.missing_block_head_facets(ROOT, corpus_cases), set())
        self.assertEqual(parity.missing_alias_pairs(ROOT, corpus_cases), set())

    def test_real_live_case_projection_and_compact_binding(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, raw = "**alpha**", "<p><strong>alpha</strong></p>"
            case = {"case_id": "test--bold", "source": source}
            reference = {"schema": parity.REFERENCE_SCHEMA, "syntax_case": parity.projected_case(case),
                         "source_sha256": parity.sha256(source), "raw_html": raw,
                         "raw_html_sha256": parity.sha256(raw),
                         "captured_at": "2026-08-15T00:00:00+00:00",
                         "provenance": {"module": "edit/PagePreviewModule", "authenticated": False,
                                        "mutated": False, "site": "sandbox-for-codex",
                                        "site_domain": "sandbox-for-codex.wikidot.com",
                                        "wikidot_py_version": "4.4.1", "wikidot_py_commit": "a" * 40,
                                        "requirements_sha256": "b" * 64}}
            comparison = {"schema": parity.COMPARISON_SCHEMA, "case_id": "test--bold", "status": "mismatch",
                          "checks": {name: {"status": "mismatch" if name == "dom_tree" else "match",
                                            "diagnostic": "not retained"} for name in parity.CHECKS},
                          "identities": {"source_sha256": parity.sha256(source),
                                         "wikidot_html_sha256": parity.sha256(raw),
                                         "ftml_html_sha256": parity.sha256("candidate")}}
            verdict = {"schema": parity.VERDICT_SCHEMA, "comparisons": [comparison],
                       "summary": {"total": 1, "match": 0, "mismatch": 1,
                                   "runner-error": 0, "not-applicable": 0}}
            (root / "test/bold").mkdir(parents=True)
            (root / "test/bold/input.ftml").write_text(source, encoding="utf-8")
            (root / "test/unreferenced").mkdir(parents=True)
            (root / "test/unreferenced/input.ftml").write_text(
                "[[div]]x[[/div]]", encoding="utf-8")
            old_reference = json.loads(json.dumps(reference))
            old_reference["syntax_case"]["source"] = "**old**"
            old_reference["source_sha256"] = parity.sha256("**old**")
            (root / "references.json").write_text(
                f"{json.dumps(old_reference)}\n{json.dumps(reference)}\n")
            (root / "verdict.json").write_text(json.dumps(verdict))
            cases = parity.load_cases(root)
            references = parity.load_references([root / "references.json"], cases)
            bindings = parity.make_bindings(
                references, parity.load_verdicts([root / "verdict.json"], references))
            self.assertEqual([item["case_id"] for item in bindings["bindings"]], ["test--bold"])
            self.assertEqual(bindings["bindings"][0]["checks"], {
                "dom_tree": "mismatch", "dom_signature": "match", "visible_text": "match"})
            self.assertEqual(bindings["bindings"][0]["disposition"], "unresolved")
            (root / "bindings.json").write_text(json.dumps(bindings))
            with self.assertRaisesRegex(ValueError, "active functional issue"):
                parity.load_bindings(root / "bindings.json", cases, references)
            bindings["bindings"][0]["reason"] = "Active functional investigation: issue #123."
            (root / "bindings.json").write_text(json.dumps(bindings))
            parity.load_bindings(root / "bindings.json", cases, references)
            malformed = json.loads(json.dumps(bindings))
            malformed["bindings"][0]["reason"] = "Active functional investigation: issue #1abc"
            (root / "bindings.json").write_text(json.dumps(malformed))
            with self.assertRaisesRegex(ValueError, "active functional issue"):
                parity.load_bindings(root / "bindings.json", cases, references)
            contradictory = json.loads(json.dumps(bindings))
            contradictory["bindings"][0]["status"] = "match"
            del contradictory["bindings"][0]["disposition"]
            del contradictory["bindings"][0]["reason"]
            (root / "bindings.json").write_text(json.dumps(contradictory))
            with self.assertRaisesRegex(ValueError, "status does not match checks"):
                parity.load_bindings(root / "bindings.json", cases, references)
            invalid = json.loads(json.dumps(bindings))
            invalid["bindings"][0]["disposition"] = "arbitrary"
            (root / "bindings.json").write_text(json.dumps(invalid))
            with self.assertRaisesRegex(ValueError, "mismatch disposition is invalid"):
                parity.load_bindings(root / "bindings.json", cases, references)
            (root / "test/bold/wikidot.html").touch()
            self.assertEqual(parity.snapshot_ids(cases, root), {"test--bold"})
            reference["raw_html_sha256"] = "0" * 64
            (root / "references.json").write_text(json.dumps(reference))
            with self.assertRaisesRegex(ValueError, "raw HTML hash"):
                parity.load_references([root / "references.json"], cases)
            reference["raw_html_sha256"] = parity.sha256(raw)
            reference["provenance"]["mutated"] = True
            (root / "references.json").write_text(json.dumps(reference))
            with self.assertRaisesRegex(ValueError, "acquisition provenance"):
                parity.load_references([root / "references.json"], cases)


if __name__ == "__main__":
    unittest.main()
