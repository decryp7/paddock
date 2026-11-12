#!/usr/bin/env node
// Runs the app's `build.beforeBuildCommand` before `tauri build`, without the updater key, so the build passes a configuration
// that empties the hook and tauri build never starts it (ADR 0085 D7, rule 78). tauri-cli 2.12.3 would run the hook itself
// inside `tauri build` (src/build.rs:203-211, helpers/mod.rs:68-111), whose environment holds the key; the hook is the app's
// UI build and sidecar builds: npm scripts, Angular builders, cargo build scripts of branch code.
//
//   node before-build.mjs --app-dir <dir> [--config <file>]...     run the hook in <dir>, as tauri build would
//   node before-build.mjs --app-dir <dir> --print [--config ...]     print the effective command only
//
// The effective command is merged as Tauri merges it: <dir>/src-tauri/tauri.conf.json, then tauri.<platform>.conf.json, then
// each --config in order (JSON merge patch; a file path relative to <dir>). A string runs in <dir>; { script, cwd } runs in cwd
// (relative to <dir>). An empty command runs nothing. It runs through bash (Git bash on Windows; Tauri itself uses `sh -c`, or
// `cmd /S /C` on Windows: the apps' hooks are `a && b` lines that mean the same in all three). TAURI_ENV_DEBUG is false (a
// release build); the sidecar scripts read TAURI_ENV_TARGET_TRIPLE and fall back to the host triple, which is what Tauri
// passes for a build without --target. A `beforeBundleCommand` would run inside tauri build with the key, so it is refused.
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { isAbsolute, join, resolve } from 'node:path';
import { isMain } from '../src/util/is-main.mjs';

const PLATFORM = { darwin: 'macos', win32: 'windows', linux: 'linux' };

/** RFC 7396 merge patch of `patch` onto `target`. */
export function mergePatch(target, patch) {
  if (patch === null || typeof patch !== 'object' || Array.isArray(patch)) return patch;
  const out = target && typeof target === 'object' && !Array.isArray(target) ? { ...target } : {};
  for (const [k, v] of Object.entries(patch)) {
    if (v === null) delete out[k];
    else out[k] = mergePatch(out[k], v);
  }
  return out;
}

/** The merged `build` section the hooks come from, and the files read. */
export function effectiveBuild({ appDir, configs = [], platform = process.platform }) {
  const tauri = join(appDir, 'src-tauri');
  for (const f of ['tauri.conf.json5', 'Tauri.toml']) {
    if (existsSync(join(tauri, f))) throw new Error(`${join(tauri, f)} exists: before-build.mjs reads JSON configurations only`);
  }
  const files = [join(tauri, 'tauri.conf.json')];
  const plat = join(tauri, `tauri.${PLATFORM[platform] ?? platform}.conf.json`);
  if (existsSync(plat)) files.push(plat);
  files.push(...configs.map((c) => (isAbsolute(c) ? c : join(appDir, c))));
  let conf = {};
  for (const f of files) conf = mergePatch(conf, JSON.parse(readFileSync(f, 'utf8')));
  return { build: conf.build ?? {}, files };
}

/** { script, cwd } of a hook value, or null when there is none to run. */
export function hookOf(value, appDir) {
  if (value === undefined || value === null || value === '') return null;
  if (typeof value === 'string') return { script: value, cwd: appDir };
  if (typeof value === 'object' && typeof value.script === 'string') {
    if (!value.script) return null;
    return { script: value.script, cwd: value.cwd ? resolve(appDir, value.cwd) : appDir };
  }
  throw new Error(`a hook must be a string or { script, cwd }, not ${JSON.stringify(value)}`);
}

export function main(argv, { env = process.env, run = spawnSync, log = console.log, platform = process.platform } = {}) {
  const configs = [];
  let appDir;
  let print = false;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--app-dir') appDir = argv[++i];
    else if (argv[i] === '--config') configs.push(argv[++i]);
    else if (argv[i] === '--print') print = true;
    else {
      log(`::error::before-build.mjs: unknown option ${argv[i]}`);
      return 2;
    }
  }
  if (!appDir) {
    log('::error::before-build.mjs: --app-dir is required');
    return 2;
  }
  let build;
  try {
    ({ build } = effectiveBuild({ appDir, configs, platform }));
    if (hookOf(build.beforeBundleCommand, appDir)) {
      log('::error::build.beforeBundleCommand is set: tauri build would run it with the updater key in its environment (ADR 0085 D7). Next step: move it into beforeBuildCommand.');
      return 1;
    }
  } catch (e) {
    log(`::error::Reading the Tauri configuration in ${appDir} failed: ${e.message}. Next step: fix the file it names.`);
    return 1;
  }
  const hook = hookOf(build.beforeBuildCommand, appDir);
  if (print) {
    log(hook ? hook.script : '');
    return 0;
  }
  if (!hook) {
    log('before-build: no beforeBuildCommand to run.');
    return 0;
  }
  log(`before-build: running beforeBuildCommand \`${hook.script}\` in ${hook.cwd} (without the updater key)`);
  const started = Date.now();
  const r = run('bash', ['-c', hook.script], { cwd: hook.cwd, stdio: 'inherit', env: { ...env, TAURI_ENV_DEBUG: 'false' } });
  const secs = ((Date.now() - started) / 1000).toFixed(1);
  if (r.error || r.status !== 0) {
    log(`::error::beforeBuildCommand \`${hook.script}\` failed (exit ${r.status ?? r.error?.code}) after ${secs} s. Next step: the lines above name the failing step (the UI build or a sidecar build); run it in ${hook.cwd}.`);
    return r.status || 1;
  }
  log(`before-build: done in ${secs} s.`);
  return 0;
}

if (isMain(import.meta.url)) process.exit(main(process.argv.slice(2)));
