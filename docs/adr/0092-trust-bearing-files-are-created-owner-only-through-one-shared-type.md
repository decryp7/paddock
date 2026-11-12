# 92. Trust-bearing files are created owner-only through one shared type

- Status: Accepted 2026-11-10 (user); D9 (a), D2 tighten default only; other decisions as recommended. **D9 changed
  2026-11-11 (user): from (a) to the minimum cut, (d): only WP-0 (about 3.5 days; it closes W3, W4 and W5, and W1 is fixed
  by `073e5a150`) gates Pitwall 1.1. WP-1 (`paddock_env::private`) is built (`feat/private-files-type`, on train 5) and
  stays; the rest of this ADR is later. Debrief W2 (the elevated bootstrap) is covered by the elevation risk warning and
  no longer blocks Debrief 1.2 (user, 2026-11-11); WP-7's Pitcrew socket-folder part stays a Pitcrew 1.1 item.** Independent
  review approved revision 2 (`fdfb17c8f`); this text adds the review's last five P2s ("Review answers (acceptance)").
  Revision 2 of the proposal. Drafted at the user's request of 2026-11-10 under ER-21 (rule 40): one
  class, about 23 fixes in six days. Revision 2 answers the independent review of revision 1 (`3ba5a4a16`, CHANGES,
  4 P1s and 8 P2s; "Review answers (revision 2)" at the end). Nothing here is built yet. ER-23 does not apply (no
  drawings).
- Trimmed 2026-11-11 (user, comparable-practice review): TRIM beyond WP-1 and the Pitwall minimum: keep WP-0, WP-1 (built), WP-4 (shared adopters), WP-6 Dyno data root, WP-7 (Debrief elevated bootstrap and Pitcrew socket folder; the W-rows that block Pitcrew 1.1 stay, the user decided Pitcrew's SYSTEM-service fix is in Pitcrew 1.1), WP-8, WP-9 and the Clippy `disallowed-methods` entries of WP-3; dropped C106 and its register (rest of WP-3), WP-2 (extended ACLs), WP-10, Dyno's other adopters.
- Date: 2026-11-10
- Related: ER-21 / rule 40; rule 53 (reusable OS names) and ADR 0033 (identity by handle); rule 55 and ADR 0034/0075
  (copies register); ADR 0073 (no test doubles in a shipped build: the `test-support` feature); ADR 0077 (tests never
  reach system services) and ADR 0078 (one shared test scratch); ADR 0088 (one ratchet engine; accepted on
  `origin/docs/adr-windows-gate-ratchets` only, engine **not built**); the RT-5 check host (`origin/feat/rt5-check-host`,
  **not landed**); family ADR 0053 (Proposed, only on `origin/docs/dyno-adr-0027-per-machine`: per-machine installs,
  §2.5 where elevated code stages); Dyno ADR 0022 §2.1 (the protected-location rule) and Dyno ADR 0027 §3; Pitwall ADR
  0024 (Pitwall adopts 0053); ADR 0057 (Proposed, `origin/docs/adr-keychain-family`: no macOS Keychain, so more secrets
  become files).
- Number: 0092 was free on every `origin` branch on 2026-11-10 (the highest files are `0090` on
  `origin/docs/adr-captured-tool-output` and `0091` on `origin/docs/adr-evidence-heads`; no branch's `docs/` names
  "ADR 0092"; no ADR worktree under `/Volumes/d7sst7/dev-targets/wt/` holds a 0092 file). Check id C106: C105 (ADR 0091)
  is the highest check id named on any `origin` branch. Rule number 75: rules 73 and 74 are named by ADRs 0073 and 0076,
  no `origin` branch names a rule 75 (`git grep -hoE 'rule 7[5-9]|C10[6-9]|C11[0-9]'` over `docs`, `conformance` and
  `tools` of all 364 `origin` refs found nothing).
- Evidence: read on 2026-11-10 with `git show`, `git log`, `git grep`, `git merge-base --is-ancestor` and
  `git tag --contains` after `git fetch`. Paths without a branch are from this ADR's base `origin/train/2026-11-09-3` (read at `67a633ef3`; its moves to `c151422a4` touch none of the cited paths).
  Nothing was built or run; no user data was read. The heads, re-read right before the commit:

```evidence-heads
read: 2026-11-10
origin/train/2026-11-09-3 c151422a4
origin/main b51c72509
origin/docs/dyno-adr-0027-per-machine 3a6be49b5
origin/docs/adr-evidence-heads b32655fef
origin/docs/adr-keychain-family d37b7f452
origin/docs/adr-windows-gate-ratchets db7782e01
origin/feat/rt5-check-host ec8530a95
```

## Context

### The class

A **trust-bearing file** is a file or folder whose content another process or a later run of the app believes: a
secret (API keys, the Wake-on-LAN SecureOn password, Pitwall's `admin.key`), a list that grants access (the MCP client
key list, Pitwall's `apps.json` with key hashes), a discovery or bootstrap file that names a pid, port, endpoint or token
(Dyno's `agent.json`, the MCP listener's file, Debrief's elevated `bootstrap.json`), or a folder those live in,
including scratch folders a worker reads back. The bug is always one of four shapes:

- **S1 created open:** the file or folder is created with the umask's or the parent's rights and narrowed afterwards
  (or never), so for a moment, or for good, others can read or replace it.
- **S2 created in a place others control:** a fixed or guessable name in a shared folder (`/tmp/Dyno`, `%TEMP%`,
  `C:\ProgramData`, a CI runner's work folder), which another account can create first or link elsewhere.
- **S3 trusted without a check by handle:** the owner and rights are read by path (`fs::metadata`, `SetNamedSecurityInfoW`)
  and the file is then opened by path, or not checked at all on one OS.
- **S4 a process with more rights reads or writes where a process with fewer rights can change things**: an installer,
  uninstaller or SYSTEM service following a link the user planted, or an elevated child reading its input from a file the
  same user's unelevated processes can rewrite.

### What the type can and cannot defend against

Owner, mode and DACL checks separate **accounts**. A process of the same user at medium integrity passes every one of
them: it is the owner. For data the user's own processes read at the user's own level that is the right boundary
(anything running as the user can already read the user's memory and files). It is **not** a boundary for an elevated
reader (S4): an elevated process that reads a file in a folder the user owns can be fed by any unelevated process of that
user, which is the UAC-bypass shape of the Debrief environment-override finding the user made a Debrief 1.2 blocker
(user decision, 2026-11-09). §4 (rule 75) therefore governs S4 separately: no owner-only
file is a sufficient channel into an elevated process.

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

**Copies of the mechanism in production code: 15 rows, 17 call sites** (row 14 is three sites). A row **carries the
Windows rule** when, on Windows, it either creates the object with an owner-only security descriptor or refuses an
existing one after reading its owner and DACL from a handle; rows 6, 8 and 13 do, the others do neither.

| # | Where | What it does | Gaps |
|---|---|---|---|
| 1 | `crates/paddock-env/src/folder.rs` (`create_private_dir_all`, `check_folder`, `tighten_group_write`) | the "one folder-trust check" (added by `d4cc7bcdd`, not in any tag) | **Windows check is links and folder-ness only**: `read_folder` returns `uid 0, mode 0o700` for any folder ("Windows folders under the user's profile are private by their default permissions", lines 102-103 and 149-161) |
| 2 | `crates/paddock-env/src/private.rs` (`private_fallback_dir`) | `<temp>/<name>-<uid>` 0700, checked by handle | Unix only (fine: Windows is an error) |
| 3 | `crates/paddock-env/src/atomic.rs` `create` | temp file 0600 when `private` | folder by `create_dir_all` (umask); Windows: "the per-user folder is already private" (line 109) |
| 4 | `crates/paddock-env/src/dirs.rs` `create_private_dir` | `create_dir_all`, then `chmod 0700` by path, best effort | S1 window; chmods a folder the user may share |
| 5 | `crates/paddock-proc/src/hint.rs` `write_private_atomic` | discovery files (Dyno `agent.json`, MCP listener) | second atomic writer; no fsync; folder by `create_dir_all`; Windows inherits the parent |
| 6 | `crates/paddock-secrets/src/file.rs` `secure_folder` | 0700 on Unix; Windows checks the DACL (`acl::folder_refusal`) | Unix reads by path (`fs::metadata`) and chmods an own world-writable folder to 0700 (lines 147-157); Windows creates with the inherited ACL, then checks; `folder_security` (`file_windows.rs:321-327`) opens without `FILE_FLAG_OPEN_REPARSE_POINT`, so it reads a junction's target |
| 8 | `crates/paddock-secrets/src/file_windows.rs` `create_owner_only` | secret file `D:P(A;;FA;;;<user>)`, no sharing | the right shape, for files only; path-based (ancestors not anchored) |
| 9 | `crates/paddock-mcp/src/key_list.rs` `secure_folder`, `untrusted` | Unix 0700 and a mode check by path | **Windows: no check and no creation rights at all** (lines 84-87, 110-111); identical at `dyno-v1.1.0` |
| 10 | `apps/debrief/crates/debrief-ipc/src/scratch.rs` `private_tempdir` | random name, 0700 on Unix | Windows inherits `%TEMP%`'s ACL |
| 11 | `apps/debrief/crates/debrief-ipc/src/endpoint.rs:157` | socket folder 0700, random name | Unix only (pipes on Windows) |
| 12 | `apps/dyno/crates/dyno-agent/src/wol.rs` `prepare_folder` (moved onto the type by WP-6, `fix/dyno-adr0092-wp6`) | `create_dir_all`, then chmod 0700 by path, also when it existed wider | S1 window; re-chmods an existing folder |
| 13 | `apps/dyno/crates/dyno-platform/src/hardware.rs` `create_owner_only` (deleted by WP-6: `fresh_private_folder` uses `PrivateDir`) | Windows `CreateDirectoryW` with `D:P(A;OICI;FA;;;OW)` | the right descriptor, path-based, a private copy |
| 14 | `apps/dyno/crates/dyno-agent/src/cache/db.rs`, `geo_fetch.rs:313`, `intel_fetch.rs:189` (moved onto the type by WP-6) | three chmod-after-create sites | S1 windows; Windows nothing |
| 15 | `apps/pitwall/crates/pitwall-server/src/paths.rs` `write_atomic`, `restrict`, `restrict_dir` | `apps.json` written to a **fixed** `apps.tmp` with `fs::write` (follows a link), chmod 0600 after the rename; Windows no-op | S1, S2 |
| 16 | `apps/pitcrew/crates/pitcrew-service/src/ipc.rs:245-251` | socket folder `create_dir_all`, chmod 0700 ignoring failure | S1 window |

Row 7 of revision 1, `paddock_secrets::make_folder_private` (pub, path-based, re-ACLs any existing folder), has only test
callers (`dyno-platform/src/wol/secrets.rs:40`, `pitwall-server/src/auth.rs:496`,
`pitwall-server/tests/it/common/mod.rs:339`); it is test support, handled by D7, not a production copy.

Also on Pitwall's key path: `paths::random32` (`paths.rs:159-170`) falls back to a time-seeded value when `getrandom`
fails, and that value becomes key material (`auth.rs:31`, `:242`, `:270`, `:410`); the same code is in `pitwall-v1.0.3`
(`paths.rs:155`). It must fail closed.

The privileged side (S4) already has one canonical rule in code, `paddock_service::privileged::trust` (the
protected-chain rule of Dyno ADR 0022 §2.1, used by the Dyno helper's `protect.rs` and Pitwall's
`service/placement.rs`), but no family document states it.

### Weaknesses in shipped or train code, rated, with the release that closes each

| # | Where | What | Rating | Closed by |
|---|---|---|---|---|
| W1 | `pitwall-v1.0.3` LocalSystem service log (`git show pitwall-v1.0.3:.../service/windows.rs` lines 488-505) | `%ProgramData%\Pitwall\logs\service.log` made with `create_dir_all`, then size-checked, removed and opened **by path** as SYSTEM: a user who pre-creates the folder plants a link and the service writes or deletes where the link points | **High** (local privilege escalation) where the opt-in service is installed | `073e5a150` on the train, in **Pitwall 1.1**. The user decided on 2026-11-08 to ship the fix in 1.1 only, no 1.0.4, (user decision, 2026-11-08), and kept it after foundation-first pushed 1.1 to about January. Every day 1.1 is later keeps W1 live on installs with the service |
| W2 | Debrief elevated bootstrap, `debrief-v1.0.0` to `debrief-v1.1.1` and the train (`apps/debrief/crates/debrief-ipc/src/elevation.rs:104-124`) | the unelevated side writes the endpoint, token and sandbox spec to `%TEMP%\debrief-elevate-<random>\bootstrap.json` (at `debrief-v1.1.1` under `temp_dir()` with 8 random bytes, line 113; on the train through `private_tempdir`); the elevated child reads it by path. Any unelevated process of the same user can rewrite it between write and read (the type's limit above) | **Medium**: needs code already running as the user; it crosses the UAC boundary, the same shape as the env-override bypass already blocking Debrief 1.2 | no longer a Debrief 1.2 blocker (user, 2026-11-11): covered by the elevation risk warning; the fix (WP-7: input through a channel rule 75 allows) is later |
| W3 | `pitwall-v1.0.3` `admin.key` (`auth.rs:110-119` at that tag) | written to a fixed temp name, renamed, then chmod 0600: on Linux, under a 0644 default, another account watching with inotify can read the key in the window | **Low to medium** (Linux, multi-user) | the train writes it through `SecretFile`, but **keeps an existing key** (`auth.rs:24-26`), so a key read in 1.0.3 survives. Pitwall 1.1: rotate the admin key once on the first start after an upgrade from 1.0.3 or earlier (WP-0), or say it in the release notes (D11) |
| W4 | `apps.json`, `pitwall-v1.0.3` and the train (`auth.rs:134`, `:153-156`; `paths.rs:106-116`) | fixed `apps.tmp`, `fs::write` follows a link, chmod after rename; and the `.bak` fallback: when the primary is damaged the backup is used, which can bring back a key revoked since | **Low to medium** (integrity of the access list) | **Pitwall 1.1** (WP-0) |
| W5 | `pitwall-v1.0.3` and train `paths::random32` | time-seeded key material when the OS RNG fails | **Low** (an RNG failure is rare) but fails open | **Pitwall 1.1** (WP-0) |
| W6 | `dyno-v1.1.0` data folder fallback (`paths.rs` line 43 at that tag) | fixed `temp_dir()/Dyno` when no `LOCALAPPDATA` or `HOME` is known | **Low** | `a78d2fc94` on the train; **next Dyno tag** |
| W7 | `dyno-v1.1.0` MCP key list on Windows | no check, no owner-only creation. It lives in `AppDirs::config()` (`apps/dyno/app/src-tauri/src/mcp.rs:147`), which `DYNO_DATA_DIR` does **not** move; only portable mode (data beside the executable) or `<ID>_HOME` (debug builds only) puts it outside `%LOCALAPPDATA%` | **Low** (portable mode on a folder others can write) | deferred 2026-11-11 (user): the risk is accepted; was the next Dyno tag (WP-6) |
| W8 | `DYNO_DATA_DIR` on Windows (`paths.rs:98`, honoured in release builds) | the agent's data folder is not checked against others' rights | **Low** (the user sets it) | **next Dyno tag** (WP-6) |
| W9 | `paddock-secrets` and `paddock-mcp` on Unix (rows 6, 9) | an own folder open to others is chmodded to 0700 and used | **Hardening only**. The reason to refuse instead: tightening cannot evict what was planted while the folder was open (a file, a link, a subfolder another account owns) | the train, ungated (WP-4) |

## Decision (proposed)

### 1. One type: `paddock_env::private` (`PrivateDir`, `PrivateFile`)

A trust-bearing folder or file is created and opened only through these, in `paddock-env` (which `paddock-secrets`,
`paddock-mcp` and `paddock-proc` already sit above or beside; `paddock-proc::hint` moves its writer onto it).

**Anchoring.** Every operation after the first is relative to a held folder handle, never a path, so an ancestor that
is renamed, replaced or turned into a junction afterwards changes which path names the folder, not what the handle
refers to:

- **Unix:** `mkdirat(parent_fd, name, 0o700)`, `openat` with `O_DIRECTORY | O_NOFOLLOW`, then `fchmod(fd, 0o700)` on the
  fresh handle (the mode is then exact whatever the umask), `fstat` for owner and mode. Files: `openat` with
  `O_CREAT | O_EXCL | O_NOFOLLOW` and 0600, `fchmod`, fsync, `renameat` inside the held folder. On macOS, extended ACLs
  are read with `acl_get_fd` and a folder or file with an allow entry for another principal is refused (Linux POSIX
  ACLs show in the group bits through the mask, which the mode check already covers).
- **Windows**, D10 (a), recommended: folders are created with `NtCreateFile` (`FILE_DIRECTORY_FILE`, `FILE_CREATE`,
  `FILE_OPEN_REPARSE_POINT`) with `OBJECT_ATTRIBUTES.RootDirectory` set to the held parent handle and the owner-only
  security descriptor in `OBJECT_ATTRIBUTES.SecurityDescriptor`; files likewise, without sharing; the atomic replace is
  `SetFileInformationByHandle(FileRenameInfo)` with `RootDirectory` set to the held folder. Path-based `CreateDirectoryW`
  applies the descriptor at creation (with NULL "the ACLs ... are inherited from its parent directory",
  <https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createdirectoryw>) but resolves every
  ancestor by name, following junctions, so it is not enough. D10 (b), the fallback: hold each folder handle opened with
  `FILE_FLAG_OPEN_REPARSE_POINT` and without `FILE_SHARE_DELETE` (it cannot be renamed or deleted while held), and
  compare `GetFinalPathNameByHandleW` with the expected path before each child operation by path.
- **cap-std first.** `cap-std` 4.0.3 (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT, Bytecode Alliance; read
  2026-11-10 at <https://docs.rs/cap-std/latest/cap_std/fs/struct.DirBuilder.html> and
  <https://github.com/bytecodealliance/cap-std>) gives handle-relative `Dir` operations on Unix (`openat`, a
  `DirBuilderExt::mode`) and on Windows opens relative to a directory handle through its own `CreateFileAtW`, passing the
  caller's security attributes through (`cap-primitives/src/windows/fs/open_unchecked.rs`). Not verified: whether
  `CreateFileAtW` sets `RootDirectory` (the helper was not read), and whether it can create a folder with a descriptor
  (`DirBuilder` documents no Windows option). WP-1 starts with a one-day spike: if cap-std covers the Unix side and
  Windows file opens, the type wraps it and adds only the Windows folder creation and the descriptor checks; if not, the
  type calls `NtCreateFile` itself (the `windows` crate is already a dependency of `paddock-secrets`).

**Checks.** `PrivateDir::open(path, Links)` opens by handle with links and junctions refused (`O_NOFOLLOW`;
`FILE_FLAG_OPEN_REPARSE_POINT` and a refusal when the attributes say reparse point, which today's `folder_security`
lacks), reads owner and mode or owner and DACL from the handle (the code now in `paddock-secrets/src/file_windows.rs`
`security` and `acl.rs`), and returns the existing `Refusal` enum, gaining `OpenToOthersAcl { who }`. Windows stops
being a no-op. The parent is checked too (D4): no other account may rename or replace the folder (no group or other
write on Unix unless sticky with the folder ours, as `check_temp_parent`; on Windows no `DELETE` on the folder or
`FILE_DELETE_CHILD` on the parent for an untrusted principal, the rights Dyno ADR 0022 §2.1 lists).

**Writing.** `PrivateDir::write_atomic(name, bytes)` is the one atomic writer (unique temp name, created exclusive and
owner-only, fsync, rename inside the held folder); `PrivateDir::open_file(name)` reads with a check by handle.
`paddock-env::write_atomic(private = true)` and `paddock-proc::hint::write_private_atomic` become calls into it.
`PrivateFile` keeps the parent handle, so every later step acts on the checked object (rule 53, ADR 0033).

Every chmod-after-create site is deleted from production code.

### 2. Existing folders: refuse with a fix-it message

A folder the type finds open to others is refused with a message naming the reason and the next step (`chmod go-w
<folder>`, or on Windows "remove the entries for other users in the folder's Security tab, or point <APP>_DATA_DIR at a
folder of your own"). Tightening is not a fix: it cannot evict what was planted while the folder was open. The one open
question is D2: whether Paddock may still remove group write from the app's **default** data location (never an
override, never a folder the user named) in the narrow case `tighten_group_write` proves (`b4e60e9c9`: owned by the
account, group write only, a group named like the account with no other member). `paddock-secrets`' and `paddock-mcp`'s
"chmod an own folder to 0700" (rows 6, 9) and Dyno's `prepare_folder` (row 12) stop in every option.
`make_folder_private` survives only under `cfg(any(test, feature = "test-support"))` (ADR 0073), used by
`paddock_testkit::scratch` (ADR 0078) to make a test's folder private on a CI runner.

### 3. The register and check C106

`docs/trust-files.toml` lists each trust-bearing file and folder: id, owning crate, path expression, kind (`secret`,
`grant-list`, `discovery`, `bootstrap`, `scratch`), the writer and reader functions, and the OS set.

C106 is a syn pass that runs on the RT-5 check host with an ADR 0088 ratchet. Both are **unbuilt dependencies**: ADR
0088 is accepted on `origin/docs/adr-windows-gate-ratchets` only and its engine does not exist; the RT-5 host is on
`origin/feat/rt5-check-host`, not landed. WP-3 cannot start before both land; until then Clippy `disallowed-methods`
carries the API list alone. The check fails a non-test file that, outside `paddock_env::private`, calls any of:

- Unix: `fs::set_permissions`, `File::set_permissions`, `DirBuilderExt::mode`, `OpenOptionsExt::mode`, and `libc`
  `chmod`, `fchmod`, `fchmodat`, `mkdir`, `mkdirat`, and `open`/`openat` with a mode argument;
- Windows: `CreateDirectoryW`, `CreateDirectoryExW`, `CreateFileW` and `NtCreateFile` with security attributes,
  `SetNamedSecurityInfoW`, `SetSecurityInfo`, `SetFileSecurityW`, `SetKernelObjectSecurity`.

The same list triggers the register: any file that marks something private through `paddock_env::private` must name a
register entry (the writer function appears in one), so an unregistered trust-bearing file is caught, not only a raw
API call. A register entry whose writer or reader does not reach `PrivateDir`/`PrivateFile` fails too. A planted bad
file is its positive control (ER-33). Known non-secret uses (an executable bit on an unpacked plugin,
`debrief-plugin-package/src/archive.rs:280`) are register lines with a reason, not exemptions in code.

### 4. Rule 75: more rights never read what fewer rights can change

Proposed rule 75: "A process with more rights than the user who started it (elevated, SYSTEM, root) never reads, writes,
runs or follows a link in a place that user, or any process of that user at a lower integrity level, can change. What it
runs passes the protected-chain rule (`paddock_service::privileged::trust`); where it stages or logs is a protected folder
it created (ADR 0053 §2.5). It gets its input only through a channel the lower-integrity side cannot change after the
fact: a handle it inherits, a pipe whose server it has verified (the server process by identity, ADR 0033), or a folder
the elevated side created with a High mandatory label (`S:(ML;OICI;NWNRNX;;;HI)`) and owner-only DACL, which it then uses
only relative to the handle it created, or after re-reading the label and owner from the handle it opened. An owner-only
file in the user's folders is not such a channel."

Note (2026-11-11, user): user-chosen elevation (Run or Restart as administrator) is governed by rule 77, not rule 75:
it needs only a clear risk warning (ADR 0073 amendment `amendments/0073-elevated-helper-2026-11-11.md`). Rule 75 binds
privileged services and helpers that run on their own.

The last clause closes a rename hole: the folder's parent is the user's, so a medium-integrity process holding
`FILE_DELETE_CHILD` there can rename the High-label folder away and put its own in its place; only a check on the handle
the elevated side holds sees that. For input that flows from low to high, as W2's does, the label channel does not apply
(the low side writes the input), and handles are not inherited across a `runas` elevation, so the channel is the verified
pipe: the unelevated side creates the pipe, the pipe's address goes in the elevated child's arguments, and the child
checks `GetNamedPipeServerProcessId` against the requester's pid and start time passed with it (rule 53,
`paddock_proc::ProcessId`) before it reads anything.

The text of Dyno ADR 0022 §2.1 moves into a family document this ADR links (when 0053 is accepted, its §2.5 is the
per-machine half); the app ADRs then link instead of restating. The code is already shared; this makes the rule binding
for Debrief's elevated child (W2), and for the next app with a helper or service (Pitcrew 1.1, ADR 0072 M0).

### 5. Copies leave in the adopting change

Each production row gets a `docs/known-copies.toml` entry with `canonical = "paddock_env::private"` and a delete-by
tag: shared rows and Pitwall's at `pitwall-v1.1.0` where D9 gates them, otherwise the next tag of each app that ships
them; Dyno's at the next Dyno tag; Debrief's and Pitcrew's at `debrief-v1.2.0` and `pitcrew-v1.1.0`. A later fix to the
mechanism follows rule 55.

### The failing tests the work starts with (ER-11)

1. `paddock-env`, Windows: a folder whose DACL grants `Everyone` write (`D:P(A;OICI;FA;;;<me>)(A;OICI;FA;;;WD)`) passes
   `check_own_folder` today; it must be refused with `OpenToOthersAcl`.
2. `paddock-mcp`, Windows: `FileKeys::save` into such a folder succeeds and `load` trusts the list today; both must refuse.
3. `paddock-secrets`, Unix: `FileSecrets` over an existing own folder with mode 0777 chmods it to 0700 and proceeds today;
   it must refuse and leave the mode unchanged.
4. Pitwall, Unix: a link planted at `apps.tmp` is followed by `paths::write_atomic` today; the write must fail with the
   target untouched. And: a damaged `apps.json` with a `.bak` that still holds a revoked key loads the key today; it
   must load nothing and say so.
5. Pitwall: `random32` with a failing RNG (an injected source) returns bytes today; it must return an error and no key.
6. Both OS: a folder created through the type under umask 002 (Unix) or under a parent granting Authenticated Users
   modify (Windows) has owner-only rights from its first `fstat` / `GetSecurityInfo`, with no later narrowing call.
7. Windows: with an ancestor of the target replaced by a junction after the parent handle is held, creation lands in the
   held folder or fails; it never lands at the junction's target.
8. Debrief, Windows: the elevated child accepts a `bootstrap.json` rewritten by an unelevated process of the same user
   today; with the new channel the rewrite has no effect.

## Options considered

| Option | Licence | Windows | Creates atomically, anchored | Effort | Verdict |
|---|---|---|---|---|---|
| **A. `paddock_env::private`, on cap-std where it fits, with our NT-native Windows creation and descriptor checks (moved from rows 6, 8, 13)** | ours + cap-std (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT) | yes | yes | see WPs | recommended |
| B. `fs-mistrust` (Tor project) for the checks, ours for creation | MIT OR Apache-2.0 | no: "On Windows, we accept all file permissions and owners" (<https://docs.rs/fs-mistrust/latest/fs_mistrust/>, read 2026-11-10, version 0.16.1) | `make_directory`, Unix modes only | similar, plus a second model of trust | not chosen: the gap this ADR closes is Windows |
| C. Keep fixing instances | - | partly | no | unbounded | not chosen: 23 fixes in six days; the latest shared check is a Windows no-op |

Privacy and performance: the type adds one `fstat` or `GetSecurityInfo` per open of a trust-bearing file, a few per
start; not measured. No bulk or long operation is involved, so there is no "Parallelism and cost" section. Logs keep
naming reasons without paths (ER-27, as `note_result` does now).

## Decisions for the user

- **D1 Mechanism.** (a) **One `PrivateDir`/`PrivateFile` type in `paddock-env`, every writer through it** · (b)
  `fs-mistrust` for checks plus our creation · (c) keep fixing instances.
- **D2 Tightening the default data location.** Decided (user, 2026-11-10): Paddock removes group write only from the
  app's **default** data location, only in the proven own-private-group case (Linux umask 002), and logs it; an override
  location (`<APP>_DATA_DIR`, portable mode, `<ID>_HOME`) open to others is always refused with the fix-it message.
- **D3 The Windows descriptor.** (a) user SID only · (b) **user SID and SYSTEM, explicit SID, protected, `OICI`, and
  `FILE_ATTRIBUTE_NOT_CONTENT_INDEXED` on every `PrivateDir`** · (c) user, SYSTEM and Administrators (today's
  `make_private`). The descriptor also sets the owner, `O:<user SID>`, so a folder made while the app ran elevated is
  owned by the user and is not refused on the next normal run. An explicit SID rather than `OW` (row 13) because inherited `OW` entries apply to whoever owns each
  child. A folder created by an elevated process is owned by `BUILTIN\Administrators` (the default owner for an
  administrator's token), so the check accepts that owner only for folders the elevated side created (rule 75), never
  for the user's own `PrivateDir`.
- **D4 How far up the check looks.** (a) **the folder and its parent** · (b) every ancestor, as `fs-mistrust` · (c) the
  folder only (today). The parent is enough because every later operation is relative to the held handle (§1): renaming
  an ancestor moves the whole subtree but cannot change what the handle refers to, and the parent check stops the folder
  itself being replaced under the same name between creation and the first open. (b) refuses legitimate setups (a data
  folder under a shared mount) for no gain once operations are anchored.
- **D5 Enforcement.** (a) **register `docs/trust-files.toml`, C106 and Clippy `disallowed-methods`, ratcheted; Clippy
  alone until ADR 0088's engine and RT-5 land** · (b) Clippy only · (c) review only.
- **D6 Copies.** (a) **delete every copy in its app's adopting change, registered with delete-by tags (§5)** · (b)
  migrate the shared crates only, apps at leisure.
- **D7 `make_folder_private`.** (a) **test-support only, behind `test-support` (as you decided)** · (b) keep public.
- **D8 Elevated and protected locations.** (a) **rule 75 as in §4, the protected-chain text lifted from Dyno ADR 0022
  §2.1, app ADRs link it** · (b) leave it in the app ADRs and 0053.
- **D9 What it gates.** Decided (user, 2026-11-10): **(a)**. **Changed (user, 2026-11-11): (d), the minimum cut: WP-0
  only gates Pitwall 1.1; WP-1 is built and stays; the rest is later.** (a) the Pitwall 1.1 tag on WP-1 to WP-5, WP-8 and WP-10; the next Dyno tag on WP-6; Debrief 1.2
  and Pitcrew 1.1 on WP-7 · (b) Pitwall 1.1 on the whole type · (c) gate nothing · (d) an exposure cut, WP-0 (about
  3.5 days), gates Pitwall 1.1; WP-6 gates the next Dyno tag; WP-7 (with W2) gates Debrief 1.2 and Pitcrew 1.1; the type
  and the rest land on the train without gating Pitwall (recommended in revision 2, not chosen).
  - The user chose (a) knowing this trade-off. Why (d) was recommended: Pitwall 1.1's exposure on the train is small. Its service writes no file (`073e5a150`); `admin.key` and
    the chat secrets already go through `paddock-secrets`, which refuses a Windows folder others can write (row 6);
    Pitwall's data folder is under `%APPDATA%`, private by default. What remains is W3, W4 and W5, and WP-0 closes them
    with code that exists today.
  - The trade-off: (a) puts about 20 days of work (WP-1 to WP-5, WP-8, WP-10) to Pitwall 1.1's critical path, and every day 1.1 is later keeps W1, a
    High LPE in the shipped `pitwall-v1.0.3`, live on installs with the service (no 1.0.4, your decision of 2026-11-08).
    (d) ships 1.1 with the type unfinished, so Pitwall's non-secret private files (prefs, slots, cache) keep their
    current Unix-only narrowing until WP-5 lands on the train.
- **D10 Windows anchoring.** (a) **`NtCreateFile` with `RootDirectory` and the descriptor, `SetFileInformationByHandle`
  rename with `RootDirectory`, behind cap-std where it covers it** · (b) held handles without `FILE_SHARE_DELETE` plus a
  `GetFinalPathNameByHandleW` re-check before each child operation · (c) path-based (today).
- **D11 Admin keys read in 1.0.3.** (a) **rotate once on the first start after an upgrade from 1.0.3 or earlier (paired
  apps keep working; the window gets the new key), recorded by a one-time upgrade marker in the data folder, with a
  release-note line for scripts that hold the old admin key** · (b) a release-note line telling Linux users how to rotate · (c)
  nothing.

## Work packages

| WP | What | Days | Gates (D9 (a)) |
|---|---|---|---|
| WP-0 | Pitwall exposure cut: `apps.json` through a unique-temp `O_EXCL \| O_NOFOLLOW` owner-only writer (`paddock_env::write_atomic(private)` on Unix; on Windows a new public writer in `paddock-secrets` that composes `paddock_env::write_atomic_created` (`atomic.rs:37`) with `file_windows::create_owner_only`, today `pub(crate)` at `file_windows.rs:132`, added as a shared-integration change); no `.bak` fallback for the access list; a Windows DACL refusal on the data root through `paddock_secrets::acl::folder_refusal`; `random32` fails closed; admin-key rotation from 1.0.3 behind a one-time upgrade marker, and its release-note line (D11); failing tests 4 and 5 first | 4 | Pitwall 1.1 |
| WP-1 | `paddock_env::private`: cap-std spike (1 day), Unix (`mkdirat`, `fchmod`, `fstat`, `renameat`), Windows NT-native creation and rename with `RootDirectory`, descriptors, reparse refusal; failing tests 6 and 7 first | 7 | Pitwall 1.1 |
| WP-2 | Existing folders: `Refusal::OpenToOthersAcl`, macOS extended ACLs, the parent rule (D4), fix-it texts, tightening per D2; `check_folder` real on Windows; failing test 1 first | 2.5 | Pitwall 1.1 |
| WP-3 | `docs/trust-files.toml`, C106 on the RT-5 host with an ADR 0088 ratchet (after both land), Clippy `disallowed-methods` now, a planted positive control | 3 | Pitwall 1.1 |
| WP-4 | Shared adopters: `paddock-env` atomic writer and `dirs`, `paddock-proc::hint`, `paddock-secrets` (`secure_folder` refuses, no chmod), `paddock-mcp` key list (gains Windows); failing tests 2 and 3 first | 3 | Pitwall 1.1 |
| WP-5 | Pitwall onto the type: `admin.key` folder, chat secrets and prefs, slots folder, `cache_db`; delete `paths::write_atomic`, `restrict`, `restrict_dir` | 1.5 | Pitwall 1.1 |
| WP-6 | Dyno onto the type: data root (W8), `agent.json`, the MCP key list's folder (W7), WoL folder, cache db, geo and intel caches, battery folder (`hardware.rs` copy deleted) | 2 | next Dyno (D-6, user 2026-11-12: gates Dyno 1.1.1). Built on `fix/dyno-adr0092-wp6` (2026-11-12): the data root, `agent.json` (writer and reader), the AI access folder, and the extra adopters the trim had dropped: WoL folder, cache db (created through `PrivateDir::create_new_file`), geo and intel caches, battery folder (row 13's copy deleted). A refused data root runs the agent on memory plus a per-run scratch folder; a refused AI access folder fails closed (no keys read, no listener, `keys_problem` set). **Stays until WP-4:** `paddock-mcp`'s own `secure_folder` chmod (row 9) and the listener's discovery-by-path write (row 5): Dyno stops calling the key list and the listener from a refused folder, but their code still chmods and writes by path in a folder that passed the check. The instance lock file (`agent.lock`) is still taken in the data folder before the check. |
| WP-7 | Debrief: the elevated bootstrap through the verified pipe of rule 75 (address in the arguments, `GetNamedPipeServerProcessId` checked against the requester's pid and start time; W2; failing test 8 first), `private_tempdir` and the IPC socket folder; Pitcrew socket folder | 3 | Pitcrew 1.1 (the Debrief part no longer gates Debrief 1.2, user, 2026-11-11) |
| WP-8 | `make_folder_private` behind `test-support`; tests through `paddock_testkit::scratch` | 1 | Pitwall 1.1 |
| WP-9 | Rule 75 text and the family protected-location document; Dyno ADR 0022, Pitwall ADR 0024 and 0053 link it | 1 | none (docs) |
| WP-10 | Windows positive controls in CI and the test VM: a planted junction, an Everyone-writable folder, portable mode and `DYNO_DATA_DIR` on a folder others can write, a rewritten bootstrap | 1.5 | Pitwall 1.1 |

Total 29.5 days (revision 1 said 21: WP-1 grew by 3 for NT-native anchoring, WP-0 and the Debrief bootstrap are new, WP-2
grew for macOS ACLs; the acceptance added 0.5 for the public Windows writer). Under D9 (a) **WP-0 to WP-5, WP-8 and WP-10
gate the Pitwall 1.1 tag** (23.5 days; WP-0's items stay in that set), WP-6 gates the next Dyno tag, WP-7 gates Debrief 1.2
and Pitcrew 1.1, and WP-9 is docs only. WP-3 still waits for ADR 0088's engine and RT-5, so their landing is on Pitwall
1.1's critical path too. WP-0 is independent of everything else and comes first. WP-1 comes before WP-2 to WP-8; WP-3 also
waits for ADR 0088's engine and RT-5; WP-5 to WP-7 are independent of each other once WP-1 is done.

## Consequences

- One place creates and checks trust-bearing files on both OS, anchored to held handles; Windows gets the checks Unix
  has had since 2026-11-06.
- The owner-only type is explicitly not a boundary between a user's own processes; elevated readers get their input
  through rule-75 channels instead.
- A user whose data folder is open to others sees a refusal with the step to fix it, instead of a silent chmod.
- Fifteen production copies leave; the copies register tracks them until their tags.
- Risk: the Windows handle and descriptor code is unsafe FFI. Most of it moves from `paddock-secrets`, which already has
  Windows tests on the CI runner; the NT-native create and rename are new.
- Risk: a folder the user shares on purpose (a synced folder used as `DYNO_DATA_DIR`) is now refused on Windows where it
  passed before. That is the intent; the message names the override.

## Review answers (revision 2)

The independent review of revision 1 (`3ba5a4a16`) judged the diagnosis and core design right and asked for changes.

- **P1-1 (same-user processes; elevated readers):** the limit is stated ("What the type can and cannot defend against");
  rule 75 now names the allowed channels; the Debrief bootstrap is W2, an S4 instance gating Debrief 1.2 (WP-7, test 8).
- **P1-2 (Windows anchoring):** §1 specifies `NtCreateFile` and the rename with `RootDirectory`, with held handles and
  `GetFinalPathNameByHandleW` as the fallback (D10); cap-std is evaluated first, with what was and was not verified;
  `folder_security`'s missing `FILE_FLAG_OPEN_REPARSE_POINT` is named (row 6); WP-1 is 7 days.
- **P1-3 (shipped weaknesses):** the W1 to W9 table, rated, with the release that closes each; W1 cites the 2026-11-08
  decision; W3 adds D11; W7's route is corrected (`AppDirs::config()`, not moved by `DYNO_DATA_DIR`); W9 states the
  real reason to refuse.
- **P1-4 (D9):** option (d) and WP-0, recommended, with the escalation trade-off; WP-3's unbuilt dependencies are
  named; the total is 29 days.
- **P2s:** the copy table is 15 rows and 17 sites, row 7 moved out as test support, "carries the Windows rule" defined;
  D2 is a question, limited to the default location; Unix `fchmod` and macOS ACLs are in §1 and WP-2; C106's API list is
  extended and the register is triggered from the type's use; D3 adds `FILE_ATTRIBUTE_NOT_CONTENT_INDEXED` and the
  elevated owner; D4 says why the parent is enough; the evidence heads add `origin/docs/adr-windows-gate-ratchets` and
  `origin/feat/rt5-check-host`, both marked unbuilt; `random32` is W5 with test 5, in WP-0.

## Review answers (acceptance)

The review approved revision 2 and listed five P2s, answered here with the user's decisions.

- Rule 75's High-label channel: the elevated side acts relative to the handle it created or re-verifies label and owner
  on its opened handle (the rename hole through the parent's `FILE_DELETE_CHILD`); W2 uses the verified pipe, since its
  input flows from low to high and `runas` does not inherit handles (§4, WP-7).
- D3: the descriptor sets `O:<user SID>`.
- WP-0 needs a public Windows writer; `create_owner_only` is `pub(crate)`. Added as a shared-integration change, +0.5 day.
- Memory files are no longer cited; W1 and the env override cite the user decisions of 2026-11-08 and 2026-11-09.
- D11: a one-time upgrade marker and a release-note line for scripts holding the old key.
