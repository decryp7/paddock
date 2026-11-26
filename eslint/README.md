# @decryp7/paddock-eslint

The ESLint flat-config preset of the Paddock apps (plan X11): rules 27 to 39 as rules, on top of ESLint, typescript-eslint
and angular-eslint (peer dependencies). It replaces the four near-identical 27-line `app/eslint.config.js` copies and
Debrief's `lint-effects.mjs` / `lint-styles.mjs`.

```js
// app/eslint.config.js
const { defineConfig } = require('eslint/config');
const { preset } = require('@decryp7/paddock-eslint');

module.exports = defineConfig(...preset({ prefix: 'pc' }));
```

Options: `prefix` (required), `ignores`, `rules` (extra for `*.ts`), `templateRules` (extra for `*.html`; a reasoned override such
as Debrief's two accessibility rules), `dialogStores` (globs of the stores a dialog shows: their `onInit` loads must be tracked, `paddock/dialog-load-tracked`), `storeFlagsAllowedIn` (regex of the file that defines `withRequestStatus`), `signalForms`
(rule 37: forbid new reactive forms; off while existing forms stay).

| Rule | Rule no. | What |
|---|---|---|
| `paddock/onpush-explicit` | 27 | `changeDetection: ChangeDetectionStrategy.OnPush` is written out; `Eager`/`Default` needs a comment |
| `paddock/no-zone` | 27, 38 | no `zone.js`, no `provideZoneChangeDetection`, no `fakeAsync`/`tick`/`waitForAsync` |
| `paddock/effect-state` | 28 | an effect that writes state has an `// effect-state: <reason>` comment above it |
| `paddock/signal-read-before-await` | 29 | `this.<signal>()` after the first `await` (class fields from `signal`, `computed`, `input`, ...); `// after-await:` marks a race guard |
| `paddock/store-rules` | 30, 31, 32 | no `loading`/`error` flags in `withState`; `signalStore` in `*.store.ts`, `protectedState` on; `providedIn: 'root'` needs `// app-wide:`; at most two injected stores |
| `no-restricted-imports` (preset) | 31 | `patchState` only in `*.store.ts`, shared store features and specs |
| `paddock/no-raw-style-values` | design guide | no hard-coded colours, font families or sizes in component `styles` and `*-styles.ts` |
| `paddock/no-backend-call-in-loop` | PRINCIPLES, Bulk APIs | a backend, port or `invoke` call per item: in a loop body, a `.forEach` callback, or a `.map` joined by `Promise.all`/`forkJoin`; `// per-item: <reason>` opts out. Report-only (see below) |
| `paddock/template-no-ng-class` | 33 | `[class.x]` instead of `[ngClass]` |
| `paddock/template-no-inner-html` | security | no `[innerHTML]` / `[outerHTML]` binding |
| angular-eslint (preset) | 33 | `prefer-inject`, `prefer-signals`, `prefer-signal-model`, `template/prefer-control-flow`, `template/prefer-class-binding` |

Not lintable (kept as review rules or other checks): 34 (window bootstrap shape), 35 (lazy panels: `paddock-conformance window-weight`
measures the initial chunk), 36 (prefer `resource`), 38 (each lifecycle effect has a failing test), 39 (shared code in Paddock). The
"unknown `var(--x)`" half of Debrief's `lint-styles` needs the app's token list and stays an app script. Rule 29 is a class
based heuristic; reads through injected stores are not seen.

Tests: `npm test` (RuleTester valid and invalid cases per rule, and the preset run through ESLint on temp files). Needs
`npm install` for the peer dependencies as dev dependencies.

## `e2ePinnedClock`: the spec folders

`...e2ePinnedClock()` (a flat-config block for `e2e/**/*.ts` except `e2e/test.ts`) turns on the rule `paddock/e2e-pinned-clock`, which refuses what would give a spec an unpinned page: a value, default, namespace or subpath import or a re-export of `test` from `@playwright/test` or `@decryp7/paddock-e2e` (`import type` is allowed), a dynamic `import()` or `require()` of them (also by template literal or variable), `clock.install` without a literal `{ time }`, `.newContext()` on any receiver and `browser.newPage()`. A spec imports `test` and `expect` from the app's `e2e/test.ts`, which wraps `pinnedClockTest`, and makes a context of its own with `newPinnedContext`. It is a rule of its own, not `no-restricted-*` options, because flat-config rule options do not merge: an app's own `no-restricted-syntax` in the same files would replace them. Put `e2e/**/*.ts` in the app's `lintFilePatterns`; `test/wiring.test.cjs` checks that and that an app's other e2e rules still fire. `paddock/e2e-no-control-or-meta` (an error; `controlOrMeta` option lowers it) refuses `ControlOrMeta` in any string or template of a spec: the key follows the machine the tests run on, the app follows the page, and WebKit on Linux is a Mac page (docs/lessons.md 2026-10-29 and 2026-11-26); use `pressMod` or `pageModKey` from `@decryp7/paddock-e2e`. What a linter cannot say (a reason on every opt-out, a wait over an empty set) is left to review (conformance C133 was dropped on 2026-11-25).

## `bulkCalls()`: one backend call for all items

`...bulkCalls()` (src/bulk.cjs; in `preset()` already, and in every app's eslint.config.js) turns on `paddock/no-backend-call-in-loop`
for `src/**/*.ts` and `projects/**/*.ts`, without specs and mocks. The receiver is known by name (`backend`, `*Backend`, `port`, `*Port`,
`ipc`, `invoke`, a `typeof invoke` parameter, and anything made by `inject(<...Backend|...BACKEND|...Port|...PORT>)`); calls through a
helper function or a store, and a component per row that calls in its own effect, are not seen: the counting seams cover those at run
time (`expectBackendCalls` of `@decryp7/paddock-ui/ipc/testing`, `paddock_proc::assert_starts`). Report-only (ER-41): off until its
first green run on the real tree, which is recorded in src/bulk.cjs; read the findings with `PADDOCK_BULK_LINT=warn npx eslint src`
(the ui: `projects`). `test/wiring.test.cjs` checks for every app and the ui that a planted loop gets the rule when it is on and not when
it is off.
