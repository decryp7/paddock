#!/usr/bin/env bash
# Job isolation on a host runner (.gitea/actions/README.md, "Job isolation"; ADR 0025): any number of jobs may run on one
# host at once (several capacity-1 runners of a Pitcrew pool, all sharing ONE CI home) without one touching another's
# folders, processes or ports. The one way an action or workflow reaches a folder of the CI home.
#
#   job-isolation.sh acquire                 the job's slot (first bash step); exports PADDOCK_CI_SLOT, PADDOCK_CI_SLOT_TOKEN,
#                                            PADDOCK_JOB_TEMP, TMPDIR; sweeps idle slots and checks the free-disk floor
#   job-isolation.sh release                 gives the slot back (last step, if: always()); fails when the slot's holder
#                                            died during the job (its isolation was then not guaranteed)
#   job-isolation.sh status                  which slots are held, by which job folder (the stalled-holder recovery)
#   job-isolation.sh path <kind> [<name>]    a kept folder of kept-folders.txt (created, its time stamp renewed), for this slot
#   job-isolation.sh temp                    the job's own temp folder (the runner's $RUNNER_TEMP)
#   job-isolation.sh owns <folder> [<kind>]  exit 0 when <folder> is a kept folder (of that kind) of this job's slot
#   job-isolation.sh port <name>             the first port of this slot's block for <name> in ports.txt
#   job-isolation.sh pattern <regex>         a process pattern with {tmp}, {build} and {slot} filled in; exit 1 when it
#                                            names none of them (it would match other jobs' processes), or when {slot}
#                                            does not end the pattern right after a kept folder's name
#   job-isolation.sh sweep-slot [all]        (internal) deletes an idle slot's kept folders; runs only under
#                                            `dev ci slot idle-run`, which holds that slot's lock while it runs
#
# Two kinds of state:
# - Job-scoped (temp folders, the clean-up's temp sweep and process patterns): the runner's own per-job folder. The Gitea
#   runner gives every job <workdir_parent>/<16 random hex>/{hostexecutor,tmp,tool_cache,act}, sets RUNNER_TEMP to its tmp,
#   and deletes the whole folder after the job. acquire points TMPDIR (TMP and TEMP on Windows) at it.
# - Kept (kept-folders.txt): a stable absolute path per slot, so caches stay warm. A slot s<n> (n = 1..PADDOCK_CI_SLOTS) is
#   an OS file lock on <ci-home>/slots/s<n>.lock, held by a detached holder process (`./dev ci slot acquire`, ADR 0022's
#   paddock-dev) that lives until the job's RUNNER_TEMP is gone or release asks it to stop. The kernel drops the lock when
#   the holder dies, so there is no stale lock and nothing is ever reclaimed: two jobs never hold one slot.
# - Disk: a slot's kept folders are deleted only under `dev ci slot idle-run` (the slot's lock held for the whole deletion,
#   so no job can take the slot meanwhile): at acquire and release, the folders of idle slots above PADDOCK_CI_KEEP_SLOTS
#   (default: PADDOCK_CI_SLOTS, the pool's maximum), kept folders no job used for PADDOCK_CI_ORPHAN_DAYS (default 14; `path`
#   renews a folder's time stamp, so a folder no workflow names any more ages out) and, below the free-disk floor
#   PADDOCK_CI_MIN_FREE_GB (default 30), every idle slot's folders from the highest slot down. The first job in slot s1
#   renames a kept folder of the old unslotted layout to its -s1 name (a warm cache, no second copy).
#
# PADDOCK_CI_SLOTS: the most slots one host hands out (default 4); set it in every runner's environment (config.yaml
# runner.envs) to the pool's maximum number of runners on the host. Linux containers take no slot (one job per Linux host
# today; see ADR 0025 before running more): acquire and release do nothing there and everything else uses s1.
# PADDOCK_DEV: the paddock-dev program to run (tests); default the repository's ./dev wrapper, which keeps its binary in the
# shared kept folder tools/ (outside every job folder, so a Windows job folder can still be deleted while the holder runs).
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
  touch "$home/$rel" 2>/dev/null || true  # in use: the orphan sweep removes only folders no job asked for in ORPHAN_DAYS
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

# legacy_folders <home>: kept folders of the old unslotted layout (a slot layout without its -{slot}; names that end in a slot
# are the new layout's), one per line.
legacy_folders() {
  local home="$1" kind lay scope glob f
  while read -r kind lay scope; do
    case "$kind" in '' | \#*) continue ;; esac
    [ "$scope" = slot ] || continue
    glob="${lay//-\{slot\}/}"; glob="${glob//\{name\}/*}"
    for f in "$home"/$glob; do
      [ -d "$f" ] && [ ! -L "$f" ] || continue
      [[ "$(basename "$f")" =~ -s[0-9]+$ ]] && continue
      printf '%s\n' "$f"
    done
  done < "$table"
}

keep_slots() {
  local keep="${PADDOCK_CI_KEEP_SLOTS:-}"
  case "$keep" in '' | *[!0-9]*) keep="$(max_slots)" ;; esac
  printf '%s\n' "$keep"
}

max_slots() {
  local max="${PADDOCK_CI_SLOTS:-4}"
  case "$max" in '' | 0 | *[!0-9]*) die "PADDOCK_CI_SLOTS must be a whole number above 0 (got '$max')" ;; esac
  printf '%s\n' "$max"
}

# old <folder> <days>: true when the folder's time stamp is older than <days> days (path renews it on every use).
old() { [ -n "$(find "$1" -maxdepth 0 -mtime +"$2" 2>/dev/null)" ]; }

# sweep-slot [all]: the idle slot's folders above the kept slots (all of them with `all`), and its folders (plus, for s1, the
# old layout's) that no job used for ORPHAN_DAYS. Only under `dev ci slot idle-run`, which holds the slot's lock throughout:
# no job can take the slot while its folders go.
cmd_sweep_slot() {
  local s="${PADDOCK_CI_IDLE_SLOT:-}" mode="${1:-}" home n keep days f why
  [[ "$s" =~ ^s[1-9][0-9]*$ ]] || die "sweep-slot runs only under \`dev ci slot idle-run\` (which holds the slot's lock)"
  n="${s#s}"; home="$(ci_home)"; keep="$(keep_slots)"
  days="${PADDOCK_CI_ORPHAN_DAYS:-14}"
  case "$days" in '' | *[!0-9]*) days=14 ;; esac
  while IFS= read -r f; do
    [ -n "$f" ] || continue
    if [ "$mode" = all ]; then why="the disk is below its free floor"
    elif [ "$n" -gt "$keep" ]; then why="slot $s is above the $keep kept slots"
    elif old "$f" "$days"; then why="no job used it for $days days"
    else continue; fi
    rm -rf "$f" && echo "job-isolation: removed $(basename "$f") ($why)"
  done < <(slot_folders "$home" "$n"; if [ "$n" = 1 ]; then legacy_folders "$home"; fi)
}

# dev_tool <args>: paddock-dev (PADDOCK_DEV, else the repository's ./dev with its binary cached in the shared tools folder).
dev_tool() {
  if [ -n "${PADDOCK_DEV:-}" ]; then "$PADDOCK_DEV" "$@"; return; fi
  local tools repo
  tools="$(cmd_path tools)"
  repo="$(cd "$here/../.." && pwd)"
  (cd "$repo" && PADDOCK_DEV_HOME="$tools/paddock-dev" PADDOCK_DEV_TARGET_DIR="$tools/paddock-dev-target" bash ./dev "$@")
}

# native <path>: the path as the operating system's own programs take it (Windows form under Git bash).
native() { if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else printf '%s\n' "$1"; fi; }

# sweep <home> [all [<floor>]]: runs sweep-slot on every idle slot that has kept folders (the slot's lock held throughout);
# the job's own held slot is never touched (its lock is held). With `all`, from the highest slot down, stopping once the free
# disk is back above <floor>.
sweep() {
  local home="$1" mode="${2:-}" floor="${3:-}" max n
  max="$(max_slots)"
  for ((n = max; n >= 1; n--)); do
    if [ -z "$(slot_folders "$home" "$n")" ] && { [ "$n" != 1 ] || [ -z "$(legacy_folders "$home")" ]; }; then continue; fi
    dev_tool ci slot idle-run --slot "s$n" --root "$(native "$home")" -- bash "$here/job-isolation.sh" sweep-slot ${mode:+"$mode"} >/dev/null \
      || echo "::warning::job-isolation: the sweep of slot s$n failed"
    if [ -n "$floor" ] && [ "$(free_gb "$home")" -ge "$floor" ]; then return 0; fi
  done
  # Slots above the limit hold no folders the loop above did not see; a held slot's folders stay with its job.
  for ((n = max + 1; n <= max + 8; n++)); do
    [ -n "$(slot_folders "$home" "$n")" ] || continue
    dev_tool ci slot idle-run --slot "s$n" --root "$(native "$home")" -- bash "$here/job-isolation.sh" sweep-slot all >/dev/null || true
  done
}

# free_gb <folder>: whole gigabytes free on the folder's disk (PADDOCK_DF replaces df in tests).
free_gb() {
  "${PADDOCK_DF:-df}" -Pk "$1" 2>/dev/null | awk 'NR == 2 { print int($4 / 1048576) }'
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

# The env lines a slot gives the job's later steps.
export_slot() {
  local out="$1" slot="$2" token="$3" temp="$4"
  {
    echo "PADDOCK_CI_SLOT=$slot"
    echo "PADDOCK_CI_SLOT_TOKEN=$token"
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
}

cmd_acquire() {
  if is_linux; then echo "job-isolation: no slot in a Linux container (one job per Linux host; ADR 0025)"; return 0; fi
  local out="${GITHUB_ENV:?GITHUB_ENV is not set}" temp home max said code=0 slot token tries=0 wait floor free
  temp="$(job_temp)"
  [ -d "$temp" ] || die "the runner's per-job temp folder $temp does not exist"
  max="$(max_slots)"
  if [[ "${PADDOCK_CI_SLOT:-}" =~ ^s[1-9][0-9]*$ ]]; then echo "job-isolation: this job already holds $PADDOCK_CI_SLOT"; return 0; fi
  home="$(ci_home)"
  wait="${PADDOCK_SLOT_WAIT_TRIES:-20}"
  # A full house is refused (exit 4). It is real (more runners than PADDOCK_CI_SLOTS) or brief (a sweep holds an idle slot's
  # lock while it deletes that slot's folders): wait a little, then fail and name the holders.
  while :; do
    code=0
    said="$(dev_tool ci slot acquire --max "$max" --owner "$(native "$temp")" --root "$(native "$home")")" || code=$?
    if [ "$code" = 0 ] || [ "$code" != 4 ] || [ "$tries" -ge "$wait" ]; then break; fi
    tries=$((tries + 1))
    echo "job-isolation: all $max slots are held; trying again ($tries of $wait)"
    sleep "${PADDOCK_SLOT_WAIT_SECONDS:-6}"
  done
  if [ "$code" != 0 ]; then
    echo "::error::job-isolation: no slot (paddock-dev exit $code). Held slots (./dev ci slot status):"
    cmd_status || true
    echo "Set PADDOCK_CI_SLOTS on every runner of this host to the pool's maximum (it is $max), or see ADR 0025, \"Stalled holder\"."
    exit 1
  fi
  read -r slot token <<< "$said"
  [[ "$slot" =~ ^s[1-9][0-9]*$ ]] && [ -n "$token" ] || die "paddock-dev answered '$said', not a slot and a token"
  export_slot "$out" "$slot" "$token" "$temp"
  echo "job-isolation: slot $slot (kept folders end in -$slot); temp folders in $temp"
  sweep "$home"
  floor="${PADDOCK_CI_MIN_FREE_GB:-30}"
  case "$floor" in '' | *[!0-9]*) floor=30 ;; esac
  free="$(free_gb "$home")"
  if [ -n "$free" ] && [ "$free" -lt "$floor" ]; then
    echo "::warning::job-isolation: $free GB free on the CI home's disk, below the floor of $floor GB (PADDOCK_CI_MIN_FREE_GB); removing idle slots' kept folders, highest slot first"
    sweep "$home" all "$floor"
    free="$(free_gb "$home")"
    if [ "$free" -lt "$floor" ]; then
      echo "::error::job-isolation: still only $free GB free on $home's disk after removing every idle slot's kept folders (floor $floor GB). Free disk on the host, then re-run the job."
      PADDOCK_CI_SLOT="$slot" PADDOCK_CI_SLOT_TOKEN="$token" cmd_release >/dev/null || true
      exit 1
    fi
  fi
}

# held_by_us <home> <slot> <token>: the slot's lock is held and its holder is the one this job started.
held_by_us() {
  local home="$1" slot="$2" token="$3" state
  [ "$(sed -n 's/^token=//p' "$home/slots/$slot.owner" 2>/dev/null | head -1)" = "$token" ] || return 1
  state="$(dev_tool ci slot status --max "${slot#s}" --root "$(native "$home")" | awk -v s="$slot" '$1 == s { print $2 }')"
  [ "$state" = busy ]
}

cmd_release() {
  is_linux && return 0
  local s="${PADDOCK_CI_SLOT:-}" token="${PADDOCK_CI_SLOT_TOKEN:-}" home ok=1
  [[ "$s" =~ ^s[1-9][0-9]*$ ]] || { echo "job-isolation: this job holds no slot"; return 0; }
  home="$(ci_home)" || return 0
  if ! held_by_us "$home" "$s" "$token"; then
    ok=0
    echo "::error::job-isolation: the holder of $s ended before the job did (killed with the job's processes?), so another job could have taken $s meanwhile: this job's isolation was not guaranteed. See ADR 0025, \"Stalled holder\"."
  fi
  # Only this job's own holder: release honours only its token.
  dev_tool ci slot release --slot "$s" --token "$token" --root "$(native "$home")" >/dev/null \
    || echo "::warning::job-isolation: $s did not let go in time (./dev ci slot status shows its holder)"
  echo "job-isolation: released $s"
  sweep "$home"
  [ "$ok" = 1 ]
}

cmd_status() {
  local home max n state owner pid
  home="$(ci_home)"; max="$(max_slots)"
  dev_tool ci slot status --max "$max" --root "$(native "$home")" | while read -r n state; do
    owner="$(sed -n 's/^owner=//p' "$home/slots/$n.owner" 2>/dev/null | head -1)" || owner=""
    pid="$(sed -n 's/^pid=//p' "$home/slots/$n.owner" 2>/dev/null | head -1)" || pid=""
    if [ "$state" = busy ]; then echo "  $n busy: job folder ${owner:-?} (holder pid ${pid:-?})"
    else echo "  $n free"; fi
  done
}

case "${1:-}" in
  acquire) cmd_acquire ;;
  release) cmd_release ;;
  status) cmd_status ;;
  sweep-slot) shift; cmd_sweep_slot "$@" ;;
  path) shift; cmd_path "$@" ;;
  temp) job_temp ;;
  owns) shift; cmd_owns "$@" ;;
  port) shift; cmd_port "$@" ;;
  pattern) shift; cmd_pattern "$@" ;;
  *) echo "usage: job-isolation.sh acquire|release|status|path <kind> [<name>]|temp|owns <folder>|port <name>|pattern <regex>" >&2; exit 2 ;;
esac
