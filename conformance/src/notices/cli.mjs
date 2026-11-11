// `paddock-notices`: regenerates (or with --check, verifies) THIRD_PARTY_NOTICES.md.
//
//   paddock-notices --app Pitcrew [--root .] [--manifest app/src-tauri/Cargo.toml ...] [--npm app] [--out app/src-tauri/resources ...]
//                   [--rust-json cargo-about.json ...] [--about about.toml] [--check]
//   paddock-notices --config apps/<app>/paddock.ci.json [--root <repo>] [--check]
//
// --config reads the app's `notices` block ({ name, manifests, npm, out, about, rustJson, components }; `components` are markdown files for bundled software that is neither a crate nor an npm package, appended verbatim; paths relative to the app's
// folder, the folder of that file) so an app states its facts once and CI and developers run the same command. The npm
// folder may be a workspace of the root lock above it (the monorepo). Without an `about.toml` of its own, the app uses the
// shared one (templates/repo/about.toml), so the accepted licences are one list.
//
// Rust licences come from `cargo about generate` (run once per --manifest, with a timeout) unless --rust-json files are
// given. The npm side reads package-lock.json and node_modules of --npm (run `npm ci --omit=dev --ignore-scripts` first).
// Exit status: 0 written or up to date, 1 --check found a stale file or a non-permissive licence, 2 usage or tool error.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { lf, nonPermissive, npmPackages, render, rustCrates } from './generate.mjs';

const MANY = new Set(['manifest', 'out', 'rust-json']);
const ONE = new Set(['app', 'root', 'npm', 'about', 'config']);
/** The shared cargo-about configuration (its `accepted` list equals deny.toml's allow list; a test keeps them equal). */
const SHARED_ABOUT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'templates', 'repo', 'about.toml');
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
  if (!o.app && !o.config) throw new Error('--app <product name> or --config <paddock.ci.json> is needed');
  return o;
}

/** Runs `cargo about generate` for each manifest and returns the parsed JSON documents. `exec` is injectable. */
export function cargoAbout({ manifests, aboutToml, root, exec = execFileSync }) {
  const tmp = mkdtempSync(join(tmpdir(), 'paddock-notices-'));
  try {
    return manifests.map((manifest, i) => {
      const out = join(tmp, `${i}.json`);
      try {
        exec('cargo', ['about', 'generate', '--locked', '-c', aboutToml, '--format', 'json', '--manifest-path', manifest, '-o', out], { cwd: root, stdio: ['ignore', 'pipe', 'pipe'], timeout: TIMEOUT_MS });
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
    let base = root;
    let name = o.app;
    let npmRel = o.npm ?? 'app';
    let outRel = o.out;
    let manifestRel = o.manifest;
    let jsonRel = o['rust-json'];
    let aboutRel = o.about;
    let componentRel = [];
    if (o.config) {
      const file = resolve(root, o.config);
      const n = JSON.parse(readFileSync(file, 'utf8')).notices;
      if (!n) throw new Error(`${o.config} has no "notices" block ({ "name": "<product>", "manifests": [...], "npm": "app", "out": [...] })`);
      base = dirname(file);
      name = o.app ?? n.name;
      if (!name) throw new Error(`${o.config}: notices.name (the product name) is needed`);
      npmRel = o.npm ?? n.npm ?? 'app';
      outRel = o.out.length ? o.out : n.out ?? [];
      manifestRel = o.manifest.length ? o.manifest : n.manifests ?? ['app/src-tauri/Cargo.toml'];
      jsonRel = o['rust-json'].length ? o['rust-json'] : n.rustJson ?? [];
      aboutRel = o.about ?? n.about;
      componentRel = n.components ?? [];
    }
    const npmDir = resolve(base, npmRel);
    const outs = (outRel.length ? outRel : ['app/src-tauri/resources']).map((d) => resolve(base, d));
    const manifests = manifestRel.length ? manifestRel : existsSync(join(base, 'app/src-tauri/Cargo.toml')) ? ['app/src-tauri/Cargo.toml'] : [];
    const aboutToml = aboutRel ? resolve(base, aboutRel) : existsSync(join(base, 'about.toml')) ? join(base, 'about.toml') : SHARED_ABOUT;
    let docs;
    if (jsonRel.length) docs = jsonRel.map((f) => JSON.parse(readFileSync(resolve(base, f), 'utf8')));
    else if (manifests.length) docs = cargoAbout({ manifests: manifests.map((m) => resolve(base, m)), aboutToml, root: base, exec });
    else throw new Error('no Rust manifest: pass --manifest <Cargo.toml> or --rust-json <file>');
    const rust = rustCrates(docs, base);
    const npm = npmPackages(npmDir);
    const { markdown, counts } = render(rust.packages, npm.packages, name, componentRel.map((f) => readFileSync(resolve(base, f), 'utf8')));
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
