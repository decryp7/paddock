#!/bin/sh
# Claude Code PreToolUse guard (ADR 0083): the installed paddock-dev decides; until it is installed only `./dev hooks install` passes. Shell builtins only (no git: about 10 ms a call).
d=${CLAUDE_PROJECT_DIR:-$PWD}; while [ -n "$d" ] && [ ! -e "$d/.git" ] && { [ ! -f "$d/HEAD" ] || [ ! -d "$d/objects" ]; }; do n=${d%/*}; [ "$n" = "$d" ] && n=; d=$n; done
if [ -f "$d/.git" ]; then read -r k g < "$d/.git"; case $g in /*) ;; *) g=$d/$g;; esac; c=; [ -f "$g/commondir" ] && read -r c < "$g/commondir"; case $c in "") d=$g;; /*) d=$c;; *) d=$g/$c;; esac; elif [ -d "$d/.git" ]; then d=$d/.git; fi
b=$d/paddock/hooks/paddock-dev; [ -x "$b" ] || b=$b.exe; [ -x "$b" ] && exec "$b" hook pre-tool-use
IFS= read -r j; case $j in *'"command":"./dev hooks install"'*|*'"command":"./dev hooks install --local"'*) exit 0;; esac
printf '%s\n' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"Blocked: the guard hook is not installed. Run `./dev hooks install` (a binary built from origin/main; --local for a branch build) first. In a bare repository run it from one of its worktrees. Only that command is allowed until then."}}'
