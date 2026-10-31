# 22. The tray popup shows widgets you choose

- Status: accepted (2026-10-31, user: every app with a tray gets the same customizable popup)
- Related: family ADR 0035 (widget layouts are shared: `paddock-widgets`, `@decryp7/paddock-ui/widgets`), ADR 0021 (quitting),
  ADR 0017 (model update notices), AGENTS.md "Where logic lives"; code:
  [`widgets/`](../../crates/pitwall-core/src/widgets/mod.rs), [`popup.rs`](../../app/src-tauri/src/popup.rs),
  [`features/tray/`](../../app/src/app/features/tray/tray-popover.ts)

## Context

The popup was a fixed card: memory, throughput, three loaded models, two banners and three buttons. The person wants to
choose what it shows (family ADR 0035), and the popup should answer one question: is my local AI working, and what is it doing.

## Decision

- **Seven widgets, each a closed kind** (`PopoverWidget`): Server (serving, paused, stopped or not running, the port, engine
  health), Activity (tokens per second, running, waiting), Memory (RAM and video memory with a tone and the OS pressure), Loaded
  models (2 rows, "+N more"), Downloads (running ones are the shared *working* state with a progress bar and Cancel; paused
  and failed ones are 2 rows), Updates (model update notices, ADR 0017) and Recent errors (error events of the last day, 2 rows). Each is
  placed at most once. **Defaults:** Server and Activity (half width each), Memory and Loaded models: measured in the browser build: 415 px calm, 411 px empty, 477 px worst (a notice, a paused server, memory critical, five models) and 513 px with the server down, against the 520 px cap, so Recent errors, Downloads and Updates are one click away in Settings rather than default. Offline mode no
  longer exists (removed in 1.1), so there is no offline widget.
- **All domain logic is Rust, in `pitwall-core::widgets`**, pure and given the clock: `Facts` is the accumulator the live feed writes
  (`seed` from the dashboard snapshot, `apply` per pushed update, `set_disconnected`); `build` turns facts and layout into the
  values, the header (status pill, whether Pause or Resume and Release memory work) and the surface notice. The engine decides tones
  (meters turn warning above 85 % and danger above 95 %), which row comes first, the row counts (`rows_of`, also the loading
  shape), whether an empty state is calm (`WidgetState::Empty { calm }`: no downloads, no errors in the last day, models up to
  date), what is stale (a lost feed keeps the last readings and marks them stale from the first failure) and the one notice (a
  server that does not answer, then apps waiting to pair).
- **The layout is the app's own**, in `popup-widgets.json` in the data folder (`paddock-widgets` reads it leniently and versioned;
  a newer or damaged file is kept and never lost; one level of undo), because it is a preference of this app and the popup must
  draw while the server is down. `PopupState` (app) holds the layout and the facts, saves an edit before it takes effect and
  publishes `pitwall://popover` (layout, values, header) only when the snapshot changed.
- **What the feed does not carry** (downloads, recent events, update notices) is read once after each connect, and the notices again
  on a model-update event. A failed read leaves the widget saying "not checked yet".
- **The UI only draws.** Templates per kind (`widget-set.ts`) and the status pill map the engine's enums to words and icons; the
  mock serves the engine's recorded output for the scenarios `demo`, `empty`, `worst`, `down`, `paused` and `connecting`
  (`popover-fixtures.json`, written and checked by the `widgets::build` tests), so it cannot drift.
- **Footer and menu match Dyno's:** "Open Pitwall" right-aligned beside a ⋯ menu: Customize widgets…, Pause or Resume serving,
  Release memory, Settings…, a separator, Quit Pitwall. "Customize widgets…" opens the Settings window at the Tray popup page: the
  popup asks the main window for the route `/settings/tray-popup`, which opens (or re-targets) the Settings dialog.
- **The popup fits the family's 520 px cap in its worst state**, proven by `e2e/tray.e2e.ts` (the defaults in the demo, empty, worst
  and down states), and a loading widget is as tall as a ready one (the same e2e compares them).

## Parallelism and cost

Nothing here is slow: the widgets read facts already held in memory. The only extra reads are four small calls after a connect and
two after a model-update event; the snapshot is compared before it is pushed, so the one-second memory samples cost one small event
per change, not per tick.

## Consequences

- A new widget is a new variant, a `KindSpec`, a builder arm and a template; the chooser, the preview, the limits and the undo come
  from the shared crate and package.
- The popup no longer has a live feed of its own: it draws what the app pushes (the tray window's backend calls are the five
  commands and one event listed in `tray-window.ts`).
- Pairing and model-update banners are gone as such: a pairing is the popup's notice, updates are the Updates widget.
