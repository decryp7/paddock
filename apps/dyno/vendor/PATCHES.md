# Vendored crates

## tray-icon 0.25.1

- Source: crates.io `tray-icon` 0.25.1, `.crate` SHA-256 `b2b9c52859a94554803ccd4a24b98f74148ebc73b90676d783f3490b1bff9d72`
  (the version Tauri 2.12.5 resolves in `Cargo.lock`; copied and read 2026-11-03). Repository
  <https://github.com/tauri-apps/tray-icon>.
- Licence: `MIT OR Apache-2.0` (kept: `LICENSE-MIT`, `LICENSE-APACHE`, `LICENSE.spdx`, and `license` in its `Cargo.toml`).
  cargo-deny sees it as a path dependency with that licence; the notices generator reads it from `cargo metadata` like any
  other package.
- Two patches on the copy, nothing else: [`tray-icon-guid-provider.patch`](tray-icon-guid-provider.patch) (Windows, below) and
  [`tray-icon-image-reps.patch`](tray-icon-image-reps.patch) (macOS, below). The GUID patch: `src/lib.rs`: `set_guid_provider` and its
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
