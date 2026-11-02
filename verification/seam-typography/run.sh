#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"

summarize() {
  (cd "$1" && CARGO_INCREMENTAL=0 cargo test --lib --quiet seam_equivalence -- --ignored --nocapture 2>&1 || true) \
  | python3 -I -c '
import re, sys
raw = sys.stdin.read()
bound = None
for line in raw.splitlines():
    m = re.match(r"SEAM-BOUND max_tokens=(\d+)", line)
    if m: bound = m.group(1)
if bound is None:
    sys.stderr.write(raw[-3000:] + "\nCHECK DID NOT RUN (build or test harness error)\n"); sys.exit(3)
sys.stderr.write("bound: max_tokens=%s\n" % bound)
for line in raw.splitlines():
    m = re.match(r"CLASS (.*?)\|(\d+)\|(\d+)\|", line)
    if m: print("%s|%s|%s" % (m.group(1), m.group(2), m.group(3)))
'
}

known_for_bound() {
  { grep -v '^#' "$here/known-findings.txt" || true; } | awk -F'|' -v b="$1" '$2 <= b { if (b == 5) print $1 "|" $2 "|" $3; else print $1 "|" $2 }' | sort
}

compare() {
  known_for_bound "$2" > "$1.known"
  if [ "$2" = 5 ]; then sort "$1" > "$1.actual"; else cut -d'|' -f1,2 "$1" | sort > "$1.actual"; fi
  if diff -u "$1.known" "$1.actual" > "$1.diff"; then
    echo "MATCHES known finding classes ($(wc -l < "$1.known") known)"; return 0
  fi
  echo "DIFFERS ('-' known but gone = fixed or masked; '+' new class or shorter input = new finding):"
  grep '^[-+][^-+]' "$1.diff"; return 1
}

run_one() {
  BOUND="$( { summarize "$1" > "$2"; } 2>&1 >/dev/null | sed -n 's/^bound: max_tokens=//p' )"
  [ -n "$BOUND" ] || { cat "$2"; echo "CHECK DID NOT RUN"; return 3; }
}

if [ "${1:-}" != "mutants" ]; then
  tmp="$(mktemp)"; trap 'rm -f "$tmp" "$tmp".*' EXIT
  run_one "$repo" "$tmp"; echo "bound: max_tokens=$BOUND"
  compare "$tmp" "$BOUND"; exit $?
fi

work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
mkdir -p "$work/repo"
(cd "$repo" && tar --exclude=./target --exclude=./.git --exclude='./verification/*/target' --exclude=./fuzz -cf - .) | tar -xf - -C "$work/repo"
export CARGO_TARGET_DIR="$work/target"
src="$work/repo/src/delayed/seam.rs"; orig="$work/seam.rs.orig"; cp "$src" "$orig"
failed=0
mutate() { # name, old text, new text
  cp "$orig" "$src"
  python3 -I - "$src" "$2" "$3" <<'PY'
import sys
p, old, new = sys.argv[1:4]
s = open(p).read()
assert s.count(old) == 1, ("mutant search text must occur once", old, s.count(old))
open(p, "w").write(s.replace(old, new))
PY
  if run_one "$work/repo" "$work/sum" && compare "$work/sum" "$BOUND" >/dev/null; then
    echo "MUTANT SURVIVED (check is too weak): $1"; failed=1
  else
    echo "caught: $1 -> $(grep -m1 '^+[^+]' "$work/sum.diff" 2>/dev/null | cut -c1-100 || true)"
  fi
}
mutate "seam edits never applied" \
  "    if edits.is_empty() {" "    if true {"
mutate "text after an edit is emitted from the wrong place" \
  "        cursor = range.end;
    }
    emit_source_range" "        cursor = range.start;
    }
    emit_source_range"
mutate "seam positions not de-duplicated or sorted" \
  "    seams.sort_unstable();
    seams.dedup();" "    seams.reverse();"
exit $failed
