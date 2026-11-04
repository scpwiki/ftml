#!/usr/bin/env python3

import argparse
import base64
import html
import json
import re
import subprocess
import tempfile
from pathlib import Path

try:
    from corpus_cases import load_corpus_cases
except ImportError:
    from scripts.corpus_cases import load_corpus_cases


ROOT = Path(__file__).resolve().parents[1]
ARTIFACT_DIR = ROOT / "tests/fixtures/wikidot-parity"
CONTRACTS = ARTIFACT_DIR / "comparison-normalization-contracts.json"
BINDINGS = ARTIFACT_DIR / "bindings.json"
INPUT_SCHEMA = "wikijump_syntax_differential.syntax_case.v1"


def read_jsonl(path: Path) -> list[dict]:
    return [
        json.loads(line)
        for line in path.read_text(encoding="utf-8").split("\n")
        if line.strip()
    ]


def current_normalization_inputs() -> list[dict]:
    contracts = json.loads(CONTRACTS.read_text(encoding="utf-8"))["contracts"]
    contract_by_id = {contract["case_id"]: contract for contract in contracts}
    contract_ids = list(contract_by_id)
    cases = load_corpus_cases(ROOT)
    bindings = {
        binding["case_id"]: binding
        for binding in json.loads(BINDINGS.read_text(encoding="utf-8"))["bindings"]
    }

    references: dict[str, dict] = {}
    for path in sorted(ARTIFACT_DIR.glob("references-*.jsonl")):
        for reference in read_jsonl(path):
            case_id = reference["syntax_case"]["case_id"]
            if case_id not in contract_ids:
                continue
            case = cases[case_id]
            if reference["source_sha256"] != case["source_sha256"]:
                continue
            previous = references.get(case_id)
            if previous is None or reference["captured_at"] > previous["captured_at"]:
                references[case_id] = reference

    rows = []
    for case_id in contract_ids:
        case = cases.get(case_id)
        binding = bindings.get(case_id)
        reference = references.get(case_id)
        if case is None or binding is None or reference is None:
            raise ValueError(f"{case_id}: missing case, binding, or current reference")
        if binding.get("disposition") != "comparison-normalization":
            raise ValueError(f"{case_id}: binding is not comparison-normalization")
        rows.append(
            {
                "case_id": case_id,
                "source": case["source"],
                "wikidot_html": reference["raw_html"],
                "difference_class": contract_by_id[case_id]["difference_class"],
            }
        )
    return rows


def render_ftml(renderer: Path, rows: list[dict]) -> dict[str, str]:
    payload = "".join(
        json.dumps(
            {
                "schema": INPUT_SCHEMA,
                "case_id": row["case_id"],
                "source": row["source"],
                "title": row["case_id"],
                "layout": "wikidot",
            },
            ensure_ascii=False,
        )
        + "\n"
        for row in rows
    )
    completed = subprocess.run(
        [str(renderer)],
        input=payload,
        text=True,
        capture_output=True,
        check=False,
        timeout=120,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"FTML renderer exited {completed.returncode}: {completed.stderr[-4000:]}"
        )
    results = [
        json.loads(line) for line in completed.stdout.splitlines() if line.strip()
    ]
    if len(results) != len(rows):
        raise RuntimeError(f"renderer returned {len(results)} rows for {len(rows)} cases")
    output = {}
    for result in results:
        if result.get("status") != "rendered":
            raise RuntimeError(f"renderer failed for {result.get('case_id')}: {result!r}")
        output[result["case_id"]] = result["html"]
    return output


def b64(value: str) -> str:
    return base64.b64encode(value.encode("utf-8")).decode("ascii")


def browser_document(pairs: list[dict]) -> str:
    payload = json.dumps(
        [
            {
                "case_id": pair["case_id"],
                "difference_class": pair["difference_class"],
                "wikidot": b64(pair["wikidot_html"]),
                "ftml": b64(pair["ftml_html"]),
            }
            for pair in pairs
        ],
        separators=(",", ":"),
    )
    return f"""<!doctype html>
<meta charset="utf-8">
<body>
<script id="payload" type="application/json">{payload}</script>
<script>
const decoder = new TextDecoder();
const decode = value => decoder.decode(Uint8Array.from(atob(value), c => c.charCodeAt(0)));
const asciiLeft = /^[\\t\\n\\f\\r ]+/;
const asciiRight = /[\\t\\n\\f\\r ]+$/;

function trimRootWhitespace(fragment) {{
  while (fragment.firstChild && fragment.firstChild.nodeType === Node.TEXT_NODE) {{
    fragment.firstChild.data = fragment.firstChild.data.replace(asciiLeft, "");
    if (fragment.firstChild.data.length === 0) fragment.firstChild.remove();
    else break;
  }}
  while (fragment.lastChild && fragment.lastChild.nodeType === Node.TEXT_NODE) {{
    fragment.lastChild.data = fragment.lastChild.data.replace(asciiRight, "");
    if (fragment.lastChild.data.length === 0) fragment.lastChild.remove();
    else break;
  }}
}}

function summarize(node) {{
  if (node.nodeType === Node.TEXT_NODE) return ["text", node.data];
  if (node.nodeType === Node.COMMENT_NODE) return ["comment", node.data];
  if (node.nodeType !== Node.ELEMENT_NODE) return ["node", node.nodeType];
  const attrs = Array.from(node.attributes, attr => [attr.name, attr.value])
    .sort((a, b) => a[0].localeCompare(b[0]) || a[1].localeCompare(b[1]));
  return [
    "element",
    node.localName,
    attrs,
    Array.from(node.childNodes, summarize),
  ];
}}

function elementSkeleton(node) {{
  if (node.nodeType !== Node.ELEMENT_NODE) return null;
  const attrs = Array.from(node.attributes, attr => [attr.name, attr.value])
    .sort((a, b) => a[0].localeCompare(b[0]) || a[1].localeCompare(b[1]));
  return [
    node.localName,
    attrs,
    Array.from(node.children, elementSkeleton),
  ];
}}

function directTextSignature(node) {{
  const direct = Array.from(node.childNodes)
    .filter(child => child.nodeType === Node.TEXT_NODE)
    .map(child => child.data)
    .join("")
    .replace(/[\\t\\n\\f\\r ]+/g, " ")
    .trim();
  return [direct, Array.from(node.children, directTextSignature)];
}}

function parse(html) {{
  const host = document.createElement("div");
  host.innerHTML = html;
  trimRootWhitespace(host);
  document.body.appendChild(host);
  const strict = Array.from(host.childNodes, summarize);
  const skeleton = Array.from(host.children, elementSkeleton);
  const directText = Array.from(host.children, directTextSignature);
  const rootDirectText = Array.from(host.childNodes)
    .filter(child => child.nodeType === Node.TEXT_NODE)
    .map(child => child.data)
    .join("")
    .replace(/[\\t\\n\\f\\r ]+/g, " ")
    .trim();
  const innerText = host.innerText;
  const collapsedText = host.textContent.replace(/[\\t\\n\\f\\r ]+/g, " ").trim();
  const preText = Array.from(host.querySelectorAll("pre,textarea"), node => node.textContent);
  host.remove();
  return {{ strict, skeleton, directText, rootDirectText, innerText, collapsedText, preText }};
}}

const pairs = JSON.parse(document.getElementById("payload").textContent);
const results = pairs.map(pair => {{
  const wikidot = parse(decode(pair.wikidot));
  const ftml = parse(decode(pair.ftml));
  const strictEqual = JSON.stringify(wikidot.strict) === JSON.stringify(ftml.strict);
  const renderEqual =
    JSON.stringify(wikidot.skeleton) === JSON.stringify(ftml.skeleton) &&
    JSON.stringify(wikidot.directText) === JSON.stringify(ftml.directText) &&
    wikidot.rootDirectText === ftml.rootDirectText &&
    wikidot.innerText === ftml.innerText &&
    JSON.stringify(wikidot.preText) === JSON.stringify(ftml.preText);
  return {{
    case_id: pair.case_id,
    difference_class: pair.difference_class,
    strict_equal: strictEqual,
    render_equal: renderEqual,
    wikidot,
    ftml,
  }};
}});
document.body.innerHTML = '<pre id="result"></pre>';
document.getElementById("result").textContent = JSON.stringify(results);
</script>
</body>
"""


def extract_browser_results(dumped_html: str) -> list[dict]:
    match = re.search(r'<pre id="result">(.*?)</pre>', dumped_html, flags=re.S)
    if match is None:
        raise RuntimeError("browser output did not contain the result payload")
    return json.loads(html.unescape(match.group(1)))


def run_browser(chrome: Path, pairs: list[dict]) -> list[dict]:
    with tempfile.TemporaryDirectory() as directory:
        directory = Path(directory)
        document = directory / "normalization.html"
        document.write_text(browser_document(pairs), encoding="utf-8")
        user_data = directory / "chrome-profile"
        completed = subprocess.run(
            [
                str(chrome),
                "--headless=new",
                "--no-sandbox",
                "--disable-gpu",
                "--disable-dev-shm-usage",
                f"--user-data-dir={user_data}",
                "--virtual-time-budget=1000",
                "--dump-dom",
                document.resolve().as_uri(),
            ],
            text=True,
            capture_output=True,
            check=False,
            timeout=120,
        )
    if completed.returncode != 0:
        raise RuntimeError(
            f"browser exited {completed.returncode}: {completed.stderr[-4000:]}"
        )
    return extract_browser_results(completed.stdout)


def main() -> None:
    parser = argparse.ArgumentParser(
        description=(
            "Verify comparison-normalization bindings in a real browser DOM, "
            "allowing only ASCII whitespace at the fragment root boundaries."
        )
    )
    parser.add_argument("--chrome", required=True, type=Path)
    parser.add_argument("--renderer", type=Path)
    parser.add_argument("--build-renderer", action="store_true")
    args = parser.parse_args()

    renderer = args.renderer
    if args.build_renderer:
        subprocess.run(
            ["cargo", "build", "--example", "render_html_jsonl"], cwd=ROOT, check=True
        )
        renderer = ROOT / "target/debug/examples/render_html_jsonl"
    if renderer is None:
        renderer = ROOT / "target/debug/examples/render_html_jsonl"
    renderer = renderer.resolve()
    chrome = args.chrome.resolve()
    if not renderer.is_file():
        raise SystemExit(f"renderer not found: {renderer}; pass --build-renderer")
    if not chrome.is_file():
        raise SystemExit(f"Chrome executable not found: {chrome}")

    try:
        rows = current_normalization_inputs()
        rendered = render_ftml(renderer, rows)
        pairs = [
            {
                **row,
                "ftml_html": rendered[row["case_id"]],
            }
            for row in rows
        ]
        results = run_browser(chrome, pairs)
    except (ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
        raise SystemExit(str(error)) from error

    mismatches = []
    for result in results:
        difference_class = result["difference_class"]
        if difference_class in {"page-preview-root-whitespace", "html-serialization"}:
            accepted = result["strict_equal"]
        elif difference_class == "browser-rendering-whitespace":
            accepted = result["render_equal"]
        else:
            accepted = False
        if not accepted:
            mismatches.append(result)
    if mismatches:
        compact = [
            {
                "case_id": result["case_id"],
                "difference_class": result["difference_class"],
                "strict_equal": result["strict_equal"],
                "render_equal": result["render_equal"],
                "wikidot": result["wikidot"],
                "ftml": result["ftml"],
            }
            for result in mismatches
        ]
        raise SystemExit(
            "browser DOM mismatch outside root whitespace normalization:\n"
            + json.dumps(compact, ensure_ascii=False, indent=2)
        )

    print(
        json.dumps(
            {
                "cases": len(results),
                "strict_root_equivalent": sum(
                    result["strict_equal"] for result in results
                ),
                "browser_render_equivalent": sum(
                    result["render_equal"] for result in results
                ),
                "status": "pass",
            },
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
