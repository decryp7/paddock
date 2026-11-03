import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { CONSOLE, GUI, problems, specsFor, subsystemOf } from './check-pe-subsystem.mjs';

/** A minimal PE image with the given optional-header magic and subsystem. */
function pe(subsystem, magic = 0x20b) {
  const b = Buffer.alloc(0x200);
  b.writeUInt16LE(0x5a4d, 0);
  b.writeUInt32LE(0x80, 0x3c);
  b.writeUInt32LE(0x4550, 0x80);
  b.writeUInt16LE(magic, 0x80 + 24);
  b.writeUInt16LE(subsystem, 0x80 + 24 + 68);
  return b;
}

test('the subsystem is read from PE32 and PE32+ images', () => {
  assert.equal(subsystemOf(pe(GUI)), 2);
  assert.equal(subsystemOf(pe(CONSOLE, 0x10b)), 3);
});

test('a console agent and a GUI command line are both reported, the right ones pass', () => {
  const files = { 'dyno-agent.exe': pe(CONSOLE), 'dyno.exe': pe(CONSOLE), 'gui.exe': pe(GUI) };
  const read = (p) => files[p];
  assert.deepEqual(problems(['dyno.exe=3', 'gui.exe=2'], read), []);
  const found = problems(['dyno-agent.exe=2', 'gui.exe=3'], read);
  assert.equal(found.length, 2);
  assert.match(found[0], /dyno-agent\.exe has PE subsystem 3, expected 2/);
});

test('files that are not programs fail with a reason, never throw', () => {
  const found = problems(['a=2', 'b=2', 'c=2'], (p) => ({ a: Buffer.from('hello'), b: Buffer.alloc(0x200), c: Buffer.alloc(10) })[p]);
  assert.equal(found.length, 3);
  assert.match(found[0], /no MZ header/);
});

test('--dir names the release list in that folder', () => {
  assert.deepEqual(specsFor(['--dir', 'C:/b/dist/']), ['C:/b/dist/dyno-agent.exe=2', 'C:/b/dist/dyno.exe=3', 'C:/b/dist/dyno-bench.exe=3']);
  assert.deepEqual(specsFor(['x.exe=2']), ['x.exe=2']);
});

test('run as a program it exits 1 for a console agent and 0 for a GUI one', () => {
  const dir = mkdtempSync(join(tmpdir(), 'dyno-pe-'));
  try {
    const bad = join(dir, 'dyno-agent.exe');
    const cli = join(dir, 'dyno.exe');
    writeFileSync(bad, pe(CONSOLE));
    writeFileSync(cli, pe(CONSOLE));
    writeFileSync(join(dir, 'dyno-bench.exe'), pe(CONSOLE));
    const script = fileURLToPath(new URL('./check-pe-subsystem.mjs', import.meta.url));
    const run = (...args) => spawnSync(process.execPath, [script, ...args], { encoding: 'utf8' });
    const failed = run('--dir', dir);
    assert.equal(failed.status, 1);
    assert.match(failed.stdout, /::error::.*dyno-agent\.exe has PE subsystem 3, expected 2/);
    writeFileSync(bad, pe(GUI));
    assert.equal(run('--dir', dir).status, 0);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
