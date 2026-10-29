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

section=$(awk -v h="## [$version]" '
  index($0, h) == 1 { on = 1; next }
  on && /^## / { exit }
  on && /^\[[^]]+\]: / { next }
  on
' "$changelog")
if ! printf '%s\n' "$section" | grep -q '[^[:space:]]'; then
  echo "::error::the '## [$version]' section of $changelog is empty"
  exit 1
fi
if [ -n "${OUT:-}" ]; then
  mkdir -p "$(dirname "$OUT")"
  printf '%s\n' "$section" > "$OUT"
fi
