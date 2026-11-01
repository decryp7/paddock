#!/usr/bin/env bash
# The release notes of an app's FIRST release come from its CHANGELOG, not from git-cliff (the monorepo's shared paths
# reach back through years of other apps' commits). Used by app-release.yml twice: in the bundle job (fail fast, before
# three OS build) and in the release job (writes the notes).
#
#   FIRST_CHANGELOG  the app's CHANGELOG.md; empty: nothing to do
#   TAG              the tag being released (refs/tags name), PREFIX the app's tag prefix (dyno-)
#   OUT              where to write the section; unset: only check it
#
# Exit 0 and nothing written: this is not a first release (an earlier PREFIXv* tag exists) or no CHANGELOG is configured,
# so git-cliff takes over. Exit 0 with the section written: the first release. Exit 1: the first release has no usable
# section (missing, an Unreleased heading, or nothing but blank lines and link references).
#
# The awk program stays inside what POSIX awk, mawk (Ubuntu's default), gawk and BSD awk all read the same: no bracket
# expression that holds a ']' (the test lints for it and runs the script under every awk it finds).
set -euo pipefail

changelog=${FIRST_CHANGELOG:-}
tag=${TAG:?TAG is required}
prefix=${PREFIX:-}
[ -n "$changelog" ] || exit 0

# Earlier tags of the app. A CI checkout is shallow and holds only its own tag, so ask the server (ls-remote downloads no
# objects); without an origin remote (a local run) the repository's own tags are all there is. A listing that fails is an
# error, never "no earlier tag": that would turn a later release into a first one.
if git remote get-url origin >/dev/null 2>&1; then
  if ! listed=$(git ls-remote --tags --refs origin "refs/tags/${prefix}v*" 2>&1); then
    echo "::error::could not list the tags of origin: $listed"
    exit 1
  fi
  earlier=$(printf '%s\n' "$listed" | sed -n 's#.*refs/tags/##p')
else
  earlier=$(git tag -l "${prefix}v*")
fi
if [ -n "$(printf '%s\n' "$earlier" | grep -vxF "$tag" | grep -v '^$' || true)" ]; then
  exit 0
fi

version=${tag#"${prefix}v"}
[ -f "$changelog" ] || { echo "::error::$changelog does not exist"; exit 1; }

heading=$(grep -m1 -F "## [$version]" "$changelog" || true)
[ -n "$heading" ] || { echo "::error::$changelog has no '## [$version]' section: write the release notes there before tagging"; exit 1; }
if printf '%s' "$heading" | grep -qi 'unreleased'; then
  echo "::error::$changelog still says '$heading': date the section (bump the version, then tag)"
  exit 1
fi

# Diagnostics on stderr (ER-27): what the section was read with, from which file, so a repeat failure explains itself.
awk_id=$( (awk -W version 2>/dev/null || awk --version 2>/dev/null) | head -n 1 || true)
bytes=$(wc -c < "$changelog" | tr -d ' ')
sha=$( (sha256sum "$changelog" 2>/dev/null || shasum -a 256 "$changelog" 2>/dev/null) | cut -d' ' -f1 || true)
heading_line=$(grep -n -m1 -F "## [$version]" "$changelog" | cut -d: -f1 || true)
echo "first-release-notes: awk='${awk_id:-unknown}' changelog=$changelog bytes=$bytes sha256=${sha:-unknown} heading_line=${heading_line:-?}" >&2

section=$(awk -v h="## [$version]" '
  index($0, h) == 1 { on = 1; next }
  on && /^## / { exit }
  on && substr($0, 1, 1) == "[" && index($0, "]: ") > 2 { next }
  on
' "$changelog")
# No pipe into grep -q here: under pipefail a reader that quits early (SIGPIPE) would report a non-empty section as empty.
echo "first-release-notes: section lines=$(printf '%s\n' "$section" | wc -l | tr -d ' ') bytes=$(printf '%s\n' "$section" | wc -c | tr -d ' ')" >&2
if [[ ! $section =~ [^[:space:]] ]]; then
  echo "first-release-notes: the first 3 lines after the heading:" >&2
  tail -n +"$((${heading_line:-0} + 1))" "$changelog" | head -n 3 | cut -c1-120 | sed 's/^/  | /' >&2
  echo "::error::the '## [$version]' section of $changelog is empty"
  exit 1
fi
if [ -n "${OUT:-}" ]; then
  mkdir -p "$(dirname "$OUT")"
  printf '%s\n' "$section" > "$OUT"
fi
