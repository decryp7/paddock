# 0028. Every menu-bar item is its own icon, and the OS is given what it needs to remember its place

- Status: Proposed (2026-11-03; every claim marked [V] needs the VM checks at the end before this is Accepted)
- Date: 2026-11-03
- Related: ADR 0023 (the app shell), ADR 0027 (the helper for 1.1.0, on branch docs/dyno-helper-1-1; no dependency, listed so
  the numbers are not confused), the user's request of 2026-11-03 ("one for each metric so I can rearrange position on the menu
  bar or system tray"), ER-21, ER-22, ER-27 (logs).
- Evidence marks: [S] read in source on 2026-11-03: tauri 2.12.5, tray-icon 0.25.1, objc2-app-kit 0.3.2 (all in
  `~/.cargo/registry/src`); [I] inference; [V] still to verify on a VM.

## Context

Dyno builds one `TrayIconBuilder` icon per item, but the settings capped the items at 4, and the OS could not tell the icons
apart across runs, and `sync_items` removed and made every icon on any change.

## Decision

1. **Cap.** `MAX_ITEMS = 32` (`dyno-core` `widgets/menu_bar.rs`), the dashboard's bound; the item id is at most 32 characters of
   `[a-z0-9-]`, which the identities derive from. More than a few dozen icons cannot be placed in any bar. Cost: the live
   pictures are drawn only for changed items, the template flag and the picture are set in one call, and the tooltip is sent
   only when it changed; the drawing time of 32 graph items is in "Measured" below.
2. **Which icons change** (`tray_model::plan_sync`): only items that are gone or differ are removed and only new or differing
   ones made, so an unchanged icon keeps its native item. A layout change no longer replaces all icons.
3. **Identity** (`tray_identity.rs`, pure, deterministic, no clock): the item id is the key. The id comes from the widget kind
   (`ring`, `ring-2`), not from the metric, and is kept in the layout: removing an item and adding another of the same kind
   gives the new one the old id and so the old place; two items of one kind swap places if the first is removed.
   - **macOS:** `NSStatusItem.autosaveName = net.decryptology.dyno.menubar.<id>` [S: `NSStatusItem::setAutosaveName`;
     tray-icon exposes the item as `TrayIcon::ns_status_item()`, Tauri as `with_inner_tray_icon`], set right after the icon is
     built. AppKit keeps the position under the defaults key `NSStatusItem Preferred Position <name>` and, once the user has
     hidden or shown the item, `NSStatusItem Visible <name>`; the visibility key does not exist until the user changes it, so
     "hidden stays hidden" holds only for items the user hid [V]. AppKit is **believed to delete these keys when the status item
     is removed** (tray-icon's `Drop` calls `removeStatusItem`; Tauri removes all icons on exit) [I, V]. So Dyno copies the keys
     to `net.decryptology.dyno.saved.<key>` before it removes an item, at `ExitRequested` and `Exit`, and every 30th picture of
     an item (a drag in the bar tells us nothing), and puts the **position** back before an item is made, never over a key AppKit
     holds and never the visibility copy (an item the user hid once would stay hidden whenever it is made again; the copy only
     keeps the value from being lost). Copies of removed items stay in the app's defaults for good, bounded by the id space
     (at most two keys per id, ids are `[a-z0-9-]` up to 32 characters, in practice a few dozen) (`tray_position.rs`). Whether the copy is needed at all is a [V] item.
   - **Windows:** `guidItem`, a GUID from SHA-256 of a Dyno-only name, **UUID version 8** (RFC 9562, custom; not version 5, which
     is SHA-1) with the RFC variant bits. tray-icon has `TrayIconAttributes::guid` [S: `src/lib.rs`, `windows/mod.rs`
     `apply_guid`], but **Tauri 2.12.5's `TrayIconBuilder` has no setter** (`src/tray/mod.rs`) and a GUID can only be given when
     the icon is added. The smallest safe change is a vendored tray-icon 0.25.1 with a GUID provider, plus a recovery path
     (`apps/dyno/vendor/PATCHES.md`, `[patch.crates-io]`). **Dyno is not Authenticode-signed**, and Windows binds a GUID to the
     path of the first executable that used it unless the binary is signed [S: the doc comment on `guid`; Microsoft's
     NOTIFYICONDATA remarks]. So `NIM_ADD` can fail when another `dyno.exe` (an older install folder, a build in another
     place) or a crashed instance holds the GUID. The patch then deletes the GUID's registration and adds again, and if that
     fails adds the icon without a GUID; either way the icon appears, and the result (`Registered`,
     `RegisteredAfterReclaim`, `FellBackToNoGuid`, `NotAdded`) is in the log line; the same recovery runs when the icon is added again after
     `TaskbarCreated` (Explorer restarted, or it was not up at the first add) of each icon (ER-27). Consequences for an
     unsigned build: places survive restarts and updates **only while the executable stays at the same path**; an install to a new
     path takes the GUID over (position reset [V]); a build that cannot take it over has no remembered place. Signing the
     releases would remove the path rule; that is a release decision, not made here.
   - **Linux:** appindicator's id is `tray-icon tray app dyno-<id>` (stable); StatusNotifier hosts choose the order, so there is
     nothing more to set [I].
4. **Dependencies:** `tray-icon` is pinned `=0.25.1` with default features off in Dyno's manifest (a Windows-only dependency
   for the provider), `deny.toml` allows one `tray-icon` version, and the notices are regenerated from the lock.
5. **Logs (ER-27):** one line per icon created with its id, metric and identity (and on Windows the GUID outcome), one line per
   sync with what is kept, removed and made, the saved and restored macOS keys, and every refused picture.

## The approved look (2026-11-03)

The renderers are `dyno_core::tray::{menu_bar, pixel, text}` (docs/design/dyno-tray/README.md). `tray_look.rs` maps an item to a
format and renders it (macOS: 1x and @2x templates of one point size; Windows and Linux: a square icon at the taskbar's pixel size,
re-read from `Shell_TrayWnd`'s DPI on every update so a DPI or theme change shows within a second). The CoreText adapter of the text
port is `tray_text.rs`, the app's edge. The macOS pictures reach the status item through the second tray-icon patch
(`vendor/tray-icon-image-reps.patch`). The default set is four stacked numbers (CPU, memory, network, temperature). The alert comes from
Dyno's own alarms: an item whose metric an enabled alarm rule covers (the overheating alert for the temperature, the user's
rules for theirs; `tray_look::AlertPolicy`) shows its alert exactly while the agent has an open alarm for that metric (the shell
follows the alarm events; an alarm raised before the app connected is not known until it changes: open item). A load no rule
covers uses the Settings value `alerts.trayAlertPercent` (default 90, 50 to 100): the level held for 3 s of sample time, cleared
5 points below (`AlertGate`). A number item has a format, stacked (the default) or one line (`NumberFormat` on the wire,
mirrored by ts-rs, a select in the item's settings), and Settings says what macOS and Windows 11 do with items that do not fit once
more than four are on. When the agent has been away for 3 s (`tray_away::AwayTimer` on the injected `paddock_jobs::Clock`, rule 60) every item shows its unavailable frame (a dimmed dash at the
same width) and the live pictures return with the agent.

## Measured

2026-11-03, this Mac, debug build, `cargo test -p dyno-app --lib measure_32 -- --ignored --nocapture`: 32 graph items, every
picture changing every tick: 6400 frames in 200 ticks, **about 0.21 ms per tick** to draw all 32. Drawing is not the cost to
worry about; the native calls are (one image set per changed item per tick, one call each), and they are bounded by the model
returning only changed pictures and by `paint_ops` (one image call, tooltip only when changed). Not measured: the native cost of
32 status items on a Mac and a PC (a [V] item: CPU of Dyno with 32 items, from Activity Monitor and Task Manager).

## The macOS flicker (user report, 2026-11-03)

Suspects and what settles each: (1) two image sets per picture (`set_icon` renders the image as non-template, then
`set_icon_as_template` renders it again: visible in tray-icon's macOS `set_icon`, which passes `false`) is removed by one
`set_icon_with_as_template` call, pinned by `paint_ops` (`image_sets == 1`); (2) all icons replaced on any change is removed by
`plan_sync`; (3) a picture whose size changes moves the status item's width: `an_items_picture_keeps_its_size_whatever_the_value`
shows every glyph keeps one size across 0 to 100 %; (4) redraws without a change: `an_unchanged_value_draws_nothing`. Each
picture is logged at debug (`tray icon <id>: picture #n set (1 call, ...)`). Which suspect caused the user's flicker is **not yet
measured** [V]: record the menu bar (screen recording or rapid screenshots) on the 1.0.0 build and on this one, with the debug
level on, and count image sets per tick in the log.

## To verify [V] (VMs, after the coordinator's go)

- macOS: reorder, quit, start; change the layout; read `defaults read` for the keys above before and after removal; upgrade from 1.0.
- Windows 11: pin and reorder, quit, start, update; run a second `dyno.exe` from another folder; kill and start; restart Explorer.
- Linux: smoke test that the icons appear.
