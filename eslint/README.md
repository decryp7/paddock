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
as Debrief's two accessibility rules), `storeFlagsAllowedIn` (regex of the file that defines `withRequestStatus`), `signalForms`
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
| `paddock/template-no-ng-class` | 33 | `[class.x]` instead of `[ngClass]` |
| `paddock/template-no-inner-html` | security | no `[innerHTML]` / `[outerHTML]` binding |
| angular-eslint (preset) | 33 | `prefer-inject`, `prefer-signals`, `prefer-signal-model`, `template/prefer-control-flow`, `template/prefer-class-binding` |

Not lintable (kept as review rules or other checks): 34 (window bootstrap shape), 35 (lazy panels: conformance C29 measures the
initial chunk), 36 (prefer `resource`), 38 (each lifecycle effect has a failing test), 39 (shared code in Paddock). The
"unknown `var(--x)`" half of Debrief's `lint-styles` needs the app's token list and stays an app script. Rule 29 is a class
based heuristic; reads through injected stores are not seen.

Tests: `npm test` (RuleTester valid and invalid cases per rule, and the preset run through ESLint on temp files). Needs
`npm install` for the peer dependencies as dev dependencies.
