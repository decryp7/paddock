#!/usr/bin/env bash
# Merge driver for docs/lessons.md (`merge=lessons` in .gitattributes; `./dev hooks install` sets `merge.lessons.driver`):
#   lessons-merge.sh <base> <ours> <theirs>      result is written to <ours>, as git's drivers do
# The file only gains sections, so two branches that both add one at the end are merged with git's union merge (both sides' lines
# kept). That drops a blank line: a line both sides added identically (the separator before a `## ` heading) is taken as one shared
# line, so the second branch's heading ends up glued to the last line of the first branch's section (9ec43be3c, b9bf5fc4b, 75ce0d2c8,
# 5c241d93f; measured with `git merge-file --union` on two branches that each append "\n## X\n- x\n"). After the union this puts the
# blank line back before every `## ` heading outside a fenced block (the rule conformance C145 checks), so the union is whole.
set -eu
[ "$#" -eq 3 ] || { echo "::error::lessons-merge: usage: lessons-merge.sh <base> <ours> <theirs>" >&2; exit 2; }
base="$1" ours="$2" theirs="$3"
code=0
git merge-file --union "$ours" "$base" "$theirs" || code=$?
# merge-file --union reports resolved conflicts as a count, never an error; a status of 128 or more is a failure.
if [ "$code" -ge 128 ]; then echo "::error::lessons-merge: git merge-file failed (exit $code). Next step: merge docs/lessons.md by hand and keep both sides' sections." >&2; exit 1; fi
tmp="$(mktemp "$ours.XXXXXX")"
trap 'rm -f "$tmp"' EXIT
awk '
  /^(```|~~~)/ { fence = !fence }
  !fence && /^## / && NR > 1 && last != "" { print "" }
  { print; last = $0 }
' "$ours" > "$tmp"
mv "$tmp" "$ours"
trap - EXIT
exit 0
