'use strict';
// A read right after a bare visibility wait (ER-21 class "a spec reads on 'it is in the DOM' while the page is not ready or still moving": the
// sweep of 2026-11-17, docs/lessons.md). `await expect(x).toBeVisible()` and `x.waitFor()` say an element exists; they do not say its content, its
// position or its siblings are final (an unfiltered list, an overlay not yet positioned, a section whose update pass has not run, a stream still
// writing). The next statement of the same block that READS the page (`boundingBox`, `allInnerTexts`, `allTextContents`, `innerText`, `textContent`,
// and `evaluate` / `evaluateAll` when their result is used) is reported, unless a ready helper (@decryp7/paddock-e2e `ready.ts`: expectListFiltered, expectOverlayAlignedWith,
// expectWired, expectStreamDone, expectDockPlanned, expectNotMoving) or any other wait stands between the two. An error in every app's and the ui's e2e lint block
// (src/e2e.cjs), so the existing lint steps (`--max-warnings 0`) block it on every change to an app or the ui; no separate gate, baseline or switch.
// The window is one statement: only the next statement of the same block is read. A read two statements later, a read after a `waitForTimeout`
// (itself a C71 finding), or one inside a callback is not seen; that keeps false positives low and is the limit of the rule.
// `evaluate` and `evaluateAll` are reads only when their result is used (an assignment, an argument, a return, a chain): a bare `await x.evaluate(...)`
// statement acts or waits.
// A read that is meant to be immediate carries `// eslint-disable-next-line paddock/e2e-ready-reads -- <why>` (ESLint's own directive; unused ones are reported).
// Conformance C139 requires the description and counts the directives per file in conformance/c139-allow.json, a list that only shrinks (ER-46).
const READS = new Set(['boundingBox', 'allInnerTexts', 'allTextContents', 'innerText', 'textContent']);
const RESULT_READS = new Set(['evaluate', 'evaluateAll']);

/** The call is the whole statement (`await x.evaluate(...)`), so its result is not used. `parent` is the node that holds the call. */
const isBareStatement = (call, parent) => parent && (parent.type === 'ExpressionStatement' || (parent.type === 'AwaitExpression' && parent.parent?.type === 'ExpressionStatement'));
const unwrap = (n) => (n && n.type === 'AwaitExpression' ? n.argument : n);
const calleeName = (call) => (call && call.type === 'CallExpression' && call.callee.type === 'MemberExpression' && !call.callee.computed && call.callee.property.type === 'Identifier' ? call.callee.property.name : null);

/** Whether the statement is `await expect(...).toBeVisible()` or `await <locator>.waitFor()` (no state, or state 'visible'). */
function isBareVisibleWait(stmt) {
  if (stmt.type !== 'ExpressionStatement') return false;
  const call = unwrap(stmt.expression);
  const name = calleeName(call);
  if (name === 'toBeVisible') {
    // expect(x).toBeVisible(): the receiver is expect(...) itself, not `.not.` or a poll.
    const obj = call.callee.object;
    return obj.type === 'CallExpression' && obj.callee.type === 'Identifier' && obj.callee.name === 'expect';
  }
  if (name === 'waitFor') {
    const arg = call.arguments[0];
    if (!arg) return true;
    if (arg.type !== 'ObjectExpression') return false;
    const state = arg.properties.find((p) => p.type === 'Property' && !p.computed && (p.key.name ?? p.key.value) === 'state');
    return !state || (state.value.type === 'Literal' && state.value.value === 'visible');
  }
  return false;
}

/** The first read call anywhere inside `node` (not inside a nested function), or null. */
function findRead(node, keys, parent = null) {
  if (!node || typeof node.type !== 'string') return null;
  if (node.type === 'CallExpression') {
    const name = calleeName(node) ?? '';
    if (READS.has(name)) return node;
    // `evaluate` and `evaluateAll` also act and wait (`await page.evaluate(() => window.__ready)`): a read only when the result is used.
    if (RESULT_READS.has(name) && !isBareStatement(node, parent)) return node;
  }
  if (/Function/.test(node.type)) return null;
  for (const key of keys[node.type] ?? []) {
    const child = node[key];
    for (const c of Array.isArray(child) ? child : [child]) {
      const found = findRead(c, keys, node);
      if (found) return found;
    }
  }
  return null;
}

/** @type {import('eslint').Rule.RuleModule} */
const rule = {
  meta: {
    type: 'suggestion',
    docs: { description: 'A read right after a bare visibility wait measures a page that may not be ready (use a ready helper).' },
    schema: [],
    messages: {
      read: '`{{read}}` reads the page right after a bare visibility wait: the element exists, its content or position may not be final. Wait with a ready helper from @decryp7/paddock-e2e (expectListFiltered, expectOverlayAlignedWith, expectWired, expectStreamDone, expectDockPlanned, expectNotMoving), or add `// eslint-disable-next-line paddock/e2e-ready-reads -- <why>` where the read is meant to be immediate.',
    },
  },
  create(context) {
    const source = context.sourceCode ?? context.getSourceCode();
    const keys = source.visitorKeys;
    const check = (body) => {
      for (let i = 0; i + 1 < body.length; i++) {
        if (!isBareVisibleWait(body[i])) continue;
        const next = body[i + 1];
        const read = findRead(next, keys);
        if (read) context.report({ node: read, messageId: 'read', data: { read: `.${calleeName(read)}()` } });
      }
    };
    return {
      BlockStatement: (n) => check(n.body),
      Program: (n) => check(n.body),
    };
  },
};

module.exports = rule;
