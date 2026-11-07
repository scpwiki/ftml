import unittest

from scripts.check_comparison_normalization_browser import (
    browser_document,
    current_normalization_inputs,
    extract_browser_results,
)


class ComparisonNormalizationBrowserTest(unittest.TestCase):
    def test_contracts_resolve_to_current_cases_and_references(self):
        rows = current_normalization_inputs()
        self.assertEqual(len(rows), 43)
        self.assertEqual(len({row["case_id"] for row in rows}), 43)
        self.assertTrue(all(row["source"] for row in rows))
        self.assertTrue(all("wikidot_html" in row for row in rows))
        self.assertTrue(all("difference_class" in row for row in rows))

    def test_browser_document_embeds_only_base64_fragment_payloads(self):
        document = browser_document(
            [
                {
                    "case_id": "example",
                    "difference_class": "page-preview-root-whitespace",
                    "wikidot_html": "<p>A</p>",
                    "ftml_html": "<p>A</p>",
                }
            ]
        )
        self.assertIn('"case_id":"example"', document)
        self.assertNotIn("<p>A</p>", document)
        self.assertIn("trimRootWhitespace", document)

    def test_extract_browser_results_decodes_dumped_pre_text(self):
        dumped = '<html><body><pre id="result">[{&quot;case_id&quot;:&quot;x&quot;,&quot;equal&quot;:true}]</pre></body></html>'
        self.assertEqual(
            extract_browser_results(dumped),
            [{"case_id": "x", "equal": True}],
        )


if __name__ == "__main__":
    unittest.main()
