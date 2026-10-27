#!/usr/bin/env bash
# Windows host runners: a finished node_modules per package-lock.json, kept in %USERPROFILE%\.debrief-ci, linked into the
# checkout as a directory junction, so `npm ci` runs only when the lockfile changed (unpacking ~900 packages into a fresh
# node_modules took 7-14 min per job even with the npm cache on disk).
#
#   bash apps/debrief/scripts/ci/windows-node-modules.sh link     (from the repo root: the monorepo's one lock, ADR 0014 §1)
#   bash apps/debrief/scripts/ci/windows-node-modules.sh unlink   (the job's last step, `if: always()`)
#
# The checkout may be a kept folder (stable-workspace), so a junction an earlier, killed job left is removed first. Only the
# junction is ever removed (rmdir on a junction removes the link, never the target's files).
set -euo pipefail
action=${1:?usage: windows-node-modules.sh link|unlink}
base="${DEBRIEF_CI_HOME:-$USERPROFILE/.debrief-ci}"

unlink_junction() {
  [ -e node_modules ] || [ -L node_modules ] || return 0
  # A junction (Git bash reports it as a link) is removed with rmdir, which never touches the target.
  cmd //c rmdir node_modules 2>/dev/null || true
  if [ -L node_modules ]; then
    echo "::warning::could not remove the node_modules junction; leaving it (never deleting through it)."
    return 0
  fi
  # A real folder (an earlier npm ci in this checkout) is the checkout's own.
  if [ -e node_modules ]; then rm -rf node_modules; fi
}

case "$action" in
  unlink) unlink_junction ;;
  link)
    unlink_junction
    key=$(sha256sum package-lock.json | cut -c1-16)
    # <hash>/node_modules, not node_modules-<hash>: Node resolves packages from the junction's real path, so it must still
    # contain a folder named node_modules (else e.g. @tauri-apps/cli cannot find its native binary).
    cached="$base/node-cache/$key/node_modules"
    mkdir -p "$base/node-cache/$key"
    if [ ! -f "$cached/.complete" ]; then
      # An incomplete copy (a killed job) goes. The smoke and installer jobs can install the same new lockfile at once:
      # each renames its finished copy into place in one step, and the one that comes second keeps the first one's.
      rm -rf "$cached"
      npm ci
      touch node_modules/.complete
      mine="$cached.$$"
      mv node_modules "$mine"
      mv -T "$mine" "$cached" 2>/dev/null || rm -rf "$mine"
    fi
    powershell.exe -NoProfile -Command "New-Item -ItemType Junction -Path '$(cygpath -w "$PWD/node_modules")' -Target '$(cygpath -w "$cached")' | Out-Null"
    # Older lockfiles' copies go (and the first layout's node_modules-<hash> folders).
    for old in "$base"/node-cache/*; do [ "$old" = "$base/node-cache/$key" ] || rm -rf "$old"; done
    rm -rf "$base"/node_modules-*
    ;;
  *) echo "usage: windows-node-modules.sh link|unlink" >&2; exit 2 ;;
esac
