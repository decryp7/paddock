'use strict';
const plugin = require('./plugin.cjs');
const { preset, DEFAULT_IGNORES } = require('./preset.cjs');

module.exports = { plugin, preset, DEFAULT_IGNORES, rules: plugin.rules };
