'use strict';
const templateRules = require('./template-rules.cjs');
const dialogRules = require('./dialog-frame.cjs');

module.exports = {
  'onpush-explicit': require('./onpush-explicit.cjs'),
  'no-zone': require('./no-zone.cjs'),
  'effect-state': require('./effect-state.cjs'),
  'signal-read-before-await': require('./signal-read-before-await.cjs'),
  'dialog-load-tracked': require('./dialog-load-tracked.cjs'),
  'store-rules': require('./store-rules.cjs'),
  'no-raw-style-values': require('./no-raw-style-values.cjs'),
  'no-backend-call-in-loop': require('./no-backend-call-in-loop.cjs'),
  'e2e-pinned-clock': require('./e2e-pinned-clock.cjs'),
  'e2e-ready-reads': require('./e2e-ready-reads.cjs'),
  'e2e-transient-wait': require('./e2e-transient-wait.cjs'),
  ...templateRules,
  ...dialogRules,
};
