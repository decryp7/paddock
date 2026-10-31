# 12. Widgets and tray items: the dashboard and the menu-bar items are data, not code

- Status: accepted (2026-10-22); **superseded in part by family ADR [0035](../../../../docs/adr/0035-widget-layouts-are-shared.md) (2026-10-31)**: the layout model (the catalogue, limits, edits, presets, undo, reading and saving), the `tray` and `trayItems` settings and their TypeScript copy moved into the shared `paddock-widgets` crate and `@decryp7/paddock-ui/widgets`; the three surfaces (dashboard, menu-bar items, tray popup) are Dyno catalogues in `dyno_core::widgets`. What stays here: widgets and tray items are data, not code, and the reasons for the dashboard's grid look and the per-OS tray rendering
- Related: ADR 0009 (native feel), 0011 (design system), `app/src/app/features/dashboard`, `app/src/app/features/tray`,
  `dyno_core::tray`, `dyno_core::settings`

## Context

Users of iStat Menus and Stats expect to choose what the dashboard shows and to put several live items in the menu bar or
system tray (CPU, memory, network, per-core bars). Hard-coding layouts per screen would make every new metric a UI edit, and
the three OSes render tray items very differently.

## Decision

1. **A layout is data.** The dashboard is a list of `WidgetInstance { id, kind, size, options }` (kinds: tiles such as CPU,
   per-core CPU, memory, disk, network, GPU, battery, top apps, health, history chart). The tray is a list of
   `TrayItem { id, metric, style, options }`. Both live in `settings` (versioned, optional keys with defaults, a migration
   per breaking change, damage falls back to the backup then to the defaults). The agent stores them; the UI only edits them.
2. **A catalogue, not free code.** Widget and tray kinds are a closed enum in the core with their metadata (title, needed
   readings, allowed sizes and styles). Unknown kinds in a settings file are dropped with a note, never rendered.
3. **Edit mode, gallery, presets.** The UI offers an edit mode (drag, resize, remove), a gallery of every kind and presets
   (Minimal, Developer, Gamer, Laptop) which are just layouts shipped as data. Applying a preset is undoable (the previous layout
   is kept until the next save).
4. **Pure rendering models.** What a tray item shows (value, colour band, per-core bars, label) is computed by a pure function in
   `dyno_core::tray` from the latest `LiveUpdate` and the item; the shell and the UI draw it.
5. **Per-OS rendering.** macOS: several `NSStatusItem`s (template images or drawn text and bars) and a native popover (ADR 0009).
   Windows: one tray icon per item is not possible with Tauri's single tray, so extra items are shown as a composed icon plus
   the popover; Linux: a status-notifier icon (one item) plus the popover. The number of simultaneous items is therefore an
   OS capability the shell reports, and the editor limits choices to it.
6. **Bounded.** At most 24 widgets and 6 tray items; sizes and options are validated in Rust.

## Consequences

- A new metric is a catalogue entry plus a pure rendering model, not a new screen.
- Tray item counts differ per OS; docs and screenshots show the macOS menu bar and the single-icon fallback.
