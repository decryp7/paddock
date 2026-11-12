#!/bin/bash
# PreToolUse(Bash): the ER-2 guard (RT-7), blocking.
#  1. An unguarded `cd` is refused: `cd X` followed by `;` or a newline and more commands. If the cd fails (a removed
#     worktree) the rest runs in the wrong folder, which on 2026-11-10 merged into the main checkout's `main`.
#     Allowed: `cd X && ...`, `cd X || exit 1; ...` (or `|| return`), a bare `cd X`, and the same inside `( ... )`.
#  2. A subagent's git writes in the main checkout are refused (checkout, switch, commit, merge, rebase, reset, push,
#     cherry-pick, tag, branch -D/-d/-m/-M, worktree remove/prune/move). The effective folder is `git -C`, else the
#     last `cd` target, else the hook input's cwd. Read-only git, fetch and `worktree add|list|lock` stay allowed.
#     A subagent is told from the coordinator by `agent_id` in the hook input; the coordinator is not refused here.
# Never fails closed: a parse problem allows the command and warns on stderr.
# WORKTREE_GUARD_MAIN overrides the main checkout path (tests).
exec python3 -I "$(dirname "$0")/worktree-guard.py"
