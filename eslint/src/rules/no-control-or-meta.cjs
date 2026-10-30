'use strict';
// E2E keyboard shortcuts: Playwright's `ControlOrMeta` follows the machine the tests run on, but an app binds `Mod` by the
// page's own platform. WebKit on a Linux runner emulates a Mac (`devices['Desktop Safari']`), so the app wants Cmd while
// `ControlOrMeta` presses Ctrl (CI run 7134, and the e2e Linux container). `pressMod` of @decryp7/paddock-e2e follows the page.
const WORD = /\bControlOrMeta\b/;

/** @type {import('eslint').Rule.RuleModule} */
module.exports = {
  meta: {
    type: 'problem',
    docs: { description: 'E2E code presses `Mod` with `pressMod` (or `pageModKey`), never with Playwright\'s `ControlOrMeta`.' },
    schema: [],
    messages: {
      banned:
        '`ControlOrMeta` follows the test machine, not the page (WebKit on Linux emulates a Mac). Use `pressMod(page, \'Mod+k\')` (or `pageModKey(page)` for click modifiers) from @decryp7/paddock-e2e.',
    },
  },
  create(context) {
    return {
      Literal(node) {
        if (typeof node.value === 'string' && WORD.test(node.value)) context.report({ node, messageId: 'banned' });
      },
      TemplateElement(node) {
        if (WORD.test(node.value.cooked ?? node.value.raw)) context.report({ node, messageId: 'banned' });
      },
    };
  },
};
