// lefthook.yml names folders and files (a `root:`, the files a script runs). Three times a path or a tool change broke the
// pre-push hook with nothing to say so; this reads every command and checks them (ER-21).
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';
import { PADDOCK } from './helpers.mjs';

/** Every command of the file as { name, root, run } (a line reader: the file is flat). */
export function commands(text) {
  const out = [];
  let cur = null;
  for (const line of text.split('\n')) {
    const name = /^ {4}([\w-]+):\s*$/.exec(line);
    if (name) {
      cur = { name: name[1], root: '', run: '' };
      out.push(cur);
      continue;
    }
    const kv = /^ {6}(root|run):\s*(.*)$/.exec(line);
    if (cur && kv) cur[kv[1]] = kv[2].replace(/^"(.*)"$/, '$1').trim();
  }
  return out.filter((c) => c.run);
}

const globRe = (g) => new RegExp('^' + g.replace(/[.+^$()|[\]\\]/g, '\\$&').replace(/\*\*\//g, '(?:.*/)?').replace(/\*/g, '[^/]*').replace(/\?/g, '[^/]') + '$');

/** What is wrong with the paths of one command: [] when every one exists, matches a file, and is a file where node --test needs one. */
export function problems(cmd, tracked, root = PADDOCK) {
  const found = [];
  const base = cmd.root.replace(/\/$/, '');
  if (base && !(existsSync(join(root, base)) && statSync(join(root, base)).isDirectory())) found.push(`${cmd.name}: root ${base} is not a folder`);
  const nodeTest = /\bnode\b[^|&;]*--test\b/.test(cmd.run);
  for (const tok of cmd.run.split(/\s+/)) {
    if (!tok || tok.startsWith('-') || tok.includes('{') || !/[/.]/.test(tok) || /^(?:https?:|\.\/dev$)/.test(tok)) continue;
    const rel = [base, tok].filter(Boolean).join('/');
    if (/[*?]/.test(tok)) {
      const re = globRe(rel);
      if (!tracked.some((f) => re.test(f))) found.push(`${cmd.name}: ${tok} matches no file under ${base || '.'}`);
    } else if (!existsSync(join(root, rel))) {
      found.push(`${cmd.name}: ${tok} does not exist under ${base || '.'}`);
    } else if (nodeTest && statSync(join(root, rel)).isDirectory()) {
      found.push(`${cmd.name}: ${tok} is a folder; node --test needs files (a glob such as ${tok.replace(/\/?$/, '/*.test.mjs')})`);
    }
  }
  return found;
}

const tracked = () => spawnSync('git', ['ls-files'], { cwd: PADDOCK, encoding: 'utf8', maxBuffer: 1 << 26 }).stdout.split('\n').filter(Boolean);

test('every path and glob a lefthook command names exists, and node --test is given files, not folders', () => {
  const cmds = commands(readFileSync(join(PADDOCK, 'lefthook.yml'), 'utf8'));
  assert.ok(cmds.length >= 20, `commands found (${cmds.length})`);
  const files = tracked();
  assert.deepEqual(cmds.flatMap((c) => problems(c, files)), []);
});

test('the check sees the old Pitwall entry (a folder for node --test), a missing file and an empty glob', () => {
  const files = tracked();
  const old = { name: 'pitwall-scripts', root: 'apps/pitwall/', run: 'node --test scripts/commit-lint.test.mjs scripts/release/latest-json.test.mjs scripts/ci/' };
  assert.ok(problems(old, files).some((p) => p.includes('scripts/ci/ is a folder')), problems(old, files).join('|'));
  assert.deepEqual(problems({ ...old, run: 'node --test scripts/ci/*.test.mjs' }, files), []);
  assert.equal(problems({ name: 'x', root: 'crates/', run: 'bash scripts/nothing.sh' }, files).length, 1);
  assert.equal(problems({ name: 'x', root: 'crates/', run: 'node --test zz/*.test.mjs' }, files).length, 1);
});
