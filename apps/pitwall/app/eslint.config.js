// @ts-check
// Flat ESLint config for the Pitwall UI: ESLint + typescript-eslint recommended rules and angular-eslint recommended
// rules for TypeScript and templates (including accessibility).
const eslint = require('@eslint/js');
const { defineConfig } = require('eslint/config');
const tseslint = require('typescript-eslint');
const angular = require('angular-eslint');
// Every e2e page runs on the pinned clock: specs reach `test` through e2e/test.ts only (C133).
const { bulkCalls, e2ePinnedClock } = require('../../../eslint/src/index.cjs');

module.exports = defineConfig(
  {
    ignores: ['dist/**', '.angular/**', 'coverage/**', 'node_modules/**', 'src-tauri/**', 'playwright-report/**', 'test-results/**', 'playwright.config.ts'],
  },
  {
    files: ['**/*.ts'],
    extends: [eslint.configs.recommended, tseslint.configs.recommended, angular.configs.tsRecommended],
    processor: angular.processInlineTemplates,
    rules: {
      '@angular-eslint/directive-selector': ['error', { type: 'attribute', prefix: 'pw', style: 'camelCase' }],
      '@angular-eslint/component-selector': ['error', { type: 'element', prefix: 'pw', style: 'kebab-case' }],
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_', varsIgnorePattern: '^_', caughtErrorsIgnorePattern: '^_' }],
    },
  },
  {
    files: ['**/*.html'],
    extends: [angular.configs.templateRecommended, angular.configs.templateAccessibility],
  },
  ...e2ePinnedClock(),
  // PRINCIPLES, Bulk APIs: no backend call per item (report-only: PADDOCK_BULK_LINT=warn reads it).
  ...bulkCalls(),
);
