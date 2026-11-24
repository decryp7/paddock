# Lessons moved out of the generic engineering-rules repo

These passages held Paddock names, run ids, train numbers or machine paths, so they left the generic engineering-rules repo. Each section names its source file and holds the original text verbatim (moved 2026-11-24). The generic repo keeps a project-neutral version of each.

## From LESSONS.md

**Judge a red CI by failing tests and skipped steps, not by jobs (2026-11-04, Dyno 1.1.0).** Symptom: a release waited on red runs. Cause: run 7485 failed mostly in code the app ships; run 7489 failed in the shell-demo's i18n check (outside the app) and in aggregate coverage, whose gate printed no per-crate output while a crate the app depends on was in it; those failures also skipped the dependency audit and several ui steps. Rule: ER-31: tag the tested commit in CI only when every failing test is outside the app's dependency set and every skipped step is re-run green or shown not to touch it; an aggregate coverage failure and the release path always block; land the branch by merge or fast-forward so the tag stays an ancestor of main.

**Agents filled the home folder with build folders and worktrees (2026-10-28).** Symptom: about 160 GB of `.debrief-*` and `.paddock-*` build folders and worktrees in `~` on a Mac with a small system disk. Cause: agents chose their own `CARGO_TARGET_DIR` and worktree paths and nothing checked them. Rule: build folders, worktrees and logs live on the work disk, never in `~`; `scripts/heavy.sh` refuses a command whose `CARGO_TARGET_DIR` or `--target-dir` / `build.target-dir` is inside `$HOME` (`HEAVY_ALLOW_HOME_TARGET=1` opts out) ([ER-14](RULES.md), [ER-24](RULES.md)).

**A new wiki was wrong on day one; only an independent fact-check caught it (2026-10-31).** Symptom: the first Paddock wiki pages (an ER-30 pilot) held a missed release blocker the writer had not noticed and four wrong facts, including log entries cited by a typed time and rule text restated as paraphrase. Cause: one agent wrote the pages from its own reading, so nothing compared them with the sources, the rules on open branches or the ER text. Rule: fact-check every claim independently before pages land, cite log entries by file:line with dates from the clock, lint against rules on main and on open rule branches, link rules instead of restating them, and age facts out with `verified:` and a re-verify on each ingest ([ER-30](RULES.md), [wiki](skills/wiki/SKILL.md)).

**Stuck waiters piled up because a `pgrep -f` loop matches its own command line (2026-11-03, Paddock).** Symptom: 36 stuck or stacked waiters stopped by
hand over two days (14, 18 stacked on one event, then 4); the machine's load and the agents' attention went to them. Cause: the loop's
own shell contains the pattern, so the match never ends; agents re-armed waiters instead of waiting once. Rule: ER-32: wait on a
real completion signal with a maximum duration; a hook catches the common loop shapes and is tested, but it is not a parser.

**A Mac-only run missed what the Linux and Windows runners found (2026-11-04, Paddock run 7485).** Symptom: a branch green on the Mac
failed in CI on several things at once. Cause: the CI containers run as root, font metrics and the rasteriser differ, and Windows
tests that compare paths saw `\` for `/`. Rule: ER-9: run the affected tests on Linux (VM or container) before pushing shared or
cross-platform code, run the path-comparing Windows tests, and type-check the Windows target.

## From README.md (machine layout)

The shared rules in this repo are numbered **ER-1 to ER-51** and are always cited as `ER-n`. A project's own rule set (its
`AGENTS.md`, or a family file such as Paddock's `docs/rules.md`) keeps its own numbers; those do not match these, so a bare "rule 21"
is ambiguous and is never used. Numbers are append-only: a rule is never renumbered, so a citation stays valid.
## Where this repo lives
On this Mac the clone is at `/Volumes/d8sst7/dev.decryptology.net/engineering-rules` (canonical), and `~/engineering-rules` is a
symlink to it. Every document and skill refers to `~/engineering-rules/...`, so that path must resolve on every machine:
- **Other machines:** clone straight into `~/engineering-rules`, or clone anywhere and run `ln -s <clone> ~/engineering-rules`.
1. Clone: `git clone https://dev.decryptology.net/decryp7/engineering-rules.git ~/engineering-rules` (on this Mac:
   clone to `/Volumes/d8sst7/dev.decryptology.net/engineering-rules` and `link /Volumes/d8sst7/dev.decryptology.net/engineering-rules ~/engineering-rules`).
`node --test scripts/ci-timings.test.mjs`, `node --test scripts/wiki-lint.test.mjs`, the wiki lint `node scripts/wiki-lint.mjs` (settings in `wiki-lint.json`), and `shellcheck scripts/*.sh`. CI (`.gitea/workflows/ci.yml`, copy in
`.github/workflows/ci.yml`) runs the same. The Gitea job runs in `paddock-ci:v2`; both install shellcheck as a pinned
release binary (v0.11.0, SHA-256 verified, skipped when that version is already installed); the GitHub copy stays generic on `ubuntu-latest`.
Project rules (for example Paddock's `docs/rules.md`) add to these and win on conflicts. Cite shared rules as `ER-n`; project rules keep their own numbers.

## From skills/ci-speed/SKILL.md

- Cranelift/mold hype without measurement; in one measurement (Paddock, Rust stable toolchain of 2026-10) the linker gains were negligible. Measure on your own toolchain.
## Worked example (Paddock)
Stable checkout path + local sccache + pinned host tools + one workspace-wide nextest run + image-baked browsers were
**estimated** to take the Windows leg from about 23 to about 7 minutes. That figure is an estimate, not a measurement; measure
your own before and after with `node ~/engineering-rules/scripts/ci-timings.mjs` (median and p90 over the last 30 runs). The plan job (`scripts/affected.mjs`) skips unaffected projects, WebKit and
bundles run on main, tags and nightly only.

## From skills/icon-family/SKILL.md

9. Last step, after the review: refresh the user's profile repo (`decryp7/.profile`) from the new images with its
   `scripts/refresh-images.sh`, update its README text if it changed, and push (user, 2026-10-23). It embeds copies of the
   images and goes stale otherwise.
## Worked example (Paddock)
An isometric motorsport family: one object per app on a coloured tile, shared toolkit (`iso.mjs`), `STYLE.md` with the value
ladder, matched visual weight 0.68, one family shadow, 2:1 pixel isometric at 32/24, flat symbol at 16, whole-pixel trays.

## From skills/release/SKILL.md

Worked example (Paddock): one `vX.Y.Z` tag publishes all crates and packages via a workflow; each crate bumps its own version
only when it changes and already-published versions are skipped; Paddock's own `node scripts/publish.mjs` is the dry run.

## From skills/repo-import/SKILL.md

Worked example (Paddock): eight app repos imported under `apps/<app>` one at a time, tags `debrief-v1.0.0` style, a plan job
and `release-<app>.yml` per app, old repos frozen as pointers.

## From agents/integrator.md

- Before pushing a train, run the full conformance CI line on the merged tree, not only on the touched files (on 2026-11-10 six approved branches landed in train 4 with C53/C71 errors found only at train time).

## From checklists/pre-review.md

- [ ] **Every fix made in answer to a review comment comes with a test that fails without it:** revert the fix and show the test red
      before you push. Why: twice on 2026-11-10, review-found fixes (the Debrief worker's join, and its exit fallback) landed with
      no test, and reverting them left everything green.
- [ ] **Before pushing a train, run the full conformance CI line on the merged tree,** not only on the touched files. Why: on
      2026-11-10, six approved Rust-tool branches passed review and landed in train 4 with C53/C71 conformance errors, which were
      found only at train time.
- [ ] **Before review, run the repository-wide gates, not only the touched-file ones: the full conformance tests, the gate register check, and a locked resolve of every workspace that depends on a changed shared package.**
      Why: on 2026-11-11, train 5 found three such failures in review-approved branches (a conformance test, an ignored test with
      no gate-register entry, a lockfile updated in one workspace only), and train 4 found C53 and C71 the same way on 2026-11-10.

## From checklists/speed-and-quality.md

- [ ] The Linux run uses a container native to the machine's CPU (an emulated one can hide sandbox, process and timing behaviour) and the CI runners' limits: open files, CPU share, test threads and temp location as the project declares them. Why: on 2026-11-09 a Dyno test passed locally at 20,480 open files and failed on CI at 1,024, and a plugin sandbox could not start in an emulated container at all.

## From checklists/new-machine.md and prompts/new-machine.md (machine layout)

- [ ] `git clone <host>/engineering-rules.git ~/engineering-rules`; on the Mac the canonical clone is `/Volumes/d8sst7/dev.decryptology.net/engineering-rules` with `~/engineering-rules` as a symlink to it (other machines: clone straight to `~/engineering-rules`)

1. The rules repo must be reachable as ~/engineering-rules. If it is missing: `git clone https://dev.decryptology.net/decryp7/engineering-rules.git ~/engineering-rules`
   (otherwise `git -C ~/engineering-rules pull --ff-only`). If I keep the clone elsewhere (on my Mac:
   /Volumes/d8sst7/dev.decryptology.net/engineering-rules), make ~/engineering-rules a symlink to it and point every ~/.claude

## From wiki/README.md (whole file, removed from the generic repo)

````
# Engineering-rules wiki

A small, sourced summary of how things work **today**, so an agent reads a page instead of re-deriving the state from the
lessons, the ADRs, the logs and the hand-off notes. The pattern is ER-30 in [RULES.md](../RULES.md); the adapted
source is Andrej Karpathy's [LLM Wiki gist](https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f) (read as
data). This is the repo's own wiki and the worked example for [the setup checklist](../checklists/wiki-setup.md); the kit to copy is in [templates/wiki](../templates/wiki/README.md).

## What belongs here and what does not
- Here: the current state of a topic (how CI, releasing, runners or a component work now), open decisions, and links to
  where the truth lives.
- Not here: rules, ADR decisions, registers, checks. They stay authoritative in [RULES.md](../RULES.md), [PRINCIPLES.md](../PRINCIPLES.md) and [LESSONS.md](../LESSONS.md). A page **links** to them and says what they are about; it never restates their text, not even reworded. Where a
  page disagrees with one of them, the page is wrong and is fixed.
- Not here: lessons. The lessons file keeps incidents; a page may link one as a source.

## Layers
1. Sources, never edited by the wiki: code, workflows, ADRs, rules, lessons, commits, CI runs, hand-off files.
2. The wiki: topic pages in this folder, plus `index.md` (one line per page) and `log.md` (one dated line per change).
3. This schema.

## Page format
See [page-template.md](../templates/wiki/page-template.md). In short: front matter with `title`, `verified: YYYY-MM-DD` and `sources:`; a short
summary; "Current state" with one claim per bullet, each bullet carrying its source in the same bullet (a Markdown link, or
`src: <path:line or id>` for a source that cannot be linked); "Open questions"; "See also".
- `verified:` is the day someone last checked every claim of the page against its sources. An old date draws a lint warning (the
  `maxAge` setting, 30 days by default); the writer re-verifies the page, nobody deletes it.
- Dates come from the clock (`date +%F`), never typed from memory. Log and hand-off entries are named by `file:line`; a
  typed clock time identifies nothing.
- A fact that exists only on a branch says so (branch and commit), so the page claims no more than the main branch has.
- No binding words in a wiki sentence. Write "rule 12 covers this" and link the rule.

## Operations
- **Ingest** (after a merged batch, an incident, a decision): collect the `Wiki notes:` of the agents' reports and the sources;
  for each affected page re-verify all its claims against their sources, change what moved, set the source and `verified`;
  update `index.md`; append to `log.md`; run the lint; have new or changed pages fact-checked by someone who did not write them
  and fix what they find; commit `docs(wiki): ...` as its own commit.
- **Query**: read `index.md`, then the pages it points to, and answer with the page's sources. If the answer was missing or
  wrong, leave a wiki note for the next ingest. In briefs, link the page instead of restating context.
- **Lint**: the wiki lint (checklist step 5; `scripts/wiki-lint.mjs`, settings in `wiki-lint.json`, contract in [LINT.md](../templates/wiki/LINT.md)) on every wiki change (links and anchors resolve; every page is indexed and linked from
  another page; `verified:` and `sources:` present; every bullet names a source; log lines in shape; stale pages flagged; no run
  of 8 or more words shared with any rules source, for the rules sources listed in `wiki-lint.json`). Weekly, a
  bounded reading pass checks a few pages at a time against the rules and ADRs they cite.

## One writer
Only the coordinator edits this folder. Every other agent ends its report with a `Wiki notes:` line (facts
that changed, each with its source, or "none") and does not touch the wiki. This keeps `index.md` and `log.md` free of merge
conflicts and the pages under one editor.

## Pages
See [index.md](index.md).
````

## From wiki/index.md (whole file, removed from the generic repo)

````
# Wiki index

One line per page. Read this first, then the page. Rules and lessons stay authoritative; see [README](README.md).

- [How the rules are organised and numbered](rules-structure.md): sections, numbering, where lessons go. Verified 2026-11-04.
- [Who imports the rules and how](adopters.md): user CLAUDE.md, symlinks, templates, Paddock mapping. Verified 2026-10-31.
- [Skills, agents and commands](skills-and-agents.md): what exists and where. Verified 2026-10-31.
- [Change history highlights](history.md): commits that shaped the rule set. Verified 2026-10-31.
- [Open questions](open-questions.md): unsettled items with their evidence. Verified 2026-10-31.
````

## From wiki/log.md (whole file, removed from the generic repo)

````
# Wiki log

Append-only. One line per change: `## [YYYY-MM-DD] ingest|lint|fix | page | subject`, the date from `date +%F` (never typed),
the subject saying what changed and from which source or batch.

## [2026-10-31] ingest | all | seed five pages from main a8064e1 and branch docs/er-30-wiki, using templates/wiki
## [2026-10-31] fix | all | review fixes: history.md heavy.sh/worktree.sh commits, machine-local and Paddock-specific marks, citations, restated rule text removed
## [2026-11-04] fix | rules-structure | rule count ER-1 to ER-32 and section line numbers after ER-31 and ER-32 (RULES.md)
````

## From wiki/history.md (whole file, removed from the generic repo)

````
---
title: Change history highlights
verified: 2026-10-31
sources: [git log of this repository]
---
The commits that shaped the rule set, newest first. Hashes are on main unless stated; `git log` is the full record.

## Current state
- ER-30 (project wiki) and the wiki skill, templates and checklist are on branch `docs/er-30-wiki`, not on main (src: `git log docs/er-30-wiki`, commit `290cd0d` is the first).
- ER-28 and ER-29 arrived on 2026-10-31 (src: `d35fd81`); ER-12 and ER-28 were tightened the same day (src: `a44f1d5`).
- ER-27 was added on 2026-10-31 and widened to CI, release pipelines and tools in later commits (src: `e846d9a`, `223b0fc`, `84e65b9`).
- ER-26 was added on 2026-10-30 (src: `ebf7087`); ER-20 on 2026-10-29 (src: `432a0d8`).
- ER-1 and ER-25 were amended on 2026-10-28 (src: `de99149`, `1b14337`).
- heavy.sh fixes of 2026-10-27 and 2026-10-28 (src: `4501735`, `98af20a`, `a18cb5d`) and the worktree.sh fix of 2026-10-27 (src: `9cf4f03`).
- The skills worktree-lifecycle and integrate were added on 2026-10-27 (src: `0d9612f`), the scout role the same day (src: `07120ae`).

## Open questions
- None recorded (src: [index](index.md)).

## See also
- [How the rules are organised](rules-structure.md)
````

## From wiki/adopters.md (whole file, removed from the generic repo)

````
---
title: Who imports the rules and how
verified: 2026-10-31
sources: [README.md, templates/AGENTS.md, ~/.claude/CLAUDE.md, .gitea/workflows/ci.yml, .github/workflows/ci.yml]
---
The channels through which the rules reach a session or a project, and how Paddock maps its own numbers onto the ER numbers.

## Current state
- Machine-local (the user's Mac, read 2026-10-31): the user-level `~/.claude/CLAUDE.md` imports RULES.md and PRINCIPLES.md by absolute path into every project (src: `~/.claude/CLAUDE.md:1-2`, read 2026-10-31).
- Machine-local (the user's Mac): `~/.claude/agents` and `~/.claude/commands` are symlinks to this repo's folders, and each skill folder has its own symlink in `~/.claude/skills` (src: `ls -la ~/.claude` run on 2026-10-31).
- Machine-local (the user's Mac): the wiki skill has no symlink in `~/.claude/skills` yet (src: `ls -la ~/.claude/skills` on 2026-10-31, branch not merged).
- A project can import the rules in its own CLAUDE.md or keep a synced copy, see [README.md](../README.md#use-in-a-project) (src: `README.md:88`).
- New projects start from [templates/AGENTS.md](../templates/AGENTS.md), whose Baseline section links RULES.md and PRINCIPLES.md (src: `templates/AGENTS.md:3-5`).
- Paddock-specific, on a branch only: Paddock keeps its own numbered rules in `docs/rules.md` and cites the ER number where one exists: rule 1 cites ER-1 (src: Paddock `docs/rules.md:10` on branch `docs/rca-rules` at `9fba4291f`), rule 51 cites ER-20 (`docs/rules.md:183`), rule 57 cites ER-28 (`docs/rules.md:302`), rule 58 cites ER-29 (`docs/rules.md:309`), all on that branch.
- This repo's own CI runs the script tests on Gitea and GitHub (src: `.gitea/workflows/ci.yml:35-43`, `.github/workflows/ci.yml:34-42`).

## Open questions
- Whether the ER-30 wiki skill gets its symlink on every machine once merged: see [open questions](open-questions.md).

## See also
- [How the rules are organised](rules-structure.md)
````

## From wiki/open-questions.md (whole file, removed from the generic repo)

````
---
title: Open questions
verified: 2026-10-31
sources: [docs/er-30-wiki branch, Paddock docs/rules.md on docs/rca-rules]
---
Things this repo has not settled. Each line says where the evidence is; deciding belongs to the user.

## Current state
- Paddock-specific: Paddock rules 53, 54 and 55 carry no ER citation, so whether any should become an ER rule is open (src: Paddock `docs/rules.md:228`, `:248`, `:259` on branch `docs/rca-rules` at `9fba4291f`).
- Paddock-specific: Paddock rule 56 has no ER citation although ER-27 covers the same subject (src: Paddock `docs/rules.md:284` on that branch, and `RULES.md:49`).
- ER-30 is on branch `docs/er-30-wiki` for review and is not merged (src: `git log docs/er-30-wiki`).
- Machine-local (the user's Mac): the wiki skill is not yet linked in `~/.claude/skills` (see [adopters](adopters.md)).
- The Paddock pilot evaluation (ten questions, before/after) was not run before ER-30 was drafted; the user ended the pilot early (src: handoff `merge-steward.md:719`).

## Open questions
- See the list above (src: [adopters](adopters.md)).

## See also
- [Who imports the rules](adopters.md)
````

## From wiki/rules-structure.md (whole file, removed from the generic repo)

````
---
title: How the rules are organised and numbered
verified: 2026-11-04
sources: [RULES.md, README.md, LESSONS.md, prompts/add-rule.md]
---
Where the shared rules live, how they are grouped and numbered, and how a lesson becomes a rule. The rules themselves are in
[RULES.md](../RULES.md); this page only maps them.

## Current state
- [RULES.md](../RULES.md) holds ER-1 to ER-32, see src: `RULES.md:6`; the principles are a separate file, [PRINCIPLES.md](../PRINCIPLES.md).
- Sections in file order: Way of working (src: `RULES.md:10`), Testing (`RULES.md:18`), Releases (`RULES.md:30`), Long operations (`RULES.md:40`), Design (`RULES.md:46`), Machine load (`RULES.md:51`), Diagnostics (`RULES.md:54`), Agents (`RULES.md:58`), Knowledge (`RULES.md:62`).
- Numbers follow the order rules were added, not the section: ER-26 and ER-29 sit under Testing (src: `RULES.md:27`, `RULES.md:28`), ER-25 under Agents (src: `RULES.md:59`), ER-27 and ER-28 under Diagnostics (src: `RULES.md:55`, `RULES.md:56`).
- The numbering policy is in [README.md](../README.md#rule-numbers) and `RULES.md:6-8`; this page does not restate it (src: `README.md:20-22`).
- Lessons are collected in [LESSONS.md](../LESSONS.md); how one is promoted to a rule is in [add-rule](../prompts/add-rule.md) (src: `LESSONS.md:3-5`).
- Several rules link a skill that carries the procedure, for example ER-30 links [the wiki skill](../skills/wiki/SKILL.md) (src: `RULES.md:63`).

## Open questions
- None recorded (src: [index](index.md)).

## See also
- [Skills and agents](skills-and-agents.md)
- [Who imports the rules](adopters.md)
````

## From wiki/skills-and-agents.md (whole file, removed from the generic repo)

````
---
title: Skills, agents and commands
verified: 2026-10-31
sources: [README.md, agents/, skills/, commands/, prompts/]
---
Index of the non-rule content of the repo: agents, skills, commands, checklists and templates.

## Current state
- Five agent roles, each with a model in its front matter: scout on Haiku (src: `agents/scout.md:4`), implementer on Sonnet (`agents/implementer.md:4`), designer, researcher and reviewer on Opus (`agents/designer.md:4`, `agents/researcher.md:4`, `agents/reviewer.md:4`).
- The skill folders on this branch are: bug-fix, ci-speed, ci-triage, delegate, icon-family, independent-review, integrate, recurring-bug-adr, release, repo-import, wiki, worktree-lifecycle (src: `ls skills` on 2026-10-31).
- The skills table is in [README.md](../README.md#skills) (src: `README.md:46-64`).
- Four slash commands exist, `rules-adopt`, `rules-new-project`, `rules-review`, `rules-sync` (src: `ls commands`), with matching pages in [prompts](../prompts/new-project.md).
- The setup checklists are in `checklists/`, among them [wiki-setup](../checklists/wiki-setup.md) (src: `ls checklists`).
- Starter files are in `templates/`, including the [wiki kit](../templates/wiki/README.md) (src: `ls templates`).

## Open questions
- None recorded (src: [index](index.md)).

## See also
- [How the rules are organised](rules-structure.md)
- [Change history](history.md)
````

## From wiki-lint.json (whole file, removed from the generic repo)

````
{
  "wiki": "wiki",
  "rules": ["RULES.md", "PRINCIPLES.md"],
  "citeByLine": []
}
````
