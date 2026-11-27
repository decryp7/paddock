#!/usr/bin/env bash
# ADR 0120's runner move, as changed on 2026-11-25 (one daemon runner, capacity 4, GUI jobs off the Mac): each --daemon lane
# leaves the user's GUI login session and becomes a LaunchDaemon in the system domain. It runs as --user through UserName,
# starts at boot before any login and has no window server. Each --retire lane is stopped and its agent parked, unless it is
# already stopped and its agent gone from ~/Library/LaunchAgents, which is fine. A lane's program, folder and environment are
# copied from its current LaunchAgent (~/Library/LaunchAgents/<prefix><name>.plist), through plutil, into
# mac-runners/runner-daemon.plist.
#
# The USER runs this, with sudo; an agent never does (ER-14, rule 7). --dry-run needs no sudo and changes nothing: it checks,
# renders the plist into a temporary folder, shows it and prints every step it would take.
#
#   sudo bash tools/mac-runner-move.sh install  --user <name> --daemon <lane>... [--retire <lane>...] [--dry-run]
#   sudo bash tools/mac-runner-move.sh rollback --user <name> --daemon <lane>... [--retire <lane>...] [--dry-run]
#        bash tools/mac-runner-move.sh status   --user <name> --daemon <lane>... [--retire <lane>...]
#
# Options: --jobs-per-host N (PADDOCK_CI_JOBS_PER_HOST, default 4: the one runner's capacity), --verify-max SECONDS (how long a
# started lane may take to declare itself to Gitea, default 90), --wait-max SECONDS (how long install and rollback wait for a running
# job, default 3600), --label-prefix P (default net.decryptology.gitea-runner.).
#
# A running job is waited for, visibly and bounded, before anything changes (the 2026-11-26 install stopped a runner under a running job,
# left the job's helpers behind and never started the new daemon). A lane whose runner has processes of a job is told to take no new
# job (`launchctl kill SIGTERM`, the runner's own shutdown: "waiting for running jobs"); then install prints which job runs and, every
# 30 s, how long it has waited, until every job has ended or --wait-max is over. At the end of the wait it fails with the old runner
# still loaded and nothing changed on disk; it never ends a job or a helper itself (the job's own cleanup does, ER-51), and it finds
# nothing by name: the runner's pid is read from `launchctl print` of its label, and its jobs are the processes under that pid.
# Each lane is then booted out with its check for a job and its boot-out back to back (launchd starts a runner that exited, and that
# one may have taken a job): a job seen there sends install back into the same bounded wait, so a boot-out never meets a job.
# Only then does it swap and start, and verify: the new daemon must be `state = running` in `launchctl print` and have logged
# "declare successfully"; a `launchctl bootstrap` that fails is a failed lane too.
#
# install: preflight (nothing changes if a lane cannot be rendered), then per lane: the daemon plist to /Library/LaunchDaemons
# (root:wheel 0644), the old agent booted out (the runner first finishes a running job, up to its shutdown_timeout) and
# parked as <plist>.before-adr0120, the daemon bootstrapped; then each started lane must log "declare successfully" within
# --verify-max and run as --user. A lane the server no longer knows ("unregistered runner") is stopped again. Re-running
# install is safe: a parked .before-adr0120 copy is the source when it exists.
# rollback: the daemons booted out and their plists removed, every agent this tool parked (.before-adr0120) put back and
# bootstrapped. Agents parked by hand elsewhere are left where they are.
# status: read-only; where each lane's plist is, which launchd domain runs it, its state, pid, process user and last
# registration line.
#
# The daemon's environment names no throttle lock folder: throttle decides its own (the per-user state folder of the daemon's UserName, found
# through the user database; docs/dev-setup.md), so CI's `throttle mark` and the agents' heavy steps meet in one queue (ADR 0102).
#
# The runner's program is signed with the Mac CI identity (tools/mac-ci-signing.sh, identifier net.decryptology.gitea-runner) as
# the user, after the old runner stopped and before the daemon starts, so a Full Disk Access grant given once to that identity
# stays across runner updates: after replacing the runner binary, run install again. A missing identity fails the preflight
# (next: tools/mac-ci-signing.sh create); --no-sign leaves the program as it is. status shows whether it carries the identity.
#
# Tests point PADDOCK_RUNNER_MOVE_SYSROOT at a scratch folder (it prefixes /Library), PADDOCK_RUNNER_MOVE_DEV at a stand-in for
# ./dev, and put shims first on PATH.
# PADDOCK_TOOLS_OS overrides `uname -s`; PADDOCK_MAC_SIGNING replaces tools/mac-ci-signing.sh.
set -uo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/.." && pwd)
templates="$here/mac-runners"
MARK='@set-by-mac-runner-move@'
PARKED='.before-adr0120'

cmd="${1:-}"
[ $# -gt 0 ] && shift
user=""; jobs=4; prefix="net.decryptology.gitea-runner."; dry=0; verify_max=90; sign=1; wait_max=3600
RUNNER_ID='net.decryptology.gitea-runner'
daemons=(); retire=()
while [ $# -gt 0 ]; do
  case "$1" in
    --user) user="${2:?--user needs a value}"; shift ;;
    --daemon) daemons+=("${2:?--daemon needs a lane name}"); shift ;;
    --retire) retire+=("${2:?--retire needs a lane name}"); shift ;;
    --jobs-per-host) jobs="${2:?}"; shift ;;
    --verify-max) verify_max="${2:?}"; shift ;;
    --wait-max) wait_max="${2:?}"; shift ;;
    --label-prefix) prefix="${2:?}"; shift ;;
    --dry-run) dry=1 ;;
    --no-sign) sign=0 ;;
    *) echo "mac-runner-move: unknown option $1 (see the header of $0)" >&2; exit 2 ;;
  esac
  shift
done

fail() { echo "error: $1" >&2; [ -n "${2:-}" ] && echo "  next: $2" >&2; exit "${3:-1}"; }
case "$cmd" in install | rollback | uninstall | status) ;; *) fail "the command must be install, rollback or status, not '${cmd}'" "see the header of $0" 2 ;; esac
[ "$cmd" = uninstall ] && cmd=rollback
[ ${#daemons[@]} -gt 0 ] || [ ${#retire[@]} -gt 0 ] || fail "no lanes named" "pass --daemon (and --retire) with the runner names" 2
case "$jobs" in '' | *[!0-9]*) fail "--jobs-per-host must be a whole number, not '$jobs'" "" 2 ;; esac
case "$verify_max" in '' | *[!0-9]*) fail "--verify-max must be seconds, not '$verify_max'" "" 2 ;; esac
case "$wait_max" in '' | *[!0-9]*) fail "--wait-max must be seconds, not '$wait_max'" "" 2 ;; esac
poll="${PADDOCK_RUNNER_MOVE_POLL:-5}"; report="${PADDOCK_RUNNER_MOVE_REPORT:-30}" # tests make both small
[ "${PADDOCK_TOOLS_OS:-$(uname -s)}" = Darwin ] || fail "this moves macOS launchd jobs; it runs on the Mac only" "" 2

[ -n "$user" ] || user="${SUDO_USER:-$(id -un)}"
[ "$user" != root ] || fail "--user is root" "name the account the runners run as today: --user <name>" 2
uid=$(id -u "$user" 2>/dev/null) || fail "no user '$user' on this Mac" "check --user" 2
group=$(id -gn "$user" 2>/dev/null) || fail "cannot read the primary group of '$user'" "" 2
home=$(dscl . -read "/Users/$user" NFSHomeDirectory 2>/dev/null | awk '{print $2; exit}')
[ -n "$home" ] && [ -d "$home" ] || fail "cannot find the home folder of '$user'" "dscl . -read /Users/$user NFSHomeDirectory" 2

sysroot="${PADDOCK_RUNNER_MOVE_SYSROOT:-}"
daemon_dir="$sysroot/Library/LaunchDaemons"
log_dir="$sysroot/Library/Logs/gitea-runner"
agent_dir="$home/Library/LaunchAgents"

if [ "$cmd" != status ] && [ "$dry" -eq 0 ] && [ "$(id -u)" != 0 ]; then
  fail "install and rollback change /Library and the system launchd domain, so they need sudo" "run it with sudo, or add --dry-run to see the steps"
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/mac-runner-move.XXXXXX") || fail "cannot make a temporary folder" ""
trap 'rm -rf "$tmp"' EXIT

quote() { local a out=""; for a in "$@"; do out+="$(printf '%q' "$a") "; done; printf '%s' "${out% }"; }
# One changing step: printed, then run (or only printed under --dry-run).
step() {
  if [ "$dry" -eq 1 ]; then printf '  would run: %s\n' "$(quote "$@")"; return 0; fi
  printf '  + %s\n' "$(quote "$@")"
  "$@"
}

# Every lane named, one per line (bash 3.2, the one sudo finds, has no empty-array expansion under set -u).
all_lanes() { printf '%s\n' ${daemons[@]+"${daemons[@]}"} ${retire[@]+"${retire[@]}"}; }
label_of() { printf '%s%s' "$prefix" "$1"; }
live_agent() { printf '%s/%s.plist' "$agent_dir" "$(label_of "$1")"; }
daemon_plist() { printf '%s/%s.plist' "$daemon_dir" "$(label_of "$1")"; }
daemon_log() { printf '%s/%s.log' "$log_dir" "$1"; }
# The lane's original agent: the parked copy once a run has parked it, else the live one.
source_agent() {
  local live; live=$(live_agent "$1")
  if [ -f "$live$PARKED" ]; then printf '%s' "$live$PARKED"; elif [ -f "$live" ]; then printf '%s' "$live"; else return 1; fi
}
value() { plutil -extract "$1" raw -o - "$2" 2>/dev/null; }
signing="${PADDOCK_MAC_SIGNING:-$here/mac-ci-signing.sh}"
# tools/mac-ci-signing.sh as the user (the identity is in the user's keychain), with its exit code.
as_user_signing() {
  if [ "$(id -u)" = "$uid" ]; then bash "$signing" "$@"; else sudo -u "$user" env HOME="$home" bash "$signing" "$@"; fi
}
# A lane's runner program, from its original agent.
program_of() { local src; src=$(source_agent "$1") && value ProgramArguments.0 "$src"; }
loaded() { launchctl print "$1/$2" >/dev/null 2>&1; }

# Renders a lane's daemon plist from the template into $2. $1 = lane name.
render() {
  local name="$1" out="$2" src program workdir
  src=$(source_agent "$name") || { echo "no LaunchAgent for lane $name: $(live_agent "$name") (nor its $PARKED copy)"; return 1; }
  program=$(value ProgramArguments.0 "$src") || { echo "$src has no ProgramArguments"; return 1; }
  [ -x "$program" ] || { echo "the runner program $program (from $src) is missing or not executable"; return 1; }
  workdir=$(value WorkingDirectory "$src") || workdir=$(dirname "$program")
  cp "$templates/runner-daemon.plist" "$out" || return 1
  local env_xml
  env_xml=$(plutil -extract EnvironmentVariables xml1 -o - "$src" 2>/dev/null) || env_xml='<dict/>'
  plutil -replace Label -string "$(label_of "$name")" "$out" &&
    plutil -replace ProgramArguments -xml "$(plutil -extract ProgramArguments xml1 -o - "$src")" "$out" &&
    plutil -replace WorkingDirectory -string "$workdir" "$out" &&
    plutil -replace EnvironmentVariables -xml "$env_xml" "$out" &&
    plutil -replace EnvironmentVariables.HOME -string "$home" "$out" &&
    plutil -replace EnvironmentVariables.PADDOCK_CI_JOBS_PER_HOST -string "$jobs" "$out" || { echo "plutil could not fill $out"; return 1; }
  # A daemon gets no per-user launchd environment, so USER, LOGNAME and the user's own temp folder are set here.
  local user_tmp
  if [ "$(id -u)" = "$uid" ]; then user_tmp=$(getconf DARWIN_USER_TEMP_DIR); else user_tmp=$(sudo -u "$user" getconf DARWIN_USER_TEMP_DIR); fi
  [ -n "$user_tmp" ] || { echo "cannot read $user's temp folder (getconf DARWIN_USER_TEMP_DIR)"; return 1; }
  plutil -replace UserName -string "$user" "$out" &&
    plutil -replace GroupName -string "$group" "$out" &&
    plutil -replace EnvironmentVariables.USER -string "$user" "$out" &&
    plutil -replace EnvironmentVariables.LOGNAME -string "$user" "$out" &&
    plutil -replace EnvironmentVariables.TMPDIR -string "$user_tmp" "$out" &&
    plutil -replace StandardOutPath -string "$(daemon_log "$name")" "$out" &&
    plutil -replace StandardErrorPath -string "$(daemon_log "$name")" "$out" || { echo "plutil could not fill $out"; return 1; }
  if grep -q "$MARK" "$out"; then echo "$out still has a $MARK value"; return 1; fi
  plutil -lint "$out" >/dev/null || { echo "plutil -lint rejects $out"; return 1; }
}

# The address of the Gitea server a lane registered with, from its .runner file (JSON, read with plutil); never the token.
server_of() {
  local workdir="$1"
  value address "$workdir/.runner" || printf '%s' '<the Gitea URL>'
}

# Waits, at most --verify-max seconds, until the lane's log shows it declared itself to Gitea after `offset`. Prints one
# result line and returns 0 (declared), 3 (the server does not know the runner) or 1 (not running or no answer in time).
verify_lane() {
  local domain="$1" name="$2" log="$3" offset="$4" label state pid who text
  label=$(label_of "$name")
  local deadline=$((SECONDS + verify_max))
  while :; do
    state=""; pid=""
    if launchctl print "$domain/$label" >"$tmp/print" 2>&1; then
      state=$(awk -F'= ' '/^\tstate/ {print $2; exit}' "$tmp/print")
      pid=$(awk -F'= ' '/^\tpid/ {print $2; exit}' "$tmp/print")
    fi
    text=$(tail -c "+$((offset + 1))" "$log" 2>/dev/null)
    case "$text" in
      *"declare successfully"*)
        if [ -n "$state" ] && [ "$state" != running ]; then
          echo "  fail  $name ($domain): the log says it declared itself, but launchd's state is '$state', not running"
          echo "        next: launchctl print $domain/$label; tail -50 $log"; return 1
        fi
        who=""; [ -n "$pid" ] && who=$(ps -o user= -p "$pid" 2>/dev/null | tr -d ' ')
        if [ -n "$who" ] && [ "$who" != "$user" ]; then
          echo "  fail  $name ($domain): runs as '$who', not '$user'"; return 1
        fi
        echo "  ok    $name ($domain): ${state:-?}, pid ${pid:-?}, user ${who:-?}; $(printf '%s\n' "$text" | grep 'declare successfully' | tail -1 | sed 's/.*msg="//; s/"$//')"
        return 0 ;;
      *"unregistered runner"*)
        echo "  fail  $name ($domain): Gitea answers 'unregistered runner': this runner's registration is gone on the server"
        return 3 ;;
    esac
    if [ $SECONDS -ge $deadline ]; then
      echo "  fail  $name ($domain): no 'declare successfully' in $log within ${verify_max}s (launchd state: ${state:-not loaded})"
      echo "        next: tail -50 $log; launchctl print $domain/$label"
      return 1
    fi
    sleep 2
  done
}

log_size() { if [ -f "$1" ]; then wc -c <"$1" | tr -d ' '; else echo 0; fi; }

status() {
  local name label live
  echo "Runner lanes of $user (uid $uid)"
  for name in $(all_lanes); do
    label=$(label_of "$name"); live=$(live_agent "$name")
    echo "- $name"
    [ -f "$(daemon_plist "$name")" ] && echo "    daemon plist  $(daemon_plist "$name")"
    [ -f "$live" ] && echo "    agent plist   $live"
    [ -f "$live$PARKED" ] && echo "    parked agent  $live$PARKED"
    local domain
    for domain in system "gui/$uid"; do
      if launchctl print "$domain/$label" >"$tmp/print" 2>&1; then
        local state pid who=""
        state=$(awk -F'= ' '/^\tstate/ {print $2; exit}' "$tmp/print")
        pid=$(awk -F'= ' '/^\tpid/ {print $2; exit}' "$tmp/print")
        [ -n "$pid" ] && who=$(ps -o user= -p "$pid" 2>/dev/null | tr -d ' ')
        echo "    loaded in     $domain: ${state:-?}, pid ${pid:--}, user ${who:--}"
      fi
    done
    local log=""
    if [ -f "$(daemon_plist "$name")" ]; then log=$(daemon_log "$name"); else log=$(value StandardErrorPath "$live" || true); fi
    if [ -n "$log" ] && [ -f "$log" ]; then
      echo "    last start    $(grep -E 'declare successfully|unregistered runner' "$log" | tail -1 | cut -c1-200)"
    fi
    local program out rc
    if program=$(program_of "$name"); then
      out=$(as_user_signing check --identifier "$RUNNER_ID" "$program" 2>&1); rc=$?
      case $rc in
        0) echo "    signature     the Mac CI identity ($RUNNER_ID)" ;;
        6) echo "    signature     not measured: no Mac CI identity (bash $signing create)" ;;
        *) echo "    signature     NOT the Mac CI identity: $(printf '%s' "$out" | grep -E 'has:' | sed -n 1p | sed 's/^ *//')" ;;
      esac
    fi
  done
}

# ---- preflight: everything is read and rendered before anything changes -----------------------------------------------
preflight() {
  local name problems=0 why
  echo "Preflight ($cmd, user $user, uid $uid, group $group, PADDOCK_CI_JOBS_PER_HOST=$jobs)"
  for name in $(all_lanes); do
    local live; live=$(live_agent "$name")
    if [ -f "$live" ] && [ -f "$live$PARKED" ] && [ "$cmd" = install ]; then
      echo "  fail  $name: both $live and $live$PARKED exist; launchd would load the live one at the next login"
      echo "        next: compare them, keep the right one as $live$PARKED and remove the other"
      problems=$((problems + 1))
    fi
  done
  if [ "$cmd" = install ]; then
    for name in ${daemons[@]+"${daemons[@]}"}; do
      if why=$(render "$name" "$tmp/$name.daemon.plist"); then echo "  ok    $name: daemon plist rendered"; else
        echo "  fail  $name: $why"; problems=$((problems + 1)); fi
    done
    for name in ${daemons[@]+"${daemons[@]}"}; do
      local program rc
      [ "$sign" -eq 1 ] || { echo "  warn  $name: --no-sign: the runner keeps its signature; a Full Disk Access grant to an ad-hoc runner ends with the next update"; continue; }
      program=$(program_of "$name") || continue
      as_user_signing check --identifier "$RUNNER_ID" "$program" >/dev/null 2>&1; rc=$?
      case $rc in
        0) echo "  ok    $name: $program carries the Mac CI identity" ;;
        5) echo "  ok    $name: $program will be signed with the Mac CI identity ($RUNNER_ID)" ;;
        6) echo "  fail  $name: no Mac CI signing identity for $user"
           echo "        next: as $user, without sudo: bash $signing create  (or pass --no-sign)"
           problems=$((problems + 1)) ;;
        *) echo "  fail  $name: cannot check the signature of $program (tools/mac-ci-signing.sh check exit $rc)"; problems=$((problems + 1)) ;;
      esac
    done
    for name in ${retire[@]+"${retire[@]}"}; do
      if source_agent "$name" >/dev/null; then echo "  ok    $name: agent found (to retire)"
      elif loaded "gui/$uid" "$(label_of "$name")"; then echo "  ok    $name: loaded with no plist in $agent_dir (to stop)"
      else echo "  ok    $name: already stopped and parked outside $agent_dir; nothing to do"; fi
    done
    for name in ${daemons[@]+"${daemons[@]}"}; do
      local src wd
      src=$(source_agent "$name") || continue
      wd=$(value WorkingDirectory "$src") || continue
      [ -f "$wd/.runner" ] || echo "  warn  $name: no $wd/.runner; the lane is not registered with Gitea"
    done
    # Daemons start before any login. A runner folder or work folder on an external disk is there only if macOS mounts
    # external disks without a login.
    local automount
    automount=$(defaults read "$sysroot/Library/Preferences/SystemConfiguration/autodiskmount" AutomountDisksWithoutUserLogin 2>/dev/null || true)
    if [ "$automount" = 1 ]; then echo "  ok    external disks mount without a login (AutomountDisksWithoutUserLogin = 1)"; else
      echo "  warn  external disks mount only after a login (AutomountDisksWithoutUserLogin is not 1); daemon lanes whose folders are under /Volumes wait for a login"; fi
  fi
  [ "$problems" -eq 0 ] || fail "$problems problem(s) in the preflight; nothing was changed" "fix the lines marked fail above and run it again"
}

# ---- a running job is waited for before anything changes ----------------------------------------------------------------
# The launchd domain a lane's runner is loaded in (system for the daemon, gui/<uid> for the agent); nothing when it is not loaded.
domain_of() {
  local label; label=$(label_of "$1")
  if loaded system "$label"; then echo system; elif loaded "gui/$uid" "$label"; then echo "gui/$uid"; fi
}
# The runner's pid, from `launchctl print` of its label (the one handle: a pid is never looked up by name).
runner_pid() { launchctl print "$1/$2" 2>/dev/null | awk -F'= ' '/^\tpid/ {print $2; exit}'; }
# The processes of the jobs the runner `$1` runs: everything under its pid, one `pid command` per line; none when it is idle.
job_processes() {
  ps -axo pid=,ppid=,command= 2>/dev/null | awk -v root="$1" '
    { id[NR] = $1; par[$1] = $2; $1 = $2 = ""; cmd[id[NR]] = substr($0, 3) }
    END {
      under[root] = 1
      do { grew = 0; for (i = 1; i <= NR; i++) if (!(id[i] in under) && (par[id[i]] in under)) { under[id[i]] = 1; grew = 1 } } while (grew)
      for (i = 1; i <= NR; i++) if ((id[i] in under) && id[i] != root) print id[i] " " cmd[id[i]]
    }'
}
# The last line the lane's runner logged about a task it took (what it says about the job), or nothing.
task_line() {
  local name="$1" domain="$2" log
  if [ "$domain" = system ]; then log=$(daemon_log "$name"); else log=$(value StandardErrorPath "$(live_agent "$name")") || return 0; fi
  [ -f "$log" ] && grep -E 'task [0-9]+' "$log" 2>/dev/null | tail -1 | cut -c1-200
}

# Before anything changes, every loaded lane is stopped without ending a job: its runner is told to take no new job, install waits, up
# to --wait-max, until no runner runs a job (printing which job runs and how long it has waited), and then boots each lane out. The
# check and the boot-out of a lane are back to back, and a job seen by the check (one a restarted runner took, since launchd starts a
# runner that exited) sends install back into the same bounded wait, so a boot-out never meets a job. Past --wait-max it fails with
# nothing changed: only signals were sent, no lane was booted out and no file touched.
stop_lanes() {
  local name domain label pid procs line lanes=() busy=() still stopped=" " again
  for name in $(all_lanes); do
    domain=$(domain_of "$name"); [ -n "$domain" ] || continue
    lanes+=("$name")
    label=$(label_of "$name"); pid=$(runner_pid "$domain" "$label"); procs=""
    [ -z "$pid" ] || procs=$(job_processes "$pid")
    if [ -n "$procs" ]; then
      busy+=("$name")
      echo "- $name: a job is running ($domain/$label, runner pid $pid)"
      line=$(task_line "$name" "$domain"); [ -z "$line" ] || echo "    log: $line"
      sed -n '1,5s/^/    process: /p' <<<"$procs"
    fi
    if [ "$dry" -eq 1 ]; then
      [ -z "$procs" ] || echo "  would wait for the job on $name (at most ${wait_max}s), after telling its runner to take no new job: launchctl kill SIGTERM $domain/$label"
    else
      step launchctl kill SIGTERM "$domain/$label" || fail "$name: the runner could not be told to take no new job (launchctl kill failed); nothing was changed" "launchctl print $domain/$label"
    fi
  done
  [ ${#busy[@]} -gt 0 ] || echo "No job is running on any lane."
  [ "$dry" -eq 0 ] && [ ${#lanes[@]} -gt 0 ] || return 0
  local began=$SECONDS next=$SECONDS
  while :; do
    still=()
    for name in "${lanes[@]}"; do
      case "$stopped" in *" $name "*) continue ;; esac
      domain=$(domain_of "$name"); [ -n "$domain" ] || continue
      pid=$(runner_pid "$domain" "$(label_of "$name")"); [ -n "$pid" ] || continue
      procs=$(job_processes "$pid"); [ -z "$procs" ] || still+=("$name")
    done
    if [ ${#still[@]} -gt 0 ]; then
      if [ $((SECONDS - began)) -ge "$wait_max" ]; then
        fail "${still[0]} still runs a job after ${wait_max} s; nothing was changed: no file and no launchd job, no process was ended (the runner takes no new job, finishes this one and launchd starts it again when it exits)" "wait for the job and run install again, or give it longer with --wait-max SECONDS"
      fi
      if [ "$SECONDS" -ge "$next" ]; then
        echo "  .. waiting for the job on ${still[*]} ($((SECONDS - began)) s of at most ${wait_max} s): $(sed -n 1p <<<"$procs")"
        next=$((SECONDS + report))
      fi
      sleep "$poll"; continue
    fi
    [ ${#busy[@]} -eq 0 ] || { echo "  all jobs have finished (waited $((SECONDS - began)) s); stopping the lanes"; busy=(); }
    # Every runner is idle: stop each lane now, the check and the boot-out back to back. A job seen here is waited for, not stopped.
    again=0
    for name in "${lanes[@]}"; do
      case "$stopped" in *" $name "*) continue ;; esac
      domain=$(domain_of "$name")
      if [ -n "$domain" ]; then
        label=$(label_of "$name"); pid=$(runner_pid "$domain" "$label"); procs=""
        [ -z "$pid" ] || procs=$(job_processes "$pid")
        if [ -n "$procs" ]; then
          echo "  .. a job started on $name before it could be stopped; waiting for it too"
          step launchctl kill SIGTERM "$domain/$label" || fail "$name: the runner could not be told to take no new job (launchctl kill failed); nothing was booted out" "launchctl print $domain/$label"
          again=1; break
        fi
        step launchctl bootout "$domain/$label"
      fi
      stopped="$stopped$name "
    done
    [ "$again" -eq 1 ] || return 0
  done
}

stop_agent() { # bootout the lane's agent if loaded
  local name="$1"
  if loaded "gui/$uid" "$(label_of "$name")"; then
    echo "  .. $name: stopping the agent; a running job finishes first (up to the runner's shutdown_timeout)"
    step launchctl bootout "gui/$uid/$(label_of "$name")"
  fi
}
park_agent() {
  local live; live=$(live_agent "$1")
  if [ -f "$live" ] && [ ! -f "$live$PARKED" ]; then step mv "$live" "$live$PARKED"; fi
}

register_hint() {
  local name="$1" wd src
  src=$(source_agent "$name") && wd=$(value WorkingDirectory "$src") || wd="<the lane's runner folder>"
  echo "        next: register the runner again as $user, without sudo, with a token from Gitea (Site Administration,"
  echo "              Actions, Runners, Create new runner):"
  echo "              cd $(printf '%q' "$wd") && ./gitea-runner --config config.yaml register --no-interactive --instance $(server_of "$wd") --name $name --token <token>"
  echo "              then run this install again."
}

install() {
  preflight
  echo
  echo "Jobs"
  stop_lanes
  echo
  echo "Install"
  local name failed=0 rc
  if [ ${#daemons[@]} -gt 0 ]; then
    [ -d "$log_dir" ] || step mkdir -p "$log_dir"
    for name in ${daemons[@]+"${daemons[@]}"}; do
      local log; log=$(daemon_log "$name")
      [ -f "$log" ] || step touch "$log"
      step chown "$user:$group" "$log"
    done
  fi
  for name in ${daemons[@]+"${daemons[@]}"}; do
    local target label; target=$(daemon_plist "$name"); label=$(label_of "$name")
    echo "- $name: LaunchDaemon (system domain, runs as $user)"
    [ "$dry" -eq 1 ] && { echo "  rendered plist:"; plutil -p "$tmp/$name.daemon.plist" | sed 's/^/    /'; }
    step cp "$tmp/$name.daemon.plist" "$target"
    step chown root:wheel "$target"
    step chmod 0644 "$target"
    stop_agent "$name"
    park_agent "$name"
    loaded system "$label" && step launchctl bootout "system/$label"
    if [ "$sign" -eq 1 ]; then
      local program; program=$(program_of "$name")
      if [ "$dry" -eq 1 ]; then echo "  would run (as $user): bash $signing sign --identifier $RUNNER_ID $program"
      else
        echo "  + (as $user) bash $signing sign --identifier $RUNNER_ID $program"
        as_user_signing sign --identifier "$RUNNER_ID" "$program" | sed 's/^/    /'
        [ "${PIPESTATUS[0]}" -eq 0 ] || fail "$name: the runner could not be signed; the daemon was not started" "read the lines above; then run install again (or rollback)"
      fi
    fi
    log_size "$(daemon_log "$name")" >"$tmp/offset.$name"
    step launchctl bootstrap system "$target" || echo "$?" >"$tmp/bootfail.$name"
  done
  for name in ${retire[@]+"${retire[@]}"}; do
    if ! loaded "gui/$uid" "$(label_of "$name")" && [ ! -f "$(live_agent "$name")" ]; then
      echo "- $name: retired already (not loaded, no agent in $agent_dir)"
      continue
    fi
    echo "- $name: retired (stopped, its agent parked as $PARKED)"
    stop_agent "$name"
    park_agent "$name"
  done
  if [ "$dry" -eq 1 ]; then
    echo
    echo "Verify (would wait up to ${verify_max}s per lane for 'declare successfully' in its log, and check the process user)"
    echo
    echo "Dry run: nothing was changed."
    return 0
  fi
  echo
  echo "Verify (each lane must declare itself to Gitea within ${verify_max}s)"
  for name in ${daemons[@]+"${daemons[@]}"}; do
    if [ -f "$tmp/bootfail.$name" ]; then
      echo "  fail  $name (system): launchctl bootstrap failed (exit $(cat "$tmp/bootfail.$name")); the daemon is not running"
      echo "        next: launchctl print system/$(label_of "$name"); plutil -lint $(daemon_plist "$name"); then run install again"
      failed=$((failed + 1)); continue
    fi
    verify_lane system "$name" "$(daemon_log "$name")" "$(cat "$tmp/offset.$name")"; rc=$?
    if [ $rc -eq 3 ]; then step launchctl bootout "system/$(label_of "$name")"; register_hint "$name"; fi
    [ $rc -eq 0 ] || failed=$((failed + 1))
  done
  echo
  if [ "$failed" -gt 0 ]; then
    echo "error: $failed lane(s) did not come up (lines marked fail above). "
    echo "  next: fix and run install again, or go back to the LaunchAgents: sudo bash $0 rollback $(lanes_args)"
    return 1
  fi
  echo "Done. Check later with: bash $0 status $(lanes_args)"
}

lanes_args() {
  local out="--user $user" name
  for name in ${daemons[@]+"${daemons[@]}"}; do out+=" --daemon $name"; done
  for name in ${retire[@]+"${retire[@]}"}; do out+=" --retire $name"; done
  printf '%s' "$out"
}

rollback() {
  echo "Rollback to the LaunchAgents of $user"
  echo "Jobs"
  stop_lanes
  local name restored=() failed=0 rc
  for name in ${daemons[@]+"${daemons[@]}"}; do
    local label target; label=$(label_of "$name"); target=$(daemon_plist "$name")
    echo "- $name: daemon off"
    if loaded system "$label"; then
      echo "  .. $name: stopping the daemon; a running job finishes first (up to the runner's shutdown_timeout)"
      step launchctl bootout "system/$label"
    fi
    [ -f "$target" ] && step rm "$target"
  done
  for name in $(all_lanes); do
    local live; live=$(live_agent "$name")
    if [ ! -f "$live$PARKED" ]; then
      echo "- $name: no parked agent ($live$PARKED); left as it is"
      continue
    fi
    echo "- $name: agent restored"
    stop_agent "$name"
    step mv "$live$PARKED" "$live"
    [ "$dry" -eq 1 ] || log_size "$(value StandardErrorPath "$live")" >"$tmp/offset.$name"
    step launchctl bootstrap "gui/$uid" "$live"
    restored+=("$name")
  done
  if [ "$dry" -eq 1 ]; then echo; echo "Dry run: nothing was changed."; return 0; fi
  echo
  echo "Verify"
  for name in ${restored[@]+"${restored[@]}"}; do
    verify_lane "gui/$uid" "$name" "$(value StandardErrorPath "$(live_agent "$name")")" "$(cat "$tmp/offset.$name")"; rc=$?
    if [ $rc -eq 3 ]; then
      echo "  warn  $name: restored as it was, but Gitea does not know it; register it again or retire it"
    elif [ $rc -ne 0 ]; then failed=$((failed + 1)); fi
  done
  [ "$failed" -eq 0 ] || { echo "error: $failed restored lane(s) did not come up (lines marked fail above)"; return 1; }
  echo "Rolled back. /Library/Logs/gitea-runner is left in place (logs only)."
}

case "$cmd" in
  status) status ;;
  install) install ;;
  rollback) rollback ;;
esac
