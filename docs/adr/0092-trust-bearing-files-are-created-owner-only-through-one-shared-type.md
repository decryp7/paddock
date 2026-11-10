# 92. Trust-bearing files are created owner-only through one shared type

- Status: Proposed 2026-11-10. Drafted at the user's request of 2026-11-10 under ER-21 (rule 40): one class, about 23
  fixes in six days. Nothing here is built yet. ER-23 does not apply (no drawings).
- Date: 2026-11-10
- Related: ER-21 / rule 40; rule 53 (reusable OS names) and ADR 0033 (identity by handle); rule 55 and ADR 0034/0075
  (copies register); ADR 0073 (no test doubles in a shipped build: the `test-support` feature); ADR 0077 (tests never
  reach system services) and ADR 0078 (one shared test scratch); ADR 0088 (one ratchet engine); family ADR 0053
  (Proposed, only on `origin/docs/dyno-adr-0027-per-machine`: per-machine installs, §2.5 where elevated code stages);
  Dyno ADR 0022 §2.1 (the protected-location rule) and Dyno ADR 0027 §3 (the helper's trust and residual risks); Pitwall
  ADR 0024 (Pitwall adopts 0053); ADR 0057 (Proposed, `origin/docs/adr-keychain-family`: no macOS Keychain, so more
  secrets become files, which raises the stakes of this ADR).
- Number: 0092 was free on every `origin` branch on 2026-11-10 (the highest files are `0090` on
  `origin/docs/adr-captured-tool-output` and `0091` on `origin/docs/adr-evidence-heads`; no branch's `docs/` names
  "ADR 0092"; no ADR worktree under `/Volumes/d7sst7/dev-targets/wt/` holds a 0092 file). Check id C106: C105 (ADR 0091)
  is the highest check id named on any `origin` branch. Rule number 75: rules 73 and 74 are named by ADRs 0073 and 0076,
  no `origin` branch names a rule 75 (`git grep -hoE 'rule 7[5-9]|C10[6-9]|C11[0-9]'` over `docs`, `conformance` and
  `tools` of all 364 `origin` refs found nothing).
- Evidence: read on 2026-11-10 with `git show`, `git log`, `git grep`, `git merge-base --is-ancestor` and
  `git tag --contains` after `git fetch`. Paths without a branch are from this ADR's base `origin/train/2026-11-09-3`.
  Nothing was built or run; no user data was read. The heads, re-read right before the commit:

```evidence-heads
read: 2026-11-10
origin/train/2026-11-09-3 67a633ef3
origin/main b51c72509
origin/docs/dyno-adr-0027-per-machine 3a6be49b5
origin/docs/adr-evidence-heads f2bdba1df
origin/docs/adr-keychain-family d37b7f452
```

## Context

### The class

A **trust-bearing file** is a file or folder whose content another process or a later run of the app believes: a
secret (API keys, the Wake-on-LAN SecureOn password, Pitwall's `admin.key`), a list that grants access (the MCP client
key list, Pitwall's `apps.json` with key hashes), a discovery file that names a pid, port and token (Dyno's
`agent.json`, the MCP listener's file), or a folder those live in, including scratch folders a worker reads back. The
bug is always one of four shapes:

- **S1 created open:** the file or folder is created with the umask's or the parent's rights and narrowed afterwards
  (or never), so for a moment, or for good, others can read or replace it.
- **S2 created in a place others control:** a fixed or guessable name in a shared folder (`/tmp/Dyno`, `%TEMP%`,
  `C:\ProgramData`, a CI runner's work folder), which another account can create first or link elsewhere.
- **S3 trusted without a check by handle:** the owner and rights are read by path (`fs::metadata`, `SetNamedSecurityInfoW`)
  and the file is then opened by path, or not checked at all on one OS.
- **S4 an elevated process touches a user-controlled place** (an installer, uninstaller or SYSTEM service following a link
  the user planted).

### The fixes (all verified with `git show --stat` and `git tag --contains`)

| Commit | Date | Instance | Shape | Shipped in |
|---|---|---|---|---|
| `3d03ede45` | 11-05 | MCP key list moves from the Keychain to an owner-only file | S1 | `dyno-v1.1.0` |
| `fc53b73a8` | 11-06 | MCP key-list folder 0700 whatever the umask | S1 | `dyno-v1.1.0` |
| `4af753edc` | 11-06 | MCP key list trusted only for the running user in a safe folder | S3 | `dyno-v1.1.0` |
| `91bffa558` | 11-05 | Dyno WoL SecureOn password in an owner-only file on macOS | S1 | `dyno-v1.1.0` |
| `3d6f22c16` | 11-06 | `paddock-secrets`: unique temp file, owner checks, private folder for `FileSecrets` | S1, S3 | `dyno-v1.1.0` |
| `504ccc402` | 11-08 | `paddock-secrets`: Windows secret files owner-only and unshared from creation | S1 | none (train) |
| `a0cb3d36f` | 11-09 | `paddock-secrets`: tests in a private temp folder; `make_folder_private` | S2 | none |
| `a78d2fc94` | 11-08 | Dyno: never fall back to a shared temp folder for data or config | S2 | none |
| `8990ed7f4` | 11-09 | Dyno: create the data folder private, tighten an own private-group folder | S1 | none |
| `d4cc7bcdd` | 11-09 | Dyno: one shared folder-trust check (`paddock_env::folder`), battery report in a fresh folder | S2, S3 | none |
| `40fe6e03f` | 11-09 | `paddock-env`: create private folders, tighten, note refusals per caller | S1 | none |
| `b4e60e9c9` | 11-09 | `paddock-env`: tighten only when the group is named like the account | S1 | none |
| `f5c753158` | 11-09 | Debrief: no fixed or guessable folder in the shared temp folder | S2 | none |
| `000685e63` | 11-09 | Pitwall tests: private data folders so `admin.key` is stored on a Windows runner | S2 | none |
| `a121f5d91`, `e20ae50c3` | 11-08 | `paddock-service`: the elevated install and uninstall touch no user-controlled folder | S4 | none |
| `073e5a150` | 11-08 | Pitwall: the SYSTEM service writes no file a user can redirect (the LPE fix) | S4 | none |
| `6c6597f02` | 11-08 | Pitwall: the service is registered only for a protected copy in Program Files | S4 | none |
| `1d55c2726`, `2442ca3c2` | 11-08 | Dyno uninstaller runs the agent and helper only from an accepted folder | S4 | none |
| `0d141cbc8` | 11-05 | setup refusal note through the checked log, not `%TEMP%` | S2, S4 | `dyno-v1.1.0` |
| `fc140fc6f` | 11-04 | Dyno helper: create and lock by handle, verify before I/O | S1, S3, S4 | `dyno-v1.1.0` |
| `ac4a5edb0` | 11-05 | Dyno: folder rule before the prompt, junction check | S3, S4 | `dyno-v1.1.0` |

Every commit is an ancestor of the base. "none" means `git tag --contains` lists no tag: the fix is on the train only.

### What the code does today

**Copies of the mechanism (production code, not tests).** Each creates or narrows an owner-only folder or file its own
way:

| # | Where | What it does | Gaps |
|---|---|---|---|
| 1 | `crates/paddock-env/src/folder.rs` (`create_private_dir_all`, `check_folder`, `tighten_group_write`) | the "one folder-trust check" (added by `d4cc7bcdd`, not in any tag) | **Windows check is links and folder-ness only**: `read_folder` returns `uid 0, mode 0o700` for any folder ("Windows folders under the user's profile are private by their default permissions", lines 102-103 and 149-161) |
| 2 | `crates/paddock-env/src/private.rs` (`private_fallback_dir`) | `<temp>/<name>-<uid>` 0700, checked by handle | Unix only (fine: Windows is an error) |
| 3 | `crates/paddock-env/src/atomic.rs` `create` | temp file 0600 when `private` | folder by `create_dir_all` (umask); Windows: "the per-user folder is already private" (line 109) |
| 4 | `crates/paddock-env/src/dirs.rs` `create_private_dir` | `create_dir_all`, then `chmod 0700` by path, best effort | S1 window; chmods a folder the user may share |
| 5 | `crates/paddock-proc/src/hint.rs` `write_private_atomic` | discovery files (Dyno `agent.json`, MCP listener) | second atomic writer; no fsync; folder by `create_dir_all`; Windows inherits the parent |
| 6 | `crates/paddock-secrets/src/file.rs` `secure_folder` | 0700 on Unix; Windows checks the DACL (`acl::folder_refusal`) | Unix reads by path (`fs::metadata`) and **chmods an own world-writable folder to 0700** (lines 147-157); Windows creates with inherited ACL, then checks |
| 7 | `crates/paddock-secrets/src/file.rs` `make_folder_private` (pub) | 0700, or a protected DACL by name (`SetNamedSecurityInfoW`) | path-based; re-ACLs any existing folder; callers are tests only (`dyno-platform/src/wol/secrets.rs:40`, `pitwall-server/src/auth.rs:496`, `pitwall-server/tests/it/common/mod.rs:339`) |
| 8 | `crates/paddock-secrets/src/file_windows.rs` `create_owner_only` | secret file `D:P(A;;FA;;;<user>)`, no sharing | the right shape, for files only |
| 9 | `crates/paddock-mcp/src/key_list.rs` `secure_folder`, `untrusted` | Unix 0700 and a mode check by path | **Windows: no check and no creation rights at all** (lines 84-87, 110-111); identical at `dyno-v1.1.0` |
| 10 | `apps/debrief/crates/debrief-ipc/src/scratch.rs` `private_tempdir` | random name, 0700 on Unix | Windows inherits `%TEMP%`'s ACL |
| 11 | `apps/debrief/crates/debrief-ipc/src/endpoint.rs:157` | socket folder 0700, random name | Unix only (pipes on Windows) |
| 12 | `apps/dyno/crates/dyno-agent/src/wol.rs` `prepare_folder` | `create_dir_all`, then chmod 0700 by path, also when it existed wider | S1 window; re-chmods an existing folder |
| 13 | `apps/dyno/crates/dyno-platform/src/hardware.rs` `create_owner_only` | Windows `CreateDirectoryW` with `D:P(A;OICI;FA;;;OW)` | the right shape, a private copy |
| 14 | `apps/dyno/crates/dyno-agent/src/cache/db.rs`, `geo_fetch.rs:313`, `intel_fetch.rs:189` | three more chmod-after-create sites | S1 windows; Windows nothing |
| 15 | `apps/pitwall/crates/pitwall-server/src/paths.rs` `write_atomic`, `restrict`, `restrict_dir` | `apps.json` written to a **fixed** `apps.tmp` with `fs::write` (follows a link), chmod 0600 after the rename; Windows no-op | S1, S2; `admin.key` was written this way in `pitwall-v1.0.3` (`git show pitwall-v1.0.3:.../auth.rs` lines 116-117); on the train `admin.key` is a `SecretFile`, `apps.json` still uses it (`auth.rs:153-156`) |
| 16 | `apps/pitcrew/crates/pitcrew-service/src/ipc.rs:245-251` | socket folder `create_dir_all`, chmod 0700 ignoring failure | S1 window |

Three of them carry the Windows rule; ten are Unix-only; two are neither. The privileged side (S4) already has one
canonical rule in code, `paddock_service::privileged::trust` (the protected-chain rule of Dyno ADR 0022 §2.1, used by
the Dyno helper's `protect.rs` and Pitwall's `service/placement.rs`), but no family document states it: it lives in a
Dyno ADR, a Proposed family ADR (0053) on a branch, and Pitwall ADR 0024 by reference.

**Shipped weaknesses found while verifying** (not new findings of the fixes above, but worth naming):

- `dyno-v1.1.0` falls back to the fixed `std::env::temp_dir().join("Dyno")` when no `LOCALAPPDATA` or `HOME` is known
  (`git show dyno-v1.1.0:apps/dyno/crates/dyno-agent/src/paths.rs`, line 43). Fixed on the train by `a78d2fc94`.
- `dyno-v1.1.0`'s MCP key list has no Windows check (row 9). On Windows, `DYNO_DATA_DIR` (honoured in release builds,
  `paths.rs:98`) or portable mode (`paddock_env::PORTABLE_FLAG`, data beside the executable) can put trust-bearing files
  in a folder whose inherited ACL lets other accounts write, and nothing refuses it. Whether a stock non-system drive
  root grants Authenticated Users modify rights is read, not verified here; the check must not depend on it.
- `pitwall-v1.0.3` wrote `admin.key` through row 15 (S1 and S2).

These are low-likelihood on a single-user machine, which is the family's main case; they matter because Pitwall 1.1
adds a LocalSystem service and the family moves secrets out of the Keychain (ADR 0057).

### Why fixing instances has not converged

Each fix closed the shape it met on the OS it met it on: Unix modes first, Windows only where CI run 7821 hit a runner
folder. The same three steps (create private, verify by handle, refuse links) were written 16 times with different
gaps, and the "one folder-trust check" added on 2026-11-09 is itself a no-op on Windows.

## Decision (proposed)

### 1. One type: `paddock_env::private` (`PrivateDir`, `PrivateFile`)

A trust-bearing folder or file is created and opened only through these, in `paddock-env` (which `paddock-secrets`,
`paddock-mcp` and `paddock-proc` already sit above or beside; `paddock-proc::hint` moves its writer onto it):

- `PrivateDir::create(parent: &CheckedDir, name) -> PrivateDir` and `PrivateDir::create_fresh(parent, prefix)` (random
  name, never reused). Creation is atomic with its rights: Unix `mkdirat(parent_fd, name, 0o700)` then `openat` with
  `O_DIRECTORY | O_NOFOLLOW`; Windows `CreateDirectoryW` with a protected security descriptor in `SECURITY_ATTRIBUTES`
  (the descriptor applies at creation; with NULL the ACL is inherited from the parent, per
  <https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createdirectoryw>), then opened with
  `FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT` and refused if it is a reparse point.
- `PrivateDir::open(path, Links)` for an existing folder: opens by handle, reads owner and mode (Unix `fstat`) or owner
  and DACL (Windows `GetSecurityInfo` on the handle, the code now in `paddock-secrets/src/file_windows.rs` `security` and
  `acl.rs`), and returns a typed `Refusal` (the existing enum, gaining `OpenToOthersAcl { who }`). Windows stops being a
  no-op.
- The check covers the folder and its parent: the parent must not let others rename the folder (no group or other write
  on Unix unless sticky with the folder ours, the rule `check_temp_parent` already has; on Windows no `DELETE` on the
  folder or `FILE_DELETE_CHILD` on the parent for an untrusted principal, the rights Dyno ADR 0022 §2.1 lists).
- `PrivateDir::write_atomic(name, bytes)` and `PrivateDir::open_file(name)`: the one atomic writer (unique temp name,
  `O_CREAT | O_EXCL | O_NOFOLLOW` 0600 or the Windows owner-only unshared create of `file_windows.rs` row 8, fsync,
  `renameat` inside the held folder) and a reader that checks the file by handle. `paddock-env::write_atomic` with
  `private = true` and `paddock-proc::hint::write_private_atomic` become calls into it.
- `PrivateFile` keeps the parent handle, so every later step acts on the checked object (rule 53, ADR 0033).

`make_folder_private` and every chmod-after-create site are deleted from production code.

### 2. Existing folders: refuse with a fix-it message, never re-ACL what Paddock did not create

As the user decided: a folder the type finds open to others is refused with a message naming the reason and the next
step (`chmod go-w <folder>`, or on Windows "remove the entries for other users in the folder's Security tab, or point
<APP>_DATA_DIR at a folder of your own"). The only change Paddock makes to an existing folder is the narrow case
`tighten_group_write` already proves safe (`b4e60e9c9`): the app's own data root, owned by the account, group write only,
group named like the account with no other member. `paddock-secrets`' and `paddock-mcp`'s "chmod an own folder to 0700"
(rows 6, 9) and Dyno's `prepare_folder` (row 12) stop doing it. `make_folder_private` survives only under
`cfg(any(test, feature = "test-support"))` (ADR 0073), used by `paddock_testkit::scratch` (ADR 0078) to make a test's
folder private on a CI runner.

### 3. The register and check C106

`docs/trust-files.toml` lists each trust-bearing file and folder: id, owning crate, path expression, kind
(`secret`, `grant-list`, `discovery`, `scratch`), the writer and reader functions, and the OS set. Check C106 (a syn
pass on the RT-5 host, with a ratchet from ADR 0088) fails a non-test file that calls `fs::set_permissions`,
`DirBuilderExt::mode`, `OpenOptionsExt::mode`, `SetNamedSecurityInfoW`, `CreateDirectoryW` or `SetFileSecurityW` outside
`paddock_env::private`, and a register entry whose writer or reader does not reach `PrivateDir`/`PrivateFile`. Clippy
`disallowed-methods` carries the same list, as for kills (ADR 0033). A planted bad file is its positive control (ER-33).
Known non-secret uses (an executable bit on an unpacked plugin, `debrief-plugin-package/src/archive.rs:280`) are register
lines with a reason, not exemptions in code.

### 4. The elevated and protected-location rules become a family rule

Proposed rule 75: "A process with more rights than the user who started it never reads, writes, runs or follows a link in
a place that user can change; what it runs passes the protected-chain rule (`paddock_service::privileged::trust`), and
where it stages or logs is a protected folder it created (ADR 0053 §2.5)." The text of Dyno ADR 0022 §2.1 moves into a
family document this ADR links (when 0053 is accepted, its §2.5 is the per-machine half); the app ADRs then link instead
of restating. The code is already shared; this makes the rule binding for the next app with a helper or service
(Pitcrew 1.1, ADR 0072 M0) and gives C53-style review a text to cite.

### 5. Copies leave in the adopting change

Each row of the copies table gets a `docs/known-copies.toml` entry with `canonical = "paddock_env::private"` and a
delete-by tag: shared rows and Pitwall's at `pitwall-v1.1.0`, Dyno's at the next Dyno tag, Debrief's and Pitcrew's at
`debrief-v1.2.0` and `pitcrew-v1.1.0`. A later fix to the mechanism follows rule 55.

### The failing tests the work starts with (ER-11)

1. `paddock-env`, Windows: a folder whose DACL grants `Everyone` write (`D:P(A;OICI;FA;;;<me>)(A;OICI;FA;;;WD)`) passes
   `check_own_folder` today; it must be refused with `OpenToOthersAcl`.
2. `paddock-mcp`, Windows: `FileKeys::save` into such a folder succeeds and `load` trusts the list today; both must refuse.
3. `paddock-secrets`, Unix: `FileSecrets` over an existing own folder with mode 0777 chmods it to 0700 and proceeds today;
   it must refuse and leave the mode unchanged.
4. Unix, every writer: a link planted at the temp name (`apps.tmp` for Pitwall's `apps.json`) is followed today; the
   write must fail with the target untouched.
5. Both OS: a folder created through the type under umask 002 (Unix) or under a parent granting Authenticated Users
   modify (Windows) has owner-only rights from its first `fstat` / `GetSecurityInfo`, with no later narrowing call.

## Options considered

| Option | Licence | Windows | Creates atomically | Effort | Verdict |
|---|---|---|---|---|---|
| **A. `paddock_env::private`, built from the code in rows 1, 2, 8, 13** | ours | yes (moved from `paddock-secrets`) | yes | 21 days for all | recommended |
| B. `fs-mistrust` (Tor project) for the checks, ours for creation | MIT OR Apache-2.0 | no: "On Windows, we accept all file permissions and owners" (<https://docs.rs/fs-mistrust/latest/fs_mistrust/>, read 2026-11-10, version 0.16.1) | `make_directory`, Unix modes only | similar, plus a second model of trust | not chosen: the gap this ADR closes is Windows |
| C. Keep fixing instances | - | partly | partly | unbounded | not chosen: 23 fixes in six days, the latest shared check is a Windows no-op |

`fs-mistrust` checks every ancestor and follows symlink targets' ancestors; D4 below weighs that against our folder plus
parent rule. `cap-std` (handle-relative `Dir`) was not evaluated here; it would replace the `openat` plumbing, not the
Windows ACL work, so it does not change the recommendation (unverified).

Privacy and performance: the type adds one `fstat` or `GetSecurityInfo` per open of a trust-bearing file, a few per
start; not measured, no bulk or long operation is involved, so there is no "Parallelism and cost" section. Logs keep
naming reasons without paths (ER-27, as `note_result` does now).

## Decisions for the user

- **D1 Mechanism.** (a) **One `PrivateDir`/`PrivateFile` type in `paddock-env`, every writer through it** · (b)
  `fs-mistrust` for checks plus our creation · (c) keep fixing instances.
- **D2 An existing folder open to others.** (a) **Refuse with a fix-it message; the only change Paddock makes is the
  own-private-group `tighten_group_write` case for the app's data root** · (b) refuse always, no tightening · (c) narrow
  any own folder (today's rows 6, 9, 12). (a) and (b) both honour the user's earlier decision (no re-ACL of a folder Paddock did not create); (a) keeps Linux
  users with umask 002 working.
- **D3 The Windows descriptor.** (a) user SID only · (b) **user SID and SYSTEM, explicit SID, protected, `OICI`** ·
  (c) user, SYSTEM and Administrators (today's `make_private`). An explicit SID rather than `OW` (OWNER RIGHTS, Dyno row 13)
  because inherited `OW` entries apply to whoever owns each child. Whether a SYSTEM entry is needed (backup and search
  services) is not verified here; (a) is the strictest and Administrators can take ownership in any case.
- **D4 How far up the check looks.** (a) **the folder and its parent** (who can rename it) · (b) every ancestor, as
  `fs-mistrust` · (c) the folder only (today). (b) refuses legitimate setups (a data folder under a shared mount) for
  little gain once the parent is checked.
- **D5 Enforcement.** (a) **register `docs/trust-files.toml` plus C106 and Clippy `disallowed-methods`, ratcheted** ·
  (b) Clippy only · (c) review only.
- **D6 Copies.** (a) **delete every copy in its app's adopting change, registered with delete-by tags (§5)** · (b)
  migrate shared crates only, apps at leisure.
- **D7 `make_folder_private`.** (a) **test-support only, behind `test-support` (as the user decided)** · (b) keep public.
- **D8 Elevated and protected locations.** (a) **a family rule 75 with the protected-chain text lifted from Dyno ADR
  0022 §2.1, app ADRs link it** · (b) leave it in the app ADRs and 0053.
- **D9 What it gates.** (a) **the Pitwall 1.1 tag on WP-1 to WP-5, WP-8 and WP-10; the next Dyno tag on WP-6; Debrief
  1.2 and Pitcrew 1.1 on WP-7** · (b) Pitwall 1.1 only · (c) gate nothing, schedule after the releases.

Why Pitwall 1.1 (D9): it is the next tag that ships new trust-bearing files on Windows (`admin.key`, `apps.json`, the
chat host's `FileSecrets` and prefs, `pitwall-server/src/auth.rs`, `chat_host/commands.rs:46`, `chat_host/chats.rs:132`)
next to a LocalSystem service (`073e5a150`, `6c6597f02`), and its `apps.json` writer (row 15) is the weakest copy left. The
next Dyno tag (after `dyno-v1.1.0`, 2026-11-08) ships the MCP key list and `agent.json`, whose Windows checks are
absent (rows 1, 5, 9); its fixes `a78d2fc94`, `8990ed7f4` and `d4cc7bcdd` are already on the train and the type
replaces them without new behaviour, so it needs only WP-6. Debrief 1.2 and Pitcrew 1.1 are months out and carry rows
10, 11 and 16.

## Work packages

| WP | What | Days | Gates |
|---|---|---|---|
| WP-1 | `paddock_env::private`: `PrivateDir`, `PrivateFile`, `CheckedDir` on Unix (`mkdirat`/`openat`/`fstat`/`renameat`) and Windows (`CreateDirectoryW` with descriptor, handle open with reparse refusal, `GetSecurityInfo`), Windows code moved from `paddock-secrets`; failing tests 1 and 5 first | 4 | Pitwall 1.1 |
| WP-2 | Existing folders: `Refusal::OpenToOthersAcl`, parent rule (D4), fix-it texts, `tighten_group_write` kept for the data root only; `check_folder` real on Windows | 2 | Pitwall 1.1 |
| WP-3 | `docs/trust-files.toml`, C106 on the RT-5 host with an ADR 0088 ratchet, Clippy `disallowed-methods`, planted positive control | 3 | Pitwall 1.1 |
| WP-4 | Shared adopters: `paddock-env` atomic writer and `dirs`, `paddock-proc::hint`, `paddock-secrets` (`secure_folder` refuses, no chmod), `paddock-mcp` key list (gains Windows); failing tests 2 and 3 first | 3 | Pitwall 1.1 |
| WP-5 | Pitwall: `apps.json` through `PrivateDir::write_atomic` (failing test 4 first), `admin.key` folder, chat secrets and prefs, slots folder, `cache_db`; delete `paths::write_atomic`, `restrict`, `restrict_dir` | 2 | Pitwall 1.1 |
| WP-6 | Dyno: data root, `agent.json`, WoL folder, cache db, geo and intel caches, battery folder (`hardware.rs` copy deleted) | 2 | next Dyno |
| WP-7 | Debrief `private_tempdir` and IPC socket folder, Pitcrew socket folder | 1.5 | Debrief 1.2, Pitcrew 1.1 |
| WP-8 | `make_folder_private` behind `test-support`; tests through `paddock_testkit::scratch` | 1 | Pitwall 1.1 |
| WP-9 | Rule 75 text and the family protected-location document; Dyno ADR 0022, Pitwall ADR 0024 and 0053 link it | 1 | none (docs) |
| WP-10 | Windows positive controls in CI and the test VM: a planted junction, an Everyone-writable folder, `DYNO_DATA_DIR` and portable mode on a folder others can write | 1.5 | Pitwall 1.1 |

Total 21 days; the Pitwall 1.1 cut (WP-1 to WP-5, WP-8, WP-10) is 16.5 days. WP-1 comes first; WP-2 and WP-3 can run beside
WP-4 once WP-1's API is fixed; WP-5 to WP-7 are independent of each other.

## Consequences

- One place creates and checks trust-bearing files on both OS; Windows gets the checks Unix has had since 2026-11-06.
- A user whose data folder is open to others sees a refusal with the step to fix it, instead of a silent chmod. Linux
  users with umask 002 and a private group keep working (D2 (a)).
- Sixteen copies leave; the copies register tracks them until their tags.
- Risk: the Windows handle and descriptor code is unsafe FFI; it is moved, not rewritten, from `paddock-secrets`, which
  already has Windows tests on the CI runner.
- Risk: a folder the user shares on purpose (a synced folder used as `DYNO_DATA_DIR`) is now refused on Windows where it
  passed before. That is the intent; the message names the override.
