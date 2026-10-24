# Paddock Shell Demo

The shell demo (`ui/projects/shell-demo`) as an installable desktop app on the real `paddock-shell` plugin (plan
[docs/plans/workbench.md](../../docs/plans/workbench.md) §10, S18): to check the shared UI on every OS, and to start new
apps from.

What it shows, all through the shell's real parts:

- the pages frame with sample pages, menus and keys, notifications;
- service health: **Break the demo engine** (its Restart works) and **Break the search index** (its Reset fails, so the
  notification keeps the new cause), both reported by the app's Rust side;
- jobs: **Start sample job** (finding files, reading, writing the index that cannot be cancelled) and **Start quiet job**
  (reports nothing until you cancel it: "Still working");
- Settings (General and Storage: one field of every kind), About, Updates (no update key yet: "Updates are not
  available in this build") and What's New with [CHANGELOG.md](CHANGELOG.md);
- dialogs of every kind as real OS windows (Dialogs page);
- the tray icon: a left click opens the popover (Show window, Start sample job, Quit), the menu shows the window or quits.

## Layout

| Path | What |
|---|---|
| `src-tauri/` | the app's Rust side, its own Cargo workspace: `lib.rs` (the shell plugin with the demo's parts, `shell(Place)`), `engine.rs` (services and jobs), `commands.rs` (the demo's guarded commands), `sections.rs`, `tray.rs`; `tests/app.rs` on Tauri's mock runtime |
| `src-tauri/icons/` | from the icon pipeline (`icons/shell-demo/tauri`, `ONLY=shell-demo node icons/tools/build.mjs`); a neutral silver tile that keeps the family's CVD minimums |
| `../../ui/projects/shell-demo/` | the UI: in the browser it plays the engine with in-memory stand-ins, in the app every part is the plugin's (`demo-ipc.ts`, `main-window.ts`, `dialog-window.ts`, `tray-window.ts`) |
| `CHANGELOG.md` | the app's own changes (What's New); the version's single source is `src-tauri/Cargo.toml` |

The shell crates are path dependencies here; an app outside this repository takes them from the `decryp7` registry
([crates/README.md](../../crates/README.md), "Consuming the crates").

## Build and test

```sh
cd ui && npm ci && npx ng build shell-demo                     # the UI the app embeds (tauri.conf.json frontendDist)
cd apps/shell-demo/src-tauri && cargo test                      # the app on the mock runtime (no window, no user folders)
cd ui && npx playwright test --project=shell-demo               # the UI in the browser, and over a fake Tauri IPC (shell-demo-desktop.spec.ts)
scripts/shell-demo/bundle.sh app,dmg                            # a local DMG to try (never published)
scripts/shell-demo/verify-macos.sh                              # the ad-hoc seal, `codesign --verify --deep --strict`, the app inside the DMG
```

`cd apps/shell-demo && npm ci && npx tauri dev` runs it against `ng serve shell-demo`.

## Releases

Only from a `shell-demo-v<version>` tag in CI (`.gitea/workflows/shell-demo-release.yml`; rule 9), separate from the
packages' `v*` tags: the gates, then a DMG (sealed ad-hoc, hardened runtime off, verified), a Windows NSIS installer and
Linux deb and AppImage, and the Gitea release with the version's CHANGELOG section and the First launch notes. Before
tagging: bump `src-tauri/Cargo.toml` and add the CHANGELOG section, `cargo deny check advisories` in `src-tauri`, one
manual run of the workflow (it builds the installers without releasing), the icon check
(`node scripts/shell-demo-release.mjs icons`).

## Start a new app from the demo

```sh
node scripts/new-app.mjs pacenotes "Pacenotes"                  # writes ../pacenotes (or --dest <dir>); --identifier com.example.x
```

It copies the UI and the Rust side into the family layout (`app/` with its own Angular workspace, `app/src-tauri/`),
renames the product name, id, bundle identifier, crate, tray images and every path, and takes `@decryp7/paddock-ui`,
`@decryp7/paddock-e2e` and the shell crates from the registries. Every rewrite must match or the script stops, so a
change to the demo that the starter does not know fails `npm run test:scripts` here, not in a new app.

Then, in the new app:

1. `cd app && npm install && npx ng build`, then `cd src-tauri && cargo test` (it writes `Cargo.lock`; commit both
   lock files).
2. Its icon: add it to `icons/tools/design.mjs` in this repository (`DEMOS` or `APPS`), run the pipeline with
   `ONLY=<id>`, check the CVD numbers (`node icons/tools/cvd.mjs`), copy `icons/<id>/tauri` to `app/src-tauri/icons`
   and the tray images to `icons/tray` with the names `src/tray.rs` uses.
3. Replace the samples one at a time: the pages, services, jobs, settings and dialogs keep the demo's `sd.` ids until
   then. After each change: `PADDOCK_WRITE_TS=1 cargo test --test app` (the UI's command names) and the UI's specs
   (`dialog-contracts.json` and `paddock-manifest.json` are written from the registry). `paddock-check-dialog-commands`
   is not set up in the copy: it walks the shell's TypeScript sources, which the npm package does not ship yet.
4. CI and releases: copy `.gitea/workflows/shell-demo-release.yml` and `scripts/shell-demo/` from here and rename the
   tag prefix, or follow an app that already releases (Pitwall).
