#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"

run_check() {
  (cd "$1" && CARGO_INCREMENTAL=0 cargo test --release --quiet -- --nocapture 2>&1)
}

if [ "${1:-}" != "mutants" ]; then
  if out="$(run_check "$here")"; then
    echo "$out" | grep -oE "checked [0-9]+ inputs, all properties held"; echo "PASS"; exit 0
  else
    echo "$out" | grep -E "inputs checked|^\"|^'|panicked|^error" | head -12; echo "FAIL"; exit 1
  fi
fi

work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
mkdir -p "$work/repo"
(cd "$repo" && tar --exclude=./target --exclude=./.git --exclude='./verification/*/target' --exclude=./fuzz -cf - .) | tar -xf - -C "$work/repo"
export CARGO_TARGET_DIR="$work/target"
src="$work/repo/src/includes/mod.rs"; orig="$work/mod.rs.orig"; cp "$src" "$orig"
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
  if run_check "$work/repo/verification/include-expansion" >"$work/out.txt"; then
    echo "MUTANT SURVIVED (check is too weak): $1"; failed=1
  else
    echo "caught: $1 -> $(grep -m1 -E '^"' "$work/out.txt" | cut -c1-140)"
  fi
}
mutate "empty page no longer removes the include range" \
  "if replace_with.is_empty() {" "if false {"
mutate "included page not recorded" \
  "pages.push(page_ref);" "let _ = &page_ref;"
mutate "byte index off by one before the include" \
  "input[..range.start].ends_with" "input[..range.start + 1].ends_with"
mutate "byte index off by one after the include" \
  "input[range.end..].starts_with" "input[range.end + 1..].starts_with"
exit $failed
