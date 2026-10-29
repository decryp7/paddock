# 13. Paddock workbench: the docked IDE layer on the app shell

- Status: Accepted (2026-10-24; user: the full shell is built first and Pitwall then adopts it; decisions D1–D6 as recommended)
- Date: 2026-10-24
- Related: ADR 0012 (the app shell, which this builds on), ADRs 0002 (declarative views), 0004 (`paddock-pitwall`), 0009,
  0010, 0011 (layout by declared constraints); rules 17, 18, 20, 34, 35, 39, 40; Debrief ADRs 0050, 0056, 0057, 0059;
  Pacenotes `docs/architecture-direction.md`; Sighter `docs/proposal.md` sections 5 and 11; plan:
  [docs/plans/workbench.md](../plans/workbench.md)
- Users: Debrief (moves onto it), Pacenotes (first new app built on it), Sighter; Dyno later (see Consequences)

## Context

Debrief, Pacenotes and Sighter are the same kind of program: a tree of things on the left, several documents and tool
windows arranged by the user, a status bar that reports long work, a command palette, an AI assistant beside the work,
and windows that pop out onto another display. Debrief has built all of it (dock 5,680 lines of TypeScript, pop-outs
2,514, activity bar and side bar, palette, the AI chat 1,937, the layout planner of ADR 0056, pop-out windows 913 lines
of Rust plus the drag ghost 1,256) and learned the hard parts the hard way: the layout squeeze series that became ADR
0056, the multi-display placement bug, the per-window bootstraps of rule 34. Pacenotes (a code browser: tree, code
viewer, graph, search, assistant) and Sighter (a studio: recordings, frame canvas, review queue, inspector) would
otherwise rebuild it, or depend on Debrief.

The decided order is: Debrief 1.1, then the Pitwall release (which builds the app shell of ADR 0012), then Pacenotes,
which starts by extracting Debrief's workbench into Paddock and moving Debrief onto it, then Sighter.

## Why a second ADR and not one

The shell (ADR 0012) and the workbench differ in who uses them (seven apps against three), when they are built (the
Pitwall release against Pacenotes' start), what they depend on (the workbench adds dockview, pop-out windows and the
Pitwall assistant client) and how fast they change (the workbench will move with Pacenotes and Sighter for a while; the
shell should settle). One ADR would tie the manager apps to the workbench's churn and make the Pitwall release wait for
decisions that only Pacenotes needs. They share one contribution model (ADR 0012 section 2), which this ADR extends.

## Decision

### 1. What the workbench is

A layer on the app shell, delivered as `@decryp7/paddock-ui/workbench/*` entry points and two Rust crates, that gives an
app a VS Code-like workspace without a plugin runtime:

| Part | The workbench provides | Taken from Debrief |
|---|---|---|
| Frame | `pd-workbench`: title bar, activity bar, side bar, dock stage, status bar, regions (F6), welcome slot, narrow-window rules | `app.ts`, `shell/regions.ts`, `shell/narrow-window.ts` |
| Activity bar | contributed items (view, command or fly-out menu), badges, pulse, overflow into More, bottom section | `shell/activity-bar.ts`, `activity-overflow.ts`, `extensions/activity-items.ts` |
| Explorer / side bar | a side bar with sections from **Explorer providers** (lazy trees with middle-truncated names, context menus, drag sources), the sidebar's open/close decided by `planLayout` | the frame of `shell/sidebar.ts`, `explorer-names.ts`, `sidebar-sections.ts` |
| Dock | dockview host, the `planLayout` adapter (every group gets `setConstraints` from its views), auto-hide edges, floating groups, drag-out to a window, perspectives and named layouts, layout persistence (versioned, fitted on load with one Undo notification) | `dock/` (39 files), `layout-plan/` |
| Pop-out windows | a panel moved to its own OS window (own bootstrap, rule 34), shared UI state by events, the drag ghost, placement on any display clamped to a work area | `popout/`, `panel_windows.rs`, `drag_ghost/`, `ui_dispatch.rs` |
| Command palette | palette over contributed commands and views, fuzzy matching, recent, "did you mean", **palette providers** (go to file, go to symbol, search everything) | `palette/`, `extensions/fuzzy.ts`, the frame of `search-everything/` |
| Jobs pane | the shell's jobs list as a `status`-area view | `jobs/jobs-panel.ts` |
| AI assistant pane | chat UI (streamed text bound as text, never HTML; citations as links; Stop; "needs local AI" state), conversation history, context chips; engine = Pitwall through `paddock-pitwall` | `ai/chat/` frame, `debrief_ai::llm::prompted` |
| Multi-window | several workspace windows of one app, window titles, close prompts generic over "unsaved" | the generic half of `windowing/` |

What stays per app: the views themselves (Debrief's grid and timeline, Pacenotes' code viewer and graph, Sighter's frame
canvas), the Explorer providers' data, the engine, the assistant's tools, and every domain type. The workbench imports
nothing from an app.

### 2. Contributions added by the workbench

The ADR 0012 model gains these kinds, with the same rules (typed `define*`, static data plus defensive functions over
the typed context, generated manifest, one lint):

```ts
provideContributions<PacenotesContext>({
  views: [defineView({
    id: 'pn.code', title: $localize`:@@pn.view.code:Code`, icon: 'pn:code', instances: true,
    layout: { area: 'document', min: { width: 480, height: 240 }, pref: { width: 900, height: 600 }, priority: 100 },
    component: () => import('./code/code-view'), category: 'core',
  })],
  activityItems: [defineActivityItem({ id: 'pn.explorer', icon: 'pd:files', order: 100, run: { explorer: 'pn.files' } })],
  explorerProviders: [defineExplorerProvider({ id: 'pn.files', title: ..., roots: (ctx) => ctx.index.roots(), children: (node) => ..., open: (node, ctx) => ... })],
  paletteProviders: [definePaletteProvider({ id: 'pn.symbols', prefix: '@', search: (q, signal) => ... })],
  perspectives: [definePerspective({ id: 'pn.review', title: ..., views: ['pn.diff', 'pn.findings', 'pd.assistant'] })],
  assistantContext: [defineAssistantContext({ id: 'pn.selection', label: ..., value: (ctx) => ctx.selection() })],
});
```

- **Views declare layout** (ADR 0011): area, min, pref, priority; no view without it (the lint). Plugin views of ADR 0002
  (`ViewDoc`s from out-of-process plugins) are hosted by a generic `pd-view` view with conservative area defaults.
- **Explorer providers** are lazy and paged (rule: large results are paged), cancellable, and return display data only;
  the engine decides what the tree contains.
- **Palette providers** take an `AbortSignal`, return at most a capped page, and call the engine for matching (rule 20:
  search finds identifiers however they are written is an engine operation).
- **Assistant tools are Rust**, not TypeScript: an app registers `impl AssistantTool` (name, JSON schema, read-only flag,
  budget, `call`) with the workbench builder; the UI contributes only context chips. Tools that write are never offered
  to the model without a click (the Pacenotes and Sighter safety rules).

### 3. Rust side

| Crate | Content | Taken from |
|---|---|---|
| `paddock-panels` | pop-out panel windows (create, place on any display, clamp to a work area, close with host), the drag ghost window, UI-state forwarding between a host and its pop-outs and dialogs | Debrief `panel_windows.rs`, `drag_ghost/`, `ui_dispatch.rs` |
| `paddock-assistant` | the `LlmProvider` port, the prompted tool protocol (Hermes `<tool_call>`, think-block filter, JSON-schema mode, image limits), the tool loop with budgets and cancel, conversation persistence (versioned), and the Pitwall adapter on `paddock-pitwall` (ADR 0004: discovery, pairing, priorities, typed errors) | `debrief_ai::llm::prompted`, `debrief_ai::engine` (Pitwall half), Debrief chat persistence |
| `paddock-workbench` | the Tauri plugin (`plugin:paddock-workbench|...`): panel and assistant commands and events (`paddock://panels`, `paddock://assistant`), built with the `paddock-commands` macro like the shell plugin | new (thin) |

Engine choice is the app's: an app may register another `LlmProvider` behind the same port (Debrief keeps its built-in
llama.cpp engine as its own adapter and its selection rules of Debrief ADR 0059); the assistant pane does not know which
engine answers. Pacenotes and Sighter use Pitwall only (Pitwall is optional: without it the pane shows the "needs local
AI" state of ADR 0004).

### 4. Theming, i18n, testing, versioning

As ADR 0012 sections 5-8, plus:

- Dock chrome (tabs, sashes, drop guides, floating groups) styled from `--pd-*` tokens only; Debrief's dockview theme moves
  into the package.
- Workbench messages use `@@pd.wb.` ids.
- `@decryp7/paddock-e2e` gains page objects for the dock (open, move, float, auto-hide, pop out), the palette, the
  Explorer and the assistant, and the scenario-table runner of ADR 0011 (each pane at or above its declared minimum at
  1440x900 and 1920x1080, a resize case, a saved layout that does not fit).
- `workbench-demo` in the Paddock repo exercises every workbench contribution kind against the mock backend.
- The workbench entry points and crates version with the repo tag like the shell; dockview is a peer dependency pinned to
  one major (`^8`), upgraded by a Paddock release, never by an app alone.

## Consequences

- Pacenotes and Sighter start with a working workspace on day one and spend their time on their own views; Debrief
  becomes the workbench's second consumer instead of its only copy.
- Debrief's migration is large (about 30,000 lines of TypeScript move or change imports); it is done with no behaviour
  change against a screenshot and e2e baseline taken first (plan, work package W0).
- The contribution model replaces Debrief's `DebriefView`/`DebriefCommand`/`DebriefActivityItem` and its plugin UI
  contract; the Debrief plugin API version for views bumps (its `layout` fields stay as they are).
- **Dyno** starts on the shell's pages layout. It moves to the workbench when its dashboard tiles or its task manager
  become views the user arranges side by side (at least three views wanted at once, or pop-out of a chart to another
  display); the tiles already declare min/pref sizes for `planLayout`, so the move is a frame swap, not a redesign.
- Pitwall, Pitcrew and Spotter stay on the pages layout; none is expected to move.

## Alternatives considered

- **Eclipse Theia or OpenSumi as the workbench.** Complete IDE frameworks, but React/InversifyJS (not Angular), large,
  built for text editors and an extension host; Theia's core is EPL-2.0. Rejected: wrong framework, wrong weight.
- **The VS Code workbench (code-oss) or Monaco's workbench services.** Not separable from VS Code's DI and extension host;
  rejected. Monaco itself remains an option for a Pacenotes code view (an app view, not the workbench).
- **A plugin runtime with web contributions** (VS Code extension host style). ADR 0002 forbids plugin web code (CSP, no
  `innerHTML`, theming, accessibility); in-process TypeScript contributions from the app plus `ViewDoc`s from plugins
  cover the need.
- **Another docking library** (Golden Layout, FlexLayout, a CSS-grid shell). dockview is MIT, dependency-free, has
  floating groups, pop-out support and `setConstraints` that ADR 0011 maps onto; Debrief's investment is in it.
- **Pacenotes and Sighter depend on Debrief's code.** Couples three release trains to Debrief and violates rule 39.
- **Build the workbench before Pacenotes needs it.** Rule 1 (one app at a time) and YAGNI: Pacenotes' first screens are
  the second consumer that proves the contribution model.

## Amendment (2026-10-29): the palette on the pages frame

`workbench/palette` (the store, `<pd-command-palette />`, the matcher and `providePalette`) is not only the workbench
frame's: an app on the pages frame (ADR 0012) adopts it with `providePagesPalette()` and places `<pd-command-palette />`
beside `<pd-shell-frame />`. The first user is Dyno 1.0 (Mod+P goes to a page, Mod+Shift+P runs a command).

- **Dependency direction.** A pages-frame app, and `workbench/palette` itself, now depend on each other's neighbours:
  the app imports `workbench/palette` (and, through it, the workbench's contribution kinds and `WorkbenchRegistry`), and
  `workbench/palette` imports `shell/pages` for the "Go to" provider. The workbench frame, the dock and the Explorer are
  not pulled in. This is accepted for 1.0 because the palette core has no dock dependency (it reaches the workbench only
  through `WORKBENCH_PORTS`, whose default degrades with a warning).
- **Later.** Moving the palette core to `shell/palette` (the store, the component, the matcher, the provider kinds) makes
  the dependency point the right way (workbench and pages frames both on the shell). It is a mechanical move with no
  behaviour change; do it when a second pages-frame app adopts the palette or before Paddock's v1, whichever comes first.
- **Options.** `providePagesPalette({ goToPages })`: by default the pages are listed on plain text (Mod+P); `{ prefix }`
  puts them behind a symbol so an app's own plain-text provider can keep the plain text; `false` leaves them out.
- **Menus.** The palette's two commands are items of the View menu with their keys, so the native macOS menu bar
  carries them (the native menu is built from the same model).
