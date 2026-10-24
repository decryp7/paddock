// Helpers for the tools' tests: every script runs in a scratch folder with a PATH of shims, so nothing on the machine is touched.
import { spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const TOOLS = join(dirname(fileURLToPath(import.meta.url)), '..');

export function scratch(prefix = 'paddock-tools-') {
  return mkdtempSync(join(tmpdir(), prefix));
}

/** Writes `files` ({ path: content }) under `root`; content starting with #! becomes executable. */
export function write(root, files) {
  for (const [p, content] of Object.entries(files)) {
    const file = join(root, p);
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, content);
    if (String(content).startsWith('#!')) chmodSync(file, 0o755);
  }
  return root;
}

/** A shim: a program that appends its arguments to `<bin>/calls.log` and then runs `body` (bash). */
export function shim(bin, name, body = '') {
  write(bin, { [name]: `#!/bin/bash\necho "${name} $*" >> "${bin}/calls.log"\n${body}\n` });
}

export const calls = (bin) => {
  try {
    return readFileSync(join(bin, 'calls.log'), 'utf8').trim().split('\n').filter(Boolean);
  } catch {
    return [];
  }
};

/** Tools the scripts probe for; a CI container has some of them in /usr/bin, so the system folder offered to a script hides them. */
const HIDDEN = new Set(['node', 'nodejs', 'npm', 'npx', 'cargo', 'rustc', 'rustup', 'pkg-config', 'systemctl', 'sw_vers', 'xcode-select', 'docker']);
let systemBin;
/** A folder of links to everything in /usr/bin and /bin except the probed tools, built once per test process. */
export function systemFolder() {
  if (systemBin) return systemBin;
  systemBin = scratch('paddock-sys-');
  for (const dir of ['/usr/bin', '/bin']) {
    let names = [];
    try {
      names = readdirSync(dir);
    } catch {
      continue;
    }
    for (const n of names) {
      if (HIDDEN.has(n) || existsSync(join(systemBin, n))) continue;
      try {
        symlinkSync(join(dir, n), join(systemBin, n));
      } catch {
        /* a name that cannot be linked is simply not offered */
      }
    }
  }
  return systemBin;
}

/** Runs `bash <tools>/<script> args` with PATH = shims first, then the system folder without the probed tools. */
export function run(script, args, { bin, env = {}, cwd } = {}) {
  const r = spawnSync('bash', [join(TOOLS, script), ...args], {
    cwd,
    encoding: 'utf8',
    timeout: 30_000,
    env: { PATH: `${bin ?? ''}:${systemFolder()}`, HOME: cwd ?? tmpdir(), TMPDIR: tmpdir(), ...env },
  });
  return { status: r.status, out: `${r.stdout}${r.stderr}` };
}
