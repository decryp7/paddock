'use strict';
// Rule 27: every component declares `changeDetection: ChangeDetectionStrategy.OnPush` explicitly (v22 defaults to it, but
// explicit survives a lint or a future default change). `Eager` (or `Default`) needs a comment saying why.
const { prop } = require('../ast.cjs');

/** @type {import('eslint').Rule.RuleModule} */
module.exports = {
  meta: {
    type: 'problem',
    docs: { description: 'Rule 27: components declare ChangeDetectionStrategy.OnPush explicitly; Eager needs a comment.' },
    schema: [],
    messages: {
      missing: 'Declare `changeDetection: ChangeDetectionStrategy.OnPush` on the component explicitly (rule 27).',
      eager: '`ChangeDetectionStrategy.{{strategy}}` needs a comment saying why OnPush is not enough (rule 27).',
    },
  },
  create(context) {
    const sc = context.sourceCode;
    return {
      Decorator(node) {
        const call = node.expression;
        if (call.type !== 'CallExpression' || call.callee.type !== 'Identifier' || call.callee.name !== 'Component') return;
        const meta = call.arguments[0];
        const p = prop(meta, 'changeDetection');
        if (!p) return context.report({ node, messageId: 'missing' });
        const v = p.value;
        const strategy = v.type === 'MemberExpression' && v.property.type === 'Identifier' ? v.property.name : null;
        if (strategy === 'OnPush') return;
        const commented = sc.getCommentsBefore(p).length > 0 || sc.getCommentsAfter(p).some((c) => c.loc.start.line === p.loc.end.line) || sc.getTokenAfter(p, { includeComments: true })?.type === 'Line';
        if (!commented) context.report({ node: p, messageId: 'eager', data: { strategy: strategy ?? 'a computed value' } });
      },
    };
  },
};
