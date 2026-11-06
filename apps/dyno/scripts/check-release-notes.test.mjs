import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { newestSection, problems } from './check-release-notes.mjs';

const CHANGELOG = join(dirname(fileURLToPath(import.meta.url)), '..', 'CHANGELOG.md');

test('the shipped notes (the newest release) have no ADR, work-package or identifier references', () => {
  assert.deepEqual(problems(readFileSync(CHANGELOG, 'utf8')), []);
});

test('finds each kind of reference', () => {
  const md = '# Changelog\n\n## [2.0.0] - 2026-01-01\n\n### Added\n\n- Benchmarks, disk (ADR 0030 WP-B3)\n- Helper requests for SCHEDULE_WAKE and more\n- A plain sentence.\n- Use `dyno awake` here\n';
  assert.equal(problems(md).length, 3);
  assert.match(problems(md)[0], /ADR 0030/);
});

test('reads only the newest release', () => {
  const md = '## [2.0.0] - 2026-01-01\n\n- Fine.\n\n## [1.0.0] - 2025-01-01\n\n- Old (ADR 0001)\n';
  assert.deepEqual(problems(md), []);
  assert.equal(newestSection(md).lines.length, 3);
});

test('a changelog with no release has nothing to check', () => {
  assert.deepEqual(problems('# Changelog\n'), []);
});

test('catches the SNAKE_CASE identifiers the VM harness catches (digits, one-letter parts, trailing digit groups)', () => {
  for (const id of ['S3_BUCKET', 'I_O', 'SHA256_SUM', 'WAKE_2', 'IN_PROGRESS']) {
    const md = `## [2.0.0] - 2026-01-01\n\n- Shows ${id} in a sentence.\n`;
    assert.equal(problems(md).length, 1, id);
  }
});

test('leaves ordinary capitals and units alone', () => {
  const md = '## [2.0.0] - 2026-01-01\n\n- Reads CPU and RAM, shows GB/s, and a USB-C port. Version 2_0 is not an identifier.\n';
  assert.deepEqual(problems(md), []);
});
