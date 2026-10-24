'use strict';
// Rule 28: `effect()` is for side effects on things outside the signal graph. An effect that writes state (`.set(`,
// `.update(`, `patchState(`) needs a comment right above it that starts with `effect-state:` and gives the reason it is not
// a computed, linkedSignal or resource. (Debrief's lint-effects.mjs as a rule.)
const { walk, hasTagBefore } = require('../ast.cjs');

const TAG = 'effect-state:';

/** @type {import('eslint').Rule.RuleModule} */
module.exports = {
  meta: {
    type: 'problem',
    docs: { description: 'Rule 28: an effect that writes state says why in an `effect-state:` comment.' },
    schema: [],
    messages: { unjustified: 'This effect writes state without a `// effect-state: <reason>` comment above it; use computed, linkedSignal or resource, or justify it (rule 28).' },
  },
  create(context) {
    const sc = context.sourceCode;
    return {
      CallExpression(node) {
        if (node.callee.type !== 'Identifier' || node.callee.name !== 'effect') return;
        const fn = node.arguments[0];
        if (!fn || !/FunctionExpression$/.test(fn.type)) return;
        let writes = false;
        walk(fn.body, sc.visitorKeys, (n) => {
          if (n.type !== 'CallExpression') return;
          const c = n.callee;
          if (c.type === 'Identifier' && c.name === 'patchState') writes = true;
          else if (c.type === 'MemberExpression' && !c.computed && c.property.type === 'Identifier' && (c.property.name === 'set' || c.property.name === 'update')) writes = true;
        });
        if (writes && !hasTagBefore(sc, node, TAG)) context.report({ node, messageId: 'unjustified' });
      },
    };
  },
};
