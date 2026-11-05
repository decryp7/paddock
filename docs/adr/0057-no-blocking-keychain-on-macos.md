# 57. Paddock apps never block on the macOS Keychain

- Status: Proposed (2026-11-06). Decisions D1 to D5 below are open for the user.
- Date: 2026-11-06
- Related: ER-21 / rule 40 (a recurring bug means a missing design rule), ER-33 (a release check counts only with a
  positive control), rule 10 (nothing long on the UI thread), rule 12 (the UI never times out a job), rule 23 (ad-hoc
  macOS bundles), rule 42 (share first), rule 54 (the release path builds installers), rule 55 and ADR 0034 (one
  implementation per mechanism), ADR 0032 D6 (`SecureOn` in `paddock-secrets`; Dyno's amendments of 2026-11-05),
  Pitcrew ADR 0022 (the service's `FileSecrets` with backup exclusion), Debrief ADR 0059 (Pitwall as the AI engine; the
  key "never in a file"), memory note "code signing dropped" (2026-11-05: the apps stay unsigned).
- Number: 0057. The highest ADR file on `origin/main` is 0049; 0053 to 0056 are on open branches
  (`git ls-tree --name-only <branch> docs/adr/` over all branches, 2026-11-06). The highest check named anywhere is C98
  and the highest rule 71 (ADR 0054 on `origin/docs/adr-os-lifecycle-and-edges-amend1`, ADR 0055 on
  `origin/docs/adr-family-logging`), so this ADR claims check **C99** and rule **72**. Renumber at landing if those
  branches land first with more.

## Evidence

Measured facts come from the research note `docs/research/2026-11-05-keychain-unsigned-updates.md` on
`origin/docs/keychain-unsigned-updates` (`95b27810d`, not yet on `main`; link it once it lands), with its raw log, screenshots and test program in
`/Volumes/d7sst7/dev-targets/handoff/evidence-keychain-research/` (`kc.log`). They ran in a throw-away Tart VM (macOS
26.6.2, Apple Silicon) with an invented test program using the apps' own call path (`keyring` 4.2) and with two real
Dyno 1.1.0 DMGs (runs 7575 and 7570). Single runs, not benchmarks. Everything else here is marked as read in code or as
judgement.

Measured (research note, table "Measured"):

- **Every update loses silent access.** A login-keychain item made by an ad-hoc build is bound to `cdhash:<build>`. The
  next build gets a SecurityAgent dialog that asks for the login password, once per item and per program.
- **Always Allow lasts only until the next build.** It adds that one build's hash to the partition list.
- **Self-signed does not help.** With the same certificate for builds A, B and C, the partition stayed `cdhash:` and each
  new build needed a new grant. Apple's source agrees: only Apple-chained identities get `apple:` or `teamid:`
  (`securityd` `clientid.cpp`, read 2026-11-05).
- **A blocked read waits with no time limit.** Nobody answered, and a launchd agent in `gui/501` with no window was
  still blocked after 15 minutes. A read from the main thread holds the first window back (real Dyno 1.1.0: dialog at
  once, no Dyno window).
- **Interaction off never blocks.** After `SecKeychainSetUserInteractionAllowed(false)` the read returns `-25293` in 2
  to 6 ms. The switch is process-wide. The per-call `kSecUseAuthenticationUIFail` was ignored for this keychain.
- **Deleting needs no dialog.** Another build deletes the item with interaction off, with no dialog.
- **The data protection keychain is closed.** Without an entitlement it refuses (`-34018`); with one and no
  provisioning profile the kernel kills the program (`AMFI ... restricted entitlements`).

Read in Apple's source, not measured: Developer ID gets a `teamid:` partition, so its items would survive updates. The
user dropped code signing on 2026-11-05, so that route is closed for now.

Read in code on `origin/main` (`87c9b8449`): five manifests use `keyring` directly
(`apps/debrief/app/src-tauri/Cargo.toml:42`, `apps/pitcrew/app/src-tauri/Cargo.toml:40`,
`apps/pitwall/app/src-tauri/Cargo.toml:43`, `apps/pitwall/crates/pitwall-cli/Cargo.toml:29`,
`apps/spotter/app/src-tauri/Cargo.toml:47`), and `apps/shell-demo/src-tauri/Cargo.toml:43` turns on
`paddock-secrets/keyring` for every target. `crates/paddock-secrets` already has the port (`Secrets`, `Vault` in
`src/vault.rs`), the OS adapter behind the `keyring` feature (`KeyringSecrets`, `OsVault` in `src/os/`), and
`FileSecrets` (`src/file.rs`: one `0600` file per secret, written through a temporary file, refused on read if others
can read it).

Dyno's fix on `origin/fix/dyno-mcp-keys-file` (head `da4201dc5`, read): the MCP client key list moved to an owner-only
file (`crates/paddock-mcp/src/key_list.rs`, `3d03ede45`, hardened in `8e1ed6aa9` and `4af753edc`: trusted only for the
running user in a safe folder), and the Wake-on-LAN `SecureOn` password moved to `FileSecrets` on macOS
(`apps/dyno/crates/dyno-platform/src/wol/secrets.rs`, `91bffa558`). `dyno-platform` enables `paddock-secrets/keyring`
only under `cfg(not(target_os = "macos"))`. `apps/dyno/crates/dyno-platform/tests/no_keychain_on_macos.rs` checks it
two ways, each with a positive control: a scan of every manifest in Dyno's Cargo graph for any table naming `keyring`
that can reach macOS (control: it must find `dyno-platform`'s own entry), and `cargo tree --offline -i keyring` per
macOS target (control: the Windows graph must show `keyring`; offline failure reports `not measured`).

## Context

The same bug has now shown up in five shapes: Dyno's MCP key list read on the main thread before the first window, a
Wake-on-LAN job in a windowless background agent, Debrief's Pitwall key probe every 15 s
(`apps/debrief/app/src-tauri/src/ai/pitwall.rs:22`), Spotter's scheduler and mail notifier, and Pitcrew's notification
path. Each is a blocking OS dialog reached from code that nobody designed as UI. Under ER-21 that is a missing rule, not
four bugs. Fixing each app by hand already happened once (Dyno), and the other apps still carry the shape.

The Keychain cannot be made to work silently for unsigned apps, because the partition is tied to each build's hash. The
honest choices are to stop reading it, or to read it without UI and ask the user to allow access after every update.

## Decision

### 1. The rule (proposed rule 72)

> **No Paddock process reads the macOS Keychain where it can block.** Never before the first window, never on the main
> thread, never in a background job or agent, and never without a bound. If a Keychain backend stays anywhere,
> interaction is off, reads return a typed `NeedsPermission`, and `ask()` (the only read that may show a dialog) runs
> only from a user action. *Why:* ad-hoc and self-signed builds lose silent Keychain access on every update, and a
> blocked read waits with no time limit and no window (research 2026-11-05).

### 2. One mechanism (rule 55)

- **One door.** Only `paddock-secrets` may depend on `keyring`. Apps code against the `Secrets` port and never name
  `keyring::Entry`.
- **One platform default.** `paddock_secrets::for_app(...)` returns `FileSecrets` in the app's data folder on macOS and
  the OS store on Windows and Linux. The `keyring` feature is enabled only under `cfg(not(target_os = "macos"))`, as
  Dyno's `dyno-platform` does, so the Keychain adapter is not compiled into a macOS build. Backup exclusion
  (`tmutil addexclusion` on macOS, as Pitcrew ADR 0022 does) moves into `FileSecrets`, so every app gets it.
- **Presence is not a read.** The port gets `contains(&Name) -> Result<bool, SecretsError>`. Screens that only show a
  tick ("token set") call it and never fetch the value. On `FileSecrets` it is a metadata check.
- **If a Keychain backend stays** (option (b) in section 4), `OsVault` serves it behind one mutex with
  `SecKeychainSetUserInteractionAllowed(false)` around every call, because only the process-wide switch works (measured;
  the per-call flag is ignored). `-25293` and `-25308` become `SecretsError::NeedsPermission`, never `None`. An
  interactive read exists only as `ask()`, refuses the main thread, and is called only from a user action. Whether
  `SecKeychainSetUserInteractionAllowed` is marked deprecated was not checked (research note, "Not verified").

### 3. The check (C99)

Dyno's test becomes a family conformance check, C99 "no Keychain on macOS", over every Cargo workspace (`crates/`,
each `apps/<app>/`, `tools/paddock-dev`):

1. **Manifest scan.** Every `Cargo.toml` table that names `keyring` (a dependency, a renamed or inherited one, a
   feature entry such as `paddock-secrets/keyring` or `paddock-secrets?/keyring`) fails unless the table's `cfg` is one
   of `not(target_os = "macos")`, `windows`, `target_os = "windows"`, `target_os = "linux"`. `paddock-secrets`' own
   manifest, which defines the feature, is the one exception. Positive control: a fixture manifest with
   `features = ["keyring"]` under `[dependencies]` must be flagged, and `crates/paddock-secrets`' Windows entry must be
   found.
2. **Graph probe.** `cargo tree --offline --locked -i keyring --target aarch64-apple-darwin` (and `x86_64-apple-darwin`)
   per workspace finds nothing. Positive control: the same probe for `x86_64-pc-windows-msvc` must find `keyring` in at
   least one workspace, and `paddock-secrets` must be found for the macOS targets. When the graph cannot be read
   offline, the probe reports `not measured` (ER-33) and the manifest scan stands alone.
3. **Clippy.** `lint/clippy.toml` gets `disallowed-types = ["keyring::Entry"]` with the reason, so a new direct use
   fails at lint time inside `paddock-secrets` too, except in `src/os/native.rs` (one `#[allow]` with a reason).

The check starts with a waiver for each of the six manifests that fail today; the waiver list is the to-do list and
empties with work packages WP-3 to WP-7. Dyno's own test stays until C99 lands, then is deleted with the app's copy of
the scan (rule 43).

### 4. Per app (read in code on `origin/main`)

| App | What moves | What the user sees after the update | Release |
| --- | --- | --- | --- |
| Dyno | MCP client key list (names and SHA-256 hashes, `keys.rs:104` "they are not secrets") and the WoL `SecureOn` password, to owner-only files | Nothing: 1.0.0 had no `keyring`, so 1.1.0 is the first writer and nothing migrates | 1.1.0, before the tag (done on `fix/dyno-mcp-keys-file`) |
| Spotter | Forge and registry tokens, mail password (`spotter-engine` check `:204`, notify `:457`), to `FileSecrets` behind the existing `Secrets` port (`lifecycle.rs:108`); `mail_status` and the token list use `contains` | Nothing: Spotter 1.0 is untagged, so no released build wrote items | 1.0, before the tag |
| Pitwall and the CLI | The admin key leaves the Keychain; `<data>/admin.key`, which the server already owns, becomes the only store. Remove `OsKeyStore` (`credentials.rs`) and the CLI's `os_store` (`pitwall-cli/src/discover.rs:163`) | A silent move: the file already holds the key, so the new build reads the file and never the item | 1.1 |
| Debrief | The Pitwall API key (`ai/pitwall.rs`), to `FileSecrets`. Amend Debrief ADR 0059, which says "never in a file". The 15 s probe uses `contains` | Re-pair once with Pitwall (a six-digit code). The new build never reads the old item, so no dialog | 1.2 |
| Pitcrew | Gitea tokens, registration tokens, webhook, ntfy and mail secrets (`credentials.rs`, `channels.rs`), to `FileSecrets` with backup exclusion; `has_secret` and `has_api_token` use `contains` | Re-enter each token once, from one "Re-enter after the update" notice the user opens. Never asked from a notification or the tray | 1.1 |
| shell-demo | `mcp.rs` follows the shared default | Nothing (demo data) | After the Dyno tag |

Released builds that wrote Keychain items: Debrief 1.1.1, Pitcrew 1.0.1, Pitwall 1.0.3 (research note, "Per app").
There is no compatibility shim beyond the one-time re-entry above (memory note "no backward compatibility before v1").
The old item is never read. It stays in the login keychain, because the macOS build no longer links the adapter that
could delete it; the release notes name it for removal in Keychain Access. Deleting it with `/usr/bin/security
delete-generic-password` from the app would avoid that, but whether that tool prompts was not measured, so it is not
planned (the measured silent delete went through the API with interaction off).

### 5. The trade-off, plainly

An owner-only file is readable by any program running as the same user, the same as `~/.ssh` and most command-line
tools. It protects against other users, and FileVault protects a stolen disk. Today a same-user program reading a
Keychain item would at least raise a dialog, for an item made by another program. That dialog is weak protection here,
because people who type their password into it after every update learn to type it into any such dialog (judgement;
the dialog does not show which build asks, research note "Dialog text on Dyno").

It matters only for real third-party tokens: Spotter's forge and registry tokens and mail password, and Pitcrew's Gitea
tokens and channel secrets. Dyno's key list holds hashes, Pitwall's key is already in a file, and Debrief's key is a
local-only `pw_` key.

For those tokens there are two options:

- **(a) Owner-only file** (`FileSecrets`, backup-excluded). Never blocks, no dialogs, exists and is tested. Same-user
  programs can read it silently. About 0 extra days.
- **(b) Keychain with no-UI reads plus a visible action** ("Allow access after the update"): reads with interaction
  off, a health row "Needs permission", and a button that calls `ask()`. Keeps the same-user dialog. Costs one password
  dialog per item per update, and about 2 days per app with UI (Spotter, Pitcrew): the action, the health row and tests,
  plus a VM row that answers the dialog.

**Recommendation: (a)** for a personal family of apps that update often. It is the user's call (D2).

### 6. Release check (ER-33)

`scripts/vm-smoke` gets a row per app with secrets: install build A, create the app's secrets, install build B, open
it. Pass means the first window appears within the row's bound and no SecurityAgent window exists. Its positive control
reads an item that another build created, which must show the dialog; otherwise the row reports `check broken`. The
earlier run showed the pitfalls (research note, "VM release row"): a press on "Allow" matched the label `Allow | _NS:8`
by accident, and after one AX press the dialog's tree was empty, so the row finds the dialog in the window list, not
the accessibility tree.

## Consequences

- No Paddock app on macOS can hang on a Keychain dialog, because the adapter is not compiled in. C99 keeps it so.
- Secrets on macOS are as safe as `~/.ssh` (section 5). Backups exclude them; a synced or network home folder may still
  copy them or lose the file mode (Pitcrew ADR 0022 warns of this).
- Windows and Linux keep the OS store. Windows Credential Manager and the Linux Secret Service have no per-build
  binding (read in Microsoft's `CredReadW` page and the Secret Service spec, not measured), so they are unaffected.
- If Developer ID is ever bought, the Keychain becomes viable again (`teamid:` partition, read in Apple's source, not
  measured); the rule then changes with a new ADR, and `for_app` is the one place to switch.
- A Dyno change touches the release path, so rule 54 asks for an installer build before it lands.

## Work packages

| WP | What | Days | Lands in |
| --- | --- | --- | --- |
| WP-1 | Dyno: finish and land `fix/dyno-mcp-keys-file` (review, installer build under rule 54) | 0.5 | Dyno 1.1.0 |
| WP-2 | Shared: `for_app` default, `contains`, backup exclusion in `FileSecrets`, typed `NeedsPermission`; C99 with positive controls and the waiver list; Clippy `disallowed-types`; rule 72 text; Dyno's test removed | 1.5 | after the Dyno tag (no shared changes before it) |
| WP-3 | Spotter: `FileSecrets` on macOS, `contains` for status, VM row | 0.5 to 1 | Spotter 1.0 |
| WP-4 | Pitwall and CLI: remove `OsKeyStore` and `os_store`; release note names the old item | 0.5 | Pitwall 1.1 |
| WP-5 | Debrief: `FileSecrets`, re-pair, probe on `contains`; amend ADR 0059 | 0.5 | Debrief 1.2 |
| WP-6 | Pitcrew: `FileSecrets` with exclusion, re-enter notice, release note names the old items | 1 | Pitcrew 1.1 |
| WP-7 | shell-demo follows the default | 0.25 | after the Dyno tag |
| WP-8 | VM release row (`scripts/vm-smoke`, ER-33 shaped) | 1 | before the next app tag after WP-2 |
| WP-9 (if D2 = b) | No-UI reads plus the "Allow access" action, health row and VM row, per app with UI | 2 per app | Spotter, Pitcrew |
| WP-10 (if D5 = yes) | Test a free Apple Development certificate for a `teamid:` partition | 0.5 | anytime |

Total about 6 to 7 days for WP-1 to WP-8; option (b) adds about 4 days (Spotter and Pitcrew). Spotter 1.0 is before
WP-2, so WP-3 does Spotter's change through Spotter's own `Secrets` port and adopts `for_app` with WP-2.

## Decisions for the user

- **D1.** Dyno 1.1.0: land the fix before the tag (recommended, WP-1, 0.5 day), or tag and accept that users with an AI
  key meet a no-window dialog on the first update.
- **D2.** Real tokens (Spotter, Pitcrew) on macOS: (a) owner-only file (recommended), or (b) Keychain with no-UI reads
  and an "Allow access" action (about 2 days per app).
- **D3.** Debrief: amend ADR 0059 to allow the file. Otherwise the key stays in the Keychain and the 15 s probe must
  become no-UI.
- **D4.** Accept this ADR's mechanism (one door, one platform default, C99) and rule 72's text.
- **D5.** Spend half a day testing a free Apple Development certificate? It is the one free route that could give a
  `teamid:` partition. It needs the user's Apple ID on the test VM (or a certificate exported from it) and a check of
  Apple's terms. It does not reopen the signing decision unless the user wants it to.

## Not verified

- Only macOS 26.6.2 in a VM; not macOS 27, Touch ID, a Secure Enclave, a locked keychain, a locked screen, sleep or fast
  user switching (research note, "Not verified").
- Developer ID and Apple Development partitions are from Apple's source, not measured.
- The real Pitwall, Debrief, Pitcrew, Spotter and Pitwall CLI were not run; their behaviour is from code plus the test
  program. Dyno's WoL path was not run either.
- Whether a same-user program can already reach Dyno's agent or MCP server anyway, which would make the key file's
  exposure moot, was not checked.
- The check and rule numbers (C99, 72) are the next free ones on the branches read on 2026-11-06.
