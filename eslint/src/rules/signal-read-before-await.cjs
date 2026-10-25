'use strict';
// Rule 29: signals are read before the first `await` of an async function or effect (a read after it is not tracked and may
// see a stale value). Heuristic, class based: a class field initialised with a signal factory (`signal`, `computed`,
// `input`, `model`, `linkedSignal`, `viewChild`, `toSignal`, ...) is a signal; `this.<field>()` after the first `await` of
// an async method, an async arrow field or an async callback is reported. Reads through injected stores are not seen.
// A deliberate read after the await (a race guard: "is this still the document I asked for?") says so in a comment right
// above its statement that starts with `after-await:`.
const { hasTagBefore, walk } = require('../ast.cjs');

const FACTORIES = new Set(['signal', 'computed', 'input', 'model', 'linkedSignal', 'viewChild', 'viewChildren', 'contentChild', 'contentChildren', 'toSignal']);

function isSignalFactory(init) {
  if (!init || init.type !== 'CallExpression') return false;
  const c = init.callee;
  if (c.type === 'Identifier') return FACTORIES.has(c.name);
  return c.type === 'MemberExpression' && c.object.type === 'Identifier' && FACTORIES.has(c.object.name) && c.property.name === 'required';
}

/** @type {import('eslint').Rule.RuleModule} */
module.exports = {
  meta: {
    type: 'problem',
    docs: { description: 'Rule 29: read signals before the first await.' },
    schema: [],
    messages: { late: '`this.{{name}}()` reads a signal after an `await`; read it before the first await (rule 29).' },
  },
  create(context) {
    const sc = context.sourceCode;
    const stack = [];
    const check = (fn, signals) => {
      if (!fn.async || !signals.size) return;
      // The function suspends when its innermost first `await` has evaluated its operand, so a read inside that operand
      // (`await store.reopen(this.list(), path)`) still happens before the suspension.
      let firstAwait = Infinity;
      walk(fn.body, sc.visitorKeys, (n) => {
        if (n.type === 'AwaitExpression') firstAwait = Math.min(firstAwait, n.range[1]);
      });
      if (firstAwait === Infinity) return;
      walk(fn.body, sc.visitorKeys, (n) => {
        if (n.type !== 'CallExpression' || n.range[0] < firstAwait) return;
        const c = n.callee;
        if (c.type === 'MemberExpression' && c.object.type === 'ThisExpression' && !c.computed && c.property.type === 'Identifier' && signals.has(c.property.name) && !hasTagBefore(sc, n, 'after-await:')) {
          context.report({ node: n, messageId: 'late', data: { name: c.property.name } });
        }
      });
    };
    return {
      ClassBody(node) {
        const signals = new Set();
        for (const m of node.body) if (m.type === 'PropertyDefinition' && m.key.type === 'Identifier' && isSignalFactory(m.value)) signals.add(m.key.name);
        stack.push(signals);
        for (const m of node.body) {
          if (m.type === 'MethodDefinition' && m.value.type === 'FunctionExpression') check(m.value, signals);
          else if (m.type === 'PropertyDefinition' && m.value && /FunctionExpression$/.test(m.value.type)) check(m.value, signals);
        }
      },
      'ClassBody:exit'() {
        stack.pop();
      },
      CallExpression(node) {
        // An async callback handed to effect(): the same rule, for the signals of the enclosing class.
        if (node.callee.type !== 'Identifier' || node.callee.name !== 'effect' || !stack.length) return;
        const fn = node.arguments[0];
        if (fn && /FunctionExpression$/.test(fn.type)) check(fn, stack[stack.length - 1]);
      },
    };
  },
};
