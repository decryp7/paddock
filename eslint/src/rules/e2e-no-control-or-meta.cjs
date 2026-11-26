'use strict';
// A spec that presses or clicks with Playwright's `ControlOrMeta`. That key follows the machine the tests run on, while the app binds `Mod` by
// the page's navigator: WebKit on a Linux runner has a Mac user agent, so the app wants Cmd where `ControlOrMeta` presses Ctrl (docs/lessons.md
// 2026-10-29: CI run 7134; 2026-11-26: nightly 9103, 7 specs). A spec uses `pressMod(page, 'Mod+K')` or `pageModKey(page)` from
// `@decryp7/paddock-e2e`, which read the page. Reported for the name in any string or template of a spec.
// Blocks from its first green run on the real tree (ER-41). Recorded: the change that added the rule moved every use (108 in apps/debrief,
// 2 files of scripts, 5 specs of the ui) and linted every e2e folder clean, 2026-11-26. Review date: 2027-02-26 (delete it if nothing was caught).

/** @type {import('eslint').Rule.RuleModule} */
module.exports = {
  meta: {
    type: 'problem',
    docs: { description: "Press the app's shortcuts with the page's own modifier (pressMod), not Playwright's host-following ControlOrMeta." },
    schema: [],
    messages: { hostKey: "`ControlOrMeta` follows the machine the tests run on, the app follows the page (WebKit on Linux is a Mac page). Use `pressMod(page, 'Mod+…')` or `pageModKey(page)` from @decryp7/paddock-e2e." },
  },
  create(context) {
    return {
      Literal(node) {
        if (typeof node.value === 'string' && node.value.includes('ControlOrMeta')) context.report({ node, messageId: 'hostKey' });
      },
      TemplateElement(node) {
        if (node.value.cooked?.includes('ControlOrMeta')) context.report({ node, messageId: 'hostKey' });
      },
    };
  },
};
