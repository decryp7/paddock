'use strict';
// Every e2e page runs on the pinned clock (`pinnedClockTest` in @decryp7/paddock-e2e; conformance C133 keeps what a linter cannot say). A
// spec reaches `test` through the app's `e2e/test.ts` only, and makes a page of its own with `newPinnedContext`. One rule, not
// `no-restricted-*` selectors: flat-config rule options do not merge, so an app's own `no-restricted-syntax` in the same files would replace
// the selectors of another block (Debrief's raw-row selectors, run 8056); a rule of its own has nothing to collide with.
//
// Reported (messageIds):
// - `test`: a value import of `test` (also `test as x`), a default import, a namespace import (`import * as pw`) or a re-export
//   (`export { test } from`, `export * from`) of `@playwright/test` or `@decryp7/paddock-e2e`, subpaths included; `import type` is fine.
// - `dynamic`: a dynamic `import()` or `require()` of either module, or one whose module is not a plain string (a template literal or a
//   variable: it cannot be checked).
// - `install`: `clock.install` with no argument, a non-literal argument (a variable may carry nothing), an object without `time`, or
//   `time: undefined`: it restarts the pinned clock at the real time.
// - `context`: `.newContext(` on any receiver, or `.newPage(` on a receiver named like a browser (`browser`, `pwBrowser`): a page the
//   fixtures did not pin. Use `newPinnedContext(browser, options)`.

const MODULE = /^(?:@playwright\/test|@decryp7\/paddock-e2e)(?:\/.*)?$/;
const isModule = (value) => typeof value === 'string' && MODULE.test(value);
const WRAPPER = "the app's e2e/test.ts (it wraps pinnedClockTest, so every page starts on the pinned clock)";

/** The last name of a receiver: `browser`, `this.browser`, `ctx.browser()`. */
function receiverName(node) {
  if (!node) return '';
  if (node.type === 'Identifier') return node.name;
  if (node.type === 'MemberExpression' && !node.computed && node.property.type === 'Identifier') return node.property.name;
  if (node.type === 'CallExpression') return receiverName(node.callee);
  return '';
}

/** @type {import('eslint').Rule.RuleModule} */
module.exports = {
  meta: {
    type: 'problem',
    docs: { description: 'E2E specs reach test through the pinned-clock wrapper and make pages the fixtures pin (C133).' },
    schema: [],
    messages: {
      test: `{{what}} from {{source}}: import test and expect from ${WRAPPER}; a type import is fine.`,
      dynamic: `{{what}}: use ${WRAPPER}.`,
      install: '{{what}} restarts the pinned clock at the real time: drop it (the pin is installed) or pass a literal { time }.',
      context: '{{what}} makes a page the fixtures did not pin: use newPinnedContext(browser, options) from @decryp7/paddock-e2e.',
    },
  },
  create(context) {
    const importSource = (node) => (node.source && node.source.type === 'Literal' ? node.source.value : undefined);
    return {
      ImportDeclaration(node) {
        const source = importSource(node);
        if (node.importKind === 'type' || !isModule(source)) return;
        for (const s of node.specifiers) {
          if (s.type === 'ImportDefaultSpecifier') context.report({ node: s, messageId: 'test', data: { what: 'A default import', source } });
          else if (s.type === 'ImportNamespaceSpecifier') context.report({ node: s, messageId: 'test', data: { what: 'A namespace import (it can reach .test)', source } });
          else if (s.type === 'ImportSpecifier' && s.importKind !== 'type' && ['test', 'default'].includes(s.imported.name ?? s.imported.value)) {
            context.report({ node: s, messageId: 'test', data: { what: 'A value import of `test`', source } });
          }
        }
      },
      ExportNamedDeclaration(node) {
        const source = importSource(node);
        if (node.exportKind === 'type' || !isModule(source)) return;
        for (const s of node.specifiers) {
          if (s.exportKind !== 'type' && ['test', 'default'].includes(s.local.name ?? s.local.value)) context.report({ node: s, messageId: 'test', data: { what: 'A re-export of `test`', source } });
        }
      },
      ExportAllDeclaration(node) {
        const source = importSource(node);
        if (node.exportKind !== 'type' && isModule(source)) context.report({ node, messageId: 'test', data: { what: 'A re-export of everything (`export *`)', source } });
      },
      ImportExpression(node) {
        const s = node.source;
        if (s.type === 'Literal') {
          if (isModule(s.value)) context.report({ node, messageId: 'dynamic', data: { what: `A dynamic import() of ${s.value}` } });
        } else if (s.type === 'TemplateLiteral' && s.expressions.length === 0) {
          if (isModule(s.quasis[0].value.cooked)) context.report({ node, messageId: 'dynamic', data: { what: `A dynamic import() of ${s.quasis[0].value.cooked}` } });
        } else {
          context.report({ node, messageId: 'dynamic', data: { what: 'A dynamic import() whose module is not a plain string (it cannot be checked)' } });
        }
      },
      CallExpression(node) {
        const callee = node.callee;
        if (callee.type === 'Identifier' && callee.name === 'require') {
          const a = node.arguments[0];
          if (!a || a.type !== 'Literal') context.report({ node, messageId: 'dynamic', data: { what: 'A require() whose module is not a plain string (it cannot be checked)' } });
          else if (isModule(a.value)) context.report({ node, messageId: 'dynamic', data: { what: `A require() of ${a.value}` } });
          return;
        }
        if (callee.type !== 'MemberExpression' || callee.computed || callee.property.type !== 'Identifier') return;
        const name = callee.property.name;
        if (name === 'install' && receiverName(callee.object) === 'clock') {
          const arg = node.arguments[0];
          let why = null;
          if (!arg) why = 'A bare clock.install()';
          else if (arg.type !== 'ObjectExpression') why = 'clock.install with an argument that is not an object literal';
          else {
            const time = arg.properties.find((p) => p.type === 'Property' && !p.computed && (p.key.name ?? p.key.value) === 'time');
            if (!time) why = 'clock.install without a time';
            else if (time.value.type === 'Identifier' && time.value.name === 'undefined') why = 'clock.install({ time: undefined })';
          }
          if (why) context.report({ node, messageId: 'install', data: { what: why } });
        } else if (name === 'newContext') {
          context.report({ node, messageId: 'context', data: { what: '.newContext()' } });
        } else if (name === 'newPage' && /browser/i.test(receiverName(callee.object))) {
          context.report({ node, messageId: 'context', data: { what: 'browser.newPage()' } });
        }
      },
    };
  },
};
