#!/bin/bash
# plock: the heavy-step lock for Pitcrew agents (ER-21, rule 53). One heavy step (cargo, npm, ng, playwright, llvm-cov, xwin) at a time.
#   plock run [--high] [--no-gate] <agent-name> -- <command...>   queue, take the lock, wait for the load gate (1-minute load < 30), run, release
#   plock status                                                  show who holds the lock, the queue and the high-priority markers
# The lock is a directory that appears ATOMICALLY with its owner record inside (written in a temp dir, verified non-empty, then renamed
# into place), so an ownerless lock is never published. The owner record is 6 lines: pid, process start time as text (informational,
# for old readers), agent, epoch seconds when the record was written, cwd, masked command. Old versions read the first 3 lines.
# Liveness is locale- and timezone-free: the process start epoch is now minus `ps -o etime=`; a pid is a RECYCLED one only when that start
# epoch is later than the record's epoch (file mtime for old 3-line records) plus 2 s. If ps shows nothing, `kill -0` decides
# (EPERM counts as alive); a live pid is never called dead.
# A lock is broken ONLY by this tool, ONLY when its owner is dead/recycled, or when its owner file is EMPTY or missing and the lock dir
# is older than PLOCK_UNREADABLE_LIMIT seconds. A non-empty but unparsable owner file is never broken (status reports it).
# The break renames the dir aside, re-checks inode and owner, and renames it back if they changed. Breaks go to the breaks log.
# Fairness: normal runs take a ticket (pitcrew-heavy.ticket.<microseconds>-<pid> in the lock's parent) and are served in ticket order;
# tickets of dead processes are removed by the same identity check. --high skips the queue and goes first.
# Waiting logs one line every PLOCK_WAIT_LOG seconds (default 60) to stderr and to the activity log, saying what it waits for.
# Environment (tests): PLOCK_DIR (or PLOCK_ROOT) lock parent dir, PLOCK_POLL poll seconds, PLOCK_WAIT_LOG, PLOCK_UNREADABLE_LIMIT,
# PLOCK_TEST_OWNER_FAIL=N fails the next N owner writes, PLOCK_TEST_OWNER_EMPTY=N makes them "succeed" with an empty file, PLOCK_TEST_TICKET_FAIL=1 fails the ticket write,
# PLOCK_TEST_BEFORE_ASIDE="cmd" runs once inside a break, after the decision and before the rename (race tests).
ROOT=${PLOCK_DIR:-${PLOCK_ROOT:-/private/tmp/claude-501}}
LOCK=$ROOT/pitcrew-heavy-v2.lock
OLD=$ROOT/pitcrew-heavy.lock
LOG=$ROOT/pitcrew-lock-breaks.log        # breaks and removals of stale things only
ACT=$ROOT/pitcrew-lock-activity.log      # waits, takes, dones
POLL=${PLOCK_POLL:-5}
WAIT_LOG=${PLOCK_WAIT_LOG:-60}
UNREADABLE_LIMIT=${PLOCK_UNREADABLE_LIMIT:-120}
GATE_LOAD=30
mkdir -p "$ROOT" 2>/dev/null

nap() { perl -e 'select(undef,undef,undef,$ARGV[0])' "$1"; }
now() { date +%s; }
started() { ps -o lstart= -p "$1" 2>/dev/null | sed 's/^ *//'; }
mtime() { local m; m=$(stat -f %m "$1" 2>/dev/null); case "$m" in ''|*[!0-9]*) return 1;; esac; echo "$m"; }
inode() { stat -f %i "$1" 2>/dev/null; }
age_of() { local m; m=$(mtime "$1") || return 1; echo $(( $(now) - m )); }   # seconds, or fails when the file is gone/unreadable
age_str() { local a; a=$(age_of "$1") && a="${a}s" || a=unknown; echo "$a"; }
mins() { [ -n "$1" ] && echo "$(( ($(now) - $1) / 60 )) min" || echo "an unknown time"; }
say() { echo "plock: $*" >&2; }
die() { say "$*"; exit 1; }
blog() { echo "$(date '+%F %T') $*" >> "$LOG"; }
alog() { echo "$(date '+%F %T') $*" >> "$ACT"; }
oneline() { printf '%s' "$*" | tr '\n\r\t' '   '; }

etime_secs() { # [[dd-]hh:]mm:ss -> seconds; fails on anything else
  local r=$1 d=0 a b c
  case "$r" in ''|*[!0-9:-]*) return 1;; esac
  case "$r" in *-*) d=${r%%-*}; r=${r#*-};; esac
  IFS=: read -r a b c <<EOF
$r
EOF
  [ -n "$b" ] || return 1
  if [ -z "$c" ]; then c=$b; b=$a; a=0; fi
  echo $(( 10#${d:-0} * 86400 + 10#${a:-0} * 3600 + 10#${b:-0} * 60 + 10#${c:-0} ))
}
kill_alive() { local err; err=$(kill -0 "$1" 2>&1) && return 0; case "$err" in *ermitted*) return 0;; esac; return 1; }
pid_alive() { # pid, epoch the record was written ('' = unknown). Same pid AND not started after the record (+2 s).
  case "$1" in ''|*[!0-9]*) return 1;; esac
  local et secs s
  et=$(ps -o etime= -p "$1" 2>/dev/null); et=${et//[[:space:]]/}
  if [ -z "$et" ]; then kill_alive "$1"; return; fi          # ps blocked or pid gone: kill -0 decides
  secs=$(etime_secs "$et") || { kill_alive "$1"; return; }
  case "$2" in ''|*[!0-9]*) return 0;; esac
  s=$(( $(now) - secs ))
  [ "$s" -le $(( $2 + 2 )) ]
}

read_owner() { # [dir]: 0 ok, 1 owner file missing or empty, 2 non-empty but unparsable
  local f=${1:-$LOCK}/owner
  OWNER_PID=; OWNER_START=; OWNER_NAME=; OWNER_EPOCH=; OWNER_CWD=; OWNER_CMD=
  [ -s "$f" ] || return 1
  { IFS= read -r OWNER_PID; IFS= read -r OWNER_START; IFS= read -r OWNER_NAME; IFS= read -r OWNER_EPOCH; IFS= read -r OWNER_CWD; IFS= read -r OWNER_CMD; } 2>/dev/null < "$f"
  case "$OWNER_PID" in ''|*[!0-9]*) return 2;; esac
  case "$OWNER_EPOCH" in *[!0-9]*) OWNER_EPOCH=;; esac
  [ -n "$OWNER_EPOCH" ] || OWNER_EPOCH=$(mtime "$f")      # old 3-line record: its file mtime (may stay empty = unknown)
  return 0
}

# Words that make an argument secret. Matched as whole words (split on non-alphanumerics and camelCase), not substrings.
is_secret_word() {
  local w
  # shellcheck disable=SC2046
  for w in $(printf '%s' "$1" | sed 's/\([a-z0-9]\)\([A-Z]\)/\1 \2/g' | tr '[:upper:]' '[:lower:]' | tr -c 'a-z0-9\n' ' '); do
    case "$w" in token|password|passwd|pass|pwd|secret|key|apikey|credential|credentials|authorization|auth|bearer|pat) return 0;; esac
  done
  return 1
}
mask_cmd() { # the command line with secrets replaced by ***
  local out="" a k v tool="" login=0 pending=0 rest auth
  for a in "$@"; do   # -u / -p mean a credential only for known tools (otherwise: cargo -p crate, docker run -p 80:80)
    case "${a##*/}" in mysql|mysqldump|mariadb) [ -n "$tool" ] || tool=mysql;; docker) [ -n "$tool" ] || tool=docker;; curl) [ -n "$tool" ] || tool=curl;; login) login=1;; esac
  done
  local pw=0; { [ "$tool" = mysql ] || { [ "$tool" = docker ] && [ $login = 1 ]; }; } && pw=1
  for a in "$@"; do
    case "$a" in *://*@*) rest=${a#*://}; auth=${rest%%/*}; case "$auth" in *@*) a="${a%%://*}://***@${rest#*@}";; esac;; esac
    if [ $pending = 1 ] && [ "${a#-}" = "$a" ]; then a='***'; pending=0
    else
      pending=0
      case "$a" in
        -u|--user|--proxy-user) [ "$tool" = curl ] && pending=1;;
        -u?*) [ "$tool" = curl ] && a='-u***';;
        -p) [ $pw = 1 ] && pending=1;;
        -p?*) [ $pw = 1 ] && a='-p***';;
        *=*) k=${a%%=*}; v=${a#*=}
             if is_secret_word "$k"; then a="$k=***"
             else case "$v" in *[:\ ]*) is_secret_word "$v" && a="$k=***";; esac; fi;;     # --header="Authorization: Bearer x"
        -*) if is_secret_word "$a"; then pending=1; fi;;                                      # --token <value>
        *[:\ ]*) if is_secret_word "$a"; then case "$a" in *:) a='***'; pending=1;; *) a='***';; esac; fi;;   # "X-Api-Key: v"
      esac
    fi
    out="$out $a"
  done
  oneline "${out# }" | cut -c1-500
}

write_owner() { # file, start time as text
  local ep; ep=$(now); case "$ep" in ''|*[!0-9]*) return 1;; esac      # never a record without an epoch
  if [ "${PLOCK_TEST_OWNER_FAIL:-0}" -gt 0 ] 2>/dev/null; then PLOCK_TEST_OWNER_FAIL=$((PLOCK_TEST_OWNER_FAIL - 1)); : > "$1"; return 1; fi
  if [ "${PLOCK_TEST_OWNER_EMPTY:-0}" -gt 0 ] 2>/dev/null; then PLOCK_TEST_OWNER_EMPTY=$((PLOCK_TEST_OWNER_EMPTY - 1)); : > "$1"; return 0; fi   # a write that "succeeds" but leaves nothing
  printf '%s\n%s\n%s\n%s\n%s\n%s\n' "$$" "$2" "$NAME" "$ep" "$CWD1" "$CMDLINE" > "$1"
}
try_take() { # 0 taken, 1 somebody else holds it, 2 could not write the owner record
  local tmp="$ROOT/.plock.$$.$RANDOM"
  mkdir "$tmp" 2>/dev/null || return 2
  if ! { write_owner "$tmp/owner.tmp" "$(started $$)" && [ -s "$tmp/owner.tmp" ] && mv "$tmp/owner.tmp" "$tmp/owner" && [ -s "$tmp/owner" ]; }; then
    rm -rf "$tmp"; return 2    # never publish a lock without a readable owner record
  fi
  # rename(2) of a directory onto an existing NON-EMPTY directory fails; the lock dir is never empty
  if [ ! -e "$LOCK" ] && perl -e 'rename($ARGV[0],$ARGV[1]) or exit 1' "$tmp" "$LOCK" 2>/dev/null; then return 0; fi
  rm -rf "$tmp"; return 1
}
mv_dir() { perl -e 'rename($ARGV[0],$ARGV[1]) or exit 1' "$1" "$2" 2>/dev/null; }
break_if_stale() {
  local ino rc who why age pid0 ep0 aside same=0
  ino=$(inode "$LOCK"); [ -n "$ino" ] || return 1
  read_owner; rc=$?
  case $rc in
    0) pid_alive "$OWNER_PID" "$OWNER_EPOCH" && return 1
       who=$OWNER_NAME; pid0=$OWNER_PID; ep0=$OWNER_EPOCH
       why="pid $OWNER_PID is gone, or recycled (started after the record was written)";;
    2) return 1;;
    *) age=$(age_of "$LOCK") || return 1       # no owner file content at all: only age may speak
       [ "$age" -ge "$UNREADABLE_LIMIT" ] || return 1
       who="an unknown owner"; why="owner record empty or missing and lock ${age}s old (limit ${UNREADABLE_LIMIT}s)";;
  esac
  aside="$ROOT/.plock.break.$$.$RANDOM"
  if [ -n "${PLOCK_TEST_BEFORE_ASIDE:-}" ]; then eval "$PLOCK_TEST_BEFORE_ASIDE"; PLOCK_TEST_BEFORE_ASIDE=; fi   # test hook: change the lock after the decision, before the break
  mv_dir "$LOCK" "$aside" || return 1
  if [ "$(inode "$aside")" = "$ino" ]; then   # re-check on the dir we now own exclusively
    read_owner "$aside"; local rc2=$?
    if [ $rc = 0 ]; then [ $rc2 = 0 ] && [ "$OWNER_PID" = "$pid0" ] && [ "$OWNER_EPOCH" = "$ep0" ] && same=1
    else [ $rc2 = 1 ] && same=1; fi
  fi
  if [ $same = 1 ]; then blog "$NAME broke a stale lock of $who ($why)"; rm -rf "$aside"; return 0; fi
  if [ ! -e "$LOCK" ] && mv_dir "$aside" "$LOCK"; then return 1; fi
  blog "$NAME found the lock changed while breaking it and could not put it back; left at $aside"; return 1
}

live_high_marker() { # prints the first live marker's name; removes markers of dead processes
  local f b pid ep
  for f in "$ROOT"/pitcrew-heavy.high.*; do
    [ -e "$f" ] || continue
    b=${f##*/}; pid=${b#pitcrew-heavy.high.}; pid=${pid%%-*}; ep=
    { IFS= read -r _; IFS= read -r _; IFS= read -r ep; } 2>/dev/null < "$f"
    case "$ep" in ''|*[!0-9]*) ep=$(mtime "$f");; esac
    if pid_alive "$pid" "$ep"; then echo "${b#pitcrew-heavy.high.}"; return 0; fi
    blog "$NAME removed the stale high-priority marker $b"; rm -f -- "$f"
  done
  return 1
}
make_ticket() {
  local n ep; n=$(perl -MTime::HiRes=time -e 'printf "%016.0f", time*1000000'); ep=$(now)
  case "$n" in ''|*[!0-9]*) die "cannot make a queue ticket (no time stamp); giving up, no lock taken";; esac
  TICKET=$ROOT/pitcrew-heavy.ticket.$n-$$
  if [ -z "${PLOCK_TEST_TICKET_FAIL:-}" ] && printf '%s\n%s\n%s\n%s\n' "$$" "$(started $$)" "$NAME" "$ep" > "$TICKET.tmp" && [ -s "$TICKET.tmp" ] && mv "$TICKET.tmp" "$TICKET"; then return 0; fi
  die "cannot write a queue ticket in $ROOT (is it writable and not full?); giving up, no lock taken"
}
my_turn() { # sets QPOS QLEN QHEAD; prunes tickets of dead processes; 0 when my ticket is first
  QPOS=0; QLEN=0; QHEAD=; local f pid nm ep age
  [ -f "$TICKET" ] || make_ticket
  for f in "$ROOT"/pitcrew-heavy.ticket.*; do
    [ -e "$f" ] || continue
    case "$f" in *.tmp) age=$(age_of "$f") && [ "$age" -ge "$UNREADABLE_LIMIT" ] && rm -f -- "$f"; continue;; esac
    pid=; nm=; ep=
    { IFS= read -r pid; IFS= read -r _; IFS= read -r nm; IFS= read -r ep; } 2>/dev/null < "$f"
    case "$ep" in ''|*[!0-9]*) ep=$(mtime "$f");; esac
    if [ -z "$pid" ]; then   # unreadable ticket: only age may remove it
      if age=$(age_of "$f") && [ "$age" -ge "$UNREADABLE_LIMIT" ]; then blog "$NAME removed the unreadable ticket ${f##*/}"; rm -f -- "$f"; continue; fi
      nm='?'
    elif ! pid_alive "$pid" "$ep"; then blog "$NAME removed the stale ticket ${f##*/} of $nm (pid $pid gone or recycled)"; rm -f -- "$f"; continue; fi
    QLEN=$((QLEN + 1)); [ -n "$QHEAD" ] || QHEAD=$nm
    [ "$f" = "$TICKET" ] && QPOS=$QLEN
  done
  [ "$QPOS" = 1 ]
}

cmd_status() {
  local f
  if [ -d "$LOCK" ]; then
    read_owner; case $? in
      0) echo "held by $OWNER_NAME pid $OWNER_PID since '$OWNER_START' for $(mins "$OWNER_EPOCH") (alive: $(pid_alive "$OWNER_PID" "$OWNER_EPOCH" && echo yes || echo NO))"
         [ -n "$OWNER_CMD" ] && echo "  command: $OWNER_CMD"
         [ -n "$OWNER_CWD" ] && echo "  cwd: $OWNER_CWD";;
      1) echo "lock dir with an empty owner record (age $(age_str "$LOCK"); broken after ${UNREADABLE_LIMIT}s)";;
      *) echo "lock dir with an UNPARSABLE owner record (never broken automatically; inspect $LOCK/owner)";;
    esac
  else echo "free"; fi
  [ -d "$OLD" ] && echo "old-style lock $OLD exists (an agent still on the old script)"
  for f in "$ROOT"/pitcrew-heavy.ticket.*; do
    [ -e "$f" ] || continue; case "$f" in *.tmp) continue;; esac
    { IFS= read -r pid; IFS= read -r _; IFS= read -r nm; } 2>/dev/null < "$f"; echo "queued: $nm pid $pid"
  done
  for f in "$ROOT"/pitcrew-heavy.high.*; do [ -e "$f" ] && echo "high marker: $f"; done
  [ -f "$LOG" ] && { echo "breaks (last 5):"; tail -5 "$LOG"; }
  [ -f "$ACT" ] && { echo "activity (last 3):"; tail -3 "$ACT"; }
  return 0
}

due() { [ $(( $(now) - LAST_NOTE )) -ge "$WAIT_LOG" ]; }
note() { local t; t=$(now); say "$NAME waiting $((t - T0))s: $1"; alog "$NAME waiting $((t - T0))s: $1"; LAST_NOTE=$t; }
holder_note() {
  local rc; read_owner; rc=$?
  case $rc in
    0) echo "the lock is held by $OWNER_NAME (pid $OWNER_PID) for $(mins "$OWNER_EPOCH")${OWNER_CMD:+, running: $OWNER_CMD}";;
    1) echo "the lock dir has an empty owner record (age $(age_str "$LOCK"))";;
    *) echo "the lock dir has an unparsable owner record (not broken automatically)";;
  esac
}

cmd_run() {
  local HIGH=0 GATE=1 fails=0 rc hm MARK=
  while [ "${1#--}" != "$1" ] && [ "$1" != "--" ]; do case "$1" in --high) HIGH=1;; --no-gate) GATE=0;; *) echo "plock: unknown option $1" >&2; exit 2;; esac; shift; done
  NAME=$1; shift; [ "$1" = "--" ] && shift
  [ -n "$NAME" ] && [ $# -gt 0 ] || { echo "usage: plock run [--high] [--no-gate] <agent-name> -- <command...>" >&2; exit 2; }
  NAME=$(oneline "$NAME")
  CWD1=$(oneline "$(pwd)"); CMDLINE=$(mask_cmd "$@")
  T0=$(now); LAST_NOTE=$T0; TICKET=/nonexistent.ticket; HELD=0
  # shellcheck disable=SC2329
  cleanup() { [ -n "$MARK" ] && rm -f -- "$MARK"; rm -f -- "$TICKET" "$TICKET.tmp"
    if [ "$HELD" = 1 ] && read_owner && [ "$OWNER_PID" = "$$" ]; then rm -rf "$LOCK"; fi; }
  trap cleanup EXIT; trap 'exit 130' INT TERM
  if [ $HIGH = 1 ]; then
    MARK=$ROOT/pitcrew-heavy.high.$$-$(printf '%s' "$NAME" | tr -c 'A-Za-z0-9._-' '_')
    printf '%s\n%s\n%s\n' "$$" "$(started $$)" "$(now)" > "$MARK"
  else make_ticket; fi
  until [ "$HELD" = 1 ]; do
    if [ $HIGH = 0 ] && hm=$(live_high_marker); then due && note "a high-priority run goes first ($hm)"
    elif [ -d "$OLD" ]; then due && note "the old-style lock $OLD is held (an agent still on the old script)"
    elif [ $HIGH = 0 ] && ! my_turn; then due && note "queued, position $QPOS of $QLEN, behind $QHEAD"
    elif [ -d "$LOCK" ]; then
      break_if_stale && continue
      due && note "$(holder_note)"
    else
      try_take; rc=$?
      case $rc in
        0) HELD=1; continue;;
        2) fails=$((fails + 1)); say "$NAME could not write the lock owner record (attempt $fails of 3)"
           if [ $fails -ge 3 ]; then blog "$NAME gave up: owner record could not be written in $ROOT"; die "giving up: no lock taken (is $ROOT writable and not full?)"; fi;;
      esac
    fi
    nap "$POLL"
  done
  rm -f -- "$TICKET"
  T1=$(now)
  say "$NAME holds the lock (pid $$) at $(date +%T), waited $((T1 - T0))s, load $(sysctl -n vm.loadavg)"
  alog "$NAME took the lock after $((T1 - T0))s: $CMDLINE (cwd $CWD1)"
  if [ $GATE = 1 ]; then
    LAST_NOTE=$T1
    while [ "$(sysctl -n vm.loadavg | awk '{print int($2)}')" -ge "$GATE_LOAD" ]; do
      due && note "load gate, 1-minute load $(sysctl -n vm.loadavg | awk '{print $2}') >= $GATE_LOAD"; nap 20
    done
    say "gate ok, load $(sysctl -n vm.loadavg)"
  fi
  T2=$(now)
  "$@"; rc=$?
  T3=$(now)
  say "$NAME done rc=$rc at $(date +%T), ran $((T3 - T2))s (held $((T3 - T1))s, waited $((T1 - T0))s), releasing"
  alog "$NAME done rc=$rc, ran $((T3 - T2))s (held $((T3 - T1))s, waited $((T1 - T0))s): $CMDLINE"
  exit $rc
}
case "$1" in run) shift; cmd_run "$@";; status) cmd_status;; *) echo "usage: plock run|status" >&2; exit 2;; esac
