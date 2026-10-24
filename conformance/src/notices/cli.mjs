// `paddock-notices`: regenerates (or with --check, verifies) THIRD_PARTY_NOTICES.md.
//
//   paddock-notices --app Pitcrew [--root .] [--manifest app/src-tauri/Cargo.toml ...] [--npm app] [--out app/src-tauri/resources ...]
//                   [--rust-json cargo-about.json ...] [--about about.toml] [--check]
//
// Rust licences come from `cargo about generate` (run once per --manifest, with a timeout) unless --rust-json files are
// given. The npm side reads package-lock.json and node_modules of --npm (run `npm ci --omit=dev --ignore-scripts` first).
// Exit status: 0 written or up to date, 1 --check found a stale file or a non-permissive licence, 2 usage or tool error.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { lf, nonPermissive, npmPackages, render, rustCrates } from './generate.mjs';

const MANY = new Set(['manifest', 'out', 'rust-json']);
const ONE = new Set(['app', 'root', 'npm', 'about']);
const TIMEOUT_MS = 10 * 60 * 1000;

/** Parses the arguments; repeated --manifest/--out/--rust-json collect into arrays. Throws on anything unknown. */
export function parseArgs(argv) {
  const o = { manifest: [], out: [], 'rust-json': [], check: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (!a.startsWith('--')) throw new Error(`unexpected argument: ${a}`);
    const k = a.slice(2);
    if (k === 'check') o.check = true;
    else if (MANY.has(k) || ONE.has(k)) {
      if (argv[i + 1] === undefined || argv[i + 1].startsWith('--')) throw new Error(`--${k} needs a value`);
      if (MANY.has(k)) o[k].push(argv[++i]);
      else o[k] = argv[++i];
    } else throw new Error(`unknown option: ${a}`);
  }
  if (!o.app) throw new Error('--app <product name> is needed');
  return o;
}

/** Runs `cargo about generate` for each manifest and returns the parsed JSON documents. `exec` is injectable. */
export function cargoAbout({ manifests, aboutToml, root, exec = execFileSync }) {
  const tmp = mkdtempSync(join(tmpdir(), 'paddock-notices-'));
  try {
    return manifests.map((manifest, i) => {
      const out = join(tmp, `${i}.json`);
      try {
        exec('cargo', ['about', 'generate', '-c', aboutToml, '--format', 'json', '--manifest-path', manifest, '-o', out], { cwd: root, stdio: ['ignore', 'pipe', 'pipe'], timeout: TIMEOUT_MS });
      } catch (e) {
        if (e.code === 'ENOENT') throw new Error('cargo is not installed');
        const why = `${e.stderr ?? ''}`.trim() || e.message;
        if (/no such command|not found/i.test(why)) throw new Error('cargo-about is not installed: cargo install cargo-about --locked --features cli');
        throw new Error(`cargo about failed for ${manifest}: ${why.split('\n').slice(-3).join(' ')}`);
      }
      return JSON.parse(readFileSync(out, 'utf8'));
    });
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}

/** Entry point; returns the exit status. */
export function main(argv, { log = console.log, err = console.error, exec } = {}) {
  try {
    const o = parseArgs(argv);
    const root = resolve(o.root ?? '.');
    const npmDir = resolve(root, o.npm ?? 'app');
    const outs = (o.out.length ? o.out : ['app/src-tauri/resources']).map((d) => resolve(root, d));
    const manifests = o.manifest.length ? o.manifest : existsSync(join(root, 'app/src-tauri/Cargo.toml')) ? ['app/src-tauri/Cargo.toml'] : [];
    let docs;
    if (o['rust-json'].length) docs = o['rust-json'].map((f) => JSON.parse(readFileSync(resolve(root, f), 'utf8')));
    else if (manifests.length) docs = cargoAbout({ manifests: manifests.map((m) => resolve(root, m)), aboutToml: resolve(root, o.about ?? 'about.toml'), root, exec });
    else throw new Error('no Rust manifest: pass --manifest <Cargo.toml> or --rust-json <file>');
    const rust = rustCrates(docs);
    const npm = npmPackages(npmDir);
    const { markdown, counts } = render(rust.packages, npm.packages, o.app);
    for (const w of [...rust.warnings, ...npm.warnings]) err(`warning: ${w}`);
    const bad = nonPermissive([...rust.packages, ...npm.packages]);
    for (const b of bad) err(`NON-PERMISSIVE: ${b}`);
    log(`notices: ${counts.rust} Rust crates, ${counts.npm} npm packages, ${counts.texts} distinct licence texts, ${counts.tally.size} licence expressions`);
    if (o.check) {
      let stale = 0;
      for (const dir of outs) {
        const file = join(dir, 'THIRD_PARTY_NOTICES.md');
        const have = existsSync(file) ? lf(readFileSync(file, 'utf8')) : null;
        if (have !== markdown) {
          err(`${file} is ${have === null ? 'missing' : 'out of date'}: run paddock-notices (without --check) and commit the result`);
          stale++;
        }
      }
      return stale || bad.length ? 1 : 0;
    }
    for (const dir of outs) {
      mkdirSync(dir, { recursive: true });
      writeFileSync(join(dir, 'THIRD_PARTY_NOTICES.md'), markdown);
      log(`wrote ${join(dir, 'THIRD_PARTY_NOTICES.md')}`);
    }
    return bad.length ? 1 : 0;
  } catch (e) {
    err(`paddock-notices: ${e.message}`);
    return 2;
  }
}
