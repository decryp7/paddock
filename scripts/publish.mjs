#!/usr/bin/env node
// Publishes every Paddock package: the crates to the Gitea cargo registry `decryp7`, the npm packages
// (@decryp7/paddock-ui with all its entry points, @decryp7/paddock-e2e) to the Gitea npm registry, then the Gitea release.
//
//   node scripts/publish.mjs               dry run: builds the npm packages and lists every crate, npm package and entry
//                                          point with what a tag would publish; uploads nothing
//   node scripts/publish.mjs --skip-build  dry run on the existing ui/dist (CI, after its build steps)
//   node scripts/publish.mjs --offline     dry run without asking the registries which versions they already have
//   node scripts/publish.mjs --publish     only in CI on a v* tag (.gitea/workflows/publish.yml, rule 9); refuses elsewhere
//
// Held back, never published: crates and npm packages still marked as scaffolds (`[package.metadata.paddock] scaffold =
// true` in Cargo.toml, `"paddock": { "scaffold": true }` in package.json; the work package that fills one removes the
// marker), crates with `publish = false`, and versions the registry already has (a crate bumps its own version only when
// it changes, ADR 0006). The npm packages carry the tag's version (lockstep with @decryp7/paddock-ui).
//
// Auth (publish only): CARGO_REGISTRIES_DECRYP7_TOKEN, else PACKAGE_AUTH (base64 of user:password, basic auth; CI builds it
// from the DOCKER_* secrets), else PACKAGE_TOKEN (a token with write:package). GITEA_TOKEN creates the release. Secrets
// go into throw-away files or the child's environment and are masked in all output.
import { execFileSync, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const REGISTRY_NAME = 'decryp7';
export const CARGO_INDEX = 'https://dev.decryptology.net/api/packages/decryp7/cargo/';
export const NPM_REGISTRY = 'https://dev.decryptology.net/api/packages/decryp7/npm/';
const GITEA_API = 'https://dev.decryptology.net/api/v1/repos/decryp7/paddock';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const crates = join(root, 'crates');
const ui = join(root, 'ui');

// ---------------------------------------------------------------------------------------------------------------- pure

/** Parses the command line; unknown flags fail fast. */
export function parseArgs(argv) {
  const known = new Set(['--publish', '--skip-build', '--offline']);
  const unknown = argv.filter((a) => !known.has(a));
  if (unknown.length) throw new Error(`unknown option ${unknown.join(' ')} (use --publish, --skip-build or --offline)`);
  const publish = argv.includes('--publish');
  if (publish && (argv.includes('--offline') || argv.includes('--skip-build'))) {
    throw new Error('--publish always builds and asks the registries; --skip-build and --offline are for the dry run');
  }
  return { publish, skipBuild: argv.includes('--skip-build'), offline: argv.includes('--offline') };
}

/** The path of a crate's file in a cargo (sparse) index: `1/a`, `2/ab`, `3/a/abc`, `pa/dd/paddock-jobs`. */
export function indexPath(name) {
  const n = name.toLowerCase();
  if (n.length === 0) throw new Error('empty crate name');
  if (n.length === 1) return `1/${n}`;
  if (n.length === 2) return `2/${n}`;
  if (n.length === 3) return `3/${n[0]}/${n}`;
  return `${n.slice(0, 2)}/${n.slice(2, 4)}/${n}`;
}

/** cargo-hakari's crate (crates/.config/hakari.toml): never published; `cargo hakari publish` drops the dependency on it. */
export const WORKSPACE_HACK = 'paddock-workspace-hack';

/** The workspace's crates from `cargo metadata --no-deps`, with their dependencies on each other (dev-dependencies excluded: cargo strips path-only ones). */
export function workspaceCrates(metadata) {
  const members = new Set(metadata.workspace_members);
  const pkgs = metadata.packages.filter((p) => members.has(p.id) && p.name !== WORKSPACE_HACK);
  const names = new Set(pkgs.map((p) => p.name));
  return pkgs
    .map((p) => ({
      name: p.name,
      version: p.version,
      publish: p.publish,
      scaffold: p.metadata?.paddock?.scaffold === true,
      deps: [...new Set(p.dependencies.filter((d) => d.kind !== 'dev' && names.has(d.name)).map((d) => d.name))].sort(),
      dir: dirname(p.manifest_path),
    }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

/** Crates whose `publish` would send them anywhere but the `decryp7` registry (no `publish` at all means crates.io). */
export function crateProblems(list) {
  const problems = [];
  for (const c of list) {
    const ok = Array.isArray(c.publish) && (c.publish.length === 0 || (c.publish.length === 1 && c.publish[0] === REGISTRY_NAME));
    if (!ok) problems.push(`${c.name}: set publish = ["${REGISTRY_NAME}"] (or publish = false) in its Cargo.toml; now ${JSON.stringify(c.publish)}`);
  }
  return problems;
}

/** Dependency order (a crate after everything it depends on; ties by name), so each publish finds its dependencies in the registry. */
export function publishOrder(list) {
  const byName = new Map(list.map((c) => [c.name, c]));
  const done = new Set();
  const order = [];
  while (order.length < list.length) {
    const ready = list.filter((c) => !done.has(c.name) && c.deps.every((d) => done.has(d) || !byName.has(d)));
    if (ready.length === 0) {
      const left = list.filter((c) => !done.has(c.name)).map((c) => c.name);
      throw new Error(`dependency cycle between ${left.join(', ')}`);
    }
    for (const c of ready.sort((a, b) => a.name.localeCompare(b.name))) {
      done.add(c.name);
      order.push(c);
    }
  }
  return order;
}

/** Secondary entry points of an ng-packagr library: every folder below `libRoot` with an ng-package.json, as `shell/startup`. */
export function entryPoints(libRoot) {
  const found = [];
  const walk = (dir) => {
    for (const e of readdirSync(dir, { withFileTypes: true })) {
      if (!e.isDirectory() || e.name === 'node_modules' || e.name.startsWith('.')) continue;
      const sub = join(dir, e.name);
      if (existsSync(join(sub, 'ng-package.json'))) found.push(relative(libRoot, sub).split('\\').join('/'));
      walk(sub);
    }
  };
  walk(libRoot);
  return found.sort();
}

/** Entry points that the built package does not export (ng-packagr writes one `./<entry>` export per entry point). */
export function missingExports(entries, distPackageJson) {
  const exported = new Set(Object.keys(distPackageJson.exports ?? {}));
  return entries.filter((e) => !exported.has(`./${e}`));
}

/** The npm packages in ui/projects (every `@decryp7/*` package.json that is not private), built into ui/dist/<folder>. */
export function npmPackages(projectsDir, distDir) {
  return readdirSync(projectsDir, { withFileTypes: true })
    .filter((e) => e.isDirectory() && existsSync(join(projectsDir, e.name, 'package.json')))
    .map((e) => ({ folder: e.name, pkg: JSON.parse(readFileSync(join(projectsDir, e.name, 'package.json'), 'utf8')) }))
    .filter(({ pkg }) => typeof pkg.name === 'string' && pkg.name.startsWith('@decryp7/') && !pkg.private)
    .map(({ folder, pkg }) => ({ name: pkg.name, version: pkg.version, scaffold: pkg.paddock?.scaffold === true, dist: join(distDir, folder) }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

/** The npm packages release in lockstep: each carries @decryp7/paddock-ui's version. */
export function lockstepProblems(packages) {
  const lead = packages.find((p) => p.name === '@decryp7/paddock-ui');
  if (!lead) return ['@decryp7/paddock-ui is missing from ui/projects'];
  return packages.filter((p) => p.version !== lead.version).map((p) => `${p.name} is ${p.version}; npm packages carry @decryp7/paddock-ui's version ${lead.version}`);
}

/**
 * The shell plugin crate and the npm package are one version across the boundary (ADR 0012 §3): the UI checks
 * `shell_version` against its own at start-up, so `paddock-shell` carries @decryp7/paddock-ui's version.
 */
export function shellVersionProblems(crateList, packages) {
  const shell = crateList.find((c) => c.name === 'paddock-shell');
  const lead = packages.find((p) => p.name === '@decryp7/paddock-ui');
  if (!shell || !lead || shell.version === lead.version) return [];
  return [`paddock-shell is ${shell.version}; it carries @decryp7/paddock-ui's version ${lead.version} (bump both together)`];
}

/** Publishing runs only in CI on the tag of this version (rule 9). */
export function tagProblems(env, version) {
  const problems = [];
  if (env.CI !== 'true' && env.GITEA_ACTIONS !== 'true') problems.push('--publish runs only in CI (publish.yml on a v* tag), never on a workstation');
  const ref = env.GITHUB_REF ?? '';
  if (ref !== `refs/tags/v${version}`) problems.push(`--publish needs the tag v${version} (GITHUB_REF is "${ref}")`);
  return problems;
}

/** Workspace crates that have no clippy and test (nextest) step in ci.yml's per-crate job. */
export function crateStepsMissing(ciYaml, names) {
  return names.filter((n) => {
    const esc = n.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    return !new RegExp(`cargo clippy -p ${esc} [^\\n]*--all-features`).test(ciYaml) || !new RegExp(`cargo nextest run -p ${esc} --all-features`).test(ciYaml);
  });
}

/** What a run does with each crate and npm package. `published` maps a name to the versions the registry has, or is null (not asked). */
export function planRows(crateList, packages, published) {
  const decide = (name, version, scaffold, publishable) => {
    if (!publishable) return { action: 'skip', reason: 'publish = false' };
    if (scaffold) return { action: 'skip', reason: 'scaffold (held back until its work package lands)' };
    if (!published) return { action: 'publish', reason: 'registry not asked (--offline)' };
    const have = published.get(name);
    if (!have) return { action: 'publish', reason: 'registry not reachable' };
    if (have.has(version)) return { action: 'skip', reason: 'already published' };
    return { action: 'publish', reason: have.size ? `registry has ${[...have].sort().join(', ')}` : 'new' };
  };
  return [
    ...crateList.map((c) => ({ kind: 'crate', name: c.name, version: c.version, ...decide(c.name, c.version, c.scaffold, c.publish.length > 0) })),
    ...packages.map((p) => ({ kind: 'npm', name: p.name, version: p.version, ...decide(p.name, p.version, p.scaffold, true) })),
  ];
}

/** A plain text table. */
export function formatTable(rows) {
  const cols = ['kind', 'name', 'version', 'action', 'reason'];
  const width = cols.map((c) => Math.max(c.length, ...rows.map((r) => String(r[c]).length)));
  const line = (r) => cols.map((c, i) => String(r[c]).padEnd(width[i])).join('  ').trimEnd();
  return [line(Object.fromEntries(cols.map((c) => [c, c]))), ...rows.map(line)].join('\n');
}

/** Versions in a sparse-index file (one JSON object per line). */
export function indexVersions(text) {
  return new Set(text.split('\n').filter((l) => l.trim()).map((l) => JSON.parse(l).vers));
}

/** The Authorization value for the cargo registry from the environment, or null. */
export function cargoToken(env) {
  if (env.CARGO_REGISTRIES_DECRYP7_TOKEN) return env.CARGO_REGISTRIES_DECRYP7_TOKEN;
  if (env.PACKAGE_AUTH) return `Basic ${env.PACKAGE_AUTH}`;
  if (env.PACKAGE_TOKEN) return `Bearer ${env.PACKAGE_TOKEN}`;
  return null;
}

/** Replaces every secret in text with ***. */
export function mask(text, secrets) {
  return secrets.filter(Boolean).reduce((t, s) => t.split(s).join('***'), text);
}

// ---------------------------------------------------------------------------------------------------------------- I/O

const run = (cmd, args, cwd) => execFileSync(cmd, args, { cwd, stdio: 'inherit' });
const capture = (cmd, args, cwd) => execFileSync(cmd, args, { cwd, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });

/** Versions each registry already has: Map name → Set, a missing name when the registry could not be asked. */
async function registryVersions(crateList, packages, required) {
  const out = new Map();
  const ask = async (name, url, parse) => {
    try {
      const res = await fetch(url, { signal: AbortSignal.timeout(15_000) });
      if (res.status === 404) out.set(name, new Set());
      else if (res.ok) out.set(name, parse(await res.text()));
      else throw new Error(`HTTP ${res.status}`);
    } catch (e) {
      if (required) throw new Error(`cannot ask the registry about ${name}: ${e.message}`);
      console.warn(`warning: cannot ask the registry about ${name} (${e.message}); assuming it is not published`);
    }
  };
  for (const c of crateList) await ask(c.name, CARGO_INDEX + indexPath(c.name), indexVersions);
  for (const p of packages) {
    await ask(p.name, NPM_REGISTRY + p.name.replace('/', '%2f'), (t) => new Set(Object.keys(JSON.parse(t).versions ?? {})));
  }
  return out;
}

function prepareNpm(skipBuild) {
  if (!skipBuild) {
    run('npm', ['run', 'build'], ui);
    run('npm', ['run', 'build:e2e'], ui);
  }
  const dist = join(ui, 'dist/paddock-ui');
  if (!existsSync(join(dist, 'package.json'))) throw new Error('ui/dist/paddock-ui is not built (run without --skip-build)');
  for (const f of ['LICENSE-MIT', 'LICENSE-APACHE']) copyFileSync(join(root, f), join(dist, f));
  copyFileSync(join(ui, 'README.md'), join(dist, 'README.md'));
  return dist;
}

function listCrate(c) {
  const files = capture('cargo', ['package', '-p', c.name, '--list', '--allow-dirty', '--registry', REGISTRY_NAME], crates)
    .split('\n')
    .filter(Boolean);
  console.log(`\ncrate ${c.name} ${c.version} (${files.length} files${c.deps.length ? `; after ${c.deps.join(', ')}` : ''})`);
  for (const f of files) console.log(`  ${f}`);
}

function listNpm(p) {
  if (!existsSync(join(p.dist, 'package.json'))) throw new Error(`${p.name} is not built: ${relative(root, p.dist)} is missing`);
  const [info] = JSON.parse(capture('npm', ['pack', p.dist, '--dry-run', '--json', '--ignore-scripts'], ui));
  console.log(`\nnpm ${p.name} ${p.version} (${info.entryCount} files, ${info.size} bytes packed)`);
  for (const f of info.files) console.log(`  ${f.path}`);
}

function publishCrate(c, token) {
  // `cargo hakari publish` removes the dependency on the workspace-hack crate for the upload and puts it back. That edits a
  // manifest, so `--locked` cannot be passed here: main() checks the lock file before anything is published.
  const res = spawnSync('cargo', ['hakari', 'publish', '-p', c.name, '--', '--registry', REGISTRY_NAME], {
    cwd: crates,
    encoding: 'utf8',
    env: { ...process.env, CARGO_REGISTRIES_DECRYP7_TOKEN: token },
  });
  process.stdout.write(mask(`${res.stdout}${res.stderr}`, [token]));
  if (res.status !== 0) throw new Error(`cargo publish ${c.name} failed (exit ${res.status})`);
  console.log(`published crate ${c.name} ${c.version}`);
}

function publishNpm(p, env) {
  const basic = env.PACKAGE_AUTH;
  const token = env.PACKAGE_TOKEN;
  if (!basic && !token) throw new Error('PACKAGE_AUTH (basic) or PACKAGE_TOKEN is not set: it needs the write:package scope');
  const dir = mkdtempSync(join(tmpdir(), 'paddock-npm-'));
  const npmrc = join(dir, 'npmrc');
  const host = NPM_REGISTRY.replace(/^https:\/\//, '');
  writeFileSync(npmrc, `@decryp7:registry=${NPM_REGISTRY}\n//${host}:${basic ? `_auth=${basic}` : `_authToken=${token}`}\n`, { mode: 0o600 });
  try {
    const res = spawnSync('npm', ['publish', p.dist, '--userconfig', npmrc, '--registry', NPM_REGISTRY], { cwd: ui, encoding: 'utf8' });
    process.stdout.write(mask(`${res.stdout}${res.stderr}`, [basic, token]));
    if (res.status !== 0) throw new Error(`npm publish ${p.name} failed (exit ${res.status})`);
    console.log(`published ${p.name}@${p.version}`);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

async function createRelease(version, published, env) {
  const token = env.GITEA_TOKEN || env.PACKAGE_TOKEN;
  const headers = token ? { Authorization: `token ${token}` } : { Authorization: `Basic ${env.PACKAGE_AUTH}` };
  const tag = `v${version}`;
  let res = await fetch(`${GITEA_API}/releases/tags/${tag}`, { headers });
  if (res.ok) return console.log(`release ${tag} already exists`);
  const list = published.length ? published.map((r) => `- ${r.kind === 'crate' ? 'crate' : 'npm'} \`${r.name}\` ${r.version}`).join('\n') : '- nothing new';
  res = await fetch(`${GITEA_API}/releases`, {
    method: 'POST',
    headers: { ...headers, 'Content-Type': 'application/json' },
    body: JSON.stringify({
      tag_name: tag,
      name: tag,
      body: `Published to the Gitea registries (cargo \`${REGISTRY_NAME}\`: ${CARGO_INDEX}, npm: ${NPM_REGISTRY}):\n\n${list}\n\nSee CHANGELOG.md.`,
    }),
  });
  if (!res.ok) throw new Error(`create release failed: ${res.status} (the token needs write:repository)`);
  console.log(`created release ${tag}`);
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const env = process.env;

  // The lock file is current (publishCrate cannot pass --locked): cargo metadata fails when it would have to change it.
  if (opts.publish) capture('cargo', ['metadata', '--format-version', '1', '--locked'], crates);
  const metadata = JSON.parse(capture('cargo', ['metadata', '--format-version', '1', '--no-deps'], crates));
  const crateList = publishOrder(workspaceCrates(metadata));
  const packages = npmPackages(join(ui, 'projects'), join(ui, 'dist'));
  const lead = packages.find((p) => p.name === '@decryp7/paddock-ui');
  const ciYaml = readFileSync(join(root, '.gitea/workflows/ci.yml'), 'utf8');
  const problems = [
    ...crateProblems(crateList),
    ...lockstepProblems(packages),
    ...shellVersionProblems(crateList, packages),
    ...crateStepsMissing(ciYaml, crateList.map((c) => c.name)).map((n) => `${n} has no clippy and test step in .gitea/workflows/ci.yml (job crate)`),
    ...(opts.publish ? tagProblems(env, lead?.version) : []),
  ];
  if (problems.length) throw new Error(`refusing to continue:\n  ${problems.join('\n  ')}`);

  prepareNpm(opts.skipBuild);
  const entries = entryPoints(join(ui, 'projects/paddock-ui'));
  const distPkg = JSON.parse(readFileSync(join(ui, 'dist/paddock-ui/package.json'), 'utf8'));
  const missing = missingExports(entries, distPkg);
  if (missing.length) throw new Error(`the built @decryp7/paddock-ui does not export: ${missing.join(', ')}`);

  for (const c of crateList) if (c.publish.length) listCrate(c);
  for (const p of packages) listNpm(p);
  console.log(`\nnpm @decryp7/paddock-ui entry points (${entries.length + 1}):\n  @decryp7/paddock-ui\n${entries.map((e) => `  @decryp7/paddock-ui/${e}`).join('\n')}`);

  const published = opts.offline ? null : await registryVersions(crateList, packages, opts.publish);
  const rows = planRows(crateList, packages, published);
  console.log(`\n${opts.publish ? 'Publishing' : 'Dry run (nothing is published)'}, in publish order:\n${formatTable(rows)}`);

  if (!opts.publish) {
    const n = rows.filter((r) => r.action === 'publish').length;
    return console.log(`\ndry run: a tag v${lead.version} would publish ${n} of ${rows.length}; nothing was uploaded`);
  }
  const token = cargoToken(env);
  const todo = rows.filter((r) => r.action === 'publish');
  if (todo.some((r) => r.kind === 'crate') && !token) throw new Error('no cargo registry token (CARGO_REGISTRIES_DECRYP7_TOKEN, PACKAGE_AUTH or PACKAGE_TOKEN)');
  for (const r of todo.filter((r) => r.kind === 'crate')) publishCrate(crateList.find((c) => c.name === r.name), token);
  for (const r of todo.filter((r) => r.kind === 'npm')) publishNpm(packages.find((p) => p.name === r.name), env);
  await createRelease(lead.version, todo, env);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((e) => {
    console.error(`publish: ${e.message}`);
    process.exit(1);
  });
}
