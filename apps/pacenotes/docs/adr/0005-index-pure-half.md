# 5. The index, pure half: schema v1, change plan, denylist and the trap

Status: proposed (2026-10-30), design note of package A3a (`pacenotes-index`, plan BN5). Numbered at merge. ADR 0002 fixes the
vocabulary and the schema contract; this note says how the first half of the index is built and what it leaves to A3.

## Context

The index turns a workspace into rows: files with fingerprints, symbols with stable ids, imports, calls and chunks. A3a is the
part that is pure or thin: the SQLite schema and its rebuild rule, the plan that turns a folder snapshot into changes (renames
keep their ids), the secret denylist, and the walk that refuses to leave the workspace. It runs no watcher and no jobs, spawns
nothing and parses nothing itself: parsing is a port, implemented by the parser worker in A3 (ADR 0003: the grammar library is
not crash-isolated, so the app never calls it in process).

## Decision

### Ports and what is in the crate

| Piece | What it is |
|---|---|
| `WorkspaceFs` (port) | `list(dir)` and `read(path, max)` over the workspace; the adapter `DiskFs` refuses to follow a link and checks every path against the canonical root. A recording double in tests is the **read seam** |
| `SymbolSource` (port) | `extract(path, bytes, cancel) -> Option<FileFacts>`: the facts of one file. The real one is the A3 worker; A3a's tests use `pacenotes-parse` in the test process only |
| `Denylist` | pure matcher over `RepoPath` (names, extensions, folders) and `redact(text)` for credential-looking text |
| `plan_changes` | pure: previous rows and a fresh snapshot with fingerprints give added, modified, deleted, renamed (matched by path first, then by fingerprint) and unchanged |
| `IndexStore` | SQLite file, schema v1, rebuild rule, the read side of the core `Store` port, symbol and path search |
| `Indexer` | snapshot, fingerprint, plan, extract, and one transaction that swaps the new generation in |
| `GuardedHistory` | wraps any `History`: a denied path is `HistoryError::Denied`, never content |

### The secret denylist stands in front of every reader of content

AGENTS.md: a denylist keeps `.env` and key material out of the index and masks credentials before they reach an MCP reply, a log
or the UI. It is applied at **three** doors, and one test per door:

1. **The index:** a denied file is never opened for reading (it does not reach `files_read`), never fingerprinted, never listed
   in `files`, never chunked.
2. **`ReadAt`:** the host call and any other reader of the `History` port go through `GuardedHistory`, because the object database
   holds whatever was ever committed (ADR 0004).
3. **MCP and the UI:** every text that leaves the engine for a client passes `redact`, and every file read for a client passes
   `Denylist::check` (the MCP crate calls both; `chunk_text` in this crate is the model).

The rules are **data**: `crates/pacenotes-index/src/denylist.default.toml` (names, glob patterns, an allow list, key-material
extensions with their backup suffixes, folders, and the redaction regexes), parsed by `Denylist`; a pack that fails to parse
makes the default fail closed (everything is withheld). The format is written to be owned by a shared crate.
**TODO (ER-22, after shared code unfreezes):** Debrief and Sighter also mask secrets; move the pack format and the matcher into
one shared crate (`paddock-secrets` is the candidate; it is untouched here because shared crates are frozen) and make the three
apps load the same default pack. The list that follows is a summary of the pack, which is the source of truth.

The default list: names `.env`, `.env.*` (not `.env.example`, `.env.sample`, `.env.template`), `.envrc`, `.netrc`, `.npmrc`,
`.pypirc`, `.htpasswd`, `.git-credentials`, `id_rsa`/`id_dsa`/`id_ecdsa`/`id_ed25519` (not `*.pub`), `credentials`,
`credentials.json`, `secrets.json`/`.yml`/`.yaml`, `service-account*.json`, `terraform.tfstate*`; extensions `.pem`, `.key`,
`.p12`, `.pfx`, `.jks`, `.keystore`, `.kdbx`, `.ppk`, `.gpg`, `.tfvars`; folders `.git`, `.ssh`, `.aws`, `.gnupg`, `.azure`.
Matching is case-insensitive. Users extend it in Settings (a later package), never shrink it below the default: `Denylist::from_pack`
reads a user pack in the same format and **merges it over** the default. Names, patterns, extensions, folders and redaction rules
are added; the default always applies first, a user `allow` cannot lift what the default denies, and the default's redaction rules
run before the user's. A pack that does not parse is refused with the message that says what to fix (and a default that does not
parse fails closed). A `plain_globs` entry is a name pattern for names without a dot (`*deploy*key*` denies `deploy_key` and
`github-deploy-key`, not `docs/deploy-keys.md`).

### Walking the workspace

The walk takes an `Ignore` port (default `NoIgnore`): a file or folder the port hides is counted as *ignored* and not visited. The
denylist is checked first, so ignoring never relaxes it. The engine supplies the `.gitignore` implementation from the shared
`paddock-walk` (today `paddock-repo-files`) walker (ER-22), see ADR 0006.

`DiskFs` lists with `symlink_metadata`, never follows a link (a link is skipped and counted, whatever it points at: a file, a
folder, `/`, or itself), skips any `.git` entry (a folder or a gitfile) and every denied name, and checks that the canonical
path of what it reads is under the canonical root. Bounds: entries (default 1,000,000), depth (64) and file size (2 MiB; a larger
file is listed, not read). The check-then-read race is the one of ADR 0004 and is accepted for the same reason.

### Change plan

`plan_changes(previous, current)`: a path present in both is `unchanged` (same fingerprint) or `modified`; the leftovers are
paired **by fingerprint** into `renamed` (sorted by path, so the pairing is deterministic when several files are identical);
what remains is `added` and `deleted`. A rename keeps the `FileId`, so every `SymbolId` survives (ADR 0002); a rename plus an
edit is not detected (it is a delete and an add, and the ids change), and A3 adds similarity matching if the loss shows.
`FileId`s come from `FileIdAllocator`, whose next sequence is stored in `meta`, so an id is never reused.

### Schema v1 and the rebuild rule

One SQLite file in the app's data folder, never in the repository. WAL, `meta(schema_version, generation, next_file_sequence)`,
`files`, `symbols`, `imports`, `calls`, `chunks`; `files.module_path` is the separate, mutable column of ADR 0002 (A3 fills it).
`open` trusts nothing: a file that is not SQLite (header), fails `quick_check`, has no `schema_version` or has another version
is moved aside (`<name>.corrupt`, replacing an earlier one) with its `-wal` and `-shm`, and a new store is created; the caller
learns why (`Opened::rebuilt`). All of a run's changes are one transaction, so a crash or a cancel leaves the previous generation
readable ("build new data, then swap").

### The trap test (audit PN1) moves here

The ignored placeholder in `pacenotes-trap` is deleted and its test is written in this crate (`tests/it/trap.rs`) because the
index cannot be a dependency of the fixture crate. It indexes the generated trap repository through the read seam and a source
double that records every call, runs `TRAP_QUERIES` through the store's own search, reads every chunk through `chunk_text`, and
asserts `Observations::verdict` is empty: nothing ran (no child is started, the marker is absent), nothing leaked (`.env` is in
no table, no search reply and no chunk text, in any letter case), nothing escaped (no read outside the workspace; links and the
nested gitfile are skipped).

## Parallelism and cost (rule 51)

A3a is deliberately single-threaded and simple, and says what A3 must do to be fast. **Cost:** every run lists the workspace,
reads and fingerprints every non-denied file (one read per file in the walk and one more for changed files, at most 2 MiB each), and extracts only added or modified
files; unchanged files cost a read and a hash. A3 adds the cheaper path (skip a read when size and modification time match the
stored row, which the watcher also feeds) and the **parallel** one: fingerprinting and extraction are independent per file and
map over a pool of cores minus one at background priority with results applied in path order (`par_map_ordered`), thread
counts a Setting with Auto; the single writer applies one batch per transaction. The plan and the denylist are one pass over
sorted vectors and a set. Measurements go to `docs/performance.md` with the first large repository (rule 48); A3a's tests use
small generated workspaces.

## Consequences

- A rename plus an edit loses ids until similarity matching exists.
- A denied file is invisible to the index even for search by name; the user sees it listed as "withheld" in the report counts.
- `pacenotes-index` has `rusqlite` (bundled SQLite, the family's version) as its one heavy dependency and no process, network
  or parser dependency.

## Amendment (2026-11-04, ADR 0012): schema v2, the manifest and the per-content fact cache

Accepted by the user on 2026-11-04 with ADR 0012, which replaces these parts of this ADR:

- **Schema.** Schema v1's per-row fact tables (`symbols`, `imports`, `calls`, `chunks`) are replaced by schema v2 (ADR 0012 D4):
  the manifest, `generation_files` (path, `FileId` and fact key per generation), the per-content fact blobs (`facts` appended in
  insertion order, `fact_keys` as a narrow index; benchmark B9 may move the blobs into Arrow IPC segment files) and quarantine. The
  rebuild rule of this ADR stays: a schema v1 file is moved aside and rebuilt, with no migration before v1.
- **Change plan.** `plan_changes` keeps its rules (unchanged, modified, renamed by content key, added, deleted; a rename keeps the
  `FileId`). Its input is now the `ChangeSet` of the manifest (ADR 0012 D1), whose stat adds ctime and the file id to size and mtime.
- **"All of a run's changes are one transaction".** Now only the swap of the generation's path-to-fact-key map is one transaction.
  The fact blobs are content-addressed and are written as they arrive, in transactions of about 1,000. A crash or a cancel before
  the swap leaves the previous generation readable, plus unreferenced blobs that the sweep job removes. "Build new data, then swap"
  still holds.
- **`symbols_in_file` and search** read the derived snapshot (ADR 0012 D5) instead of SQLite tables. The `LIKE` search over
  `lower()` goes away; names are found through the snapshot's FST.

Unchanged: the ports `WorkspaceFs` and `SymbolSource` (whose result becomes a fact blob), the denylist and its three doors, the
walk's link refusal and bounds, and the trap test.
