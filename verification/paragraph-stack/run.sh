#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"

summarize() {
  (cd "$1" && CARGO_INCREMENTAL=0 cargo test --lib --quiet stack_orders -- --ignored --nocapture 2>&1 || true) \
  | python3 -I -c '
import re, sys
best = {}
bound = None
raw = sys.stdin.read()
for line in raw.splitlines():
    m = re.match(r"MINIMAL-FAILURE wikidot=(\w+) ops=\[(.*?)\] panic=(.*)$", line)
    if m:
        n = len(m.group(2).split(", "))
        msg = re.sub(r" at \S+:\d+$", "", m.group(3))
        msg = re.sub(r"\d+", "N", msg)
        best[msg] = min(best.get(msg, 99), n)
    b = re.match(r"ORDERS-BOUND (.*)", line)
    if b:
        bound = b.group(1)
if bound is None:
    sys.stderr.write(raw[-3000:] + "\nCHECK DID NOT RUN (build or test harness error)\n")
    sys.exit(3)
sys.stderr.write("bound: %s\n" % bound)
for msg, n in sorted(best.items()):
    print("%s|%d" % (msg, n))
'
}

compare() {
  { grep -v '^#' "$here/known-findings.txt" || true; } | sort > "$1.known"
  sort "$1" > "$1.actual"
  if diff -u "$1.known" "$1.actual" > "$1.diff"; then
    echo "MATCHES known findings ($(wc -l < "$1.known") known)"; return 0
  fi
  echo "DIFFERS from known-findings.txt ('-' known but gone = fixed or masked; '+' new or shorter = new finding):"
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
src="$work/repo/src/parsing/paragraph/stack.rs"
orig="$work/stack.rs.orig"; cp "$src" "$orig"
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
  if summarize "$work/repo" > "$work/sum" 2>"$work/err" && compare "$work/sum" >/dev/null; then
    echo "MUTANT SURVIVED (check is too weak): $1"; failed=1
  else
    echo "caught: $1 -> $(grep -m1 '^+[^+]' "$work/sum.diff" 2>/dev/null | cut -c1-120 || true)"
  fi
}
mutate "table merge assumes the last element is a table" \
  'Some(other) => {
                self.finished.push(other);
                return;
            }' \
  'Some(_) => unreachable!("mutant: last element is not a table"),'
mutate "pop_line_break removes the wrong index" \
  '            self.current.remove(index);
        } else if self.wikidot' \
  '            self.current.remove(index + 1);
        } else if self.wikidot'
mutate "footnote reopening armed even without a preceding paragraph" \
  '        self.wikidot_reopen_for_footnote = self.wikidot
            && matches!(
                self.finished.last(),
                Some(Element::Container(container))
                    if container.ctype() == ContainerType::Paragraph
            );' \
  '        self.wikidot_reopen_for_footnote = self.wikidot;'
mutate "finished-list pop_line_break removes the wrong index" \
  'self.finished.remove(index);
        }
    }

    pub(crate) fn ensure_wikidot_trailing_line_break' \
  'self.finished.remove(index + 1);
        }
    }

    pub(crate) fn ensure_wikidot_trailing_line_break'
exit $failed
