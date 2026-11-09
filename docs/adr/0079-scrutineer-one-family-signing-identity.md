# 79. Scrutineer: one family signing identity on every OS, so macOS grants survive updates

- Status: Proposed (2026-11-09), revision 2. Needs the user's decisions (§9) and an independent re-review before any
  build.
- Revision 2 (2026-11-09) answers the independent review of revision 1 (`2c4b347cf`, verdict CHANGES):
  - P1-1: the host Full Disk Access evidence is now in the repo
    ([research note](../research/2026-11-09-macos-signing-identity-spike.md), "Host check") and quoted in §1.2; Removable
    Volumes is marked as covered by FDA, not measured on its own.
  - P1-2: the leaf-leak risk is stated plainly (§6.2), and D3 now offers real options for where the family key may sign.
  - P1-3: the search-list window is a risk to every job under the same account (§2.3, D9); `doctor` detects a leftover
    entry.
  - P1-4: the macOS track now stands alone (the CLI and the gate no longer wait for the Windows and Linux packages) and
    its figures are re-added (§10); D13 and D14 re-checked.
  - P2s: the one family key is recorded as decided (§9.1); home and placeholder location merged into D12; key types,
    lifetimes, the DR form and revocation are stated technical choices (§2.8); D8 says it changes the old plan's H5; the
    8-hex check comes before the fingerprint is shown; Linux corrections (openSUSE, Fedora to S1, the EKU already blocks
    TLS); where the root passphrase is kept; an unsourced Finder claim dropped; `paddock-dev` wording fixed.
- Date: 2026-11-09
- Number: 0079 was free on every `origin` branch on 2026-11-09 (`git ls-tree --name-only <branch> docs/adr/` over all
  `git branch -r`; family ADRs end at 0078 on `origin/docs/adr-test-isolation`).
- History:
  - 2026-11-04: the user asked for own-CA signing and a tool, **Scrutineer**, that generates certificates, signs,
    verifies, installs, reminds of expiry and gates releases, on shared crates with a thin CLI. Plan and research:
    `origin/docs/code-signing-plan` (`7a422b3f8`), `docs/plans/2026-11-04-code-signing.md` and
    `docs/research/2026-11-04-code-signing.md`, cited here as **P§n** and **R§n**. Neither landed.
  - 2026-11-05: dropped, because an own-CA signature does not remove Gatekeeper's or SmartScreen's warnings. Removal on
    main: `b2dd0e88b`, `92b7b6c28`, `87b453c4a` (docs) and `87c9b8449` (icons left on main).
  - 2026-11-09: revived after a spike (§1.2). The same day the user widened the scope to all three OS as peers, with the
    install functions of the old plan ("include all OS. Install root cert etc functions"), asked for the functions,
    placeholder icons and family-doc entries to come back, and clarified that Scrutineer is "a gui with command line app":
    a family app on the shared shell with a full command line (§2.7). The old plan had the GUI as "Later" (P§3 row 23).
- This ADR replaces the old plan's reasoning. It keeps the old plan's functions (P§2.3, P§3), records the one family key
  as decided (the old D16 recommended a separate key for Dyno's helper), reverses the old D9 wording and the "never in the
  search list" keychain handling (the spike, §1.2), and adds Linux as a peer (§4.3).
- Related:
  - Rules: 7 / ER-14 (tests never touch the user's machine), 23 (macOS bundles are ad-hoc sealed; amended by WP-E2),
    25 (before tagging), 42 / 43 / 55 (share first, test once, one implementation per mechanism), 52 (no starvable
    level for waited work), 53 (act only on a held or re-verified identity), 54 (installer build before a release-path
    change lands), 71 / ER-33 (a check counts only if its probe can see), ER-27 (logs explain every failure).
  - Family ADRs: 0014 (the family minisign update key, one endpoint per app), 0022 (`./dev`, tools in their own
    workspace), 0025 (CI job isolation), 0039 (release gates inspect the artifacts, `release.toml`), 0051 U5
    (`paddock-tools` Authenticode check, on `origin/docs/paddock-tools-design-v2`), 0053 (per-machine Windows installs and
    `paddock-setup.exe`, on `origin/docs/dyno-adr-0027-per-machine`), 0057 (Pitwall: no macOS Keychain), 0073 (no test
    double in a shipped build, on `origin/docs/adr-0073-no-fakes-family`), 0074 (declared hosts and machine leases).
  - Dyno ADRs 0022 (helper installed with the app), 0026 (dotted code-signing identifiers), 0027 (helper for 1.1.0: plist
    LaunchDaemon, not `SMAppService`, D2).
  - Evidence: [spike research note](../research/2026-11-09-macos-signing-identity-spike.md) (2026-11-09, copied from the
    scratch notes); the keychain finding of 2026-11-05 (`docs/keychain-unsigned-updates`, `95b27810d`); the CI runner TCC
    research of 2026-11-09 (scratch, `/Volumes/d7sst7/dev-targets/scratch/tcc-research/notes.md`).

## 1. Context

### 1.1 The measured problem: every update is a new identity on macOS

macOS records an app's **designated requirement (DR)** wherever it authorises the app, and checks every later version
against it: the TCC database (Full Disk Access, Removable Volumes, Accessibility, Input Monitoring, Screen Recording and
the other privacy services) and keychain item ACLs. Apple, TN3127: ad-hoc code "has a DR but it's tied to that specific
version of the code", and the keychain shows authorisation alerts when the DR changes
(<https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements>, read 2026-11-09).

Every Paddock build is ad-hoc sealed (rule 23), so its DR is `cdhash H"…"` and changes with every build. Today:

- **The apps** lose every TCC grant at every update. Dyno's disk map and file finder need Full Disk Access; Recce will
  need it; Debrief needs it for protected logs.
- **Keychain items** made by one build block the next one. On 2026-11-05 a Dyno build waited forever in
  `SecKeychainFindGenericPassword` on an item made by the previous build (3 of 3 runs, VM). The family answered by moving
  Dyno's secrets to owner-only files and by ADR 0057 (no macOS Keychain in Pitwall).
- **The CI runner binaries** (`act_runner` and two helpers) are ad-hoc with the identifier `a.out`. Each runner upgrade
  loses Full Disk Access and Removable Volumes (tcc-research notes, 2026-11-09: FDA off for all three).
- **CI-built test apps and test binaries** get a new identity every build and touch the work folder on the external
  volume, so the user sees Removable Volumes prompts.

### 1.2 The spike (2026-11-09): a stable self-signed identity keeps the grants

Full record: [research note](../research/2026-11-09-macos-signing-identity-spike.md). A throwaway self-signed
certificate (RSA 2048, `CA:false`, EKU `codeSigning`, **untrusted, no trust setting**) and a fixed identifier give the DR
`identifier "<id>" and certificate root = H"<sha1>"`. The DR does not depend on the cdhash, so every rebuild signed with
the same key satisfies it.

| Check | Self-signed v1 then rebuilt v2 | Ad-hoc v1 then v2 | Other certificate, same id |
|---|---|---|---|
| `codesign -v -R="<v1 DR>" v2` (guest macOS 26.6 and host 27.2) | 0 (satisfied) | 3 (failed) | 3 |
| Keychain item of v1 read by v2 (guest and host) | 0, no prompt | -25293 (`errSecAuthFailed`) | not run |
| Accessibility and Input Monitoring after replacing v1 with v2 (guest, SIP off, grant row written as System Settings writes it) | still granted | denied | denied |
| **Full Disk Access, host macOS 27.2, SIP on, granted by the user in System Settings** | **v2 read the FDA-protected file and the external-volume file, no prompt** | not run on the host | not run on the host |

The host evidence, as recorded in the research note ("Host check on macOS 27.2, SIP on"):

- system TCC.db row after the user's grant to v1: `kTCCServiceSystemPolicyAllFiles|2|4|2026-11-09 15:33:00` for client
  `net.decryptology.sign-spike`;
- `check.sh update-v2` output (v2 is different code, same identity, same path):

  ```text
  variant=v2 pid=41153
  probe /Users/decryp7/Library/Safari/Bookmarks.plist: OK size=1229464
  probe /Volumes/d7sst7/dev-targets/scratch/sign-spike/host-check/rv-target.txt: OK size=46
  designated => identifier "net.decryptology.sign-spike" and certificate root = H"617e8ff2dc3c95f5618dd3a17b9099b0c7e89541"
  ```

- no new prompt was reported; `check.sh cleanup` then reset both services for the test bundle id.

**What this measures and what it does not.** Full Disk Access across a rebuild is measured on the host. The read on the
external volume succeeded **under the FDA grant**: Removable Volumes was covered by FDA here and was **not measured on
its own**. The ad-hoc refusal on the host is shown by `codesign -R` and the keychain, not by a second TCC run. Screen
Recording and the other services rely on the same DR mechanism (TN3127) and are not measured.

Facts the design depends on, all from the research note:

1. **Trust is not needed** for TCC or the keychain. The certificate stayed `CSSMERR_TP_NOT_TRUSTED`.
2. **The signing keychain must be on the user's keychain search list.** With `codesign --keychain <file>` alone,
   `codesign` reported "no identity found". This reverses R§3.6 ("never in the search list").
3. **Always pass `-i <identifier>` for a bare binary.** The Rust linker's ad-hoc identifier is hash-suffixed
   (`sign_spike-e98c96c375ef330b`).
4. **Tauri needs `APPLE_SIGNING_IDENTITY=<SHA-1>`.** `tauri-macos-sign` finds identities only by Apple common-name
   prefixes (`keychain/identity.rs`, read in the source, not measured), so `APPLE_CERTIFICATE` alone would not find ours.
5. **A key change loses every grant once**, because the DR names the certificate.
6. **A binary copied out of a signed bundle is invalid** (its Info.plist is bound into the signature). Sign the bundle
   and run the binary in place; sign a standalone tool as a standalone Mach-O.
7. **LaunchServices fails with -10810** when `open --stdout` points at a removable volume the app has no grant for yet.
   That is a launch-harness trap, not a signing one.
8. **Not measured:** a three-tier chain (root, intermediate, leaf) and whether a leaf rotation keeps grants; library
   validation under the hardened runtime without a Team ID; ECDSA keys; certificate expiry; the data-protection keychain
   (it needs an Apple provisioning profile, so it is out of reach for self-signed code).

### 1.3 Why it was dropped still holds, and is not the reason now

- **Gatekeeper** still rejects the build (`spctl -a -vv`: `rejected, origin=Sign Spike Code Signing (throwaway)`). The
  "First launch" note in the release notes stays (rule 25).
- **SmartScreen:** Microsoft's table says a self-signed certificate gets "Same behavior as no signature"
  (<https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation>, updated 2026-08-17, read
  2026-11-09). A private root earns no reputation.

The reason now is **identity**: on macOS a stable identity keeps the person's grants and keychain items; on Windows and
Linux it lets a machine that chose to trust the family know a family binary from a lookalike (§4).

## 2. Decision

### 2.1 One family signing identity, made and held by Scrutineer

Scrutineer (a family app with a GUI and a full command line, §2.7; nothing in its signing core names Paddock) generates
and keeps **one family signing identity**:

```text
Family root CA         (offline, 20 y, CA, pathLen 1, EKU codeSigning)     Standby root (offline, never used until needed)
  └─ Intermediate CA   (owner's Mac, 5 y, CA, pathLen 0, EKU codeSigning)
       └─ Family code-signing leaf (signing account only, §2.3; 2 y, digitalSignature, EKU codeSigning, CN/O "decryp7")

Family OpenPGP package key (Linux packages and AppImages; primary key offline, signing subkey with the leaf)
Family minisign update key (unchanged format, ADR 0014; moves to the signing account with the leaf, §2.3)

Local root "Paddock local" (on the Mac mini only; never trusted anywhere; signs test apps and CI runner binaries, §2.5)
```

- **One family identity, never per app** (decided by the user, 2026-11-04). Each format needs its own key type, so the
  identity has one key per format and purpose (X.509 chain, OpenPGP, minisign), each family-wide.
- Fixed certificate **profiles** only (root, standby, intermediate, family leaf, local root, local leaf); no free-form
  templates (P§2.2). The CA certificates carry EKU `codeSigning`.

### 2.2 Crates and command layer (rule 42; old plan P§2.2 and P§2.3 kept)

| Crate | Owns |
|---|---|
| `paddock-pki` (new) | profiles, generation (`rcgen`), fingerprints (SHA-256 for people, SHA-1 for DRs), encrypted PKCS#8 and PKCS#12, the public inventory, CRLs, expiry rules, renewal and rotation plans, lint |
| `paddock-sign` (new) | `Signer` and `Verifier` ports; bounded format detection (PE, MSI, Mach-O, bundle, DMG, `.deb`, `.rpm`, AppImage, any file); inside-out walk; the macOS DR builder; TSA client with fallbacks; OpenPGP signing; minisign signing and verify (reusing `paddock-updater::signature`, rule 55); the verify report; the audit record; the release policy check |
| `paddock-trust-store` (new) | install, remove, list and check on each OS (§2.4); ownership markers so `remove` touches only ours; the read-only status used by installers and About |

The command layer keeps every command of P§2.3: `ca init | issue | renew | rotate | revoke | crl | export --public |
backup --paper`, `fingerprint`, `trust install | remove | status | sync`, `sign`, `verify [--policy] [--json]`,
`doctor`, `status`, `audit show`, `keys minisign | openpgp`, `ci prepare | cleanup`. Changes from the old plan:

- `ca init` also makes the family OpenPGP key and the local root (`--local`).
- `keys minisign` **never makes per-app keys**; it inventories, backs up and verifies the family update key.
- `ci prepare` on macOS puts the ephemeral keychain on the search list (§2.3).
- `trust install` covers Linux (§2.4) instead of refusing it.
- Each OS's commands are compiled per target, so the macOS commands ship without the Windows and Linux adapters (§10).

Least privilege as in P§2.3: only `trust install | remove | sync` change system state; nothing runs as a service or at
login; network only for the TSAs and the CRL; logs and audit records never hold keys, passphrases or file contents
(ER-27).

### 2.3 Signing, per OS, and where the family key lives

**Where the family key signs (D3).** Signing with the family leaf happens **only in a separate signing account**, never
in a job of the `paddock` repository (the reasons are in §6.2):

- A separate repository, `decryp7/paddock-signing`, that only the user can push to, holds the family leaf, the OpenPGP
  subkey and the minisign update key as its secrets.
- Its runners run under their own OS accounts: macOS user `paddock-signer` on the Mac mini, and the same on the Windows
  CI PC for Authenticode. They are registered to that repository only, so no `paddock` workflow can target them.
- A `paddock` release workflow builds unsigned bundles from the tag, uploads them, and dispatches the signing workflow
  with its run id. The signing workflow **checks provenance itself** before it signs: through the Gitea API, the run
  belongs to `decryp7/paddock`, its ref is a protected `<app>-v*` tag, and the tagged commit is an ancestor of `main`
  (Gitea's protected tags; to confirm on the instance in WP-S1). A dispatch from anywhere else signs nothing.
- It signs, rebuilds the DMG, `.app.tar.gz` and updater `.sig` over the signed bytes, and attaches them to the release.
  The `paddock` verify job (ADR 0039) then checks the published artifacts (§3).

**macOS (Tauri bundles, sidecars and helpers, the Dyno privileged helper, Scrutineer itself).** `./dev`
(`paddock-dev`) is not signed: it is a content-addressed developer tool (ADR 0022) that never ships to users.

- Every Mach-O is signed with the family leaf and an **explicit DR** (§2.8):
  `codesign -f -s <leaf SHA-1> -i <id> -o <flags> --timestamp=none -r='designated => identifier "<id>" and (certificate root = H"<root>" or certificate root = H"<standby>")' <path>`.
- Bundles are built with `tauri build --bundles app` (ad hoc) in the `paddock` job; the signing job re-signs inside-out
  with the explicit DR per identifier, keeping Dyno's dotted identifiers (Dyno ADR 0026), then makes the DMG, tarball and
  `.sig` from the signed `.app` (R§3.7). `APPLE_SIGNING_IDENTITY=<SHA-1>` (spike fact 4) is used only where Tauri signs
  inside the signing account.
- The Dyno helper (`/Library/LaunchDaemons/net.decryptology.dyno.helper.plist`, root-owned copy) is signed like any
  Mach-O. With a stable DR the helper can check its caller's code signature against the family DR, in addition to the
  uid in `LOCAL_PEERTOKEN` (WP-H1, optional). `SMAppService` stays out: Apple DTS says it wants an Apple-issued identity
  (Dyno ADR 0027 D2).
- **Hardened runtime: off** in the first signed release (§5, D4).

**The keychain recipe (research note, "Recipe outline"),** run by the signing job under `paddock-signer`:

1. `scrutineer ci prepare`: `security create-keychain` in the job's `TMPDIR` (ADR 0025), `set-keychain-settings`,
   `unlock-keychain`, `import` the PKCS#12 with `-T /usr/bin/codesign`, `set-key-partition-list -S apple-tool:,apple:`,
   then **save the account's search list and add the keychain to it**.
2. Sign.
3. `scrutineer ci cleanup` in an `always()` step: restore the saved search list **only if the current list is still the
   one this job set** (rule 53: act on what was verified just before), then `delete-keychain` on the file this job
   created (held by path and inode, never a glob).

**The search-list window (D9).** Between steps 1 and 3 the keychain is unlocked and on the search list of the account
that runs the job. **Any other process of that account can sign with it during that window**; a lock that only orders
signing jobs does not stop a different job from using it. That is why the family key signs only under `paddock-signer`,
which runs nothing but the signing workflow, one job at a time (runner capacity 1). The local identity (§2.5) uses the
same recipe on the main runner account; there, every job can use it during the window, which is accepted because it only
satisfies `.test` identifiers and the runner account already holds the runner's own grants. A killed job can leave an
entry behind: **`doctor` lists search-list entries that point at a missing file or at a keychain under a job `TMPDIR`**,
and `ci prepare` refuses to start while such an entry exists (with the command to remove it).

Signing is waited-for work: normal priority, never a starvable QoS (rule 52).

**Windows (NSIS installers, `<app>.exe`, sidecars, the NSIS uninstaller and plug-in DLLs, `paddock-setup.exe`,
`dyno-helper.exe`, Scrutineer; MSI if one ever appears).**

- Authenticode with the family leaf through `SignerSignEx2` (no SDK tool), an RFC 3161 timestamp from a public TSA list
  with retries, **never unstamped** (R§2.3). On the signing account, the leaf is an owner-only PKCS#12 file under the
  job's `TMPDIR`, deleted in cleanup.
- Because the key is not in the `paddock` job, the signing job either runs the whole `tauri build` with
  `bundle.windows.signCommand = "scrutineer sign %1"`, or signs the payload (`paddock-setup.exe`, `dyno-helper.exe`, the
  app binaries) and rebuilds and signs the installer; WP-S1 picks one.
- **Kernel drivers:** the family ships none. An own CA cannot sign one for 64-bit Windows with Secure Boot (Microsoft's
  attestation signing is required); Scrutineer does not offer it.

**Linux (`.deb`, `.rpm`, AppImage).**

- `.rpm`: an embedded OpenPGP signature with the family package key (the `rpm` crate, MIT or Apache-2.0, if WP-S1
  confirms it signs; otherwise `rpmsign` as a tool).
- AppImage: the embedded signature (`appimagetool --sign` layout) with the family package key.
- `.deb`: no per-package signature (dpkg does not check one by default, R§4). Apt verifies the **repository**; Gitea's
  Debian registry signs it with a key Gitea makes per owner, fetched from `repository.key`
  (<https://docs.gitea.com/usage/packages/debian>, read 2026-11-09). Publishing to it is WP-L2 (Should).
- The updater keeps minisign on every OS.

**Branch and PR builds** sign with a throwaway CA made in the job (`ca init --ephemeral`), so the signing and gate code
runs on every build without any family secret (P§2.4).

### 2.4 Installing trust, per OS: always the user's own action

Installing a root changes what the machine trusts. It is **the person's own action**: `scrutineer trust install` (or
the Trust screen), run by them, never silent, never on an update, never from a test (§6.3).

**The fingerprint check.** The person first types the first 8 hex digits of the root's SHA-256 **from their own record**
(the paper sheet or their password manager). Scrutineer compares them with the certificate it is about to install **and**
with the fingerprint pinned in its own build (`docs/signing/family.toml`, compiled in). Only after both match does it show
the full fingerprint, subject, validity and constraints, and enable Install; then the OS asks for elevation. Showing the
fingerprint first would let the person copy what the screen shows instead of checking their record.

| OS | Where | Constraint | Undo |
|---|---|---|---|
| Windows | LocalMachine **Trusted Root** (the root) and LocalMachine **Trusted Publishers** (the leaf, needed for PowerShell `AllSigned`, ClickOnce and some policies; renewed with each leaf) | EKU `codeSigning` in the CA certificates plus `CERT_ENHKEY_USAGE_PROP_ID` on the installed copy (R§2.1) | `trust remove` by fingerprint and ownership marker |
| macOS | **System keychain**, trust setting for the `codeSign` policy only, through `SecTrustSettingsSetTrustSettings`; macOS asks for an administrator password (R§3.5) | `codeSign` policy; no SSL trust | `trust remove` |
| Linux, Fedora, RHEL and Arch (`update-ca-trust`, p11-kit) | a p11-kit anchor in the extended `TRUSTED CERTIFICATE` format limited to code signing; `update-ca-trust(8)` lists `objsign-ca-bundle.pem` for "CA certificates trusted for code signing" (read 2026-11-09). Whether the anchor lands only there is **WP-S1** | purpose flags | `trust remove` |
| Linux, openSUSE | `update-ca-certificates` with `/etc/pki/trust/anchors` (p11-kit based); whether it honours purpose flags is **WP-S1**; until then treated like Debian | to measure | `trust remove` |
| Linux, Debian and Ubuntu | **refused with a reason**: `update-ca-certificates` trusts every certificate in `/usr/local/share/ca-certificates` and has no purpose limit (`update-ca-certificates(8)`, read 2026-11-09) | none in the store | — |
| Linux, packages | the family OpenPGP public key into `/etc/apt/keyrings/` with a `signed-by` source (with Gitea's repository key, WP-L2), and `rpm --import` for rpm-based systems | per repository (`signed-by`) | `trust remove` |

The Debian refusal is **conservative**: the CA certificates' `codeSigning`-only EKU already makes TLS stacks that check
the EKU along the chain (OpenSSL, NSS and Go, per the review of revision 1; WP-S1 confirms with a TLS handshake against a
test root) refuse the root for server authentication. The refusal stays until that is measured, because a mistake there
widens TLS trust on the machine.

`trust sync` fetches the published CRL and pushes revoked certificates into Windows `Disallowed` and a macOS deny
setting (P§2.3). `doctor` reports, per machine: what is installed where, chains that do not match the inventory, leaves
and keys near expiry, Smart App Control state, TrustedPublisher drift, leftover search-list entries (§2.3), and on macOS
whether the installed apps' DRs name the family root.

**From the app installers (D8).** The Windows per-machine installer (ADR 0053, already elevated) can offer an **unticked**
box on its finish page: install the family root, with the same 8-hex check, through `paddock-trust-store` (the same code
as Scrutineer, rule 55), never on `/UPDATE`, with reference-counted removal on the last app's uninstall. macOS has no
installer step (a DMG), and the apps do not need trust there (spike fact 1). Linux packages do not install an X.509 root;
a `.deb` may install the family package keyring for its repository. Every installer also shows a **read-only** status
line (WP-F4).

### 2.5 Test apps and runner binaries: a separate local identity

ADR 0073 keeps test doubles out of shipped builds. A test build signed with the family leaf **and the release
identifier** would satisfy the family DR and inherit the person's Full Disk Access, keychain items and helper trust. So:

- Test apps, test binaries and CI runner binaries are signed with the **local root** ("Paddock local", made by
  `ca init --local` on the Mac mini, never trusted anywhere, never in a release).
- Test identifiers carry a `.test` suffix, so even a leaked local key cannot satisfy a release DR (the DR names both the
  identifier and the root).
- The runner binaries get `net.decryptology.gitea-runner` (and two siblings), signed after each runner upgrade by a
  `./dev` step (research note, recipe step 5), started from launchd so each is its own responsible process; the person
  grants FDA once. Playwright's `chrome-headless-shell` stays out (a fixed browser path is the remedy, tcc-research
  option 3).

### 2.6 Fit with the family's other mechanisms

- **minisign family update key (ADR 0014).** Unchanged in format: updates are verified by minisign on every OS. The key
  moves to the signing account (§2.3), so a branch workflow can no longer read it either. The `.sig` is computed over the
  **signed** artifact; the gate checks both. One family key, no per-app keys.
- **Per-machine installers (ADR 0053).** `paddock-setup.exe` raises a UAC prompt at every update; with the root
  installed and a timestamp it is expected to show the family publisher, also after the leaf expires (WP-S1 measures).
- **No test doubles shipped (ADR 0073).** The shipped set ADR 0073 defines (every binary an installer carries) is the set
  the gate requires to be signed with the family leaf. Test builds use the local identity (§2.5).
- **FDA onboarding (idea, 2026-11-09: detect, open the Settings pane, drag the icon, poll, relaunch).** With a stable DR
  it runs once per machine, again only after a migration or key rotation. It must also detect the stale case: the switch
  in System Settings shows on for an old DR while access is denied; the flow then says "remove the app from the list and
  add it again".
- **Keychain decisions.** ADR 0057 and Dyno's owner-only files stay. A stable identity would make the login keychain
  usable again; whether to go back is D10, not part of this ADR's work.
- **ADR 0051 U5.** `paddock-sign` owns the Authenticode verifier; `paddock-tools` calls it.

### 2.7 Scrutineer is an app: GUI on the shared shell, the same functions as a command line

- **One app, two faces.** A Tauri app on the shared Paddock shell (`paddock-ui`, Angular Material first, rule 47; Angular
  CDK for behaviour; dialogs are real windows, rule 17), like the other family apps. The same binary run with arguments
  (`scrutineer <command>`, no window) is the full command layer of §2.2, for CI and scripts; on Windows a bundled console
  twin `scrutineer-cli.exe` gives a console without a window flash. The installers put the CLI on the PATH on request.
- **No logic in the UI.** The GUI and the CLI call the same crates (`paddock-pki`, `paddock-sign`, `paddock-trust-store`)
  through one command layer; the Rust core is the single source of truth (AGENTS.md "Where logic lives"). The Angular
  side renders what the engine returns; a mock backend exists only for UI tests.
- **Long work is a job** (rules 10 to 12): signing or verifying many files, generating keys, `trust sync` run as
  `paddock-jobs` jobs with live phases and Cancel; they never run at a starvable level (rule 52).
- **Elevation only for the one operation.** The app runs as the user. `trust install | remove | sync` starts the CLI as an
  elevated child for that one command (Windows UAC; macOS: the OS's own authorisation prompt from `SecTrustSettings` in
  the admin domain; Linux: `pkexec`), passing the fingerprint the person confirmed; the child re-checks it before acting
  (rule 53).
- **Private keys in the GUI.** The root is opened only during a ceremony from its offline file and dropped from memory
  after; the family leaf is never loaded by the GUI (it lives in the signing account); passphrases are typed each use (D2).

Screens (each with its skeleton, empty and error states, as every family page):

| Screen | Shows and does |
|---|---|
| **Certificate authority** (home) | The inventory: family root, standby root, intermediate, family leaf, OpenPGP and minisign keys, local root; for each, fingerprint, validity, days left, where it is kept, and a chain view root → intermediate → leaf |
| **Generate and rotate** | `ca init` (with `--local`), issue a leaf (PKCS#12 out for the signing account), renew, rotate the intermediate or the root (promote the standby), each as a step-by-step dialog that shows the plan before it acts and says what a root change costs (grants lost once, §6) |
| **Sign and verify** | Drop files to sign (chooses the format by detection) or to verify; **drag and drop any file to see its verify report**: signer, chain and which root, trusted here or not, timestamp, DR and identifier, nested files, minisign and OpenPGP signatures, with the reason for any failure |
| **Trust** | Per OS (§2.4): what is installed where, Install and Uninstall. Install asks for the **first 8 hex digits from the person's own record first** and shows the full fingerprint only after they match (§2.4), then the OS elevation prompt. Debian-style Linux shows why the root is not installed and offers the package keyring instead |
| **Doctor** | The `doctor` report as a checklist with a next step per row (expiry, chains that do not match the inventory, Smart App Control, TrustedPublisher drift, leftover search-list entries, installed apps whose DR does not name the family root) |
| **Expiry reminders** | Days left per certificate and key, warn at 60 and fail at 14 (§2.8); the shell's notifications (ADR 0010) when the app opens; no login item or service (D15); the same thresholds in `sign`, `doctor` and the CI gate |
| **Key backup** | `ca backup --paper`: the printable sheet (encrypted key lines with checksums, fingerprints, restore steps; never a passphrase), a restore check that reads a sheet back, and the "where is each key" record |
| Settings, About, Updates | the shell's standard pages; updates through the family minisign key like every app |

As a family app it gets `apps/scrutineer/` with its own `paddock.ci.json`, `.gitea/workflows/scrutineer.yml` (gates on all
three OS), a CODEOWNERS line, per-OS installers, and `scrutineer-v*` tags (D12). Its own releases are signed through the
signing account like every app.

### 2.8 Technical choices (stated, not user decisions)

- **Key types: RSA**, 4096 for the CAs and 3072 for the leaf. RSA was measured in the spike; ECDSA is unmeasured on
  `codesign` and `SignerSignEx2`. WP-S1 may try ECDSA for information; no change without a new measurement.
- **Lifetimes:** root 20 years, intermediate 5, leaf 2, OpenPGP subkey 2 (primary key without expiry). `sign`, `doctor`,
  the app and the CI gate warn at 60 days and fail at 14. Expiry behaviour on macOS is measured in WP-S1 (VM clock moved).
- **DR form:** always explicit (`-r`), naming the identifier and **the family root or the standby root**; never the leaf
  or the intermediate, so leaf and intermediate rotations keep grants (WP-S1 confirms on a three-tier chain).
- **Revocation:** a signed CRL file on a Gitea release plus `trust sync`; no CRL distribution point in the leaves
  (Windows rarely checks private CRLs, and macOS TCC never does, §6.2).
- **Timestamps:** public RFC 3161 TSAs with a fallback list and retries; a release fails rather than ship unstamped
  (R§2.3).
- **Engines:** Apple `codesign` (macOS), `SignerSignEx2` and `WinVerifyTrust` (Windows), the `rpm` and `pgp` crates or
  `rpmsign` (Linux); rcodesign (MPL-2.0, last release 0.29.0 of 2024-11-29) stays a later option.

## 3. Release gate (ADR 0039, rule 54, rule 71)

`scrutineer verify --policy release.toml` runs inside `./dev release verify` on every downloaded artifact and fails
closed:

- **macOS:** every Mach-O in the `.app` (and inside the DMG and `.app.tar.gz`) has a DR of the form
  `identifier "<declared id>" and (certificate root = H"<family root>" or …)`; **a DR that names a `cdhash`, a `.test`
  identifier or another root fails**; `codesign --verify --deep --strict` passes; the identifiers match the declared list.
- **Windows:** every PE in the installer (including the NSIS plug-ins, uninstaller, `paddock-setup.exe`) chains to the
  pinned family root, has an RFC 3161 timestamp, and the leaf was valid at signing time.
- **Linux:** `.rpm` and AppImage signatures verify with the family package key.
- **All:** the minisign `.sig` verifies over the signed bytes; the audit log (`signing-log.jsonl`, a release asset written
  by the signing account) lists every signed file.
- **Probe control (rule 71):** the same job runs the check on an ad-hoc copy of one Mach-O and an unsigned copy of one
  PE and requires both to fail; otherwise the gate reports `check broken`, not a pass.

Each OS's rules are added with that OS's CI work package, so the macOS gate does not wait for Windows or Linux (§10). The
C04 conformance check and the `macos-verify` action change from "ad-hoc seal" to this policy, with the rule 23 amendment.

## 4. What each OS actually gets

### 4.1 macOS: measured benefit

- Full Disk Access survives an update signed by the same identity: measured on the host, 27.2 with SIP on (§1.2).
  Keychain item access: measured on the host and in a VM. Accessibility and Input Monitoring: measured in a VM with SIP
  off. Removable Volumes on its own, Screen Recording and the other services: not measured; the same DR mechanism
  (TN3127) is expected to hold.
- `scrutineer verify` and `doctor` name the signer and the chain.
- **Not** gained: Gatekeeper still blocks the first launch of a quarantined download; no notarization.

### 4.2 Windows: a known publisher on machines that chose to trust it; nothing for SmartScreen or Defender

- With the root installed, UAC is expected to show "Verified publisher: decryp7" for the installer, `paddock-setup.exe
  apply-update` and `dyno-helper.exe` (no Microsoft page says so for a private root; WP-S1 measures it).
- **SmartScreen: no benefit** (Microsoft's table, §1.3). **Smart App Control** blocks code not signed by a CA in the
  Microsoft Trusted Root Program, "all executable files, not just those downloaded from the Internet"
  (<https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/overview>, R§2.2).
- **Defender and the removals of 2026-11-08** (`Trojan:Win32/Bearfoos.A!ml` on the unsigned `paddock-setup.exe`, twice):
  I found no Microsoft document saying a privately rooted signature changes Defender Antivirus detection. The one
  certificate allow-list Microsoft documents is Defender for Endpoint's certificate indicators: they need "Microsoft
  Defender Antivirus (in active mode) and cloud-based protection", are set from the Defender for Endpoint portal, and
  accept "a custom (self-signed) certificate … as long as it's trusted by the client (Root CA certificate is installed
  under the Local Machine 'Trusted Root Certification Authorities')"
  (<https://learn.microsoft.com/en-us/defender-endpoint/indicator-certificates>, updated 2026-08-14, read 2026-11-09).
  That is an enterprise product the user does not run. So own-CA signing **does not address the removals**; the user's
  choice stands (submit false positives, installers fail loudly when a component is removed). A paid publicly trusted
  signature (Microsoft Artifact Signing, from US$9.99 a month per the SmartScreen page) is the only thing documented to
  build reputation, and only over time.
- **Question for the user (D6):** does a stable publisher still matter on Windows, given it shows only on machines that
  installed the root and does nothing for SmartScreen or Defender?

### 4.3 Linux: package verification, and little else

- No Linux desktop or kernel checks an X.509 code signature on a binary (R§4); an X.509 root there buys only Scrutineer's
  own `verify` and, on p11-kit systems, the `objsign` bundle that few programs read.
- **What does work:** apt verifies a signed repository (`signed-by`); dnf and rpm verify package signatures against
  imported keys; AppImageUpdate and some launchers verify an embedded AppImage signature.
- The updater's minisign check already protects updates on Linux.

## 5. Hardened runtime: open risk and the test plan (Dyno)

Rule 23 keeps the hardened runtime off while ad hoc, because library validation accepts only code from the same Team ID
or Apple, and a self-signed certificate has no Team ID (R§3.3). Whether code all signed by the one family leaf passes
library validation is **unmeasured**. The runtime is required only for notarization, which the family does not do.

Plan, on Dyno (the app with the most Mach-Os: the app, `dyno-agent`, `dyno`, the helper, bundled dylibs), in a VM and
then on the host, inside WP-S1:

1. Sign everything with a throwaway three-tier chain and `-o runtime`, no entitlements. Start the app from Finder, the
   agent from launchd, the helper from its plist; run each sidecar's `--self-test` from the built app; record the
   unified log for `Library Validation` and `AMFI` denials.
2. Repeat with `com.apple.security.cs.disable-library-validation` on the app only.
3. Repeat with the runtime off.
4. Check that FDA granted under 1 (or 2) survives a rebuilt v2.

Recommendation (D4): ship the first signed release **with the runtime off**; turn it on later only if step 1 is clean.
Rule 23 is reworded from "while ad-hoc" to "until a step-1 pass is recorded".

## 6. Key custody, leaks and loss

### 6.1 Where each key and passphrase is kept

| Key | Kept | Passphrase kept | Backup | Used by |
|---|---|---|---|---|
| Family root, standby root | offline only: an encrypted file on two offline media | in the owner's password manager, and on a paper slip sealed and stored **apart** from the key media and the paper backup | paper sheet (`ca backup --paper`: encrypted key as base64 lines with checksums, fingerprints, restore steps; never a passphrase) | `ca issue` of an intermediate, every 5 years |
| Intermediate | the owner's Mac, encrypted file | typed each use, held in the password manager (D2) | same media | `ca issue` and `ca renew` of the leaf |
| Family leaf, OpenPGP subkey, minisign key | secrets of `decryp7/paddock-signing` only (§2.3) | a second secret of that repository | none needed for the leaf (reissued); the minisign key and OpenPGP primary on the paper sheet | the signing workflow |
| Local root and leaf | the Mac mini's main runner account | none (file mode 600) | none: re-made, the person re-grants test apps and runners once | test apps, runner binaries |

### 6.2 If a key leaks

**A leaked family leaf lets anyone make code that satisfies the family DR.** That code then inherits every Full Disk
Access grant and keychain item the person gave a family app on their Mac, silently. TCC and the keychain **never check a
CRL**, and the leaf's 2-year lifetime does not help: the DR pins the root, not the leaf, so a revoked leaf still
satisfies it, and an expired one probably does too (expiry is unmeasured, WP-S1). Revocation protects Windows machines
that ran `trust sync`; on macOS the remedy is the runbook (WP-G1): reset the affected grants (`tccutil reset All <id>`
for each family id), rotate to the standby root, re-sign, re-grant.

**How a leaf would leak if it were a `paddock` secret.** A Gitea Actions workflow in a repository reads that repository's
secrets. Gitea withholds them from **fork** pull requests (only the built-in token reaches a fork's run; Gitea change
#26759, seen in mirrored commits such as
<https://git.causa-arcana.com/forks/forgejo/commit/4f5a2117c37f1bf89d1686f407dff600a8783a97>, secondary), but a branch
pushed to the repository itself runs with them, and agents push branches to `paddock` every day. I found no Gitea
documentation for protected environments or for secrets limited to protected refs (WP-S1 checks the instance's
version). A workflow `if:` that loads the secret only on tags is not a control: the branch can edit the workflow. So:

| Option for D3 | Who could read the family leaf | Cost |
|---|---|---|
| A. Repo secret in `paddock`, loaded only by tag workflows | any workflow on any branch pushed to `paddock` | none; **not a control** |
| B. A dedicated signing runner label in `paddock`, key on that runner's disk | any `paddock` workflow that names the label | small; **not a control** |
| C. **A separate signing repository** that only the user can push to, its own runners under their own OS accounts, provenance checked before signing (§2.3) | only the user's own commits to the signing repository | WP-SR, 4–6 days; one more repository and two runner registrations |
| D. The owner signs each release by hand on their Mac (`scrutineer sign-release <tag>`) | only the owner's account | per release, about 10 minutes of the owner's time; the release waits for them |

Recommendation: **C**. It keeps releases automatic (ER-15: still built by CI from the tag), takes both the family leaf
and the minisign update key out of reach of every `paddock` branch, and closes the search-list window (§2.3) for the
family key by giving it its own account. D is the fallback if a second repository is unwanted.

A leaked intermediate is handled the same way one level up (revoke, reissue from the root, rotate to the standby if code
under it may have been signed). A leaked local key only satisfies `.test` identifiers.

### 6.3 If a key is lost

- **Leaf:** reissue from the intermediate. macOS grants survive (the DR pins the root; WP-S1 confirms). Windows
  signatures stay valid (timestamped). Nothing for the person to do.
- **Intermediate:** reissue it from the offline root, then a new leaf. Same as above.
- **Root (its file copies and the paper sheet):** promote the standby root. macOS grants survive (the standby clause).
  Windows machines install the standby root once. If the standby is lost too: a new root, **every macOS grant is lost
  once**, every Windows machine re-installs, Linux machines re-import the package key.
- **minisign key:** installed apps can no longer verify updates and need a manual reinstall (ADR 0014); the paper sheet is
  its backup.

**ER-14.** Tests never install or remove a root on the user's machine and never touch the user's keychain: the native
layer of `paddock-trust-store` and the macOS signer refuse unless `PADDOCK_TRUST_REAL=1` (the `paddock-secrets` pattern),
contract tests use a temporary store or keychain file never added to the search list, and real-store tests run on the
local VMs. **Remaining exception:** the CI signing steps on the Mac mini edit an account's keychain search list for the
signing window: the `paddock-signer` account for the family key, and the main runner account for the local identity
(§2.3). Neither is the user's login session, but the Mac mini is the user's machine.

## 7. Migration

1. The first signed release of each app is an ordinary update. Its DR differs from the ad-hoc one, so **the person
   grants FDA and the other services once more**, and keychain items made by ad-hoc builds are not readable without a
   prompt (the family's macOS secrets are already in files, §2.6). The release notes' "What's new" says so in one line.
2. From then on grants survive updates. A root rotation without the standby repeats step 1 once.
3. The runner binaries and test apps move to the local identity whenever WP-R0 runs: one FDA and Removable Volumes grant
   per runner binary, then none.
4. Windows: nothing changes until the person installs the root (optional). Linux: nothing changes until they add the
   key or repository.

## 8. Release order: what this blocks and what it speeds up

Order (2026-11-05 and later): Dyno (1.1.0 tagged 2026-11-08; 1.1.1 in progress), then Debrief 1.2 (end about
2027-06-11), then Pitwall 1.1 (tag about 2026-12-18 to 2027-01-13).

- **Blocks nothing.** No release waits for Scrutineer. Each app adopts signing at its first release whose freeze comes
  after WP-E2 lands; adoption touches the release path, so that app's installers are built first (rule 54).
- **When the macOS track can land.** About 26.5–36.5 agent-days of work, with a critical path of about 21.5–30 days for
  two builders (§10), starting after the decisions. Decided this week, it lands in mid-to-late December 2026 at the
  earliest, while the foundation redesigns and three release lines compete for the same builders. **Pitwall 1.1 is
  therefore unlikely to be the first signed release** unless its freeze moves into January. The realistic first signed
  release is the first Dyno release after 1.1.1, or Debrief 1.2 (D13).
- **Speeds up now:** WP-R0 (runner binaries, half a day after `ca init --local`, or with the spike recipe as an interim
  that costs one more re-grant later) ends the runner's FDA and Removable Volumes losses and the prompts from CI-built
  test apps. It does not touch any app's release path.
- **Speeds up Dyno:** disk map and file finder keep FDA across updates; the FDA onboarding flow becomes a one-time step.
- **The app itself (D14).** The signing account and the gate need only the command layer, which CI builds from the same
  crates. So the GUI app is **not on the critical path**: recommended place in the release order **after Pitwall 1.1**,
  with its screens built beside the release work while load allows. A local build of the CLI is enough for the owner's
  key ceremony (ER-15 allows a local build that is never published).
- **Competes for:** builder slots. The new crates are new code no release depends on, so they may run beside the release
  work while load allows (ER-1, ER-24).

## 9. Decisions

### 9.1 Already decided by the user

- One family identity, **one family update key, no per-app keys** (2026-11-04; corrects the old plan's D16).
- Shared crates (`paddock-pki`, `paddock-sign`, `paddock-trust-store`) under the app and its command line (2026-11-04).
- Scrutineer generates certificates, not only installs them; scope: generate, sign, verify, install, doctor, expiry
  reminders and a CI release gate (2026-11-04); all three OS as peers with the install functions (2026-11-09).
- Scrutineer is a GUI app with a command-line mode (2026-11-09).
- Its icon, hero image and page come later; placeholders now (2026-11-04, 2026-11-09).
- Personal use; no paid certificates unless warnings ever matter (2026-11-05). Windows: stay unsigned for Defender,
  report false positives (2026-11-08).

### 9.2 For the user (each with a recommendation)

| # | Question | Recommendation |
|---|---|---|
| D1 | Chain: root, intermediate and leaf; or one self-signed leaf as in the spike | **Three tiers**, if WP-S1 shows a leaf and an intermediate rotation keep TCC grants with the root-pinned DR. Fallback: one self-signed 20-year leaf (measured), with grants lost once at each key change |
| D2 | Passphrases | **Root passphrase in your password manager plus a sealed paper slip stored apart from the key media; intermediate passphrase typed each use** (not the login keychain); YubiKey later |
| D3 | Where the family key signs (§6.2) | **C: a separate signing repository with its own runners under their own OS accounts**, provenance checked before signing; D (by hand per release) as the fallback; A and B are not controls |
| D4 | Hardened runtime | **Off** in the first signed release; on only after the §5 step-1 pass |
| D5 | Test apps and runner binaries | **A separate local identity** on the Mac mini with `.test` identifiers (§2.5); it is not a per-app key |
| D6 | Windows: does a stable publisher still matter (UAC only, root installed, no SmartScreen or Defender gain) | **Build it in the same line** (one adapter, one signing path), ship it only if WP-S1 shows UAC naming the publisher |
| D7 | Linux | **OpenPGP signatures on `.rpm` and AppImage; the root only as a p11-kit code-signing anchor where WP-S1 shows it stays out of TLS; refused on Debian and Ubuntu**; signed apt repository through Gitea as a Should |
| D8 | Root install from the app installers | **Windows only, unticked opt-in on the finish page with the 8-hex check, never on update, reference-counted.** This **changes** the old plan, which listed it as H5 "Later, not recommended" (R§7): the user asked for it, and the opt-in, the check and the shared code keep its risk the same as Scrutineer's own install |
| D9 | The keychain search-list window (§2.3) | **Family key: only under `paddock-signer`, capacity 1** (follows D3). Local identity: accept the window on the main runner account. If D3 is not C, lease the **whole** Mac runner for the signing step (ADR 0074), not only a signing lock |
| D10 | Go back to the login keychain on macOS once signed | **Not now**; keep owner-only files and ADR 0057; revisit after one signed release cycle |
| D11 | Publisher name in the certificates | **"decryp7"** in CN and O; no e-mail or real name |
| D12 | Where Scrutineer lives | **`apps/scrutineer/` from its first code (AP1)**, like every family app: it ships installers, has its own release tag and CI contract, and runs on the shared shell; `tools/` holds developer scripts. **Until then the placeholder page stays at `tools/scrutineer/`**, where it was before the drop, because an `apps/<app>/` folder must carry `paddock.ci.json`, a workflow and a CODEOWNERS line (`scripts/affected.test.mjs` checks all three). Alternative: move the placeholder now to `docs/planned/scrutineer/`, as Recce does |
| D13 | Order of the signing work, and the first signed release | **macOS track first** (S1, A1, A2, B1, B3, C1-macOS, CLI-macOS, SR, E1m, E2-macOS, F1 for one app), Windows and Linux beside it; first signed release: the first app release whose freeze comes after E2, likely Dyno's next after 1.1.1 or Debrief 1.2; Pitwall 1.1 only if its freeze moves into January |
| D14 | Where the Scrutineer app goes in the release order | **After Pitwall 1.1** (after Dyno, Debrief 1.2 and Pitwall 1.1); the command layer the signing account and gate need is not blocked by the app's release. Earlier only if you want the GUI for the first key ceremony (then AP1 to AP7, 12–17.5 days, move ahead) |
| D15 | Expiry reminders outside the app | **No login item or background service**: reminders when the app opens, in `doctor`, and as CI warnings from 60 days; a login-item check is a later option |

## 10. Work packages

Days are agent-days for one implementer. Every crate has 99% line coverage (its native glue in `native.rs` excluded with
reasons) and contract tests in a `testing` feature (rule 43); real stores only on the local VMs (ER-14). Per-OS code is
behind target cfgs, so a package marked "macOS" does not wait for the Windows or Linux ones.

| WP | Scope | Depends | Days |
|---|---|---|---|
| **S1** | Spike on local VMs and one host step. macOS (2–3): three-tier chain with root-pinned DR across a leaf and intermediate rotation and the standby clause; expiry; the §5 hardened-runtime plan on Dyno; `tauri bundle` reusing a signed `.app`. Windows and Linux (1–1.5): UAC with the root (and with the leaf in Trusted Publishers), `signCommand` coverage, signature after leaf expiry with a timestamp; Fedora `objsign` extraction and openSUSE purpose flags; a TLS handshake against a code-signing-only test root; `rpm` crate signing; Gitea protected tags and secret scoping on the instance | decisions | 3–4.5 |
| **ADR** | Revise this ADR with S1, independent review | S1 | 1–1.5 |
| **A1** | `paddock-pki` core: profiles, generation, fingerprints, encrypted PKCS#8 and PKCS#12, inventory, CRL, expiry, lint | ADR | 4–5.5 |
| **A2** | `paddock-pki` rotation: standby, renew and rotate plans, the DR hash set | A1 | 1.5–2 |
| **B1** | `paddock-sign` core: ports, bounded and fuzzed detection, nested walk, TSA client, minisign, OpenPGP, report, audit record, policy | A1 | 3–4 |
| **B3** | macOS adapter: `codesign` with the explicit DR, `-i` always, ephemeral keychain on the search list with verified restore and the leftover check | B1, A2 | 2.5–3.5 |
| **B2** | Windows adapter: `SignerSignEx2`, `WinVerifyTrust` with pins | B1 | 3–4.5 |
| **B5** | Linux adapter: `.rpm` and AppImage signatures and their verify | B1 | 2.5–3.5 |
| **C1** | `paddock-trust-store` macOS (`SecTrustSettings`, `codeSign`) 2–2.5, then Windows (Root, TrustedPublisher, Disallowed, usage property) 2–3 | A1 | 4–5.5 |
| **C2** | `paddock-trust-store` Linux: p11-kit anchor with purposes, Debian refusal, apt keyring and `rpm --import` | A1, S1 | 1.5–2 |
| **CLI** | The command layer and CLI: every command of §2.2 (shared by the GUI), the 8-hex-first confirmation, the elevated child for `trust`, `doctor`, `audit show`, `backup --paper`. macOS commands first (2–2.5, needs A2, B3, C1-macOS); Windows and Linux commands as B2, B5, C2 land (1–1.5) | as stated | 3–4 |
| **G1** | Owner guides: the key ceremony, passphrase and backup places, renewal calendar, "a key leaked" runbook, per-OS trust install | CLI-macOS | 1.5–2 |
| **SR** | The signing account (D3 C): `decryp7/paddock-signing`, macOS user `paddock-signer` and its runner, provenance check, dispatch, publish, `signing-log.jsonl` (3–4); the Windows signing account and runner (1–2) | CLI-macOS, ceremony done | 4–6 |
| **R0** | Local identity on the Mac mini; `./dev` step that signs the runner binaries after an upgrade; test-app signing in CI with `.test` ids | CLI-macOS (or the spike recipe as an interim) | 0.5–1 |
| **E1m** | CI macOS: unsigned bundle upload and dispatch from the release workflow; the signing workflow's macOS steps (inside-out re-sign, DMG, tarball, `.sig`) | SR | 2–3 |
| **E1w** | CI Windows: the signing workflow's Windows steps (signed `tauri build` or payload signing and installer rebuild, `paddock-setup.exe`, `dyno-helper.exe`) | SR, B2 | 1.5–2 |
| **E1l** | CI Linux: package and AppImage signing in the signing workflow | SR, B5 | 1–1.5 |
| **E2** | Release gate (§3) with probe controls; C04 and `macos-verify` rewritten; rule 23 and 25 amendments. macOS rules 1–1.5 (needs E1m); Windows and Linux rules 0.5 (with E1w and E1l) | as stated | 1.5–2 |
| **F1** | Per-app adoption: identifiers, release-note text, installers built once per app (rule 54); 1–1.5 for the first app, 1–1.5 for the rest | E2-macOS | 2–3 |
| **F3** | `paddock-tools` U5 onto `paddock-sign`'s verifier | B2, ADR 0051 | 0.5–1 |
| **K1** | `keys minisign` and `keys openpgp`: inventory, backup, verify (no per-app keys) | B1 | 0.5–1 |
| AP1 | The app: `apps/scrutineer/` on the shared shell, `paddock.ci.json`, `scrutineer.yml`, CODEOWNERS line, installers on three OS, `scrutineer-v*` release | CLI | 2–3 |
| AP2 | Certificate authority screen and expiry reminders | AP1 | 1.5–2.5 |
| AP3 | Generate and rotate dialogs, key backup screen | AP1 | 2.5–3.5 |
| AP4 | Sign and verify with drag and drop and the verify report | AP1 | 1.5–2 |
| AP5 | Trust screen per OS with the 8-hex-first confirm and the elevated child | AP1, C1, C2 | 2–2.5 |
| AP6 | Doctor screen | AP1 | 0.5–1 |
| AP7 | Mock backend, unit and e2e tests, screenshots and their independent review (rule 22) | AP2–AP6 | 2–3 |
| F2 | Windows installer opt-in root install (D8) in the `paddock-setup` include | C1, ADR 0053 | 3–4 |
| F4 | Read-only trust status in the installers and About | C1, ADR 0028 | 1–1.5 |
| H1 | Dyno helper checks its caller against the family DR | B3, F1 | 1.5–2 |
| L2 | Signed apt repository through Gitea's Debian registry | E1l | 1–2 |
| Later | YubiKey/PKCS#11 (2–3), rcodesign adapter (1.5–2), plugin key under `keys` (1) | | 4.5–6 |

**Totals.**

- Signing path, Must (bold rows): **44–63 days**. With two builders that is about **4.5–6.5 weeks** of calendar after
  the decisions.
- The app (AP1 to AP7): **12–17.5 days**. Should (F2, F4, H1, L2): **6.5–9.5 days**. Later: 4.5–6 days. All together
  about **67–96 days**.
- **The macOS track to a first signed app** (S1-macOS 2–3, ADR 1–1.5, A1 4–5.5, A2 1.5–2, B1 3–4, B3 2.5–3.5, C1-macOS
  2–2.5, CLI-macOS 2–2.5, G1 1.5–2, SR-macOS 3–4, E1m 2–3, E2-macOS 1–1.5, F1 first app 1–1.5): **26.5–36.5 days** of
  work. Its critical path (S1-macOS, ADR, A1, B1, B3, CLI-macOS, SR-macOS, E1m, E2-macOS, F1) is about **21.5–30 days**;
  A2, C1-macOS, G1 and R0 run beside it on a second builder.

**Parallelism and cost (rule 51).** Signing is per file: hashing plus, on Windows, one TSA round trip. `sign` runs
independent files in parallel (`--jobs`, Auto = 4, network-bound); the macOS walk parallelises within one nesting level
only, because inner code is signed before its container. The signing account runs one job at a time (D9), so releases
of several apps queue there; a release has tens of files, so seconds per release are expected (unmeasured, S1 measures).
`--if-needed` skips a file whose signature already matches the leaf and policy, for reruns.

## 11. Consequences

- macOS users grant once per machine, not once per update. CI stops losing the runner's grants.
- The family key and the minisign update key leave the `paddock` repository's secrets; a branch can no longer read them.
- Rule 23 changes (bundles signed with the family identity, runtime off until measured); rule 25's signing-secret step
  moves to the signing repository; the "First launch" release-note text stays on macOS and Windows.
- The family gains key custody duties: an offline root, a paper backup, passphrases kept apart, a leaf to renew every two
  years, a CRL, a second repository and two runner accounts. `doctor` and the gate warn ahead of every expiry.
- A leaked family leaf exposes every macOS grant given to family apps until the grants are reset and the root rotated;
  a lost root without the standby costs every macOS grant once and a re-install on every trusting Windows machine.
- Scrutineer joins the family as an app with a command line; its placeholder page, icons and family-doc entries come
  back with this ADR; the real icon, hero and page are later work (the user, 2026-11-04). The unlanded
  `chore/remove-scrutineer-icons` must not land.
