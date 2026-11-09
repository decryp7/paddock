# Spike: a stable self-signed code-signing identity on macOS (2026-11-09)

- Copied into the repo from the spike notes (`/Volumes/d7sst7/dev-targets/scratch/sign-spike/notes.md`, 2026-11-09) so ADR 0079 can cite
  it. The throwaway certificate and the test bundle were deleted after the run; no keychain, trust setting or TCC row of
  the host was left changed (`check.sh cleanup`).


Question: if every build is signed with the same self-signed certificate instead of ad-hoc, do TCC grants and keychain
item access survive a rebuild or update? This is about identity stability only. Gatekeeper is not affected:
`spctl -a -vv` still prints `rejected, origin=Sign Spike Code Signing (throwaway)` on the host.

## Verdict

**Yes, in every case measured.** The designated requirement (DR) becomes
`identifier "<id>" and certificate root = H"<sha1 of the cert>"`. It does not depend on the cdhash, so every rebuild
signed with the same key satisfies it. Keychain ACLs and TCC rows (both store the DR) keep matching after the code is
replaced. **No trust setting is needed.** The certificate stays `CSSMERR_TP_NOT_TRUSTED`, and
`find-identity -v` lists 0 valid identities.

## Setup

- Probe: a throwaway Rust binary (no crates, FFI to Security, ApplicationServices and IOKit). It has modes `kc-add`,
  `kc-read`, `probe <paths>` (open plus one byte read, then prints only the size) and `tcc`
  (`AXIsProcessTrusted()`, `IOHIDCheckAccess(ListenEvent)`; neither prompts). v1 and v2 differ in code
  (`SPIKE_VARIANT`), so their cdhashes differ.
- Bundle: `SignSpike.app` with `CFBundleIdentifier` `net.decryptology.sign-spike`, started with `open` so that it is
  its own responsible process for TCC.
- Certificate: OpenSSL 4.1 on the host, RSA 2048, self-signed, `basicConstraints=critical,CA:false`,
  `keyUsage=critical,digitalSignature`, `extendedKeyUsage=critical,codeSigning`. Exported as a PKCS#12 file with
  legacy algorithms (`-legacy -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1`). SHA-1
  `617E8FF2DC3C95F5618DD3A17B9099B0C7E89541`.
- VM: a throwaway clone of the local Tart image `vmgui-base-tahoe-4aaa751f`, taken under the host VM slot lock
  (`./dev ci slot acquire/release`, root `$TART_HOME/.paddock-vm`) and deleted afterwards. **Guest is macOS 26.6.2
  (25G83), SIP disabled.** The host is macOS 27.2 (26B123); the host checks below did not touch the login keychain,
  the search list (diffed before and after) or TCC.

## Results

### (1) Designated requirement (`codesign -d -r-`)

| Build | DR |
|---|---|
| self-signed v1 and v2 (guest, and re-read on the host, 27.2) | `identifier "net.decryptology.sign-spike" and certificate root = H"617e8ff2…5541"` (identical) |
| ad-hoc v1 | `cdhash H"ef47c396…"` |
| ad-hoc v2 | `cdhash H"cbc7370f…"` |
| a second self-signed cert, same id | `identifier "net.decryptology.sign-spike" and certificate leaf = H"d6723fa8…"` |

- `codesign -v -R="<v1 DR>" v2` returns 0 for the self-signed builds, on both the guest and the **host (27.2)**. The
  ad-hoc pair returns 3 ("failed to satisfy").
- The other certificate fails the v1 DR (rc 3), so an identical bundle id signed with a different key cannot take
  over the grants.
- The DR uses `root` or `leaf` depending on the certificate. Both pin the one self-signed certificate. To make it
  explicit and deterministic, pass `-r='designated => identifier "<id>" and certificate leaf = H"<sha1>"'`.

### (2) Keychain (legacy file keychain, `SecKeychainAddGenericPassword`, UI disabled with `SecKeychainSetUserInteractionAllowed(false)`, so a would-be prompt shows as an error)

| Run | Guest 26.6 | Host 27.2 |
|---|---|---|
| self-signed v1 adds the item, then v1 reads it | 0 | 0 |
| v2 replaced at the same path reads it | **0** (value from v1) | **0** |
| v2 at a different path reads it | **0** | **0** |
| ad-hoc v1 adds, then v1 reads | 0 | 0 |
| ad-hoc v2 (same path) reads | **-25293** (errSecAuthFailed) | **-25293** |

- The item's ACL records the app and, for self-signed code, `requirement: identifier "…" and certificate root = H"617e…"`.
  For ad-hoc code it records `requirement: cdhash H"…"`. `security dump-keychain -a` showed no partition-ID entry for
  these generic-password items.
- **Trap:** a binary copied *out* of a signed bundle is invalid (`invalid Info.plist`, because the plist is bound
  into the signature) and gets -25293 even for its own item. Sign the bundle and run the binary in place. Sign a
  standalone tool as a standalone Mach-O.
- Not covered: the data-protection keychain. It needs a `keychain-access-groups` entitlement, which needs an Apple
  provisioning profile, so it is not available to self-signed code.

### (3) TCC

- **FDA and Removable Volumes: not measured in the VM (ER-33, the probe could not see them).** With SIP disabled,
  file protections are not enforced: v1 with no grant read `~/Library/Safari/CloudHistoryRemoteConfiguration.plist`
  and the user TCC.db. The VM has no `Bookmarks.plist`.
- Measured instead: the same csreq mechanism with services that tccd enforces even with SIP off, Accessibility
  (`AXIsProcessTrusted`) and Input Monitoring (`IOHIDCheckAccess`). The grant was simulated by writing the row that
  the System Settings switch writes: client = bundle id, client_type 0, auth_value 2, csreq = the compiled DR of the
  app (`csreq -r dr.txt -b`). tccd was restarted afterwards.

| Step | self-signed | ad-hoc |
|---|---|---|
| v1, no row (control) | AX 0, Listen 2 (unknown) | AX 0, Listen 2 |
| v1 after grant | AX 1, Listen 0 (granted) | AX 1, Listen 0 |
| **v2 replaced at the same path, no new grant** | **AX 1, Listen 0** | **AX 0, Listen 1 (denied)** |
| v2 signed by a different self-signed cert | AX 0, Listen 1 | n/a |

- TCC.db csreq stored and read back: `identifier "net.decryptology.sign-spike" and certificate root = H"617e8ff2dc3c95f5618dd3a17b9099b0c7e89541"`.
- **Still open:** a real FDA and Removable Volumes click on SIP-enabled 27.2. This is the user step below. Expected
  result: same behaviour, because TN3127 describes one DR mechanism for all privacy grants (secondary claim). It is
  unverified for FDA on 27.2.

### (4) Trust

- No trust setting anywhere in the guest (`dump-trust-settings` and `-d`: none). The identity showed
  `CSSMERR_TP_NOT_TRUSTED`, and signing, keychain and TCC matching all worked.
- What *is* needed: **the signing keychain must be on the user search list.**
  `codesign --keychain <file> -s <name or SHA-1>` gave "no identity found" when the keychain was not on the list. It
  worked once the keychain was added with `security list-keychains -d user -s …`.

## Recipe outline

1. **Certificate, once.** Generate it with OpenSSL as above, long validity (for example 20 years; behaviour after
   expiry is unverified). Export a legacy-PBE p12. Rotating the key changes the DR, so every grant is lost once.
2. **CI secrets.** `MACOS_SIGNING_P12` (base64), `MACOS_SIGNING_P12_PASSWORD`. The SHA-1 is not secret and can live in
   the repo.
3. **CI signing step.**
   - `security create-keychain -p $R $KC`
   - `security set-keychain-settings $KC`
   - `security unlock-keychain -p $R $KC`
   - `security import p12 -k $KC -P $PW -T /usr/bin/codesign`
   - `security set-key-partition-list -S apple-tool:,apple: -s -k $R $KC`
   - add `$KC` to the user search list (save the old list, restore it in an `always()` cleanup, then
     `delete-keychain`)
4. **Sign.** `codesign -f -s <SHA-1> -i <stable id> -o runtime --timestamp=none <path>`. For a bundle, the identifier
   comes from `CFBundleIdentifier`. **Always pass `-i` for a bare binary**: the Rust linker's ad-hoc identifier is
   `sign_spike-e98c96c375ef330b`, which is hash-suffixed.
5. **gitea-runner (act_runner) binaries:**
   `codesign -f -s <SHA-1> -i net.decryptology.gitea-runner --timestamp=none <binary>` (the hardened runtime is
   optional). Start it from launchd so that it is its own responsible process, then grant FDA to that path once.
6. **Tauri.** Set `APPLE_SIGNING_IDENTITY=<SHA-1>` after step 3. **Do not rely on `APPLE_CERTIFICATE` alone**:
   tauri-macos-sign looks up identities only by Apple common-name prefixes ("Developer ID Application:", "Apple
   Development:" and so on, `keychain/identity.rs`), so it would not find a self-signed certificate. That is read in
   the source, not measured.
   - Paddock's `tauri-bundle` action already passes `APPLE_SIGNING_IDENTITY`. A real identity turns the hardened
     runtime on.
   - Untested risk: library validation under the hardened runtime for code with no Team ID (bundled dylibs and
     sidecars). Try one Paddock app before rollout.

## User step (one-time, host 27.2, SIP on)

Files are in `/Volumes/d7sst7/dev-targets/scratch/sign-spike/host-check/`.

1. Run `check.sh run-v1`. Expect a Removable Volumes prompt for `rv-target.txt`: click Allow. `Bookmarks.plist`
   should fail with "Operation not permitted".
2. In System Settings, open Privacy & Security, then Full Disk Access, and add
   `/Volumes/d7sst7/dev-targets/scratch/sign-spike/host-check/SignSpike.app`.
3. Run `check.sh run-v1` and check that both files now report a size.
4. Run `check.sh update-v2`. The pass criterion is that both report a size and there is no prompt.
5. Run `check.sh csreq` (Terminal needs FDA to read the system TCC.db).
6. Run `check.sh cleanup` (tccutil reset for both services, then remove the folder).

## Sources

- Apple, TN3127 "Inside Code Signing: Requirements":
  https://developer.apple.com/documentation/technotes/tn3127-inside-code-signing-requirements. Read via its JSON
  endpoint. It says macOS records the app's DR in the database of authorised apps and checks each new version against
  it; ad-hoc code "has a DR but it's tied to that specific version of the code"; and the keychain shows authorisation
  alerts when the DR changes.
- Apple DTS (Eskimo), forum thread 730043: TCC validation depends on the DR, see TN3127.
  https://developer.apple.com/forums/thread/730043 (search snippet, not read in full).
- Apple forum thread 712043 "Fixing an untrusted code signing certificate": https://developer.apple.com/forums/thread/712043
  (search result only, not read).
- Third-party blog, "tcc cdhash trap": https://nickboy-github-io.pages.dev/posts/tcc-cdhash-trap/ (anecdotal: a
  self-signed certificate avoids cdhash DRs; not read in full).
- Tauri v2 macOS signing: https://v2.tauri.app/distribute/sign/macos/ (`APPLE_CERTIFICATE`,
  `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` or `bundle.macOS.signingIdentity`; no mention of self-signed
  certificates).
- tauri-macos-sign source: https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-macos-sign/src/keychain.rs and
  `keychain/identity.rs`.

## Host check on macOS 27.2, SIP on (2026-11-09, user step, recorded by the coordinator)

- Launch gotcha found first: `open --stdout <file on /Volumes/d7sst7>` failed with LaunchServices -10810 for the not-yet-granted app; output moved to the internal TMPDIR, after which `open` worked.
- The user granted Full Disk Access to SignSpike.app v1. System TCC.db row: `kTCCServiceSystemPolicyAllFiles|2|4|2026-11-09 15:33:00` for client `net.decryptology.sign-spike`.
- The user then ran `check.sh update-v2` (v2 = different code, same self-signed identity, same path). Output, verbatim:
  - `variant=v2 pid=41153`
  - `probe /Users/decryp7/Library/Safari/Bookmarks.plist: OK size=1229464`
  - `probe /Volumes/d7sst7/dev-targets/scratch/sign-spike/host-check/rv-target.txt: OK size=46`
  - `designated => identifier "net.decryptology.sign-spike" and certificate root = H"617e8ff2dc3c95f5618dd3a17b9099b0c7e89541"`
- No new prompt was reported. **Result: the FDA grant made to v1 held for the rebuilt v2 (measured).** Removable Volumes was covered by FDA here, not measured on its own.
- The user ran `check.sh cleanup` (tccutil reset of both services for the test bundle id, host-check folder removed).
