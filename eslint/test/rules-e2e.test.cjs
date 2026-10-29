'use strict';
const { ts } = require('./helpers.cjs');
const rules = require('../src/rules/index.cjs');

ts().run('e2e-axe-through-kit (ER-12)', rules['e2e-axe-through-kit'], {
  valid: [
    "import { axeViolations } from '@decryp7/paddock-e2e';",
    "import { expect } from '@playwright/test';",
    "const m = await import('./axe');",
    "const x = require('node:fs');",
  ],
  invalid: [
    { code: "import AxeBuilder from '@axe-core/playwright';", errors: [{ messageId: 'direct' }] },
    { code: "const { default: AxeBuilder } = await import('@axe-core/playwright');", errors: [{ messageId: 'direct' }] },
    { code: "const AxeBuilder = require('@axe-core/playwright');", errors: [{ messageId: 'direct' }] },
  ],
});
