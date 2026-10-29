# Windows 11 Arm test VM (local only)

A dedicated, throw-away Windows 11 on Arm VM in VMware Fusion on the Mac mini, for the tests CI cannot do: install,
update, services, login items. **Local only, never CI** (decision 2026-10-28). It is not the user's own Fusion VM: that
one is personal and encrypted and no script here ever starts, stops or changes it (`scripts/vm-win.sh` acts only on the
VM under `PADDOCK_WIN_VM_DIR`).

Background and the hypervisor choice: the research note
`/Volumes/d7sst7/dev-targets/research/2026-10-local-linux-windows-vms.md` (section 4).

| | |
| --- | --- |
| Hypervisor | VMware Fusion 26.0.1 (free), `vmrun` headless; everything inside the guest over OpenSSH (Fusion has no guest commands for Arm guests) |
| Guest | Windows 11 Pro on Arm, **not activated** (user decision 2026-10-29: no licence key; Setup uses Microsoft's generic Pro key, which only picks the edition) |
| Language | English (United Kingdom): `en-GB` UI and locales, UK keyboard (`0809:00000809`), matching the ISO (`Windows11_Client_arm64_en-gb_26300_9457`, build 26300, SHA-256 `500bcb6f…7dee61`) |
| Size | 4 vCPU, 8 GB RAM, 80 GB growable NVMe disk, NAT with a fixed MAC (`00:50:56:2A:D0:C7`) |
| Accounts | `paddock-admin` (setup and admin tasks), `paddock-test` (standard user; tests run as this user) |
| Access | SSH, key-only, from this Mac only (firewall rule: local subnet); keys in `<vm dir>/keys` |
| Like the CI runner | Git for Windows with `core.autocrlf=true` (CRLF checkouts), plain NTFS, Defender real-time protection on, no Dev Drive |
| Test folder | `C:\paddock-test` (owned by `paddock-test`) |
| Power | never sleeps or hibernates |

## Files

Everything lives on d7, never under `~` (ER-14). Default `PADDOCK_WIN_VM_DIR=/Volumes/d7sst7/vms/paddock-win11-test`:

```
keys/       paddock-test, paddock-admin (SSH key pairs), setup-passwords, known_hosts   (all private, mode 600/700)
payload/    Git for Windows and Win32-OpenSSH installers, pinned in scripts/vm-win/payload.lock and hash-checked
iso/        windows11-arm64.iso (the user downloads it), paddock-setup.iso (built by prepare.sh, holds the answer file)
vm/         paddock-win11-test.vmwarevm
```

In the repository: `scripts/vm-win.sh` (daily use), `scripts/vm-win/prepare.sh` (builds the files above),
`scripts/vm-win/autounattend.xml.in` (the answer file template; no secrets, no licence key),
`scripts/vm-win/firstlogon.ps1` (runs once in the guest), `scripts/vm-win.test.mjs` (`node --test scripts/vm-win.test.mjs`).

## Disk and memory

| | Size |
| --- | --- |
| RAM while running | 8 GB resident, plus Fusion overhead; given back on stop. Run it only when a test needs it, and never two Windows VMs at once |
| VM disk | grows up to 80 GB; a fresh Windows 11 install usually takes about 25 to 35 GB [unverified estimate] |
| Snapshots | `clean-baseline` costs almost nothing when taken; each test run's changes grow a redo log until the next revert |
| Windows ISO | about 5 to 7 GB [unverified estimate]; may be deleted after `baseline` |
| Setup disc + payload | about 170 MB + 70 MB |

Budget about 50 GB on d7 for a working VM with a baseline and a few test runs.

## TPM: the decision

Windows 11 needs a TPM 2.0. Fusion's virtual TPM cannot exist without encryption: the smallest choice is **"Only the
files needed to support a TPM are encrypted"** (Fusion's *Fast* encryption: `.nvram`, snapshot and suspend files and
part of the `.vmx`; the virtual disk stays unencrypted) [Fusion 13 docs, community reports; see sources]. Fusion's
`managedVM.autoAddVTPM = "software"` setting is experimental, Broadcom recommends against it, and users report it still
encrypts the VM with an unknown password, so it is not used here.

| | A. Virtual TPM, TPM files encrypted (recommended) | B. No TPM (`prepare.sh --no-tpm`) |
| --- | --- | --- |
| Encryption | only the TPM files; the disk is not encrypted | none |
| Headless start | yes: `vmrun -vp <password>`; `vm-win.sh` reads the password from the login keychain | yes, no password |
| Matches users' PCs | yes (TPM 2.0, Secure Boot) | no: Setup skips the TPM and Secure Boot checks (LabConfig) |
| Windows Update | supported configuration | Microsoft warns such devices may not be entitled to updates |
| Set-up effort | one Fusion window: encrypt TPM files, add the TPM (2 minutes) | none |
| Risk | the password passes through `vmrun`'s command line (visible to `ps` for the life of that call); losing it means rebuilding the VM; `vmcli` may not edit an encrypted `.vmx` (then detach the discs in Fusion's settings) | a less realistic guest; future Windows feature updates may refuse to install |

Recommendation: **A**. The disk stays unencrypted (no I/O cost), the guest is a supported Windows 11, and headless runs
work through the keychain. Choose B only if the keychain step is unwanted.

## One-time setup (the user)

1. **Get the Windows 11 Arm64 ISO from Microsoft** (about 5 to 7 GB; needs your OK to download). Use one of the official
   routes, no mirrors:
   - Microsoft's page <https://www.microsoft.com/software-download/windows11arm64>: choose *Windows 11 (multi-edition ISO
     for Arm64)*, the language *English International* (the en-GB image this VM uses), and download through the browser. Save or move the file to
     `/Volumes/d7sst7/vms/paddock-win11-test/iso/windows11-arm64.iso`.
   - or Fusion's *File > New > Get Windows from Microsoft*, which downloads Microsoft's ESD and converts it to an ISO.
     Cancel the wizard after the ISO is saved and move the ISO to the path above. (Older Fusion releases built broken
     24H2 images with it; 26.0.1 is current, but prefer Microsoft's page.)
2. **Prepare** (already done once on 2026-10-29; re-run after changing the scripts or to switch to option B):
   `scripts/vm-win/prepare.sh` (or `--no-tpm`). It never downloads Windows. The answer file's language must be one the
   ISO contains, or Setup stops at the language step: the default is `--lang en-GB` (keyboard `0809:00000809`); for
   another ISO pass `--lang <tag>` and, except for en-US, `--input-locale <language id>:<layout id>`.
3. **Option A only, add the TPM** (the VM must be off):
   1. Create the password and keep it in the login keychain:
      `security add-generic-password -s paddock-win11-test-vm -a vmrun -w "$(openssl rand -base64 24)"`,
      then show it once to paste: `security find-generic-password -s paddock-win11-test-vm -a vmrun -w`.
   2. In Fusion: *File > Open* `/Volumes/d7sst7/vms/paddock-win11-test/vm/paddock-win11-test.vmwarevm`, then
      *Settings > Encryption & Restrictions*: turn on encryption, choose **Only the files needed to support a TPM are
      encrypted**, paste the password, and leave *Remember password in Mac Keychain* off (the script reads its own item).
   3. *Settings > Add Device > Trusted Platform Module > Add*. Close the settings and quit the VM window (do not start it).
4. **Install**: `scripts/vm-win.sh install`. It opens the VM in a Fusion window once; click into it and press a key at
   *Press any key to boot from CD or DVD*. Setup then runs unattended: partitions the disk, installs Windows 11 Pro,
   creates both accounts, and at the first logon `firstlogon.ps1` installs OpenSSH Server and Git, sets power, the test
   folder and SSH, checks Defender, deletes the answer file copies, and powers off. The script waits ("still working")
   until the VM is off. If the VM stays on, read `C:\paddock-setup\setup.log` in the window.
5. **Baseline**: `scripts/vm-win.sh baseline` detaches both discs, boots headless, checks `C:\paddock-setup\done.marker`
   over SSH, shuts down and takes the `clean-baseline` snapshot. The Windows ISO may be deleted afterwards.

## Daily use

```sh
scripts/vm-win.sh revert                  # back to clean-baseline (the VM is off afterwards)
scripts/vm-win.sh start                   # headless; waits until SSH answers (refuses when the 1-minute load is >= 25)
scripts/vm-win.sh ssh 'git --version'     # as paddock-test, PowerShell is the remote shell
scripts/vm-win.sh ssh --admin 'Get-Service sshd'
scripts/vm-win.sh snapshot before-update  # optional extra snapshot
scripts/vm-win.sh stop                    # always stop when done: the VM holds 8 GB
```

Copy files with `scp -i <vm dir>/keys/paddock-test -o UserKnownHostsFile=<vm dir>/keys/known_hosts <file>
paddock-test@$(scripts/vm-win.sh ip):C:/paddock-test/`. Always revert afterwards; never keep test state in the baseline.

Monthly (or before a release test): revert, start, run Windows Update as `paddock-admin`, stop, then replace the
baseline (`vmrun deleteSnapshot`, `scripts/vm-win.sh snapshot clean-baseline`).

## The pre-release update test on Windows

The same procedure as the macOS run in `apps/<app>/docs/releasing.md` (Spotter's "Pre-release update test"): host-built
test builds signed with a key made for the run (never the family key), a local HTTPS feed, a test CA that only the test
user trusts, and the VM reverted afterwards.

1. **On the host**, in a scratch worktree (uncommitted): build `<app>-test.1` and `<app>-test.2` for
   `aarch64-pc-windows-msvc` (or x64 when the test is about Prism) with the run's updater key, the feed URL pointing at
   the host's NAT address, and `latest.json` for test.2. The host's address on Fusion's NAT network is `.1` of
   `VNET_8_HOSTONLY_SUBNET` in `/Library/Preferences/VMware Fusion/networking` (172.16.119.1 on 2026-10-29).
2. **Feed**: a small HTTPS server on the host bound to that address only (never `0.0.0.0`), serving `latest.json` and
   the bundles, with a leaf certificate for that IP issued by a throw-away test CA made for the run. Stop it after the run.
3. **VM**: `vm-win.sh revert && vm-win.sh start`, then copy the CA certificate and test.1's installer to
   `C:\paddock-test\`.
4. **Trust the CA for `paddock-test` only** (the CurrentUser root store, not the machine store). `Import-Certificate
   -CertStoreLocation Cert:\CurrentUser\Root` asks for confirmation in a dialog, which an SSH session cannot answer;
   write the certificate into the user's registry-backed store instead [unverified: check it on the first run]:

   ```powershell
   $c = [Security.Cryptography.X509Certificates.X509Certificate2]::new('C:\paddock-test\test-ca.cer')
   $store = [Security.Cryptography.X509Certificates.X509Store]::new('Root', 'CurrentUser')
   # If Open/Add raises the confirmation dialog over SSH, fall back to the registry blob under
   # HKCU:\Software\Microsoft\SystemCertificates\Root\Certificates\<thumbprint>.
   $store.Open('ReadWrite'); $store.Add($c); $store.Close()
   ```

   Check the trust from the guest: `curl.exe --ssl-no-revoke https://<host>:<port>/latest.json` must succeed without
   `-k` (Schannel, the store the app's verifier uses: Spotter verifies TLS with `rustls-platform-verifier`, which uses
   the Windows certificate chain engine and so the user's store). A test CA has no revocation endpoint; if the app
   refuses the feed for that reason, note it in the results rather than weakening the app.
5. **Run the steps** as `paddock-test`: install test.1 the way users do, start it, check for updates,
   download, verify, install, relaunch, confirm test.2; then the wrong-key build must be refused. Install-mode and
   service tests that need elevation run their elevated step as `paddock-admin`.
6. **After**: copy logs back with `scp`, `vm-win.sh stop`, `vm-win.sh revert`. Record the result table in the app's
   releasing doc with the commit, builds, OS build and evidence path (never real user data).

GUI steps (a window must open) need the console session: SSH sessions have no desktop. Until a logon helper exists,
open the VM in Fusion's window, log on as `paddock-test` there, and drive the GUI by hand; everything else stays over
SSH.

## Security

- NAT only, no shared folders, no clipboard or drag and drop (`isolation.tools.*` in the `.vmx`), no bridged network.
- SSH accepts keys only, only for the two Paddock accounts, only from the local subnet. The private keys and setup
  passwords never leave `<vm dir>/keys`; the filled answer file exists only on `paddock-setup.iso` (mode 600) and is
  deleted from the guest by `firstlogon.ps1`.
- The guest runs agent-built code: treat it as untrusted and revert after every run.

## Sources

Checked 2026-10-29.

- Windows 11 Arm64 ISO: <https://www.microsoft.com/software-download/windows11arm64>
- Generic KMS client (GVLK) keys, Windows 11 Pro `W269N-...`, "won't activate" without a KMS host:
  <https://learn.microsoft.com/en-us/windows-server/get-started/kms-client-activation-keys>
- Windows 11 on devices below the requirements, update entitlement:
  <https://support.microsoft.com/en-us/windows/installing-windows-11-on-devices-that-don-t-meet-minimum-system-requirements-0b2dc4a2-5933-4ad4-9c09-ef0a331518f1>
- Broadcom KB 315650, Windows 11 guests (vTPM needs encryption; LabConfig bypass): <https://knowledge.broadcom.com/external/article/315650>
- Fusion 13 Fast vs Full encryption, encrypted files: <https://docs.vmware.com/en/VMware-Fusion/13/com.vmware.fusion.using.doc/GUID-AE911FA6-0BA2-4765-B1F5-628C27A6477A.html>
  (secondary: VMware community thread "TPM without encrypting VMDKs",
  <https://communities.vmware.com/t5/VMware-Workstation-Pro/TPM-without-encrypting-VDKs/m-p/2870629>)
- `autoAddVTPM = "software"` side effects (community, unverified):
  <https://communities.vmware.com/t5/VMware-Workstation-Pro/What-is-the-default-encryption-password-for-virtual-TPM/m-p/2919355>
- Fusion "Get Windows from Microsoft" and 24H2 (community): <https://community.broadcom.com/vmware-cloud-foundation/discussion/windows-11-arm-iso>
- Fusion Arm guest limits (no guest commands): <https://knowledge.broadcom.com/external/article/315602>
- Answer-file driver loading: <https://learn.microsoft.com/en-us/windows-hardware/manufacture/desktop/add-device-drivers-to-windows-during-windows-setup>
  (the setup disc also carries the drivers in `$WinPEDriver$`, which Setup scans [unverified], and `firstlogon.ps1` adds
  them with `pnputil` as a fallback)
