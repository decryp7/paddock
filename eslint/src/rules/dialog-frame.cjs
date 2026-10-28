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

module.exports = {
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
