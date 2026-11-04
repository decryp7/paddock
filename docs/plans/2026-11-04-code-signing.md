# Plan: the family signs with the owner's own CA, through Scrutineer (2026-11-04)

- Status: **draft for the user's decisions** (section 9). An ADR follows once they are made. Sizes become firm with the
  ADR's work packages ("estimate after design").
- Research and sources: [docs/research/2026-11-04-code-signing.md](../research/2026-11-04-code-signing.md) (cited as
  R§n).
- The user's direction (2026-11-04):
  - personal use;
  - the next release line after Dyno 1.1.0 is signed with the user's own certificate, from a self-made CA;
  - CI signs every build;
  - one shared mechanism for every app;
  - one tool, **Scrutineer** (name accepted), that generates the certificates, installs the root on the user's
    machines, signs and verifies, and works for any app the user builds. CI uses the same command (rule 55).
  - Shared library crates under a thin CLI: `paddock-pki`, `paddock-sign`, `paddock-trust-store`.
- Related:
  - rules 23, 24, 25, 42, 43, 54 and 55;
  - ADR 0039 (release gates inspect the artifacts);
  - ADR 0053 (per-machine Windows installs, "What stays", on `origin/docs/dyno-adr-0027-per-machine`);
  - ADR 0051 (paddock-tools, U5 Authenticode, on `origin/docs/paddock-tools-design-v2`);
  - ADR 0022 (the Rust developer tool, tools in their own workspace);
  - Dyno ADRs 0022, 0026 and 0027;
  - lessons on TCC and the ad-hoc re-sign.

## 1. What changes for the user

| Today (ad hoc, unsigned) | After |
| --- | --- |
| UAC: "Publisher: Unknown" for every installer, `paddock-setup.exe` updates (ADR 0053) and `dyno-helper.exe` | "Verified publisher: <your name>" on every machine where Scrutineer installed your root (S0 confirms) |
| macOS: Full Disk Access, Screen Recording and Accessibility grants lost at every update (ad-hoc DR = `cdhash`, R§9) | Grants survive updates, leaf renewals, intermediate rotations and one planned root rotation (explicit root-hash DR, R§3.1; S0 confirms) |
| No way to tell a family binary from a lookalike | `scrutineer verify <file>` names the signer, chain, timestamp and whether this machine trusts it |
| SmartScreen on a browser download; Gatekeeper "Open Anyway" on first install; Smart App Control blocks | **Unchanged.** None of them honours a private root (R§2.2, R§3.2). The "First launch" release-note text stays |

## 2. Design

### 2.1 Chain (defaults; decisions D1 to D5)

```text
Root CA (offline, 20 y, P-384, EKU codeSigning, pathLen 1)    Standby root (offline, never installed)
   └─ Intermediate "Code Signing CA <year>" (owner's Mac, 5 y, P-384, EKU codeSigning, pathLen 0)
        └─ CI signing leaf (CI secret, 1 y, P-256, digitalSignature, EKU codeSigning)
```

- Machines trust the **root**, Windows in LocalMachine `Root` with the code-signing-only property, macOS in trust
  settings with `-p codeSign`, never on Linux (R§2.1, R§3.5, R§4).
- Every macOS signature carries an explicit designated requirement:
  `identifier "<id>" and (certificate root = H"<root>" or certificate root = H"<standby>")` (R§3.1).
- Windows signatures carry an RFC 3161 timestamp from a public TSA list. No timestamp means no signature (R§2.3).
- The root key never reaches CI. The CI secret is the leaf only (R§5.2).

### 2.2 Crates (rule 42: shared first), and what they reuse

| Crate | Owns | Reuses (existing) | Pure core / adapters |
| --- | --- | --- | --- |
| **`paddock-pki`** (new) | Fixed certificate **profiles** (root, standby, intermediate, ci-leaf, test-leaf; no free-form templates), generation (`rcgen`), fingerprints (SHA-256 for people, SHA-1 for DRs), encrypted PKCS#8 and PKCS#12 files, the public **inventory** (which certificates exist, expiry, roles), CRLs, expiry and renewal plans, rotation plans, lint | `paddock-secrets` (the intermediate's passphrase in the OS keychain) | Pure: profiles, plans, lint, inventory. Adapters: key stores (file; YubiKey/PKCS#11 later) |
| **`paddock-sign`** (new) | The `Signer` and `Verifier` ports; format detection with bounded parsers (PE, Mach-O, bundle, DMG, any file); the TSA client with fallbacks; the inside-out walk of nested files; the macOS DR builder; minisign signing; the **verify report**; the **audit record**; a release **policy** check | `paddock-updater::signature` (minisign verify: the same code the apps use, rule 55); `paddock-net` (the TSA egress, an explicit plain-HTTP allowance for RFC 3161); `paddock-proc` (runs `codesign` and `security` supervised, with timeouts) | Pure: detection, walk order, DR text, report, policy. Adapters: Windows (`SignerSignEx2`, `WinVerifyTrust`), macOS (`codesign` with an ephemeral keychain), portable Authenticode (later), rcodesign (later) |
| **`paddock-trust-store`** (new) | Install, remove, list and check the root in Windows `Root` (with `CERT_ENHKEY_USAGE_PROP_ID`), optionally the leaf in `TrustedPublisher`, revoked certificates in `Disallowed`; macOS trust settings via `SecTrustSettingsSetTrustSettings` with the `codeSign` policy (admin or user domain); Linux: refuse with a reason; the read side for status in apps and installers | `paddock-proc` and the elevation pattern of ADR 0053's `paddock-setup` (the elevated part is a single command whose arguments are re-validated) | Pure: the plan (what to add or remove where), the confirmation text. Adapters: one `native.rs` per OS |

**ADR 0051 overlap.** `paddock-tools` U5 verifies third-party Authenticode signatures with `WinVerifyTrust`, thumbprint
pins and a required timestamp. That check moves into `paddock-sign`'s Windows verifier, and `paddock-tools` calls it:
one implementation (rule 55, WP F3). Its `./dev tools pin` reads thumbprints through `paddock-sign`'s report.

**Not reused.** `paddock-service`'s privileged helper: trust changes are rare, one-shot and need the person's
confirmation, so no service is involved. `paddock-plugin-package`'s Ed25519 key keeps its own format and purpose
(docs/plugin-signing-key.md: one key per purpose); Scrutineer can later manage it too (H6).

### 2.3 Scrutineer: the thin CLI

- Location: `tools/scrutineer/`, its own Cargo workspace and lockfile, like `tools/paddock-dev` (ADR 0022).
- Generic: nothing in it names Paddock, and it signs any app.
- Released from a `scrutineer-v*` tag, its own binaries signed by itself, with the minisign signature as well.

| Command | Does | Privilege | Network |
| --- | --- | --- | --- |
| `ca init` | Root, standby root and intermediate. Prints fingerprints and **"Never copy a CA key to CI or a repository"**. Writes encrypted key files and a paper backup sheet | user | none |
| `ca issue --profile ci-leaf\|test-leaf` | A leaf from the intermediate; `--p12 <out>` for the CI secret | user | none |
| `ca renew`, `ca rotate --intermediate\|--root` | Plans, shows and does the rotation; refuses to touch the root without `--root` and the offline file | user | none |
| `ca revoke`, `ca crl` | Adds to the CRL; writes the signed CRL file for a Gitea release | user | none |
| `ca export --public` | The root or chain as `.cer`/`.pem` for installing. **Refuses** to write a private CA key under a git checkout or a CI work folder | user | none |
| `ca backup --paper` | The printable sheet: encrypted key as base64 lines with line checksums, fingerprints, restore steps; never the passphrase | user | none |
| `fingerprint <file>` | SHA-256 (grouped for reading aloud) and SHA-1 of a certificate, or of the signer of a signed file | user | none |
| `trust install <root.cer>` | Shows subject, validity, constraints and SHA-256, then asks for the first 8 hex digits **from the person's own record** (`--expect-sha256` for scripts still prints everything). Installs per R§2.1 or R§3.5. Never silent | Windows: elevates this one command (UAC); macOS: the OS password prompt | none |
| `trust remove`, `trust status` | Removes ours only (by fingerprint); lists what is installed where | remove as install; status as user | none |
| `trust sync` | Fetches the published CRL, shows the revoked certificates, adds them to `Disallowed` or a macOS deny setting | as install | the CRL URL only |
| `sign <files…>` | Detects the format; Authenticode with a timestamp (fallback list, retries, **never** unstamped), `codesign` inside-out with the DR, `--minisign` for updater signatures. Writes an audit record per file. Refuses an expired leaf; warns at 60 days, fails at 14 | user | the TSA list only |
| `verify <file> [--policy <toml>] [--json]` | A readable report: signer, chain to which root, trusted here or not, timestamp, digest, DR, nested files, minisign. With `--policy`, exits non-zero on any breach (the release gate) | user | none (`WTD_CACHE_ONLY_URL_RETRIEVAL`, as ADR 0051 U5) |
| `doctor` / `status` | Every certificate (inventory and stores) with expiry; chains that do not match the inventory; a leaf near expiry; root installed here or not; Smart App Control state; TrustedPublisher drift | user | none |
| `audit show` | Reads the audit log (JSONL) | user | none |
| `keys minisign generate\|rotate\|pubkey` | The updater keys (Tauri's minisign format), one per purpose (Dyno ADR 0022 P1's Dyno-only key) | user | none |
| `ci prepare` / `ci cleanup` | Loads the leaf from environment secrets into a temp file or ephemeral keychain under the job's `TMPDIR`, owner-only; cleanup deletes them. Prints no secret | user | none |

Least privilege:

- Only `trust install`, `remove` and `sync` change system state, and each shows the confirmation text first.
- Nothing runs as a service or at login.
- Network is used only by `sign` (the TSAs) and `trust sync` (the CRL), and every request is logged.
- Logs and audit records never contain keys, passphrases or file contents (ER-27, R§5.2).

### 2.4 CI flow (one signing path)

**Windows bundle job:**

1. `cargo build` produces `paddock-setup.exe` and `dyno-helper.exe`; `scrutineer sign` signs them.
2. `tauri build` runs with `bundle.windows.signCommand = "scrutineer sign %1"`, so Tauri's own signing goes through
   Scrutineer (R§2.4).
3. Tauri writes the updater `.sig` over the signed installer (S0 confirms the order).

**macOS bundle job:**

1. `tauri build --bundles app`.
2. `scrutineer sign --deep` re-signs inside-out with the explicit DR per identifier. Dyno's dotted identifiers are kept
   (Dyno ADR 0026).
3. The DMG, `.app.tar.gz` and minisign `.sig` are made from the signed `.app`: through `tauri bundle` if S0 shows it
   reuses the `.app`, else `hdiutil` plus `scrutineer sign --minisign`.

**Linux:** unchanged; minisign only (R§4).

**Which builds sign with the real leaf (D6):**

- tags, `main` nightly installers and manual installer runs;
- **branch and PR builds sign with a throwaway CA made in the job** (`ca init --ephemeral`), so the signing and gate
  code runs on every build without the secret.

**Gate (ADR 0039, rule 54):**

- `scrutineer verify --policy release.toml` on every published artifact. It checks:
  - the chain to the pinned root fingerprint;
  - the timestamp;
  - the leaf's validity at signing time;
  - the DR form and identifier on every Mach-O;
  - the minisign signature over the signed bytes;
  - that no file in the bundle is unsigned.
- It replaces `macos-verify`'s "ad-hoc seal" check, which is rewritten with rule 23. The sidecar self-test stays.

**Audit:**

- Every signature appends `{time, CI run id, workflow, ref, commit, file, sha256 before and after, leaf serial and
  fingerprint, TSA, tool version}`. No secrets.
- The log is published as `signing-log.jsonl` on the release, and `doctor` can check an installed file against it.

## 3. Feature catalogue

Personal-use focused: one owner, a handful of machines and VMs, one CI. Priority: **Must** = needed for the first
signed release; **Should** = within the same line, soon after; **Later** = when wanted. "Asked" marks the user's list of
2026-11-04; the inspiration column cites R§8.

| # | Feature | Priority | Why | Inspired by | Size (d) |
| --- | --- | --- | --- | --- | --- |
| 1 | `ca init` / `issue` / `export --public`, fixed profiles, code-signing EKU only, never-to-CI warning | Must | The certificates themselves; profiles make wrong certificates unrepresentable | step profiles, Vault roles, mkcert's warning | in A1, D1 |
| 2 | Encrypted key files; intermediate passphrase in the OS keychain | Must | Keys at rest | step (`--insecure` for unencrypted only) | in A1 |
| 3 | `fingerprint` and fingerprint confirmation before any install | Must (asked) | The only real check for a self-made root (R§7) | step, mkcert | in A1, D1 |
| 4 | `trust install` / `remove` / `status`, constrained to code signing, idempotent, `--dry-run` | Must | One step per machine and VM | mkcert `-install`/`-uninstall` | C1, D1 |
| 5 | `sign`: Authenticode with timestamp fallback list, `codesign` inside-out with explicit DR, minisign | Must (asked) | One signing path for CI and the owner (rule 55) | jsign TSA list, SignPath nested signing | B1 to B3 |
| 6 | Stable macOS designated requirements with a standby root | Must (asked) | The TCC pain; the main win on macOS | R§3.1 | in B3, A2 |
| 7 | `verify` for any file with a readable report, `--json` | Must (asked) | Debugging trust on a VM; the gate's engine | jsign 8 verify, rcodesign `print-signature-info` | in B1 to B3 |
| 8 | Release check: CI refuses unsigned or wrongly signed artifacts | Must (asked) | Rule 54 and ADR 0039 | — | E2 |
| 9 | Signing audit log (who, what, when, hash; no secrets) | Must (asked) | "Did I sign this?"; the provenance a private CA can offer | Vault audit, SignPath origin verification | in B1, E2 |
| 10 | Expiry warnings in `sign`, `status` and CI (60 days warn, 14 fail) | Must (asked: expiry reminder) | A forgotten leaf would stop a release | step `needs-renewal` | in A1, B1 |
| 11 | `doctor` / `status`: installed where, expiry, mismatched chains, Smart App Control | Must (asked) | One look per machine | step inspect, XCA | D1 |
| 12 | Key backup and escrow guide with `ca backup --paper` | Must (asked) | Losing the root means re-trusting every machine and losing TCC grants | XCA export, paper-key practice | G1, in D1 |
| 13 | Throwaway CA for branch and PR builds | Must | Exercises the path without the secret | — | in E1 |
| 14 | `ca renew` for the leaf (yearly) | Must | The first renewal comes within a year | step renew | in A2 |
| 15 | `ca rotate` intermediate and root, standby promotion | Should | Every 5 or 20 years, or on compromise | Vault multi-issuer | in A2 |
| 16 | Revocation: CRL on a Gitea release, `trust sync` into `Disallowed` / deny | Should (asked) | A leaked leaf; Windows rarely checks private CRLs, so the local push is the real remedy | XCA, Vault | in A1, C1 |
| 17 | `keys minisign`: updater key generation and rotation, one per purpose | Should (asked) | Dyno-only update key (Dyno ADR 0022 P1); the family key's rotation | — | 1–1.5 |
| 18 | Read-only trust check in the installer include and the About page | Should | Tells the person why UAC says "Unknown" (R§7) | — | F2 |
| 19 | Optional leaf in TrustedPublisher (PowerShell `AllSigned`) | Should | Only for scripts; not needed for UAC (S0) | certutil | in C1 |
| 20 | Portable Authenticode signing (sign a Windows exe on the Mac or Linux) | Should | "Any app the user builds", from any machine | jsign, psign, osslsigncode | B4 |
| 21 | `lint` of certificates against the profiles | Should | Catches a hand-made or imported certificate that would misbehave | step lint | 0.5 |
| 22 | YubiKey PIV / PKCS#11 for the root or intermediate | Later (asked, optional) | A cheap real gain for the root; files suffice at first | step `--kms`, jsign, XCA | H1 |
| 23 | GUI on the shared shell | Later (asked) | CLI first | XCA | H2 |
| 24 | rcodesign adapter (sign Mac bundles on Linux) | Later | Its releases stalled in 2024-11 | rcodesign | H3 |
| 25 | Signed apt repository through Gitea's Debian registry | Later | The only Linux signing that apt checks | Gitea | H4 |
| 26 | Root install from app installers (opt-in) | Later, not recommended (D11) | R§7 | — | H5 |
| 27 | Plugin key managed by `keys` | Later | One key tool | — | H6 |

Rejected:

- a CA server or renewal daemon (CI checks at sign time);
- a public transparency log;
- free-form certificate templates;
- osslsigncode (GPL-3.0);
- HashiCorp Vault (BUSL);
- installing into browser, NSS or Java stores (code signing does not use them, and they would widen trust).

## 4. Work packages

Sizes are in days for one implementer. Every crate has 99% line coverage (the CI gate) with its native glue in
`native.rs` excluded with reasons, and contract tests in a `testing` feature (rule 43). Tests never touch the real
stores or keychain: the native layer refuses unless `PADDOCK_TRUST_REAL=1` (the `paddock-secrets` pattern), and the
real-store checks run on the local VMs only (ER-14).

| WP | Scope | Paths | Depends | Size |
| --- | --- | --- | --- | --- |
| **S0** | Spike on the local VMs (R§10): UAC, ECDSA vs RSA, nested EKU and the store property, SmartScreen text, Smart App Control detection, MOTW and quarantine on updater downloads, Tauri 2.12 `signCommand` coverage and order, expiry with and without a timestamp; macOS DR and TCC across leaf, intermediate, standby root and expiry, trust-settings prompts, `tauri bundle` reuse, SMAppService across a leaf change, hardened runtime, Gatekeeper. Results in `docs/benchmarks.md` style blocks and the ADR | scratch, VMs; `docs/research/` | Dyno 1.1.0 tagged | 2.5–4 |
| **ADR** | Family ADR "Signing with the owner's CA" from this plan, the decisions and S0; independent review | `docs/adr/` | S0, decisions | 1–1.5 |
| **A1** | `paddock-pki` core: profiles, generation (`rcgen` on `ring`; RSA behind an `rsa` feature on `aws_lc_rs` if S0 needs it), fingerprints, encrypted PKCS#8, PKCS#12 (`p12-keystore`), inventory, CRL, expiry rules, lint | `crates/paddock-pki/` | ADR | 4–5.5 |
| **A2** | `paddock-pki` rotation: standby root, renew and rotate plans, the DR hash set exported for signers | `crates/paddock-pki/src/rotate.rs` | A1 | 1.5–2 |
| **B1** | `paddock-sign` core: ports, bounded format detection (fuzzed, R§8 osslsigncode lesson), nested walk, TSA client with fallbacks and retries (never unstamped), minisign signing, the report, the audit record, the policy check | `crates/paddock-sign/` | A1 | 3–4 |
| **B2** | Windows adapter: `SignerSignEx2`, `WinVerifyTrust` with pins (ADR 0051 U5 semantics), report | `crates/paddock-sign/src/windows/` | B1 | 3–4.5 |
| **B3** | macOS adapter: `codesign` with an ephemeral keychain (`--keychain`, never in the search list), inside-out signing with the DR, `codesign -d -r-` and `--verify` into the report | `crates/paddock-sign/src/macos/` | B1, A2 | 3–4 |
| **C1** | `paddock-trust-store`: Windows `Root` with the EKU property, `TrustedPublisher`, `Disallowed`; macOS `SecTrustSettings` with `codeSign`; Linux refusal; the read side; ownership markers so `remove` touches only ours | `crates/paddock-trust-store/` | A1 | 4–5.5 |
| **D1** | Scrutineer CLI: every command of 2.3, the confirmation flow, the Windows self-elevation of `trust` commands, `doctor`, `audit show`, `ca backup --paper` | `tools/scrutineer/` | A2, B2, B3, C1 | 3–4 |
| **D2** | Scrutineer's own release workflow (`scrutineer-v*`, three OS, self-signed, minisign), README | `.gitea/workflows/scrutineer-release.yml`, `tools/scrutineer/README.md` | D1 | 1–1.5 |
| **G1** | Owner guides: the CA ceremony (like `docs/plugin-signing-key.md`), backup and escrow, renewal calendar, "a key leaked" runbook | `docs/code-signing-ca.md` | D1 | 1.5–2 |
| **E1** | CI wiring: a shared `.gitea/actions/sign` (`ci prepare`/`cleanup`), `conformance/ci/tauri-bundle.sh` (Windows `signCommand`; macOS app, then sign, then DMG/tarball/`.sig`), `paddock-setup.exe` and `dyno-helper.exe` signing, the throwaway CA for branches, secrets `PADDOCK_CODESIGN_P12` and `PADDOCK_CODESIGN_P12_PASSWORD`, `strip-signing-keys` | `.gitea/actions/`, `conformance/ci/`, `.gitea/workflows/app-release.yml`, `dyno-release.yml` | D1, owner ceremony done | 3–4.5 |
| **E2** | Release gate: `scrutineer verify --policy` in the ADR 0039 gate; `macos-verify` rewritten (rule 23 amendment); `signing-log.jsonl` as a release asset | `.gitea/actions/macos-verify/`, `.gitea/actions/release-verdict/`, `conformance/release/` | E1 | 1.5–2 |
| **F1** | Per app: `tauri.windows.conf.json` `signCommand`, DR identifiers per binary, release-note "First launch" text (SmartScreen and Gatekeeper stay; UAC wording goes), rules 23 and 25 amended; each app's installers built once (rule 54) | `apps/*/app/src-tauri/`, `apps/*/docs/`, `docs/rules.md` | E2 | 2–3 |
| **F3** | `paddock-tools` U5 onto `paddock-sign`'s verifier (rule 55; `docs/known-copies.toml` entry if both exist for a while) | `crates/paddock-tools/` (ADR 0051, not on `main` yet) | B2, ADR 0051 WP | 0.5–1 |
| B4 | Portable Authenticode adapter (psign crates if published, else jsign as a tool) | `crates/paddock-sign/src/portable/` | B1 | 1.5–2.5 |
| F2 | Installer include and About: read-only trust status and the one-line hint | `crates/paddock-setup/` (ADR 0053, not on `main` yet), `crates/paddock-about/` (ADR 0028, not on `main` yet) | C1, ADR 0053 and 0028 crates | 1–1.5 |
| K1 | `keys minisign` (feature 17) | `tools/scrutineer/`, `crates/paddock-sign/` | B1 | 1–1.5 |
| H1–H6 | Later: YubiKey/PKCS#11 (2–3), GUI on the shell (5–8), rcodesign adapter (1.5–2), Gitea apt repo (1–2), installer opt-in (3–4), plugin key in `keys` (1) | | | 13.5–20 |

**Totals:**

- Must: S0, ADR, A1, A2, B1 to B3, C1, D1, D2, G1, E1, E2, F1 and F3, about **35–48 days**.
- Should: B4, F2, K1 and lint, **4–6.5 days**.

The ADR's work packages make these firm.

**Parallelism and cost (rule 51).**

- Signing is per file: hashing (I/O) plus one TSA round trip per Authenticode file.
- `sign` signs independent files in parallel (`--jobs`, Auto = 4: network-bound, a few per TSA host). The macOS walk
  parallelises within one nesting level only, because inner code must be signed before its container.
- Cheaper: Authenticode has no batch timestamp, so skip nothing that must be stamped. Skip re-signing a file whose
  signature already matches the leaf and policy (`--if-needed`), which helps reruns.
- S0 measures the time per file. A family installer has tens of files, so seconds are expected (unmeasured).

## 5. Order relative to the release queue

Queue (memory, 2026-11-04): Dyno 1.1.0, then Spotter 1.0, then Pitwall 1.1, then Pitcrew 1.1, then Debrief 1.2, then
Pacenotes and Sighter.

1. **Now:** this plan only. No builds, VMs or CI (Dyno's release first, ER-1).
2. **After the Dyno 1.1.0 tag:** S0 on the local VMs, then the ADR and its review.
3. **Alongside Spotter 1.0:** A1 to D2 and G1. These are new crates and a new tool that no release depends on, so they
   may progress in parallel while load allows (ER-1, ER-24). Then the owner runs the CA ceremony with the built tool
   and stores the CI secrets.
4. **After the Spotter 1.0 tag, before Pitwall 1.1's freeze:** E1, E2, F1 and F3. These touch the release path, so each
   app's installers are built before they land (rule 54).
5. **First signed release: Pitwall 1.1**, then Pitcrew 1.1, Debrief 1.2, Dyno's next release (1.1.x or 1.2), Spotter's
   next, the demos, then Pacenotes and Sighter from their first release.

If the user wants Spotter 1.0 signed, step 4 moves before its tag and Spotter waits about 8 to 12 working days (D13).

## 6. Which apps and builds change

| App or build | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Debrief, Dyno, Pitcrew, Pitwall, Spotter (own workflows or `app-release.yml`) | every exe, sidecar, plug-in DLL, uninstaller, installer; `paddock-setup.exe` once ADR 0053 lands; Dyno's `dyno-helper.exe` | every Mach-O re-signed with the DR; DMG and updater tarball from the signed app | unchanged |
| Shell demo, workbench demo (`*-release.yml`, Linux leg) | if they ship Windows builds, as above | as above | unchanged |
| Pacenotes, Sighter | from their first release | from their first release | unchanged |
| Nightly installers (`nightly.yml`) | real leaf (on `main`) | real leaf | unchanged |
| Branch and PR builds | throwaway CA | throwaway CA | unchanged |
| `paddock-dev` binaries (`./dev`) | not signed (developer tool, content-addressed, ADR 0022) | not signed | — |
| Scrutineer | self-signed with the leaf | self-signed | minisign only |

## 7. Risks

| Risk | Effect | Mitigation |
| --- | --- | --- |
| TCC does not honour a root-hash DR for a self-signed chain | The main macOS win is lost | S0 first. Fallback: a leaf-pinned DR with a 5-year leaf and grants lost once per renewal |
| ECDSA rejected by `SignerSignEx2`, UAC or `codesign` | Wrong key type baked into the root | S0 before `ca init`; RSA fallback (R§5.1) |
| The CI leaf leaks (secrets readable by every workflow, Dyno ADR 0022 P1) | Someone signs as the owner | Real leaf only on `main`, tags and manual runs; one-year life; audit log; revoke and `trust sync`; scoped secrets if Gitea supports them (to check) |
| TSA outage at release time | Release blocked | Fallback list and retries; a clear ER-27 error; re-run later. Never unstamped |
| Root key lost | Every machine re-trusts a new root; TCC grants lost once (unless the standby survives) | Paper and two offline copies; the standby root kept separately |
| Tauri `signCommand` misses a file (plug-in DLLs before the May 2026 fix) | An unsigned file in the installer | The release gate lists every PE inside the NSIS installer and fails on an unsigned one |
| Smart App Control on a clean Windows 11 install | The apps are blocked | `doctor` says so; the person turns it off. Only public trust fixes it (out of scope) |
| macOS trust prompts change in a later macOS | `trust install` fails | Not needed for TCC; `doctor` reports; the OS prompt is never bypassed |
| rcodesign stays unreleased | No Mac signing on Linux | `codesign` is the default; rcodesign is Later |

## 8. Name

**Scrutineer** (accepted 2026-11-04). Scrutineering is the technical inspection that certifies a car before it may run.
That matches what the tool does: it certifies the build (signs), inspects it (verify, doctor), and keeps the paperwork
(the CA, the audit log). It fits Debrief, Dyno, Pacenotes, Pitcrew, Pitwall, Sighter, Spotter and Recce: a real
paddock role, one word, no clash with an existing family name or a well-known signing tool that I found. The
`steward/` branch prefix is not a product name.

Alternatives considered:

- **Homologation:** certifying a design as eligible. Accurate for a CA, but long and hard to type as a command.
- **Parc fermé:** the sealed area after a race. Evokes sealing, but two words with an accent.
- **Marshal:** clear, but generic and widely used in software (serialization).

## 9. Decisions for the user (each with a recommended default)

| # | Question | Recommended default |
| --- | --- | --- |
| D1 | Chain: root, then intermediate, then leaf; or root then leaf | **Three tiers.** The root is touched once in five years; renewals use the Mac-held intermediate |
| D2 | Key types | **ECDSA P-384 for the CAs, P-256 for the leaf**, if S0 confirms Windows and macOS accept them; else RSA-4096/3072 |
| D3 | Lifetimes | **Root 20 years, intermediate 5, leaf 1 (397 days)**; warn at 60 days, fail at 14 |
| D4 | Root key storage | **Encrypted file on two offline media plus a paper backup**; YubiKey later (H1) |
| D5 | Timestamps | **Public TSAs** (DigiCert, then Sectigo, then one more), retries, release fails rather than shipping unstamped |
| D6 | Which builds use the real leaf | **Tags, `main` nightly, manual installer runs; branches and PRs use a throwaway CA** |
| D7 | Windows engine | **Native `SignerSignEx2`/`WinVerifyTrust`**; portable adapter as Should (B4) |
| D8 | macOS engine | **Apple `codesign` with an ephemeral keychain**; rcodesign Later |
| D9 | macOS hardened runtime | **Stays off** (rule 23 reworded "until notarized") |
| D10 | Where trust is installed | **Windows LocalMachine `Root` with the code-signing property; macOS admin domain, `codeSign` only; Linux never.** TrustedPublisher only on request |
| D11 | App installers install the root too | **No.** Scrutineer only; installers and About show a read-only status (R§7). Fallback: Windows-only unticked opt-in (H5) |
| D12 | Revocation | **CRL file on a Gitea release plus `trust sync`; no CRL distribution point in leaves** |
| D13 | Timing | **First signed release Pitwall 1.1**; Spotter 1.0 does not wait |
| D14 | ADR 0051 U5 | **`paddock-sign` owns the Authenticode verifier; `paddock-tools` calls it** |
| D15 | Publisher name in certificates (shown by UAC) | **"decryp7"** in CN and O; no e-mail or real name (privacy) |
| D16 | Separate minisign key for Dyno's helper updates (Dyno ADR 0022 P1) | **Yes, through `keys minisign`**, when the helper-run updates come; the family key unchanged until then |
| D17 | Scrutineer's home | **`tools/scrutineer/` in the monorepo**, generic, own workspace and `scrutineer-v*` releases |
| D18 | Name constraints on the intermediate | **Off**; EKU constraints do the work (R§2.1) |
