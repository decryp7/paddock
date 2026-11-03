# smartmontools 7.5: pinned packages

Pinned for the helper's `InstallTool` (Dyno ADR 0027 section 7). The table the helper checks is compiled into
`dyno-platform/src/tool_install.rs` (`PINS`); this record is its evidence. smartmontools is GPL-2.0-or-later: Dyno never links,
bundles or mirrors it; the user downloads the upstream release on a click and Dyno runs `smartctl` as a separate program.

Source: GitHub release `RELEASE_7_5` (published 2025-05-12), downloaded 2026-11-03 inside local VMs (WP-0, ADR 0027 "WP-0 results"
point 3); the Windows installer in two VMs, hashes equal.

| File | Bytes | SHA-256 | Upstream `.md5` | In the helper's table |
| --- | --- | --- | --- | --- |
| `smartmontools-7.5.win32-setup.exe` | 1,508,408 | `896337fcc253220614cf8cdbd5cf2321c5aa326a37a04160a672a281e6104c70` | `bb1e199ad6a3db3e1c27ae54b835cbd5` (matches) | yes (Windows) |
| `smartmontools-7.5.dmg` | 1,619,514 | `4aa176c161612488f3db053fdf3eee47bbc5178c8fe46afb2002d8dbdf563cc5` | `414d905b7a6c6ea8861d4e6f7fa14c1c` | no: macOS SMART runs in the agent from its own `paddock-runtime` store (no root needed) |
| `smartmontools-7.5.tar.gz` | 1,122,317 | `690b83ca331378da9ea0d9d61008c4b22dde391387b9bbad7f29387f2595f76e` | | no: there is no upstream Linux binary |

## Signatures

`gpg --verify` (GnuPG 2.5.24, macOS VM): `Good signature from "Smartmontools Signing Key (through 2025)
<smartmontools-support@listi.jpberlin.de>" [expired]` for all three artefacts, signatures made 2025-04-30 08:29 UTC.

- Key: RSA 4096, fingerprint `0C95 77FD 2C4C FCB4 B9A5 9964 0A30 812E FF3A EFF5`, created 2020-12-21, expired 2025-12-31 (after
  the signatures were made).
- Taken from `keyserver.ubuntu.com` (keys.openpgp.org has no copy). Its short id `0xFF3AEFF5` matches `KEYID` in upstream's
  `do_release` at the tag.
- Fingerprint confirmed by the user against smartmontools.org, 2026-11-03 (key ID `FF3AEFF5`, matching the keyserver copy above).
- The Windows installer also carries a valid Authenticode signature (`CN=SignPath Foundation`).
- The next smartmontools release will be signed with a new key: the pinning step must expect a key change and redo this record.

## Installer facts the planner relies on

`os_win32/installer.nsi` at the tag: `/SO` takes a comma list of components, any not named is off, `/D=` must come last. Measured as
LocalSystem with only `SeChangeNotifyPrivilege`: `/S /SO x64,smartctl,drivedb /D=<dir>` exits 0 and writes exactly `bin\smartctl.exe`,
`bin\drivedb.h` and `bin\update-smart-drivedb.ps1`; no registry key, no `smartd` service, no Start-menu folder, no PATH change.
