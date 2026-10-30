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
      manual:
        'Do not choose Meta or Control by platform by hand: the page decides. Use `pressMod(page, \'Mod+k\')` or `pageModKey(page)`; for the browser\'s own text-editing keys `pressHostMod`.',
      banned:
        '`ControlOrMeta` follows the test machine, not the page (WebKit on Linux emulates a Mac). Use `pressMod(page, \'Mod+k\')` (or `pageModKey(page)` for click modifiers) from @decryp7/paddock-e2e.',
    },
  },
  create(context) {
    return {
      Literal(node) {
        if (typeof node.value === 'string' && WORD.test(node.value)) context.report({ node, messageId: 'banned' });
      },
      // `process.platform === 'darwin' ? 'Meta+k' : 'Control+k'` (or navigator.*) decides by hand what pressMod decides by the page.
      ConditionalExpression(node) {
        const keyText = (n) => (n.type === 'Literal' && typeof n.value === 'string' ? n.value : n.type === 'TemplateLiteral' ? n.quasis[0]?.value.cooked ?? '' : null);
        const a = keyText(node.consequent);
        const b = keyText(node.alternate);
        const isMod = (t) => t !== null && /^(Meta|Control)\b/.test(t);
        if (isMod(a) && isMod(b) && /\b(process\.platform|navigator|platform)\b/.test(context.sourceCode.getText(node.test))) context.report({ node, messageId: 'manual' });
      },
      TemplateElement(node) {
        if (WORD.test(node.value.cooked ?? node.value.raw)) context.report({ node, messageId: 'banned' });
      },
    };
  },
};
