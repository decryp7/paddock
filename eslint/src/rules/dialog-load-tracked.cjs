'use strict';
// A dialog window is reported settled (`data-fit-settled`, the size the e2e fit check reads) only when the app is stable,
// i.e. when every load the dialog started has registered a pending task (ADR 0026 §6; lessons: File hash fit on WebKit).
// So async work a dialog starts by itself, when it is created or opened, must be tracked:
//  - a dialog content (a class that implements DialogContent, declares dialogActions or injects DIALOG_REF): in its
//    constructor, ngOnInit, ngAfterViewInit and field initializers;
//  - with the `hooks` option, a store's `withHooks({ onInit })` (for the stores a dialog shows: the preset's `dialogStores`).
// Use `resource()` (it registers the task itself) or `injectTrackLoad()` from `@decryp7/paddock-ui/shell/store`. Flagged: an
// `await`, `.then/.catch/.finally`, `void <call>` or an `async` hook, unless inside a call to `trackLoad(...)`.
const { keyName } = require('../ast.cjs');

const HOOKS = new Set(['ngOnInit', 'ngAfterViewInit', 'ngOnChanges']);
const SETTLERS = new Set(['then', 'catch', 'finally']);

function isDialogContent(cls, sc) {
  if ((cls.implements ?? []).some((i) => sc.getText(i).startsWith('DialogContent'))) return true;
  return cls.body.body.some((m) => (m.key && m.key.name === 'dialogActions') || /inject(<[^>]*>)?\(DIALOG_REF\)/.test(sc.getText(m)));
}

function isTrackCall(node) {
  if (node.type !== 'CallExpression') return false;
  const c = node.callee;
  const name = c.type === 'Identifier' ? c.name : c.type === 'MemberExpression' && !c.computed && c.property.type === 'Identifier' ? c.property.name : '';
  return /trackLoad$/i.test(name);
}

/** @type {import('eslint').Rule.RuleModule} */
module.exports = {
  meta: {
    type: 'problem',
    docs: { description: 'A load a dialog or its store starts on creation is a pending task (resource() or trackLoad), so the window settles after it.' },
    // `{ hooks: true }` also checks a store's `withHooks({ onInit })` (the preset turns it on for the stores a dialog shows).
    schema: [{ type: 'object', properties: { hooks: { type: 'boolean' } }, additionalProperties: false }],
    messages: {
      untracked: 'Async work started by a dialog (or its store) on creation must be tracked: use `resource()` or wrap it in `trackLoad(() => ...)` from `injectTrackLoad()` (`@decryp7/paddock-ui/shell/store`), so the window is settled only after it ends.',
    },
  },
  create(context) {
    const sc = context.sourceCode;
    const hooks = context.options[0]?.hooks === true;
    const keys = sc.visitorKeys;
    const report = (node) => context.report({ node, messageId: 'untracked' });
    /** Reports untracked async starts under `node`; nested functions run later and are not entered. */
    const scan = (node) => {
      if (!node || typeof node.type !== 'string') return;
      if (isTrackCall(node)) return;
      if (node.type === 'AwaitExpression') return report(node);
      if (node.type === 'UnaryExpression' && node.operator === 'void' && node.argument.type === 'CallExpression') {
        if (isTrackCall(node.argument)) return;
        return report(node);
      }
      if (node.type === 'CallExpression' && node.callee.type === 'MemberExpression' && !node.callee.computed && SETTLERS.has(node.callee.property.name)) return report(node);
      for (const key of keys[node.type] ?? []) {
        const child = node[key];
        if (Array.isArray(child)) child.forEach((c) => c && c.type !== undefined && !isFunction(c) && scan(c));
        else if (child && typeof child.type === 'string' && !isFunction(child)) scan(child);
      }
    };
    const isFunction = (n) => n.type === 'ArrowFunctionExpression' || n.type === 'FunctionExpression' || n.type === 'FunctionDeclaration';
    const scanFunction = (fn) => {
      if (fn.async) return report(fn);
      scan(fn.body);
    };
    return {
      ClassBody(body) {
        const cls = body.parent;
        if (!isDialogContent(cls, sc)) return;
        for (const m of body.body) {
          if (m.type === 'MethodDefinition' && (m.kind === 'constructor' || (m.key && HOOKS.has(m.key.name)))) scanFunction(m.value);
          else if (m.type === 'PropertyDefinition' && m.value && !m.static && !isFunction(m.value)) scan(m.value);
        }
      },
      CallExpression(node) {
        if (!hooks || node.callee.type !== 'Identifier' || node.callee.name !== 'withHooks') return;
        const walkHooks = (n) => {
          if (!n || typeof n.type !== 'string') return;
          if (n.type === 'Property' && keyName(n) === 'onInit' && (n.value.type === 'FunctionExpression' || n.value.type === 'ArrowFunctionExpression')) scanFunction(n.value);
          else if (n.type === 'Property' && n.method && keyName(n) === 'onInit') scanFunction(n.value);
          else for (const key of keys[n.type] ?? []) {
            const child = n[key];
            if (Array.isArray(child)) child.forEach(walkHooks);
            else walkHooks(child);
          }
        };
        node.arguments.forEach(walkHooks);
      },
    };
  },
};
