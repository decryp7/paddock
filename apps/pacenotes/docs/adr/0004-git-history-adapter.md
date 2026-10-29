# 4. The history adapter: gix behind `History`, opened so that nothing a repository configures can run

Status: proposed (2026-10-29), design note of package C2 (`pacenotes-git`, plan BN4). Numbered at merge.

## Context

`History` (ADR 0002) is the port through which the core reads a repository's past: HEAD, a file at a revision, the files that
changed between two revisions. C2 implements it with `gix` (in process), adds the log and the churn and hot-spot figures that
Understand and Review need, and must hold the rule that Pacenotes **never executes repository code** (AGENTS.md, audit PN1).
Git itself can run programs named in a repository: `core.fsmonitor`, `filter.<name>.clean|smudge|process`,
`diff.<name>.textconv|command`, `core.hooksPath`, `core.sshCommand`, `include.path`, and it follows `objects/info/alternates`
and a `.git` file to any place on disk. The analysed repository is hostile input.

## Decision

### What the crate is

- `GixHistory` implements `History` over a repository opened with `open(workspace)`.
- `log(from, max_commits, cancel)` returns, per commit on the first-parent line, its time, author and changed files.
- Pure functions turn that into `FileChurn` and ranked `HotSpot`s (recency-weighted, deterministic: score descending, path
  ascending). They take `now` as an argument and read no clock.
- Blame follows in a later change (gix has `blame`); nothing here depends on it.
- The crate never starts a process: `clippy.toml` forbids `Command::new` workspace-wide, and a test scans the sources.

### How a repository is opened safely

1. **Only the workspace's own repository.** `open` takes the workspace root the user put on the allow-list, canonicalises it
   and looks at `<root>/.git` only; it never searches parent folders (`open_path_as_is`).
2. **Everything git may point elsewhere is checked first, before gix sees it, and refused with a typed error naming the
   thing and the path** (`OpenError::OutsideWorkspace`): a `.git` *file* (`gitdir:`) whose target is outside the root; the
   git dir and its `objects` folder after resolving symlinks; `commondir`; and every line of `objects/info/alternates`
   (followed up to five levels, as git does). A relative alternate is resolved against the objects folder. gix has no option
   to turn alternates off, so a repository that borrows objects from outside the workspace is **not read** (the caller shows
   tier 1 and says why); an alternate inside the workspace is allowed.
3. **gix is opened isolated:** `Options::isolated()` (no system, user or environment configuration, no includes), reduced
   trust, `core.fsmonitor=false` overridden. The repository's own `config` is read as data.
4. **What gix is never asked to do:** no worktree, index, status or checkout (so `core.fsmonitor` is never reached); no
   filter pipeline (blobs are read from the object database as stored, so `filter.*` and `.gitattributes` are never
   consulted); no blob diff, so no `diff.*.textconv` or external diff (changes come from the pure tree-to-tree diff of
   `gix-diff`, which reads trees only); no resource cache and no attribute stack; no hooks; no network.
5. **Renames are found from data we already have:** a deleted and an added entry with the same blob id are a rename (exact);
   no similarity pass, because that would read blobs through the diff pipeline, which is where textconv lives.

### The trap test is the proof (plan A3a row, C2 part)

The trap repository (`pacenotes-trap`) has real history, then a planted `.git/config` with `fsmonitor`, filter and diff
drivers, `.gitattributes` binding them to every file, alternates pointing outside, and a nested `.git` file pointing
outside. C2's tests: (a) `open` refuses the workspace while the alternates file is present and the nested gitfile at all,
with the right error; (b) with the alternates line removed, `open` succeeds and `head`, `read_at`, `changed_between` and
`log` all work on the trapped repository, and the marker file the drivers would create **does not exist** afterwards;
(c) the sources contain no process spawning. History reads the object database as it is: keeping `.env` out of the *index* is
the denylist's job in A3a, not the port's.

## Parallelism and cost (rule 51)

Reads are independent. `GixHistory` holds a `ThreadSafeRepository` and takes a thread-local handle per call, so callers may
read from several threads; the bound is the disk (SSD a few, HDD one or two), a Setting with Auto, owned by the caller.
Cheaper paths: `read_at` asks the object header for the size first and refuses an over-limit file without reading it;
`changed_between` diffs trees only (no blob is read) and stops at the limit; `log` walks first-parent only, diffs each commit
against its first parent, caps the commits (default 5,000, at most 100,000) and the files per commit (10,000), and stops at
cancel; the pure ranking is one pass with a map. Measurements (rule 48) go to `docs/performance.md` when a large repository is
used; the C2 tests use small generated repositories.

## Consequences

- A repository with outside alternates or an outside gitfile is not readable, by design; the message says what to change.
- File names that are not valid `RepoPath`s (a backslash, a control character) are not listed.
- Rename detection is exact only in v1; a moved-and-edited file shows as delete plus add.
