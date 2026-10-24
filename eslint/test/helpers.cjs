'use strict';
const { RuleTester } = require('eslint');
const { describe, it } = require('node:test');
const tseslint = require('typescript-eslint');
const templateParser = require('@angular-eslint/template-parser');

RuleTester.describe = describe;
RuleTester.it = it;
RuleTester.itOnly = it.only;

/** A RuleTester for TypeScript sources (filename decides store-file rules). */
const ts = () => new RuleTester({ languageOptions: { parser: tseslint.parser, ecmaVersion: 2023, sourceType: 'module' } });
/** A RuleTester for Angular templates. */
const html = () => new RuleTester({ languageOptions: { parser: templateParser } });

module.exports = { ts, html };
