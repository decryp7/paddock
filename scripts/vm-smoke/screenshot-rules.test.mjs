import assert from 'node:assert/strict';
import { test } from 'node:test';
import { importScreenshotRules, rulesProblem } from './screenshot-rules.mjs';

test('a missing rules file says the round needs a full checkout; a missing node_modules says npm ci; both are steps for the person, not product failures', async () => {
  assert.match(rulesProblem('/x/ui/a.ts', { exists: false }), /full checkout.*ui\//);
  assert.match(rulesProblem('/x/a.ts', { exists: true, error: "Cannot find package '@playwright/test' imported from /x" }), /npm ci/);
  assert.match(rulesProblem('/x/a.ts', { exists: true, error: 'SyntaxError: x\nmore' }), /could not be loaded: SyntaxError: x$/);
  await assert.rejects(importScreenshotRules('/nonexistent/screenshot-check.ts'), /full checkout/);
});
