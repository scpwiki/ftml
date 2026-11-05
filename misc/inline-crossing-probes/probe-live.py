#!/usr/bin/env python3
"""Capture live Wikidot PagePreview bytes for the inline-crossing probes.

Usage:
    probe-live.py [batch.json]

`batch.json` is a JSON object mapping probe id -> source text.  It defaults to
`probes.json` next to this script.  Captures are merged into
`live-results.json` in the same directory, in this shape:

    {
      "<id>": {
        "source": "<source text>",
        "runs": ["<body from run 1>", "<body from run 2>"],
        "det": true | false
      },
      ...
    }

Every requested case is fetched twice through the anonymous
`edit/PagePreviewModule` AMC endpoint, ~0.35 s apart.  Cached ids whose source
is unchanged are not re-probed; an id that already maps to a different source
is a hard error (ids are never reused for new sources).  A case whose request
still errors after one 2 s retry is recorded with an `"error"` string and
whatever runs succeeded, and the capture continues.

Requires the wikidot.py uv environment:

    cd /home/roku/src/Rokurolize/wikidot.py && \
        uv run python <path-to-this-script> [batch.json]
"""

import json
import os
import sys
import time
from pathlib import Path

REC = Path(__file__).resolve().parent
DEFAULT_BATCH = REC / "probes.json"
OUT_PATH = REC / "live-results.json"

SITE = "sandbox-for-codex"
REQUEST_INTERVAL = 0.35  # seconds between HTTP requests
RETRY_DELAY = 2.0        # seconds before the single retry


def make_client():
    from wikidot.connector.ajax import (
        AjaxModuleConnectorClient,
        AjaxModuleConnectorConfig,
    )

    AjaxModuleConnectorClient._check_existence_and_ssl = lambda self: True
    return AjaxModuleConnectorClient(
        site_name=SITE,
        config=AjaxModuleConnectorConfig(
            allow_insecure_session_transport_for=SITE
        ),
    )


def fetch_body(client, source):
    (response,) = client.request(
        [{"moduleName": "edit/PagePreviewModule", "source": source}]
    )
    body = response.json().get("body")
    # Keep the raw body verbatim; only a JSON null becomes the empty string.
    return body or ""


def fetch_with_retry(client, source):
    """Return (body, None) on success or (None, error_text) after one retry."""
    try:
        return fetch_body(client, source), None
    except Exception as first_error:
        time.sleep(RETRY_DELAY)
        try:
            return fetch_body(client, source), None
        except Exception as second_error:
            return None, "{}: {} (retry: {}: {})".format(
                type(first_error).__name__,
                first_error,
                type(second_error).__name__,
                second_error,
            )


def probe_case(client, case_id, source):
    runs = []
    for run_index in range(2):
        body, error = fetch_with_retry(client, source)
        if error is not None:
            print(
                "  {}: run {} failed: {}".format(case_id, run_index + 1, error),
                flush=True,
            )
            return {
                "source": source,
                "runs": runs,
                "det": False,
                "error": error,
            }
        runs.append(body)
        time.sleep(REQUEST_INTERVAL)
    det = runs[0] == runs[1]
    if not det:
        print("  {}: NONDETERMINISTIC".format(case_id), flush=True)
    return {"source": source, "runs": runs, "det": det}


def load_json(path):
    if not path.exists():
        return {}
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def write_json(path, value):
    tmp = path.with_name(path.name + ".tmp")
    with tmp.open("w", encoding="utf-8") as handle:
        json.dump(value, handle, ensure_ascii=False, indent=2)
        handle.write("\n")
    os.replace(tmp, path)


def is_complete_capture(entry, source):
    """A cached capture that should not be probed again."""
    return (
        isinstance(entry, dict)
        and entry.get("source") == source
        and isinstance(entry.get("runs"), list)
        and len(entry["runs"]) == 2
        and "error" not in entry
    )


def main(argv):
    if len(argv) > 2:
        sys.exit("usage: probe-live.py [batch.json]")

    batch_path = Path(argv[1]) if len(argv) == 2 else DEFAULT_BATCH
    if not batch_path.is_absolute():
        batch_path = Path.cwd() / batch_path
    with batch_path.open(encoding="utf-8") as handle:
        batch = json.load(handle)
    if not isinstance(batch, dict):
        sys.exit("error: batch file must be a JSON object {id: source}")
    for case_id, source in batch.items():
        if not isinstance(source, str):
            sys.exit("error: source for id {!r} is not a string".format(case_id))

    results = load_json(OUT_PATH)

    # An id is never reused for a different source, cached or not.
    conflicts = [
        (case_id, results[case_id].get("source"), source)
        for case_id, source in batch.items()
        if isinstance(results.get(case_id), dict)
        and results[case_id].get("source") != source
    ]
    if conflicts:
        for case_id, cached_source, new_source in conflicts:
            print(
                "error: id {!r} reused with a different source:\n"
                "  cached: {!r}\n  batch:  {!r}".format(
                    case_id, cached_source, new_source
                ),
                file=sys.stderr,
            )
        sys.exit(1)

    pending = [
        (case_id, source)
        for case_id, source in batch.items()
        if not is_complete_capture(results.get(case_id), source)
    ]
    skipped = len(batch) - len(pending)
    print(
        "batch {}: {} case(s), {} cached, {} to probe".format(
            batch_path, len(batch), skipped, len(pending)
        ),
        flush=True,
    )

    client = None
    failures = []
    nondeterministic = []
    for index, (case_id, source) in enumerate(pending, 1):
        if client is None:
            client = make_client()
        entry = probe_case(client, case_id, source)
        results[case_id] = entry
        write_json(OUT_PATH, results)
        elapsed = ""
        if "error" in entry:
            failures.append(case_id)
            status = "ERROR"
        else:
            status = "ok" if entry["det"] else "NONDETERMINISTIC"
            if not entry["det"]:
                nondeterministic.append(case_id)
        print(
            "[{}/{}] {}: {} ({} run(s)){}".format(
                index, len(pending), case_id, status, len(entry["runs"]), elapsed
            ),
            flush=True,
        )

    # Our own det bookkeeping, including cached entries touched this run.
    for case_id, source in batch.items():
        entry = results.get(case_id)
        if isinstance(entry, dict) and is_complete_capture(entry, source):
            entry["det"] = entry["runs"][0] == entry["runs"][1]
            if not entry["det"] and case_id not in nondeterministic:
                nondeterministic.append(case_id)
    write_json(OUT_PATH, results)

    probed = len(pending) - len(failures)
    total_ok = sum(
        1
        for case_id, source in batch.items()
        if is_complete_capture(results.get(case_id), source)
    )
    print()
    print(
        "captured {}/{} case(s) ({} probed this run, {} cached), {} failure(s)".format(
            total_ok, len(batch), probed, skipped, len(failures)
        )
    )
    print(
        "nondeterministic: {}".format(
            " ".join(sorted(nondeterministic)) if nondeterministic else "none"
        )
    )
    if failures:
        print("failed ids: {}".format(" ".join(sorted(failures))))
    print("wrote {}".format(OUT_PATH))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
