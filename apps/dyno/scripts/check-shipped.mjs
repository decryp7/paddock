// What Dyno ships. Three checks:
//  1. no shipped binary other than the helper itself depends on dyno-helper, by `cargo tree -e normal` on every target;
//  2. no source of a shipped crate asks the OS for elevation;
//  3. no shipped package enables `test-support` (the test doubles, ADR 0033) on dyno-platform, dyno-core or paddock-power, by
//     `cargo tree -e normal -f '{p} {f}'` on every target, with a positive control (ER-33): the same probe with dev edges must see
//     the feature, or the check reports `check broken` instead of passing.
// The shipped set is derived, not kept by hand: `dyno-app` plus every `bundle.externalBin` of `app/src-tauri/tauri.conf.json`.
import { execFileSync } from 'node:child_process';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

/** The package of the app itself (its `mainBinaryName` is `dyno-app`). */
export const APP_PACKAGE = 'dyno-app';
/** The crates that must not be in any shipped binary's dependency tree (the helper's own tree names itself, see `forbiddenIn`). */
export const FORBIDDEN_CRATES = ['dyno-helper'];
/** The crates that carry the test doubles behind a `test-support` feature (ADR 0033). */
export const TEST_SUPPORT_CRATES = ['dyno-platform', 'dyno-core', 'paddock-power'];
/** The feature that must never reach a shipped binary. */
export const TEST_SUPPORT_FEATURE = 'test-support';

/**
 * The packages that ship: the app plus the package of each `bundle.externalBin` binary. `metadata` is `cargo metadata --no-deps`
 * output (it maps a binary target to its package), `conf` the parsed `tauri.conf.json`. A binary that no package builds throws.
 */
export function shippedPackages(metadata, conf) {
  const byBin = new Map();
  for (const p of metadata.packages) for (const t of p.targets) if (t.kind.includes('bin')) byBin.set(t.name, p.name);
  const names = [APP_PACKAGE];
  for (const entry of conf.bundle.externalBin) {
    const bin = entry.split('/').pop();
    const pkg = byBin.get(bin);
    if (!pkg) throw new Error(`bundle.externalBin names ${bin}, which no package of the workspace builds`);
    if (!names.includes(pkg)) names.push(pkg);
  }
  return names;
}

/** The shipped packages of the workspace under `root`. */
export function shipped(root) {
  const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1'], { cwd: root, encoding: 'utf8', maxBuffer: 64 << 20 }));
  const conf = JSON.parse(readFileSync(join(root, 'app/src-tauri/tauri.conf.json'), 'utf8'));
  return shippedPackages(metadata, conf);
}
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

/** The forbidden crates present in a dependency tree. `pkg` is the tree's root: the helper may contain itself. */
export function forbiddenIn(treeOutput, pkg) {
  const crates = cratesIn(treeOutput);
  return FORBIDDEN_CRATES.filter((c) => c !== pkg && crates.has(c));
}

/**
 * The test-support crates that have the `test-support` feature enabled in the output of `cargo tree --prefix none -f '{p} {f}'`
 * (a line is `name vX.Y.Z (path) feature,feature`).
 */
export function testSupportEnabledIn(treeOutput) {
  const found = new Set();
  for (const line of treeOutput.split('\n')) {
    const m = /^(\S+) v\S+(?: \([^)]*\))?(?: (.*?))?(?: \(\*\))?$/.exec(line.trim());
    if (!m || !TEST_SUPPORT_CRATES.includes(m[1])) continue;
    if ((m[2] ?? '').split(',').map((f) => f.trim()).includes(TEST_SUPPORT_FEATURE)) found.add(m[1]);
  }
  return [...found].sort();
}

/** `cargo tree` of `pkg` with `edges` ("normal" is what ships, "normal,dev" is what tests build), printing the enabled features. */
export function featureTree(root, pkg, edges) {
  return execFileSync('cargo', ['tree', '-p', pkg, '-e', edges, '--prefix', 'none', '--target', 'all', '-f', '{p} {f}'], {
    cwd: root,
    encoding: 'utf8',
    timeout: 300_000,
    maxBuffer: 64 << 20,
  });
}

/**
 * The probe's positive control (ER-33): with dev edges, `dyno-agent` must show `dyno-platform`, `dyno-core` and `paddock-power` with `test-support`. Otherwise the probe
 * or the feature name has changed and a clean result of the real check would mean nothing: throw `check broken`.
 */
export function assertProbeSees(root, treeOutput = featureTree(root, 'dyno-agent', 'normal,dev')) {
  const seen = testSupportEnabledIn(treeOutput);
  const missing = TEST_SUPPORT_CRATES.filter((c) => !seen.includes(c));
  if (missing.length) {
    throw new Error(
      `check broken: \`cargo tree -p dyno-agent -e normal,dev\` does not show ${missing.join(', ')} with ${TEST_SUPPORT_FEATURE} (dyno-platform's feature must enable all three), so the probe cannot see the feature. ` +
        'Fix the probe in scripts/check-shipped.mjs, or the feature name, before trusting this check.',
    );
  }
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
export const SHIPPED_DIRS = ['crates/dyno-core', 'crates/dyno-platform', 'crates/dyno-agent', 'crates/dyno-cli', 'crates/dyno-bench', 'crates/dyno-bench-gpu', 'app/src-tauri/src'];

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split('/').pop())) {
  const root = new URL('..', import.meta.url).pathname;
  const problems = [];
  const packages = shipped(root);
  for (const pkg of packages) for (const c of forbiddenIn(tree(root, pkg), pkg)) problems.push(`${pkg} depends on ${c}`);
  assertProbeSees(root);
  for (const pkg of packages) for (const c of testSupportEnabledIn(featureTree(root, pkg, 'normal'))) problems.push(`${pkg} enables ${TEST_SUPPORT_FEATURE} on ${c}`);
  problems.push(...elevationOffences(root, SHIPPED_DIRS).map((o) => `elevation call: ${o}`));
  if (problems.length) {
    console.error(`A Dyno build must not ship the helper's callers, test doubles or elevation code:\n${problems.join('\n')}`);
    process.exit(1);
  }
  console.log('No shipped binary depends on the helper, enables test-support or asks for administrator rights.');
}
