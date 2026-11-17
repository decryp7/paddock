'use strict';
const plugin = require('./plugin.cjs');
const { preset, DEFAULT_IGNORES } = require('./preset.cjs');
const { e2ePinnedClock } = require('./e2e.cjs');

module.exports = { plugin, preset, e2ePinnedClock, DEFAULT_IGNORES, rules: plugin.rules };
