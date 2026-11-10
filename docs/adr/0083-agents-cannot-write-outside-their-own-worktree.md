# 83. Agents cannot write outside their own worktree: one worktree tool with owner locks, cleanup that skips locks, a primary-checkout guard hook and a probe

- Status: Proposed (2026-11-10). The user asked for this redesign on 2026-11-10 (ER-21). Needs the decisions below and an
  independent review before any build.
- Date: 2026-11-10
- Related: ER-2 / rule 2 (every agent works in its own worktree; the main checkout stays on `main` for the coordinator),
  ER-3 (no `git stash`), ER-14 (an agent's run must not change the machine), ER-21 / rule 40 (recurring bugs mean a
  missing design rule), ER-25 (roles), ER-27 (a refusal says the cause and the next step), ER-32 (waits; the
  `no-pgrep-wait` hook is the precedent for a guard hook), ER-33 / rule 71 (a check counts only with a positive control),
  ER-36 (change the process, not the instance), rule 53 and ADR 0033 (act only on what you hold or have just verified).
- Composes with: **ADR 0022** (scripts move to the Rust `./dev`), **ADR 0078** (one shared test scratch: the same "name
  the path you own" idea for scratch folders), the engineering-rules
  [`scripts/worktree.sh`](https://dev.decryptology.net/decryp7/engineering-rules) and its `worktree-lifecycle` skill.
- Number: 0081, 0082 and 0085 are held by live agent worktrees (`wt/adr-0081-0082`, `wt/adr-0085`); family ADRs on
  `origin` branches end at 0080 (`git ls-tree -r --name-only` over the remote branches, 2026-11-10). Rule and check
  numbers are assigned on acceptance.
- Evidence read at `origin/main` `b51c72509` and in the primary checkout's reflog and worktree list on 2026-11-10 unless a
  row names a branch. Measurements in "Verified on synthetic repositories" were made in a throw-away repository in the
  session scratch folder, never in the real one.

## Pattern

**An agent writes into a checkout it does not own.** Whenever a command's working directory is not what the agent
believes it is (a `cd` that failed, a shell that resets to the primary checkout between calls, a worktree removed under
it) or a cleanup chooses its targets by a property ("pushed and clean", "the scratch root") instead of by ownership,
git and file writes land in the primary checkout or in another agent's worktree. Git does not care where it runs.

**The invariant that should hold.** Every agent worktree is created by one tool, locked at creation with an owner note,
and only its owner (or the coordinator after the owner has ended) unlocks it; no cleanup removes a locked worktree; an
agent's git writes and file edits never resolve into the primary checkout; and a probe shows the primary checkout is on
`main`, with no local-only commits and no tracked changes.

## Context: the incidents

| # | Date, source | What happened | Class |
| --- | --- | --- | --- |
| 1 | 2026-11-10, memory `cleanup-skips-live-worktrees.md` | The coordinator's d7 disk cleanup removed `/Volumes/d7sst7/dev-targets/wt/train-2` because it was "pushed and clean", while the integrator agent was still working in it. | Cleanup by property, not ownership |
| 2 | 2026-11-10, same; primary reflog `main@{2026-11-10 09:05:05}: merge 97b746958` | The integrator then ran `cd <missing worktree>; git merge ...`. The `cd` failed, the shell stayed in the primary checkout, and the merge created `e35db82cf` ("Merge fix/dyno-7855-tests into train/2026-11-09-2") on local `main`. The agent's `reset --hard` was refused by the permission system; the user reset `main` by hand (`main@{2026-11-10 11:00:40}: reset: moving to 9e18230c5`). | `;` after `cd`; git write in the primary |
| 3 | 2026-11-08, `docs/lessons.md:1491` | Twice a failed `cd` sent the next git command to the main checkout: once a sparse checkout and a merge, once a fast-forward of local `main`. The lesson names "a hook that refuses mutating git in the main checkout" as an ER-21 candidate, not built. | Same as 2 |
| 4 | 2026-10-23, `docs/lessons.md:161`, memory `agents-own-worktrees.md` | Three Debrief agents were briefed with the main checkout's path; two edited it at once on one branch and mixed their changes. | No own worktree |
| 5 | 2026-10-28, memory `reviewers-read-only.md` | Twice a reviewer ran `git checkout --detach` in the author's worktree (switched straight back; could have clobbered uncommitted work). | Write in another agent's worktree |
| 6 | 2026-10-31 and 2026-11-08, memory `briefs-name-cleanup-paths.md`, `docs/lessons.md:1487` | Agents told to "clean up" ran `rm -rf .../scratchpad/*` on the shared scratch root and wiped every agent's logs, scripts and wait files. | Cleanup by glob, not ownership |
| 7 | 2026-10-23 and 2026-10-28, memory `agent-hygiene-docker.md`, `docs/lessons.md:60` | A blanket `docker ps -q \| xargs docker stop` destroyed a user's container; an agent ran `pkill -f "ng serve"`, which would kill every agent's dev server. | Same shape outside git (ADR 0077 owns it) |
| 8 | 2026-10-30 06:45 (file times), primary `git status` | 96 untracked Python wheels and source archives (`torch`, `open_clip_torch`, `opencv_python`, ...) sit in the primary checkout's root. Origin not traced. | File writes in the primary |

Every fix so far is a brief sentence ("absolute paths or `git -C`, chain only with `&&`", "name your scratch folder",
"never checkout in the author's worktree") or a memory note. Briefs are read by a model under load; incident 2 came
two days after lesson 3 said what to do.

## Instances: what exists today (2026-11-10)

| Item | State | Evidence |
| --- | --- | --- |
| Worktrees of `paddock` | 72 (70 under `/Volumes/d7sst7/dev-targets/wt`, 1 under `dev-targets/scratch`, the primary); 9 detached | `git worktree list --porcelain` |
| Locked worktrees | 7 of 71 linked (3 with an owner note, 4 locked without a reason) | same, `locked` lines |
| Created by the one tool | 0 of 71: no worktree has the `.worktree.env` that `worktree.sh add` writes; Paddock worktrees are made by hand with `git worktree add` | `test -f <wt>/.worktree.env` per worktree |
| Worktree tool | engineering-rules `scripts/worktree.sh` (`add`, `list`, `clean`, `disk`): no lock, no owner; `clean` removes worktrees that are merged and clean. `./dev` has no worktree command (`tools/paddock-dev/src/cli.rs`: `self`, `ci`, `wait`, `graph`). The `worktree-lifecycle` skill never mentions locks. | files read |
| Guard hooks | The primary checkout runs `.claude/hooks/no-pgrep-wait.sh` and `no-force-push.sh` from an **untracked** `.claude/settings.json`; the tested version of `no-pgrep-wait` (`.py` matcher plus `.test.sh`) lives only on the unmerged branch `origin/docs/rules-2026-11-04` (`332d9e2c5`..`4200cb816`, since 2026-11-04). The live copy is an older one-line regex. A session started in a worktree loads none of them. | `git status`, `git branch -r --contains 332d9e2c5`, `git merge-base --is-ancestor` |
| Subagent shells | An agent's Bash calls start in the primary checkout every time ("Agent threads always have their cwd reset between bash calls"), so every relative git command of an agent targets the primary unless it is chained after a successful `cd` or uses `-C`. | Claude Code agent notes in this session |
| Primary checkout now | On `main` at `9e18230c5`, 166 commits behind `origin/main`, no local-only commits after the hand reset, 100 untracked entries (96 archives, `.claude/densepack-vault/`, `.claude/hooks/`, `.claude/settings.json`, `.serena/`) | `git status -sb`, `git status --porcelain` |

## Verified on synthetic repositories (git 2.56.0, scratch folder)

- `git worktree add --lock --reason "<note>" -b <branch> <path> <base>` creates and locks in one step (no window between
  creating and locking).
- `git worktree remove --force` refuses a locked worktree ("cannot remove a locked working tree, lock reason: ...");
  only `remove -f -f` overrides. So `worktree.sh clean`, which uses a single `--force`, already skips locked worktrees, but
  says nothing about why.
- `rm -rf <worktree>` ignores the lock completely: the folder is gone, `git worktree prune` keeps the locked entry, and
  the owner finds its shell in a missing directory. **A lock protects only against git;** the cleanup tools and the hook
  must refuse to delete a locked path themselves.
- The existing `no-pgrep-wait.sh` (shell plus Python matcher) takes about 50 ms per call on this Mac at a load average
  of 53 (50 calls, synthetic command): the hook budget below is the same order.

Read, not measured: Claude Code's hook input carries `cwd`, `tool_name`, `tool_input` and, when the hook fires inside a
subagent, `agent_id` and `agent_type`; hooks from settings files also run inside subagents; a PreToolUse hook refuses
with `hookSpecificOutput.permissionDecision: "deny"` (or asks with `"ask"`)
([hooks reference](https://code.claude.com/docs/en/hooks), read 2026-11-10). WP-2 confirms `agent_id` in a real
subagent call before the hook relies on it.

## Decision

### 1. One worktree tool, locked with an owner note

`./dev worktree` (Rust, `tools/paddock-dev`, ADR 0022) is the only way a Paddock agent worktree is made or removed; the
engineering-rules `scripts/worktree.sh` gets the same semantics for other projects (generic, ER-22).

| Command | Does |
| --- | --- |
| `./dev worktree add <name> --base origin/main [--sparse <paths>] --owner "<role>:<task>"` | `git worktree add --lock --reason "owner=<role>:<task>; branch=<name>; created=<UTC>; by=<coordinator\|agent>"` under the configured root (`/Volumes/d7sst7/dev-targets/wt`), sparse cone if asked, prints the path, branch, `CARGO_TARGET_DIR` and e2e port (`e2ePort` already derives from the path) as JSON. Refuses an existing path or branch. |
| `./dev worktree release <path>` | Unlocks: the owner at hand-off, or the coordinator after the agent's completion notification. Prints what it unlocked. |
| `./dev worktree list` | Every worktree: branch, lock note, age of the lock, merged into `origin/main` (yes, no, empty), dirty, and whether any process has its current directory inside it (`lsof -a -d cwd`, a hint only). |
| `./dev worktree clean [--dry-run]` | Removes only worktrees that are **unlocked**, merged into `origin/main` (pushed is not enough), clean, and with no process inside; then their branch and build folder. Locked ones are listed with their owner note and kept. Never `-f -f`. |

The lock note is the declared owner. A lock older than the threshold of D3 is reported, never removed automatically.
The coordinator's agent list and the lock notes are compared on each `list`; a lock without a live agent is a question
for the coordinator, not a deletion (rule 53: a note is a hint, the coordinator's completion signal is the proof).

### 2. A PreToolUse guard hook: `.claude/hooks/own-worktree.sh`

One hook on `Bash`, `Edit`, `Write` and `NotebookEdit`, built like `no-pgrep-wait` (shell wrapper, a Python matcher run
with `python3 -I`, quoted text and heredoc bodies removed first, `bash -c`/`eval`/heredocs fed to a shell scanned again;
the parsing helpers move to one shared `.claude/hooks/shellparse.py` that both hooks import). The primary checkout is
`dirname(git -C "$CLAUDE_PROJECT_DIR" rev-parse --path-format=absolute --git-common-dir)`. The caller is an agent when
the input has `agent_id`, the coordinator otherwise.

| # | Refuses | For | Message (ER-27: cause and next step) |
| --- | --- | --- | --- |
| R1 | `cd` (or `pushd`) followed by `;` or a newline and another command | everyone | "A failed cd would run the rest in the current folder. Chain with `&&`, or use absolute paths and `git -C <worktree>`." |
| R2 | A git write whose effective directory resolves into the primary checkout | agents: deny; coordinator: `ask`, except `fetch`, `pull --ff-only`, `merge --ff-only`, `worktree add/lock/unlock/list` and `./dev worktree` | "This would change the primary checkout. Use your own worktree: `git -C <your worktree> ...`." |
| R3 | A write to the `main` branch ref from anywhere (`branch -f main`, `update-ref refs/heads/main`, `fetch <remote> x:main`, `push . x:main`) | agents | "Only the coordinator moves local main." (refs are shared by every worktree, so a worktree can move `main` too) |
| R4 | `rm -r`, `mv`, `trash` or `git worktree remove -f -f` / `unlock` of a path that is, or contains, a **locked** worktree | everyone (`unlock` of its own: see D3) | "<path> is locked by <owner note>. Release it with `./dev worktree release` once its owner has ended, then `./dev worktree clean`." |
| R5 | `Edit`, `Write`, `NotebookEdit` of a file inside the primary checkout | agents | "Edit the file in your own worktree." |

- **Effective directory.** The hook tracks the directory through `cd X &&` (a target that does not exist makes the
  chain stop, so it is not a risk) and `git -C X` (several `-C` compose), honours `--git-dir`, `--work-tree`, `GIT_DIR=`
  and `GIT_WORK_TREE=`, and starts from the input's `cwd`.
- **Git writes** are the subcommands that change a working tree, index, HEAD or a local ref: `add`, `am`, `apply`
  (not `--check`), `branch` with `-d -D -m -M -c -C -f` or a new name, `checkout`, `cherry-pick`, `clean`, `commit`,
  `config` (not `--get*`/`--list`), `merge`, `mv`, `notes`, `pull`, `rebase`, `reset`, `restore`, `revert`, `rm`,
  `sparse-checkout`, `stash`, `switch`, `tag`, `update-index`, `update-ref`, `worktree move/prune/remove/repair`. Reads
  (`status`, `log`, `diff`, `show`, `rev-parse`, `ls-files`, `worktree list`, `fetch`, `push`) pass.
- **On a parse it does not understand, it passes** (D7) and the probe of §3 catches what got through.
- **Logs.** Each refusal prints the rule id, the resolved directory and the subcommand to stderr (no file contents).

**Tests:** `.claude/hooks/own-worktree.test.sh`, like `no-pgrep-wait.test.sh`: synthetic JSON inputs against a throw-away
repository with one locked and one unlocked linked worktree made by the test itself, at least these cases:

| Want | Case |
| --- | --- |
| deny | `cd /missing; git merge x` (incident 2); `cd $WT\ngit commit -m x`; agent `git commit -m x` with `cwd` = primary; agent `git -C <primary> reset --hard`; agent `git --git-dir=<primary>/.git --work-tree=<primary> checkout x`; agent `GIT_DIR=<primary>/.git git commit`; agent `git -C $WT branch -f main HEAD`; agent `git -C $WT fetch origin x:main`; `rm -rf <locked wt>`; `rm -rf <parent of locked wt>/*`; `bash -c 'cd /x; git merge y'`; agent `Write` of `<primary>/docs/x.md` |
| ask | coordinator `git merge x` in the primary |
| pass | `cd $WT && git merge x`; agent `git -C $WT commit -m x`; agent `git status` in the primary; agent `git log` / `git show origin/main:x` in the primary; coordinator `git pull --ff-only` in the primary; `rm -rf <unlocked wt>`; `git commit -m "docs: never cd x; git merge"` (quoted); a heredoc that only mentions `cd x; git merge`; agent `Write` of a file in its own worktree or the scratchpad |

The test prints the number of cases and fails when none ran (rule 71). The hook and its test land in git with the two
existing hooks and a tracked `.claude/settings.json` (WP-0), so a session started in any worktree gets them.

### 3. The probe: `./dev check primary`

Run by the coordinator at the start of a session, before and after every landing batch, and after any cleanup:

- the primary is on `main`, `HEAD` equals `refs/heads/main`, and `main` is an ancestor of `origin/main` (no local-only
  commit like `e35db82cf`);
- no tracked change, staged or not, and no sparse-checkout or in-progress merge, rebase or cherry-pick;
- untracked entries outside a declared allow-list (`.claude/densepack-vault/`, `.serena/`, `node_modules`) are listed;
- every live agent of the coordinator's list has a locked worktree, and every locked worktree's note names one (both
  directions reported).

A difference prints one `::error::`-style line per item with the next step (ER-27). **Positive control:** its test
builds a throw-away repository with a stray merge on `main`, a tracked change and an unlocked live worktree, and must
report all three; with no worktree list to read it reports `not measured` (ER-33).

### 4. Rule text (proposed; amends rule 2 and ER-2)

"Every agent works in its own git worktree, created and locked with an owner note by the project's worktree tool, with
its own build directory and test ports; only its owner or the coordinator after the owner ended unlocks it, and no
cleanup removes a locked worktree. The primary checkout stays on `main` for the coordinator; a guard hook refuses an
agent's git writes and edits there and any `cd` chained with `;`, and the coordinator's primary-checkout probe runs at
every landing."

## Alternatives rejected

- **Brief sentences and memory notes** (today): incident 2 happened two days after lesson 3 said the same thing.
- **Make the primary checkout read-only** (`chmod -R a-w`, or a bare repository plus worktrees only): blocks the
  coordinator's own fast-forwards and the untracked tool folders (`.claude/`, `.serena/`) that live there; a bare primary
  is a larger move of every path in briefs and tools. Worth a later look (D1 (c)).
- **A `post-checkout`/`reference-transaction` git hook in `.git/hooks`:** it sees ref changes but not who made them,
  cannot tell an agent from the coordinator, misses file edits, and `reference-transaction` cannot refuse a working-tree
  change. The Claude Code hook knows the caller (`agent_id`).
- **Only cleanup fixes:** closes incident 1, not 2 to 5 or 8.
- **Deny every `cd` in agent commands:** forces `git -C` everywhere, including tools that have no `-C` (npm, cargo); `&&`
  after `cd` is safe.

## Consequences

- An agent whose `cd` fails or whose shell starts in the primary gets a refusal with the next step instead of a commit
  on local `main`.
- Cleanup can run while agents work; it removes less (only unlocked, merged, clean, unused), and the coordinator
  releases each agent's lock when its completion notification arrives (one more step per hand-off, done by the tool).
- Every Bash call pays one more hook (about 50 ms under load, measured on the existing one; WP-2 records the new
  hook's cost in `docs/benchmarks.md`, rule 48).
- The hooks become tracked files: the untracked copies in the primary must be moved aside before the coordinator's
  fast-forward brings in the tracked ones (git refuses to overwrite untracked files).
- Residual risk: non-git file writes through Bash (`sed -i`, `npm install`, `pip download`) in the primary are not parsed
  by the hook; the probe lists them afterwards (incident 8 would show up as untracked entries).

## Work packages (raw agent-days, one agent)

| WP | Content | Days |
| --- | --- | --- |
| WP-0 | Land the existing hooks (`no-pgrep-wait` from `docs/rules-2026-11-04`, `no-force-push`) and a tracked `.claude/settings.json`; extract `shellparse.py` from `no-pgrep-wait.py` with its test unchanged; a note for the coordinator on moving the untracked copies aside | 0.5 |
| WP-1 | `./dev worktree add/release/list/clean` with tests on throw-away repositories (lock at create, clean skips locked and unmerged, `rm`-free removal, sparse cones, JSON output) | 2 |
| WP-2 | `own-worktree` hook R1-R5 and its test (the table above, about 40 cases); confirm `agent_id` in a real subagent call; record the hook's cost | 1.5 |
| WP-3 | `./dev check primary` with its positive control; the coordinator's landing checklist (integrate skill) and session start call it | 1 |
| WP-4 | engineering-rules: `worktree.sh add --owner` (locks), `release`, `clean` names the locked ones it keeps and never deletes a locked path; the `worktree-lifecycle` and `delegate` skills (briefs: "created by the tool, locked; absolute paths or `git -C`; `&&` only"); a generic copy of the hook in `templates/` with `templates/claude-settings.json`; the ER-2 amendment | 1 |
| WP-5 | Paddock: rule 2 amendment, the 2026-11-10 lessons entry (memory says it is not in `docs/lessons.md` yet), ADR index | 0.5 |
| WP-6 | Migration: lock every live agent worktree with an owner note through the tool, report the 4 locks without a reason and the 9 detached worktrees to the coordinator; ask the user about the 96 archives in the primary | 0.5 |
| | **Total** | **7** |

First cut (closes incidents 1 to 3, the ones that cost the user time): WP-0, WP-2 (R1, R2, R4 first), WP-1 `add` and
`clean` = **3.5 days**. The rest follows in the next process batch.

## Release impact (ER-1, ER-31)

No shipped code changes. `.claude/` is outside every app's dependency set; `tools/paddock-dev` marks the `plan` and
`tools` jobs only (AGENTS.md, affected planning). It can land while any release is in flight and needs no release gate.

## Decisions for the user

| # | Question | Options | Recommendation |
| --- | --- | --- | --- |
| D1 | The coordinator in the primary checkout | (a) Agents denied; the coordinator gets `ask` for git writes there, except fast-forwards, fetch and worktree management, which pass. (b) The coordinator is exempt entirely. (c) No exemption: the coordinator also integrates only in an integration worktree (train worktrees already are) and the primary only fast-forwards; later, a bare primary. | (a): it keeps today's landing flow, makes a non-fast-forward write in the primary a deliberate act, and leaves (c) open. |
| D2 | Where the tool lives | (a) `./dev worktree` for Paddock, with `worktree.sh` updated to the same semantics for other projects. (b) `worktree.sh` everywhere. | (a): ADR 0022 moves Paddock's scripts to Rust; the generic script stays the reference for projects without `./dev`. |
| D3 | Unlocking and stale locks | (a) Only the tool unlocks: the owner at hand-off or the coordinator after the agent's completion notification; locks older than 7 days are reported by `list` and `check primary`, never expired automatically. (b) Locks expire after N days. | (a): an expired lock is how incident 1 happens again, just later. |
| D4 | `cd` chained with `;` | (a) Denied for everyone. (b) Denied for agents, `ask` for the coordinator. (c) Warning only. | (a): `&&` costs nothing and the coordinator's shell persists between calls, which makes a failed `cd` worse there. |
| D5 | Agent file edits in the primary (`Edit`, `Write`) | (a) Denied. (b) `ask`. | (a): no brief asks an agent to edit the primary; memory and scratch folders are outside it. |
| D6 | Engineering rule | (a) Amend ER-2 with the text of §4 (tool, lock, cleanup, hook, probe). (b) A new ER-37. | (a): it is ER-2's own mechanism; a new number would split one rule. |
| D7 | Hook failure mode | (a) Refuse only on a positive match; a command it cannot parse passes and the probe catches the damage. (b) Refuse what it cannot parse. | (a): a false refusal of an odd but harmless command stalls agents; the probe is the backstop. |
| D8 | Scope now | (a) All WPs (7 days). (b) The first cut (3.5 days) now, the rest in the next process batch. | (b). |
