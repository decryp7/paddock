# Code signing with the owner's own CA: research (2026-11-04)

- Status: research for the plan [docs/plans/2026-11-04-code-signing.md](../plans/2026-11-04-code-signing.md). No ADR yet.
- Asked by: the user, 2026-11-04. The apps are for personal use. In the next release line (after Dyno 1.1.0), every family
  build is signed with the user's own code-signing certificate, from a CA the user makes. CI signs every build. A tool,
  **Scrutineer** (name accepted 2026-11-04), generates the certificates, installs the root on the user's machines, signs
  and verifies. It is one shared mechanism for every app, and it works for any app the user builds.
- Method: the Paddock sources (cited by path, read on `origin/main` at `4b6d99f57`, plus two design branches), web
  sources (each with a URL, read 2026-11-04), and two small local checks on macOS 27.2 (26B123) with throwaway data
  (section 9). **Verified** means I read it in a primary source or measured it here. **Secondary** means I took it from
  a blog, forum or other secondary source. **Unverified** means no source I could check confirms it; the plan's spike
  S0 measures it on the local test VMs.

## 1. Where the family stands today

| Area | Today | Source |
| --- | --- | --- |
| macOS | Ad-hoc seal (`signingIdentity: "-"`, `hardenedRuntime: false`) in every app; CI verifies with `codesign --verify --deep --strict` | rule 23 ([docs/rules.md](../rules.md)); `apps/*/app/src-tauri/tauri.macos.conf.json`; `.gitea/actions/macos-verify/action.yml` |
| Windows | NSIS, unsigned. No Authenticode configuration or secret | `conformance/ci/tauri-bundle.sh`; Dyno ADR [0022](../../apps/dyno/docs/adr/0022-helper-installed-with-the-app.md) "Signing in the release workflow" |
| Updater | minisign through Tauri; one family key `PADDOCK_TAURI_SIGNING_PRIVATE_KEY`; verified by `paddock-updater` (`minisign-verify`) | `.gitea/workflows/app-release.yml`, `dyno-release.yml`; `crates/paddock-updater/src/signature.rs` |
| Plugins | A separate Ed25519 family plugin key (`cargo-paddock-plugin keygen`) | [docs/plugin-signing-key.md](../plugin-signing-key.md) |
| Release notes | A "First launch" section (macOS **Open Anyway**, Windows **More info > Run anyway**) | rule 25; lessons "Release checklist" |
| Per-machine Windows (Dyno 1.1.0, then all) | The UAC prompt for `paddock-setup.exe` reads "Unknown publisher" until the family has a signing identity | ADR 0053 §4.1, "What stays" (branch `origin/docs/dyno-adr-0027-per-machine`, `docs/adr/0053-per-machine-windows-installs.md`) |
| `paddock-tools` | Checks the Authenticode signature of third-party installers: `WinVerifyTrust`, leaf and root SHA-256 pins, timestamp required (U5, D5) | ADR 0051 (branch `origin/docs/paddock-tools-design-v2`) |

Pain points the user has met, all from the ad-hoc seal:

- **macOS permission grants do not survive an update.** Lessons: "macOS permission grants (Screen Recording,
  Accessibility) are tied to the code signature, and an ad-hoc re-sign on update changes it." Section 9 shows why: an
  ad-hoc signature's designated requirement is a list of `cdhash` values, which change with every build.
- **SMAppService and launch constraints.** Lessons 2026-10-23: SMAppService keeps the old ad-hoc build's launch
  constraint (LWCR), so after every update the agent exits with `EX_CONFIG` (78) and loops until it is re-registered
  (rule 24).
- **Dyno's helper on macOS uses a plist LaunchDaemon instead of `SMAppService`.** Dyno ADR 0027 D2: Apple DTS says
  SMAppService daemons want an Apple-issued identity (forum answers, secondary).
- **"Unknown publisher" in every UAC prompt** (ADR 0053, Dyno ADR 0022 problem (a)).
- **Hardened runtime off.** Ad-hoc code under the hardened runtime cannot load vendor-signed libraries ("different Team
  IDs", libduckdb, rule 23).

## 2. Windows

### 2.1 Chain and stores

- **Profile.** A self-made root CA (`basicConstraints CA:TRUE`, `keyUsage keyCertSign, cRLSign`) issues a code-signing
  leaf (`keyUsage digitalSignature`, `extendedKeyUsage 1.3.6.1.5.5.7.3.3`). An optional intermediate sits between them
  (`pathLen 0`).
- **Where each certificate goes:**
  - **Trusted Root Certification Authorities (`Root`)** makes the chain trusted. `WinVerifyTrust` (UAC and the
    Authenticode checks) builds the chain to a trusted root.
  - **LocalMachine** is the store to use. Microsoft: a certificate added to the local machine Root store is also seen by
    every user's Root store
    (<https://learn.microsoft.com/en-au/windows-hardware/drivers/install/local-machine-and-current-user-certificate-stores>,
    verified). It needs an administrator.
  - **CurrentUser Root** works without admin, but Windows shows its own security warning ("You are about to install a
    certificate from a certification authority…") and only that user trusts it (Stack Overflow 4196997, secondary).
  - **Trusted Publishers (`TrustedPublisher`)** holds the **leaf** (not the root). It is what PowerShell `AllSigned`,
    ClickOnce, driver-package prompts and some software-restriction policies consult to mean "run without asking". UAC
    does not need it to show the publisher (unverified; S0 measures UAC with and without it). Putting the leaf there
    also means every leaf rotation is an install step on every machine. **Recommendation:** Root only by default.
    TrustedPublisher is an opt-in for the leaf, for PowerShell `AllSigned` users.
- **Constraining the root to code signing:**
  - **EKU in the CA certificates themselves.** Microsoft's `CERT_USAGE_MATCH` documents AND logic: a certificate is
    valid for a usage only if every certificate in the path supports it, and an absent EKU on a CA counts as support
    (<https://learn.microsoft.com/en-us/windows/win32/api/wincrypt/ns-wincrypt-cert_usage_match>, verified). A former
    Microsoft PKI engineer describes Windows' nested-EKU enforcement: once a CA in the path lists EKUs, nothing below it
    is good for an unlisted EKU (<https://unmitigatedrisk.com/?p=57>, secondary). So the root and intermediate carry
    `extendedKeyUsage codeSigning` (plus `timeStamping` only if a self-run TSA is ever chosen, which section 2.4 rejects).
    Windows then refuses the root for TLS server authentication.
  - **The store property.** `CERT_ENHKEY_USAGE_PROP_ID` on the installed copy is what the certificate MMC's "Enable only
    the following purposes" sets, as an ASN.1 `CERT_ENHKEY_USAGE` blob
    (<https://learn.microsoft.com/de-de/windows/desktop/api/certenroll/ne-certenroll-certenroll_propertyid>, verified
    that the property exists). The install tool sets it with `CertSetCertificateContextProperty` as a second, local
    restriction. That chain building honours it on current Windows is unverified; S0 checks a TLS handshake and a
    `WinVerifyTrust` call against a root with only the property and no EKU extension.
  - **Name constraints** constrain names (DNS, e-mail, directory names) in issued certificates, not their use. For code
    signing they add little. A directory-name constraint on the subject `O=` would stop a leaked intermediate from
    minting a leaf with another publisher name, but not a leaf with ours. rcgen supports `NameConstraints`
    (<https://docs.rs/rcgen/latest/rcgen/>, verified). **Recommendation:** EKU constraints, yes. Name constraints, an
    optional `permittedSubtrees` directoryName on the intermediate, off by default.

### 2.2 What UAC and SmartScreen show

- **UAC.** With a valid chain to a trusted root, the consent prompt shows "Verified publisher: <CN of the leaf>" instead
  of "Publisher: Unknown". I found no Microsoft page that says so for a privately rooted certificate (unverified). This
  is the main thing S0 measures, for the NSIS installer, `paddock-setup.exe apply-update` (ADR 0053 §4.1) and
  `dyno-helper.exe`.
- **SmartScreen** is reputation-based (file hash and publisher, from Microsoft's cloud) and checks files with the Mark of
  the Web. Microsoft's table, updated 2026-08-17: a self-signed certificate gets "Same behavior as no signature", and
  even an OV/EV certificate is "flagged as unrecognized until reputation accumulates". EV no longer bypasses it
  (<https://learn.microsoft.com/windows/apps/package-and-deploy/smartscreen-reputation>, verified). A locally trusted
  root earns no reputation, so **a browser-downloaded installer still meets "Windows protected your PC"**. Whether "More
  info" then shows our publisher name is unverified (S0). The release notes' Windows "First launch" text stays.
- **Smart App Control** (Windows 11, on only for clean installs that pass its evaluation) blocks unsigned or unknown
  code unless it is "signed with a certificate from a CA in the Trusted Root Program", and its checks "apply to all
  executable files, not just those downloaded from the Internet"
  (<https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/overview>, verified; the SmartScreen page
  above, verified). A self-made root is not in Microsoft's program, so **Smart App Control in enforcement mode blocks
  the family's apps either way**. Turning it off is the user's step. `scrutineer doctor` reports its state (Windows
  Security registry value; to be located in S0).
- **Updates through `paddock-updater`** are files the app downloads itself. Whether they carry the Mark of the Web is
  unverified (S0). They are verified by minisign before running (ADR 0053 §4.2).

### 2.3 Timestamps and expiry

- **Without a timestamp, an Authenticode signature fails once the signing certificate expires. With an RFC 3161
  countersignature it stays valid.** Microsoft: "time stamping allows Authenticode signatures to be verifiable even
  after the certificates used for signature have expired"
  (<https://learn.microsoft.com/en-us/windows/win32/seccrypto/time-stamping-authenticode-signatures>, verified). A
  measured demonstration: <https://blog.didierstevens.com/2015/11/24/authenticode-and-timestamping-and-sha256/>
  (secondary). For the family this matters twice:
  - the installed `paddock-setup.exe` raises a UAC prompt at every update (ADR 0053), and would fall back to "Unknown
    publisher" once the leaf has expired;
  - `paddock-tools` refuses a signature without a timestamp when its evidence says `timestamp = "required"` (ADR 0051 U5).
- **Options:**

  | Option | For | Against |
  | --- | --- | --- |
  | **A. A public RFC 3161 TSA** (DigiCert `http://timestamp.digicert.com`, Sectigo, others as fallbacks) | Free and widely used. It timestamps any signature hash, whoever the signer is (it never sees our certificate's trust). Its own chain is in every Windows root store | Network from the CI job (plain HTTP; the token is signed, so HTTP is normal). It sees a hash and the runner's address only. DigiCert rotated its TSA certificates on 2026-09-03 (<https://knowledge.digicert.com/general-information/rfc3161-compliant-time-stamp-authority-server>, verified) |
  | B. A self-run TSA (a timestamping leaf under our root) | No outside service | Its key would sit next to the signing key in CI, so it proves nothing against a leaked CI key. Our root would need the `timeStamping` EKU, which widens it. More code to run and keep alive |
  | C. No timestamp | Simplest | Every signature dies with its leaf, so short leaves become impossible. `paddock-tools`-style checks refuse it |

  **Recommendation: A**, with a fallback list, bounded retries and **no silent unstamped fallback**. If no TSA answers,
  the signing step fails with an ER-27 error naming each server tried. The survey found MariaDB's signtool retry falling
  back to an unstamped signature as the counter-example (<https://github.com/MariaDB/server/commit/99ecb33389>,
  secondary); jsign's `--tsaurl` list with `--tsretries` and `--tsretrywait` is the model (<https://ebourg.github.io/jsign/>,
  verified by the survey).

### 2.4 Signing in CI: engines

| Engine | Licence | Platforms | Maintenance (read 2026-11-04) | Notes |
| --- | --- | --- | --- | --- |
| Windows `SignerSignEx2`/`SignerSignEx3` (mssign32) and `WinVerifyTrust`, called from Rust through `windows` | OS API | Windows | Microsoft | No external tool and no SDK install. Signs PE files (exe, dll, the NSIS installer) with an in-memory certificate context. The `windows` crate is already in the family tree |
| `signtool.exe` | Proprietary, Windows SDK | Windows | Microsoft | What Tauri expects with `certificateThumbprint` (cert in a store). No TSA fallback of its own (survey: AdoptOpenJDK issue 1963) |
| jsign | Apache-2.0, Java | any | 7.6, 2026-10-12 (<https://github.com/ebourg/jsign>, verified) | TSA fallback list and retries, PKCS#11/YubiKey, exe, msi, cab, cat, appx, scripts. Needs a JRE on the runner |
| psign (Devolutions) | MIT, Rust | Windows; portable mode on any OS | No release shown on GitHub (<https://github.com/Devolutions/psign>, read) | A Rust port of signtool. The portable mode does RSA/SHA-256 PE and MSI signing and RFC 3161, and portable verify. No ECDSA mentioned. Promising, but young |
| osslsigncode | **GPL-3.0** | any | 2.14, 2026-07-20; 2.13 fixed memory-safety bugs (survey, <https://github.com/mtrojnar/osslsigncode/releases>) | Executed as a tool, not linked, but strong copyleft is outside the family policy without an ADR (PRINCIPLES "Dependencies") |
| relic | Apache-2.0, Go | any | v8.2.0, 2025-01-17 (survey) | A signing server plus CLI; slow releases |

Tauri hooks:

- **`bundle.windows.signCommand`** runs any command per file, with `%1` as the file
  (<https://v2.tauri.app/distribute/sign/windows/>, verified; the page uses relic as its example).
- **What Tauri signs** (from Tauri's change history, secondary; to be confirmed for the family's Tauri 2.12 in S0):
  - the main exe;
  - sidecars (`externalBin`), "if they are not signed already";
  - the NSIS uninstaller, through `!uninstfinalize` (a 2023 change; PR 13334 fixed relative paths, April 2025);
  - the stock NSIS plug-in DLLs. They were left unsigned until a May 2026 PR (<https://github.com/tauri-apps/tauri/pull/15422>).
    Whether 2.12.4 includes it is unverified.
- **What Tauri does not sign:**
  - `paddock-setup.exe` (ADR 0053 §2), which the NSIS include embeds with `File` and installs into `$INSTDIR`;
  - `dyno-helper.exe`, if it is not an `externalBin`.

  Both are signed by the bundle job before `tauri build`.

**Recommendation:** `paddock-sign`'s Windows adapter calls `SignerSignEx2` plus `WinVerifyTrust` natively (no tool to
install, one code path), and Tauri's `signCommand` calls `scrutineer sign %1`. That keeps one signing path (rule 55) and
covers every file Tauri signs. A **portable adapter** (psign's crates if they become a published library, else jsign as
an external tool) is "should have", so a Windows exe can be signed from the Mac or Linux. osslsigncode is not used.
That `SignerSignEx2` accepts an ECDSA leaf is unverified (S0); RSA-3072 is the fallback (section 6).

### 2.5 Which family files are signed

| File | Signed by | When |
| --- | --- | --- |
| `<app>.exe`, sidecars (`dyno-agent.exe`, `dyno.exe`, …), NSIS plug-in DLLs, the uninstaller, the installer | Tauri's `signCommand` -> `scrutineer sign` | during `tauri build` |
| `paddock-setup.exe` (ADR 0053), `dyno-helper.exe` | `scrutineer sign` in the bundle job | after `cargo build`, before `tauri build` embeds them |
| `scrutineer.exe` itself | its own release job, with the same leaf | at its tag |

The minisign `.sig` must be computed over the **signed** installer. That Tauri signs before it writes the updater
signature is unverified (S0). The ADR 0039 gate then checks both (section 8).

## 3. macOS

### 3.1 Designated requirements and TCC

- **TCC stores a code requirement (`csreq`) for each client when access is granted and checks later code against it.**
  It is commonly documented, but in secondary sources only:
  <https://book.hacktricks.xyz/macos-hardening/macos-security-and-privilege-escalation/macos-security-protections/macos-tcc>
  and the Jamf community "csreq blob" threads. Apple's own documentation of TCC internals is not public.
- **Ad hoc:** measured here (section 9). An ad-hoc signed binary's designated requirement (DR) is
  `cdhash H"…" or cdhash H"…" or …`. Every rebuild changes it, so every update loses Full Disk Access, Screen Recording
  and Accessibility grants. This is the known pain.
- **A self-signed identity:** `codesign`'s default DR for a self-signed certificate is
  `identifier "<id>" and certificate leaf = H"<sha1 of leaf>"`. The author reports TCC grants surviving rebuilds with
  one certificate, and that the certificate does not need to be trusted for this
  (<https://nickboy-github-io.pages.dev/posts/tcc-cdhash-trap/>, 2026-05-10, updated 2026-08-02, secondary). With a
  **leaf** hash, every leaf rotation breaks the grants once.
- **The requirement language allows anchoring on the root instead:** `certificate root` ("the anchor certificate"),
  `certificate leaf`, hash syntax `H"<40 hex>"` (SHA-1 only), and `anchor trusted` (succeeds if a certificate in the
  chain "is marked trusted for the code signing certificate policy in the system's Trust Settings database")
  (<https://developer.apple.com/library/archive/documentation/Security/Conceptual/CodeSigningGuide/RequirementLang/RequirementLang.html>,
  verified). It says nothing about expiry.
- **Recommendation:** sign every Mach-O with an **explicit DR**, passed as `codesign -r`:

  ```text
  designated => identifier "<bundle or binary id>" and
                (certificate root = H"<sha1 of current root>" or certificate root = H"<sha1 of standby root>")
  ```

  - Rotating the leaf (or the intermediate) keeps every grant.
  - A **standby root**, generated with the current one and kept offline, lets one root rotation also keep the grants:
    TCC's stored `csreq` already accepts code under the standby root.
  - `anchor trusted` is not used, because it would make a grant depend on the trust settings, which an expired or
    removed root breaks.
  - `csreq` compiles this form (section 9). That TCC honours it for a self-signed root is S0's main macOS measurement:
    grant Full Disk Access, update to a build signed with a new leaf, then to one under the standby root.
- **Dyno's dotted identifier** (Dyno ADR 0026: the kernel socket list needs a code-signing identifier with a dot)
  survives: the identifier is part of the DR, and `codesign -i` stays.

### 3.2 Gatekeeper, notarization, quarantine

- Gatekeeper's assessment of a quarantined download needs a Developer ID signature and notarization. A locally trusted
  private root does not satisfy it (secondary: rcodesign's documentation says self-signed signatures "won't confer the
  same trust", <https://gregoryszorc.com/docs/apple-codesign/main/apple_codesign_certificate_management.html>; to be
  confirmed in S0). **The first install from a browser download still needs "Open Anyway"**, so the macOS "First
  launch" text stays.
- Whether `paddock-updater`'s own download sets `com.apple.quarantine` is unverified (S0). If it does not, updates are
  not assessed, as today.
- Notarization needs an Apple Developer Program membership (USD 99 a year; Dyno ADR 0022 D4). That stays out of scope
  by the user's decision of 2026-10-23 and 2026-11-04.

### 3.3 Hardened runtime and library validation

- Library validation accepts only libraries from the same Team ID or Apple. A self-signed certificate carries no Apple
  Team ID (the `OU` is taken as Team ID only for Apple-issued certificates; requirement-language page above, verified
  for the `subject.OU` meaning). Whether every Mach-O re-signed with the same self-signed identity passes library
  validation is unverified.
- The hardened runtime is needed only for notarization. **Recommendation:** keep it off (rule 23 already turns it off
  while ad hoc; the amendment changes the wording from "while ad hoc" to "until notarized"). S0 tries it on with
  `com.apple.security.cs.disable-library-validation`, for information only.

### 3.4 SMAppService and launchd helpers

- DTS forum answers say `SMAppService` daemons want an Apple-issued identity (Dyno ADR 0027, forum threads 733046,
  761249, 757463; secondary).
- A stable, explicit DR may also end the launch-constraint loop of lessons 2026-10-23, if LWCR records the DR rather
  than the `cdhash` (unverified).
- **Recommendation:** no design change now. Dyno's plist LaunchDaemon (ADR 0027 D2) and the agents' re-registration
  (rule 24) stay. S0 records whether SMAppService agents keep working across an update signed with a new leaf, and
  whether a daemon registers at all, so a later ADR can decide.

### 3.5 Installing trust

- `security add-trusted-cert -d -r trustRoot -p codeSign -k /Library/Keychains/System.keychain root.cer` (admin domain)
  or without `-d` (user domain).
- **Since macOS 11, root alone cannot change trust settings.** Apple's 11.0.1 release notes, quoted in the forum thread:
  the admin domain needs an administrator's password and the user domain the user's own, and a script run as root gets
  "The authorization was denied since no user interaction was possible"
  (<https://developer.apple.com/forums/thread/686068>, <https://github.com/FiloSottile/mkcert/issues/415>, secondary).
  The `authorizationdb` workaround no longer works on Sequoia (Jamf community, secondary). Only an MDM configuration
  profile avoids the prompt.
- That is the behaviour we want: **the person confirms every trust change**. The tool calls `SecTrustSettingsSetTrustSettings`
  (Apple recommends the API over the `security` tool, same forum thread), shows the fingerprint first, and lets macOS
  show its own password prompt.
- `-p codeSign` limits the trust to the code-signing policy. For SSL the root then has no trust setting, so it is not
  trusted for TLS. Forum reports say a self-signed chain may also need the leaf marked
  (<https://developer.apple.com/forums/thread/712043>, secondary). S0 checks the root alone.
- Signing does not need trust at all. `codesign` only needs the private key, and an untrusted identity is hidden by
  `security find-identity -v` but usable (nickboy, secondary). On macOS the root install is needed only for `anchor
  trusted` checks and for `codesign --verify` to say "valid" with a trusted chain; **TCC does not need it** if the DR
  pins the root hash. On macOS the trust install is therefore a nice-to-have, kept for `verify`/`doctor` and for Finder's
  "Get Info" showing a trusted signer.

### 3.6 Signing engines

| Engine | Licence | Notes |
| --- | --- | --- |
| Apple `codesign` | Proprietary, macOS only | The reference. Needs the identity in a keychain. CI uses an **ephemeral keychain** in the job's temp folder, passed with `codesign --keychain`, never added to the search list, deleted at job end. The Mac runner is the user's Mac mini, so ER-14 applies |
| rcodesign (`apple-codesign`) | MPL-2.0 (allowed) | Signs Mach-O, bundles, DMG and pkg from any OS with a PEM or p12, no keychain. **The last release is 0.29.0 of 2024-11-29**; the repository still has commits in 2026 (<https://lib.rs/crates/apple-codesign>, verified; survey). A large dependency tree |

**Recommendation:** `codesign` with an ephemeral keychain as the default adapter. rcodesign is a "later" adapter, pinned
to a git revision, for signing Mac bundles on Linux.

### 3.7 Tauri on macOS

- Tauri signs the `.app` with `signingIdentity` (and can import `APPLE_CERTIFICATE`), then builds the DMG and the
  updater `.app.tar.gz` with its minisign `.sig` in the same run (`conformance/ci/tauri-bundle.sh`).
- Tauri does not pass a custom designated requirement (unverified), so the explicit DR of 3.1 needs our own signing
  pass: `tauri build --bundles app` (Tauri seals ad hoc), then `scrutineer sign --deep` re-signs inside-out with the DR
  per identifier, then the DMG, the `.app.tar.gz` and its minisign signature are made from the signed `.app`. Whether
  `tauri bundle --bundles dmg,updater` reuses an existing `.app` is unverified (S0). If not, the bundle script makes
  the DMG with `hdiutil` and the tarball plus minisign signature itself, the latter through `scrutineer sign
  --minisign`.

## 4. Linux

- **AppImage** can embed a GPG signature (`appimagetool --sign`), but neither the kernel nor desktops check it. Only
  AppImageUpdate and some launchers do (secondary).
- **.deb.** dpkg does not verify package signatures by default (`debsig-verify` is opt-in). apt verifies the
  **repository's** signed `Release` file. Gitea's Debian registry signs its repository index with a key Gitea makes
  (<https://docs.gitea.com/usage/packages/debian>; read, the signing detail secondary).
- The Tauri updater on Linux already verifies minisign.
- **An X.509 root on Linux would be trusted for TLS.** `update-ca-certificates` has no per-purpose trust, and p11-kit's
  purposes are not honoured by every TLS stack (secondary).
- **Recommendation:** no X.509 signing on Linux and **no root install on Linux** (`scrutineer trust install` refuses
  with a reason). Later, optionally, publish the `.deb` through Gitea's Debian registry, so apt checks a signed
  repository.

## 5. Key handling

### 5.1 Hierarchy

| | Root | Intermediate (recommended) | CI signing leaf |
| --- | --- | --- | --- |
| Key | ECDSA P-384 (fallback RSA-4096) | ECDSA P-384 | ECDSA P-256 (fallback RSA-3072) |
| Life | 20 years | 5 years | 1 year (397 days); renew at 60 days left |
| Constraints | CA, `pathLen 1`, EKU codeSigning | CA, `pathLen 0`, EKU codeSigning, optional name constraint | not CA, `digitalSignature`, EKU codeSigning only |
| Kept | Offline: an encrypted file on two offline media, a paper backup; optional YubiKey | The owner's Mac: encrypted file, passphrase in the login keychain; optional YubiKey | A CI secret (PKCS#12, password a second secret), loaded only by signing jobs |
| Used | Generates the intermediate, signs CRLs for it; every 5 years | Issues a leaf every year, signs leaf CRLs | Every signed build |

- **Why an intermediate:** the root key is touched once in five years, and the yearly leaf needs only the Mac-held
  intermediate. Every machine's trust (Windows Root, macOS trust settings) and every macOS DR name the **root**, so
  replacing the intermediate or the leaf changes nothing on installed machines. Cost: one more certificate in every
  signature, and one more ceremony to document. A two-tier chain (root issues leaves) is the simpler alternative (D1).
- **A standby root** is generated at `ca init`, kept offline like the root and never installed. Its hash sits in every
  DR from the first signed build, so a planned root rotation keeps TCC grants (section 3.1).
- **ECDSA:** rcgen generates P-256 and P-384 with `ring`, the family's crypto provider. RSA key generation needs rcgen's
  `aws_lc_rs` feature (`RsaKeySize`, <https://docs.rs/rcgen/latest/rcgen/>, verified). That ECDSA leaves work in
  `SignerSignEx2`, `WinVerifyTrust`, UAC and `codesign` is S0's first check. If one fails, the leaf and its chain fall
  back to RSA through `aws_lc_rs`, in the offline tool only.

### 5.2 Never the root key in CI

- The root and intermediate keys never leave the owner's machine or the offline media. CI gets the leaf's PKCS#12 and
  password, nothing else.
- Every CA command prints the warning, and `scrutineer ca export` refuses to write a CA private key to a path under a git
  checkout or the CI work folder.
- The family's secrets are readable by every workflow on every branch (Dyno ADR 0022 "Open for 1.1" P1). The signing
  leaf has the same exposure as the minisign key today. Mitigations: signing jobs only on `main`, tags and manual
  dispatch; branch and PR builds sign with a **throwaway CA made in the job**, so the signing path is still exercised;
  a one-year leaf; the signing audit log (section 8). Whether Gitea Actions can scope secrets to protected refs is
  unverified (to check before WP-C1).

### 5.3 Rotation, revocation, expiry

- **Leaf renewal (yearly):**
  1. `scrutineer ca issue --profile ci-leaf` on the owner's Mac, using the intermediate.
  2. Store the new PKCS#12 as the CI secret.
  3. The old leaf stays valid for signatures already made, thanks to the timestamp.
  4. Nothing changes on installed machines.
- **Intermediate rotation (5 years):** the same, with the root brought out. Nothing changes on installed machines.
- **Root rotation (planned, 20 years or on compromise):**
  1. Install the standby root on every machine (`scrutineer trust install`).
  2. Make it the active root and generate a new standby.
  3. Sign new builds under the new chain. Their DRs list the new pair; TCC grants carry over through the old DR's
     standby clause.
  4. After every machine has updated, remove the old root.

  Installed apps signed under the old root keep validating while the old root is trusted. Removing it turns their
  Windows signature "untrusted" (UAC: "Unknown publisher") until they update; macOS TCC is unaffected because DRs pin
  hashes, not trust.
- **Revocation:**
  - A CRL per CA (rcgen `CertificateRevocationListParams`, verified), published as a file on a Gitea release.
  - No CRL distribution point in the leaf by default. A CDP makes every verification depend on reaching the CRL URL,
    and Windows' handling of an offline CDP for a private root is unverified.
  - **The working revocation is local:** `scrutineer trust sync` puts revoked certificates into Windows' `Disallowed`
    store and gives them a macOS "deny" trust setting, from the published CRL, after showing the list.
- **Expiry:** see 2.3. `scrutineer status` and the signing step warn at 60 days left and fail at 14 days; the signing
  step refuses an expired leaf.

### 5.4 Storage

| Store | For | Crate (licence, latest, read 2026-11-04) |
| --- | --- | --- |
| Encrypted PKCS#8 (PBES2, scrypt, AES-256) | root, standby, intermediate (default) | `pkcs8` 0.11.0 (Apache-2.0 OR MIT, RustCrypto) |
| PKCS#12 | the CI leaf, for `SignerSignEx2` and `codesign` import | `p12-keystore` 0.4.1 (MIT/Apache-2.0) |
| OS keychain | the intermediate's passphrase | `paddock-secrets` (`keyring` 4.2), existing |
| YubiKey PIV | root or intermediate, optional | `yubikey` 0.8.0 (BSD-2-Clause, iqlusion) or `cryptoki` 0.12.1 (Apache-2.0, PKCS#11) |
| Paper | root and standby backup | the encrypted PKCS#8 as base64 lines with a per-line checksum, plus the fingerprints; the passphrase kept separately |

- The macOS Secure Enclave holds only non-exportable P-256 keys, so it cannot be backed up: not for a CA.
- Windows CNG non-exportable keys have the same problem.

## 6. Crates for generation, signing and verifying (licences against `crates/deny.toml`)

| Crate | Use | Licence | Latest (crates.io, read 2026-11-04) |
| --- | --- | --- | --- |
| `rcgen` | certificates, CSRs, CRLs, EKU, name constraints, `pathLen` | MIT/Apache-2.0 (rustls org) | 0.14.10, 2026-08-28 |
| `x509-parser` | inspect, fingerprint, lint any certificate | MIT OR Apache-2.0 | 0.18.1 |
| `x509-cert`, `cms` | parse PKCS#7/CMS signatures and RFC 3161 tokens in the portable `verify` | Apache-2.0 OR MIT | 0.3.0, 0.2.3 |
| `pkcs8`, `p12-keystore` | key files | above | above |
| `minisign` | updater key generation and signing (verify stays `minisign-verify` in `paddock-updater`) | MIT | 0.10.0 |
| `windows` | `SignerSignEx2`, `WinVerifyTrust`, cert stores | MIT OR Apache-2.0 | in the tree |
| `security-framework` | `SecTrustSettings`, keychain | MIT OR Apache-2.0 | in the tree (unverified that it wraps trust-settings writes; else FFI in `native.rs`) |
| `yubikey`, `cryptoki` | optional hardware keys | BSD-2-Clause, Apache-2.0 | above |
| `apple-codesign` | later macOS-on-Linux adapter | MPL-2.0 | 0.29.0, 2024-11-29 |

All are on the allow-list. Executed tools: Apple `codesign` and `security` (macOS), Windows nothing, optionally jsign
(Apache-2.0, external JRE). osslsigncode (GPL-3.0) is not used.

## 7. Installing the certificate from every app installer (evaluated on request)

The user asked whether each app's installer could also install the certificate.

**The risk.** A root trusted machine-wide is trusted for everything it is not constrained against. Unconstrained, a
leaked root key lets its holder mint TLS certificates for any site (and Firefox imports Windows' enterprise roots by
default, secondary), as well as sign code that UAC shows as ours. Every machine that ever installed it is exposed until
the root is removed.

**Mitigations, and what each buys:**

| Mitigation | Effect | Verified? |
| --- | --- | --- |
| EKU codeSigning in the root and intermediate (nested EKU) | Windows refuses the chain for TLS | Microsoft documents AND logic; nested enforcement secondary; S0 measures |
| `CERT_ENHKEY_USAGE_PROP_ID` on the installed root | A second, local restriction | property exists (verified); enforcement S0 |
| macOS trust settings `-p codeSign` | Trusted only for code signing | Apple documents per-policy trust; S0 |
| Name constraints | Do not constrain use; little value here | rcgen supports them |
| TrustedPublisher only (no Root) | Without a trusted root `WinVerifyTrust` fails: UAC would still say "Unknown publisher". **Not enough on its own** | Secondary; S0 |
| Opt-in, unticked, with fingerprint and publisher shown | Consent | design |
| Never on an update | No silent widening | design |
| Remove on the last app's uninstall (reference count) | Leaves nothing behind | design |

**Chicken and egg.** The first installer cannot be verified by the certificate it installs. The fingerprint it shows
is part of the same unverified download: a tampered installer shows the attacker's fingerprint just as convincingly.
Consent from a fingerprint is only worth something when the person compares it with a copy they got another way. Here
the person is the CA's owner, who has the fingerprint on paper and in the password manager. That holds equally for
Scrutineer and for an installer step; neither verifies itself.

**Platform reach.**

- Windows NSIS can do it: the per-machine installer is already elevated, so the `paddock-setup` include could call
  `paddock-trust-store`.
- macOS cannot: a DMG has no installer, and only the app itself could ask, with its own password prompt.
- Linux must not (section 4).

So the installer route covers one platform of three and adds a second way to change trust, against "one shared
mechanism".

**Other costs.**

- The NSIS include gains a page, a checkbox and reference counting (registry markers per app plus one for
  Scrutineer).
- ADR 0053's elevated surface grows.
- Every installer embeds the root certificate, so a root rotation needs a release of every app.
- Uninstalling the last app silently removes trust the user may still want for other self-signed tools.

**Recommendation: no install from the app installers.** Scrutineer is the one place that changes trust, once per
machine, with the fingerprint shown and compared. The installers get a **read-only check** instead:

- the `paddock-setup` include logs whether the family root is trusted and, if not, says once at the end of a manual
  install: "Windows will show 'Unknown publisher' for <App>'s updates. Run Scrutineer to trust your signing
  certificate.";
- the app's About page shows the same status, from `paddock-trust-store`'s read side.

**Fallback,** if the user wants it anyway: Windows only, an unticked box on the NSIS finish page with the publisher
and SHA-256 fingerprint, never on `/UPDATE`, root with both EKU constraints, reference-counted removal, all through
`paddock-trust-store` (the same code as Scrutineer, rule 55). Size: 3 to 4 days.

## 8. Comparable tools: what they offer, what users praise and miss

From a survey of 2026-11-04 (about 28 lookups). Release dates come from GitHub's releases API; licences marked
"(unverified)" were not re-read.

| Tool | Relevant offer | Praised | Missing or criticised | Licence, latest |
| --- | --- | --- | --- | --- |
| mkcert | `-install`/`-uninstall` into the system, NSS and Java stores; `$CAROOT`; only `rootCA.pem` goes to other machines (<https://github.com/FiloSottile/mkcert>) | one command that just works; a plain warning that the root key gives "complete power to intercept" | TLS only; no rotation or revocation; no name constraints on the root (asked for in issue 131); stale | BSD-3 (unverified), v1.4.4 2022-04-26 |
| step-cli / step-ca | `certificate create --profile root-ca\|intermediate-ca\|leaf`, `--not-after`, `inspect`, `fingerprint`, `lint`, `needs-renewal`, `install`/`uninstall`, `--kms yubikey:\|pkcs11:`; an unencrypted key only with `--insecure` (<https://smallstep.com/docs/step-cli/reference/certificate/>); `ca renew --daemon` at 2/3 of life | short-lived certificates as the default way of working; inspect and lint | no code signing; step-ca is a server, more than one person needs | Apache-2.0 (unverified); cli v0.31.0 2026-09-25 |
| XCA | GUI; key and certificate database; templates; CRLs; PKCS#11 tokens; name constraints (<https://hohnstaedt.de/>) | complete, for a desktop CA | GUI only, not for CI | BSD-3 (unverified), 2.9.0 2025-03-28 |
| Vault PKI / OpenBao | roles that fix EKU, key usage and TTL (`code_signing_flag`, `ext_key_usage`); several issuers per mount for rotation; CRL and OCSP; audit devices (<https://developer.hashicorp.com/vault/api-docs/secret/pki>) | policy enforced by role; audit | a server; Vault is BUSL-1.1 (unverified), not on the allow-list; OpenBao (MPL-2.0) gained external keys in 2.6 (<https://openssf.org/blog/2026/08/06/announcing-openbao-v2-6/>) | — |
| signtool / certutil | sign and verify; `certutil -addstore Root\|TrustedPublisher` | the reference | no TSA fallback; `/t` vs `/tr` mix-ups (AdoptOpenJDK issue 1963) | proprietary |
| osslsigncode | sign, verify, timestamp, attach and extract for PE, MSI, CAB, CAT, APPX, scripts, on any OS | cross-platform | 2.13 was a security release (PE and APPX parsing): parsers of untrusted files need bounds and fuzzing | GPL-3.0, 2.14 2026-07-20 |
| jsign | TSA list with retries; PKCS#11, YubiKey, cloud KMS; `verify` in 8.0 preview (<https://ebourg.github.io/jsign/>) | the best timestamp handling | recent fixes for locked files and Homebrew YubiKey paths | Apache-2.0, 7.6 2026-10-12 |
| relic | one signing server for many formats behind one key layer | breadth | slow releases | Apache-2.0 (unverified), v8.2.0 2025-01-17 |
| SignPath | an artifact configuration that signs nested files (an MSI and its contents) in one pass and checks third-party signatures; origin verification ties a request to a CI build of a commit (<https://docs.signpath.io/artifact-configuration>, <https://docs.signpath.io/origin-verification>) | nested signing; provenance | SaaS; the free Foundation tier needs OSI-licensed code, and the Foundation becomes the publisher (<https://signpath.org/terms>) | commercial |
| Azure Artifact Signing (Trusted Signing) | public trust, short-lived certificates, SmartScreen reputation | Microsoft's recommended path | "Individual developers must be located in the United States or Canada" (<https://learn.microsoft.com/azure/artifact-signing/quickstart>, updated 2026-10-08); paid | SaaS |
| codesign / notarytool | the reference signer; notarization | — | macOS only; notarization needs the paid program | proprietary |
| rcodesign | signs on Linux and Windows; `analyze-certificate`, `print-signature-info`, `diff-signatures`; PEM, p12, keychain, YubiKey, PKCS#11 (<https://gregoryszorc.com/docs/apple-codesign/main/apple_codesign_debugging.html>) | cross-platform, readable signature dumps | releases stalled since 2024-11 | MPL-2.0, 0.29.0 2024-11-29 |
| Sigstore / cosign | keyless OIDC signing, Rekor transparency log, `verify-blob --certificate-identity` (<https://docs.sigstore.dev/cosign/verifying/verify/>) | provenance, offline bundles | no OS-native story: Fulcio's root is in no OS store, with no plans (<https://github.com/sigstore/fulcio/issues/250>) | Apache-2.0, v3.1.4 2026-10-20 |

Ideas taken for Scrutineer (the catalogue is in the plan):

- mkcert's one-command install, uninstall and status, with a public-only export;
- step's fixed profiles, `--not-after`, `inspect`, `fingerprint`, `lint`, `needs-renewal`, and its "unencrypted key
  only on purpose";
- Vault's roles as **fixed profiles** (code signing only, nothing free-form);
- jsign's TSA list with retries and never an unstamped fallback;
- SignPath's nested signing from a declared manifest, and its origin record in the audit log;
- rcodesign's readable signature dumps and its diff against Apple's tools;
- osslsigncode's lesson that parsers of untrusted files are fuzzed and bounded.

Rejected for one person:

- a CA server (step-ca, Vault, relic);
- a public transparency log (cosign/Rekor): the OS does not use it, and the apps are private.

## 9. Local checks (macOS 27.2, 26B123, synthetic data, 2026-11-04)

1. **Ad-hoc DR.** `cp /bin/echo probe; codesign --force -s - -i net.decryptology.probe probe; codesign -d -r- probe`
   printed `designated => cdhash H"8399e9fa…" or cdhash H"4cc371f8…" or cdhash H"a3862d2c…"`, `Signature=adhoc`,
   `TeamIdentifier=not set`. Any rebuild changes those hashes, so a TCC grant cannot survive it.
2. **The explicit DR compiles.** `csreq -r='identifier "net.decryptology.dyno" and (certificate root = H"<40 hex>" or
   certificate root = H"<40 hex>") …' -b req.bin; csreq -r req.bin -t` round-trips the expression. Compiling it proves
   only the syntax; whether TCC accepts it is S0.

No keychain, trust setting or system state was touched. The scratch files were deleted.

## 10. Open measurements for spike S0 (local VMs only, after Dyno 1.1.0; ER-14, never CI)

Windows 11 VM:

- UAC with the root in LocalMachine Root, with and without the leaf in TrustedPublisher;
- `SignerSignEx2` and `WinVerifyTrust` with ECDSA P-256 and RSA-3072 leaves;
- nested EKU and `CERT_ENHKEY_USAGE_PROP_ID` against a TLS handshake;
- SmartScreen's "More info" text for a Mark-of-the-Web installer;
- Smart App Control state detection;
- the Mark of the Web on `paddock-updater` downloads;
- Tauri 2.12's `signCommand` coverage (plug-in DLLs, uninstaller, sidecars), and the order of signing and the updater
  `.sig`;
- signature validity after the leaf's expiry, with and without a timestamp (VM clock moved).

macOS VM:

- an explicit root-hash DR, and a Full Disk Access and Screen Recording grant across:
  - a new leaf;
  - a new intermediate;
  - the standby root;
  - an expired leaf;
- `SecTrustSettingsSetTrustSettings` with `codeSign` in both domains, and the prompt text;
- quarantine on updater downloads;
- `tauri bundle` reusing a signed `.app`;
- SMAppService agents across a leaf change;
- the hardened runtime with one self-signed identity;
- Gatekeeper on a quarantined, self-signed DMG.
