#!/usr/bin/env node
// Affected projects between two commits of the Paddock monorepo (ADR 0014 §6): CI's plan job and a local helper.
//
//   node scripts/affected.mjs <base> [<head>]        the plan as JSON on stdout (head defaults to HEAD)
//   node scripts/affected.mjs --files a,b,c          the same for an explicit list of changed files
//   node scripts/affected.mjs --all                  everything (what a tag, a nightly or a dispatch without a base runs)
//   node scripts/affected.mjs --ci                   CI: picks the base from the event (below), writes the job outputs
//   options: --explain   the reason for every project
//
// Projects: `crates` (the shared Cargo workspace), `ui` (the shared Angular workspace), `eslint`, `conformance`, `tools`
// (tools/, templates/ and the scripts' tests), `paddock-dev` (tools/paddock-dev/ and the ./dev wrapper), `icons`, `ci-image` and `app:<name>` for every apps/<name>/ with a
// paddock.ci.json. A changed file marks the project that owns it, then every project that reaches it through the graph,
// read from the working tree:
//   - Cargo path dependencies of every Cargo workspace (`cargo metadata --no-deps`, run inside the repo: cargo reads
//     .cargo/config.toml from the current folder upwards, and the crates name the registry defined there);
//   - npm dependencies by package name across every tracked package.json;
//   - a Tauri shell whose tauri.conf.json `build.frontendDist` points into another package (shell-demo embeds ui/dist);
//   - tsconfig*.json `paths`, angular.json and eslint.config.* references outside the package (an app that maps
//     @decryp7/paddock-ui to ui/dist has no npm edge to ui/; ui's ESLint config requires ../eslint).
// Rules for the rest: GLOBAL inputs mark everything; an app's release workflow marks that app; docs mark nothing but the
// link check, except DOC_INPUTS, markdown that code or tests read (scripts/affected.test.mjs greps for undeclared ones);
// the root package-lock.json marks the workspaces whose resolved packages changed; unknown root files mark everything.
//
// CI base (--ci, from the GitHub-style environment Gitea Actions sets):
//   pull_request         the merge base with the target branch
//   push to main         the newest commit with a green ci.yml push run on main that HEAD contains (Gitea API), so a red
//                        main is not forgotten by the next push; everything when there is none or the API cannot be read
//   workflow_dispatch    the `base` input when given, else everything
//   tags, schedule       everything
//   PLAN_APP=<app>       (ci.yml's `app` input, a release run's gates) only that app's legs, both halves, whatever the event
// No dependencies; Node 22+.
import { execFileSync } from 'node:child_process';
import { appendFileSync, readFileSync } from 'node:fs';
import { dirname, join, posix, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isMain } from '../conformance/src/util/is-main.mjs';

/**
 * `p` relative to `root`, always with `/` separators: every key of the graphs is a `/` path, and on Windows `path.relative`
 * returns `crates\\paddock-cli`, which matches none of them (the crate and its apps were then unreachable).
 * `rel` and `separator` are injectable so that a test can use `path.win32`.
 */
export function repoRelative(root, p, rel = relative, separator = sep) {
  return rel(root, p).split(separator).join('/');
}

export const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const git = (args, cwd = ROOT) => execFileSync('git', args, { cwd, encoding: 'utf8', maxBuffer: 1 << 28, stdio: ['ignore', 'pipe', 'pipe'] });

/** Inputs that every project's build or tests read: a change re-runs everything. */
export const GLOBAL = [
  /^\.gitea\//, // workflows (but see appOfWorkflow) and the shared actions every leg runs
  /^\.cargo\//,
  /^rust-toolchain(\.toml)?$/,
  /^package\.json$/,
  /^\.npmrc$/,
  /^\.nvmrc$/,
  /^\.gitattributes$/,
  /^scripts\/affected(\.test)?\.mjs$/,
  /^lint\//, // the shared clippy and workspace lint presets
  /^tsconfig\//, // the shared TypeScript options
  /^conformance\/(ci|release)\//, // the release and CI scripts the shared actions run (ADR 0014's scripts/release)
];

/**
 * The app a workflow belongs to, or null: `release-<app>.yml`, `<app>.yml` or `<app>-<anything>.yml` for a known app
 * (`debrief.yml`, `debrief-fuzz.yml`, `shell-demo-release.yml`). Such a workflow marks only its app.
 */
export function appOfWorkflow(path, appNames) {
  const m = /^\.gitea\/workflows\/([a-z0-9-]+)\.ya?ml$/.exec(path);
  if (!m) return null;
  const name = m[1].startsWith('release-') ? m[1].slice('release-'.length) : m[1];
  const hits = [...appNames].filter((a) => name === a || name.startsWith(`${a}-`)).sort((a, b) => b.length - a.length);
  return hits[0] ?? null;
}

/**
 * Files every app's playwright.config.ts imports by relative path (the e2e port and build guard of @decryp7/paddock-e2e): no
 * npm or tsconfig edge says so, so a change to one re-runs the UI half of every app.
 */
export const E2E_GUARD_INPUTS = /^ui\/projects\/paddock-e2e\/src\/(env|build-guard)\.ts$/;

/** Editor and VCS metadata: marks nothing. */
export const META = [/(^|\/)\.gitignore$/, /^\.editorconfig$/, /(^|\/)\.DS_Store$/, /^lefthook\.yml$/];

/** Documentation: marks only the link check... */
export const DOCS = [/\.md$/, /^docs\//, /^apps\/[^/]+\/docs\//, /^diagrams\//, /(^|\/)LICENSE[^/]*$/];

/**
 * ...except markdown that code or tests read, a declared contract: `re` matches the path, `projects` are what it marks
 * (default: its owner). scripts/affected.test.mjs greps the code for markdown it reads and fails on an undeclared file.
 */
export const DOC_INPUTS = [
  { re: /^(apps\/[^/]+|crates\/[^/]+|ui\/projects\/[^/]+)\/CHANGELOG\.md$/, why: "What's New (include_str!), release notes" },
  { re: /(^|\/)(resources|samples|src|tests|test|templates|e2e|fixtures)\/.*\.md$/, why: 'bundled, template or fixture markdown' },
  { re: /^docs\/rules\.md$/, projects: ['conformance', 'tools'], why: 'the rules block of every AGENTS.md (conformance sync, new-app)' },
  { re: /^docs\/blocks\//, projects: ['conformance', 'tools'], why: 'the shared doc blocks (conformance sync, release notes, new-app)' },
  { re: /^apps\/(shell-demo|workbench-demo)\/README\.md$/, projects: ['tools'], why: 'new-app.mjs copies its "Start a new app" section' },
  { re: /(^|\/)REVIEWED\.md$/, projects: (f) => (f.startsWith('apps/') ? owners(f) : ['conformance']), why: 'the image reviews conformance C24 checks' },
  { re: /^apps\/debrief\/docs\/ai\/sql-views\.md$/, why: 'debrief-ai tests/sql_tool.rs (from the import)' },
  { re: /^apps\/debrief\/docs\/testing\/quarantine\.md$/, why: 'scripts/ci/check-quarantine (from the import)' },
];

/** Top-level folders and the projects that own them (apps/<name>/ is app:<name>). */
const OWNERS = [
  [/^crates\//, ['crates']],
  [/^ui\//, ['ui']],
  [/^eslint\//, ['eslint']],
  [/^conformance\//, ['conformance']],
  [/^paddock\.(conformance\.json|waivers\.toml)$/, ['conformance']],
  [/^(tools\/paddock-dev\/|dev$|dev\.cmd$)/, ['paddock-dev']], // the developer tool (ADR 0022): its own three-OS leg
  [/^(tools|templates)\//, ['tools']],
  // The scripts' tests run in the ui job (`npm run test:scripts`) and in the tools job.
  [/^scripts\//, ['tools', 'ui']],
  [/^icons\//, ['icons']],
  [/^ci-image\//, ['ci-image', 'ui']], // scripts/ci-image.test.mjs checks the image against the lock
  [/^(renovate\/|renovate\.json$|CODEOWNERS$|AGENTS\.md$)/, ['tools']], // checked by scripts/affected.test.mjs
];

/** An app's `src-tauri/Cargo.toml` and `capabilities/`, which conformance C48 reads (the app itself is marked as their owner). */
export const APP_CAPABILITY_INPUT = /^apps\/[^/]+\/(app\/)?src-tauri\/(Cargo\.toml|capabilities\/)/;

/** Generated TypeScript and JSON of the shared crates, written by their tests (a `generated` folder under ui, `.ts` or `.json`). */
export const GENERATED = /^ui\/.*\/generated\/[^/]+\.(ts|json)$/;

/** The crate folders whose Rust sources name the generated file `f` (`generated/<name>`), for the crates' own tests. */
export function crateDirsNaming(f, cargo) {
  let hits = [];
  try {
    hits = git(['grep', '-l', '-F', `generated/${posix.basename(f)}`, '--', 'crates/**/*.rs'], ROOT).split('\n').filter(Boolean);
  } catch {
    // git grep exits 1 when nothing matches.
  }
  return [...new Set(hits.map((h) => nodeOf(h, cargo.keys())).filter((d) => d && cargo.get(d).workspace === 'crates'))];
}

/** The projects that own a path before dependencies, or null for an unknown root file. */
export function owners(path) {
  const m = /^apps\/([^/]+)\//.exec(path);
  if (m) return [`app:${m[1]}`];
  for (const [re, p] of OWNERS) if (re.test(path)) return p;
  return null;
}

/** The deepest graph node (crate or package folder) that contains a path, or null. */
export function nodeOf(path, dirs) {
  let best = null;
  for (const d of dirs) if ((path === d || path.startsWith(`${d}/`)) && (!best || d.length > best.length)) best = d;
  return best;
}

// ------------------------------------------------------------------------------------------------------------- graph

/** Tracked files of the working tree. */
export function trackedFiles(cwd = ROOT) {
  return git(['ls-files'], cwd).split('\n').filter(Boolean);
}

/** The apps: name -> their paddock.ci.json (the CI contract of a matrix leg), with `dir`. */
export function readApps(files, root = ROOT) {
  const apps = new Map();
  for (const f of files.filter((x) => /^apps\/[^/]+\/paddock\.ci\.json$/.test(x))) {
    const name = f.split('/')[1];
    const j = JSON.parse(readFileSync(join(root, f), 'utf8'));
    apps.set(name, { ...j, dir: `apps/${name}` });
  }
  return apps;
}

/** Rust crates: dir -> { name, workspace, deps: [dir] } from `cargo metadata --no-deps` of every workspace root. */
export function cargoGraph(files, root = ROOT) {
  const crates = new Map();
  const roots = files.filter((f) => f.endsWith('Cargo.toml') && /^\[workspace\]/m.test(readFileSync(join(root, f), 'utf8')));
  for (const m of roots) {
    let meta;
    try {
      meta = JSON.parse(
        execFileSync('cargo', ['metadata', '--format-version', '1', '--no-deps', '--offline', '--manifest-path', join(root, m)], {
          cwd: dirname(join(root, m)),
          encoding: 'utf8',
          maxBuffer: 1 << 28,
          stdio: ['ignore', 'pipe', 'pipe'],
        }),
      );
    } catch (e) {
      throw new Error(`cargo metadata failed for ${m}: ${String(e.stderr ?? e.message).trim().split('\n').slice(-3).join(' ')}`);
    }
    const workspace = dirname(m);
    for (const p of meta.packages) {
      const dir = repoRelative(root, dirname(p.manifest_path));
      const deps = p.dependencies.filter((d) => d.path).map((d) => repoRelative(root, d.path));
      crates.set(dir, { name: p.name, workspace, deps });
    }
  }
  return crates;
}

/** Path references outside a config file's folder: `"../../ui/dist/x"` or `'../eslint/src/index.cjs'`. */
const pathRefs = (text) => [...text.matchAll(/["']((?:\.\.\/)+[^"'*]+)/g)].map((m) => m[1]);

/** npm packages: dir -> { name, deps: [dir] }, plus the Tauri frontendDist and config-path edges. */
export function npmGraph(files, root = ROOT) {
  const pkgs = new Map();
  for (const f of files.filter((x) => x.endsWith('package.json') && x !== 'package.json')) {
    const j = JSON.parse(readFileSync(join(root, f), 'utf8'));
    pkgs.set(dirname(f), { name: j.name, raw: j, deps: [] });
  }
  const byName = new Map([...pkgs].map(([d, p]) => [p.name, d]));
  for (const p of pkgs.values()) {
    const names = Object.keys({ ...p.raw.dependencies, ...p.raw.devDependencies, ...p.raw.peerDependencies });
    p.deps = names.filter((n) => byName.has(n)).map((n) => byName.get(n));
    delete p.raw;
  }
  const edge = (from, to) => {
    if (to && from !== to && !pkgs.get(from).deps.includes(to)) pkgs.get(from).deps.push(to);
  };
  // A Tauri shell that embeds a UI built elsewhere depends on the package that builds it (its own node).
  for (const f of files.filter((x) => /(^|\/)tauri\.conf\.json$/.test(x))) {
    const dist = JSON.parse(readFileSync(join(root, f), 'utf8')).build?.frontendDist;
    if (typeof dist !== 'string' || /^https?:/.test(dist)) continue;
    const self = dirname(f);
    const target = nodeOf(repoRelative(root, resolve(root, self, dist)), pkgs.keys());
    if (!target || self === target || self.startsWith(`${target}/`)) continue;
    if (!pkgs.has(self)) pkgs.set(self, { name: self, deps: [] });
    edge(self, target);
  }
  // Path imports that bypass npm.
  for (const f of files.filter((x) => /(^|\/)(tsconfig[^/]*\.json|angular\.json|eslint\.config\.[cm]?js)$/.test(x))) {
    const from = nodeOf(f, pkgs.keys());
    if (!from) continue;
    for (const ref of pathRefs(readFileSync(join(root, f), 'utf8'))) edge(from, nodeOf(repoRelative(root, resolve(root, dirname(f), ref)), pkgs.keys()));
  }
  return pkgs;
}

/** Reverse closure: every node that reaches one of `start` through `graph` (start included). */
export function dependents(start, graph) {
  const rev = new Map();
  for (const [d, n] of graph) for (const t of n.deps) rev.set(t, [...(rev.get(t) ?? []), d]);
  const seen = new Set(start);
  const todo = [...start];
  while (todo.length) {
    for (const r of rev.get(todo.pop()) ?? []) {
      if (!seen.has(r)) {
        seen.add(r);
        todo.push(r);
      }
    }
  }
  return seen;
}

/** Workspaces whose resolved packages differ between two root package-lock.json texts (lock v3, node's lookup order). */
export function lockDiff(before, after) {
  const a = before ? (JSON.parse(before).packages ?? {}) : {};
  const b = after ? (JSON.parse(after).packages ?? {}) : {};
  const closure = (pk, ws) => {
    const out = new Map();
    const todo = [ws];
    while (todo.length) {
      const at = todo.pop();
      const e = pk[at];
      if (!e || out.has(at)) continue;
      out.set(at, `${e.version}|${e.integrity ?? e.resolved ?? ''}`);
      for (const n of Object.keys({ ...e.dependencies, ...e.devDependencies, ...e.peerDependencies, ...e.optionalDependencies })) {
        for (let base = at; ; base = base.includes('/node_modules/') ? base.slice(0, base.lastIndexOf('/node_modules/')) : '') {
          const cand = base ? `${base}/node_modules/${n}` : `node_modules/${n}`;
          if (pk[cand]) {
            todo.push(pk[cand].link ? pk[cand].resolved : cand);
            break;
          }
          if (!base) break;
        }
      }
    }
    return [...out].sort().join('\n');
  };
  const spaces = new Set([...Object.keys(a), ...Object.keys(b)].filter((k) => k && !k.includes('node_modules')));
  return [...spaces].filter((ws) => closure(a, ws) !== closure(b, ws)).sort();
}

// ------------------------------------------------------------------------------------------------------------- plan

const PROJECTS = ['crates', 'ui', 'eslint', 'conformance', 'tools', 'icons', 'ci-image', 'paddock-dev'];

/**
 * The plan for a list of changed files. `graph` is { cargo, npm, apps } from the readers above; `all` selects everything.
 * Result: { all, crates, ui, eslint, conformance, tools, icons, ciImage, paddockDev, docs, apps: [name], kinds: { app: ['rust','ui'] },
 * packages: { workspace: [crate] } } and `why` with `explain`.
 */
export function affected(files, { cargo, npm, apps }, { lockBefore = null, lockAfter = null, all = false, explain = false } = {}) {
  const why = new Map();
  const kinds = new Map();
  const appProjects = [...apps.keys()].map((a) => `app:${a}`);
  const everyProject = [...PROJECTS, ...appProjects];
  const mark = (p, reason) => why.set(p, [...(why.get(p) ?? []), reason]);
  const kind = (p, k) => p?.startsWith('app:') && kinds.set(p, new Set([...(kinds.get(p) ?? []), ...(k === 'all' ? ['rust', 'ui'] : [k])]));
  const projectOfDir = (d) => owners(`${d}/x`)?.[0] ?? null;
  let everything = all;
  let docs = false;
  const changedCrates = new Set();
  const changedPkgs = new Set();
  const markAll = (reason) => {
    everything = true;
    for (const p of everyProject) {
      mark(p, reason);
      kind(p, 'all');
    }
  };
  if (all) markAll('everything (no base)');

  for (const f of all ? [] : files) {
    const wf = appOfWorkflow(f, apps.keys());
    if (wf) {
      mark(`app:${wf}`, `its workflow ${f}`);
      kind(`app:${wf}`, 'all');
      continue;
    }
    if (META.some((r) => r.test(f))) continue;
    if (GLOBAL.some((r) => r.test(f))) {
      markAll(`global ${f}`);
      continue;
    }
    if (E2E_GUARD_INPUTS.test(f)) {
      for (const name of apps.keys()) {
        mark(`app:${name}`, `e2e guard ${f}`);
        kind(`app:${name}`, 'ui');
      }
    }
    if (f === 'package-lock.json') {
      for (const ws of lockDiff(lockBefore, lockAfter)) {
        for (const p of owners(`${ws}/x`) ?? []) {
          mark(p, `lock ${ws}`);
          kind(p, 'ui');
        }
        if (npm.has(ws)) changedPkgs.add(ws);
      }
      continue;
    }
    const input = DOC_INPUTS.find((d) => d.re.test(f));
    if (DOCS.some((r) => r.test(f)) && !input) {
      docs = true;
      continue;
    }
    const own = typeof input?.projects === 'function' ? input.projects(f) : (input?.projects ?? owners(f));
    if (!own) {
      markAll(`unknown root file ${f}`); // fail safe
      continue;
    }
    for (const p of own) mark(p, f);
    const c = nodeOf(f, cargo.keys());
    const n = nodeOf(f, npm.keys());
    if (c && own.includes(projectOfDir(c))) changedCrates.add(c);
    if (n && own.includes(projectOfDir(n))) changedPkgs.add(n);
    // Which half of an app: Rust when the deepest node is a crate (or a src-tauri file), UI when it is a package.
    // A declared doc input is read by either half (a CHANGELOG is What's New in Rust and in the UI): both.
    const half = input ? 'all' : c && !(n && n.length > c.length) ? 'rust' : n && !(c && c.length > n.length) && !/\/src-tauri\//.test(f) ? 'ui' : 'all';
    for (const p of own) kind(p, half);
    // A change to a shared workspace's own config (crates/Cargo.toml, crates/.config/...) reaches all its members.
    if (!c && own.includes('crates')) for (const [d, x] of cargo) if (x.workspace === 'crates') changedCrates.add(d);
    // A generated TypeScript or JSON file under ui/ is compared with its Rust source by a crate's test (PADDOCK_WRITE_TS):
    // a stale one must fail on the branch, so the crates that name the file are tested (all crates when none does).
    if (GENERATED.test(f)) {
      mark('crates', `generated file ${f}`);
      const owners_ = crateDirsNaming(f, cargo);
      for (const d of owners_.length ? owners_ : [...cargo].filter(([, x]) => x.workspace === 'crates').map(([d]) => d)) changedCrates.add(d);
    }
    // ui/ is one Angular workspace: a change anywhere in it can change every output built from it.
    if (own.includes('ui') && f.startsWith('ui/')) changedPkgs.add('ui');
    // An app's icons (icons/<app>/) are checked by its release facts; the pipeline (icons/tools/) feeds every app.
    if (f.startsWith('icons/')) {
      const sub = f.split('/')[1];
      const targets = apps.has(sub) ? [sub] : sub === 'tools' || sub === 'package.json' ? [...apps.keys()] : [];
      for (const a of targets) {
        mark(`app:${a}`, `icons ${f}`);
        kind(`app:${a}`, 'rust');
      }
    }
    // An app's Cargo dependencies and capabilities are read by conformance C48 (a shell app grants its dialog windows the
    // shell's dialog set), which runs in the conformance job.
    if (APP_CAPABILITY_INPUT.test(f)) mark('conformance', `C48 reads ${f}`);
    // The shared actions' scripts in conformance/ run in every app's gates.
    if (f.startsWith('conformance/') && !/^conformance\/(test|fixtures)\//.test(f)) {
      for (const a of apps.keys()) {
        mark(`app:${a}`, `conformance ${f}`);
        kind(`app:${a}`, 'all');
      }
    }
  }

  const crates = dependents(changedCrates, cargo);
  for (const d of crates) {
    if (changedCrates.has(d)) continue;
    const p = projectOfDir(d);
    mark(p, `cargo ${d} reaches ${[...changedCrates].sort().join(', ')}`);
    kind(p, 'rust');
  }
  for (const d of dependents(changedPkgs, npm)) {
    if (changedPkgs.has(d)) continue;
    const p = projectOfDir(d);
    mark(p, `npm ${d}`);
    // A shell that embeds a changed UI: both halves (the Rust build embeds the UI).
    kind(p, /src-tauri$/.test(d) ? 'all' : 'ui');
  }

  const set = new Set([...why.keys()].filter(Boolean));
  const packages = {};
  for (const d of crates) (packages[cargo.get(d).workspace] ??= []).push(cargo.get(d).name);
  for (const k of Object.keys(packages)) packages[k].sort();
  const out = {
    all: everything,
    crates: set.has('crates'),
    ui: set.has('ui'),
    eslint: set.has('eslint'),
    conformance: set.has('conformance'),
    tools: set.has('tools'),
    icons: set.has('icons'),
    ciImage: set.has('ci-image'),
    paddockDev: set.has('paddock-dev'),
    docs,
    apps: [...set].filter((p) => p.startsWith('app:') && apps.has(p.slice(4))).map((p) => p.slice(4)).sort(),
    kinds: Object.fromEntries([...kinds].filter(([p]) => apps.has(p.slice(4))).map(([p, k]) => [p.slice(4), [...k].sort()]).sort()),
    packages,
  };
  if (explain) out.why = Object.fromEntries([...why].filter(([p]) => p).sort());
  return out;
}

/** The runner that carries an app's UI half, prepare steps and checks (the others run only the Rust gates). */
export const LINUX = 'ubuntu-latest';

/**
 * The matrix legs of the affected apps: one object per app and operating system (paddock.ci.json `os`, default Linux only)
 * with the app's contract, the halves to run and the nextest filter (only when `narrow`: a pull request whose change
 * reaches part of the app's Cargo workspace; '' runs it all). The Linux leg runs everything the change reaches; a Windows or
 * macOS leg runs only the Rust gates, and only when the change reaches the Rust half.
 */
export function matrix(plan, { cargo, apps }, { narrow = false, webkit = false } = {}) {
  // An app with a workflow of its own (paddock.ci.json `workflow`) is not a leg: its workflow asks `--ci --app`. An app
  // with nothing to run (docs only: no Cargo workspace, no commands) is no leg either.
  const runs = (a) => !a.workflow && (a.cargo || a.prepare?.length || a.checks?.length || a.ui?.length);
  return plan.apps.filter((name) => runs(apps.get(name))).flatMap((name) => {
    const app = apps.get(name);
    const oses = app.os?.length ? app.os : [LINUX];
    if (!oses.includes(LINUX)) throw new Error(`apps/${name}/paddock.ci.json: "os" must include ${LINUX} (it carries the UI half and the checks)`);
    const cargoDir = app.cargo ? posix.join(app.dir, app.cargo) : '';
    const members = [...cargo.values()].filter((c) => c.workspace === cargoDir).map((c) => c.name);
    const hit = plan.packages[cargoDir] ?? [];
    const filter = narrow && !plan.all && hit.length && hit.length < members.length ? hit.map((n) => `package(=${n})`).join(' | ') : '';
    const rust = (plan.kinds[name] ?? []).includes('rust');
    const ui = (plan.kinds[name] ?? []).includes('ui');
    const legs = oses.filter((os) => os === LINUX || (rust && app.cargo)).map((os) => {
      const linux = os === LINUX;
      return {
        app: name,
        os,
        dir: app.dir,
        cargo: cargoDir,
        nextest_config: app.nextestConfig ?? '',
        prepare: linux ? (app.prepare ?? []).join('\n') : '',
        checks: linux ? (app.checks ?? []).join('\n') : '',
        rust_checks: linux ? (app.rustChecks ?? []).join('\n') : '',
        ui_checks: linux ? (app.ui ?? []).join('\n') : '',
        angular_dir: linux && app.angularDir ? posix.join(app.dir, app.angularDir) : '',
        last_packages: (app.lastPackages ?? []).join(' '),
        windows_clippy: app.windowsClippy ? 'true' : 'false',
        process_patterns: (app.processPatterns ?? []).join('\n'),
        test_browser_prefix: app.testBrowser?.envPrefix ?? '',
        test_browser_catalogue: app.testBrowser?.catalogue ?? '',
        test_browser_no_sandbox: linux && app.testBrowser?.noSandboxOnLinux ? 'true' : 'false',
        display: linux && app.display ? 'true' : 'false',
        rust,
        ui: linux && ui,
        filter,
        e2e_checks: '',
        e2e_shard: '',
        e2e_leg: '',
        e2e_artifacts: linux && ui ? (app.e2eArtifacts ?? '') : '',
      };
    });
    // paddock.ci.json `e2e` (commands) with `e2eShards` (default 1): the end-to-end tests are legs of their own on Linux, one per
    // shard `i/N` (the command reads $E2E_SHARD and $E2E_INDEX), running at the same time; the main leg keeps the other UI lines.
    const shards = ui && app.e2e?.length ? Math.max(1, app.e2eShards ?? 1) : 0;
    const base = legs.find((l) => l.os === LINUX);
    for (let i = 1; i <= shards; i += 1) {
      legs.push({
        ...base,
        prepare: '',
        checks: '',
        ui_checks: '',
        rust: false,
        ui: false,
        display: 'false',
        e2e_checks: app.e2e.join('\n'),
        e2e_shard: `${i}/${shards}`,
        e2e_leg: `e2e-${i}`,
      });
    }
    // `e2eWebkit` (commands): the WebKit end-to-end tests, a leg of their own when the run asks for WebKit (see wantsWebkit).
    if (ui && webkit && app.e2eWebkit?.length) {
      legs.push({ ...base, prepare: '', checks: '', ui_checks: '', rust: false, ui: false, display: 'false', e2e_checks: app.e2eWebkit.join('\n'), e2e_shard: '', e2e_leg: 'e2e-webkit' });
    }
    return legs;
  });
}

/**
 * Whether this run wants the WebKit tests: a push to main or a tag, the nightly, or a manual run that asked for them
 * (`PLAN_WEBKIT`, from ci.yml's `webkit` input). Never a plain pull request.
 */
export function wantsWebkit(env) {
  if (env.PLAN_WEBKIT) return true;
  const ref = env.GITHUB_REF ?? '';
  return env.GITHUB_EVENT_NAME === 'schedule' || (env.GITHUB_EVENT_NAME !== 'pull_request' && (ref === 'refs/heads/main' || ref.startsWith('refs/tags/')));
}

/**
 * The plan of one app only (`--ci` with PLAN_APP, from ci.yml's `app` input): that app's legs with both halves and no
 * other project. A release run gates a tag with exactly the legs the app's own changes would run (the app's own tag
 * workflow calls ci.yml for it), not the whole monorepo.
 */
export function onlyApp(plan, app) {
  return {
    ...plan,
    all: false,
    crates: false,
    ui: false,
    eslint: false,
    conformance: false,
    tools: false,
    icons: false,
    ciImage: false,
    paddockDev: false,
    docs: false,
    apps: [app],
    kinds: { [app]: ['rust', 'ui'] },
    packages: {},
  };
}

// ------------------------------------------------------------------------------------------------------------- base

/**
 * The base of a CI run, or null for everything. `runs` is the newest-first list of ci.yml runs from the Gitea API (only
 * read for a push to main); `contains(sha)` says whether HEAD contains a commit; `mergeBase(ref)` gives a pull request's.
 * Returns { base, reason }.
 */
export function chooseBase({ event, ref, inputBase, baseRef, mainRef = 'refs/heads/main', runs = null, contains, mergeBase }) {
  if (event === 'pull_request' || event === 'pull_request_target') {
    const b = mergeBase(baseRef);
    return b ? { base: b, reason: `merge base with ${baseRef}` } : { base: null, reason: `no merge base with ${baseRef}: everything` };
  }
  if (event === 'workflow_dispatch') return inputBase ? { base: inputBase, reason: `the base input ${inputBase}` } : { base: null, reason: 'a manual run without a base: everything' };
  if (event === 'push' && ref === mainRef) {
    if (!runs) return { base: null, reason: 'the CI runs could not be read: everything' };
    const green = runs.find((r) => r.conclusion === 'success' && r.event === 'push' && r.head_branch === 'main' && /^ci\.ya?ml@/.test(r.path ?? 'ci.yml@') && contains(r.head_sha));
    return green ? { base: green.head_sha, reason: `the last green ci run on main (${green.head_sha.slice(0, 10)}, run ${green.id})` } : { base: null, reason: 'no green ci run on main that HEAD contains: everything' };
  }
  return { base: null, reason: `${event} ${ref}: everything` };
}

/** The newest ci.yml runs on main (Gitea API), or null with a warning when they cannot be read. */
async function ciRuns(env, warn) {
  const api = `${env.GITHUB_SERVER_URL ?? 'https://dev.decryptology.net'}/api/v1/repos/${env.GITHUB_REPOSITORY ?? 'decryp7/paddock'}`;
  const out = [];
  try {
    for (let page = 1; page <= 4; page++) {
      const res = await fetch(`${api}/actions/runs?branch=main&event=push&limit=50&page=${page}`, {
        headers: env.PLAN_TOKEN ? { Authorization: `token ${env.PLAN_TOKEN}` } : {},
        signal: AbortSignal.timeout(15_000),
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const runs = (await res.json()).workflow_runs ?? [];
      out.push(...runs);
      if (runs.length < 50 || out.some((r) => r.conclusion === 'success' && /^ci\.ya?ml@/.test(r.path ?? ''))) break;
    }
    return out;
  } catch (e) {
    warn(`::warning::the CI runs of main could not be read (${e.message}); this run tests everything`);
    return null;
  }
}

// ------------------------------------------------------------------------------------------------------------- I/O

/** The plan between two commits (or for a file list) of the working tree. */
export function planFor({ base = null, head = 'HEAD', files = null, all = false, explain = false, root = ROOT }) {
  const tracked = trackedFiles(root);
  const graph = { cargo: cargoGraph(tracked, root), npm: npmGraph(tracked, root), apps: readApps(tracked, root) };
  let changed = files ?? [];
  let lockBefore = null;
  let lockAfter = null;
  if (!files && !all) {
    changed = git(['diff', '--name-only', '--no-renames', base, head], root).split('\n').filter(Boolean);
    if (changed.includes('package-lock.json')) {
      const show = (r) => {
        try {
          return git(['show', `${r}:package-lock.json`], root);
        } catch {
          return null;
        }
      };
      lockBefore = show(base);
      lockAfter = show(head);
    }
  }
  return { graph, changed, plan: affected(changed, graph, { lockBefore, lockAfter, all, explain }) };
}

/** A short Markdown summary of a plan for the job summary. */
export function summary(plan, reason, legs) {
  const yes = (b) => (b ? 'yes' : 'no');
  const lines = [
    '### Affected',
    '',
    `Base: ${reason}`,
    '',
    '| Project | Runs |',
    '|---|---|',
    ...['crates', 'ui', 'eslint', 'conformance', 'tools', 'paddockDev'].map((p) => `| ${p} | ${yes(plan[p])} |`),
    ...legs.map((l) => `| app ${l.app}${l.os === LINUX ? '' : ` (${l.os})`} | ${l.e2e_leg ? `${l.e2e_leg.replace('-', ' ')}${l.e2e_shard ? ` ${l.e2e_shard}` : ''}` : [l.rust && 'rust', l.ui && 'ui'].filter(Boolean).join(' + ') || 'checks only'}${l.filter ? ` (\`${l.filter}\`)` : ''} |`),
  ];
  if (plan.docs && !legs.length && !plan.crates && !plan.ui) lines.push('', 'Docs only: nothing to build.');
  return `${lines.join('\n')}\n`;
}

/**
 * CI: the plan for this event. Without `app`, the ci.yml job outputs and summary; with `app` (an app's own workflow,
 * `--ci --app debrief`), `true` or `false` on stdout for whether the change reaches that app, the log on stderr.
 */
async function ci(env, { app = null } = {}) {
  const log = app ? (l) => console.error(l) : (l) => console.log(l);
  const contains = (sha) => {
    try {
      git(['merge-base', '--is-ancestor', sha, 'HEAD']);
      return true;
    } catch {
      return false;
    }
  };
  const mergeBase = (b) => {
    try {
      return git(['merge-base', 'HEAD', `origin/${b}`]).trim();
    } catch {
      return null;
    }
  };
  const event = env.GITHUB_EVENT_NAME;
  const ref = env.GITHUB_REF;
  const runs = event === 'push' && ref === 'refs/heads/main' ? await ciRuns(env, log) : null;
  const { base, reason } = chooseBase({ event, ref, inputBase: env.PLAN_BASE, baseRef: env.GITHUB_BASE_REF, runs, contains, mergeBase });
  const planned = planFor({ base: env.PLAN_APP ? null : base, all: env.PLAN_APP ? true : !base, explain: true });
  const { graph } = planned;
  if (env.PLAN_APP && !graph.apps.has(env.PLAN_APP)) throw new Error(`unknown app ${env.PLAN_APP} (no apps/${env.PLAN_APP}/paddock.ci.json)`);
  const plan = env.PLAN_APP ? onlyApp(planned.plan, env.PLAN_APP) : planned.plan;
  const legs = matrix(plan, graph, { narrow: event === 'pull_request' && !env.PLAN_APP, webkit: wantsWebkit(env) });
  log(env.PLAN_APP ? `base: only ${env.PLAN_APP}, every leg (a tag's gates)` : `base: ${reason}`);
  log(JSON.stringify(plan, null, 2));
  if (app) {
    if (!graph.apps.has(app)) throw new Error(`unknown app ${app} (no apps/${app}/paddock.ci.json)`);
    const run = plan.apps.includes(app);
    log(run ? `${app}: affected (${(plan.kinds[app] ?? []).join(', ')})` : `${app}: not affected by this change`);
    if (env.GITHUB_STEP_SUMMARY) appendFileSync(env.GITHUB_STEP_SUMMARY, `### ${app}\n\nBase: ${reason}\n\n${run ? 'Affected: every job runs.' : 'Not affected: nothing to run.'}\n`);
    console.log(String(run));
    return;
  }
  const outputs = {
    crates: plan.crates,
    ui: plan.ui,
    eslint: plan.eslint,
    conformance: plan.conformance,
    tools: plan.tools,
    icons: plan.icons,
    paddock_dev: plan.paddockDev,
    apps: JSON.stringify(legs),
    any_app: legs.length > 0,
  };
  if (env.GITHUB_OUTPUT) appendFileSync(env.GITHUB_OUTPUT, Object.entries(outputs).map(([k, v]) => `${k}=${v}\n`).join(''));
  if (env.GITHUB_STEP_SUMMARY) appendFileSync(env.GITHUB_STEP_SUMMARY, summary(plan, reason, legs));
}

const USAGE = 'usage: affected.mjs <base> [<head>] | --files a,b | --all | --ci [--app <app>]  [--explain]';

/**
 * The command line: `--ci [--app x]`, `--files a,b`, `--all`, `--explain` and up to two positional commits, base then head.
 * Throws (the CLI prints the usage and the reason) for anything incomplete, unknown or contradictory, so a typo never
 * plans nothing.
 */
export function parseArgs(args) {
  const known = ['--files', '--all', '--ci', '--app', '--explain'];
  const valueOf = (name) => {
    const i = args.indexOf(name);
    if (i < 0) return { i, value: null };
    const value = args[i + 1];
    if (value === undefined || value === '' || value.startsWith('--')) throw new Error(`${name} needs a value`);
    return { i, value };
  };
  for (const a of args) if (a.startsWith('--') && !known.includes(a)) throw new Error(`unknown option ${a}`);
  for (const k of known) if (args.filter((a) => a === k).length > 1) throw new Error(`${k} given more than once`);
  const { i: fi, value: filesArg } = valueOf('--files');
  const { i: ai, value: app } = valueOf('--app');
  const ci = args.includes('--ci');
  const all = args.includes('--all');
  if (app !== null && !ci) throw new Error('--app is only valid with --ci');
  const valueIdx = new Set([fi, ai].filter((i) => i >= 0).map((i) => i + 1));
  const positional = args.filter((a, i) => !a.startsWith('--') && !valueIdx.has(i));
  const [base = null, head = 'HEAD'] = positional;
  if (ci && (filesArg !== null || all || positional.length)) throw new Error('--ci takes no commits, --files or --all');
  if (positional.length > 2) throw new Error(`too many commits: ${positional.join(' ')}`);
  if (filesArg !== null && (base !== null || all)) throw new Error('--files cannot be combined with a base or --all');
  if (all && base !== null) throw new Error('--all cannot be combined with a base');
  const files = filesArg === null ? null : filesArg.split(',').filter(Boolean);
  if (files && !files.length) throw new Error('--files needs at least one file');
  return { files, base, head, all, ci, app, explain: args.includes('--explain') };
}

if (isMain(import.meta.url)) {
  try {
    const { files, base, head, all, ci: isCi, app, explain } = parseArgs(process.argv.slice(2));
    if (isCi) await ci(process.env, { app });
    else {
      if (!files && !base && !all) throw new Error('no base, --files or --all given');
      const { plan } = planFor({ base, head, files, all, explain });
      process.stdout.write(`${JSON.stringify(plan)}\n`);
    }
  } catch (e) {
    console.error(`affected: ${e.message}\n${USAGE}`);
    process.exit(1);
  }
}
