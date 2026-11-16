'use strict';
// The flat-config block for a spec folder: every page runs on the pinned clock (`pinnedClockTest` in @decryp7/paddock-e2e). A spec reaches
// `test` through the app's `e2e/test.ts` only, and makes its pages through the fixtures or `newPinnedContext`. What ESLint cannot say
// (a reason on every opt-out, the live-page steps, a wait over an empty set) is conformance C133 and C134.
// Use: `module.exports = defineConfig(..., ...e2ePinnedClock())` in the app's eslint.config.js; the block lints `e2e/**/*.ts` except `e2e/test.ts`.

const MODULES = ['@playwright/test', '@decryp7/paddock-e2e'];
const MODULE_PATTERN = '^(@playwright\\/test|@decryp7\\/paddock-e2e)$';

const WRAPPER = 'the app\'s e2e/test.ts (it wraps pinnedClockTest, so every page starts on the pinned clock)';

/**
 * @param {object} [options]
 * @param {string[]} [options.files]  globs of the specs (default `e2e/**\/*.ts`)
 * @param {string[]} [options.ignores] the wrapper and what else may import `test` itself (default `e2e/test.ts`)
 */
function e2ePinnedClock(options = {}) {
  const files = options.files ?? ['e2e/**/*.ts'];
  const ignores = options.ignores ?? ['e2e/test.ts'];
  const restrictedImport = (name) => ({
    name,
    importNames: ['test', 'default'],
    allowTypeImports: true,
    message: `Import test and expect from ${WRAPPER}; a type import is fine.`,
  });
  return [
    {
      files,
      ignores,
      rules: {
        // A value import of `test`, a default import or a namespace import (`import * as pw`), and a re-export, of Playwright or the kit.
        '@typescript-eslint/no-restricted-imports': ['error', { paths: MODULES.map(restrictedImport) }],
        'no-restricted-syntax': [
          'error',
          {
            selector: `ImportExpression[source.value=/${MODULE_PATTERN}/]`,
            message: `A dynamic import() of Playwright or the kit reaches an unpinned test: use ${WRAPPER}.`,
          },
          {
            selector: `CallExpression[callee.name='require'][arguments.0.value=/${MODULE_PATTERN}/]`,
            message: `require() of Playwright or the kit reaches an unpinned test: use ${WRAPPER}.`,
          },
          {
            selector: "CallExpression[callee.property.name='install'][callee.object.property.name='clock'][arguments.length=0]",
            message: 'A bare clock.install() restarts the pinned clock at the real time: drop it (the pin is installed) or pass { time }.',
          },
          {
            selector: "CallExpression[callee.property.name='install'][callee.object.property.name='clock'][arguments.0.type='ObjectExpression'][arguments.0.properties.length=0]",
            message: 'clock.install({}) restarts the pinned clock at the real time: drop it or pass { time }.',
          },
          {
            selector: "CallExpression[callee.property.name='install'][callee.object.property.name='clock'][arguments.0.type='Identifier'][arguments.0.name='undefined']",
            message: 'clock.install(undefined) restarts the pinned clock at the real time: drop it or pass { time }.',
          },
          {
            selector: "CallExpression[callee.object.name='browser'][callee.property.name=/^(newContext|newPage)$/]",
            message: 'browser.newContext() and browser.newPage() make a page the fixtures did not pin: use newPinnedContext(browser, options) from @decryp7/paddock-e2e.',
          },
        ],
      },
    },
  ];
}

module.exports = { e2ePinnedClock };
