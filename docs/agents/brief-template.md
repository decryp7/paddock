# Brief template

Copy this file, fill every field, delete nothing, and run `./dev brief check <file>` before sending the brief. The check
only looks for the fields below and for the known-bad patterns of the misses listed under "Why"; it does not judge the
task. The shared rules every agent gets are in the engineering-rules `templates/BRIEF.md`; this template adds what a
Paddock brief has to say each time.

## Task

Role: <implementer | designer | reviewer | researcher | integrator | scout>
Agent id: <role>:<task>, for example implementer:dev-load. Run `export PADDOCK_AGENT_ID=<this id>` before the first `./dev worktree` command.
Goal: <the outcome and why, or the problem as observed. For a reviewer: facts only, no author opinion.>
Owned files: <paths. Nothing else.>

## Where to work

Worktree: <a folder under /Volumes/d8sst7/dev-targets/wt/, or "none: read-only, use git show">
Branch: <name, or "none">
Base: <origin/branch at a pinned head, for example origin/train/2026-11-11-5 at 607bb7928>
Scratch: <the agent's own subfolder, never a root: a subfolder of /Volumes/d8sst7/dev-targets/ or /Volumes/d7sst7/dev-targets/ or of the session scratchpad, for example /Volumes/d8sst7/dev-targets/scratch/<agent id>>
Worktree order: `./dev worktree add <name> --base <base> -b <branch> --owner <role>:<task>` (it locks), then `./dev worktree claim <path>`, work, push, and at the end `./dev worktree drop <path>`. Use drop, never release, and never chain a drop after a commit that can fail.
Main checkout: no git writes there, ever. Check `git rev-parse --show-toplevel` before each git command, and write `cd <dir> && git ...`, never `cd <dir>; git ...`.

## Already settled

Plan line: quote the line this task implements, with its source as path:line, and say where you read it (origin/main or a pinned head). Do not write it from memory.
> <the quoted line, word for word>
> Source: docs/adr/NNNN-name.md:NN at <sha>
Decisions: <what the user or an accepted ADR already decided, and what was ruled out>

## Checks

Gates: <the exact commands, in order: fmt, clippy on the host and through xwin, the touched tests, `./dev gates check`, commit-lint, and the conformance line if conformance is touched>
Heavy steps: through /Volumes/d8sst7/dev.decryptology.net/engineering-rules/scripts/heavy.sh, with CARGO_BUILD_JOBS=2 and a private CARGO_TARGET_DIR under /Volumes/d8sst7/dev-targets/build/.

## Rules every brief repeats

- Commit subjects are 100 characters or fewer (the commit-msg hook rejects longer ones); put the detail in the body. After a rejected commit run `git diff --cached --stat` so staged files do not ride along with the next one.
- No attribution anywhere: no Co-Authored-By, no "Generated with", no session trailers, in commits, PRs, code or docs.
- Scratch files only under /Volumes/d8sst7/dev-targets/. Never /tmp, /var/folders or the internal disk.
- Never `git stash`. Push your branch once; do not dispatch CI unless this brief says so.

## Report

Report: tip SHA; red output of each failing-first test; gates run and not run, with why; what is unverified; `Wiki notes:` (facts that changed, each with its source, or "none"). Filtered output only, no whole logs. End with a `Recurring:` line: `none`, or the class name with its earlier instances and their sources.

The reasons for each line are in docs/lessons.md.
