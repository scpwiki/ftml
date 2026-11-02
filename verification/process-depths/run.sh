#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"

run_check() {
  (cd "$1" && CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$1/target}" CARGO_INCREMENTAL=0 \
     cargo test --release --quiet -- --nocapture 2>&1)
}

if [ "${1:-}" != "mutants" ]; then
  if out="$(run_check "$here")"; then
    echo "$out" | grep -E "^checked"; echo "PASS"
  else
    echo "$out" | grep -E "inputs checked|rows \[|panicked|error" | head -12; echo "FAIL"; exit 1
  fi
  exit 0
fi

work="$(mktemp -d)"; trap 'rm -rf "$work"' EXIT
mkdir -p "$work/src/parsing" "$work/verification"
cp "$repo/src/non_empty_vec.rs" "$work/src/"
cp -r "$here" "$work/verification/process-depths"; rm -rf "$work/verification/process-depths/target"
dst="$work/src/parsing/depth.rs"

mutate() {
  cp "$repo/src/parsing/depth.rs" "$dst"
  python3 - "$dst" "$2" "$3" <<'PY'
import sys
p,old,new=sys.argv[1:4]
s=open(p).read()
assert s.count(old)==1,(old,s.count(old))
open(p,'w').write(s.replace(old,new))
PY
  if run_check "$work/verification/process-depths" >"$work/out.txt"; then
    echo "MUTANT SURVIVED (check is too weak): $1"; failed=1
  else
    echo "caught: $1 -> $(grep -m1 -E 'rows \[' "$work/out.txt" | cut -c1-110)"
  fi
}
failed=0
export CARGO_TARGET_DIR="$work/target"
mutate "no new list when the type changes" \
  "if stack.last_type() != ltype {
            stack.new_list(ltype);
        }" "let _ = ltype;"
mutate "one level not closed when depth decreases" \
  "for _ in depth..previous {" "for _ in depth..previous.saturating_sub(1) {"
mutate "empty top-level trees kept" \
  "if !list.is_empty() {
            self.finished.push((ltype, list));
        }" "self.finished.push((ltype, list));"
mutate "closed sublist dropped" \
  "self.push(DepthItem::List(ltype, list));" "let _ = (ltype, list);"
mutate "new levels not opened for a depth increase" \
  "for _ in previous..depth {" "for _ in previous..depth.min(previous + 1) {"
exit $failed
