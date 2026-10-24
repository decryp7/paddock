'use strict';
// Rule 27 (zoneless only) and rule 38 (tests): no `zone.js`, no `provideZoneChangeDetection`, and no zone-based test
// helpers (`fakeAsync`, `tick`, `flush`, `waitForAsync`): use `await fixture.whenStable()` and a fake clock.
const ZONE_TESTING = new Set(['fakeAsync', 'tick', 'flush', 'flushMicrotasks', 'discardPeriodicTasks', 'waitForAsync']);

/** @type {import('eslint').Rule.RuleModule} */
module.exports = {
  meta: {
    type: 'problem',
    docs: { description: 'Rule 27/38: zoneless only, no zone.js and no zone-based test helpers.' },
    schema: [],
    messages: {
      zone: 'Apps are zoneless: do not import `{{source}}` (rule 27).',
      provide: '`provideZoneChangeDetection` is not allowed; apps are zoneless (rule 27).',
      testing: '`{{name}}` needs zone.js; use `await fixture.whenStable()` and a fake clock (rules 27 and 38).',
    },
  },
  create(context) {
    return {
      ImportDeclaration(node) {
        const source = String(node.source.value);
        if (source === 'zone.js' || source.startsWith('zone.js/')) context.report({ node, messageId: 'zone', data: { source } });
        if (source === '@angular/core/testing') {
          for (const s of node.specifiers) if (s.type === 'ImportSpecifier' && ZONE_TESTING.has(s.imported.name)) context.report({ node: s, messageId: 'testing', data: { name: s.imported.name } });
        }
      },
      Identifier(node) {
        if (node.name === 'provideZoneChangeDetection' && node.parent.type !== 'ImportSpecifier') context.report({ node, messageId: 'provide' });
      },
      ImportSpecifier(node) {
        if (node.imported.name === 'provideZoneChangeDetection') context.report({ node, messageId: 'provide' });
      },
    };
  },
};
