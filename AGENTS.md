# paddock-kit: engineering rules for coding agents and contributors

The shared repo of the Paddock app family (Debrief, Pitcrew, Pitwall, Dyno, Spotter): the Angular UI package `@decryp7/paddock-ui`
(`ui/`), shared Rust crates (`crates/`), the design guide and lessons (`docs/`). See README.md.

## Design principles
- One reason to change per module. Tokens name raw colours, sizes and durations; components use `var(--pd-...)` only.
- Pure core, thin edges: geometry, formatting, theme building and colour maths are pure functions with their own specs.
- Make illegal states unrepresentable: statuses, accents, densities are string-literal unions; status is never colour alone.
- Extend by adding: a new app is a new accent entry plus a migration note; a new component is a new file and an export.
- Composition over inheritance; KISS; DRY with the rule of three; fail fast with a message that tells the user what to fix.

## Security and robustness
- Nothing remote: no web fonts, no CDN, no icon URLs. Icons are inline SVG. The CSP forbids `unsafe-eval` and inline
  style attributes: bind styles with Angular style bindings only, never static `style=""` in templates or `index.html`.
- Never bind untrusted text with `[innerHTML]`.
- Components never throw on odd input (empty series, NaN, null): they render a defined state.

## Languages
- UI: Angular 22 (standalone, signals, OnPush, CDK where behaviour is needed). Shared Rust only in `crates/`. No other runtimes
  in what ships; Node scripts are build tooling only.

## Where logic lives
- Tokens in `ui/projects/paddock-ui/scss/_tokens.scss` are the single source of truth; the design guide documents them and the
  token test measures them. Angular components present; domain rules belong to each app's Rust engine.

## Robustness
- Every state is designed (skeleton, empty, error with Retry). Reduced motion is respected. Anchor every ignore rule
  (`/ui/dist/`, not `dist/`).

## Working in parallel
- Each task owns an explicit set of files. Shared integration points (`public-api.ts`, `_tokens.scss`, `crates/Cargo.toml`)
  are changed by one agent at a time or by adding a single line. Don't edit the apps from here.

## Contracts
- The public API is `ui/projects/paddock-ui/src/public-api.ts` and the CSS custom properties `--pd-*`, `data-theme`,
  `data-accent`, `data-density`. Any change is a CHANGELOG entry; a breaking change bumps the minor version before 1.0.
  No backward-compatibility shims before Paddock's v1.

## Testing and quality gates
- Every change ships with tests; every bug fix starts with a failing test. Components >= 100% lines.
- A colour change must keep `npm run test:tokens` green (WCAG AA/AAA pairs, CVD separation); change the tokens, not the test.
- Screenshots of every component in both themes and each accent are regenerated and looked at before a release.
- Before committing:

  ```sh
  cd ui
  npm run test:tokens && npm test && npm run build && npx playwright test
  ```

## Dependencies and releases
- Prefer permissive, well-maintained dependencies; commit `package-lock.json`; CI uses `npm ci`.
- Conventional Commits (`feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `ci:`, `chore:`). No Co-Authored-By or AI attribution.
- One semver tag per release (`v0.1.0`); CI attaches `paddock-ui-<version>.tgz` to the Gitea release. ADRs in `docs/adr/`.

## Style
- Match surrounding code. Comments only where intent isn't obvious; doc comments on exported items. Sentence-case copy,
  verb + object on buttons. Never run `prettier --write`.

## Working practices
- Read `docs/design-guide.md` before touching tokens or components. Use Serena symbol tools when available.
