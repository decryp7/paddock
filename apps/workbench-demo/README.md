# Paddock Workbench Demo

The workbench demo (`ui/projects/workbench-demo`) as an installable desktop app on the real `paddock-shell` and
`paddock-workbench` plugins (plan [docs/plans/workbench.md](../../docs/plans/workbench.md) §10, X50 W8): to check the shared
workbench UI on every OS, and to start new IDE-style apps from.

What it shows, all through the real parts:

- the dock: views in groups, perspectives (Understand, Review), saved layouts (kept by the engine in the config folder),
  the layout plan that adjusts a layout to the window with Undo;
- the Explorer, the activity bar, the command palette and the Jobs view with **Reindex** (a sample job with
  phases and Cancel on the shell's job registry);
- panels as windows of their own: pop a view out (or drag its tab out of the window: the drag ghost shows where it will
  open), dock it back, keep it above the main window; the state the views share follows them;
- the assistant pane on Pitwall with one read-only sample tool (`list_files`). Without Pitwall the pane shows a calm "Pitwall
  is not running" (or not installed) state with its fix; pairing needs Pitwall and the user's click.

## Layout

| Path | What |
|---|---|
| `src-tauri/` | the app's Rust side, its own Cargo workspace: `lib.rs` (`shell(place, link)` and `workbench(place, link)`: the two plugins with the demo's parts), `engine.rs` (the sample job), `assistant.rs` (Pitwall as the provider, the sample tool), `commands.rs` (the four commands of the shared needs-AI component and the re-index, behind the app's guard); `tests/app.rs` on Tauri's mock runtime |
| `src-tauri/capabilities/` | one file per window class (`main`, `popout`, `dialog`): the shell's and the workbench's permission set of that class; the drag ghost's window (`drag-ghost`) matches none |
| `src-tauri/icons/` | from the icon pipeline (`icons/workbench-demo/tauri`, `ONLY=workbench-demo node icons/tools/build.mjs`) |
| `../../ui/projects/workbench-demo/` | the UI: in the browser it plays every backend with mocks; in the app every part is the plugins' (`app/desktop.ts`: the one seam to Tauri; `main.ts` picks the window kind: main, pop-out or dialog). `public/drag-ghost.*` are the drag ghost's page, copies of `crates/paddock-panels/assets` (a test checks they agree) |
| `CHANGELOG.md` | the app's own changes; the version's single source is `src-tauri/Cargo.toml` |

The crates are path dependencies here; an app outside this repository takes them from the `decryp7` registry
([crates/README.md](../../crates/README.md), "Consuming the crates"). The Pitwall key lives in memory for the run: a demo
does not touch the user's credential store.

## Build and test

```sh
cd ui && npm ci && npx ng build workbench-demo                 # the UI the app embeds (tauri.conf.json frontendDist)
cd apps/workbench-demo/src-tauri && cargo test                  # the app on the mock runtime (no window, no user folders)
cd ui && npx playwright test --project=workbench-demo           # the UI in the browser (mock backends)
scripts/demo/bundle.sh workbench-demo app,dmg                     # a local DMG to try (never published)
scripts/demo/verify-macos.sh workbench-demo                       # the ad-hoc seal, `codesign --verify --deep --strict`, the app inside the DMG
```

`cd apps/workbench-demo && npm ci && npx tauri dev` runs it against `ng serve workbench-demo`.

## Releases

Only from a `workbench-demo-v<version>` tag in CI (`.gitea/workflows/workbench-demo-release.yml`; rule 9), separate from the
packages' `v*` tags and the shell demo's: the gates, then a DMG (sealed ad-hoc, hardened runtime off, verified), a Windows
NSIS installer and Linux deb and AppImage, and the Gitea release with the version's CHANGELOG section and the First launch
notes. Before tagging: bump `src-tauri/Cargo.toml` and add the CHANGELOG section, `cargo deny check advisories` in
`src-tauri`, one manual run of the workflow (it builds the installers without releasing), the icon check
(`node scripts/demo-release.mjs workbench-demo icons`).

## Start a new app from the workbench demo

An IDE-style app (Pacenotes, Sighter) starts from this demo with the generator:

```sh
node scripts/new-app.mjs sightr "Sightr" --template workbench-demo --accent sighter [--dest <dir>] [--identifier <id>]
```

It copies `ui/projects/workbench-demo` to `app/` and `apps/workbench-demo/src-tauri` to `app/src-tauri/` in a new
repository (the family layout), renames the crate, product name, app id (`APP_ID`, `APP_NAME`, the `<ID>_HOME` override),
bundle identifier, window titles and the sample texts, sets the window accent (`--accent`, one of the app accents; default
graphite), takes the crates and `@decryp7/paddock-ui` from the registries, and writes the repo root files (AGENTS.md, CI
baseline, CHANGELOG). The icons are the demo's: they are the placeholders until step 4. Every rewrite must match or the
script stops, so a change to the demo that the generator does not know fails `npm run test:scripts` here, not in a new app.

Then, in the new app:

1. `cd app && npm install && npx ng build`, then `cd src-tauri && cargo test` (it writes `Cargo.lock`; commit both lock
   files).
2. Keep `main.ts`'s three window kinds, `app/desktop.ts` (the one seam to Tauri) and the three capability files. Replace the
   sample views, perspectives, Explorer provider and commands in `demo.contributions.ts` with the app's (they are
   contributions: `defineView`, `definePerspective`, ...; nothing else changes), and the sample tool and system prompt in
   `assistant.rs` with the app's tools (read-only by default; a tool that writes needs the user's approval).
3. Dialogs: add them to `bootDialogWindow`'s `dialogs` and give the shell `Builder` the app's `dialog_contracts`.
4. Its icon: add it to `icons/tools/design.mjs` (`DEMOS` or `APPS`), run the pipeline with `ONLY=<id>`, check the CVD
   numbers, copy `icons/<id>/tauri` to `src-tauri/icons`; every drawing gets an independent review (rule 22).
5. CI and releases: copy `.gitea/workflows/workbench-demo-release.yml` and rename the tag prefix; the new app's row in `APPS` of
   `scripts/demo-release.mjs` is all the scripts need (`scripts/demo/` takes the app as its first argument).
