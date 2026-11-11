import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { evidenceDir, evidenceFile } from './evidence.mjs';

test('evidence files go to the run folder, are unique per call and are never the host /tmp root', () => {
  const dir = mkdtempSync(join(tmpdir(), 'ev-'));
  process.env.PADDOCK_EVIDENCE_DIR = join(dir, 'evidence');
  try {
    const a = evidenceFile('win-uac-install');
    const b = evidenceFile('win-uac-install');
    assert.notEqual(a, b, 'a second UAC prompt does not overwrite the first screenshot');
    assert.ok(a.startsWith(join(dir, 'evidence')));
    assert.ok(existsSync(evidenceDir()));
    assert.match(evidenceFile('a b/c'), /-a-b-c\.png$/);
  } finally { delete process.env.PADDOCK_EVIDENCE_DIR; rmSync(dir, { recursive: true, force: true }); }
});
