#!/usr/bin/env bash
# The Linux half of the other-os member gate (`./dev merge check --gates`, tools/paddock-dev/src/merge_check/other_os.rs): the
# member's affected tests run in the CI image, as root, with TMPDIR on an ext4 loop mount, in a FRESH shallow clone of the merge
# commit (no repo-local git config, as actions/checkout leaves it), with CI's environment variables. Docker only through
# tools/linux-container.sh (the repository is mounted read-only; nothing from $HOME).
#
#   scripts/member-gate-linux.sh <tree> <spec>...      <tree>: the tree of HEAD (the control line names it)
#   spec: `nextest <workspace> <package> [<nextest filter, the rest of the line>]` or `node <dir> <command...>`
#
# Exit: 0 passed, 100 tests failed, 75 not measured (Docker down, the checkout is not <tree>, a control failed); the control line
# `other-os-control: os=linux tree=<tree> ...` is printed only once the environment is shown to be the asked one.
# Environment: PADDOCK_MEMBER_GATE_PLATFORM (default linux/amd64, the CI image's own: on an arm64 Mac it runs under emulation, where
# a failed exec may not be reported and timing-bound tests run slower, but the native arm64 image has no pinned Chromium and gave a
# false conformance failure; set linux/arm64 for a quick look at a change that cannot depend on either).
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/.." && pwd)"
tree="${1:?usage: member-gate-linux.sh <tree> <spec>...}"; shift
[ $# -gt 0 ] || { echo "::error::member-gate-linux: no spec given. Usage: member-gate-linux.sh <tree> <spec>..." >&2; exit 2; }
docker="${DOCKER:-docker}"
"$docker" info >/dev/null 2>&1 || { echo "::error::member-gate-linux: $docker (OrbStack or Docker) is not answering, so nothing ran. Next step: start it yourself (agents never restart it, ER-14) and run this again" >&2; exit 75; }
platform_args=(--platform "${PADDOCK_MEMBER_GATE_PLATFORM:-linux/amd64}")

started=$SECONDS
bash "$root/tools/linux-container.sh" --id paddock --ci-image --loop-mount --root "$root" ${platform_args[@]+"${platform_args[@]}"} -- \
  bash /work/scripts/member-gate-linux-inner.sh "$tree" "$@"
rc=$?
echo "member-gate-linux: container run took $((SECONDS - started)) s, exit $rc" >&2
case "$rc" in 0 | 100 | 75) exit "$rc" ;; *) echo "::error::member-gate-linux: the container run exited $rc (not 0, 100 or 75), so it is not a verdict. Next step: read the output above" >&2; exit 75 ;; esac
