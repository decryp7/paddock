#!/usr/bin/env bash
# Job isolation on a host runner (.gitea/actions/README.md, "Job isolation"; ADR 0025): any number of jobs may run on one
# host at once (the runner's capacity) without one touching another's folders or processes. The one way an action or
# workflow reaches a folder of the CI home.
#
#   job-isolation.sh acquire                 the job's slot (first bash step); exports PADDOCK_CI_SLOT, PADDOCK_JOB_TEMP, TMPDIR
#   job-isolation.sh release                 gives the slot back (last step, if: always())
#   job-isolation.sh path <kind> [<name>]    a kept folder of kept-folders.txt (created), for this job's slot
#   job-isolation.sh temp                    the job's own temp folder (the runner's $RUNNER_TEMP)
#   job-isolation.sh owns <folder> [<kind>]  exit 0 when <folder> is a kept folder (of that kind) of this job's slot
#   job-isolation.sh port <name>             the first port of this slot's block for <name> in ports.txt
#   job-isolation.sh pattern <regex>         a process pattern with {tmp}, {build} and {slot} filled in; exit 1 when it
#                                            names none of them (it would match other jobs' processes), or when {slot}
#                                            does not end the pattern right after a kept folder's name
#
# Two kinds of state:
# - Job-scoped (temp folders, the clean-up's temp sweep and process patterns): the runner's own per-job folder. The Gitea
#   runner gives every job <workdir_parent>/<16 random hex>/{hostexecutor,tmp,tool_cache,act}, sets RUNNER_TEMP to its tmp,
#   and deletes the whole folder after the job. acquire points TMPDIR (TMP and TEMP on Windows) at it.
# - Kept (kept-folders.txt): a stable absolute path per slot, so caches stay warm. A slot is <ci-home>/slots/s<n>, taken by
#   an atomic mkdir (the first free one of s1..s$PADDOCK_CI_SLOTS); its owner file names the job's RUNNER_TEMP. A slot whose
#   owner is gone (that folder deleted, older than PADDOCK_SLOT_MAX_AGE_MIN, or no owner file after 5 minutes) is reclaimed;
#   a live job's slot never is. (To be replaced by the Rust slot lock, `./dev ci slot`, ADR 0025.)
# - Disk: the first job in slot s1 renames a kept folder of the old unslotted layout to its -s1 name (a warm cache, no
#   second copy). Kept folders of slots above PADDOCK_CI_KEEP_SLOTS (default 2, the capacity) are removed when their job
#   releases the slot, and swept at acquire when no job holds that slot.
#
# PADDOCK_CI_SLOTS: the most slots one host hands out; set it in the runner's environment (config.yaml runner.envs) to more
# than its capacity, or leave the default of 8. A Linux container is a new file system per job, but every container of a
# host shares the runner's /sccache mount (the compile, npm and Angular caches): acquire and release do nothing there yet
# and everything else uses the single slot s1; the Angular cache on the mount is shared by two runs of the same job on one
# Linux host until the Rust slot lock (an OS file lock on the mount, which all containers of a host see) gives containers
# slots too (ADR 0025).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
table="$here/kept-folders.txt"
ports="$here/ports.txt"
shopt -s extglob

die() { echo "::error::job-isolation: $*" >&2; exit 1; }
is_linux() { [ "${RUNNER_OS:?RUNNER_OS is not set}" = Linux ]; }

job_temp() {
  local t="${RUNNER_TEMP:?RUNNER_TEMP is not set (the per-job temp folder of the runner)}"
  if command -v cygpath >/dev/null 2>&1; then t="$(cygpath -u "$t")"; fi
  printf '%s\n' "${t%/}"
}

# The CI home: ci-home.sh is the one implementation (PADDOCK_CI_HOME or ~/.paddock-ci; a missing relocated home fails).
ci_home() {
  local h
  h="$(bash "$here/ci-home.sh")"
  printf '%s\n' "$h"
}

slot() {
  local s="${PADDOCK_CI_SLOT:-}"
  if [ -z "$s" ] && is_linux; then s=s1; fi
  [[ "$s" =~ ^s[1-9][0-9]*$ ]] || die "this job holds no slot: run the job-isolation action before any step that uses a kept folder"
  printf '%s\n' "$s"
}

# layout <kind>: "<layout> <scope>" from the table.
layout() {
  local line
  line="$(awk -v k="$1" '$1 == k && $1 !~ /^#/ { print $2, $3; exit }' "$table")"
  [ -n "$line" ] || die "no kept folder '$1' in kept-folders.txt"
  printf '%s\n' "$line"
}

cmd_path() {
  local kind="${1:?usage: job-isolation.sh path <kind> [<name>]}" name="${2:-}" spec lay scope home rel
  spec="$(layout "$kind")"; lay="${spec% *}"; scope="${spec#* }"
  case "$lay" in *'{name}'*)
    [ -n "$name" ] || die "the kept folder '$kind' needs a name"
    case "$name" in *[!A-Za-z0-9._-]* | . | ..) die "the name '$name' may only hold letters, digits, '.', '_' and '-'" ;; esac ;;
  esac
  rel="${lay//\{name\}/$name}"
  if [ "$scope" = slot ]; then rel="${rel//\{slot\}/$(slot)}"; fi
  home="$(ci_home)"
  # One-time migration: the folder of the old layout (before slots) becomes slot s1's, so it stays warm and is not kept twice.
  if [ "$scope" = slot ] && [ "$(slot)" = s1 ] && [ ! -e "$home/$rel" ]; then
    local old="${lay//-\{slot\}/}"
    old="${old//\{name\}/$name}"
    if [ "$old" != "$rel" ] && [ -d "$home/$old" ] && [ ! -L "$home/$old" ]; then
      mkdir -p "$(dirname "$home/$rel")"
      mv "$home/$old" "$home/$rel" 2>/dev/null && echo "job-isolation: moved $old to $rel (kept folders are per slot now)" >&2
    fi
  fi
  mkdir -p "$home/$rel"
  printf '%s\n' "$home/$rel"
}

# slot_folders <home> <n>: every kept folder of slot s<n> (all names), one per line.
slot_folders() {
  local home="$1" n="$2" kind lay scope glob f
  while read -r kind lay scope; do
    case "$kind" in '' | \#*) continue ;; esac
    [ "$scope" = slot ] || continue
    glob="${lay//\{name\}/*}"; glob="${glob//\{slot\}/s$n}"
    for f in "$home"/$glob; do [ -d "$f" ] && [ ! -L "$f" ] && printf '%s\n' "$f"; done
  done < "$table"
}

# sweep <home> [<own slot index>]: removes the kept folders of idle slots above PADDOCK_CI_KEEP_SLOTS (no lock held), and of
# the caller's own slot when it is above that limit (the caller is releasing it).
sweep() {
  local home="$1" own="${2:-}" keep="${PADDOCK_CI_KEEP_SLOTS:-2}" n f max
  case "$keep" in '' | *[!0-9]*) keep=2 ;; esac
  max="${PADDOCK_CI_SLOTS:-8}"
  for ((n = keep + 1; n <= max; n++)); do
    if [ "$n" != "$own" ] && [ -e "$home/slots/s$n" ]; then continue; fi
    while IFS= read -r f; do
      [ -n "$f" ] && rm -rf "$f" && echo "job-isolation: removed $(basename "$f") (slot s$n is above the $keep kept slots)"
    done < <(slot_folders "$home" "$n")
  done
}

cmd_owns() {
  local dir="${1:-}" only="${2:-}" home real rel s kind lay scope glob
  [ -n "$dir" ] && [ -d "$dir" ] && [ ! -L "$dir" ] || return 1
  if command -v cygpath >/dev/null 2>&1; then dir="$(cygpath -u "$dir")"; fi
  s="$(slot 2>/dev/null)" || return 1
  home="$(ci_home)"
  real="$(cd "$(dirname "$dir")" 2>/dev/null && pwd -P)/$(basename "$dir")" || return 1
  case "$real" in "$home"/*) rel="${real#"$home"/}" ;; *) return 1 ;; esac
  while read -r kind lay scope; do
    case "$kind" in '' | \#*) continue ;; esac
    [ "$scope" = slot ] || continue
    [ -z "$only" ] || [ "$kind" = "$only" ] || continue
    [ "${rel//[!\/]/}" = "${lay//[!\/]/}" ] || continue
    glob="${lay//\{slot\}/$s}"; glob="${glob//\{name\}/?*}"
    # shellcheck disable=SC2053  # the layout is a glob on purpose
    if [[ "$rel" == $glob ]]; then return 0; fi
  done < "$table"
  return 1
}

# A path as a pkill -f regex: every separator matches / and \, a Windows drive (C: or /c) is left out, metacharacters escaped.
path_regex() {
  local p="$1" out="" c i
  if [ "${RUNNER_OS:-}" = Windows ]; then
    case "$p" in [A-Za-z]:*) p="${p:2}" ;; /[A-Za-z]/*) p="${p:2}" ;; esac
  fi
  for ((i = 0; i < ${#p}; i++)); do
    c="${p:i:1}"
    case "$c" in
      / | \\) out+='[/\\]' ;;
      [].[*+?\(\){}\|^\$]) out+="\\$c" ;;
      *) out+="$c" ;;
    esac
  done
  printf '%s' "$out"
}

cmd_pattern() {
  local pat="${1:-}" scoped=0 v
  case "$pat" in *'{tmp}'*)
    v="${PADDOCK_JOB_TEMP:-}"; [ -n "$v" ] || return 1
    pat="${pat//\{tmp\}/$(path_regex "$v")}"; scoped=1 ;;
  esac
  case "$pat" in *'{build}'*)
    v="${PADDOCK_BUILD_FOLDER:-}"; [ -n "$v" ] || return 1
    pat="${pat//\{build\}/$(path_regex "$v")}"; scoped=1 ;;
  esac
  case "$pat" in *'{slot}'*)
    v="$(slot 2>/dev/null)" || return 1
    slot_anchored "$pat" || return 1
    # A boundary after the slot: s1 must not match s12.
    pat="${pat//\{slot\}/$v([^0-9]|\$)}"; scoped=1 ;;
  esac
  [ "$scoped" = 1 ] || return 1
  printf '%s\n' "$pat"
}

# slot_anchored <pattern>: {slot} occurs once, ends the pattern, and comes right after a slot kept folder's name: the
# layout's text before {name} (after a separator or at the start), a name, and a dash (e.g. [/\\]test-browser-spotter-{slot}).
slot_anchored() {
  local pat="$1" before kind lay scope stem
  [[ "$pat" == *'{slot}' ]] || return 1
  before="${pat%'{slot}'}"
  [[ "$before" != *'{slot}'* ]] || return 1
  while read -r kind lay scope; do
    case "$kind" in '' | \#*) continue ;; esac
    [ "$scope" = slot ] || continue
    stem="${lay%%'{name}'*}"
    stem="${stem//\//[/\\\\]}"
    if [[ "$before" == *@(/|\])"$stem"+([A-Za-z0-9._-])- ]] || [[ "$before" == "$stem"+([A-Za-z0-9._-])- ]]; then return 0; fi
  done < "$table"
  return 1
}

cmd_port() {
  local name="${1:?usage: job-isolation.sh port <name>}" line base _width stride s
  line="$(awk -v k="$name" '$1 == k && $1 !~ /^#/ { print $2, $3, $4; exit }' "$ports")"
  [ -n "$line" ] || die "no port block '$name' in ports.txt"
  read -r base _width stride <<< "$line"
  s="$(slot)"
  printf '%s\n' $((base + (${s#s} - 1) * stride))
}

# ---- slots ----

owner_field() { sed -n "s/^$2=//p" "$1/owner" 2>/dev/null | head -1; }

# stale <lock>: true when the lock's owner is gone.
stale() {
  local lock="$1" t since now max_age
  now="${PADDOCK_SLOT_NOW:-$(date +%s)}"  # the clock, overridable by the tests
  max_age="${PADDOCK_SLOT_MAX_AGE_MIN:-360}"
  if [ ! -f "$lock/owner" ]; then
    # Being written right now, or left by a job killed between mkdir and write: only the latter is old.
    [ -n "$(find "$lock" -maxdepth 0 -mmin +5 2>/dev/null)" ]
    return
  fi
  t="$(owner_field "$lock" temp)"
  since="$(owner_field "$lock" since)"
  if [ -n "$t" ] && [ ! -d "$t" ]; then return 0; fi
  case "$since" in '' | *[!0-9]*) return 0 ;; esac
  [ $((now - since)) -gt $((max_age * 60)) ]
}

cmd_acquire() {
  if is_linux; then echo "job-isolation: no slot in a Linux container (only the /sccache mount is shared; ADR 0025)"; return 0; fi
  local out="${GITHUB_ENV:?GITHUB_ENV is not set}" temp home slots max i lock away
  temp="$(job_temp)"
  [ -d "$temp" ] || die "the runner's per-job temp folder $temp does not exist"
  max="${PADDOCK_CI_SLOTS:-8}"
  case "$max" in '' | 0 | *[!0-9]*) die "PADDOCK_CI_SLOTS must be a whole number above 0 (got '$max')" ;; esac
  if [[ "${PADDOCK_CI_SLOT:-}" =~ ^s[1-9][0-9]*$ ]]; then echo "job-isolation: this job already holds $PADDOCK_CI_SLOT"; return 0; fi
  home="$(ci_home)"; slots="$home/slots"
  mkdir -p "$slots"
  for i in $(seq 1 "$max"); do
    lock="$slots/s$i"
    if ! mkdir "$lock" 2>/dev/null; then
      stale "$lock" || continue
      # Reclaim: rename it away (one reclaimer wins the rename), check the renamed lock is still a stale one (another job may
      # have reclaimed and taken the slot in between: then it goes back), and take the slot like a free one.
      away="$slots/.stale-s$i-$$"
      mv "$lock" "$away" 2>/dev/null || continue
      if ! stale "$away"; then
        if [ ! -e "$lock" ]; then mv "$away" "$lock" 2>/dev/null || true; fi
        continue
      fi
      echo "job-isolation: s$i was held by a job that is gone ($(owner_field "$away" job)); reclaimed"
      rm -rf "$away"
      mkdir "$lock" 2>/dev/null || continue
    fi
    printf 'temp=%s\njob=%s\nsince=%s\n' "$temp" "${GITHUB_RUN_ID:-?}/${GITHUB_JOB:-?}" "${PADDOCK_SLOT_NOW:-$(date +%s)}" > "$lock/.owner.$$"
    mv "$lock/.owner.$$" "$lock/owner"
    {
      echo "PADDOCK_CI_SLOT=s$i"
      if [ "$RUNNER_OS" = Windows ] && command -v cygpath >/dev/null 2>&1; then
        echo "PADDOCK_JOB_TEMP=$(cygpath -m "$temp")"
        echo "TMPDIR=$(cygpath -m "$temp")"
        echo "TMP=$(cygpath -w "$temp")"
        echo "TEMP=$(cygpath -w "$temp")"
      else
        echo "PADDOCK_JOB_TEMP=$temp"
        echo "TMPDIR=$temp"
      fi
    } >> "$out"
    echo "job-isolation: slot s$i (kept folders end in -s$i); temp folders in $temp"
    sweep "$home"
    return 0
  done
  echo "::error::job-isolation: all $max slots of this host are held by running jobs:"
  for lock in "$slots"/s*; do [ -d "$lock" ] && echo "  $(basename "$lock"): $(owner_field "$lock" job) (temp $(owner_field "$lock" temp))"; done
  echo "Set PADDOCK_CI_SLOTS on the runner above its capacity, or remove a slot folder in $slots whose job is not running."
  exit 1
}

cmd_release() {
  is_linux && return 0
  local s="${PADDOCK_CI_SLOT:-}" lock home
  [[ "$s" =~ ^s[1-9][0-9]*$ ]] || { echo "job-isolation: this job holds no slot"; return 0; }
  home="$(ci_home)" || return 0
  lock="$home/slots/$s"
  # Only this job's own lock: another job may have reclaimed it (which it never does while this job's folder exists).
  if [ "$(owner_field "$lock" temp)" != "$(job_temp)" ]; then
    echo "::warning::job-isolation: $s is not held by this job; leaving it"
    return 0
  fi
  sweep "$home" "${s#s}"
  rm -rf "$lock"
  echo "job-isolation: released $s"
}

case "${1:-}" in
  acquire) cmd_acquire ;;
  release) cmd_release || true ;;
  path) shift; cmd_path "$@" ;;
  temp) job_temp ;;
  owns) shift; cmd_owns "$@" ;;
  port) shift; cmd_port "$@" ;;
  pattern) shift; cmd_pattern "$@" ;;
  *) echo "usage: job-isolation.sh acquire|release|path <kind> [<name>]|temp|owns <folder>|port <base>|pattern <regex>" >&2; exit 2 ;;
esac
