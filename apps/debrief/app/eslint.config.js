// @ts-check
// Flat ESLint config for the Debrief Angular UI: ESLint + typescript-eslint recommended rules and
// angular-eslint recommended rules for TypeScript and templates (including accessibility).
const eslint = require('@eslint/js');
const { defineConfig } = require('eslint/config');
const tseslint = require('typescript-eslint');
const angular = require('angular-eslint');

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
    // Specs share one module graph per bundle, so test state must not leak (ER-21).
    files: ['**/*.spec.ts'],
    rules: {
      'no-restricted-syntax': [
        'error',
        {
          selector: [
            "AssignmentExpression > MemberExpression.left[object.property.name='prototype']",
            "AssignmentExpression > MemberExpression.left > TSAsExpression.object > MemberExpression.expression[property.name='prototype']",
            "AssignmentExpression > MemberExpression.left > TSAsExpression.object > TSAsExpression > MemberExpression[property.name='prototype']",
          ].join(', '),
          message:
            'A patch on a prototype that is never restored leaks into the specs that run after it. Use vi.spyOn(X.prototype, name) (restoreMocks puts it back after each test) instead of assigning (lessons: test isolation, ER-21).',
        },
        {
          selector:
            "CallExpression[callee.object.name=/^(Object|Reflect)$/][callee.property.name=/^(defineProperty|defineProperties|assign)$/] > MemberExpression.arguments:first-child[property.name='prototype']",
          message:
            'Object.defineProperty, Reflect.defineProperty or Object.assign on a prototype is a patch that is never restored and leaks into later specs. Use vi.spyOn(X.prototype, name, \'get\') (restoreMocks puts it back after each test) (lessons: test isolation, ER-21).',
        },
        {
          selector: [
            "ExpressionStatement > ChainExpression:has(CallExpression[callee.property.name='stack'])",
            "ExpressionStatement > ChainExpression[expression.callee.object.property.name='ref']",
            "ExpressionStatement > ChainExpression[expression.callee.object.optional=true]",
          ].join(', '),
          message:
            'An optional chain used as a statement does nothing when the dialog is not open yet (dialogs load lazily), so the test passes or hangs silently. Use `(await openDialog(dialogs)).ref.close()`, which fails when no dialog opens (lessons: test isolation, ER-21).',
        },
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
