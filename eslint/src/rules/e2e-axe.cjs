'use strict';
// ER-12 (wait on the condition) for axe: a browser spec runs axe through the kit (`axeViolations` of @decryp7/paddock-e2e),
// which waits for the page's animations first. A spec that builds `AxeBuilder` itself reads colours mid-fade and reports
// low contrast on a slow machine (the gallery's form hints in WebKit, 2026-10-30). The helper module (`axe.ts`) is exempt.
const AXE = '@axe-core/playwright';

/** @type {import('eslint').Rule.RuleModule} */
module.exports = {
  meta: {
    type: 'problem',
    docs: { description: 'ER-12: browser specs run axe through the kit (axeViolations), which settles animations first.' },
    schema: [],
    messages: {
      direct: 'Run axe through a helper that waits for animations (`axeViolations` of @decryp7/paddock-e2e, or the app\'s own `axe.ts`), not `{{source}}` directly (ER-12).',
    },
  },
  create(context) {
    const check = (node, source) => {
      if (source === AXE || source.startsWith(`${AXE}/`)) context.report({ node, messageId: 'direct', data: { source } });
    };
    return {
      ImportDeclaration(node) {
        // A type import runs nothing (`import type { AxeResults }`, or only inline `type` specifiers).
        const typeOnly = node.importKind === 'type' || (node.specifiers.length > 0 && node.specifiers.every((s) => s.importKind === 'type'));
        if (!typeOnly) check(node, String(node.source.value));
      },
      ImportExpression(node) {
        if (node.source.type === 'Literal') check(node, String(node.source.value));
      },
      CallExpression(node) {
        const [arg] = node.arguments;
        if (node.callee.type === 'Identifier' && node.callee.name === 'require' && arg?.type === 'Literal') check(node, String(arg.value));
      },
    };
  },
};
