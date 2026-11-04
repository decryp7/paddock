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
