# 83. Agents cannot write outside their own worktree: one worktree tool with owner locks, cleanup that skips locks, a guard hook for the primary checkout and other agents' worktrees, and a probe

- Status: Accepted 2026-11-10 (user), decisions as recommended (D1-D11). The user asked for this redesign on 2026-11-10
  (ER-21). Revised after independent review rounds 1 (4 P1, 8 P2), 2 (2 P1, 4 P2) and 3 (2 P1); the round-3 fixes were
  checked before acceptance.
- Date: 2026-11-10
- Related: ER-2 / rule 2 (every agent works in its own worktree; the main checkout stays on `main` for the coordinator),
  ER-3 (no `git stash`), ER-14 (an agent's run must not change the machine), ER-21 / rule 40 (recurring bugs mean a
  missing design rule), ER-25 (roles: the `integrator` merges and pushes to shared branches in its own locked worktree;
  engineering-rules `c8a005b`, `skills/integrate/SKILL.md` step 3: "never the primary checkout"), ER-27 (a refusal says
  the cause and the next step), ER-32 (the `no-pgrep-wait` hook is the precedent for a guard hook), ER-33 / rule 71
  (a check counts only with a positive control), ER-36 (change the process, not the instance), rule 53 and ADR 0033
  (act only on what you hold or have just verified).
- Composes with: **ADR 0022** (scripts move to the Rust `./dev`), **ADR 0078** (one shared test scratch; it owns the
  scratch-folder incidents, row 6 below), the engineering-rules `scripts/worktree.sh` and its `worktree-lifecycle` skill.
- Number: 0081, 0082 and 0085 are held by live agent worktrees (`wt/adr-0081-0082`, `wt/adr-0085`); family ADRs on
  `origin` branches end at 0080 (`git ls-tree -r --name-only` over the remote branches, 2026-11-10). Rule and check
  numbers are assigned on acceptance.
- Evidence read at `origin/main` `b51c72509` and in the primary checkout's reflog and worktree list on 2026-11-10 unless a
  row names a branch. Measurements were made in a throw-away repository and shell in the session scratch folder, never
  in the real one.

## Pattern

**An agent writes into a checkout it does not own.** Whenever a command's working directory is not what the agent
believes it is (a `cd` that failed or went somewhere else, a shell that resets to the primary checkout between calls, a
worktree removed under it) or a cleanup chooses its targets by a property ("pushed and clean", "the scratch root")
instead of by ownership, git and file writes land in the primary checkout or in another agent's worktree. Git does not
care where it runs.

**The invariant that should hold.** Every agent worktree is created by one tool, locked at creation with an owner note,
and bound to the agent that claimed it; only its owner (or the coordinator after the owner has ended) unlocks it; no
cleanup removes a locked worktree; an agent's git writes and file edits resolve only into its own worktree or its own
scratch folder; merges into trains and `main` run in the integrator's locked worktree; and a probe shows the primary
checkout is on `main`, with no local-only commits and no tracked changes.

## Context: the incidents

| # | Date, source | What happened | Class | Covered by |
| --- | --- | --- | --- | --- |
| 1 | 2026-11-10, memory `cleanup-skips-live-worktrees.md` | The coordinator's d7 disk cleanup removed `/Volumes/d7sst7/dev-targets/wt/train-2` because it was "pushed and clean", while the integrator agent was still working in it. | Cleanup by property, not ownership | §1 `clean`, R4 |
| 2 | 2026-11-10, same; primary reflog `main@{2026-11-10 09:05:05}: merge 97b746958` | The integrator then ran `cd <missing worktree>; git merge ...`. The `cd` failed, the shell stayed in the primary checkout, and the merge created `e35db82cf` ("Merge fix/dyno-7855-tests into train/2026-11-09-2") on local `main`. Its `reset --hard` was refused by the permission system; the user reset `main` by hand (`main@{2026-11-10 11:00:40}: reset: moving to 9e18230c5`). | Unguarded `cd`; git write in the primary | R1, R2 |
| 3 | 2026-11-08, `docs/lessons.md:1491` | Twice a failed `cd` sent the next git command to the main checkout: once a sparse checkout and a merge, once a fast-forward of local `main`. The lesson names "a hook that refuses mutating git in the main checkout" as an ER-21 candidate, not built. | Same as 2 | R1, R2 |
| 4 | 2026-10-23, `docs/lessons.md:161`, memory `agents-own-worktrees.md` | Three Debrief agents were briefed with the main checkout's path; two edited it at once on one branch and mixed their changes. | No own worktree | R2, R5 |
| 5 | 2026-10-28, memory `reviewers-read-only.md` | Twice a reviewer ran `git checkout --detach` in the author's worktree (switched straight back; could have clobbered uncommitted work). | Write in another agent's worktree | R6 (owner binding), R7 (roles) |
| 6 | 2026-10-31 and 2026-11-08, memory `briefs-name-cleanup-paths.md`, `docs/lessons.md:1487` | Agents told to "clean up" ran `rm -rf .../scratchpad/*` on the shared scratch root and wiped every agent's logs, scripts and wait files. | Cleanup by glob, not ownership | **ADR 0078** owns it; R4 here covers only worktree paths |
| 7 | 2026-10-23 and 2026-10-28, memory `agent-hygiene-docker.md`, `docs/lessons.md:60` | A blanket `docker ps -q \| xargs docker stop` destroyed a user's container; an agent ran `pkill -f "ng serve"`, which would kill every agent's dev server. | Same shape outside git | ADR 0077 and rule 53, not here |
| 8 | 2026-10-30 06:45 (file times), primary `git status` | 96 untracked Python wheels and source archives (`torch`, `open_clip_torch`, `opencv_python`, ...) sit in the primary checkout's root. Origin not traced. | File writes in the primary | §3 probe (after the fact); residual risk |

Every fix so far is a brief sentence ("absolute paths or `git -C`, chain only with `&&`", "name your scratch folder",
"never checkout in the author's worktree") or a memory note. Briefs are read by a model under load; incident 2 came
two days after lesson 3 said what to do. Since `c8a005b` the integrate skill says merges run in the integrator's
locked worktree, never the primary; nothing enforces it yet.

## Instances: what exists today (2026-11-10)

| Item | State | Evidence |
| --- | --- | --- |
| Worktrees of `paddock` | 72 (70 under `/Volumes/d7sst7/dev-targets/wt`, 1 under `dev-targets/scratch`, the primary); 9 detached | `git worktree list --porcelain` |
| Locked worktrees | 7 of 71 linked (3 with an owner note, 4 locked without a reason) | same, `locked` lines |
| Created by the one tool | 0 of 71: no worktree has the `.worktree.env` that `worktree.sh add` writes; Paddock worktrees are made by hand with `git worktree add` | `test -f <wt>/.worktree.env` per worktree |
| Worktree tool | engineering-rules `scripts/worktree.sh` (`add`, `list`, `clean`, `disk`): no lock, no owner; `clean` removes worktrees that are merged and clean. `./dev` has no worktree command (`tools/paddock-dev/src/cli.rs`: `self`, `ci`, `wait`, `graph`). The `worktree-lifecycle` skill never mentions locks. | files read |
| Guard hooks | The primary checkout runs `.claude/hooks/no-pgrep-wait.sh` and `no-force-push.sh` from an **untracked** `.claude/settings.json`; the tested version of `no-pgrep-wait` (`.py` matcher plus `.test.sh`) lives only on the unmerged branch `origin/docs/rules-2026-11-04` (`332d9e2c5`..`4200cb816`, since 2026-11-04). The live copy is an older one-line regex. A session started in a worktree loads none of them. | `git status`, `git branch -r --contains 332d9e2c5`, `git merge-base --is-ancestor` |
| Subagent shells | An agent's Bash calls start in the primary checkout every time ("Agent threads always have their cwd reset between bash calls"), so every relative git command of an agent targets the primary unless it follows a `cd` that succeeded or uses `-C`. | Claude Code agent notes in this session |
| Primary checkout now | On `main` at `9e18230c5`, 166 commits behind `origin/main`, no local-only commits after the hand reset, 100 untracked entries (96 archives, `.claude/densepack-vault/`, `.claude/hooks/`, `.claude/settings.json`, `.serena/`) | `git status -sb`, `git status --porcelain` |

## Verified on synthetic repositories and shells (git 2.56.0, bash, scratch folder)

- `git worktree add --lock --reason "<note>" -b <branch> <path> <base>` creates and locks in one step.
- `git worktree remove --force` refuses a locked worktree ("cannot remove a locked working tree, lock reason: ...");
  only `remove -f -f` overrides. So `worktree.sh clean` (single `--force`) already skips locked worktrees, silently.
- `rm -rf <worktree>` ignores the lock: the folder is gone, `git worktree prune` keeps the locked entry, and the owner's
  shell is in a missing directory. **A lock protects only against git;** the tools and the hook must refuse the delete.
- **Empty and unset targets do not fail:** `cd ""` keeps the current directory with exit code 0, `cd $UNSET` goes to
  `$HOME` with exit code 0, and (measured by review round 1) `git -C ""` runs in the current directory. So
  `cd "$WT" && git merge x` with an empty `$WT` merges in the primary although it is chained with `&&`.
- The existing `no-pgrep-wait.sh` (shell plus Python matcher) takes about 50 ms per call on this Mac at a load average
  of 53 (50 calls, synthetic command): the new hook's budget is the same order.

Read, not measured: Claude Code's hook input carries `cwd`, `tool_name`, `tool_input` and, inside a subagent,
`agent_id` and `agent_type`; hooks from settings files also run inside subagents; a PreToolUse hook refuses with
`hookSpecificOutput.permissionDecision: "deny"` ([hooks reference](https://code.claude.com/docs/en/hooks), read
2026-11-10). WP-2 confirms `agent_id` and `agent_type` in a real subagent call before the hook relies on them.

## Decision

### 1. One worktree tool, locked with an owner note and bound to its agent

`./dev worktree` (Rust, `tools/paddock-dev`, ADR 0022) is the only way a Paddock agent worktree is made, claimed or
removed; the engineering-rules `scripts/worktree.sh` gets the same semantics for other projects (generic, ER-22). It
runs from the primary checkout (the coordinator's shell, or an agent's shell, which starts there) and finds the
repository from it with `git rev-parse --git-common-dir`.

| Command | Does |
| --- | --- |
| `./dev worktree add <name> --base origin/main [--sparse <paths>] [--detach] --owner <role>:<task>` | `git worktree add --lock --reason "owner=<role>:<task>; branch=<name>; created=<UTC>; by=<coordinator\|agent>"` under the configured root (`/Volumes/d7sst7/dev-targets/wt`), sparse cone if asked; prints path, branch, `CARGO_TARGET_DIR` and e2e port (`e2ePort` already derives from the path) as JSON. `--detach` makes a review copy at `--base` with no branch (for R7). Refuses an existing path or branch. The integrator's train worktree is `--owner integrator:<train>` (for example `integrator:train/2026-11-10-1`). |
| `./dev worktree claim <path>` | The agent that will work in the worktree runs it first (one brief line), with a literal path. The guard hook (§2, R8) checks and records the caller's `agent_id` for that worktree in the owner map before the command runs: it refuses a target it cannot resolve, a worktree that is not locked, and a worktree the map already binds to a **different** `agent_id` ("claimed by <id>; the coordinator reassigns it"). The same agent claiming again passes. A worktree an agent makes itself with `add` is claimed by that same call, under the same checks (below). |
| `./dev worktree reassign <path>` | Coordinator only (the hook refuses it to agents): clears the owner-map entry and keeps the lock, so the next agent can claim it. Used when an agent ended without releasing, or the coordinator hands a worktree on. |
| `./dev worktree drop <path>` | The owner removes a worktree it claimed itself (a reviewer's or scout's detached review copy, a scratch-only worktree): refuses a worktree with commits not on `origin` or with uncommitted changes, then unlocks, removes it and its map entry. |
| `./dev worktree release <path>` | Unlocks and removes the owner-map entry: the owner at hand-off, or the coordinator after the agent's completion notification. |
| `./dev worktree list` | Every worktree: branch, lock note, claimed `agent_id`, lock age, merged into `origin/main` (yes, no, empty), dirty, and whether a process has its current directory inside it (`lsof -a -d cwd`, a hint only). |
| `./dev worktree clean [--dry-run]` | Removes only worktrees that are **unlocked**, merged into `origin/main` (pushed is not enough), clean, and with no process inside; then their branch and build folder. Locked ones are listed with their owner note and kept. Never `-f -f`, never `rm`. |

**The owner map** is `<git-common-dir>/paddock-worktree-owners.json` (`{ "<worktree path>": { "agent_id", "agent_type",
"claimed": "<UTC>" } }`), written by the hook on an agent's `./dev worktree add|claim` call and by the tool on
`reassign`, `release` and `drop`; the hook's one side effect. Every update is a read-check-write under an exclusive
`flock` on the sidecar `paddock-worktree-owners.lock` (the hook and `./dev` take the same lock), and the write itself is
atomic (temp file and rename), so two agents claiming at once cannot both win. An entry whose command then failed is harmless: an entry counts only
while its worktree is locked. The lock note says what the worktree is for; the map says who may write in it. A lock
older than the threshold of D3 is reported, never removed automatically (rule 53: a note is a hint, the coordinator's
completion signal is the proof).

### 2. A PreToolUse guard hook: `.claude/hooks/own-worktree.sh`

One hook, matched on `Bash`, `Edit`, `Write`, `NotebookEdit` and `mcp__serena__.*` (D11), built like
`no-pgrep-wait`: a shell wrapper and a Python matcher run with `python3 -I`; quoted text and heredoc bodies removed
first; `bash -c`, `eval`, heredocs and here-strings fed to a shell scanned again. The parsing helpers move to one shared
`.claude/hooks/shellparse.py` that both hooks import.

**Terms.**
- *Primary*: `realpath(dirname(git -C "$CLAUDE_PROJECT_DIR" rev-parse --path-format=absolute --git-common-dir))`.
- *Caller*: an agent when the input has `agent_id` (with its `agent_type`), the coordinator otherwise.
- *Checkout of a path*: `git -C <dir> rev-parse --show-toplevel` of the nearest existing directory, then `realpath`,
  compared case-insensitively on macOS and Windows (case-insensitive file systems). A worktree nested inside the
  primary's folder is its own checkout, because `--show-toplevel` answers the innermost one.
- *Effective directory* of a command: start at the input `cwd`; follow each `cd`/`pushd` that is guarded (R1) and has
  a literal target; apply `git -C X` (several compose), `--git-dir`, `--work-tree`, `GIT_DIR=` and `GIT_WORK_TREE=`.
  A variable assigned a literal in a statement that **finishes before** the command (`VAR=/abs;`, `VAR=/abs &&`,
  `export VAR=/abs;`, or the assignment on an earlier line) is substituted first. A prefix assignment (`VAR=/abs cmd`)
  sets the variable only in `cmd`'s environment; the shell expands `"$VAR"` on that same line before the assignment
  takes effect, so for expansions on its own line it sets nothing (in bash, `unset WT; WT=/tmp printf '[%s]' "$WT"`
  prints `[]`; review round 3 measured `WT=/tmp git -C "$WT" rev-parse --show-toplevel` printing the primary). A target
  is **unknown** when, after that, it is empty, still contains a variable, a command substitution or a glob, or is a
  `cd` with no argument or `cd -`.
- *Git write*: a subcommand that changes a working tree, index, HEAD or a local ref: `add`, `am`, `apply` (not
  `--check`), `branch` with `-d -D -m -M -c -C -f` or a new name, `checkout`, `cherry-pick`, `clean`, `commit`, `config`
  (not `--get*`/`--list`), `merge`, `mv`, `notes`, `pull`, `rebase`, `reset`, `restore`, `revert`, `rm`,
  `sparse-checkout`, `stash`, `switch`, `tag`, `update-index`, `update-ref`, `worktree move/prune/remove/repair/unlock`.
  Reads (`status`, `log`, `diff`, `show`, `rev-parse`, `ls-files`, `worktree list`, `fetch`, `push`) pass.

**Rules.**

| # | Refuses | For | Message (ER-27: cause and next step) |
| --- | --- | --- | --- |
| R1 | An **unguarded** `cd` or `pushd`: a later command in the same list runs whether or not the `cd` succeeded. Guarded means followed by `&&`, or by `\|\| exit`/`\|\| return`, or under an active `set -e` (set earlier in the same shell and not undone by `set +e`). `;`, a newline, `&`, and `\|\|` with anything else are unguarded; so is a later `;` after a guarded chain (`cd X && a; b`: `b` runs even when the `cd` failed). A `cd` inside a subshell, loop body or function is judged within that body. | everyone | "A failed cd would run the rest in the current folder. Chain with `&&` (or `\|\| exit 1`), or use absolute paths and `git -C <worktree>`." |
| R2 | A git write whose effective directory is the primary; **or** whose effective directory is unknown while the input `cwd` is inside the primary | agents: deny. Coordinator: deny too, except `fetch`, `pull --ff-only`, `merge --ff-only`, `branch -d` of a branch merged into `origin/main`, `worktree prune`, `worktree lock` and `./dev worktree ...`, which pass (D1) | Agents: "This would change the primary checkout. Use your own worktree: `git -C <your worktree> ...`." Coordinator: "Merges and other writes run in the integrator's locked worktree; delegate to the integrator (`./dev worktree add <train> --owner integrator:<train>`)." |
| R3 | A write to the `main` branch ref from anywhere (`branch -f main`, `update-ref refs/heads/main`, `fetch <remote> x:main`, `push . x:main`) | agents | "Only the coordinator fast-forwards local main." (refs are shared by every worktree, so any worktree can move `main`) |
| R4 | A recursive delete or move whose target is, or contains, a **locked** worktree: `rm -r`, `rm -rf`, `trash`, `mv`, `find <path> -delete` or `-exec rm`, `rsync --delete` into it, `git clean -d/-x` in a parent, `git worktree remove -f -f`, `git worktree unlock` of a worktree the caller does not own. A literal target is resolved and tested for a locked worktree beneath it; an unknown target (`rm -rf "$VAR"`, `rm -rf */`) is refused when the input `cwd` or the target's literal prefix contains a locked worktree. | everyone | "<path> is locked by <owner note>. Release it with `./dev worktree release` once its owner has ended, then `./dev worktree clean`." |
| R5 | A file edit (`Edit`/`Write` `file_path`, `NotebookEdit` `notebook_path`) whose checkout is the primary | agents | "Edit the file in your own worktree." |
| R6 | **Owner binding.** A git write (effective directory known or, when unknown, the `cwd`) or a file edit whose checkout is a **locked** worktree that the owner map binds to a different `agent_id`, or that no entry binds yet | agents | "<path> belongs to <owner note> (agent <id>). Work in your own worktree; read others with `git show <rev>:<path>` or `git archive`." Unclaimed: "Run `./dev worktree claim <path>` first." |
| R7 | **Roles.** For `agent_type` `reviewer` or `scout`: any git write or file edit whose checkout is any worktree of the repository, except a worktree the owner map binds to the same agent that is **detached** (a review copy at a fixed commit, no branch). Scratch folders and scratch clones are not worktrees of the repository and pass. Such an agent removes its own review copy with `./dev worktree drop`. | reviewer, scout | "Reviewers and scouts are read-only on the repository (ER-25): read with `git show` or `git archive` into your scratch folder, or make a detached review copy of your own (`./dev worktree add <name> --base <rev> --detach --owner reviewer:<task>`)." |
| R8 | **Worktree tool calls.** An agent's raw `git worktree add` (anywhere); `./dev worktree claim` of an unknown target, of an unlocked worktree, or of one the owner map binds to a different `agent_id`; an agent's `./dev worktree add` whose target path is unknown, **already exists**, or is already in the owner map (the hook writes an entry for `add` only for a new path, so a failing `add` can never rebind an existing worktree); an agent's `./dev worktree reassign` | agents | Raw add: "Make worktrees with `./dev worktree add <name> --owner <role>:<task>` (locks and claims it)." Claim: "<path> is claimed by <id>; the coordinator reassigns it with `./dev worktree reassign`." Add of an existing path: "<path> exists (owner <id>); pick a new name." Reassign: "Only the coordinator reassigns a worktree." |

- **Fail-open stays the default** (D7): a command the matcher cannot parse passes, except R2's, R4's and R8's
  unknown-target cases, which refuse because they are exactly incidents 2 and 1 and a claim that could take over a
  worktree.
- **Malformed input** (not JSON, no `tool_input`) passes and writes one line to stderr naming the hook and the parse
  error, never the command text.
- **Logs.** Each refusal prints the rule id, the resolved directory, the checkout and the git subcommand to stderr (no
  file contents and no other command text).

**Tests:** `.claude/hooks/own-worktree.test.sh`, like `no-pgrep-wait.test.sh`, against a throw-away repository the test
builds itself (primary; one locked worktree `$WT` claimed by `agent-A`; one locked and unclaimed; one unlocked; one
nested inside the primary's folder) and synthetic JSON inputs. `cwd` is the primary unless named; "agent" is `agent-A`
with `agent_type` `implementer`. The minimum cases:

| Want | Cases |
| --- | --- |
| deny (R1) | `cd /missing; git merge x` (incident 2); `cd $WT` newline `git commit -m x`; `cd $WT & git status`; `cd $WT \|\| echo no; git merge x`; `cd $WT && a; b`; `bash -c 'cd /x; git merge y'`; `for d in a b; do (cd $d; git pull); done`; `f() { cd /x; git merge y; }; f`; `set -e; set +e; cd /x; git merge y` |
| pass (R1) | `cd $WT && git merge x`; `cd $WT \|\| exit 1; git merge x`; `set -e; cd $WT; git merge x`; `for d in a b; do (cd $d && git pull); done`; `f() { cd /x \|\| return; git merge y; }; f` |
| deny (R2) | agent `git commit -m x`; agent `git -C <primary> reset --hard`; agent `git --git-dir=<primary>/.git --work-tree=<primary> checkout x`; agent `GIT_DIR=<primary>/.git git commit`; agent `git -C "$WT" commit -m x` (unknown target, `cwd` primary); agent `cd "$WT" && git merge x`; agent `cd $UNSET && git merge x`; agent `git -C "" commit -m x`; agent `WT=/abs/own-wt git -C "$WT" commit -m x` (prefix assignment: `$WT` is still unset when expanded); coordinator `git merge x` in the primary (message names the integrator); coordinator `git reset --hard origin/main` in the primary |
| pass (R2) | agent `git status`, `git log`, `git show origin/main:x` in the primary; agent `git -C /abs/own-wt commit -m x`; agent `WT=/abs/own-wt; git -C "$WT" commit -m x`; agent `WT=/abs/own-wt && git -C "$WT" commit -m x`; agent `export WT=/abs/own-wt; cd "$WT" && git commit -m x`; agent `git -C "$WT" commit` with `cwd` its own worktree; coordinator `git fetch`, `git pull --ff-only`, `git merge --ff-only origin/main`, `git branch -d merged-branch`, `git worktree prune`, `./dev worktree add t --owner integrator:train/2026-11-10-1` in the primary |
| deny (R3) | agent `git -C /abs/own-wt branch -f main HEAD`; agent `git -C /abs/own-wt fetch origin x:main`; agent `git push . HEAD:main` |
| deny (R4) | `rm -rf <locked wt>`; `rm -rf <parent of locked wt>/*`; `rm -rf "$VAR"` with `cwd` the parent of a locked worktree; `find <parent> -name x -delete`; `rsync -a --delete src/ <locked wt>/`; `git -C <parent repo> clean -xdf` above a locked worktree; agent `git worktree unlock <locked wt of another agent>` |
| pass (R4) | `rm -rf <unlocked wt>`; `rm -rf <own scratch>/tmp`; `find <own scratch> -delete` |
| deny (R5) | agent `Write` of `<primary>/docs/x.md`; the same with the primary's path in different letter case (macOS); agent `NotebookEdit` with `notebook_path` in the primary |
| pass (R5) | agent `Write` of a file in its own worktree, in the nested worktree it owns, in its scratch folder |
| deny (R6) | `agent-B` `git -C $WT checkout --detach` (incident 5); `agent-B` `Edit` of `$WT/x.ts`; `agent-B` `git commit` in the unclaimed locked worktree |
| pass (R6) | `agent-A` `git -C $WT commit -m x`; `agent-B` `git -C $WT log`; `agent-B` `./dev worktree claim <its own new wt>` |
| deny (R7) | `reviewer` `git -C <any wt> checkout x`; `scout` `Edit` of a file in any worktree; `reviewer` `git -C <own claimed wt on a branch> commit` |
| pass (R7) | `reviewer` `git -C <wt> show rev:path`; `reviewer` `git archive rev \| tar -x -C <own scratch>`; `scout` `git -C <own scratch clone> commit`; `reviewer` `git -C <own detached review copy> checkout <rev>`; `reviewer` `./dev worktree drop <own detached review copy>` |
| deny (R8) | agent `git worktree add ../x -b x`; `agent-B` `./dev worktree claim $WT` (claimed by `agent-A`); agent `./dev worktree claim "$WT"` (unknown target); agent `./dev worktree claim <unlocked wt>`; `agent-B` `./dev worktree add <$WT's name> --owner x:y` (path exists; the owner map still says `agent-A`); agent `./dev worktree add "$NAME"` (unknown target); agent `./dev worktree reassign $WT` |
| pass (R8) | coordinator `./dev worktree reassign $WT`, then `agent-B` `./dev worktree claim $WT`; `agent-A` `./dev worktree claim $WT` again; agent `./dev worktree add <new name> --owner implementer:x` (new path, entry written); two claims of one worktree started together: exactly one passes (the `flock` test) |
| deny (D11) | agent `mcp__serena__serena_repl` (any input); agent `mcp__serena__replace_symbol_body`, `mcp__serena__create_text_file` |
| pass (D11) | agent `mcp__serena__initial_instructions`; the coordinator's Serena calls |
| pass, logged | malformed JSON; JSON without `tool_input`; a command mentioning `cd x; git merge` only in quotes or in a heredoc fed to `cat` |

The test prints the number of cases and fails when none ran (rule 71). The hook and its test land in git with the two
existing hooks and a tracked `.claude/settings.json` (WP-0), so a session started in any worktree gets them.

### 3. The probe: `./dev check primary`

Run by the coordinator at the start of a session, before and after every landing batch, and after any cleanup:

- the primary is on `main`, `HEAD` equals `refs/heads/main`, and `main` is an ancestor of `origin/main` (no local-only
  commit like `e35db82cf`);
- no tracked change, staged or not, and no sparse checkout or in-progress merge, rebase or cherry-pick;
- untracked entries outside a declared allow-list (`.claude/densepack-vault/`, `.serena/`, `node_modules`) are listed;
- ownership: every locked worktree has an owner note and (for agent work) an owner-map entry, and every owner-map entry
  names a locked worktree; both directions reported. **Agent liveness** comes from a list the coordinator passes
  (`--live <file>`, one `agent_id` per line, written from the coordinator's own agent list); without it the probe
  prints `liveness: not measured` and calls no lock stale (ER-33).

A difference prints one `::error::`-style line per item with the next step (ER-27). **Positive control:** its test
builds a throw-away repository with a stray merge on `main`, a tracked change, a locked worktree without a map entry and
a map entry without a lock, and must report all four; with no worktree list to read it reports `not measured`.

### 4. Rule text (proposed; amends rule 2 and ER-2)

"Every agent works in its own git worktree, created, locked with an owner note and claimed through the project's
worktree tool, with its own build directory and test ports; only its owner, or the coordinator after the owner ended,
unlocks it, and no cleanup removes a locked worktree. Merges into trains and `main` run in the integrator's locked
worktree. The primary checkout stays on `main` for the coordinator's fast-forwards. A guard hook refuses unguarded
`cd`s, an agent's git writes and edits outside its own worktree and scratch, and the coordinator's other git writes in
the primary; the coordinator's primary-checkout probe runs at every landing."

**Proposed ER-2 amendment (D6 (a); to be applied in the engineering-rules repository by its owner, not by this ADR).**
Today's text (`RULES.md`):

```text
- **ER-2. Every agent works in its own git worktree,** with its own build directory and its own test ports; the main checkout stays on `main` for the coordinator. Why: parallel agents overwrite each other's files and builds otherwise ([scripts/worktree.sh](scripts/worktree.sh)).
```

Proposed text:

```diff
-- **ER-2. Every agent works in its own git worktree,** with its own build directory and its own test ports; the main checkout stays on `main` for the coordinator. Why: parallel agents overwrite each other's files and builds otherwise ([scripts/worktree.sh](scripts/worktree.sh)).
+- **ER-2. Every agent works in its own git worktree,** created, locked with an owner note and claimed through the project's one worktree tool, with its own build directory and its own test ports. Only its owner, or the coordinator after the owner has ended, unlocks it; no cleanup removes a locked worktree, and "pushed and clean" is not "unused". Merges into trains and main run in the integrator's locked worktree. The main checkout stays on `main` for the coordinator's fast-forwards. A guard hook refuses an unguarded `cd` (one not followed by `&&` or `|| exit`), an agent's git writes and edits outside its own worktree and scratch folder, and the coordinator's other git writes in the main checkout; a probe of the main checkout runs at every landing. Why: parallel agents overwrite each other's files and builds otherwise, and on 2026-11-10 a cleanup removed a live agent's worktree and its `cd ...; git merge` then merged into the main checkout's `main` ([scripts/worktree.sh](scripts/worktree.sh); Paddock ADR 0083).
```

## Alternatives rejected

- **Brief sentences and memory notes** (today): incident 2 happened two days after lesson 3 said the same thing.
- **Make the primary checkout read-only** (`chmod -R a-w`, or a bare repository plus worktrees only): blocks the
  coordinator's fast-forwards and the untracked tool folders (`.claude/`, `.serena/`) that live there; a bare primary
  is a larger move of every path in briefs and tools. Worth a later look.
- **A `post-checkout`/`reference-transaction` git hook in `.git/hooks`:** it sees ref changes but not who made them,
  cannot tell an agent from the coordinator, misses file edits, and `reference-transaction` cannot refuse a working-tree
  change. The Claude Code hook knows the caller (`agent_id`, `agent_type`).
- **Only cleanup fixes:** closes incident 1, not 2 to 5 or 8.
- **Deny every `cd` in agent commands:** forces `git -C` everywhere, including tools that have no `-C` (npm, cargo); a
  guarded `cd` with a literal target is safe.
- **Owner in the lock note only:** the coordinator creates most worktrees before the agent exists, so the note cannot
  carry an `agent_id`; the claim step binds it.

## Consequences

- An agent whose `cd` fails, whose variable is empty or whose shell starts in the primary gets a refusal with the next
  step instead of a commit on local `main`; a reviewer can no longer switch an author's worktree.
- The coordinator no longer merges in the primary; every merge goes through the integrator's worktree, as the integrate
  skill already says since `c8a005b`.
- Cleanup can run while agents work; it removes less (only unlocked, merged, clean, unused). Each agent's first step is
  `./dev worktree claim`, and the coordinator releases the lock when the agent's completion notification arrives.
- Every Bash and edit call pays one more hook (about 50 ms under load, measured on the existing one; WP-2 records the
  new hook's cost in `docs/benchmarks.md`, rule 48).
- The hooks become tracked files: the untracked copies in the primary must be moved aside before the coordinator's
  fast-forward brings in the tracked ones (git refuses to overwrite untracked files).
- **Residual risk (writes the hook does not see):** non-git writes through Bash (`sed -i`, `npm install`, `pip
  download`, a redirect `> file`) in the primary or another worktree; hidden git writes behind a git alias, `gh`
  (`gh pr checkout`, `gh repo sync`), npm and husky scripts, `./dev` subcommands and other tools that run git
  themselves; MCP tools other than Serena. The probe lists their results afterwards in the primary (incident 8
  shows up as untracked entries); in other worktrees only the owner notices.

## Work packages (raw agent-days, one agent)

| WP | Content | Days |
| --- | --- | --- |
| WP-0 | Land the existing hooks (`no-pgrep-wait` from `docs/rules-2026-11-04`, `no-force-push`) and a tracked `.claude/settings.json`; extract `shellparse.py` from `no-pgrep-wait.py` with its test unchanged; a note for the coordinator on moving the untracked copies aside | 0.5 |
| WP-1 | `./dev worktree add/claim/reassign/release/drop/list/clean` with tests on throw-away repositories (lock at create, `--detach` review copies, owner map under the `flock`, clean skips locked and unmerged, no `rm`, sparse cones, JSON output) | 3 |
| WP-2 | `own-worktree` hook R1-R8, the owner-map check and write under the `flock`, literal variable resolution, the Serena allow-list, and its test (the table above, about 90 cases); confirm `agent_id` and `agent_type` in a real subagent call; record the hook's cost | 3.5 |
| WP-3 | `./dev check primary` with `--live` and its positive control; the coordinator's landing checklist and session start call it | 1 |
| WP-4 | engineering-rules: `worktree.sh add --owner` (locks), `claim`, `release`, `clean` names the locked ones it keeps and never deletes a locked path; the `worktree-lifecycle`, `delegate` and `integrate` skills (briefs: "claim your worktree first; absolute paths or `git -C`; guarded `cd` only"; the integrator's train worktree is made with `./dev worktree add <train> --owner integrator:<train>` and every merge runs there); the `reviewer` and `scout` role files (`agents/reviewer.md`, `agents/scout.md`: read-only on worktrees except a detached review copy they made and claimed, removed with `./dev worktree drop`, as D10 says); a generic copy of the hook in `templates/` with `templates/claude-settings.json`; the ER-2 amendment | 1 |
| WP-5 | Paddock: rule 2 amendment, the 2026-11-10 lessons entry (memory says it is not in `docs/lessons.md` yet), ADR index | 0.5 |
| WP-6 | Migration: lock and claim every live agent worktree through the tool, report the 4 locks without a reason and the 9 detached worktrees to the coordinator; ask the user about the 96 archives in the primary | 0.5 |
| | **Total** | **10** |

First cut (closes incidents 1 to 5): WP-0, WP-2, and WP-1 `add`, `claim`, `reassign` and `clean` = **6 days**. The rest follows in
the next process batch.

## Release impact (ER-1, ER-31)

No shipped code changes. `.claude/` is outside every app's dependency set; `tools/paddock-dev` marks the `plan` and
`tools` jobs only (AGENTS.md, affected planning). It can land while any release is in flight and needs no release gate.

## Decisions for the user

| # | Question | Options | Recommendation |
| --- | --- | --- | --- |
| D1 | The coordinator in the primary checkout | (a) Fetch, `pull --ff-only`, `merge --ff-only`, `./dev worktree ...`, `branch -d` of merged branches and `worktree prune` pass; every other git write there is **denied** with "delegate to the integrator in its worktree". (b) Other writes get `ask`. (c) The coordinator is exempt. | (a): it is what the integrate skill (`c8a005b`) and ER-25 already say; `ask` would leave incident 2 one click away. |
| D2 | Where the tool lives | (a) `./dev worktree` for Paddock, with `worktree.sh` updated to the same semantics for other projects. (b) `worktree.sh` everywhere. | (a): ADR 0022 moves Paddock's scripts to Rust; the generic script stays the reference for projects without `./dev`. |
| D3 | Unlocking and stale locks | (a) Only the tool unlocks: the owner at hand-off or the coordinator after the agent's completion notification; locks older than 7 days are reported by `list` and `check primary`, never expired automatically. (b) Locks expire after N days. | (a): an expired lock is how incident 1 happens again, just later. |
| D4 | Unguarded `cd` (R1) | (a) Denied for everyone. (b) Denied for agents, warning for the coordinator. (c) Warning only. | (a): `&&` or `\|\| exit 1` costs nothing, and the coordinator's shell persists between calls, which makes a failed `cd` worse there. |
| D5 | Agent file edits in the primary (R5) | (a) Denied. (b) `ask`. | (a): no brief asks an agent to edit the primary; memory and scratch folders are outside it. |
| D6 | Engineering rule | (a) Amend ER-2 with the text of §4. (b) A new ER-37. | (a): it is ER-2's own mechanism; a new number would split one rule. |
| D7 | Hook failure mode | (a) Refuse on a positive match, plus R2's and R4's unknown targets; anything else it cannot parse passes and is logged, and the probe catches the damage. (b) Refuse whatever it cannot parse. | (a): a false refusal of an odd but harmless command stalls agents; the two exceptions are the measured incidents. |
| D8 | Scope now | (a) All WPs (10 days). (b) The first cut (6 days) now, the rest in the next process batch. | (b). |
| D9 | Writes in another agent's worktree (incident 5) | (a) Owner binding: the hook records the claiming `agent_id` in the owner map under a `flock`, refuses a claim of a worktree another agent holds (only the coordinator's `reassign` frees it) and refuses other agents' writes in a locked worktree (R6, R8). (b) Leave incident 5 open (briefs only). | (a): it is the only check that sees which agent writes; the cost is one `claim` line per brief. |
| D10 | Read-only roles (R7) | (a) `reviewer` and `scout` get no git writes or edits in any worktree of the repository, except a detached review copy they made and claimed themselves (removed with `./dev worktree drop`); their scratch folders and scratch clones are free. (b) Owner binding only. | (a): matches ER-25's roles and incident 5's actor, and still lets a reviewer check out the commit under review in a copy of its own. |
| D11 | Serena tools for agents | (a) The hook matches `mcp__serena__.*` and denies every Serena tool to agents except a declared read-only allow-list, today only `mcp__serena__initial_instructions`; this session's Serena exposes just `initial_instructions` and `serena_repl`, and the REPL can edit, so it is denied. (b) Match only named edit tools. | (a): tool names differ between Serena setups (the REPL hides edits behind one name), and Serena's project root is the primary (memory `serena-usage.md`), so an allow-list is the only safe shape. A read-only tool joins the list after it is shown not to write. |
