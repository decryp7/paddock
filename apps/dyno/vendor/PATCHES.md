# Vendored crates

## tray-icon 0.25.1

- Source: crates.io `tray-icon` 0.25.1, `.crate` SHA-256 `b2b9c52859a94554803ccd4a24b98f74148ebc73b90676d783f3490b1bff9d72`
  (the version Tauri 2.12.5 resolves in `Cargo.lock`; copied and read 2026-11-03). Repository
  <https://github.com/tauri-apps/tray-icon>.
- Licence: `MIT OR Apache-2.0` (kept: `LICENSE-MIT`, `LICENSE-APACHE`, `LICENSE.spdx`, and `license` in its `Cargo.toml`).
  cargo-deny sees it as a path dependency with that licence; the notices generator reads it from `cargo metadata` like any
  other package.
- Five patches on the copy, nothing else, applied in this order to the pristine crate (`patch -p1` inside a copy of the `.crate`'s contents):
  [`tray-icon-guid-provider.patch`](tray-icon-guid-provider.patch) (Windows: the GUID provider, `guid_status`, and `is_added`),
  [`tray-icon-image-reps.patch`](tray-icon-image-reps.patch) (macOS), [`tray-icon-windows-tooltip.patch`](tray-icon-windows-tooltip.patch)
  (Windows) and [`tray-icon-autosave-name.patch`](tray-icon-autosave-name.patch) (macOS autosave name and the Windows `TaskbarCreated`
  read-out) and [`tray-icon-recovery-trace.patch`](tray-icon-recovery-trace.patch) (Windows: the steps of the add ladder with `GetLastError`). Check after a change: unpack tray-icon 0.25.1, apply the five in that order, and `diff -r -x Cargo.lock -x .cargo_vcs_info.json -x .cargo-ok`
  against `vendor/tray-icon` prints nothing (done 2026-11-05). The GUID patch (which also carries `TrayIcon::is_added` and the `added` flag the shell's answer sets): `src/lib.rs`: `set_guid_provider` and its
  use in `TrayIcon::with_id`, and `GuidStatus` with `TrayIcon::guid_status`. `src/platform_impl/windows/mod.rs`: when the shell
  refuses a GUID (`NIM_ADD` fails), delete the GUID's registration (`NIM_DELETE` by GUID) and add again; if that fails too, add
  the icon without a GUID (and the window forgets the GUID); the outcome is `guid_status`, which Dyno logs. `Cargo.lock` is dropped from the copy. Nothing else differs from the `.crate`.
- Why: Windows keeps an icon's place and visibility under its `guidItem`, and tray-icon has `TrayIconAttributes::guid`
  and `TrayIconBuilder::with_guid` (0.25.1), but **Tauri 2.12.5's `TrayIconBuilder` has no GUID setter** (`src/tray/mod.rs`:
  its `inner` builder is private and `with_inner_tray_icon` runs only after the icon exists), and the GUID can only be set
  when the icon is added. No upstream Tauri API or issue found in a web search on 2026-11-03.
- Removal plan: file the upstream request (a `TrayIconBuilder::guid` in tauri-apps/tauri; link to be added here when
  filed: open item for the coordinator, as an issue needs the user's GitHub account), then delete this folder, the
  `[patch.crates-io]` entry and the `vendor` exclude in `../Cargo.toml`, and the `tray-icon` dependency in
  `app/src-tauri/Cargo.toml`, and call the new builder method in `tray.rs`. Renovate does not update a vendored copy, so a
  Tauri bump that moves tray-icon needs this copy re-made (the `[patch]` is then reported as unused or mismatched by cargo).
- Scope (ER-22): only Dyno has several tray icons whose places matter today (Pitwall, Spotter and the shell use one icon,
  through `paddock-window`). If another app wants a stable GUID, move the vendor folder and the provider to the shared
  place (`crates/`) in that change; until then it stays in Dyno. ADR 0028.

### tray-icon-image-reps.patch (macOS)

- `src/platform_impl/macos/mod.rs`, `set_icon_for_ns_status_item_button`: the NSImage is complete (size, template flag) **before**
  `setImage`; upstream sets the image first, so for a moment the button shows a non-template image at its pixel size.
- New `TrayIcon::set_icon_reps(reps, size_pt, template)` (`src/lib.rs`, macOS only): one NSImage with one `NSBitmapImageRep` per
  scale, each filled from the RGBA bytes directly (no PNG round trip), sized in points, the template flag set, then one
  `setImage`. Dyno draws the 1x and @2x menu-bar images separately (design `docs/design/dyno-tray/README.md` §2) and reaches this
  through Tauri's `with_inner_tray_icon`. `Cargo.toml`: objc2-app-kit features `NSBitmapImageRep`, `NSGraphics`, `NSImageRep`.
- Why: tray-icon 0.25.1 accepts one image as PNG bytes at a size derived from its aspect ratio, so it cannot carry two scales or a
  width that is not the image's own (read 2026-11-03).
- Removal: when tray-icon builds the image before setting it and accepts several representations (an upstream issue on
  tauri-apps/tray-icon, to be filed by the coordinator; link to add here), delete this patch and its edits and call the upstream API.

### tray-icon-windows-tooltip.patch (Windows)

- `src/platform_impl/windows/mod.rs`, `set_tooltip`: the `NIM_MODIFY` names the icon by its GUID (`tooltip_nid` calls `apply_guid`, as
  `set_icon` and `set_tray_visible` do), and a tip keeps at most 127 UTF-16 units so `szTip` always ends in a zero (upstream copies 128).
- Why: Dyno adds every icon with a GUID (ADR 0028). A modify without `NIF_GUID` was refused by the shell, so every icon kept the tip it was
  built with ("Dyno"); Dyno ignored the error then and logs it now.
- Tests: `tooltip_tests` in the same file. This vendored crate is not a workspace member, so its tests are not built by the app's
  workspace (`cargo xwin clippy -p tray-icon --all-targets` from `apps/dyno` fails on the dev-dependency `serde_json`; that is not an
  error in the crate). Build them standalone: copy `vendor/tray-icon` out of the repo, drop the `eframe`, `image`, `tao`, `winit` and gtk
  dev-dependencies, add `[workspace]`, `cargo xwin test --no-run --lib --target x86_64-pc-windows-msvc`, run the `.exe` on a Windows machine.
  Run on `paddock-win11-test` (snapshot `gui-session-tools`, 2026-11-04): without `apply_guid` `a_tip_change_names_the_icon_by_its_guid`
  fails; with it both tests pass.
- Removal: when upstream tray-icon sets the tip by GUID and caps it at 127 units, delete this patch.

### tray-icon-autosave-name.patch (macOS, and a Windows read-out)

Apply with `patch -p1 -d vendor/tray-icon < vendor/tray-icon-autosave-name.patch` on a fresh copy (after the other three patches).


- `src/lib.rs`: `set_autosave_name_provider` and its type `AutosaveNameProvider`, like the GUID provider. `src/platform_impl/macos/mod.rs`,
  `TrayIcon::create`: the status item gets its `autosaveName` from the provider right after `statusItemWithLength` and before the image,
  tooltip or menu is set. Upstream never sets one; Dyno set it after `build` (ADR 0028), so ControlCenter first saw the item as
  `Item-0` and logged `Host id changed from ...Item-0... to ...menubar.<n>`. Housekeeping, not the cause of the missing items at login
  (there is no measured effect on showing the items: lessons.md, 2026-11-05 retraction).
- `src/platform_impl/windows/mod.rs` and `src/lib.rs`: `TrayIcon::taskbar_restarts() -> (count, last outcome)`: how many `TaskbarCreated`
  messages the icon saw and what its re-add came to (`GuidStatus`). Dyno logs it in `tray diagnose:` so a diagnostic bundle from a PC
  whose icons are missing at logon shows whether Explorer's taskbar was created after the add. Elsewhere `(0, None)`.
- Removal: when upstream lets a builder set the autosave name before the item connects, delete the macOS part; the Windows read-out goes
  with the GUID patch.

### tray-icon-recovery-trace.patch (Windows)

- `src/platform_impl/windows/mod.rs`: `register_with_guid_recovery` records each step of the add ladder (add with GUID, delete the GUID's
  registration, add with GUID again, add without GUID; and the delete before a `TaskbarCreated` re-add) and the `GetLastError` of each one that
  failed, read at once after the call. The steps are kept per icon (the first add, then each `TaskbarCreated` re-add, the last 700 bytes) and
  read with `TrayIcon::recovery_trace()` (`src/lib.rs`; empty elsewhere). `remove_tray_icon` now returns the error code of a failed delete and
  still prints its line. The ladder's behaviour is unchanged; nothing else reads the trace except Dyno's log (`GUID ...: <status> (steps: ...)`
  on the icon-created line and in `tray diagnose:`).
- Why: a Windows diagnostic bundle must show which step of the ladder the shell refused and with what code (the shell sometimes leaves 0).
- Not run on Windows: compiled and linted with `cargo xwin clippy` only.
- Removal: with the GUID patch, when Tauri exposes a GUID builder.

## sysinfo 0.39.6

- Source: crates.io `sysinfo` 0.39.6, `.crate` SHA-256 `d2071df9448915b71c4fe6d25deaf1c22f12bd234f01540b77312bb8e41361e6`
  (the checksum in `Cargo.lock`; copied and read 2026-11-10). Repository <https://github.com/GuillaumeGomez/sysinfo>.
- Licence: `MIT` (kept: `LICENSE` and `license` in its `Cargo.toml`).
- Dropped from the copy: `Cargo.lock`, `.gitignore` (unanchored rules), `.github/` and `.cirrus.yml` (upstream CI). One patch,
  nothing else: [`sysinfo-null-iterator.patch`](sysinfo-null-iterator.patch). Check after a change: unpack sysinfo 0.39.6, apply it
  with `patch -p1`, and `diff -r --strip-trailing-cr -x Cargo.lock -x .gitignore -x .github -x .cirrus.yml` against `vendor/sysinfo`
  prints nothing (done 2026-11-10; git stores `md_doc/sid.md`, the one CRLF file, with LF). The test it adds runs with `cargo test --manifest-path vendor/sysinfo/Cargo.toml --lib paddock_tests` (macOS;
  delete the `Cargo.lock` it writes); it fails on the pristine crate with the panic below.
- Why: on macOS, `iterate_service_tree` (`src/unix/apple/macos/disk.rs`, behind `Disk` kind and I/O refreshes) assumed that a
  successful `IOServiceGetMatchingServices` always gives a non-zero iterator and built a `NonZeroU32` from it unchecked. Apple
  documents the opposite (`IOKitLib.h`: "If NULL is returned, the iteration was successful but found no matching services"),
  which is what a BSD name whose disk is gone gets, for example a disk image detached between sysinfo's mount listing and
  this lookup. Debug builds panic (`utils.rs:19`, `assertion left != right failed (left: 0, right: 0)`, CI run 7848); release
  builds construct a `NonZeroU32` of 0, which is undefined behaviour. The patch is upstream's fix
  (<https://github.com/GuillaumeGomez/sysinfo/pull/1709>, merged 2026-07-31, first released in 0.40.0) plus a regression test.
- Removal plan: the family moves to sysinfo 0.40 (a breaking release: `Disks::list` and others removed) in the shared crates,
  Debrief, Spotter and Dyno together; then delete this folder, the patch, this entry and the `sysinfo` line under
  `[patch.crates-io]` in `../Cargo.toml`. Renovate does not update a vendored copy.
- Scope (ER-22): Debrief and Spotter resolve the same unpatched 0.39.6 (Debrief with the `disk` feature, so it is exposed on
  macOS too). If they need the fix before the 0.40 move, move this folder to the shared place in that change.
