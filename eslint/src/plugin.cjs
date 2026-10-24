'use strict';
const rules = require('./rules/index.cjs');

/** The ESLint plugin object (`paddock`): the rules by name. */
module.exports = { meta: { name: '@decryp7/paddock-eslint', version: require('../package.json').version }, rules };
