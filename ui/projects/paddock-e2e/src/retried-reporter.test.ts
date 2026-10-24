import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import RetriedReporter, { renderFlaky } from './retried-reporter.ts';

const t = (title = 'opens | fast') => ({
  parent: { project: () => ({ name: 'chromium' }) },
  location: { file: '/x/ui/e2e/home.spec.ts' },
  titlePath: () => ['', 'chromium', 'home.spec.ts', 'Home', title],
});

test('only tests that passed after a retry are listed', () => {
  const r = new RetriedReporter();
  r.onTestEnd(t(), { status: 'passed', retry: 0 });
  r.onTestEnd(t(), { status: 'failed', retry: 1 });
  r.onTestEnd(t(), { status: 'passed', retry: 2 });
  assert.deepEqual(r.flaky, [{ project: 'chromium', file: 'home.spec.ts', title: 'Home > opens | fast', retries: 2 }]);
  assert.equal(r.printsToStdio(), false);
});

test('renders a table with escaped cells, or nothing', () => {
  assert.equal(renderFlaky([]), '');
  assert.match(renderFlaky([{ project: 'p', file: 'f', title: 'a | b', retries: 1 }]), /\| p \| f \| a \\\| b \| 1 \|/);
});

test('writes the summary and appends to the step summary; nothing without a flake; a failed write is not fatal', () => {
  const dir = mkdtempSync(join(tmpdir(), 'paddock-e2e-'));
  const file = join(dir, 'retried.md');
  const summary = join(dir, 'summary.md');
  const quiet = new RetriedReporter({ file, env: { GITHUB_STEP_SUMMARY: summary } });
  quiet.onEnd();
  assert.equal(existsSync(file), false);
  const r = new RetriedReporter({ file, env: { GITHUB_STEP_SUMMARY: summary } });
  r.onTestEnd(t('x'), { status: 'passed', retry: 1 });
  const log = console.log;
  console.log = () => undefined;
  try {
    r.onEnd();
    const unwritable = new RetriedReporter({ file: join(dir, 'missing', 'x.md'), env: {} });
    unwritable.onTestEnd(t(), { status: 'passed', retry: 1 });
    assert.doesNotThrow(() => unwritable.onEnd());
    assert.match(readFileSync(file, 'utf8'), /Home > x/);
    assert.match(readFileSync(summary, 'utf8'), /Flaky tests/);
  } finally {
    console.log = log;
    rmSync(dir, { recursive: true, force: true });
  }
});
