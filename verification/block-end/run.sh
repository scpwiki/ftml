#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"

summarize() {
  (cd "$1" && CARGO_INCREMENTAL=0 cargo test --lib --quiet block_end_closers -- --ignored --nocapture --test-threads=1 2>&1 || true) \
  | python3 -I -c '
import re, sys
raw = sys.stdin.read()
findings, pairing, bound = set(), [], None
for line in raw.splitlines():
    m = re.match(r"MINIMAL-FAILURE message=(.*?) count=\d+ source=", line)
    if m:
        findings.add("F|" + m.group(1))
    i = line.find("PAIRING ")
    if i >= 0:
        pairing.append("P|" + line[i:])
    b = re.match(r"ORDERS-BOUND (.*)", line)
    if b:
        bound = b.group(1)
if bound is None or not pairing:
    sys.stderr.write(raw[-3000:] + "\nCHECK DID NOT RUN (build or test harness error)\n")
    sys.exit(3)
sys.stderr.write("bound: %s\n" % bound)
for line in sorted(findings) + pairing:
    print(line)
'
}

compare() {
  { grep -v '^#' "$here/known-findings.txt" | sed 's/^/F|/' || true; grep -v '^#' "$here/recorded-pairing.txt" | sed 's/^/P|/'; } | sort > "$1.known"
  sort "$1" > "$1.actual"
  if diff -u "$1.known" "$1.actual" > "$1.diff"; then
    echo "MATCHES known findings and recorded pairing ($(grep -c '^F|' "$1.known" || true) findings, $(grep -c '^P|' "$1.known") pairing rows)"; return 0
  fi
  echo "DIFFERS ('-' recorded but gone = fixed or masked; '+' new = new finding or changed pairing):"
  grep '^[-+][^-+]' "$1.diff"; return 1
}

if [ "${1:-}" != "mutants" ]; then
  tmp="$(mktemp)"; trap 'rm -f "$tmp" "$tmp".*' EXIT
  summarize "$repo" > "$tmp"
  compare "$tmp"; exit $?
fi

work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
mkdir -p "$work/repo"
(cd "$repo" && tar --exclude=./target --exclude=./.git --exclude='./verification/*/target' --exclude=./fuzz -cf - .) | tar -xf - -C "$work/repo"
export CARGO_TARGET_DIR="$work/target"
failed=0
mutate() { # name, file (relative to repo), old text, new text
  local file="$work/repo/$2" orig="$work/orig.$$"
  cp "$file" "$orig"
  python3 -I - "$file" "$3" "$4" <<'PY'
import sys
p, old, new = sys.argv[1:4]
s = open(p).read()
assert s.count(old) == 1, ("mutant search text must occur once", old, s.count(old))
open(p, "w").write(s.replace(old, new))
PY
  if summarize "$work/repo" > "$work/sum" 2>"$work/err" && compare "$work/sum" >/dev/null; then
    echo "MUTANT SURVIVED (check is too weak): $1"; failed=1
  else
    echo "caught: $1 -> $(grep -m1 '^+[^+]' "$work/sum.diff" 2>/dev/null | cut -c1-120 || true)"
  fi
  cp "$orig" "$file"
}
mutate "math and bibliography closers lose the blank-remainder exemption (the #780 bug)" \
  src/parsing/rule/impls/block/parser.rs \
  '|| parser.at_blank_line_remainder())' \
  '|| (block_rule.name == "block-code" && parser.at_blank_line_remainder()))'
mutate "code closer scan rejects any suffix (the #776 bug)" \
  src/wikidot_code.rs \
  '// but any other suffix leaves the marker as body text.' \
  '// but any other suffix leaves the marker as body text.
    if closer_end != line_end {
        return false;
    }'
mutate "math stops counting nested openers" \
  src/parsing/rule/impls/block/parser.rs \
  'nested_wikidot_math += 1;' \
  'nested_wikidot_math += 0;'
exit $failed
