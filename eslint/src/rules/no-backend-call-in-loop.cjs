'use strict';
// PRINCIPLES, Bulk APIs (WP-0 G5 of the bulk-API redesign): a caller with N items makes one backend call, not N. This rule flags a
// call on a backend, a port or the IPC `invoke` that runs once per item:
//
// - inside the body, test or update of a loop over a collection: a `for...of` / `for...in` / `for await` over anything but a list
//   written out in the code, or a `for` / `while` / `do...while` whose test reads a `.length` or `.size` or drains with `.shift()` /
//   `.pop()`. A retry or paging loop (`for (;;)`, `while (true)`, `do ... while (page)`, `i < 3`) is one call per attempt or page and
//   is left alone;
// - inside a `.forEach` callback (a loop by another name), except on a list written out in the code;
// - inside a `.map` / `.flatMap` callback whose promises are started together: the map is an argument of `Promise.all`,
//   `Promise.allSettled`, `Promise.any`, `Promise.race` or rxjs `forkJoin` (directly, spread into an array, or through a `const`
//   that is passed there). ESLint's `no-await-in-loop` does not see this shape.
//
// The receiver is recognised by name, as in this repo's code: `backend`, `*Backend`, `port`, `*Port`, `ipc`, a bare `invoke(...)`,
// `*.invoke(...)`, a parameter or variable typed `typeof invoke`, and any variable or field made by `inject(<token>)` whose token's name ends in Backend / BACKEND / Port / PORT
// (`private ws = inject(DebriefBackend)`). A callback passed to another call (`act(() => backend.x())`, `.then(...)`) counts as
// running inside it, except for calls that keep it for later (timers, listeners, `effect`, `push`). Not seen: calls through a
// helper function or through a store or service whose name is not a backend's (DB-4's `charts`, DB-5's `counter`, PC-3's `status`), rxjs `mergeMap` over items, and a component
// per row that calls the backend in its own effect (DB-2, SP-4): the runtime budget tests on the counting seams cover those.
//
// Also not seen: a loop whose bound was read into a name first (`const n = items.length; for (let i = 0; i < n; i++)`), since the
// test then reads no `.length` or `.size`; write the bound in the test, or rely on the budget tests.
//
// Known false positive: a signal read on a port (`port.imageBlock()`, a `computed` the port exposes) looks like a call; mark it
// `// per-item: <reason>` or read the signal once before the loop.
//
// Opt-out: `// per-item: <reason>` right above the call's statement, the loop or the `Promise.all` statement, with a reason of at
// least three words. Conformance C140 counts every opt-out in conformance/c140-allow.json, a list that only shrinks (ER-46).
//
// Report-only (ER-41) until its first green run on the real tree: the `bulkCalls()` block (src/bulk.cjs) reads its severity from
// PADDOCK_BULK_LINT and is off by default; every app's `npm run lint:report` (bin/lint-report.cjs) turns it on in CI.
const { statementOf } = require('../ast.cjs');

const LOOPS = new Set(['ForStatement', 'ForInStatement', 'ForOfStatement', 'WhileStatement', 'DoWhileStatement']);
const FUNCTIONS = new Set(['ArrowFunctionExpression', 'FunctionExpression', 'FunctionDeclaration']);
const PROMISE_JOINS = new Set(['all', 'allSettled', 'any', 'race']);
const RX_JOINS = new Set(['forkJoin']);
const MAPS = new Set(['map', 'flatMap']);
/** Calls that keep a callback to run later, not now. */
const DEFERRING = new Set([
  'push', 'unshift', 'set', 'add', 'addEventListener', 'on', 'once', 'listen', 'subscribe', 'setTimeout', 'setInterval',
  'requestAnimationFrame', 'requestIdleCallback', 'queueMicrotask', 'effect', 'computed', 'afterNextRender', 'afterRenderEffect',
  'resource', 'rxResource', 'register', 'defer',
]);

/** A receiver name that is a backend, port or IPC object (`viewport`, `report`, `support` are not). */
const NAME = /^(backend|port|ipc)$|Backend$|Port$/;
/** A DI token whose instance is a backend or port. */
const TOKEN = /(Backend|BACKEND|Port|PORT)$/;
/** The opt-out with a reason of at least three words (conformance C140 counts every opt-out in a list that only shrinks). */
const OPT_OUT = /(^|\s)per-item:\s*\S+(\s+\S+){2,}/;
/** Members that say a loop's test reads a collection: `while (queue.length)`, `i < items.length`, `queue.size > 0`. */
const COLLECTION_MEMBERS = new Set(['length', 'size']);
/** Calls in a `while` test that drain a collection: `while (x = queue.shift())`. */
const DRAINS = new Set(['shift', 'pop']);

/** The name a member's key spells, or null. */
function memberName(node) {
  if (node.type !== 'MemberExpression') return null;
  if (node.computed) return node.property.type === 'Literal' ? String(node.property.value) : null;
  return node.property.type === 'Identifier' || node.property.type === 'PrivateIdentifier' ? node.property.name : null;
}

/** The token name of `inject(X)` or `inject(X, opts)`, or null. */
function injectedToken(node) {
  if (!node || node.type !== 'CallExpression' || node.callee.type !== 'Identifier' || node.callee.name !== 'inject') return null;
  const arg = node.arguments[0];
  if (!arg) return null;
  if (arg.type === 'Identifier') return arg.name;
  if (arg.type === 'MemberExpression') return memberName(arg);
  return null;
}

/** True when `node` (a loop's test) reads a collection's length or size, or drains one. */
function readsCollection(node) {
  if (!node || typeof node.type !== 'string') return false;
  if (node.type === 'MemberExpression' && !node.computed && node.property.type === 'Identifier' && COLLECTION_MEMBERS.has(node.property.name)) return true;
  if (node.type === 'CallExpression' && node.callee.type === 'MemberExpression' && DRAINS.has(memberName(node.callee))) return true;
  for (const key of Object.keys(node)) {
    if (key === 'parent') continue;
    const v = node[key];
    if (Array.isArray(v)) {
      if (v.some((c) => c && typeof c.type === 'string' && readsCollection(c))) return true;
    } else if (v && typeof v.type === 'string' && readsCollection(v)) return true;
  }
  return false;
}

/**
 * Whether a loop runs once per item of a collection: `for...of` / `for...in` over anything but a list written out in the code, a
 * `for (;;)` or `while` whose test reads a collection's length or size or drains it. A retry or paging loop (`for (;;)`, `while (true)`,
 * `do ... while (page)`, `i < 3`) is one call per attempt or page and is not this rule's shape.
 */
function overCollection(loop) {
  if (loop.type === 'ForOfStatement' || loop.type === 'ForInStatement') return loop.right.type !== 'ArrayExpression';
  return readsCollection(loop.test);
}

/** @type {import('eslint').Rule.RuleModule} */
module.exports = {
  meta: {
    type: 'suggestion',
    docs: { description: 'PRINCIPLES, Bulk APIs: a backend, port or invoke call runs once for all items, not once per item in a loop or a Promise.all over a map.' },
    schema: [],
    messages: {
      loop: '`{{callee}}` is called once per item in this loop: add a bulk command that takes every item and returns a result per item (ItemResult, PRINCIPLES "Bulk APIs"), or mark the call `// per-item: <reason>`.',
      parallel: '`{{callee}}` is started once per item and joined with {{join}}: add a bulk command that takes every item and returns a result per item (ItemResult, PRINCIPLES "Bulk APIs"), or mark it `// per-item: <reason>`.',
    },
  },
  create(context) {
    const sourceCode = context.sourceCode;
    /** Names bound to `inject(<backend token>)` in this file (variables and class fields). */
    const injected = new Set();
    /** Names typed `typeof invoke` (a parameter or variable that is the IPC call itself). */
    const invokers = new Set();
    /** Calls of this file, judged at its end. */
    const pending = [];

    const optedOut = (node) => sourceCode.getCommentsBefore(statementOf(node)).some((c) => OPT_OUT.test(c.value));

    /** True when `expr` (a callee's object) is a backend, port or IPC receiver. */
    function isReceiver(expr) {
      if (expr.type === 'Identifier') return NAME.test(expr.name) || injected.has(expr.name);
      if (expr.type === 'MemberExpression' && expr.object.type === 'ThisExpression') {
        const name = memberName(expr);
        return name !== null && (NAME.test(name) || injected.has(name));
      }
      if (expr.type === 'CallExpression') {
        const token = injectedToken(expr);
        return token !== null && TOKEN.test(token);
      }
      if (expr.type === 'ChainExpression') return isReceiver(expr.expression);
      return false;
    }

    /** The text naming the call for the message, or null when `call` is not a backend call. */
    function backendCallee(call) {
      const callee = call.callee.type === 'ChainExpression' ? call.callee.expression : call.callee;
      if (callee.type === 'Identifier') return callee.name === 'invoke' || invokers.has(callee.name) ? callee.name : null;
      if (callee.type !== 'MemberExpression') return null;
      if (memberName(callee) === 'invoke') return sourceCode.getText(callee);
      return isReceiver(callee.object) ? sourceCode.getText(callee) : null;
    }

    /** `Promise.all(...)` or `forkJoin(...)`: the join's name, or null. */
    function joinName(call) {
      if (!call || call.type !== 'CallExpression') return null;
      const c = call.callee;
      if (c.type === 'Identifier' && RX_JOINS.has(c.name)) return c.name;
      if (c.type === 'MemberExpression' && c.object.type === 'Identifier' && c.object.name === 'Promise' && PROMISE_JOINS.has(memberName(c))) return `Promise.${memberName(c)}`;
      return null;
    }

    /** The join `mapCall`'s promises go into, or null: its argument, spread in an array argument, or through a `const`. */
    function joinOf(mapCall) {
      let n = mapCall;
      let p = n.parent;
      if (p.type === 'SpreadElement' && p.parent.type === 'ArrayExpression') {
        n = p.parent;
        p = n.parent;
      }
      if (p.type === 'CallExpression' && p.arguments.includes(n)) {
        const name = joinName(p);
        if (name) return { name, node: p };
      }
      if (p.type === 'VariableDeclarator' && p.init === n && p.id.type === 'Identifier') {
        for (const variable of sourceCode.getDeclaredVariables(p)) {
          for (const ref of variable.references) {
            const id = ref.identifier;
            let arg = id;
            if (arg.parent.type === 'SpreadElement' && arg.parent.parent.type === 'ArrayExpression') arg = arg.parent.parent;
            if (arg.parent.type === 'CallExpression' && arg.parent.arguments.includes(arg)) {
              const name = joinName(arg.parent);
              if (name) return { name, node: arg.parent };
            }
          }
        }
      }
      return null;
    }

    /** Why `call` runs once per item: `{ kind, join?, at }`, or null. Climbs to the nearest loop or function boundary. */
    function perItem(call) {
      let child = call;
      for (let n = call.parent; n; child = n, n = n.parent) {
        if (LOOPS.has(n.type)) {
          const once = (n.type === 'ForStatement' && child === n.init) || ((n.type === 'ForOfStatement' || n.type === 'ForInStatement') && (child === n.right || child === n.left));
          if (!once && overCollection(n)) return { kind: 'loop', at: n };
        }
        if (FUNCTIONS.has(n.type)) {
          const owner = n.parent;
          // A function that is not a call's argument (declared, stored, returned, `new Promise(...)`) ends the climb.
          if (owner?.type !== 'CallExpression' || !owner.arguments.includes(n)) return null;
          const method = owner.callee.type === 'MemberExpression' ? memberName(owner.callee) : owner.callee.type === 'Identifier' ? owner.callee.name : null;
          if (method === 'forEach' && owner.callee.type === 'MemberExpression' && owner.callee.object.type !== 'ArrayExpression') return { kind: 'loop', at: owner };
          if (MAPS.has(method) && owner.callee.type === 'MemberExpression') {
            const join = joinOf(owner);
            if (join) return { kind: 'parallel', join: join.name, at: join.node };
          }
          // A callback a call keeps for later (a timer, a listener, an effect, a list of handlers) does not run per item now.
          if (method === null || DEFERRING.has(method)) return null;
          // Otherwise the callee runs it inside the call (`act(() => ...)`, `apply(...)`, `.then(...)`, a `.map` whose
          // promises are not joined): keep climbing to the loop or join around that call.
        }
      }
      return null;
    }

    function remember(id, init) {
      const token = injectedToken(init);
      if (token && TOKEN.test(token) && id) injected.add(id);
    }

    return {
      VariableDeclarator(node) {
        if (node.id.type === 'Identifier') remember(node.id.name, node.init);
      },
      PropertyDefinition(node) {
        const key = node.key.type === 'Identifier' || node.key.type === 'PrivateIdentifier' ? node.key.name : null;
        remember(key, node.value);
      },
      AssignmentPattern(node) {
        if (node.left.type === 'Identifier') remember(node.left.name, node.right);
      },
      // Calls are judged at the end, once every `inject` binding of the file is known (a field may be declared below its use).
      TSTypeQuery(node) {
        if (node.exprName.type !== 'Identifier' || node.exprName.name !== 'invoke') return;
        const annotation = node.parent?.type === 'TSTypeAnnotation' ? node.parent : null;
        const id = annotation?.parent;
        if (id?.type === 'Identifier') invokers.add(id.name);
      },
      'CallExpression:exit'(node) {
        pending.push(node);
      },
      'Program:exit'() {
        for (const call of pending) {
          const callee = backendCallee(call);
          if (!callee) continue;
          const why = perItem(call);
          if (!why || optedOut(call) || optedOut(why.at)) continue;
          context.report({ node: call, messageId: why.kind, data: { callee, join: why.join ?? '' } });
        }
      },
    };
  },
};

