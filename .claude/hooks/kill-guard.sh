#!/bin/bash
# PreToolUse(Bash), agents only (agent_id in the input; the coordinator is not refused): the kill guard (rule 53), blocking.
# An agent ends only processes it can show are its own; a pid read from `ps`, `pgrep` or an earlier output is a hint, never an
# owner (on 2026-11-14 an integrator killed another agent's `lefthook run pre-commit` by pid; docs/lessons.md).
#  Refused: `pkill`, `killall` (by name: every agent runs the same programs); `kill` fed by xargs, `$( )`, backticks or a
#  variable set from them; `kill` of a variable the hook cannot see; `kill 0`, `kill -1` and process groups (`kill -- -PID`);
#  `kill PID` (any signal) unless the hook proves the pid: alive, a descendant of this Claude session's process (CLAUDE_PID,
#  else the nearest `claude` ancestor), and its working folder inside the agent's own worktree, which is the linked worktree
#  (a folder whose `.git` is a file) holding the command's effective folder (the last `cd` target, else the input's cwd).
#  Allowed: `kill %job`, `kill $!` (or a variable set from `$!`) in the command that started it, `kill -0` (a probe),
#  `kill -l`, the TaskStop tool (not Bash), and `./dev` tools, which kill through paddock_proc.
# THIS IS A MISTAKE GUARD, NOT A SECURITY BOUNDARY. Known gaps: pids built at run time or read from files by scripts, kills
# inside scripts or other programs, the short window between the hook's check and the kill, and an agent that works in
# another agent's worktree (`cd <theirs> && kill PID`): the hook cannot tell which worktree is whose, so it takes the folder
# the command runs in as the agent's own.
# Never fails closed: a parse problem allows the command and warns on stderr. KILL_GUARD_ROOT_PID overrides the session's
# process (tests).
# WIRING: the main checkout's local .claude/settings.json (untracked; the coordinator wires it after the train lands):
#   PreToolUse, matcher "Bash", command "$CLAUDE_PROJECT_DIR"/.claude/hooks/kill-guard.sh
exec python3 -I -B "$(dirname "$0")/kill-guard.py"
