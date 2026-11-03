// What Dyno 1.0 ships must not contain the privileged helper or code that asks for administrator rights (the helper and
// elevation come in 1.1, ADR 0022). Two checks:
//  1. no shipped binary (dyno-app, dyno-agent, dyno-cli) depends on dyno-helper, by `cargo tree -e normal` on every target;
//  2. no source of a shipped crate asks the OS for elevation.
import { execFileSync } from 'node:child_process';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

/** The binaries the installers carry. */
export const SHIPPED = ['dyno-app', 'dyno-agent', 'dyno-cli', 'dyno-bench'];
/** The crates that must not be in any shipped binary's dependency tree. */
export const FORBIDDEN_CRATES = ['dyno-helper'];
/** Calls and phrases that ask the OS to run something with administrator rights. */
export const ELEVATION_TOKENS = ['ShellExecuteExW', '"runas"', '"pkexec"', '/usr/bin/pkexec', 'with administrator privileges', 'AuthorizationExecuteWithPrivileges'];

/** The crates named in the output of `cargo tree --prefix none`, one per line (`name vX.Y.Z (path) (*)`). */
export function cratesIn(treeOutput) {
  return new Set(
    treeOutput
      .split('\n')
      .map((l) => l.trim().split(/\s+/)[0])
      .filter(Boolean),
  );
}

/** The forbidden crates present in a dependency tree. */
export function forbiddenIn(treeOutput) {
  const crates = cratesIn(treeOutput);
  return FORBIDDEN_CRATES.filter((c) => crates.has(c));
}

/** `cargo tree -e normal` of `pkg` over all targets. */
export function tree(root, pkg) {
  return execFileSync('cargo', ['tree', '-p', pkg, '-e', 'normal', '--prefix', 'none', '--target', 'all'], {
    cwd: root,
    encoding: 'utf8',
    timeout: 300_000,
    maxBuffer: 64 << 20,
  });
}

function* sources(dir) {
  for (const name of readdirSync(dir)) {
    if (name === 'target' || name === 'node_modules' || name === '.angular') continue;
    const path = join(dir, name);
    const st = statSync(path);
    if (st.isDirectory()) yield* sources(path);
    else if (name.endsWith('.rs')) yield path;
  }
}

/** Elevation calls in the Rust sources under `dirs` (relative to `root`), as `file:line token`. */
export function elevationOffences(root, dirs) {
  const found = [];
  for (const d of dirs) {
    for (const file of sources(join(root, d))) {
      readFileSync(file, 'utf8')
        .split('\n')
        .forEach((line, i) => {
          const code = line.trimStart();
          if (code.startsWith('//')) return;
          for (const token of ELEVATION_TOKENS) if (line.includes(token)) found.push(`${relative(root, file)}:${i + 1} ${token}`);
        });
    }
  }
  return found;
}

/** The executables `dyno-agent` starts by name: the one list `RUNNER_NAMES` in `crates/dyno-agent/src/bench.rs` (ADR 0030 B0). */
export function startedByAgent(root) {
  const text = readFileSync(join(root, 'crates/dyno-agent/src/bench.rs'), 'utf8');
  const list = /pub const RUNNER_NAMES: &\[&str\] = &\[([^\]]*)\];/.exec(text);
  if (!list) throw new Error('crates/dyno-agent/src/bench.rs declares no RUNNER_NAMES list');
  return [...list[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

/** The folders of the shipped crates. */
export const SHIPPED_DIRS = ['crates/dyno-core', 'crates/dyno-platform', 'crates/dyno-agent', 'crates/dyno-cli', 'crates/dyno-bench', 'app/src-tauri/src'];

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split('/').pop())) {
  const root = new URL('..', import.meta.url).pathname;
  const problems = [];
  for (const pkg of SHIPPED) for (const c of forbiddenIn(tree(root, pkg))) problems.push(`${pkg} depends on ${c}`);
  problems.push(...elevationOffences(root, SHIPPED_DIRS).map((o) => `elevation call: ${o}`));
  if (problems.length) {
    console.error(`Dyno 1.0 must not ship the helper or elevation code:\n${problems.join('\n')}`);
    process.exit(1);
  }
  console.log('No shipped binary depends on the helper or asks for administrator rights.');
}
