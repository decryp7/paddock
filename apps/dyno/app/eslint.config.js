// @ts-check
// Flat ESLint config for the Dyno UI: ESLint + typescript-eslint recommended rules and angular-eslint recommended
// rules for TypeScript and templates (including accessibility).
const eslint = require('@eslint/js');
const { defineConfig } = require('eslint/config');
const tseslint = require('typescript-eslint');
const angular = require('angular-eslint');
// The shared Paddock rule that a dialog's start-up load is a pending task (ADR 0026 §6).
const { plugin: paddock } = require('../../../eslint/src/index.cjs');

module.exports = defineConfig(
  {
    ignores: ['dist/**', '.angular/**', 'coverage/**', 'node_modules/**', 'src-tauri/**'],
  },
  {
    files: ['**/*.ts'],
    extends: [eslint.configs.recommended, tseslint.configs.recommended, angular.configs.tsRecommended],
    processor: angular.processInlineTemplates,
    rules: {
      '@angular-eslint/directive-selector': ['error', { type: 'attribute', prefix: 'dy', style: 'camelCase' }],
      '@angular-eslint/component-selector': ['error', { type: 'element', prefix: 'dy', style: 'kebab-case' }],
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_', varsIgnorePattern: '^_', caughtErrorsIgnorePattern: '^_' }],
    },
  },
  {
    files: ['**/*.ts'],
    plugins: { paddock },
    rules: { 'paddock/dialog-load-tracked': 'error' },
  },
  {
    files: ['**/*.html'],
    extends: [angular.configs.templateRecommended, angular.configs.templateAccessibility],
  },
);
