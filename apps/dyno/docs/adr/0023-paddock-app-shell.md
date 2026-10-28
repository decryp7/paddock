# 0023. Dyno on the Paddock app shell and the shared dialog API

Status: accepted (2026-10-29), for Dyno 1.0 (Paddock ADR 0012, ADR 0026). This record covers the UI; the Rust side is in the same release.

## Context
Dyno wrote its own frame: a sidebar and top bar, toasts, a CDK dialog service with its own frame component, a confirm
dialog, a command palette, a Copy menu, a theme service, a tray popover page and a 1,191-line design system of its own.
Dyno 1.0 is monitoring-only (ADR 0021 of the monitoring-only record: no rules, no process actions, no helper), so most
of its old dialogs went with those backends. The Paddock shell (`@decryp7/paddock-ui/shell`, the `paddock-shell` plugin)
provides every one of these parts once.

## Decision
- **One bundle, three window kinds** (rule 34), as every shell app: `main.ts` boots a dialog window (`#/dialog?id=…`), the tray
  popover (`#/tray`) or the pages frame. Each is its own lazy chunk; every window provides the appearance.
- **Pages are contributions.** The 17 screens of the sidebar (`shell/nav.ts`) are `definePage` entries with lazy loaders
  (`dy.dashboard`, `dy.processes` …); the CPU detail is a detail route. The command palette, the sidebar, the status bar, the
  menus and the native-feel guard (browser keys, context menu, Copy) are the frame's: `dy-shell`, `dy-toasts`,
  `copy-menu`, `native-feel` and the Coming dialog are deleted. **The command palette (Ctrl+K, jump to a page) is removed for
  1.0**: the pages frame has no shared palette (only the workbench has one), and the sidebar reaches every page. A shared
  palette for the pages frame is a follow-up in Paddock; Dyno adopts it when it exists (no palette of its own is kept). The top bar's agent pill is the status item
  `dy.agent` (the tooltip keeps Dyno's own cost); the shell's service chip covers the agent being down (service `dy.agent`).
- **Settings, About, Updates and What's New are the shell's.** Dyno's page (`dy.general`) sits beside the shell's Appearance
  and Updates pages in the Settings window (a tool dialog). The theme (including `Settings.theme` in the engine), `ThemeService.set`, the About card and the `app_info`
  command are gone; `ThemeService` only answers whether the page is dark now, for the charts, from the shell's appearance store.
- **Notifications are the shell's.** `Notifier` keeps its small API (`show`, `error`) and posts through the shell's port, so
  results of actions land in the notification host and centre; an action is the notice's button (type `dy.notice-action`, the
  caller names the label).
  `dy.alarm` is the notification kind Rust posts alarms with.
- **Dialogs are OS windows with declared actions** (ADR 0026). Contracts are Rust's (`src-tauri/src/dialogs.rs`, generated to
  `generated/dialog-contracts.ts`): `dy.alert-rule` (modal form; stores the rule through the engine's own `upsertAlertRule`/`deleteAlertRule` requests, never by
  reading and writing all settings from the dialog window, and answers `true`),
  `dy.lockers` (read-only tool), `dy.web-search` (modal form), `dy.file-hash` (viewer: Copy, Close, Open VirusTotal; an unreadable
  file is the rejected state with Try again) and the shell's `pd.confirm`, which replaces `dy-confirm-dialog` for deleting an
  alert rule and forgetting the known programs. Components draw no footer. `Dialogs`, `DialogFrame` and the CDK dialog are gone.
- **Styles.** The shared theme (tokens, base, controls, Material) is loaded from `@decryp7/paddock-ui`; Dyno's `--surface-*`,
  `--text-*`, `--sp-*`, `--r-*`, status and chart tokens are a thin bridge over the `--pd-*` tokens in `styles.scss`, so the
  feature components did not change. The heat colour is the accent itself (`color-mix` over `--pd-accent`), so the heat maps follow `data-accent`. Dyno's theme, dark mixins, vibrancy and popover tokens are deleted (the shared css and
  `applyStoredAppearance` own theme, accent, density, contrast and motion).
- **Tray.** The popover page uses the shell's `pd-tray-popover`; fitting, closing and Quit are the shell's tray commands. All tray
  items open the one `tray` popover (the per-item "own popover" option and its `opens` setting are deleted). The icons are still drawn by Dyno's Rust (live, one per item), which the shell cannot yet draw.

## Consequences
The frame, toast, dialog, palette and theme code (about 1,500 lines with their specs) left Dyno; the shell's tests cover it.
What this record does not do, on purpose, and what follows: Material controls for the hand-rolled switch, selects, tabs and
tooltips (Paddock rule 47), the shared `pd-table` for the report, process, alarm and network grids, moving the `.btn`/`.input`
classes to the shared `.pd-btn`/`.pd-input`, and removing the token bridge.
