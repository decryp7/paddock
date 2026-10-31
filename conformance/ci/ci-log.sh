# CI diagnostics for bash steps (rule 56: logs explain every failure). Source it; it defines functions only.
#
#   . conformance/ci/ci-log.sh
#   ci_error "the checksum of node.tar.gz differs" "re-run once; if it persists, refresh the pin in ci-image/VERSION"
#   ci_step "clippy" cargo clippy --workspace -- -D warnings      # grouped, timed, one ::error:: naming command and exit
#   ci_skip "doc tests" "the change reaches no Rust crate"        # a skipped part, with the reason, in the summary
#   ci_summary_flush "Rust gates (dyno)"                          # one table of what ran, failed and was skipped
#
# Every failure prints exactly one `::error::` line: the cause, then "Next step:" and what to do. The prefix is assembled at
# run time, so a triager who greps a log for `::error::` finds what failed, never the echoed text of a script.
# The summary goes to $GITHUB_STEP_SUMMARY when the runner sets it, and always to the log too: whether a Gitea server renders
# job summaries depends on its version and the runner's (go-gitea/gitea#23721), the log is always there.
# Works in bash 3.2 (macOS) and bash 5 (Linux, Git Bash).

# Milliseconds since the epoch: EPOCHREALTIME in bash 5, whole seconds otherwise (macOS bash 3.2 has no sub-second clock).
_ci_now_ms() {
  if [ -n "${EPOCHREALTIME-}" ]; then
    local s=${EPOCHREALTIME%[.,]*} f=${EPOCHREALTIME#*[.,]}
    f="${f}000"
    echo $((10#$s * 1000 + 10#${f:0:3}))
  else
    echo $(($(date +%s) * 1000))
  fi
}

# "12.3 s" for a duration in milliseconds.
ci_duration() {
  local ms=$1
  printf '%d.%d s' $((ms / 1000)) $((ms % 1000 / 100))
}

# What an exit status means, in words a triager can act on: bash folds a signal into 128+n and, in Git Bash, every NTSTATUS
# crash it has no signal for into 127 (run the program through run-native.mjs to see the real code).
ci_describe_exit() {
  local code=$1 sig
  case "$code" in
    0) echo "exit 0" ;;
    126) echo "exit 126 (found but not executable: permissions or a wrong binary format)" ;;
    127) echo "exit 127 (command not found; in Git Bash also a crash whose NTSTATUS bash hides: run it through conformance/ci/run-native.mjs for the real code)" ;;
    *)
      if [ "$code" -gt 128 ] && [ "$code" -le 192 ]; then
        sig=$(kill -l $((code - 128)) 2>/dev/null || true)
        echo "exit $code (killed by signal $((code - 128))${sig:+ SIG${sig#SIG}})"
      else
        echo "exit $code"
      fi
      ;;
  esac
}

# One `::error::` line: the cause and the next step. Newlines are folded so the annotation stays one line.
ci_error() {
  local cause=$1 next=${2-}
  local msg="$cause"
  [ -n "$next" ] && msg="$msg. Next step: $next"
  msg=${msg//$'\r'/}
  msg=${msg//$'\n'/ / }
  printf '::%s::%s\n' error "$msg"
}

# Rows of the step summary, kept until ci_summary_flush.
_CI_ROWS=()
_ci_row() { _CI_ROWS+=("$1"); }

# Runs one part of a step: a log group named after the label, its duration, and on failure one ::error:: naming the exact
# command, where it ran, the exit status and how to reproduce it. Returns the command's status; never exits the shell.
ci_step() {
  local label=$1
  shift
  local cmd="$*" start code took
  echo "::group::$label"
  echo "+ $cmd"
  start=$(_ci_now_ms)
  "$@"
  code=$?
  took=$(ci_duration $(($(_ci_now_ms) - start)))
  echo "::endgroup::"
  if [ "$code" -eq 0 ]; then
    echo "ci: $label: ok in $took"
    _ci_row "| $label | ok | $took |"
  else
    echo "ci: $label: FAILED in $took, $(ci_describe_exit "$code")"
    ci_error "$label failed ($(ci_describe_exit "$code"), after $took): \`$cmd\` in $PWD" "the output above this line names what broke; reproduce with: (cd $PWD && $cmd)"
    _ci_row "| $label | **failed**, $(ci_describe_exit "$code") | $took |"
  fi
  return "$code"
}

# Records a part that did not run, with the reason (a skip is part of what the job did).
ci_skip() {
  echo "ci: $1: skipped ($2)"
  _ci_row "| $1 | skipped: $2 | |"
}

# Writes the collected rows as one table to the log and, when the runner provides it, to $GITHUB_STEP_SUMMARY.
ci_summary_flush() {
  local title=$1 row
  [ ${#_CI_ROWS[@]} -gt 0 ] || return 0
  echo "ci: summary of $title:"
  for row in "${_CI_ROWS[@]}"; do echo "ci:   $row"; done
  if [ -n "${GITHUB_STEP_SUMMARY-}" ]; then
    {
      printf '### %s\n\n| Part | Result | Time |\n|---|---|---|\n' "$title"
      for row in "${_CI_ROWS[@]}"; do echo "$row"; done
      echo
    } >> "$GITHUB_STEP_SUMMARY" 2>/dev/null || echo "ci: could not write the step summary to $GITHUB_STEP_SUMMARY (the log above has it)"
  fi
  _CI_ROWS=()
}
