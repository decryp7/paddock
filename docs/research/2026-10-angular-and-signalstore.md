# Angular 22 and NgRx SignalStore: best practices, Debrief audit, framework comparison

Research date: 2026-10-23. Audited: Debrief at commit `e54a802a` (app/), Paddock at `61da6ac`.
Status: research. The rules in section 3 are **proposed, needs user approval**; they are deliberately not in `rules.md`.

Evidence legend: **[P]** primary source (angular.dev, ngrx changelog, GitHub release notes), **[S]** secondary (blogs,
aggregators), **[?]** unconfirmed or not verifiable with the tools available. Fetch limits: several angular.dev/ngrx.io
guide pages returned only navigation or partial text, so claims that depend on them are marked. Opinion is marked
"Opinion"; everything else is fact from the cited source or from the audit (file:line).

---

## 1. Best practice summary (as of October 2026)

### 1.1 Versions and what changed recently

- Angular 22 was released 2026-06-03 [S: https://developer.mescius.com/blogs/whats-new-in-angular-22 , https://blogs.apievangelist.com/blogs/angular-2026-06-03-announcing-angular-v22/ ; official event page https://angular.dev/events/v22 (fetched 2026-10-23) confirms v22 as "a stabilization release"]. The apps are on `^22.2.x` (package.json), so they are on the latest stable major. The official blog post itself could not be fetched [?] (403); the facts below come from the GitHub release notes.
- Stable in v22 [P: https://angular.dev/events/v22]: Signal Forms, asynchronous signals (`resource` family), Angular Aria.
- Breaking changes in 22.0.0 [P: https://github.com/angular/angular/releases/tag/v22.0.0]:
  - Components with no `changeDetection` are now **OnPush by default**; `ChangeDetectionStrategy.Eager` restores the old behaviour (a migration adds `Eager` where needed).
  - `FetchBackend` is the default `HttpBackend`; `withFetch` is deprecated. Upload progress needs `provideHttpClient(withXhr)`.
  - TypeScript < 6.0 unsupported; `ComponentFactoryResolver` and `ChangeDetectorRef.checkNoChanges` removed; `TestBed.getFixture` renamed `getLastFixture`.
  - Stream resources (`rxResource`) resolve synchronously when the observable emits synchronously, which can break tests that expected async resolution.
  - Router: `paramsInheritanceStrategy` defaults to `'always'`; `provideRoutes()` removed.
- New, experimental or preview [P release notes; stability level not stated there, treat as unconfirmed]: `@Service` decorator, `injectAsync` helper, `provideWebMcpTools`. Do not adopt until documented as stable.
- NgRx: `@ngrx/signals` 22.0.0 on 2026-08-24, 22.0.1 on 2026-09-10 [P: https://raw.githubusercontent.com/ngrx/platform/main/CHANGELOG.md, fetched 2026-10-23]. Relevant changes: 22.0.0-beta.0 added resource extensions, dynamic deep signals, and a **breaking** change (union state slices with object members now yield a `DeepSignal` per member); 21.1.1 added `delegatedSignal`; 21.1.0 deprecated calling `rxMethod`/`signalMethod` outside an injection context; 21.0.0 renamed `withEffects` to `withEventHandlers` in the events plugin (a plugin that, per the v20 announcement, was released experimental [S: https://dev.to/ngrx/announcing-ngrx-v20-the-power-of-events-enhanced-dx-and-a-mature-signalstore-2fdm]; current stability [?]).

### 1.2 Change detection: zoneless and OnPush

- Zoneless is the default since v21; nothing to configure, and `provideZoneChangeDetection` must not override it [P: https://angular.dev/guide/zoneless]. Remove `zone.js` from polyfills and uninstall it. The page does not label zoneless "stable" explicitly [?], but it is the documented default.
- Change detection is scheduled by: `markForCheck` (the `async` pipe does it), `ComponentRef.setInput`, a signal read in a template changing, bound listeners, attaching a dirty view [P zoneless]. OnPush is recommended; with v22 it is the default.
- Reactive forms do not schedule change detection on value updates; expose state via signals or call `markForCheck` [P zoneless].
- Replace `NgZone.onStable` style code with `afterNextRender`/`afterEveryRender`; `NgZone.run`/`runOutsideAngular` may stay [P zoneless].
- Pitfall: components that assign fields inside `subscribe` callbacks freeze under OnPush; use signals or `toSignal` [S: GitHub migration PRs surfaced by search, e.g. https://github.com/spring-petclinic/spring-petclinic-angular/pull/216].
- Testing: `TestBed` is zoneless by default; prefer `await fixture.whenStable()`; `ExpressionChangedAfterItHasBeenChecked` is thrown when state changes without notification; `provideCheckNoChangesConfig({exhaustive: true})` finds such bindings [P zoneless].

### 1.3 Signals

- `effect` is for side effects on non-reactive APIs (DOM, logging, storage, charts). To react to state, prefer derivations (`computed`, `linkedSignal`, `resource`) [P: https://angular.dev/guide/signals]. Reads after an `await` in an async effect are untracked; read signals before the `await`. Use `untracked()` for incidental reads or calls into code that may read signals.
- Equality: default `Object.is`; a custom `equal` can suppress structurally equal updates; computed values are lazy and memoised; dependencies are dynamic [P guide/signals].
- `linkedSignal`: writable state derived from a source; pitfalls: it silently resets when the source changes (use `previous` to keep a still-valid choice), generics are required when using `previous`, manual sets are overwritten on the next source change, partial `equal` hides changes, `rawSet` can desync [P: https://angular.dev/guide/signals/linked-signal].
- `resource`/`httpResource`/`rxResource`: `params` is reactive; `undefined` params gives `idle`; the loader gets `abortSignal` (pass it to `fetch`); `value()` throws in the error state, guard with `hasValue()`; `chain` for dependent resources; statuses idle/loading/reloading/resolved/local/error [P: https://angular.dev/guide/signals/resource]. Use for read-style loading; commands and mutations stay as methods.
- Inputs, outputs, `model()` and signal queries are the current component API; decorators `@Input/@Output/@ViewChild/@HostListener` are the legacy style [P style guide: https://angular.dev/style-guide fetched 2026-10-23: group Angular members at the top, `protected` for template-only members, `readonly` on `input/model/output/queries`].

### 1.4 Templates, standalone, control flow, defer

- Standalone components are the norm (NgModules are not needed). Built-in `@if/@for/@switch` replace `*ngIf/*ngFor`; `@for` requires `track` [P style guide and v17+ docs; track requirement is a compile-time rule]. Style guide: prefer `[class.x]`/`[style.x]` over `ngClass/ngStyle`; name handlers after the action; keep template logic simple, move to `computed` [P style guide].
- `@defer` and lazy routes are the documented levers for initial bundle size; image optimisation via `NgOptimizedImage` [P: https://angular.dev/best-practices/runtime-performance links these subpages; their text was not retrievable, so details [?]].
- Virtual scrolling: `@angular/cdk/scrolling` is the supported route for long lists [S/general knowledge; no primary page fetched].

### 1.5 Dependency injection

- Prefer `inject()` over constructor injection [P style guide]. Provide at the narrowest scope that fits (component/route/environment injector); `providedIn: 'root'` only for truly app-wide singletons. Use `DestroyRef`/`takeUntilDestroyed` for cleanup of subscriptions. `@Service`/`injectAsync` (lazy DI) are new and unconfirmed as stable (see 1.1).

### 1.6 Forms

- Signal Forms are stable in v22 [P: https://angular.dev/events/v22], need v21+, and are positioned for new signal-based apps; "reactive forms remain a solid choice" for existing apps [P: https://angular.dev/guide/forms/signals/overview]. Release notes: `min/max` rules accept only numbers or null; `reloadValidation` and `FieldState.getError()` added [P release notes]. Migration guides exist but their text was not retrievable [?].

### 1.7 Routing

- Router features are function-based: guards, resolvers, `canMatch` (now requires `currentSnapshot`), `withComponentInputBinding`, lazy `loadComponent`/`loadChildren`. v22 changes `paramsInheritanceStrategy` to `'always'` [P release notes]. Debrief does not use the router (see audit).

### 1.8 SSR and hydration

- Not used by the Paddock apps (Tauri webview, static bundle). Only relevance: `resource` `id` and `PendingTasks` are SSR-only concerns; ignore.

### 1.9 Testing

- Vitest is the default runner for new CLI projects (jsdom or happy-dom; browser mode via Playwright/WebdriverIO); Karma still supported with a migration guide; the page does not call Vitest "stable" [P: https://angular.dev/guide/testing]. Component harnesses (CDK) are the recommended way to test Material/CDK UI [S/general; page content not retrievable [?]].
- TestBed is zoneless (see 1.2). Playwright for end-to-end (documented as an option; the project already uses it).

### 1.10 Accessibility, i18n, security

- Angular Aria is stable in v22 [P event page]; CDK a11y (focus trap, live announcer, FocusMonitor) remains the toolkit. [Detailed content not fetched.]
- Template sanitisation is on by default; never bind untrusted HTML with `[innerHTML]` plus `bypassSecurityTrust*`. Trusted Types and strict CSP are supported by the Angular compiler/CLI options [general knowledge; not fetched [?]]. In Tauri, CSP nonces make the webview ignore `'unsafe-inline'`, which is why `inlineCritical:false` is in `docs/lessons.md`.
- i18n: `$localize` runtime translations (Debrief loads locale files at runtime, see audit).

### 1.11 Multi-window apps in Tauri

- Tauri IPC: commands (JSON-RPC like request/response) and events (fire-and-forget, either direction), gated by permissions [P: https://v2.tauri.app/concept/inter-process-communication/]. Targeted emit/listen scoping per window was not covered on that page [?] (check "Calling the Frontend from Rust").
- Each Tauri window is a separate webview with its own JS heap, so each window is a separate Angular bootstrap with separate DI singletons and stores. State cannot be shared by reference; it must be shared through the Rust backend (source of truth) plus events (state-changed notifications), with each window holding a projection. Debrief already works this way (pop-out windows boot a small shell via `#/popout` and `UiSync`, bootstrap.ts:12-19). Opinion: this is the correct shape for dialogs-as-windows; do not try to synchronise SignalStores window to window (see 1.12 and the audit).

### 1.12 SignalStore design

Source for the rest of this section: NgRx SignalStore guide text [P via raw markdown, fetched 2026-10-23: https://raw.githubusercontent.com/ngrx/platform/main/projects/www/src/app/pages/guide/signals/signal-store/index.md]. Other ngrx.io pages returned navigation only [?].

- `signalStore(...)` composes features; a store is **not registered anywhere by default**; provide it in a component, route or root (`providedIn: 'root'`) [P]. Feature-scoped stores (component/route level) are the default recommendation; root for app-wide singletons. Opinion: dependency injection scope is the main lever for lifetime and memory.
- `withState` gives a signal per slice, and **lazy deep signals** for nested properties; state is **protected** from outside writes by default (`protectedState: false` is discouraged); changes go through `patchState` [P]. The 22.0 beta changed unions of object literals to per-member `DeepSignal`s (breaking) [P changelog].
- `withComputed` for derivations, `withMethods` for updates and side effects (promise code directly; `rxMethod` for RxJS), `withProps` for static values, observables and dependencies, `withHooks` for init/destroy [P]. `withLinkedState` is not covered on the retrieved page [?]; the API exists in recent versions (3 stores already use `withProps`; none use `withLinkedState`).
- Custom features via `signalStoreFeature` for reuse (loading/error/pagination), `withEntities` for collections, `rxMethod`/`signalMethod` for reactive method inputs; 21.1.0 deprecated calling them outside an injection context [P changelog].
- Anti-patterns (Opinion, consistent with the above sources): writing state from outside the store, effects copying one signal into another instead of `computed`/`linkedSignal`, storing derived data in state, `loading: boolean` + `error: string|null` flags that can disagree (a status union is safer), a store that hand-rolls what `resource` or `withEntities` already do, and stores calling many other stores (coupling).
- Testing: stores are plain injectables; test with `TestBed.inject` or `runInInjectionContext`. The official testing page was not retrievable [?].
- Sharing across windows: not a SignalStore feature (see 1.11). The NgRx events plugin is in-process and does not cross webviews.

---

## 2. Debrief audit (commit `e54a802a`, app/, read-only)

Scale: 918 non-spec TS files, 117,186 lines (3,457 of them `core/api.types.ts`; several thousand are mock backends); 424 spec files, 76,144 lines; 262 components; 66 `signalStore` files; Angular/CDK/Material/localize `^22.2`, `@ngrx/signals ^22.0.1`, vitest 5, TypeScript 6, Playwright and WebdriverIO (package.json:1-62).

### What is already right (keep)

- Zoneless: no `zone.js` in package.json, angular.json or polyfills; no `provideZoneChangeDetection`. (angular.json has no polyfills for zone; app.config.ts uses `provideBrowserGlobalErrorListeners`.)
- OnPush on 262 of 262 components (explicit); 0 NgModules; 0 `*ngIf/*ngFor`; 0 `templateUrl`; 0 decorator-style `@Input/@Output/@ViewChild/@HostListener`; 241 signal `input()/model()/output()` uses; 517 files use `inject()`.
- Only 1 file imports rxjs; `subscribe(` 18 times; `toSignal/toObservable` 5 times (no RxJS-first state).
- No `[innerHTML]` binding anywhere (comments at `markers/glyph-model.ts:115`, `ai/chat/rich-text.ts:7`, `updates/updates-dialog.ts:28` document it).
- Tests: `@angular/build:unit-test` with the vitest runner (angular.json:107-113), shared `test-setup.ts` that resets Tauri stubs per spec; 23 store specs, 94 TestBed/injection-context uses in them; Playwright e2e directory plus WDIO Tauri smoke tests.
- Stores: 66 files, 62 `providedIn: 'root'`, `patchState` almost always inside the store; entry-bundle split (`main.ts` loads translations, then `bootstrap.ts` lazily imports the shell or pop-out shell, bootstrap.ts:12-19).
- Budgets exist: initial 1.5MB warn / 3MB error, anyComponentStyle 16kB/32kB (angular.json:65-76).

### Findings

**P1**

1. **165 `effect()` calls, many syncing state to state (effort: M, 3-5 days, incremental).** The guide says effects are for non-reactive side effects and recommends derivations otherwise. Examples where an effect writes a signal or republishes derived data: `grid/events-grid.ts:426` (reset position when query/sort/data changes), `:456-461` (anchor to jump), `:464-476` (drop a multi-selection that no longer matches, `selection.set(...)`), `:478-483` (publish selected records to another store). The `untracked(() => ...)` wrapping at 156 call sites is the tell: it manually restores what `linkedSignal`/`computed` give for free, and effect ordering is a source of flakiness (stale or double loads). Highest density: `events-grid.ts` 6, `dump-view.ts` 5, `umdh-view.ts` 4, `dock-policy.ts` 4. Fix: convert each to `computed`/`linkedSignal` (selection reset = `linkedSignal` keyed on `sel.selected()`), keep effects only for DOM/IPC side effects. Why P1: it is the main source of hidden reactive coupling and the main risk when state moves behind a window boundary.
2. **`loading: boolean` + `error: string | null` flags that can disagree (effort: M).** Seen in `settings/parsers/parsers.store.ts:29`, `settings/engine-settings.store.ts:13`, `settings/storage/storage.store.ts:11`, `plugins-panel/plugin-packages.store.ts:29,32`, `video/video.store.ts:79`, `skew/clock-skew.store.ts:22`, `settings/performance/plugin-memory.store.ts:10`. AGENTS.md says "make illegal states unrepresentable". Fix: one reusable `withRequestStatus` custom feature (`signalStoreFeature`, status union `idle | loading | loaded | error(message)`) in paddock-ui or a shared folder; 0 `signalStoreFeature` exist today, so each store re-implements loading/error. Rule of three is already met.
3. **Dialogs-as-windows will duplicate 62 root singleton stores per window (effort: L, design needed).** Each webview bootstraps its own `appConfig` with ~40 `provideX()` calls (app.config.ts:1-80). A small dialog window should not instantiate the workspace, timeline, AI and profiling providers. Pop-out already uses a smaller shell (bootstrap.ts:12-19), but it reuses the same `appConfig.providers`. Fix: a small `dialogConfig` per dialog window containing only what the dialog needs; state arrives from the engine via commands and from events for change notifications (section 4).

**P2**

4. **No router (effort: M if wanted, otherwise document as intentional).** `grep @angular/router` finds nothing outside specs. Views are dock panels plus `#/popout?...` parsed by hand (`popout/popout-route.ts`). For dialogs-as-windows a tiny route table (`/dialog/:kind`) with lazy `loadComponent` would give lazy loading and input binding (`withComponentInputBinding`) per window for free. Opinion: adopt the router only for window entry points, not the dock layout.
5. **Almost no `@defer` / lazy chunks inside the shell (effort: S-M).** One `@defer` in the whole app (`settings/settings-screen.ts:170`). The initial budget is 1.5MB warning, which is generous for a webview where startup time is user-visible. Candidates: AI, profiling, dumps, store-pane, settings tabs, video. Fix: measure the initial chunk first (`ng build --stats-json`), then `@defer (on idle)` or lazy `loadComponent` heavy panels; tighten the budget to the measured size plus ~10%.
6. **Oversized units (effort: M, split opportunistically).** `dock/dock.service.ts` 832 lines, `grid/events-grid.ts` 710 lines (component with six effects), `ai/setup/setup-wizard.ts` 590, `ai/models/model-manager.ts` 534, `ai/runtimes/runtime-panel.ts` 504, `ai/chat/chat.store.ts` 329, `video/video.store.ts` 381. AGENTS.md: single responsibility. Split by job (layout persistence vs commands in `dock.service.ts`).
7. **Dead-end store patterns: `patchState` outside `*.store.ts` (effort: S).** `projects/bookmarks/highlight.ts:94-103` and `profiling/profile-view-state.ts:100-103` are stores defined in non-`.store.ts` files (naming drift, easy to miss when searching). Rename files to `*.store.ts`.
8. **Only 3 stores use `withProps`, none use `withLinkedState`, `withEntities`, `rxMethod`, `signalMethod` or `signalStoreFeature` (effort: S per store).** Not wrong (async/await in `withMethods` is documented) but `withLinkedState` is the intended replacement for several effect-based resets (see P1.1) and `withEntities` for keyed lists (e.g. `notifications.store.ts`, `jobs.store.ts`). Adopt where a store hand-rolls an id-keyed map.
9. **Request cancellation is manual.** No `resource`/`rxResource` use (0 hits). Paged and query loading are hand-rolled (`grid/paged-rows.store.ts`, 268 lines; `core/workspace/workspace-data.store.ts`, 225). Because the engine returns pages via Tauri `invoke`, `resource` with `abortSignal` helps only for UI-level staleness; Opinion: do not rewrite working paging code, use `resource` for new read-only loaders (settings, catalogs).

**P3**

10. **Constructor parameter injection remains for plain classes** (`settings/settings.store.ts:28`, `settings/parsers/parsers-backend.ts:53`, mock backends): these are not DI-created (default args, test seams), so fine; leave. (effort: none)
11. **Component harnesses: 0 uses** (`HarnessLoader`/`TestbedHarnessEnvironment`). With 28 files using CDK and 3 using Material, tests query the DOM directly. Harnesses would help only for Material/CDK-heavy widgets; add when a test turns brittle. (effort: S)
12. **Signal Forms not used** (1 file imports `@angular/forms`). Fine; adopt signal forms for the next new form instead of migrating. (effort: none now)
13. **Virtual scrolling**: only `home/home-view.ts` references CDK virtual scroll; the event grid uses its own windowing (`grid/events-grid.ts`, `paged-rows.store.ts`), which is justified by 10M-row pagination. No change.
14. **`markForCheck/detectChanges`: 5 non-spec uses** (acceptable; zoneless + signals cover the rest).
15. **Bundle budget `maximumWarning` for components 16kB** is fine; no per-lazy-chunk budget (effort: S, after P2.5).

### Other Paddock apps (versions from each `app/package.json`; counts are rough greps)

| App | Angular / @ngrx/signals | zone.js | Components (OnPush) | signalStore files | `effect(` |
|---|---|---|---|---|---|
| Debrief | ^22.2 / ^22.0.1 | no | 262 (262) | 66 | 165 |
| Dyno | ^22.2 / ^22.0.1 | no | 101 (92) | 30 | 10 |
| Pitcrew | ^22.2 / ^22.0.1 | no | 58 (56) | 15 | 18 |
| Pitwall | ^22.2 / ^22.0.1 | no | 56 (51) | 14 | 9 |
| Spotter | ^22.2 / ^22.0.1 | no | 49 (32) | 8 | 9 |
| paddock-ui (`paddock/ui/projects/paddock-ui`) | ^22.2 | no | 10 (9) | 0 | 0 |

- All apps are on the same majors and zoneless. Debrief is the outlier in effect density (0.6 effects/component vs 0.1-0.3 elsewhere); Pitcrew, Pitwall and Dyno import `paddock-ui` in 0 source files and Spotter in 13 (Debrief not counted), so the shared library is barely shared. Spotter (32/49) and Pitwall (51/56) have components without explicit OnPush, which in v22 means OnPush by default, so they are fine functionally but should be explicit or the lint rule should be switched to allow omission (decide once).
- paddock-ui contains 10 components and no stores: the natural home for `withRequestStatus` (P1.2) and a dialog-window bootstrap helper (P1.3).

---

## 3. Proposed rules (proposed, needs user approval; not added to rules.md)

Normative, short:

1. Zoneless only. No `zone.js` in any app, no `provideZoneChangeDetection`. Every component declares `changeDetection: ChangeDetectionStrategy.OnPush` explicitly (v22 defaults to it, but explicit survives a lint or a future default change); `Eager` requires a comment.
2. `effect()` is only for side effects on things outside the signal graph (DOM, IPC, storage, logging, charts). State derived from other state is `computed`, `linkedSignal` or `resource`. An effect that calls `.set()`/`patchState()` needs a one-line justification comment, and `untracked()` is allowed only around the side effect, not to hide a dependency.
3. Reads of signals in an async function or effect happen before the first `await`.
4. Request state is a status union (`idle | loading | loaded | error`) from one shared `withRequestStatus` feature, never separate `loading`/`error` flags.
5. A store is changed only through its own methods; `protectedState` stays on; no `patchState` outside the store file; store files are named `*.store.ts`.
6. Provide a store at the narrowest scope (route/component/dialog window); `providedIn: 'root'` only for app-wide singletons. A store injects at most two other stores; more means a missing facade or feature.
7. New code uses `inject()`, signal `input/model/output/viewChild`, `@if/@for/@switch` with `track`, standalone components, and `[class.x]` over `ngClass`.
8. Each OS window is its own Angular bootstrap with its own small provider set. The engine (Rust) is the single source of truth; windows share state by commands plus events and never by importing another window's store state. Dialog windows load only the providers they need.
9. Heavy, rarely used panels are lazy (`@defer` or `loadComponent`); every release records the initial chunk size and the budget is tightened to measured size plus 10%.
10. Prefer `resource`/`rxResource` for new read-only loading with cancellation; keep `withMethods` async for commands. Do not adopt `@Service`, `injectAsync` or the NgRx events plugin until documented stable.
11. New forms use Signal Forms; existing reactive forms stay until touched.
12. Tests: Vitest through the Angular unit-test builder, `await fixture.whenStable()`, store tests via `TestBed`; use CDK harnesses for Material/CDK widgets; each lifecycle effect gets a test that triggers the failure.
13. A shared store feature or component used by two apps lives in `paddock-ui`, not copied.

---

## 4. Debrief migration plan (ordered, small steps; fits dialogs-as-windows)

Each step is its own small merge, tested with targeted checks only (per the "test once after merges" practice).

1. **Measure first (S, 0.5 day).** `ng build --stats-json`; record the initial chunk, per-feature weight; add a `scripts/` check that logs sizes. Output feeds steps 5 and 8.
2. **`withRequestStatus` feature in paddock-ui (S, 1 day).** `signalStoreFeature` with `requestStatus` union, `setLoading/setLoaded/setError`, computed `isLoading/error`. Unit tests incl. error path.
3. **Dialog window bootstrap (M, 2-3 days; the keystone).** `dialogConfig` (logger, backend, theme, i18n, error logging) separate from `appConfig`; entry route `/dialog/:kind` using the router with lazy `loadComponent` and `withComponentInputBinding` (introduces the router only here). Dialog state is fetched by command and refreshed by backend events; no cross-window store sharing. Convert one dialog first (e.g. the import range dialog `import/import-range-dialog.ts`) as the pattern.
4. **Migrate the 8 boolean-flag stores to `withRequestStatus` (M, 2 days).** Start with the ones listed in finding 2. Mechanical (Sonnet agent), each store with its spec.
5. **Lazy-load heavy panels (S-M, 2 days).** From step 1 data: AI, profiling, dumps, store-pane, settings tabs. Tighten budgets.
6. **Effect audit, hot spots first (M, 3-5 days).** `events-grid.ts` (6), `dump-view.ts` (5), `umdh-view.ts` (4), `dock-policy.ts` (4); convert state-to-state effects to `computed`/`linkedSignal`/`withLinkedState`; keep effects that touch DOM/IPC. Add a lint rule (custom ESLint rule or `no-restricted-syntax` on `.set(` inside `effect`) so new ones are flagged.
7. **Rename `highlight.ts`/`profile-view-state.ts` to `*.store.ts`, split `dock.service.ts` (832 lines) by responsibility (S-M, 2 days).**
8. **Adopt `resource` for new read-only loaders and `withEntities` where a store hand-rolls an id map (S each, opportunistic).**
9. **Propagate to the other apps** only what is already in paddock-ui (status feature, dialog bootstrap) when each app next gets dialog windows; Spotter and Pitwall make `OnPush` explicit.

Not planned: signal-forms migration of existing forms, SSR/hydration, moving off the custom event grid windowing, and any `@Service`/`injectAsync` adoption (unconfirmed stable).

---

## 5. Comparison with other frameworks

Situation: Tauri 2 desktop webviews, large data views (virtualised grid, custom timeline, ECharts), many panes and windows, strict typing, long-lived code base, a small team plus AI agents. Existing code: ~117k lines non-test TS in Debrief alone, ~900 files, 262 components, 66 stores; five apps on the same stack.

### 5.1 Method and honesty about evidence

- Runtime benchmarks: the js-framework-benchmark (krausest) is the usual source. Its README says the snapshot page "may not have the same quality" (mixed browser versions, varying run counts), that scores use a weighted geometric mean since Chrome 118, that one full run takes ~12 hours on the author's machine, that it measures a fixed set of operations on a generated table, and that the latest archived Chrome is 152 [P: https://github.com/krausest/js-framework-benchmark, fetched 2026-10-23]. I could not retrieve the current per-framework numbers [?] (page is script-rendered), so no ranking numbers are quoted. General finding I can state without numbers (Opinion, widely repeated): fine-grained signal frameworks (Solid, Svelte 5, Vue Vapor) lead on raw table update throughput; Angular signals and React are in the same band as each other and behind them on synthetic runs; the gap is invisible for virtualised views where only ~50 rows are in the DOM. A critical view of the benchmark's relevance: https://www.pkgpulse.com/guides/problem-with-javascript-framework-benchmarks [S, opinion piece]. Paddock's grids are virtualised and data is paged from Rust, so DOM framework throughput is not the bottleneck; the engine is.
- Everything labelled "needs a third party" below is an assembly cost: each library adds a version matrix to upgrade.

### 5.2 Feature-set weight: "Angular comes with everything" (explicitly weighted, per user)

| Capability | Angular 22 (first party) | React 19+ | Vue 3 | Svelte 5 | Solid |
|---|---|---|---|---|---|
| Component model, templates, DI | built in | built in (no DI) | built in (provide/inject) | built in | built in |
| Router | `@angular/router` | third party (React Router/TanStack Router) | `vue-router` (official) | SvelteKit router (or third party) | `@solidjs/router` (official, separate) |
| Forms | reactive, template-driven, Signal Forms (stable v22) | third party (React Hook Form, TanStack Form) | third party (VeeValidate, FormKit) | third party / hand rolled | third party / hand rolled |
| HTTP | `HttpClient` + interceptors (fetch backend default v22) | `fetch` + TanStack Query | `fetch`/ofetch + library | `fetch` | `fetch` |
| State | signals; SignalStore is first-party NgRx | Zustand/Redux Toolkit/Jotai/TanStack Query (all third party) | Pinia (official) | runes/stores built in | signals/stores built in |
| i18n | `$localize` + extraction CLI | third party (react-intl/i18next) | `vue-i18n` (official-ish) | third party (paraglide etc.) | third party |
| UI primitives and a11y | CDK + Material + Aria (stable v22) | Radix/React Aria/Headless UI (third party) | Headless UI vue/Radix Vue/Reka | Bits UI/Melt (third party) | Kobalte/Corvu (third party) |
| Virtual scroll | CDK scrolling | TanStack Virtual (third party) | vue-virtual-scroller etc. | third party | TanStack Virtual (adapter) |
| Testing | Vitest builder, TestBed, harnesses | Vitest/Jest + Testing Library (third party) | Vitest + Vue Test Utils | Vitest + Testing Library | Vitest + Testing Library |
| CLI, build, lint, strict TS | Angular CLI, strict TS, angular-eslint, `ng update` migrations | Vite/Next/Expo; assembled | `create-vue` + Vite | `sv` + Vite | Vite |
| Upgrade story | one version number; `ng update` codemods; 6-month cadence | per-library | per-library | per-library | per-library |

(Facts in the Angular column are in section 1. Alternatives' columns are general knowledge about the ecosystems [?: not individually re-verified for October 2026].) Reading: for React, Vue-minus-Pinia, Svelte and Solid the count of separate third-party packages to reach Debrief's existing feature set (router, forms, i18n with extraction, UI primitives with a11y, virtual scroll, test utilities, state, HTTP/caching) is roughly 5-8, each with its own release cycle and maintainers. Opinion: with one maintainer team, that matrix is the real risk for a small team over many years; Angular's single version train with `ng update` is a genuine advantage that pure performance numbers do not show.

### 5.3 Per-alternative

- **React 19.x (Zustand/Redux Toolkit/TanStack).** React Compiler 1.0 stable 2025-10-07 and the React Foundation formed the same time [S: https://mn.react.dev/rss.xml surfaced via search; verify at react.dev]. Largest ecosystem and hiring pool, and the best AI-assistant familiarity (Opinion). Weaknesses for this case: no DI, no official router/forms/i18n, rendering model re-runs components (compiler mitigates), multi-window needs per-window stores like Angular. Upgrade risk is spread across many libraries.
- **Vue 3 (Pinia).** Vapor mode (no virtual DOM) is feature-complete in the 3.6 release candidates but **not stable**: 3.5.43 (2026-09-17) is "Latest", 3.6.0-rc.9 (2026-09-18) is pre-release [S: https://blog.openreplay.com/vue-vapor-mode/ and others; official releases page not fetched [?]; one sponsored blog claims it is stable, contradicted by all others]. Good DX, Pinia is official; smaller TypeScript guarantees than Angular's strict templates (Volar helps).
- **Svelte 5 (runes).** Runes (`$state/$derived/$effect/$props`) are stable since late 2024 [S: https://www.pkgpulse.com/guides/svelte-5-runes-complete-guide-2026]. Small bundles, fast, pleasant; satisfaction surveys favour it but the figures I found conflict and are unverified [?]. Ecosystem for dense desktop UI (docking, virtual grids, accessible primitives) is thinner; smaller AI-assistant training footprint (Opinion).
- **SolidJS.** Fine-grained signals, top synthetic numbers. Solid 2.0 is at release candidate (2.0.0-rc.14 seen), with API breaks from 1.x and SolidStart being retired in favour of "Start mode" [S: https://github.com/solidjs/solid/releases and https://socket.dev/npm/package/solid-js/overview/2.0.0-rc.14 , https://www.infoq.com/news/2026/05/solidjs-2-async/ ]. Smallest ecosystem here and an unstable major: poor fit for a 5-app long-lived suite right now.
- **Rust UI in the webview (Leptos, Dioxus, Yew).** Would collapse the language count to Rust (AGENTS.md already mandates Rust for the engine), compile-time safety end to end. Costs: wasm size and startup, tiny component and a11y ecosystem (no CDK equivalent, no docking/grid libraries), JS interop for ECharts/dockview, immature tooling. Dioxus is at 0.7.x with 0.8 alpha [S: https://github.com/DioxusLabs/dioxus/releases via search]; Leptos/Yew status not verified [?].
- **Rust-native UI (egui, iced, Slint, GPUI).** No webview, small memory, direct engine access (no IPC). Costs: accessibility (screen reader support varies and is the weakest point of immediate-mode toolkits), no CSS/ECharts/dockview, theming and i18n rebuilt, text rendering and IME differences, and AGENTS.md's design system (CSS tokens, Material/CDK, e2e Playwright) would be thrown away. GPUI/gpui-component is described by users as early stage [S: https://users.rust-lang.org/t/which-gui-framwork-should-i-choose/137581]. Slint and others were not verified in detail [?].

### 5.4 Cross-cutting

- **Bundle and startup in a webview.** Webviews load a local bundle, so network size matters little; parse/execute time and memory do. Angular's baseline runtime is larger than Svelte/Solid (Opinion, known characteristic; numbers not retrieved [?]). Mitigations already in place: runtime-loaded locale, lazy bootstrap chunk, splash page; remaining: `@defer` (finding P2.5). A tray popover is the one place where baseline size and first paint matter most.
- **TypeScript.** Angular compiles templates under strict type checking (strictTemplates) out of the box; React is TS-first but untyped JSX props are only as good as the types; Vue relies on Volar; Svelte has `svelte-check`. Opinion: Angular's template type checking is the strongest of these.
- **Stability and cadence.** Angular: 6-month majors with codemods; Debrief moved to 22 cleanly (typical). Vue 3.6 and Solid 2.0 are mid-flight; React has a long stable 19.x line; Svelte 5 is stable. NgRx follows Angular majors (22.0.0 on 2026-08-24, about 12 weeks after Angular 22; a lag worth planning for).
- **Ecosystem and AI familiarity.** React > Angular ~ Vue > Svelte > Solid for training-data volume and hiring (Opinion, not measured here). AI agents in this repo already work fluently in the Angular code (Debrief's spec/structure conventions), and a rewrite would reset that accumulated convention knowledge.
- **Multi-window.** No framework solves this: each webview is its own JS context. Angular bootstrap per window is straightforward; the pattern in 1.11 applies to every web framework. Rust-native toolkits would manage multiple windows in one process (e.g. shared state in memory), which is a real advantage in principle, but at the cost above.
- **Migration cost from Angular (order of magnitude, Opinion).** Debrief alone is ~117k non-test lines plus 76k lines of tests, 262 components, 66 stores, 4 other apps. Mock backends, API types and the whole Rust side port as-is, but the UI layer (approximately 70-80k lines) would be rewritten, including every test, dock integration (dockview is framework-agnostic), CDK/Material replacements and i18n pipeline. At the repository's observed throughput this is a multi-month effort for one app with a feature freeze, several multiples of the migration plan in section 4 (about 15 working days). No alternative gives a gain that is visible to users in a virtualised, engine-bound UI.

### 5.5 Recommendation (Opinion)

- **Stay on Angular 22 + SignalStore for all five apps.** Reasons: the full first-party stack is the dominant factor for a small team (one upgrade train, no assembly), the code is already modern (zoneless, 100% OnPush, signals, inject, no NgModules), performance is bound by the Rust engine and virtualisation rather than by DOM framework throughput, and migration cost is multi-month with no user-visible gain.
- **Where a different choice could make sense:** a tiny tray/menu-bar popover (Dyno, Pitcrew) where first paint and memory dominate: a Svelte 5 or plain TS/CSS page, or a smaller Angular dialog config (section 4 step 3) first. Trial only after measuring the Angular popover's startup and memory; if it misses budgets after `@defer` and a lean provider set, consider a Svelte popover. Opinion; no measurement here.
- **Conditions that would change this:** (a) Angular's cadence or breaking changes become a recurring cost the apps cannot absorb; (b) measured startup/memory of Angular dialog and popover windows fail agreed budgets after the lean-provider work; (c) Vue Vapor or Solid 2 become stable and a benchmark in Paddock's own data views (not synthetic) shows a user-visible gain; (d) a Rust-native toolkit reaches accessible, themable, docking-capable maturity (screen reader support, IME, i18n) and the single-language benefit outweighs rebuilding the design system; (e) AI assistants become measurably worse on Angular than on the alternative in this repository's own task logs.

---

## Sources (fetched 2026-10-23 unless noted)

- https://angular.dev/events/v22 (stable APIs list; no body on zoneless/testing)
- https://github.com/angular/angular/releases/tag/v22.0.0 (breaking changes and features)
- https://angular.dev/style-guide
- https://angular.dev/guide/signals ; https://angular.dev/guide/signals/linked-signal ; https://angular.dev/guide/signals/resource
- https://angular.dev/guide/zoneless ; https://angular.dev/guide/testing ; https://angular.dev/guide/forms/signals/overview ; https://angular.dev/best-practices/runtime-performance
- https://raw.githubusercontent.com/ngrx/platform/main/projects/www/src/app/pages/guide/signals/signal-store/index.md ; https://raw.githubusercontent.com/ngrx/platform/main/CHANGELOG.md
- https://v2.tauri.app/concept/inter-process-communication/
- https://github.com/krausest/js-framework-benchmark (README limits)
- Secondary: https://developer.mescius.com/blogs/whats-new-in-angular-22 , https://blogs.apievangelist.com/blogs/angular-2026-06-03-announcing-angular-v22/ , https://dev.to/ngrx/announcing-ngrx-v20-the-power-of-events-enhanced-dx-and-a-mature-signalstore-2fdm , https://blog.openreplay.com/vue-vapor-mode/ , https://github.com/solidjs/solid/releases , https://www.infoq.com/news/2026/05/solidjs-2-async/ , https://www.pkgpulse.com/guides/svelte-5-runes-complete-guide-2026 , https://www.pkgpulse.com/guides/problem-with-javascript-framework-benchmarks , https://github.com/DioxusLabs/dioxus/releases , https://users.rust-lang.org/t/which-gui-framwork-should-i-choose/137581 , https://mn.react.dev/rss.xml

## Unconfirmed items (summary)

Official Angular v22 blog text; stability labels for `@Service`/`injectAsync`/NgRx events plugin/`withLinkedState` docs; current js-framework-benchmark numbers; Vue 3.6 stable status on the official page; Svelte survey satisfaction figures; Leptos/Slint/egui/iced release status; Tauri per-window event targeting page; ngrx.io testing, entities, rxMethod and signalMethod guide pages; Angular CDK/Aria/i18n/security detail pages.
