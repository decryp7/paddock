# 0008. Spotter on the Paddock app shell

Status: accepted (2026-10-24; re-applied on the monorepo 2026-10-28)

## Context
Spotter wrote its own frame: a sidebar and router, toasts, a connection banner, an Operations panel with its own job
events, an in-page confirm dialog, a tray popup placed by 660 lines of Rust, its own updater and native-feel modules,
and no exit watchdog (a stuck browser render could hang Quit). The Paddock app shell (Paddock ADR 0012, the `paddock-shell` plugin
and `@decryp7/paddock-ui/shell`) provides the same parts once, as declared contracts with shared tests.

## Decision
- **One plugin, three window kinds.** `paddock_shell::Builder` is the only frame: windows (`paddock-window`: bounds,
  single instance, `--hidden`, the tray and its popover window, Quit), the start-up contract and watchdog, dialogs,
  notifications and service health, jobs, settings, About/Updates/What's New. Spotter's own commands stay in its table
  (`COMMANDS`, `command_table!`) behind the same guard, with window classes (`MAIN`, `TRAY`, `DIALOG`).
  The UI bundle boots by route (`main.ts`): the pages frame, a dialog window, or the tray popover (rule 34).
- **Pages are contributions.** Dashboard and Watches are `definePage`; the wizard and the watch detail are detail routes
  of the frame. Commands, menu items and keys (Check all, Add a watch, Mark all as seen, pause notifications) are
  contributions with a lint (`assertContributionsValid`) and a generated manifest.
- **Jobs on the shell's registry.** The engine keeps reporting through `spotter_core::job::Job` (pure); `jobs.rs` registers
  each operation as a `paddock-jobs` job, forwards phases and counts, and the shell's Cancel stops the engine's work
  (`Cancel::linked`). `spotter://job`, `cancel_job`, the Operations panel and the UI call timeouts are gone. A newer
  wizard test stops the older one in Rust. Progress never goes backwards (the engine's gate and the shell's reporter).
- **Service health replaces the connection banner.** Three services: `sp.scheduler` (supervised; Restart), `sp.sources`
  (the connection: every checked watch unreachable; Check now), `sp.renderer` (a watch needs a browser and none exists;
  Install, a job with Cancel). The decisions are pure functions with tests; repairs never depend on the broken service.
- **Notifications are the shell's.** `Notifier` posts through the shell's port; toasts, their store and the banner are gone.
  OS toasts for new releases (`notifier.rs`, `toast.rs`) are Spotter's domain and stay.
- **Dialogs are OS windows.** The confirm dialog is the `sp.confirm` decision dialog; Settings is the shell's tool window
  with Spotter's page (the existing settings page) beside the shell's Appearance and Updates pages. The theme and the
  "check for updates" settings were removed from Spotter's own settings (no compatibility before the first release).
- **Downloads are the shell's.** Release files, the automatic download action and the browser install use the app's one
  `paddock-download` downloader (segmented Range downloads, 4 connections per host across all downloads, 429/Retry-After
  back-off, resume of a kept `.part`, SHA-256 while writing against the pinned or published digest, the free-space check),
  as phases of a shell job. Spotter's own HTTP download code and its tests of it were removed; the same cases (ranges, no
  ranges, 429, a dropped connection, a wrong checksum, Cancel) are tested against an in-process server. Copies from a
  drive or share keep their own small path (no network involved).
- **Quit never hangs.** The window layer's exit watchdog bounds Quit; its teardown step cancels every job and ends any
  browser a render still holds (`spotter_render::process::end_all`, a registry of supervised process trees).

## Consequences
About 1,900 lines of frame, tray, updater and job code left Spotter; the shell's tests (dialog boot, one notification per
cause, start-up contract) now cover it. Spotter lives in the Paddock monorepo, so it takes the shell crates as path dependencies and `@decryp7/paddock-ui` from `ui/dist`; a shell change and its adoption land in one change. Spotter's settings are
not engine-described sections: its page validates through `save_settings` in Rust, and the shell's generic form is used
for Appearance and Updates only.
