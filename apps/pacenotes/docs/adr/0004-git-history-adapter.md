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
- Blame is a follow-up package, C2b (plan, 2026-10-30); nothing here depends on it.
- The crate never starts a process: `clippy.toml` forbids `Command::new` workspace-wide, and a test scans the sources.

### How a repository is opened safely

1. **Only the workspace's own repository.** `open` takes the workspace root the user put on the allow-list, canonicalises it
   and looks at `<root>/.git` only; it never searches parent folders (`open_path_as_is`).
2. **Everything git may point elsewhere is checked first, before gix reads anything, and refused with a typed error naming
   the thing and the path:**
   - a `.git` *file* (`gitdir:`) whose target is outside the root, `.git` itself as a symlink, the git dir and its `objects`
     folder after resolving symlinks, and `commondir` (`OpenError::OutsideWorkspace`);
   - **every alternate, judged by gix's own resolver** (`gix::odb::alternate::resolve`, the code that will read them), so
     the check and the read agree on quoting (a line starting with `"` uses C-style quoting with octal and other escapes),
     relative paths and nesting; a quoted line is not a way past a check that parsed lines by hand. More than eight
     alternate folders are refused; a missing one inside the workspace is ignored, as git does; one that cannot be resolved
     is refused;
   - **any symbolic link anywhere under the git dir and the common dir** (`OpenError::SymlinkInGitDir`): git never creates
     one there, and a link at `objects/pack`, `packed-refs` or `refs/heads/x` would lead the read out. The walk uses
     `symlink_metadata`, never follows a link, and is bounded (a million entries, twelve levels).
   gix has no option to turn alternates off, so a repository that borrows objects from outside the workspace is **not read**
   (the caller shows tier 1 and says why); an alternate inside the workspace is allowed.
   **Race.** These checks happen before the open; a process that replaces a folder with a link between the check and gix's
   read can still win. That needs write access to the repository while Pacenotes opens it, which is outside the threat
   (a repository's contents, not a concurrent local attacker). The index reopens through `open` for each job, so a link
   planted later is caught at the next open, and the read is never wider than what the user allow-listed.
3. **gix is opened isolated:** `Options::isolated()` (no system, user or environment configuration, no includes), reduced
   trust, `core.fsmonitor=false` overridden. The repository's own `config` is read as data.
4. **What gix is never asked to do:** no worktree, index, status or checkout (so `core.fsmonitor` is never reached); no
   filter pipeline (blobs are read from the object database as stored, so `filter.*` and `.gitattributes` are never
   consulted); no blob diff, so no `diff.*.textconv` or external diff (changes come from the pure tree-to-tree diff of
   `gix-diff`, which reads trees only; **gix's `blob-diff` feature is not enabled, so the compiler enforces it**); no resource cache and no attribute stack; no hooks; no network.
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

### What the port does not do

- **It reads the object database as it is.** A blob such as `.env` that was committed is readable through `read_at`. The
  secret denylist (AGENTS.md "Secrets are redacted") therefore sits in front of **every consumer of this port that serves
  content**: the index (A3a), the host's `HostCall::ReadAt` (ADR 0002) and anything served over MCP, not only the index.
- **It is not `paddock-repo-graph`'s `History`.** ADR 0022 section 6 says to use that port where the shapes agree. They do
  not: that port serves tracked and changed files, contents and the log for the project graph and the affected plan; this one
  is revision-addressed (`Rev`), size-capped, cancellable and bounded, and carries `ChangeSet` with truncation and skipped
  names. They stay two ports for now and are listed under ADR 0002 "Shared later" so that they are reconciled when shared
  code unfreezes.

## Parallelism and cost (rule 51)

Reads are independent. `GixHistory` holds a `ThreadSafeRepository` and takes a thread-local handle per call, so callers may
read from several threads; the bound is the disk (SSD a few, HDD one or two), a Setting with Auto, owned by the caller.
Cheaper paths: `read_at` asks the object header for the size first and refuses an over-limit file without reading it;
`changed_between` diffs trees only (no blob is read) and **stops the walk itself** after `2 * limit + 2` raw changes or at a
cancel (a visitor returns `Break`), so a commit that changes a million files is not recorded whole; `log` walks first-parent only, diffs each commit
against its first parent, caps the commits (default 5,000, at most 100,000) and the files per commit (10,000), and stops at
cancel; the pure ranking is one pass with a map. Measurements (rule 48) go to `docs/performance.md` when a large repository is
used; the C2 tests use small generated repositories.

## Consequences

- A repository with outside alternates or an outside gitfile is not readable, by design; the message says what to change.
- File names that are not valid `RepoPath`s (a backslash, a control character) are not listed, but **counted**:
  `ChangeSet::skipped_names` and `CommitChanges::skipped_names` say how many, and `truncated` says when a commit was cut.
- Author text is cut at 128 bytes.
- **A linked worktree is analysed through its main checkout.** A linked worktree's `.git` is a file that points at
  `<main>/.git/worktrees/<name>`, with a `commondir` back to the main git folder, which is usually outside the workspace the
  user picked; `open` refuses it (`OutsideWorkspace`, `gitdir`). The user opens the main checkout, or allow-lists the folder
  that holds both. Reading a worktree's history is the same as reading the main checkout's.
- **Hard links are not refused.** `git clone --local` hard-links pack and loose-object files between repositories, so a link
  count above one is normal and cannot be told from a file shared with an outside repository; refusing them would reject
  legitimate clones. The cost of a hard-linked pack is that it is the same bytes as in the other repository, which is the
  repository's own data, not a way out of the workspace.
- Rename detection is exact only in v1; a moved-and-edited file shows as delete plus add.
