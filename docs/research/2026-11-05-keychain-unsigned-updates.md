# Keychain secrets across updates of ad-hoc signed macOS apps

Date: 2026-11-05. Code at `origin/main` 87c9b8449; Dyno at `origin/dyno/wp-int` ada69e396. Measured in a throw-away Tart VM
(macOS 26.6.2, 25G83, Apple Silicon), with an invented test program using the apps' own call path (`keyring` 4.2) and with
two real Dyno 1.1.0 DMGs (runs 7575 and 7570). The 2026-11-05 decision stands: the apps stay unsigned. This note is evidence.

## Answer

- **Every update loses silent access to every Keychain item.** A login-keychain item is bound to `cdhash:<build>`; the next
  build gets a system dialog that asks for the login password, once per item and per program.
- **Always Allow lasts only until the next build.** It adds that one build's hash.
- **A free self-signed certificate does not help.** Measured, and it agrees with Apple's source: anything that does not
  chain to Apple gets a `cdhash:` partition, however stable its signature is.
- **The data protection keychain is closed to us.** Without an entitlement it refuses; with one and no provisioning profile
  the kernel kills the program.
- **The damage is mostly self-made.** Dyno's blocking item holds no secret at all, and Pitwall's is a copy of a file on disk.
  Moving those out removes the hang without any signing.
- **Blocker for the Dyno 1.1.0 tag (decision D1).** Anyone who creates an AI access key in 1.1.0 gets no window on the first
  launch of the next build until a password dialog is answered. The main thread waits in `SecKeychainFindGenericPassword`.

## Measured

Everything here ran on the VM. Timings are single runs, there to show "immediate" against "blocked", not benchmarks.
Raw log, screenshots and test source: `/Volumes/d7sst7/dev-targets/handoff/evidence-keychain-research/` (`kc.log`).

| Question | Result |
| --- | --- |
| Item made by ad-hoc build A | partition `cdhash:<A>`; A reads silently |
| Build B reads, dialog allowed | blocks in `SecKeychainFindGenericPassword`; SecurityAgent dialog with a password field |
| Deny | `-128` at once; the next read asks again |
| Allow | value returned after **two** dialogs, each with the password; nothing kept, the next read asks again |
| Always Allow | **one** dialog with the password; adds `cdhash:<B>` to the partition and the app list; B silent from then on |
| Wrong or empty password | the dialog stays |
| Next build C after B's Always Allow | **blocked again.** Each grant only adds a hash; A and B stay allowed, so a downgrade is silent |
| Three items | three dialogs, in turn; dialogs from several processes queue one at a time |
| Nobody answers, launchd agent in `gui/501`, no window | dialog shows; still blocked at **15 minutes** (where I stopped); no timeout seen |
| Client killed or booted out while its dialog is up | **the dialog stays on screen** until answered |
| Same read, agent in a Background session, or a daemon with `UserName` | no dialog; `-25308` in about 5 ms |
| Same read, daemon as root | `-25307`, no default keychain |
| Interaction off, `SecKeychainSetUserInteractionAllowed(false)` | `-25293` in 2 to 6 ms, never blocks. **This is process-wide** |
| Per-call `kSecUseAuthenticationUIFail` with `SecItemCopyMatching` | **ignored on the file keychain**: dialog up, blocked until my 10 s alarm |
| Other build writes, interaction off | `-25299` duplicate item; with UI allowed, a dialog, then stored |
| Other build deletes, interaction off | **deleted, no dialog.** Any program of the user can delete an item, then create its own |
| `security add-generic-password -A` | partition `apple-tool:`; the app still gets `-25293`. `-T` alone did not help either (earlier run) |
| Same bytes at another path | same cdhash, silent |
| Those bytes re-signed by `codesign --force --deep` | new cdhash, blocked |

Real Dyno 1.1.0, with `Dyno`/`mcp:keys` seeded through `security` as the earlier run did:

| Step | Result |
| --- | --- |
| First open | dialog at once, **no Dyno window**, main thread in `SecKeychainFindGenericPassword` |
| Always Allow | two dialogs were answered, window about 2 s later; the test app's own items took one. I did not establish why |
| Quit and reopen, same build | window in 2 s, no dialog |
| Next build | dialog again, no window |
| Deny | window in 2 s, with an empty key list (the code fails closed); dialog again on the next launch |
| Allow | window after two dialogs; dialog again on the next launch |

Earlier evidence (`handoff/evidence-mac-relaunch/cycle.log`, K2, L2 to L4) has the same block on an item the product created.

Dialog text on Dyno: "Dyno wants to use your confidential information stored in "Dyno" in your keychain." The dialog does not
show which build asks, so an impostor named the same looks identical. Asking for the login password on every update trains
people to type it into such dialogs.

## Per app

Common to all: `keyring::Entry` on macOS reaches `SecKeychainFindGenericPassword`, which blocks in a GUI session. Every direct
`get` turns any error into `None` (`.ok()`), so a Deny looks like "no secret set". No item, no dialog.

| App, items | Secret? | When read, on which thread | If blocked or denied |
| --- | --- | --- | --- |
| **Dyno** `Dyno`/`mcp:keys` | **No.** Client names and SHA-256 hashes; `keys.rs:104` says "they are not secrets" | `McpControl::new` → `load_keys` (`crates/paddock-mcp/src/control.rs:323`), from shell setup (`paddock-shell/src/builder.rs:663`): **main thread, before the first window** | No window until answered. Deny: AI clients silently stop working |
| **Dyno** `Dyno`/`wol:secure-on-<id>` | Weak. 4 or 6 bytes sent in the wake packet | `dyno-agent/src/wol.rs:231` in a wake job, also scheduled wakes; store at `dyno-platform/src/wol/mod.rs:122`. **Background agent in `gui/<uid>`, no window** | Job waits with no limit (agent measured). Unattended wake may wait for nobody. Deny: a clear job error |
| **Pitwall** `Pitwall`/`admin key` | **A copy** of `<data>/admin.key`, which the server owns (`pitwall-server/src/auth.rs:110`) | `StoredAdminKey::current` (`credentials.rs:89`) at the first server call (`server_client.rs:177,196`): sync call on an async worker. With `--hidden` at login, before any window | Calls wait. Deny: falls back to the file, then tries to write the store, which can raise more dialogs. Cached until quit. Concurrent first calls may each ask (read, not measured) |
| **Pitwall CLI**, reads the app's entry | Same copy | `pitwall-cli/src/discover.rs:163`, order: `--key-file`, env, store, file | **Cross-app: a different binary, so it asks on every version, even with no update.** Without a GUI session the read should fail and fall to the file (read; the `-25308` path measured with the test program, CLI not run) |
| **Debrief** `Debrief`/`Pitwall API key` | Low. A local-only `pw_` key; pairing issues a new one with a six-digit code. No cross-app read: its own entry | `look()` (`debrief-ai/src/engine/pitwall/mod.rs:238`) on the `debrief-pitwall` thread, once at start if Pitwall is running, then **every 15 s** unless the engine is Built-in (`ai/pitwall.rs:22`); also `:292` | Window is fine. Built-in model autoload waits behind the first probe. Deny looks unpaired and **asks again in 15 s**; plain Allow means two dialogs every 15 s |
| **Pitcrew** `Pitcrew Gitea API`, `Pitcrew registration`, `Pitcrew notification channel` | **Yes**: Gitea tokens, webhook, ntfy and mail secrets | `spawn_blocking`: `commands.rs:299,516,573`, per runner in `ops.rs:67`, test send `ops.rs:283`; **background** on each notification (`channels.rs:135`, `events.rs:146`) | Window is fine. A dialog can appear from the tray at any time. Deny: "no token", asks again on the next refresh or notification. `has_secret` and `has_api_token` read the value only to say yes or no |
| **Spotter** `Spotter`/`<scope>/<key>` | **Yes**: forge and registry tokens, mail password | Scheduler `run_check` (`spotter-engine/src/check.rs:204`), mail at notify (`:457`), `get_watch` reads values only to list names (`engine.rs:306`), `mail_status` (`channels.rs:22`). Async workers, not main | One dialog per token. Blocked checks hold async workers. Deny: checks go out with no token and it asks again next schedule |
| shell-demo | Same as Dyno's `mcp:keys` (`src/mcp.rs:51`) | Same | Demo only |

Pitcrew's own service already avoids the Keychain on macOS for this exact reason: `apps/pitcrew/docs/adr/0022` chooses
`FileSecrets` with backup exclusion. Released versions that wrote items: Debrief 1.1.1, Pitcrew 1.0.1, Pitwall 1.0.3. Dyno
1.0.0 has no `keyring`, and Spotter 1.0 is untagged, so those two need no migration if changed before their tags.

## Options

| Option | Keeps silent access across builds? | Protection against other programs of the same user | Cost |
| --- | --- | --- | --- |
| Today, with Always Allow | No | Reads need a password, while people are trained to type it; delete needs nothing | Password per item, per program, per update; hangs |
| **Self-signed, same certificate every build** | **No** (below) | Same as today | p12 in CI, rotation, hardened runtime turned on by `tauri-bundle` (conflicts with rule 23 unless changed), verify scripts |
| Developer ID (paid, comparison only) | Yes by Apple's source: `teamid:` partition. **Not measured**, no certificate here | Best available | US$99 a year; notarization also removes the Gatekeeper dialog; rule 23 changes; each item needs one grant on the move |
| Free Apple ID "Apple Development" certificate | **Possibly**: the source maps a Mac Development leaf to `teamid:`. **Not measured**; needs your Apple ID. Lifetime, terms and use on other Macs not checked | As Developer ID on the partition side | Gatekeeper dialog stays (no notarization). See D5 |
| Data protection keychain | No access | Would be best | Needs a profile, so a paid or team identity |
| **No-UI reads plus an explicit Allow action** | No, but never hangs | Same as today | About 2 days per app with UI; still a password per item per update |
| One Keychain "vault key" plus an encrypted file | No, one dialog per update instead of N | Same as today | Crypto, a file format, recovery. Only if N gets large |
| Stable helper binary (never rebuilt, holds the items) | **Yes, measured** (same bytes, same cdhash) | **About the same as a file**, since any program of the user can run the helper; encrypted at rest | Must stay out of Tauri's re-signing (a re-sign changes the cdhash, and `docs/lessons.md:1099` says Tauri re-signs sidecars); a helper fix means one more round of dialogs. 3 to 4 days plus release-path risk |
| Encrypted file, key held by the app | Yes | **None against same-user programs.** The key is in a public repo and binary. Stops only casual search of files and backups | Crypto for little gain; do not choose it over the plain file |
| **Owner-only file** (`FileSecrets`, exists and tested) | Yes | **None against same-user programs**, the same as `~/.ssh` and most CLI tools. Guards against other users; FileVault covers a stolen disk | Backup exclusion needed (`tmutil addexclusion`, ADR 0022); small |
| Move non-secrets out | Yes | Nothing lost: hashes are not secrets, and the admin key is already in a file | Small |

**Self-signing, plainly.** It does not keep Keychain access. With one certificate across builds A, B, C the designated
requirement was stable (`identifier … and certificate leaf = H"…"`) and the item's app list kept that requirement, but the
partition stayed `cdhash:`, so B and then C each got `-25293` and needed a new grant. A certificate with an OU field and no
trust setting, and one trusted in the admin domain, behaved the same. The reason is `partitionIdForProcess` in Apple's
[securityd `clientid.cpp`](https://github.com/apple-oss-distributions/Security/blob/main/securityd/src/clientid.cpp)
(Security-61901, read 2026-11-05): Apple-signed gives `apple:`, App Store, TestFlight, Developer ID and Mac Development give
`teamid:`, and everything else "cannot positively classify this code, but it's signed" gives `cdhash:`. Quinn's forum reply
about a stable designated requirement
([thread 746931](https://developer.apple.com/forums/thread/746931), Feb 2024) covers the ACL half, so taken alone it suggests
the wrong answer here. Gatekeeper is no better: a quarantined first open gave the same "Not Opened" dialog for ad-hoc and for
self-signed, and `spctl` rejected both, trusted or not. In the VM `codesign` signed with the untrusted certificate once its
temporary keychain was in the search list, so a CI trust step is not needed; `add-trusted-cert` raises a GUI dialog and hung.
It might still stabilise TCC grants, which key on the requirement (not measured, not part of this question). The updater does
not care: `crates/paddock-updater` has no code-signature, quarantine or `spctl` logic and relies on the family update key (read).

**Data protection keychain.** Ad-hoc or self-signed without an entitlement: `-34018`, "A required entitlement isn't present".
With `keychain-access-groups`, with or without `application-identifier`: `Killed: 9`, log "AMFI: bailing out because of
restricted entitlements". This is what Apple documents: entitlements "must be authorized by a provisioning profile"
([TN3137](https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains), read 2026-11-05;
[TN3125](https://developer.apple.com/documentation/technotes/tn3125-inside-code-signing-provisioning-profiles), read
2026-11-05). TN3137 also says the file-based keychain is "on the road to deprecation" but not officially deprecated. Whether
`SecKeychainSetUserInteractionAllowed` itself is marked deprecated I did not check; look before relying on it. I had no
profile, so there is no positive control; the two refusals are what I can vouch for.

**Other sources, read 2026-11-05.** Developer Program US$99 a year ([Apple](https://developer.apple.com/programs/whats-included/),
from search results, not the page itself). Open Anyway route for unnotarized apps ([Apple Support](https://support.apple.com/en-us/102445),
published 2026-05-27). Tauri documents ad-hoc and Developer ID, and says nothing on self-signed
([Tauri](https://v2.tauri.app/distribute/sign/macos/)).

**Other platforms (read, not measured).** Windows Credential Manager reads "the user's credential set … associated with the logon
session", with no build binding ([CredReadW](https://learn.microsoft.com/en-us/windows/win32/api/wincred/nf-wincred-credreadw)).
Linux Secret Service has no build binding in the spec; per-client unlock is left to the service
([spec](https://specifications.freedesktop.org/secret-service/latest/unlocking.html)). This is a macOS problem only.

## Recommendation

**First choice: on macOS, no Paddock process reads the Keychain; secrets live in `FileSecrets`, behind the existing `Secrets`
port.** Windows and Linux keep the OS store. The honest cost: for real third-party tokens a same-user program can read the file
silently, where today it would raise a dialog. For a personal family of apps that update often, with people trained to type the
password into any such dialog, I rate that as a fair trade, but it is your call (D2).

**Fallback, if you want Keychain protection for real tokens:** no-UI reads plus a visible "Allow access after the update" action,
for Spotter and Pitcrew only. **If warnings ever justify paying:** Developer ID makes the Keychain work properly.

| App | Before its next release | Days |
| --- | --- | --- |
| **Dyno 1.1.0, before the tag** | On macOS, pass `FileSecrets` instead of `KeyringSecrets` at `app/src-tauri/src/mcp.rs:144` and `dyno-platform/src/wol/mod.rs:122`. **No shared-code change.** ADR 0032 D6 still holds ("SecureOn in `paddock-secrets`"). First writer, so no migration | 0.5, plus a VM row |
| **Spotter 1.0, before the tag** | On macOS, `FileSecrets` at `lifecycle.rs:108` behind its `SecretStore` trait. Stop reading values to report presence | 0.5 to 1 |
| **Pitwall 1.1** | Remove `OsKeyStore` and the CLI's `os_store`; the file is the source. Delete the old item once: measured silent | 0.5 |
| **Debrief 1.2** | On macOS, `FileSecrets`, delete the old item, ask the user to re-pair (no read, so no dialog). Amend ADR 0059, which says "never in a file" | 0.5 |
| **Pitcrew 1.1** | On macOS, `FileSecrets` with backup exclusion. Old tokens: one explicit read on user action, or re-enter them. No read from a notification | 1 |

`shell-demo` follows the shared default below once that lands, after the Dyno tag.

### Family mechanism (ER-21)

The pattern: a blocking OS dialog reached from code that nobody thought of as UI. Instances: Dyno before the window, agent jobs,
a 15 s probe, a scheduler, a notification path. Proposed ADR:

1. **One door.** Only `paddock-secrets` may depend on `keyring`. A conformance check over every `Cargo.toml` plus Clippy
   `disallowed-types` on `keyring::Entry`; it fails today on five manifests, and the waiver list is the to-do list.
2. **One platform default.** `paddock_secrets::for_app(...)` gives `FileSecrets` on macOS and the OS store elsewhere, so the choice
   lives in one place. Backup exclusion moves into it.
3. **If a Keychain backend stays**, `OsVault` serves it under one mutex with interaction switched off, because only the process-wide
   switch works (the per-call flag is ignored). Errors `-25293` and `-25308` become a typed `NeedsPermission`, not `None`. An
   interactive read exists only as `ask()`, refuses the main thread and is called only from a user action. Rule text: "No process
   reads the Keychain with interaction allowed before its first window, on the main thread, or from background work."
4. **Presence is not a read.** The port gets `contains`, so no screen fetches a secret to draw a tick.
5. **VM release row, ER-33 shaped:** install build A, create the app's secrets, install build B, open. Expect a window within the
   bound and no SecurityAgent dialog. Its positive control is a read of an item from another build that must show the dialog,
   otherwise the row reports `check broken`. It belongs in `scripts/vm-smoke`. VNC clicks answered these dialogs here. The earlier
   run's failed press matched `^Allow$` against a label that reads `Allow | _NS:8`; AX press with a right pattern is untried. After a
   killed client the dialog's AX tree goes empty, so detect it from the window list.

Days: door and check 0.5, default and exclusion 1, VM row 1, ADR 0.5, so **3**; with the per-app work above, **about 6 to 7 in all**.
The fallback adds about 2 days per app (UI, health row, tests).

### Risks

- A same-user program can read `FileSecrets` silently (above), and unencrypted backups hold the file unless excluded.
- `FileSecrets` on a synced or network home folder may copy secrets or lose modes (ADR 0022 warns of this).
- A Dyno change now touches a release-path dependency; rule 54 asks for a build of the installers.
- Dyno's MCP key file can be edited by a same-user program, which could add a key. I did not check whether that program could already
  reach the agent or MCP server anyway; check before landing.

## Decisions for you

- **D1.** Dyno 1.1.0: fix before the tag (my recommendation, 0.5 day plus VM row), or tag and accept that anyone with an AI key
  meets a no-window dialog on the first update?
- **D2.** Real tokens (Spotter, Pitcrew) on macOS: owner-only file (recommended), or Keychain with no-UI reads and an Allow action?
- **D3.** Debrief: amend ADR 0059 to allow the file? Otherwise the key stays in the Keychain with the 15 s probe made no-UI.
- **D4.** Accept the mechanism as an ADR and rule text?
- **D5.** Spend half a day testing the free Apple Development certificate? It is the one free route that could give a `teamid:`
  partition. It needs your Apple ID on the test VM or a certificate exported from it, and a check of Apple's terms. It does not
  reopen the signing decision unless you want it to.

## Not verified

- **VM limits.** Tahoe 26.6.2 only, while your Mac reports Darwin 27.2, so this is not tested on macOS 27. No Touch ID or Secure
  Enclave path was tried; I cannot say whether real hardware offers another way to answer the dialog. TN3137 says that from 26.4 a
  keychain file may reference a protected entropy file, which could differ on hardware. The base image logs in automatically and
  its keychain has no timeout, so locked-keychain, screen-locked, sleep and fast-user-switching behaviour are untested.
- **Login items.** Measured a launchd agent bootstrapped into `gui/501` with `RunAtLoad`, not a real login, reboot or `SMAppService`
  item. App Tamer was not present.
- **Beyond 15 minutes.** No timeout was seen; I cannot say none exists.
- **Not run:** the real Pitwall, Debrief, Pitcrew, Spotter and CLI. Their behaviour is from the code plus the test program's results.
  Dyno's WoL path was not run either.
- **Two real-Dyno rows** were on an item seeded through `security`, not created in the app. The two-dialog Always Allow may come from
  that seeding.
- **Trust state.** After my first admin-domain trust, removal hung on a dialog, so "trusted" runs use certificate 1 and "untrusted"
  runs use certificate 2 (the OU one). No single certificate was compared both ways.
- **Developer ID and Apple Development partitions** are from Apple's source (dated October 2025), not measured.
- The brief's snapshot `gui-session-tools-no-dyno` is the Windows VMware VM's. I used the Tart macOS base `vmgui-base-tahoe-4aaa751f`
  under the same lock.
