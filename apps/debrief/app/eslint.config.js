// @ts-check
// Flat ESLint config for the Debrief Angular UI: ESLint + typescript-eslint recommended rules and
// angular-eslint recommended rules for TypeScript and templates (including accessibility).
const eslint = require('@eslint/js');
const { defineConfig } = require('eslint/config');
const tseslint = require('typescript-eslint');
const angular = require('angular-eslint');

/**
 * Report-only rules (ER-41) run only in `npm run lint:report` (DEBRIEF_LINT_REPORT=1). `ng lint` fails on any
 * warning (maxWarnings 0), so a warn-level rule there would already block. Each moves into the normal lint as an error
 * after its first green report on the real tree, recorded here by CI run id.
 */
const REPORT = process.env['DEBRIEF_LINT_REPORT'] === '1' ? 'warn' : 'off';

/** Selectors that also match the grid's loading placeholder rows (see the e2e block below). */
function rawRowSelectors() {
  const loaded = String.raw`(?!:not\(\.placeholder\)|\[aria-selected|\.sel|\.hl)`;
  const role = String.raw`\[role=("|')?row("|')?\]`;
  const pattern = String.raw`/(${role}\.row|debrief-events-grid \.row)${loaded}|${role}\[aria-rowindex/`;
  const message = 'This selector also matches loading placeholder rows: use gridRows(page) from e2e/app.ts.';
  return [`Literal[value=${pattern}]`, `TemplateElement[value.raw=${pattern}]`].map((selector) => ({ selector, message }));
}

module.exports = defineConfig(
  {
    ignores: [
      'dist/**',
      '.angular/**',
      'coverage/**',
      'node_modules/**',
      'src-tauri/**',
      'test-results/**',
      'playwright-report/**',
      // Smoke-test runtime data (WebView2 profiles, Debrief data), drivers and failure artifacts.
      'e2e-tauri/.tmp/**',
      'e2e-tauri/.drivers/**',
      'e2e-tauri/.artifacts/**',
    ],
  },
  {
    files: ['**/*.ts'],
    extends: [
      eslint.configs.recommended,
      tseslint.configs.recommended,
      angular.configs.tsRecommended,
    ],
    processor: angular.processInlineTemplates,
    rules: {
      '@angular-eslint/directive-selector': [
        'error',
        { type: 'attribute', prefix: 'debrief', style: 'camelCase' },
      ],
      '@angular-eslint/component-selector': [
        'error',
        { type: 'element', prefix: 'debrief', style: 'kebab-case' },
      ],
      // The codebase marks intentionally unused parameters/bindings (interface-shaped stubs, rest-omission
      // destructuring such as `const { dropped: _d, ...rest } = x`) with a leading underscore.
      '@typescript-eslint/no-unused-vars': [
        'error',
        {
          argsIgnorePattern: '^_',
          varsIgnorePattern: '^_',
          caughtErrorsIgnorePattern: '^_',
          destructuredArrayIgnorePattern: '^_',
          ignoreRestSiblings: true,
        },
      ],
      // `let ref; const done = () => ref?.destroy(); ref = effect(...)`: a closure that reads the binding before
      // its single assignment cannot use `const` (TDZ), so those are not reported.
      'prefer-const': ['error', { ignoreReadBeforeAssign: true }],
    },
  },
  {
    // Rule 31: a store is changed only through its own methods, so `patchState` lives in `*.store.ts` files (and the
    // shared store features and tests) and nowhere else.
    files: ['src/**/*.ts'],
    ignores: ['src/**/*.store.ts', 'src/**/*.spec.ts', 'src/app/shared/store/**'],
    rules: {
      'no-restricted-imports': [
        'error',
        { paths: [{ name: '@ngrx/signals', importNames: ['patchState'], message: 'Change state in the store\'s own methods (a *.store.ts file), not outside it (rule 31).' }] },
      ],
    },
  },
  {
    // A grid row whose page is still loading is a `.placeholder` with the same role and an `aria-rowindex`: a spec that
    // clicks or waits on `[role=row].row` (with or without quotes), `debrief-events-grid .row` or
    // `[role=row][aria-rowindex=…]` can get one, and clicking it selects nothing (jobs.e2e.ts, run 8056). Specs use
    // `gridRows` (e2e/app.ts). Selected (`[aria-selected`, `.sel`) and highlighted (`.hl`) rows are always loaded, so
    // those stay allowed. Report-only (`lint:report`) until its first green run (ER-41).
    files: ['e2e/**/*.ts'],
    rules: {
      'no-restricted-syntax': [REPORT, ...rawRowSelectors()],
    },
  },
  {
    // ADR 0071: a late async answer is dropped through `LatestRequest` (src/app/shared/latest-request.ts), not a
    // hand-rolled counter (`const mine = ++this.seq; ... if (mine !== this.seq) return;`). Report-only (`lint:report`) until
    // its first green run (ER-41); the guards it lists move to the helper when their files are next touched.
    files: ['src/**/*.ts'],
    ignores: ['src/**/*.spec.ts', 'src/app/shared/latest-request.ts'],
    rules: {
      'no-restricted-syntax': [
        REPORT,
        // The guard itself: `mine !== this.seq`, `generation === store.generation()`, `gen !== this.gen`.
        ...['right.name', 'right.property.name', 'right.callee.property.name'].map((path) => ({
          selector: `BinaryExpression[operator=/^[!=]==$/][left.name=/^(mine|seq|gen|generation)$/][${path}=/^(seq|gen|generation|\\w+Seq|\\w+Generation)$/]`,
          message: 'A hand-rolled request counter: use LatestRequest from src/app/shared/latest-request.ts (ADR 0071).',
        })),
      ],
    },
  },
  {
    files: ['**/*.html'],
    extends: [angular.configs.templateRecommended, angular.configs.templateAccessibility],
    rules: {
      // Debrief's composite widgets (combobox listboxes, grids, tree grids, trees, tab lists, menus) follow the
      // WAI-ARIA aria-activedescendant / roving-tabindex patterns: focus and keyboard handling live on the
      // owning input or container (or on a [attr.tabindex]-bound child these rules cannot see), and items are
      // deliberately not tab stops. These two rules only reported such cases, so they are off.
      '@angular-eslint/template/click-events-have-key-events': 'off',
      '@angular-eslint/template/interactive-supports-focus': 'off',
    },
  },
);
