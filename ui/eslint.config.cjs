// Flat ESLint config for the shared UI workspace: the Paddock preset (rules 27 to 39, angular-eslint template and
// accessibility rules) over every project. The library's selectors start with `pd`; the demo apps have their own prefixes.
const { defineConfig } = require('eslint/config');
const { preset } = require('../eslint/src/index.cjs');

const selectorsOf = (prefix) => ({
  '@angular-eslint/component-selector': ['error', { type: 'element', prefix, style: 'kebab-case' }],
  '@angular-eslint/directive-selector': ['error', { type: 'attribute', prefix, style: 'camelCase' }],
});

module.exports = defineConfig(
  ...preset({ prefix: 'pd', storeFlagsAllowedIn: 'request-status' }),
  { files: ['projects/gallery/**/*.ts'], rules: selectorsOf('gal') },
  { files: ['projects/shell-demo/**/*.ts'], rules: selectorsOf('sd') },
  { files: ['projects/workbench-demo/**/*.ts'], rules: selectorsOf('wd') },
  {
    // Test hosts and test harness components are throwaway components with arbitrary selectors.
    files: ['**/*.spec.ts', 'projects/**/testing.ts'],
    rules: { '@angular-eslint/component-selector': 'off', '@angular-eslint/directive-selector': 'off' },
  },
);
