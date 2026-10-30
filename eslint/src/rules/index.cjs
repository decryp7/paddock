'use strict';
const templateRules = require('./template-rules.cjs');
const dialogRules = require('./dialog-frame.cjs');

module.exports = {
  'onpush-explicit': require('./onpush-explicit.cjs'),
  'no-zone': require('./no-zone.cjs'),
  'effect-state': require('./effect-state.cjs'),
  'signal-read-before-await': require('./signal-read-before-await.cjs'),
  'store-rules': require('./store-rules.cjs'),
  'no-raw-style-values': require('./no-raw-style-values.cjs'),
  'e2e-axe-through-kit': require('./e2e-axe.cjs'),
  ...templateRules,
  ...dialogRules,
};
