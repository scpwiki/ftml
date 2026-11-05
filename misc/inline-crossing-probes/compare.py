#!/usr/bin/env python3
"""Compare FTML Wikidot-layout HTML against captured live PagePreview bytes.

Usage:
    python3 compare.py <ftml-results.jsonl> [--json out.json]

Reads:

    live-results.json      (next to this script; written by probe-live.py)
    <ftml-results.jsonl>   one JSON object per line: {"case_id": ..., "html": ...}

Both sides are normalized before comparison, in this order:

    1. html.unescape()               (live emits &#8212; etc.)
    2. unwrap a single outer <p>...</p>, when the whole stripped string is one
    3. style attributes: split declarations on ";", strip each, drop empties,
       rejoin with "; "  (style="color: red;" == style="color: red")
    4. <br /> / <br/> / <br> -> <br>
    5. collapse ASCII whitespace runs to one space, then strip

Prints "agree N/M" then one block per disagreeing case (id, source, normalized
live, normalized ftml, plus a whitespace-only marker when the normalized
strings are equal after removing all whitespace).  IDs present on only one side
are noted on stderr and are not part of N.

With --json, writes {id: {source, live, ftml, match, space_only}}.

The comparison is also importable:

    from compare import compare, normalize
    summary = compare(live_map, ftml_map)

where live_map maps id -> a live-results.json entry (or bare body string) and
ftml_map maps id -> html string.
"""

import argparse
import html
import json
import re
import sys
from pathlib import Path

REC = Path(__file__).resolve().parent
DEFAULT_LIVE_RESULTS = REC / "live-results.json"

_ASCII_WS = re.compile(r"[ \t\r\n\f\v]+")
_BR = re.compile(r"<br\s*/?>", re.IGNORECASE)
_STYLE_ATTR = re.compile(r'style="([^"]*)"')


def _normalize_style(match):
    declarations = [
        declaration.strip()
        for declaration in match.group(1).split(";")
        if declaration.strip()
    ]
    return 'style="{}"'.format("; ".join(declarations))


def normalize(text):
    """Normalize one HTML string for comparison (see module docstring)."""
    if not isinstance(text, str):
        text = "" if text is None else str(text)

    text = html.unescape(text)

    stripped = text.strip()
    if (
        stripped.startswith("<p>")
        and stripped.endswith("</p>")
        and stripped.count("<p>") == 1
        and stripped.count("</p>") == 1
    ):
        text = stripped[3:-4]

    text = _STYLE_ATTR.sub(_normalize_style, text)
    text = _BR.sub("<br>", text)
    text = _ASCII_WS.sub(" ", text).strip()
    return text


def _live_body(live_entry):
    """Extract the raw live body from a live-results.json entry."""
    if isinstance(live_entry, str):
        return live_entry
    if isinstance(live_entry, dict):
        runs = live_entry.get("runs")
        if isinstance(runs, list) and runs:
            return runs[0]
        return ""
    return ""


def _live_source(live_entry):
    if isinstance(live_entry, dict):
        return live_entry.get("source", "")
    return ""


def compare(live_map, ftml_map):
    """Compare two id->html maps.

    live_map values may be live-results.json entries ({"source", "runs", ...})
    or bare live body strings.  ftml_map values are HTML strings.  Returns
    {id: {source, live, ftml, match, space_only}} for ids present on both sides.
    """
    summary = {}
    for case_id in sorted(set(live_map) & set(ftml_map)):
        live_entry = live_map[case_id]
        live_normalized = normalize(_live_body(live_entry))
        ftml_normalized = normalize(ftml_map[case_id])
        match = live_normalized == ftml_normalized
        space_only = (
            not match
            and "".join(live_normalized.split())
            == "".join(ftml_normalized.split())
        )
        summary[case_id] = {
            "source": _live_source(live_entry),
            "live": live_normalized,
            "ftml": ftml_normalized,
            "match": match,
            "space_only": space_only,
        }
    return summary


def load_live_results(path=DEFAULT_LIVE_RESULTS):
    with Path(path).open(encoding="utf-8") as handle:
        live = json.load(handle)
    if not isinstance(live, dict):
        sys.exit("error: {} must be a JSON object".format(path))
    return live


def load_ftml_jsonl(path):
    ftml = {}
    with Path(path).open(encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, 1):
            line = line.strip()
            if not line:
                continue
            try:
                record = json.loads(line)
            except json.JSONDecodeError as error:
                sys.exit(
                    "error: {}:{}: invalid JSON: {}".format(
                        path, line_number, error
                    )
                )
            if (
                not isinstance(record, dict)
                or "case_id" not in record
                or "html" not in record
            ):
                sys.exit(
                    "error: {}:{}: expected an object with case_id and html".format(
                        path, line_number
                    )
                )
            ftml[str(record["case_id"])] = record["html"]
    return ftml


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Compare FTML Wikidot-layout HTML with live PagePreview bytes."
    )
    parser.add_argument(
        "ftml_results",
        metavar="ftml-results.jsonl",
        help="JSONL of {case_id, html} produced by render_html_jsonl",
    )
    parser.add_argument(
        "--json",
        dest="json_out",
        metavar="out.json",
        help="write the machine-readable comparison summary here",
    )
    args = parser.parse_args(argv)

    live = load_live_results()
    ftml = load_ftml_jsonl(args.ftml_results)
    summary = compare(live, ftml)

    total = len(summary)
    agree = sum(1 for result in summary.values() if result["match"])
    print("agree {}/{}".format(agree, total))

    missing_ftml = sorted(set(live) - set(ftml))
    if missing_ftml:
        print(
            "note: {} live case(s) with no ftml result: {}".format(
                len(missing_ftml), " ".join(missing_ftml)
            ),
            file=sys.stderr,
        )
    extra_ftml = sorted(set(ftml) - set(live))
    if extra_ftml:
        print(
            "note: {} ftml case(s) with no live capture: {}".format(
                len(extra_ftml), " ".join(extra_ftml)
            ),
            file=sys.stderr,
        )

    for case_id, result in summary.items():
        if result["match"]:
            continue
        print()
        print("DISAGREE {}".format(case_id))
        print("  source: {}".format(result["source"]))
        print("  live:   {}".format(result["live"]))
        print("  ftml:   {}".format(result["ftml"]))
        if result["space_only"]:
            print("  whitespace-only difference")

    if args.json_out:
        with Path(args.json_out).open("w", encoding="utf-8") as handle:
            json.dump(summary, handle, ensure_ascii=False, indent=2)
            handle.write("\n")
        print()
        print("wrote {}".format(args.json_out))

    return 0


if __name__ == "__main__":
    sys.exit(main())
