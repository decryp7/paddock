#!/usr/bin/env bash
# The one reader of what an actions/cache step did (ADR 0115): every cache step of a job reports through it, from the step's own outputs
# (`cache-hit`, `cache-matched-key`, and for a separate save step its outcome), never from log text or from what a later step guesses.
#
#   cache-report.sh outcome <cache-hit> <matched-key>         prints exact | partial | miss
#   cache-report.sh line <name> <key> <cache-hit> <matched-key> <saved>
#                                                              the job-summary line and one `cache-report:` line for the audit
# <cache-hit> is actions/cache's output: "true" for an exact key, "false" for a restore-keys match, empty for no match or a step that did
# not run. <saved> is yes | no | pending | n/a: `pending` is a save in actions/cache's post step at the end of the job (its result cannot be
# known here; `ci cache-audit` reads whether the key ever got saved), `n/a` is a cache that is never saved by this job.
# The `cache-report:` line is the fixed format `ci cache-audit` (tools/paddock-dev) reads from a job log: change both together.
set -euo pipefail

outcome() {
  local hit="${1:-}" matched="${2:-}"
  if [ "$hit" = true ]; then echo exact
  elif [ -n "$matched" ]; then echo partial
  else echo miss; fi
}

case "${1:-}" in
  outcome) shift; outcome "$@" ;;
  line)
    shift
    name="${1:?name}" key="${2:-}" hit="${3:-}" matched="${4:-}" saved="${5:-n/a}"
    o="$(outcome "$hit" "$matched")"
    case "$o" in
      exact) what="restored: exact hit" ;;
      partial) what="restored: partial hit (matched \`$matched\`)" ;;
      *) what="restored: miss" ;;
    esac
    case "$saved" in
      yes) sv="saved: yes (found again by key)" ;;
      no) sv="saved: NO" ;;
      pending) sv="saved: pending (actions/cache's post step at the end of the job; a failed save is only a warning in its log)" ;;
      *) sv="saved: not by this job" ;;
    esac
    echo "- $name cache \`$key\`: $what; $sv" | tee -a "${GITHUB_STEP_SUMMARY:-/dev/null}"
    echo "cache-report: name=$name outcome=$o saved=$saved key=$key"
    ;;
  *) echo "usage: cache-report.sh outcome <hit> <matched> | line <name> <key> <hit> <matched> <saved>" >&2; exit 2 ;;
esac
