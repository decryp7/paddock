'use strict';
const plugin = require('./plugin.cjs');
const { preset, e2eAxe, DEFAULT_IGNORES } = require('./preset.cjs');

module.exports = { plugin, preset, e2eAxe, DEFAULT_IGNORES, rules: plugin.rules };
