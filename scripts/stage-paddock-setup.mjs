#!/usr/bin/env node
// Builds paddock-setup.exe for an app and stages it where the app's installer include embeds it (family ADR 0053 S7):
// `apps/<app>/app/src-tauri/windows/paddock-setup.exe`, beside the hooks that `File` it. Run by the release workflows before the
// Windows bundle (app-release.yml `windows-pre-bundle`). The program carries the app's identifier, product, main binary, version and
// update key (`PADDOCK_SETUP_*`, read by `option_env!` in crates/paddock-setup); the key is the updater's public key
// (TAURI_UPDATER_PUBKEY, which conformance C91 later compares with plugins.updater.pubkey). Without a key the program still
// builds and refuses `apply-update` ("Updates are not available in this build").
//
// Fails closed: without crates/paddock-setup in the tree (this branch alone) it stops with an error, so no installer is built
// without the program its hooks embed. The include also stops makensis when the file is not there.
//
//   node scripts/stage-paddock-setup.mjs --app dyno [--require-key] [--target x86_64-pc-windows-msvc] [--root <repo>]
//
// A release (a tag ref, or --require-key) without the key is an error: the program would refuse every update.
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isMain } from '../conformance/src/util/is-main.mjs';

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), '..');

/** The key without any whitespace (a secret pasted with a newline), as .gitea/actions/tauri-bundle/action.yml strips it. */
export function cleanKey(key) {
  return String(key ?? '').replace(/\s+/g, '');
}

/** A release build: a tag ref, or `--require-key` from the job that builds installers for one. */
export function isRelease(env, requireKey) {
  return requireKey || /^refs\/tags\//.test(env.GITHUB_REF ?? env.GITEA_REF ?? '');
}

/** The `PADDOCK_SETUP_*` build environment from the app's Tauri configuration and the updater's public key. */
export function setupEnv(conf, key) {
  for (const k of ['identifier', 'productName', 'mainBinaryName', 'version']) {
    if (typeof conf[k] !== 'string' || conf[k] === '') throw new Error(`tauri.conf.json has no ${k}`);
  }
  const env = {
    PADDOCK_SETUP_IDENTIFIER: conf.identifier,
    PADDOCK_SETUP_PRODUCT: conf.productName,
    PADDOCK_SETUP_MAIN: `${conf.mainBinaryName}.exe`,
    PADDOCK_SETUP_VERSION: conf.version,
  };
  if (key) env.PADDOCK_SETUP_UPDATE_KEY = key;
  return env;
}

/** The path of the paddock-setup binary in cargo's `--message-format=json` output. */
export function executableFrom(output) {
  for (const line of output.split('\n')) {
    let m;
    try {
      m = JSON.parse(line);
    } catch {
      continue;
    }
    if (m.reason === 'compiler-artifact' && m.target?.name === 'paddock-setup' && m.target.kind?.includes('bin') && m.executable) return m.executable;
  }
  throw new Error('cargo reported no paddock-setup binary');
}

export function parseArgs(argv) {
  const o = { app: undefined, target: undefined, root: undefined, requireKey: false };
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    if (k === '--require-key') {
      o.requireKey = true;
      continue;
    }
    if (!['--app', '--target', '--root'].includes(k)) throw new Error(`unknown option: ${k}`);
    if (argv[i + 1] === undefined) throw new Error(`${k} needs a value`);
    o[k.slice(2)] = argv[++i];
  }
  if (!o.app) throw new Error('--app <name> is needed');
  return o;
}

const defaultRun = (cmd, args, opts) => execFileSync(cmd, args, { ...opts, encoding: 'utf8', maxBuffer: 256 << 20, stdio: ['ignore', 'pipe', 'inherit'] });

/** Builds and stages the program; returns the staged path. `run`, `env` are injectable for the tests. */
export function stage({ app, target, root = REPO, run = defaultRun, env = process.env, requireKey = false }) {
  const src = join(root, 'apps', app, 'app', 'src-tauri');
  if (!existsSync(join(src, 'tauri.conf.json'))) throw new Error(`no such app: ${app} (${src}/tauri.conf.json)`);
  if (!existsSync(join(root, 'crates/paddock-setup/Cargo.toml'))) {
    throw new Error('crates/paddock-setup is not in this tree: no installer can be built without the program its hooks embed (family ADR 0053 S2/S3)');
  }
  const conf = JSON.parse(readFileSync(join(src, 'tauri.conf.json'), 'utf8'));
  const key = cleanKey(env.PADDOCK_SETUP_UPDATE_KEY || env.TAURI_UPDATER_PUBKEY);
  if (!key && isRelease(env, requireKey)) {
    throw new Error('no updater public key for a release build: paddock-setup.exe would refuse every update. Set the TAURI_UPDATER_PUBKEY secret (PADDOCK_TAURI_UPDATER_PUBKEY) for this job, then run it again.');
  }
  const buildEnv = { ...env, ...setupEnv(conf, key) };
  const args = ['build', '-p', 'paddock-setup', '--release', '--message-format=json-render-diagnostics'];
  if (target) args.push('--target', target);
  console.log(`::group::paddock-setup for ${app}: cargo ${args.join(' ')}`);
  const out = run('cargo', args, { cwd: join(root, 'crates'), env: buildEnv });
  console.log('::endgroup::');
  const exe = executableFrom(out);
  const dest = join(src, 'windows', 'paddock-setup.exe');
  mkdirSync(dirname(dest), { recursive: true });
  copyFileSync(exe, dest);
  console.log(`staged ${exe} -> ${dest}`);
  return dest;
}

if (isMain(import.meta.url)) {
  try {
    stage(parseArgs(process.argv.slice(2)));
  } catch (e) {
    console.error(`::error::${e.message}`);
    process.exit(1);
  }
}
