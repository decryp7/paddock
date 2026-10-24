'use strict';
// Template rules (angular-eslint's template parser): rule 33 no `[ngClass]` (use `[class.x]`), and the security rule
// "never bind untrusted text with [innerHTML]" as `[innerHTML]` / `[outerHTML]` bindings (a safe, reviewed one is allowed
// with `// eslint-disable-next-line` or an `<!-- eslint-disable -->` and a reason).
const BOUND = { ngClass: ['ngClass', 'Use `[class.x]` bindings, not `[ngClass]` (rule 33).'], innerHTML: ['innerHTML', 'Never bind text with `[innerHTML]` (security rules); render it as text.'], outerHTML: ['outerHTML', 'Never bind text with `[outerHTML]` (security rules).'] };

/** Builds a template rule that reports bound attributes named in `names`. */
function boundAttributeRule(names, description) {
  return {
    meta: { type: 'problem', docs: { description }, schema: [], messages: { bound: '{{message}}' } },
    create(context) {
      return {
        BoundAttribute(node) {
          if (names.includes(node.name)) context.report({ loc: node.loc, messageId: 'bound', data: { message: BOUND[node.name][1] } });
        },
      };
    },
  };
}

module.exports = {
  'template-no-ng-class': boundAttributeRule(['ngClass'], 'Rule 33: [class.x] bindings instead of [ngClass].'),
  'template-no-inner-html': boundAttributeRule(['innerHTML', 'outerHTML'], 'Never bind text with [innerHTML].'),
};
