# 79. Scrutineer: one family signing identity on every OS, so macOS grants survive updates

- Status: Proposed (2026-11-09). Needs the user's decisions (§9) and an independent review before any build.
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
    install functions of the old plan ("include all OS. Install root cert etc functions"), and asked for the functions,
    placeholder icons and family-doc entries to come back.
- This ADR replaces the old plan's reasoning. It keeps the old plan's functions (P§2.3, P§3), corrects its D16 (one
  family key, §2.6) and its D9 and macOS keychain handling (the spike, §1.2), and adds Linux as a peer (§4.3).
- Related:
  - Rules: 7 / ER-14 (tests never touch the user's machine), 23 (macOS bundles are ad-hoc sealed; amended by WP-F1),
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
  - Spike notes: `/Volumes/d7sst7/dev-targets/scratch/sign-spike/notes.md` (2026-11-09); the keychain finding of
    2026-11-05 (`docs/keychain-unsigned-updates`, `95b27810d`); the CI runner TCC research of 2026-11-09
    (`/Volumes/d7sst7/dev-targets/scratch/tcc-research/notes.md`).

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
  loses Full Disk Access and Removable Volumes (`tcc-research/notes.md`, 2026-11-09: FDA off for all three).
- **CI-built test apps and test binaries** get a new identity every build and touch the work folder on the external
  volume, so the user sees Removable Volumes prompts (memory, 2026-11-06).

### 1.2 The spike (2026-11-09): a stable self-signed identity keeps the grants

A throwaway self-signed certificate (RSA 2048, `CA:false`, EKU `codeSigning`, **untrusted, no trust setting**) and a
fixed identifier give the DR `identifier "<id>" and certificate root = H"<sha1>"`. The DR does not depend on the
cdhash, so every rebuild signed with the same key satisfies it.

| Check | Self-signed v1 then rebuilt v2 | Ad-hoc v1 then v2 | Other certificate, same id |
|---|---|---|---|
| `codesign -v -R="<v1 DR>" v2` (guest 26.6 and host 27.2) | 0 (satisfied) | 3 (failed) | 3 |
| Keychain item of v1 read by v2 (guest and host) | 0, no prompt | -25293 (`errSecAuthFailed`) | not run |
| Accessibility and Input Monitoring after replacing v1 with v2 (guest, SIP off) | still granted | denied | denied |
| **Full Disk Access and Removable Volumes, host macOS 27.2, SIP on** (user step, 15:33) | **v2 read both files, no prompt** | refused | refused |

Facts the design depends on, all from the notes:

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

Scrutineer (a tool, not an app, `tools/scrutineer/`, generic: nothing in it names Paddock) generates and keeps **one
family signing identity**:

```text
Family root CA         (offline, 20 y, CA, pathLen 1, EKU codeSigning)     Standby root (offline, never used until needed)
  └─ Intermediate CA   (owner's Mac, 5 y, CA, pathLen 0, EKU codeSigning)
       └─ Family code-signing leaf (CI secret, 2 y, digitalSignature, EKU codeSigning, CN/O "decryp7")

Family OpenPGP package key (Linux packages and AppImages; CI secret; primary key offline, signing subkey in CI)
Family minisign update key (unchanged, ADR 0014; Scrutineer only inventories, backs up and verifies it)

Local root "Paddock local" (on the Mac mini only; never trusted anywhere; signs test apps and CI runner binaries, §2.5)
```

- **One family identity, never per app** (the user, 2026-11-04). Each format needs its own key type, so the identity has
  one key per format and purpose (X.509 chain, OpenPGP, minisign), each family-wide. This replaces the old D16 (a
  separate minisign key for Dyno's helper): there is one family update key.
- Fixed certificate **profiles** only (root, standby, intermediate, family leaf, local root, local leaf); no free-form
  templates (P§2.2). The CA certificates carry EKU `codeSigning`, so Windows refuses the chain for TLS (R§2.1).

### 2.2 Crates and CLI (rule 42; old plan P§2.2 and P§2.3 kept)

| Crate | Owns |
|---|---|
| `paddock-pki` (new) | profiles, generation (`rcgen`), fingerprints (SHA-256 for people, SHA-1 for DRs), encrypted PKCS#8 and PKCS#12, the public inventory, CRLs, expiry rules, renewal and rotation plans, lint |
| `paddock-sign` (new) | `Signer` and `Verifier` ports; bounded format detection (PE, MSI, Mach-O, bundle, DMG, `.deb`, `.rpm`, AppImage, any file); inside-out walk; the macOS DR builder; TSA client with fallbacks; OpenPGP signing; minisign signing and verify (reusing `paddock-updater::signature`, rule 55); the verify report; the audit record; the release policy check |
| `paddock-trust-store` (new) | install, remove, list and check on each OS (§2.4); ownership markers so `remove` touches only ours; the read-only status used by installers and About |

The CLI keeps every command of P§2.3: `ca init | issue | renew | rotate | revoke | crl | export --public | backup
--paper`, `fingerprint`, `trust install | remove | status | sync`, `sign`, `verify [--policy] [--json]`, `doctor`,
`status`, `audit show`, `keys minisign | openpgp`, `ci prepare | cleanup`. Changes from the old plan:

- `ca init` also makes the family OpenPGP key and the local root (`--local`).
- `keys minisign` **does not generate per-app keys**; it inventories, backs up and verifies the family update key.
- `ci prepare` on macOS puts the ephemeral keychain on the search list (§2.3), under a lock.
- `trust install` covers Linux (§2.4) instead of refusing it.

Least privilege as in P§2.3: only `trust install | remove | sync` change system state, each shows the certificate,
validity, constraints and SHA-256 first and asks for the first 8 hex digits **from the person's own record**; nothing
runs as a service or at login; network only for the TSAs and the CRL; logs and audit records never hold keys,
passphrases or file contents (ER-27).

### 2.3 Signing, per OS

**macOS (Tauri bundles, sidecars and helpers, the Dyno privileged helper, Scrutineer, `paddock-dev` not signed).**

- Every Mach-O is signed with the family leaf and an **explicit DR**:
  `codesign -f -s <leaf SHA-1> -i <id> -o <flags> --timestamp=none -r='designated => identifier "<id>" and (certificate root = H"<root>" or certificate root = H"<standby>")' <path>`.
  The standby clause makes one root rotation grant-neutral (unmeasured: WP-S1).
- Bundles: `tauri build --bundles app` with `APPLE_SIGNING_IDENTITY=<leaf SHA-1>` (spike fact 4), then
  `scrutineer sign --deep` re-signs inside-out with the explicit DR per identifier, keeping Dyno's dotted identifiers
  (Dyno ADR 0026), then the DMG, `.app.tar.gz` and minisign `.sig` are made from the signed `.app` (R§3.7; whether
  `tauri bundle` reuses a signed `.app` is unmeasured, WP-S1).
- The Dyno helper (`/Library/LaunchDaemons/net.decryptology.dyno.helper.plist`, root-owned copy) is signed like any
  Mach-O. With a stable DR the helper can check its caller's code signature against the family DR, in addition to the
  uid in `LOCAL_PEERTOKEN` (WP-H1, optional). `SMAppService` stays out: Apple DTS says it wants an Apple-issued identity
  (Dyno ADR 0027 D2).
- **Hardened runtime: off** in the first signed release (§5, D7).

**Windows (NSIS installers, `<app>.exe`, sidecars, the NSIS uninstaller and plug-in DLLs, `paddock-setup.exe`,
`dyno-helper.exe`, Scrutineer; MSI if one ever appears).**

- Authenticode with the family leaf through `SignerSignEx2` (no SDK tool), an RFC 3161 timestamp from a public TSA list
  with retries, **never unstamped** (R§2.3). Tauri's `bundle.windows.signCommand` calls `scrutineer sign %1`;
  `paddock-setup.exe` and `dyno-helper.exe` are signed before `tauri build` embeds them (R§2.5).
- **Kernel drivers:** the family ships none. If one ever appears, an own CA cannot sign it for 64-bit Windows with
  Secure Boot (Microsoft's attestation signing is required); Scrutineer does not offer it.

**Linux (`.deb`, `.rpm`, AppImage).**

- `.rpm`: an embedded OpenPGP signature with the family package key (the `rpm` crate, MIT or Apache-2.0, if WP-S1
  confirms it signs; otherwise `rpmsign` as a tool).
- AppImage: the embedded signature (`appimagetool --sign` layout) with the family package key.
- `.deb`: no per-package signature (dpkg does not check one by default, R§4). Apt verifies the **repository**; Gitea's
  Debian registry signs it with a key Gitea makes per owner, fetched from `repository.key`
  (<https://docs.gitea.com/usage/packages/debian>, read 2026-11-09). Publishing to it is WP-L2 (Should).
- The updater keeps minisign on every OS.

**CI flow (the spike recipe, notes "Recipe outline").**

1. `scrutineer ci prepare` (macOS): `security create-keychain` in the job's `TMPDIR` (ADR 0025), `set-keychain-settings`,
   `unlock-keychain`, `import` the PKCS#12 with `-T /usr/bin/codesign`, `set-key-partition-list -S apple-tool:,apple:`,
   then **save the user search list and add the keychain to it**.
2. Sign (above).
3. `scrutineer ci cleanup` in an `always()` step: restore the saved search list **only if the current list is still the
   one this job set** (rule 53: act on what was verified just before), then `delete-keychain` on the file this job
   created (held by path and inode, never a glob).
4. The search list is per user and the Mac runner runs several jobs at once, so steps 1 to 3 run under a plock lane
   (`./dev ci slot`-style lease, ADR 0074) named `macos-signing`. Signing is waited-for work: normal priority, never a
   starvable QoS (rule 52).
5. Windows: `ci prepare` writes the PKCS#12 to an owner-only file under `TMPDIR`; `SignerSignEx2` reads it; cleanup
   deletes it. Linux: the OpenPGP subkey in an owner-only file, same lifecycle.

Secrets: `PADDOCK_CODESIGN_P12` (base64), `PADDOCK_CODESIGN_P12_PASSWORD`, `PADDOCK_PKG_PGP_KEY`,
`PADDOCK_PKG_PGP_PASSWORD`; whitespace stripped (rule 25, `strip-signing-keys`). The fingerprints are not secret and live
in the repo (`docs/signing/family.toml`, read by the gate).

**Which builds use the family leaf:** tags, `main` nightly installers and manual installer runs. Branch and PR builds
sign with a throwaway CA made in the job (`ca init --ephemeral`), so the signing and gate code runs on every build
without the secret (P§2.4).

### 2.4 Installing trust, per OS: always the user's own action

Installing a root changes what the machine trusts. It is **the person's own action**: `scrutineer trust install`, run
by them, shows the certificate, constraints and SHA-256, asks for the first 8 hex digits from their own record, and lets
the OS ask for elevation. Never silent, never on an update, never from a test (§6).

| OS | Where | Constraint | Undo |
|---|---|---|---|
| Windows | LocalMachine **Trusted Root** (the root) and LocalMachine **Trusted Publishers** (the leaf, needed for PowerShell `AllSigned`, ClickOnce and some policies; renewed with each leaf) | EKU `codeSigning` in the CA certificates plus `CERT_ENHKEY_USAGE_PROP_ID` on the installed copy (R§2.1) | `trust remove` by fingerprint and ownership marker |
| macOS | **System keychain**, trust setting for the `codeSign` policy only, through `SecTrustSettingsSetTrustSettings`; macOS asks for an administrator password (R§3.5) | `codeSign` policy; no SSL trust | `trust remove` |
| Linux, p11-kit distributions (Fedora, RHEL, Arch, openSUSE) | a p11-kit anchor in the extended `TRUSTED CERTIFICATE` format limited to code signing; `update-ca-trust` then lists it only in `objsign-ca-bundle.pem` (`update-ca-trust(8)`, read 2026-11-09) | purpose flags | `trust remove` |
| Linux, Debian and Ubuntu | **refused with a reason**: `update-ca-certificates` trusts every certificate in `/usr/local/share/ca-certificates` for everything and has no purpose limit (`update-ca-certificates(8)`, read 2026-11-09) | none possible | — |
| Linux, packages | the family OpenPGP public key into `/etc/apt/keyrings/` with a `signed-by` source (with Gitea's repository key, WP-L2), and `rpm --import` for rpm-based systems | per repository (`signed-by`) | `trust remove` |

`trust sync` fetches the published CRL and pushes revoked certificates into Windows `Disallowed` and a macOS deny
setting (P§2.3). `doctor` reports, per machine: what is installed where, chains that do not match the inventory, leaves
and keys near expiry, Smart App Control state, TrustedPublisher drift, and on macOS whether the installed apps' DRs name
the family root.

**From the app installers (kept, opt-in, D11).** The Windows per-machine installer (ADR 0053, already elevated) can
offer an **unticked** box on its finish page with the publisher and SHA-256: install the family root, through
`paddock-trust-store` (the same code as Scrutineer, rule 55), never on `/UPDATE`, with reference-counted removal on the
last app's uninstall. macOS has no installer step (a DMG), and the apps do not need trust there (spike fact 1). Linux
packages do not install an X.509 root; a `.deb` may install the family package keyring for its repository (common
practice), behind D11 too. Every installer also shows a **read-only** status line (WP-F4).

### 2.5 Test apps and runner binaries: a separate local identity

ADR 0073 keeps test doubles out of shipped builds. A test build signed with the family leaf **and the release
identifier** would satisfy the family DR and inherit the person's Full Disk Access, keychain items and helper trust. So:

- Test apps, test binaries and CI runner binaries are signed with the **local root** ("Paddock local", made by
  `ca init --local` on the Mac mini, never trusted anywhere, never in a release).
- Test identifiers carry a `.test` suffix, so even a leaked local key cannot satisfy a release DR (the DR names both the
  identifier and the root).
- The runner binaries get `net.decryptology.gitea-runner` (and two siblings), signed after each runner upgrade by a
  `./dev` step (the spike recipe, notes step 5), started from launchd so each is its own responsible process; the person
  grants FDA once. Playwright's `chrome-headless-shell` stays out (a fixed browser path is the remedy, tcc-research
  option 3).

### 2.6 Fit with the family's other mechanisms

- **minisign family update key (ADR 0014).** Unchanged: updates are verified by minisign on every OS. On macOS and
  Windows the `.sig` is computed over the **signed** artifact; the gate checks both. One family key, no per-app keys.
- **Per-machine installers (ADR 0053).** `paddock-setup.exe` raises a UAC prompt at every update; with the root
  installed and a timestamp it shows the family publisher, also after the leaf expires (unmeasured: WP-S1).
- **No test doubles shipped (ADR 0073).** The shipped set ADR 0073 defines (every binary an installer carries) is the set
  the gate requires to be signed with the family leaf. Test builds use the local identity (§2.5).
- **FDA onboarding (idea, 2026-11-09: detect, open the Settings pane, drag the icon, poll, relaunch).** With a stable DR
  it runs once per machine, again only after a migration or key rotation. It must also detect the stale case: the switch
  in System Settings shows on for an old DR while access is denied; the flow then says "remove Dyno from the list and add
  it again".
- **Keychain decisions.** ADR 0057 and Dyno's owner-only files stay. A stable identity would make the login keychain
  usable again; whether to go back is D14, not part of this ADR's work.
- **ADR 0051 U5.** `paddock-sign` owns the Authenticode verifier; `paddock-tools` calls it (old D14 kept).

## 3. Release gate (ADR 0039, rule 54, rule 71)

`scrutineer verify --policy release.toml` runs inside `./dev release verify` on every downloaded artifact and fails
closed:

- **macOS:** every Mach-O in the `.app` (and inside the DMG and `.app.tar.gz`) has a DR of the form
  `identifier "<declared id>" and (certificate root = H"<family root>" or …)`; **a DR that names a `cdhash`, a `.test`
  identifier or another root fails**; `codesign --verify --deep --strict` passes; the identifiers match the declared list.
- **Windows:** every PE in the installer (including the NSIS plug-ins, uninstaller, `paddock-setup.exe`) chains to the
  pinned family root, has an RFC 3161 timestamp, and the leaf was valid at signing time.
- **Linux:** `.rpm` and AppImage signatures verify with the family package key.
- **All:** the minisign `.sig` verifies over the signed bytes; the audit log (`signing-log.jsonl`, a release asset) lists
  every signed file.
- **Probe control (rule 71):** the same job runs the check on an ad-hoc copy of one Mach-O and an unsigned copy of one
  PE and requires both to fail; otherwise the gate reports `check broken`, not a pass.

The C04 conformance check and the `macos-verify` action change from "ad-hoc seal" to this policy (WP-E2, with the rule
23 amendment).

## 4. What each OS actually gets

### 4.1 macOS: measured benefit

- TCC grants (FDA, Removable Volumes, Accessibility, Input Monitoring and the others) and keychain item access survive
  every update signed by the family leaf: measured on 27.2 with SIP on (§1.2).
- `scrutineer verify` and `doctor` name the signer. With the root installed (optional) Finder shows a trusted signer.
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
  choice stands (stay on the same course: submit false positives, installers fail loudly when a component is removed). A
  paid publicly trusted signature (Microsoft Artifact Signing, from US$9.99 a month per the SmartScreen page) is the
  only thing documented to build reputation, and only over time.
- **Question for the user (D9):** does a stable publisher still matter on Windows, given it shows only on machines that
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

Recommendation (D7): ship the first signed release **with the runtime off**; turn it on later only if step 1 is clean.
Rule 23 is reworded from "while ad-hoc" to "until a step-1 pass is recorded".

## 6. Key custody, loss and leaks

| Key | Kept | Backup | Used by |
|---|---|---|---|
| Family root, standby root | offline only: an encrypted file on two offline media | paper sheet (`ca backup --paper`: encrypted key as base64 lines with checksums, fingerprints, restore steps; never the passphrase) | `ca issue` of an intermediate, every 5 years |
| Intermediate | the owner's Mac, encrypted file, passphrase typed (D4) | same media | `ca issue` and `ca renew` of the leaf |
| Family leaf | CI secret (PKCS#12 plus password) | none needed: reissued from the intermediate | CI signing jobs on tags, `main` installers and manual runs |
| Family OpenPGP key | primary offline with the root; signing subkey as a CI secret | paper sheet | Linux package signing |
| Family minisign key | as today (ADR 0014) | added to the paper sheet | the updater |
| Local root and leaf | the Mac mini only | none: re-made, the person re-grants test apps and runners once | test apps, runner binaries |

**If a key is lost:**

- **Leaf:** reissue from the intermediate. macOS grants survive (the DR pins the root; to be confirmed by WP-S1).
  Windows signatures stay valid (timestamped). Nothing for the person to do.
- **Intermediate:** reissue it from the offline root, then a new leaf. Same as above.
- **Root (and its file backups and paper):** promote the standby root. macOS grants survive (the standby clause).
  Windows machines install the standby root once. If the standby is lost too: a new root, **every macOS grant is lost
  once**, every Windows machine re-installs, Linux machines re-import the package key.
- **minisign key:** outside this ADR (ADR 0014): installed apps can no longer verify updates and need a manual reinstall.

**If a key leaks:** revoke the leaf or intermediate (`ca revoke`, CRL release, `trust sync`), rotate, re-sign the next
release. On macOS, TCC and the keychain do not read CRLs: code signed by a leaked leaf under the same root still
satisfies the DR. The runbook (WP-G1) says to reset the affected grants (`tccutil reset All <id>`), rotate to the standby
root and re-grant. The CI secret is readable by any workflow (Dyno ADR 0022 P1), which is why only tag, `main` and manual
runs load it and the leaf lives 2 years.

**ER-14.** Tests never install or remove a root on the user's machine and never touch the user's keychain: the native
layer of `paddock-trust-store` and the macOS signer refuse unless `PADDOCK_TRUST_REAL=1` (the `paddock-secrets` pattern),
contract tests use a temporary store or keychain file never added to the search list, and real-store tests run on the
local VMs. **One honest exception:** the macOS CI signing step runs on the Mac mini, which is the user's machine and the
runner's user account, and it edits that account's search list for the length of the signing step (§2.3). D13 asks
whether to move the runner to its own macOS account.

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

Order (memory, 2026-11-05 and later): Dyno (1.1.0 tagged; its next tag is gated on ADR 0072 M2 to M4), then Debrief 1.2
(end about 2027-06-11), then Pitwall 1.1 (tag about 2026-12-18 to 2027-01-13).

- **Blocks nothing.** No release waits for Scrutineer. Each app adopts signing at its first release whose freeze comes
  after WP-E2 lands; adoption touches the release path, so that app's installers are built first (rule 54).
- **First signed release:** most likely **Pitwall 1.1** (if the macOS track, about 22 to 30 agent-days, lands before its
  freeze), or a Dyno release if it comes first. Debrief 1.2 gets it inside its long line at no extra cost.
- **Speeds up now:** WP-R0 (runner binaries, half a day after `ca init --local`) ends the runner's FDA and Removable
  Volumes losses and the prompts from CI-built test apps, which costs the user attention on every upgrade today. It does
  not touch any app's release path.
- **Speeds up Dyno:** disk map and file finder keep FDA across updates; the FDA onboarding flow becomes a one-time step.
- **Competes for:** builder slots. The new crates (A, B, C, D) are new code no release depends on, so they may run beside
  the release work while load allows (ER-1, ER-24).

## 9. Decisions for the user (each with a recommendation)

| # | Question | Recommendation |
|---|---|---|
| D1 | Chain: root, intermediate and leaf; or one self-signed leaf as in the spike | **Three tiers**, if WP-S1 shows a leaf rotation keeps TCC grants with the root-pinned DR. Fallback: one self-signed 20-year leaf (measured) |
| D2 | Key types | **RSA**: 4096 for the CAs, 3072 for the leaf (RSA was measured; ECDSA unmeasured on `codesign` and `SignerSignEx2`) |
| D3 | Lifetimes | **Root 20 years, intermediate 5, leaf 2**; `sign`, `doctor` and the gate warn at 60 days and fail at 14 |
| D4 | Intermediate key on the owner's Mac | **Encrypted file, passphrase typed each use** (not the login keychain); YubiKey later |
| D5 | macOS DR | **Explicit `-r`**, identifier and family root **or** standby root |
| D6 | Which builds use the family leaf | **Tags, `main` nightly installers, manual installer runs**; branches use an in-job throwaway CA |
| D7 | Hardened runtime | **Off** in the first signed release; on only after the §5 step-1 pass |
| D8 | Test apps and runner binaries | **A separate local identity** on the Mac mini with `.test` identifiers (§2.5); it is not a per-app key |
| D9 | Windows: does a stable publisher still matter (UAC only, root installed, no SmartScreen or Defender gain) | **Build it in the same line** (it is one adapter and keeps one signing path), ship it only if WP-S1 shows UAC naming the publisher |
| D10 | Linux | **OpenPGP signatures on `.rpm` and AppImage; root only as a p11-kit `objsign` anchor; refused on Debian-style stores**; signed apt repository through Gitea as a Should |
| D11 | Root install from the app installers | **Windows only, unticked opt-in on the finish page, never on update, reference-counted** (kept from the old plan's fallback); none on macOS; Linux packages may add the package keyring only |
| D12 | Revocation | **CRL file on a Gitea release plus `trust sync`**; no CRL distribution point in the leaves |
| D13 | The Mac runner edits the user's keychain search list while signing | **Accept now, under the `macos-signing` lock with verified restore**; move the runner to its own macOS account later. Alternative: rcodesign (MPL-2.0) signs from the PKCS#12 with no keychain, but its last release is 0.29.0 of 2024-11-29 |
| D14 | Go back to the login keychain on macOS once signed | **Not now**; keep owner-only files and ADR 0057; revisit after one signed release cycle |
| D15 | Publisher name in the certificates | **"decryp7"** in CN and O; no e-mail or real name |
| D16 | Update keys | **One family minisign key** (corrects the old D16); Scrutineer inventories and backs it up, never makes per-app keys |
| D17 | Home | **`tools/scrutineer/`**, own Cargo workspace and lockfile, `scrutineer-v*` releases signed by itself plus minisign |
| D18 | Order | **macOS track first in time** (S1, A, B3, C1 macOS part, D1, E1m, E2, F1 macOS), Windows and Linux adapters beside it; first signed release Pitwall 1.1 or Dyno's next, whichever freezes later than E2 |

## 10. Work packages

Days are agent-days for one implementer. Every crate has 99% line coverage (its native glue in `native.rs` excluded with
reasons) and contract tests in a `testing` feature (rule 43); real stores only on the local VMs (ER-14).

| WP | Scope | Depends | Days |
|---|---|---|---|
| **S1** | Spike on local VMs and one host step: three-tier chain with root-pinned DR across a leaf and intermediate rotation and the standby clause; RSA vs ECDSA; expiry; the §5 hardened-runtime plan on Dyno; `tauri bundle` reusing a signed `.app`; Windows UAC with the root (and with the leaf in Trusted Publishers), `signCommand` coverage and `.sig` order, signature after leaf expiry with a timestamp; Linux p11-kit `objsign` anchor, `rpm` crate signing, apt `signed-by`; results as benchmark-style blocks | decisions | 3–4.5 |
| **ADR** | Revise this ADR with S1, independent review | S1 | 1–1.5 |
| **A1** | `paddock-pki` core: profiles, generation, fingerprints, encrypted PKCS#8 and PKCS#12, inventory, CRL, expiry, lint | ADR | 4–5.5 |
| **A2** | `paddock-pki` rotation: standby, renew and rotate plans, the DR hash set | A1 | 1.5–2 |
| **B1** | `paddock-sign` core: ports, bounded and fuzzed detection, nested walk, TSA client, minisign, OpenPGP, report, audit record, policy | A1 | 3–4 |
| **B3** | macOS adapter: `codesign` with the explicit DR, `-i` always, ephemeral keychain on the search list with verified restore | B1, A2 | 2.5–3.5 |
| **B2** | Windows adapter: `SignerSignEx2`, `WinVerifyTrust` with pins | B1 | 3–4.5 |
| **B5** | Linux adapter: `.rpm` and AppImage signatures and their verify | B1 | 2.5–3.5 |
| **C1** | `paddock-trust-store` Windows (Root, TrustedPublisher, Disallowed, usage property) and macOS (`SecTrustSettings`, `codeSign`) | A1 | 4–5.5 |
| **C2** | `paddock-trust-store` Linux: p11-kit anchor with purposes, Debian refusal, apt keyring and `rpm --import` | A1 | 1.5–2 |
| **D1** | Scrutineer CLI: every command of §2.2, confirmation flow, Windows self-elevation of `trust`, `doctor`, `audit show`, `backup --paper` | A2, B2, B3, B5, C1, C2 | 3–4 |
| **D2** | Scrutineer release workflow, README | D1 | 1–1.5 |
| **G1** | Owner guides: the key ceremony, backup, renewal calendar, "a key leaked" runbook, per-OS trust install | D1 | 1.5–2 |
| **R0** | Local identity on the Mac mini; `./dev` step that signs the runner binaries after an upgrade; test-app signing in CI with `.test` ids | D1 (or the spike recipe as an interim) | 0.5–1 |
| **E1m** | CI macOS: `ci prepare`/`cleanup` under the `macos-signing` lock, `APPLE_SIGNING_IDENTITY`, inside-out re-sign, DMG, tarball, `.sig` | D1, ceremony done | 2–3 |
| **E1w** | CI Windows: `signCommand`, `paddock-setup.exe` and `dyno-helper.exe` before embedding | D1 | 1.5–2 |
| **E1l** | CI Linux: package and AppImage signing | D1 | 1–1.5 |
| **E2** | Release gate (§3) with probe controls; C04 and `macos-verify` rewritten; rule 23 and 25 amendments; `signing-log.jsonl` | E1m, E1w, E1l | 1.5–2 |
| **F1** | Per-app adoption: identifiers, `signCommand`, release-note text, installers built once per app (rule 54) | E2 | 2–3 |
| **F3** | `paddock-tools` U5 onto `paddock-sign`'s verifier | B2, ADR 0051 | 0.5–1 |
| **K1** | `keys minisign` and `keys openpgp`: inventory, backup, verify (no per-app keys) | B1 | 0.5–1 |
| F2 | Windows installer opt-in root install (D11) in the `paddock-setup` include | C1, ADR 0053 | 3–4 |
| F4 | Read-only trust status in the installers and About | C1, ADR 0028 | 1–1.5 |
| H1 | Dyno helper checks its caller against the family DR | B3, F1 | 1.5–2 |
| L2 | Signed apt repository through Gitea's Debian registry | E1l | 1–2 |
| Later | YubiKey/PKCS#11 (2–3), GUI on the shell (5–8), rcodesign adapter (1.5–2), plugin key under `keys` (1) | | 9.5–14 |

**Totals:** Must (S1 to K1, bold) **41–58.5 days**; Should (F2, F4, H1, L2) **6.5–9.5 days**; Later 9.5–14 days. The
macOS track to a first signed app is about **22–30 days** (S1, ADR, A1, A2, B1, B3, the macOS half of C1, D1, G1, E1m,
E2, F1 for one app). With two builders in parallel (A then B3 and C1; B2, B5, C2 beside them) the calendar is roughly
three to four weeks after the decisions.

**Parallelism and cost (rule 51).** Signing is per file: hashing plus, on Windows, one TSA round trip. `sign` runs
independent files in parallel (`--jobs`, Auto = 4, network-bound); the macOS walk parallelises within one nesting level
only, because inner code is signed before its container, and serialises across jobs on the `macos-signing` lock.
`--if-needed` skips a file whose signature already matches the leaf and policy, for reruns. S1 measures the time per
file; a family installer has tens of files, so seconds are expected (unmeasured).

## 11. Consequences

- macOS users grant once per machine, not once per update. CI stops losing the runner's grants.
- Rule 23 changes (bundles signed with the family identity, runtime off until measured); rule 25's signing-secret step
  covers the new secrets; the "First launch" release-note text stays on macOS and Windows.
- The family gains key custody duties: an offline root, a paper backup, a leaf to renew every two years, a CRL. `doctor`
  and the gate warn ahead of every expiry.
- A lost root without the standby costs every macOS grant once and a re-install on every trusting Windows machine.
- The Scrutineer placeholder page, icons and family-doc entries come back with this ADR; the real icon, hero and page are
  later work (the user, 2026-11-04). The unlanded `chore/remove-scrutineer-icons` must not land.
