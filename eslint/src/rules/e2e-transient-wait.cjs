'use strict';
// A spec that finds a transient element itself instead of through the app's helper for that kind (the ready-read class, docs/lessons.md:
// debrief.yml runs 8191, 8215, 8297, 8300). A menu is visible, and still, at the window's top-left until its overlay is positioned; a toast
// that is not an error leaves after its time (8 s in Debrief) and a toast stack can move under the pointer. The helpers wait for the place,
// hold a toast while a spec reads or presses it, and press with the clock paused; specs use them. Kinds (selector -> helper file):
//   getByRole('menu')            -> menu-support.ts  (anchoredMenu, contextMenu, openMenu, menuNamed for a closed or text check)
//   getByTestId('notification'), a locator() selector with [data-testid=notification] or [data-toast]
//                                -> toast-support.ts (heldToast, pressInToast, runningToast, toastNamed / toastByCause for a count or text check)
//   a locator() selector with [role=menu] -> menu-support.ts
// The option `kinds` replaces the table for another app. Report-only (ER-41; off unless PADDOCK_E2E_TRANSIENT_LINT=warn or error, see
// src/e2e.cjs) until its first green run on the real tree is recorded here. Recorded: not yet. First count 2026-11-19, Debrief: 59 menu sites
// and 29 toast sites before, 0 after (moved onto the helpers in the change that added the rule); the locator() form added in review found 5
// more toast sites, also moved.
const { basename } = require('node:path');

const DEFAULT_KINDS = [
  { method: 'getByRole', value: 'menu', helper: 'menu-support.ts', what: 'a menu can be drawn before it is positioned and move under the pointer' },
  { method: 'getByTestId', value: 'notification', helper: 'toast-support.ts', what: 'a toast leaves after its time and the stack moves as toasts come and go' },
  // A CSS selector that names a toast or a menu (`locator('[data-testid=notification][data-cause=...]')`, `[data-toast]`, `[role=menu]`).
  { method: 'locator', pattern: /\[data-testid=["']?notification["']?\]|\[data-toast\]/, value: '[data-testid=notification]', helper: 'toast-support.ts', what: 'a toast leaves after its time and the stack moves as toasts come and go' },
  { method: 'locator', pattern: /\[role=["']?menu["']?\]/, value: '[role=menu]', helper: 'menu-support.ts', what: 'a menu can be drawn before it is positioned and move under the pointer' },
];

/** @type {import('eslint').Rule.RuleModule} */
module.exports = {
  meta: {
    type: 'suggestion',
    docs: { description: 'Find transient elements (menus, toasts) through the helper that waits for them to be in place and holds them.' },
    schema: [
      {
        type: 'object',
        properties: {
          kinds: {
            type: 'array',
            items: { type: 'object', properties: { method: { type: 'string' }, value: { type: 'string' }, pattern: { type: 'string' }, helper: { type: 'string' }, what: { type: 'string' } }, required: ['method', 'value', 'helper'] },
          },
        },
        additionalProperties: false,
      },
    ],
    messages: { transient: "`{{call}}` in a spec: {{what}}. Use the helpers of {{helper}}." },
  },
  create(context) {
    const kinds = context.options[0]?.kinds ?? DEFAULT_KINDS;
    const file = basename(context.filename ?? context.getFilename());
    return {
      CallExpression(node) {
        const callee = node.callee;
        if (callee.type !== 'MemberExpression' || callee.computed || callee.property.type !== 'Identifier') return;
        const arg = node.arguments[0];
        if (!arg || arg.type !== 'Literal') return;
        if (typeof arg.value !== 'string') return;
        const kind = kinds.find((k) => k.method === callee.property.name && (k.pattern ? new RegExp(k.pattern).test(arg.value) : k.value === arg.value));
        if (!kind || kind.helper === file) return;
        context.report({ node, messageId: 'transient', data: { call: `${kind.method}('${kind.value}')`, what: kind.what ?? 'a transient element', helper: kind.helper } });
      },
    };
  },
};
