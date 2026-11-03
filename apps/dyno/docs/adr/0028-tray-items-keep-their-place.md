# 0028. Every menu-bar item is its own icon, and the OS remembers its place

- Status: Proposed (2026-11-03; needs the VM checks below before Accepted)
- Date: 2026-11-03
- Related: ADR 0023 (the app shell), the user's request of 2026-11-03 ("one for each metric so I can rearrange position on
  the menu bar or system tray"), ER-27 (logs).
- Evidence marks: [S] read in source on 2026-11-03: tauri 2.12.5, tray-icon 0.25.1, objc2-app-kit 0.3.2 (all in
  `~/.cargo/registry/src`); [I] inference; [V] still to verify on a VM.

## Context

Dyno already builds one `TrayIconBuilder` icon per item, but the settings capped the items at 4, and the OS could not tell
the icons apart across runs: the user's order and visibility were lost whenever an icon was recreated.

## Decision

1. **Cap.** `MAX_ITEMS = 32` (`dyno-core` `widgets/menu_bar.rs`), the dashboard's bound; the item id is at most 32
   characters of `[a-z0-9-]`, which the identities below derive from. More than a few dozen icons cannot be placed in any bar.
2. **Identity** (`tray_identity.rs`, pure, deterministic, no clock): the item id is the key.
   - macOS: `NSStatusItem.autosaveName = net.decryptology.dyno.menubar.<id>` [S: `NSStatusItem::setAutosaveName`; tray-icon
     exposes the item as `TrayIcon::ns_status_item()`, Tauri as `TrayIcon::with_inner_tray_icon`]. Set right after the icon is built.
   - Windows: `guidItem`, a name-based GUID (SHA-256 of a Dyno-only name, RFC 4122 version 5 and variant bits). tray-icon has
     `TrayIconAttributes::guid` [S: `src/lib.rs`, `platform_impl/windows/mod.rs` `apply_guid`], but **Tauri 2.12.5's
     `TrayIconBuilder` has no setter** (`src/tray/mod.rs`) and a GUID can only be given when the icon is added. The smallest
     safe change: a vendored copy of tray-icon 0.25.1 with one addition, `set_guid_provider`, asked by `TrayIcon::with_id`
     when no GUID was given (`apps/dyno/vendor/PATCHES.md`, `[patch.crates-io]` in `apps/dyno/Cargo.toml`). Drop it when
     Tauri exposes the GUID. Windows binds a GUID to the executable's path unless it is Authenticode-signed by the same
     publisher [S: the doc comment on `guid`], so the release (signed) keeps places across updates, an unsigned dev build does not.
   - Linux: appindicator's id is `tray-icon tray app dyno-<id>` (stable); StatusNotifier hosts choose the order, so there is
     nothing more to set. [I]
3. Logs (ER-27): one line per icon created with its id, metric and identity, or the error.

## Consequences

- Placement ids come from the layout and are kept in it, so they are stable until the user removes the item; a removed and
  re-added item may get a new id and so a new place.
- `sync_items` still replaces all icons on a change; with stable identities they come back where they were.

## To verify [V]

macOS and Windows VMs: add several items, drag them to a new order, quit and start, update the app: positions and
visibility kept. On macOS also that removing the icon on a layout change keeps the saved position (AppKit may drop a
position when its item is removed).
