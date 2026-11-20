import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { pathToFileURL } from 'node:url';

// makeApp's folders went into the system temp folder and were never removed: 730,000 `x9-fixture-*` folders piled up there,
// and Chrome, which reads through that folder when it quits, then took over 5 s to close (Spotter's close test, CI 8334,
// 8341, 8346). A process that uses makeApp leaves nothing behind in its temp folder.
test('makeApp leaves no folder behind when its process ends', () => {
  const temp = mkdtempSync(join(tmpdir(), 'helpers-cleanup-'));
  try {
    const helpers = pathToFileURL(join(import.meta.dirname, 'helpers.mjs')).href;
    const script = `import { makeApp } from ${JSON.stringify(helpers)};
      console.log(JSON.stringify([makeApp({ 'a.txt': 'a' }), makeApp({ 'b/c.txt': 'c' })]));`;
    const out = execFileSync(process.execPath, ['--input-type=module', '-e', script], {
      env: { ...process.env, TMPDIR: temp, TMP: temp, TEMP: temp },
      encoding: 'utf8',
    });
    const made = JSON.parse(out.trim().split('\n').pop());
    assert.equal(made.length, 2);
    for (const dir of made) assert.ok(dir.startsWith(temp), `${dir} is not below the process's temp folder ${temp}`);
    assert.deepEqual(made.filter((d) => existsSync(d)), [], 'makeApp folders left behind');
    assert.deepEqual(readdirSync(temp), [], 'entries left in the temp folder');
  } finally {
    rmSync(temp, { recursive: true, force: true });
  }
});
