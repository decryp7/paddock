'use strict';
// The flat-config preset: `module.exports = defineConfig(...require('@decryp7/paddock-eslint').preset({ prefix: 'pc' }))`.
// Needs eslint, @eslint/js, typescript-eslint and angular-eslint (peer dependencies) in the app.
const plugin = require('./plugin.cjs');

/** A peer dependency, resolved from the app being linted first (so a preset linked from a sibling folder works), then from here. */
function peer(name) {
  try {
    return require(require.resolve(name, { paths: [process.cwd()] }));
  } catch {
    return require(name);
  }
}

const DEFAULT_IGNORES = ['dist/**', '.angular/**', 'coverage/**', 'node_modules/**', 'src-tauri/**', 'test-results/**', 'playwright-report/**'];

/**
 * @param {object} options
 * @param {string} options.prefix            selector prefix of the app (`pc`, `debrief`)
 * @param {string[]} [options.ignores]       extra ignored globs
 * @param {object} [options.templateRules]   extra rules for `*.html` (an app's reasoned overrides)
 * @param {object} [options.rules]           extra rules for `*.ts`
 * @param {string} [options.storeFlagsAllowedIn] regex of files that may hold `loading`/`error` state flags (the withRequestStatus feature itself)
 * @param {boolean} [options.signalForms]    rule 37: forbid new reactive forms (off while existing forms stay)
 */
function preset(options) {
  if (!options || !options.prefix) throw new Error('paddock-eslint preset: { prefix } is required (the app\'s selector prefix)');
  const eslint = peer('@eslint/js');
  const tseslint = peer('typescript-eslint');
  const angular = peer('angular-eslint');
  // `no-restricted-imports` is one rule: a later block replaces an earlier one, so each block lists every path it needs.
  const formsPath = { name: '@angular/forms', importNames: ['FormBuilder', 'ReactiveFormsModule', 'FormGroup', 'FormControl'], message: 'New forms use Signal Forms (rule 37).' };
  const patchPath = { name: '@ngrx/signals', importNames: ['patchState'], message: "Change state in the store's own methods (a *.store.ts file), not outside it (rule 31)." };
  const forms = options.signalForms ? { 'no-restricted-imports': ['error', { paths: [formsPath] }] } : {};
  return [
    { ignores: [...DEFAULT_IGNORES, ...(options.ignores ?? [])] },
    {
      files: ['**/*.ts'],
      extends: [eslint.configs.recommended, tseslint.configs.recommended, angular.configs.tsRecommended],
      processor: angular.processInlineTemplates,
      plugins: { paddock: plugin },
      rules: {
        '@angular-eslint/directive-selector': ['error', { type: 'attribute', prefix: options.prefix, style: 'camelCase' }],
        '@angular-eslint/component-selector': ['error', { type: 'element', prefix: options.prefix, style: 'kebab-case' }],
        // Intentionally unused parameters and bindings carry a leading underscore.
        '@typescript-eslint/no-unused-vars': [
          'error',
          { argsIgnorePattern: '^_', varsIgnorePattern: '^_', caughtErrorsIgnorePattern: '^_', destructuredArrayIgnorePattern: '^_', ignoreRestSiblings: true },
        ],
        'prefer-const': ['error', { ignoreReadBeforeAssign: true }],
        // Rule 33: inject(), signal inputs and outputs.
        '@angular-eslint/prefer-inject': 'error',
        '@angular-eslint/prefer-signals': 'error',
        '@angular-eslint/prefer-signal-model': 'error',
        // Rules 27, 28, 29, 30, 31, 32, 38 (see the rule files).
        'paddock/onpush-explicit': 'error',
        'paddock/no-zone': 'error',
        'paddock/effect-state': 'error',
        'paddock/signal-read-before-await': 'error',
        'paddock/store-rules': ['error', options.storeFlagsAllowedIn ? { allowFlagsIn: options.storeFlagsAllowedIn } : {}],
        'paddock/no-raw-style-values': 'error',
        'paddock/dialog-content-footer': 'error',
        'paddock/dialog-load-tracked': 'error',
        ...forms,
        ...(options.rules ?? {}),
      },
    },
    {
      // Rule 31: a store is changed only through its own methods, so `patchState` is imported in `*.store.ts` files, the
      // shared store features and tests, and nowhere else.
      files: ['src/**/*.ts'],
      ignores: ['src/**/*.store.ts', 'src/**/*.spec.ts', 'src/app/shared/store/**', 'src/app/core/store/**'],
      rules: {
        'no-restricted-imports': ['error', { paths: options.signalForms ? [patchPath, formsPath] : [patchPath] }],
      },
    },
    {
      files: ['**/*.html'],
      extends: [angular.configs.templateRecommended, angular.configs.templateAccessibility],
      plugins: { paddock: plugin },
      rules: {
        // Rule 33: @if/@for/@switch and [class.x].
        '@angular-eslint/template/prefer-control-flow': 'error',
        '@angular-eslint/template/prefer-class-binding': 'error',
        'paddock/template-no-ng-class': 'error',
        'paddock/template-no-inner-html': 'error',
        ...(options.templateRules ?? {}),
      },
    },
    ...(options.dialogStores?.length
      ? [
          {
            // A store a dialog shows loads in `onInit`: that load is a pending task too (`injectTrackLoad`), or the window settles before it.
            files: options.dialogStores,
            ignores: ['**/*.spec.ts'],
            plugins: { paddock: plugin },
            rules: { 'paddock/dialog-load-tracked': ['error', { hooks: true }] },
          },
        ]
      : []),
    {
      // ADR 0026 §2: a dialog content declares its actions; the frame draws the footer (inline templates of `*dialog*.ts`).
      files: ['**/*dialog*.ts/*.html', '**/*dialog*.html'],
      // The frame itself draws the footer; specs plant footers on purpose.
      ignores: ['**/dialog-frame.ts/*.html', '**/*.spec.ts/*.html'],
      plugins: { paddock: plugin },
      rules: { 'paddock/dialog-frame-footer': 'error' },
    },
  ];
}

module.exports = { preset, DEFAULT_IGNORES };
