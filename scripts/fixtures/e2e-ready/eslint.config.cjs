'use strict';
// A fixture project shaped like the apps': the paddock plugin is registered only inside the e2e block (e2ePinnedClock), so a `--rule paddock/...` override
// on the command line cannot find it ("could not find plugin paddock"); the rule is reached through the block's own severity, set by the environment.
const tseslint = require('typescript-eslint');
const { defineConfig } = require('eslint/config');
const { e2ePinnedClock } = require('../../../eslint/src/index.cjs');

module.exports = defineConfig({ files: ['**/*.ts'], languageOptions: { parser: tseslint.parser } }, ...e2ePinnedClock());
