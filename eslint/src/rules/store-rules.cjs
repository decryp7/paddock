'use strict';
// Rules 30 to 32 about `signalStore(...)`:
//  - 30: request state is the shared `withRequestStatus` union, never `loading` / `error` flags in `withState`.
//  - 31: a store lives in a `*.store.ts` file and keeps `protectedState` on.
//  - 32: `providedIn: 'root'` is for app-wide singletons and says so in an `app-wide:` comment; a store injects at most two
//        other stores.
const { hasTagBefore, keyName, prop, walk } = require('../ast.cjs');

const FLAGS = new Set(['loading', 'isLoading', 'error', 'isError', 'pending', 'isPending', 'loaded']);
const STORE_FILE = /\.store(\.spec)?\.[cm]?ts$/;

/** @type {import('eslint').Rule.RuleModule} */
module.exports = {
  meta: {
    type: 'problem',
    docs: { description: 'Rules 30-32: request status union, store file name and protectedState, store scope and injection count.' },
    schema: [{ type: 'object', properties: { allowFlagsIn: { type: 'string' } }, additionalProperties: false }],
    messages: {
      flag: 'State property `{{name}}` is a request flag; use the shared `withRequestStatus` (idle | loading | loaded | error) (rule 30).',
      file: '`signalStore` belongs in a `*.store.ts` file (rule 31).',
      protectedState: '`protectedState: false` is not allowed; change state only through the store\'s own methods (rule 31).',
      root: '`providedIn: \'root\'` is for app-wide singletons: add a `// app-wide: <reason>` comment above, or provide the store at the route, component or dialog window (rule 32).',
      inject: 'This store injects {{count}} other stores; more than two means a missing facade or feature (rule 32).',
    },
  },
  create(context) {
    const sc = context.sourceCode;
    const allow = context.options[0]?.allowFlagsIn ? new RegExp(context.options[0].allowFlagsIn) : null;
    return {
      CallExpression(node) {
        const c = node.callee;
        if (c.type !== 'Identifier') return;
        if (c.name === 'withState' && !(allow && allow.test(context.filename))) {
          const o = node.arguments[0];
          if (o?.type === 'ObjectExpression') for (const p of o.properties) if (FLAGS.has(keyName(p))) context.report({ node: p, messageId: 'flag', data: { name: keyName(p) } });
        }
        if (c.name !== 'signalStore') return;
        if (!STORE_FILE.test(context.filename)) context.report({ node, messageId: 'file' });
        const cfg = node.arguments.find((a) => a.type === 'ObjectExpression');
        const ps = prop(cfg, 'protectedState');
        if (ps && ps.value.type === 'Literal' && ps.value.value === false) context.report({ node: ps, messageId: 'protectedState' });
        const root = prop(cfg, 'providedIn');
        if (root && root.value.type === 'Literal' && root.value.value === 'root' && !hasTagBefore(sc, node, 'app-wide:')) context.report({ node: root, messageId: 'root' });
        const injected = new Set();
        for (const a of node.arguments) {
          walk(a, sc.visitorKeys, (n) => {
            if (n.type === 'CallExpression' && n.callee.type === 'Identifier' && n.callee.name === 'inject' && n.arguments[0]?.type === 'Identifier' && /Store$/.test(n.arguments[0].name)) injected.add(n.arguments[0].name);
          });
        }
        if (injected.size > 2) context.report({ node, messageId: 'inject', data: { count: injected.size } });
      },
    };
  },
};
