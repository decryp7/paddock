'use strict';
// Dialog contents draw no footer (ADR 0026 §2, §6): the frame draws it from the content's declared `dialogActions`, so a
// dialog component's template may not hold a `footer` element, an element whose class names a foot or an actions row
// (`.foot`, `pd-x__foot`, `actions`, `pd-x__actions`), or a button labelled Close, Cancel or OK. The preset applies it to
// the templates of dialog components (`*dialog*.ts` inline templates and `*dialog*.html`).
const FOOT_CLASS = /(^|[-_])(foot|footer|actions)$/;
const WAY_OUT = /^(close|cancel|ok)$/i;

function textOf(node) {
  if (!node) return '';
  if (node.type === 'Text') return node.value ?? '';
  return (node.children ?? []).map(textOf).join('');
}

const { prop } = require('../ast.cjs');

/** The footer offences in a template's text (the TS rule reads inline templates of dialog contents this way). */
function templateOffences(text) {
  const out = [];
  if (/<footer[\s>]/.test(text)) out.push('<footer>');
  for (const m of text.matchAll(/class="([^"]*)"/g)) {
    const token = m[1].split(/\s+/).find((t) => FOOT_CLASS.test(t));
    if (token) out.push(`.${token}`);
  }
  for (const m of text.matchAll(/<button\b[^>]*>([^<]*)<\/button>/g)) {
    const label = m[1].replace(/\s+/g, ' ').trim();
    if (WAY_OUT.test(label)) out.push(`a "${label}" button`);
  }
  return out;
}

/** Whether a class is a dialog content: it implements DialogContent, declares dialogActions or injects DIALOG_REF. */
function isDialogContent(cls, sc) {
  if ((cls.implements ?? []).some((i) => sc.getText(i).startsWith('DialogContent'))) return true;
  return cls.body.body.some((m) => (m.key && m.key.name === 'dialogActions') || /inject(<[^>]*>)?\(DIALOG_REF\)/.test(sc.getText(m)));
}

module.exports = {
  'dialog-content-footer': {
    meta: {
      type: 'problem',
      docs: { description: 'ADR 0026: a dialog content (any component that is one) draws no footer; the frame does.' },
      schema: [],
      messages: { footer: 'A dialog content draws no footer ({{what}}): declare the actions in `dialogActions` and the frame lays them out (ADR 0026 §2).' },
    },
    create(context) {
      const sc = context.sourceCode;
      return {
        Decorator(node) {
          const call = node.expression;
          if (call.type !== 'CallExpression' || call.callee.type !== 'Identifier' || call.callee.name !== 'Component') return;
          const cls = node.parent;
          if (!cls || !cls.body || !isDialogContent(cls, sc)) return;
          const t = prop(call.arguments[0], 'template');
          if (!t) return;
          const text = t.value.type === 'TemplateLiteral' ? t.value.quasis.map((q) => q.value.cooked).join('') : typeof t.value.value === 'string' ? t.value.value : '';
          for (const what of templateOffences(text)) context.report({ node: t, messageId: 'footer', data: { what } });
        },
      };
    },
  },
  'dialog-frame-footer': {
    meta: {
      type: 'problem',
      docs: { description: 'ADR 0026: a dialog content declares its actions; the frame draws the footer.' },
      schema: [],
      messages: {
        footer: 'A dialog content draws no footer ({{what}}): declare the actions in `dialogActions` and the frame lays them out (ADR 0026 §2).',
        wayOut: 'A dialog content draws no "{{label}}" button: the frame adds Cancel/Close, and an answer is a declared action (ADR 0026 §2).',
      },
    },
    create(context) {
      return {
        Element(node) {
          if (node.name === 'footer') return context.report({ node, messageId: 'footer', data: { what: '<footer>' } });
          const cls = (node.attributes ?? []).find((a) => a.name === 'class');
          const token = cls?.value?.split(/\s+/).find((t) => FOOT_CLASS.test(t));
          if (token) return context.report({ node, messageId: 'footer', data: { what: `.${token}` } });
          if (node.name === 'button') {
            const label = textOf(node).replace(/\s+/g, ' ').trim();
            if (WAY_OUT.test(label)) context.report({ node, messageId: 'wayOut', data: { label } });
          }
        },
      };
    },
  },
};
