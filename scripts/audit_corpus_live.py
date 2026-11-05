#!/usr/bin/env python3
"""Audit corpus `wikidot.html` expectations against live Wikidot PagePreview.

Each `test/<family>/<case>/input.ftml` that carries a `wikidot.html`
expectation is posted, anonymously and read-only, to the
`edit/PagePreviewModule` AJAX endpoint of a public Wikidot sandbox site. The
live body and the fixture expectation are parsed as HTML5 fragments, generated
identifiers and timestamps are masked, inter-block formatting whitespace is
dropped, and the two DOMs are compared. Raw responses are cached by source
hash so a rerun never re-queries an unchanged source.

This tool is external to the ordinary offline test suite. Its cache is
evidence for an audit, not a frozen reference batch; promote a finding into
the corpus through the normal fixture path.

Requires `html5lib` (``pip install html5lib``).
"""

import argparse
import hashlib
import json
import random
import re
import string
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

import html5lib

ROOT = Path(__file__).resolve().parents[1]
MODULE = "edit/PagePreviewModule"

# Elements whose surrounding whitespace-only text nodes are formatting, not
# content: Wikidot serializes a newline after each of them, FTML does not.
BLOCK = {
    "address", "blockquote", "body", "caption", "center", "col", "colgroup",
    "dd", "div", "dl", "dt", "fieldset", "figure", "footer", "form", "h1",
    "h2", "h3", "h4", "h5", "h6", "header", "hr", "iframe", "li", "ol", "p",
    "pre", "section", "table", "tbody", "td", "tfoot", "th", "thead", "tr",
    "ul", "script", "style", "html", "head",
}

VOID = {"area", "base", "br", "col", "embed", "hr", "img", "input", "link",
        "meta", "source", "track", "wbr"}

# Generated identifiers. Each pattern is replaced inside attribute values.
MASKS = [
    (re.compile(r"wiki-tabview-[0-9a-f]{8,}"), "wiki-tabview-#"),
    (re.compile(r"wiki-tab-\d+-\d+"), "wiki-tab-#"),
    (re.compile(r"(bibitem|bibcite)-(\d+)-\d+a\b"), r"\1-\2-#"),
    (re.compile(r"\btime_\d+\b"), "time_#"),
    (re.compile(r"collapsible-[0-9a-f]{8,}"), "collapsible-#"),
    (re.compile(r"gallery-[0-9a-f]{6,}"), "gallery-#"),
    (re.compile(r"\b[0-9a-f]{32}\b"), "#hex32"),
    (re.compile(r"_[0-9]{9,}"), "_#"),
]


def sha256(text):
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def fetch(site, source, cache_dir, delay):
    key = sha256(source)
    path = cache_dir / f"{key}.json"
    if path.exists():
        return json.loads(path.read_text(encoding="utf-8")), True
    token = "".join(random.choice(string.ascii_lowercase + string.digits) for _ in range(16))
    data = urllib.parse.urlencode({
        "moduleName": MODULE,
        "source": source,
        "mode": "page",
        "wikidot_token7": token,
    }).encode()
    url = f"https://{site}.wikidot.com/ajax-module-connector.php"
    last = None
    for attempt in range(5):
        request = urllib.request.Request(url, data=data, method="POST", headers={
            "Cookie": f"wikidot_token7={token}",
            "Content-Type": "application/x-www-form-urlencoded; charset=UTF-8",
            "User-Agent": "ftml-corpus-audit/1 (anonymous PagePreview, read-only)",
            "X-Requested-With": "XMLHttpRequest",
        })
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                body = json.loads(response.read().decode("utf-8"))
            if body.get("status") != "ok":
                last = f"status {body.get('status')}: {body.get('message')}"
                time.sleep(2 ** attempt * 2)
                continue
            record = {
                "captured_at": datetime.now(timezone.utc).isoformat(),
                "site": f"{site}.wikidot.com",
                "module": MODULE,
                "authenticated": False,
                "mutated": False,
                "source_sha256": key,
                "source": source,
                "raw_html": body["body"],
                "raw_html_sha256": sha256(body["body"]),
            }
            path.write_text(json.dumps(record, ensure_ascii=False), encoding="utf-8")
            time.sleep(delay)
            return record, False
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as error:
            last = str(error)
            time.sleep(2 ** attempt * 2)
    raise RuntimeError(f"PagePreview failed: {last}")


def parse(html):
    return html5lib.parseFragment(html, container="div", treebuilder="etree",
                                  namespaceHTMLElements=False)


def mask(value):
    for pattern, replacement in MASKS:
        value = pattern.sub(replacement, value)
    return value


def tokens(element, normalize):
    """Flatten a fragment into a comparable token list.

    Text is kept exactly. With `normalize`, whitespace-only text that sits
    next to a block element (or at the edge of a block container) is
    dropped, since Wikidot emits a formatting newline there.
    """
    out = []

    def is_block(node):
        return isinstance(node.tag, str) and node.tag.lower() in BLOCK

    def emit_text(text, prev_block, next_block, container_block):
        if not text:
            return
        if normalize and not text.strip() and (prev_block or next_block or container_block):
            return
        out.append(("text", text))

    def walk(node, container_block):
        children = list(node)
        text = node.text
        emit_text(text, container_block, bool(children) and is_block(children[0]),
                  container_block and not children)
        for index, child in enumerate(children):
            if not isinstance(child.tag, str):
                # Comment nodes: html5lib etree uses a function tag.
                out.append(("comment", child.text or ""))
            else:
                attrs = sorted((k, mask(v)) for k, v in child.attrib.items())
                out.append(("open", child.tag.lower(), tuple(attrs)))
                walk(child, is_block(child))
                out.append(("close", child.tag.lower()))
            following = children[index + 1] if index + 1 < len(children) else None
            emit_text(child.tail, is_block(child) if isinstance(child.tag, str) else False,
                      following is not None and is_block(following),
                      container_block and following is None)

    walk(element, True)
    if normalize:
        # Strip fragment-edge whitespace inside the first/last text tokens.
        if out and out[0][0] == "text":
            stripped = out[0][1].lstrip()
            out[0] = ("text", stripped)
            if not stripped:
                out.pop(0)
        if out and out[-1][0] == "text":
            stripped = out[-1][1].rstrip()
            out[-1] = ("text", stripped)
            if not stripped:
                out.pop()
    return out


def visible_text(element):
    """Text content with block boundaries as separators, whitespace collapsed."""
    parts = []

    def walk(node):
        tag = node.tag.lower() if isinstance(node.tag, str) else None
        if tag in ("script", "style"):
            pass
        elif tag is not None:
            if tag in BLOCK or tag == "br":
                parts.append(" ")
            parts.append(node.text or "")
            for child in node:
                walk(child)
            if tag in BLOCK:
                parts.append(" ")
        if node is not element:
            parts.append(node.tail or "")

    walk(element)
    return re.sub(r"[ \t\n\r\f]+", " ", "".join(parts)).strip()


def serialize(tokens_list):
    out = []
    for token in tokens_list:
        if token[0] == "text":
            out.append(token[1].replace("&", "&amp;").replace("<", "&lt;"))
        elif token[0] == "comment":
            out.append(f"<!--{token[1]}-->")
        elif token[0] == "open":
            attrs = "".join(f' {k}="{v}"' for k, v in token[2])
            out.append(f"<{token[1]}{attrs}>")
        elif token[1] not in VOID:
            out.append(f"</{token[1]}>")
    return "".join(out)


def first_difference(a, b):
    for index, (left, right) in enumerate(zip(a, b)):
        if left != right:
            return index
    return min(len(a), len(b)) if len(a) != len(b) else None


def compare(live_html, fixture_html):
    live = parse(live_html)
    fixture = parse(fixture_html)
    live_exact, fixture_exact = tokens(live, False), tokens(fixture, False)
    live_norm, fixture_norm = tokens(live, True), tokens(fixture, True)
    live_text, fixture_text = visible_text(live), visible_text(fixture)
    result = {
        "dom": live_norm == fixture_norm,
        "text": live_text == fixture_text,
        "exact": live_exact == fixture_exact,
    }
    if not result["dom"]:
        index = first_difference(live_norm, fixture_norm)
        lo = max(0, index - 4)
        result["live_excerpt"] = serialize(live_norm[lo:index + 6])
        result["fixture_excerpt"] = serialize(fixture_norm[lo:index + 6])
    result["live_norm"] = serialize(live_norm)
    result["fixture_norm"] = serialize(fixture_norm)
    return result


# Tags the corpus runner gives every fixture page (src/test/ast/runner.rs).
FIXTURE_TAGS = {"fruit", "component"}
IFTAGS_HEAD = re.compile(r"\[\[\s*iftags\b([^\]]*)\]\]", re.IGNORECASE)


def iftags_gate_true(head, tags=FIXTURE_TAGS):
    required, forbidden, any_of = [], [], []
    for token in head.split():
        if token.startswith("+"):
            required.append(token[1:].lower())
        elif token.startswith("-"):
            forbidden.append(token[1:].lower())
        else:
            any_of.append(token.lower())
    return (all(tag in tags for tag in required)
            and not any(tag in tags for tag in forbidden)
            and (not any_of or any(tag in tags for tag in any_of)))


def preview_context(source):
    """Why PagePreview cannot observe what the fixture runner renders.

    Anonymous PagePreview evaluates every `[[iftags]]` gate as false (even
    `[[iftags -zzz]]`, and even with a tagged page's `page_id`), while the
    corpus runner renders with tags `fruit, component`. A source with a gate
    that is true under those tags is therefore not observable in preview.
    """
    if any(iftags_gate_true(head) for head in IFTAGS_HEAD.findall(source)):
        return "iftags-true-gate"
    return None


def strip_newpage(html):
    return re.sub(r'(<a\b[^>]*?)\s+class="newpage"', r"\1", html)


def fixtures(families):
    for case_dir in sorted((ROOT / "test").glob("*/*")):
        expected = case_dir / "wikidot.html"
        source = case_dir / "input.ftml"
        if not (expected.is_file() and source.is_file()):
            continue
        family = case_dir.parent.name
        if families and family not in families:
            continue
        yield family, case_dir


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--site", default="sandbox-for-codex")
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True,
                        help="JSONL of per-fixture verdicts")
    parser.add_argument("--family", action="append", default=[])
    parser.add_argument("--delay", type=float, default=1.0)
    parser.add_argument("--offline", action="store_true",
                        help="only compare cases already in the cache")
    args = parser.parse_args(argv)
    args.cache.mkdir(parents=True, exist_ok=True)

    counts = {"match": 0, "mismatch": 0, "skipped": 0}
    with args.output.open("w", encoding="utf-8") as out:
        for family, case_dir in fixtures(set(args.family)):
            case = f"{family}/{case_dir.name}"
            source = (case_dir / "input.ftml").read_text(encoding="utf-8")
            expected = (case_dir / "wikidot.html").read_text(encoding="utf-8")
            if args.offline and not (args.cache / f"{sha256(source)}.json").exists():
                counts["skipped"] += 1
                continue
            try:
                record, cached = fetch(args.site, source, args.cache, args.delay)
            except RuntimeError as error:
                print(f"ERROR {case}: {error}", file=sys.stderr)
                counts["skipped"] += 1
                continue
            verdict = compare(record["raw_html"], expected)
            status = "match" if verdict["dom"] and verdict["text"] else "mismatch"
            context = None
            if status == "mismatch":
                context = preview_context(source)
                if context is None:
                    relaxed = compare(strip_newpage(record["raw_html"]), expected)
                    if relaxed["dom"] and relaxed["text"]:
                        context = "page-existence-newpage-class"
                if context is not None:
                    status = "context"
            counts[status] = counts.get(status, 0) + 1
            row = {
                "case": case,
                "family": family,
                "status": status,
                "context": context,
                "source_sha256": record["source_sha256"],
                "raw_html_sha256": record["raw_html_sha256"],
                "captured_at": record["captured_at"],
                **verdict,
            }
            out.write(json.dumps(row, ensure_ascii=False) + "\n")
            out.flush()
            print(f"{status:8} {'cached' if cached else 'live  '} {case}", file=sys.stderr)
    print(json.dumps(counts), file=sys.stderr)


if __name__ == "__main__":
    main()
