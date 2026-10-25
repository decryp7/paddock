#!/usr/bin/env node
// Starts a new Paddock app from one of the demos (plan docs/plans/workbench.md §10, the starter for Pacenotes and
// Sighter): copies the demo's UI (ui/projects/<template>) and its Rust side (apps/<template>/src-tauri) into a new
// repository in the family layout (`app/` with the Angular workspace, `app/src-tauri/`), renamed, with the shell crates
// and the UI package taken from the registries instead of this repository. The templates are `shell-demo` (a plain
// shell app, the default) and `workbench-demo` (an IDE-style app: the dock, the Explorer, pop-out panels, the assistant).
//
//   node scripts/new-app.mjs <id> "<Name>" [--template shell-demo|workbench-demo] [--accent <accent>]
//                            [--dest <dir>] [--identifier <reverse.domain.id>]
//     --template  the demo to start from (default shell-demo)
//     --accent    the window accent (workbench-demo only; one of SU_ACCENTS, default graphite)
//     id          lower-case id: the app's folders, its tag prefix, its log target (`pacenotes`)
//     Name        the product name (`Pacenotes`)
//     --dest      where to write it (default: ../<id> next to this repository); must not exist or be empty
//     --identifier the bundle identifier (default: net.decryptology.<id>)
//
// The copy keeps the demo's sample pages, dialogs, services, jobs and settings (their ids start with `sd.`): they are
// the reference to replace one by one. The template's README lists the next steps (icon, CI, first page).
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { loadManifest } from '../conformance/src/sync/baseline.mjs';
import { fill } from '../conformance/src/sync/fill.mjs';
import { inspectBlock, loadBlocks } from '../conformance/src/sync/blocks.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

/**
 * What is never copied: build output, installed packages, Tauri's generated schemas, the demo's own lock file, and the
 * dialog command check's config (paddock-check-dialog-commands walks the shell's TypeScript sources, which the npm
 * package does not ship: outside this repository it cannot follow a dialog into the shell yet).
 */
const TEMPLATES = {
  'shell-demo': { id: 'shell-demo', title: 'Paddock Shell Demo', readme: 'apps/shell-demo/README.md', section: 'Start a new app from the demo' },
  'workbench-demo': { id: 'workbench-demo', title: 'Paddock Workbench Demo', readme: 'apps/workbench-demo/README.md', section: 'Start a new app from the workbench demo' },
};
/** The window accents an app can name (`SU_ACCENTS` of `@decryp7/paddock-ui`). */
const ACCENTS = ['debrief', 'pitcrew', 'pitwall', 'dyno', 'spotter', 'pacenotes', 'sighter', 'graphite'];

const SKIP = new Set(['node_modules', 'target', 'gen', 'dist', 'Cargo.lock', '.angular', 'paddock-dialog-commands.json']);

// ---------------------------------------------------------------------------------------------------------------- pure

/** Parses the command line; fails fast with what to fix. */
export function parseArgs(argv) {
  const positional = [];
  const opts = { dest: null, identifier: null, template: 'shell-demo', accent: 'graphite' };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--dest' || a === '--identifier' || a === '--template' || a === '--accent') {
      const v = argv[++i];
      if (!v) throw new Error(`${a} needs a value`);
      opts[a.slice(2)] = v;
    } else if (a.startsWith('--')) {
      throw new Error(`unknown option ${a} (use --template, --accent, --dest or --identifier)`);
    } else {
      positional.push(a);
    }
  }
  const [id, name] = positional;
  if (positional.length !== 2) throw new Error('usage: node scripts/new-app.mjs <id> "<Name>" [--template <demo>] [--accent <accent>] [--dest <dir>] [--identifier <id>]');
  if (!/^[a-z][a-z0-9-]{1,30}[a-z0-9]$/.test(id)) throw new Error(`the id ${JSON.stringify(id)} must be 3 to 32 lower-case letters, digits and dashes, starting with a letter`);
  if (!Object.hasOwn(TEMPLATES, opts.template)) throw new Error(`the template ${JSON.stringify(opts.template)} must be one of ${Object.keys(TEMPLATES).join(', ')}`);
  if (Object.hasOwn(TEMPLATES, id)) throw new Error(`choose an id of your own (${id} is a demo)`);
  if (!ACCENTS.includes(opts.accent)) throw new Error(`the accent ${JSON.stringify(opts.accent)} must be one of ${ACCENTS.join(', ')}`);
  if (!/^[\p{L}\p{N}][\p{L}\p{N} .-]{0,38}[\p{L}\p{N}]$|^[\p{L}\p{N}]$/u.test(name)) throw new Error(`the name ${JSON.stringify(name)} must be 1 to 40 letters, digits, spaces, dots and dashes`);
  const identifier = opts.identifier ?? `net.decryptology.${id}`;
  if (!/^[a-zA-Z][a-zA-Z0-9-]*(\.[a-zA-Z][a-zA-Z0-9-]*)+$/.test(identifier)) throw new Error(`the identifier ${JSON.stringify(identifier)} must be reverse-domain (net.example.app)`);
  return { id, name, identifier, dest: opts.dest, template: opts.template, accent: opts.accent };
}

/** The crate name and the Rust identifier of the app's library. */
export function crateNames(id) {
  return { crate: `${id}-app`, lib: `${id.replaceAll('-', '_')}_app` };
}

/**
 * The shell crates as registry dependencies: `{ path = "../../../crates/x", features = [...] }` becomes
 * `{ version = "<major.minor>", registry = "decryp7", features = [...] }` at the version this repository has.
 */
export function registryDependencies(cargoToml, versions) {
  return cargoToml.replace(/\{ path = "\.\.\/\.\.\/\.\.\/crates\/([a-z0-9-]+)"(, )?/g, (whole, crate, comma) => {
    const v = versions[crate];
    if (!v) throw new Error(`no version for the crate ${crate}`);
    const [major, minor] = v.split('.');
    return `{ version = "${major}.${minor}", registry = "decryp7"${comma ?? ''}`;
  });
}

/** The rewrites of the Rust side, file by file (path relative to src-tauri). Each `[from, to]` must match. */
export function rustRewrites({ id, name, identifier }, versions) {
  const { crate, lib } = crateNames(id);
  // The sample background agent (plan X4): its own crate, its executable, launchd label, service name and variable.
  const agent = `${id}-agent`;
  const agentLib = `${id.replaceAll('-', '_')}_agent`;
  const serviceName = name.replace(/[^A-Za-z0-9]/g, '') || id;
  const agentVar = `${id.toUpperCase().replaceAll('-', '_')}_AGENT`;
  const agentSpec = [
    ['"net.decryptology.paddock-shell-demo.agent"', JSON.stringify(`${identifier}.agent`)],
    ['"net.decryptology.paddock-shell-demo"', JSON.stringify(identifier)],
    ['"shell-demo-agent"', JSON.stringify(agent)],
    ['"paddock-shell-demo-agent"', JSON.stringify(agent)],
    ['"PaddockShellDemo"', JSON.stringify(serviceName)],
    ['"SHELL_DEMO_AGENT"', JSON.stringify(agentVar)],
    ['"Paddock Shell Demo agent"', JSON.stringify(`${name} agent`)],
    [/"The Paddock Shell Demo's sample background agent[^"]*"/, JSON.stringify(`${name}'s sample background agent (writes a heartbeat, nothing else).`)],
    ['"https://dev.decryptology.net/decryp7/paddock"', JSON.stringify(`https://dev.decryptology.net/decryp7/${id}`)],
    ['pub const APP_ID: &str = "shell-demo";', `pub const APP_ID: &str = "${id}";`],
    ['pub const APP_NAME: &str = "Paddock Shell Demo";', `pub const APP_NAME: &str = "${name}";`],
  ];
  return {
    'Cargo.toml': [
      [/^# Paddock Shell Demo:.*\n(#.*\n)*/m, `# ${name}: a Paddock app on the \`paddock-shell\` plugin, started from Paddock Shell Demo.\n`],
      ['name = "paddock-shell-demo"', `name = "${crate}"`],
      ['default-run = "paddock-shell-demo"', `default-run = "${crate}"`],
      ['name = "paddock_shell_demo"', `name = "${lib}"`],
      ['shell-demo-agent = { path = "agent" }', `${agent} = { path = "agent" }`],
      [/description = "Paddock Shell Demo:[^"]*"/, `description = "${name}"`],
      [/repository = "[^"]*"\n/, ''],
      ['# `shell-demo-v<version>` and CHANGELOG.md has its notes (What\'s New).', `# \`${id}-v<version>\` and CHANGELOG.md has its notes (What's New).`],
      [/# The shell crates from this repository \(path dependencies\)\.[^\n]*\n#[^\n]*\n/, '# The Paddock shell crates from the `decryp7` registry (crates/README.md in the Paddock repository).\n'],
      { all: (text) => registryDependencies(text, versions) },
    ],
    'src/main.rs': [['paddock_shell_demo::run();', `${lib}::run();`]],
    'build.rs': [['"shell-demo-agent"', `"${agent}"`]],
    'agent/Cargo.toml': [
      [/^# The Paddock Shell Demo's sample background agent/m, `# ${name}'s sample background agent (from Paddock Shell Demo's)`],
      ['name = "shell-demo-agent"', `name = "${agent}"`],
      ['name = "shell_demo_agent"', `name = "${agentLib}"`],
      [/description = "Paddock Shell Demo's sample background agent"/, `description = ${JSON.stringify(`${name}'s sample background agent`)}`],
      [/repository = "[^"]*"\n/, ''],
      { all: (text) => registryDependencies(text.replaceAll('path = "../../../../crates/', 'path = "../../../crates/'), versions) },
    ],
    'agent/src/lib.rs': agentSpec,
    'agent/src/main.rs': [['shell_demo_agent::', `${agentLib}::`]],
    'agent/sidecar.mjs': [
      ["'../../../../conformance/src/sidecar/sidecar.mjs'", "'@decryp7/paddock-conformance/sidecar'"],
      ['shell-demo-agent', agent],
    ],
    'macos/agent.plist': [
      ['net.decryptology.paddock-shell-demo.agent', `${identifier}.agent`],
      ['net.decryptology.paddock-shell-demo', identifier],
      ['shell-demo-agent', agent],
      ['SHELL_DEMO_AGENT', agentVar],
    ],
    'windows/installer-hooks.nsh': [['shell-demo-agent', agent], ['PaddockShellDemo', serviceName]],
    'tauri.macos.conf.json': [['net.decryptology.paddock-shell-demo.agent', `${identifier}.agent`]],
    'src/lib.rs': [
      [/^\/\/! Paddock Shell Demo:[^\n]*\n/m, `//! ${name}: the Paddock app shell with the sample engine of Paddock Shell Demo, to replace part by part:\n`],
      [/^\/\/! It is also the starter of new apps:[^\n]*\n/m, '//! It was started from Paddock Shell Demo (`node scripts/new-app.mjs` in the Paddock repository).\n'],
      ['pub use shell_demo_agent as agent;', `pub use ${agentLib} as agent;`],
      ['pub const APP_ID: &str = "shell-demo";', `pub const APP_ID: &str = "${id}";`],
      ['pub const APP_NAME: &str = "Paddock Shell Demo";', `pub const APP_NAME: &str = "${name}";`],
      ['"https://dev.decryptology.net/decryp7/paddock/releases"', `"https://dev.decryptology.net/decryp7/${id}/releases"`],
      ['.homepage("https://dev.decryptology.net/decryp7/paddock")', `.homepage("https://dev.decryptology.net/decryp7/${id}")`],
      ['include_str!("../../../../ui/projects/shell-demo/dialog-contracts.json")', 'include_str!("../../dialog-contracts.json")'],
      ['include_str!("../../../../ui/projects/shell-demo/paddock-manifest.json")', 'include_str!("../../paddock-manifest.json")'],
      ['include_str!("../../CHANGELOG.md")', 'include_str!("../../../CHANGELOG.md")'],
      ['include_str!("../../../../LICENSE-MIT")', 'include_str!("../../../LICENSE-MIT")'],
      ['include_str!("../../../../LICENSE-APACHE")', 'include_str!("../../../LICENSE-APACHE")'],
      [/option_env!\("PADDOCK_BUILD_(COMMIT|DATE)"\)/g, (_m, k) => `option_env!("${id.toUpperCase().replaceAll('-', '_')}_${k === 'DATE' ? 'BUILT' : k}")`],
    ],
    'src/tray.rs': [[/shell-demo-tray-/g, `${id}-tray-`]],
    'tests/app.rs': [
      ['"binaries/shell-demo-agent"', `"binaries/${agent}"`],
      ['use paddock_shell_demo::', `use ${lib}::`],
      ['paddock_shell_demo::tray::', `${lib}::tray::`],
      ['app_dir().join("../../../ui/projects/shell-demo")', 'app_dir().join("..")'],
      ['"cd apps/shell-demo/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test app"', '"cd app/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test app"'],
      ['app_dir().join("../CHANGELOG.md")', 'app_dir().join("../../CHANGELOG.md")'],
    ],
    'tauri.conf.json': [
      ['"productName": "Paddock Shell Demo"', `"productName": ${JSON.stringify(name)}`],
      ['"identifier": "net.decryptology.paddock-shell-demo"', `"identifier": ${JSON.stringify(identifier)}`],
      ['"frontendDist": "../../../ui/dist/shell-demo/browser"', '"frontendDist": "../dist/app/browser"'],
      ['"devUrl": "http://localhost:4301"', '"devUrl": "http://localhost:4200"'],
      ['"beforeDevCommand": "node src-tauri/agent/sidecar.mjs --dev && npm --prefix ../../ui run shell-demo"', '"beforeDevCommand": "node src-tauri/agent/sidecar.mjs --dev && npm run start"'],
      ['"beforeBuildCommand": "node src-tauri/agent/sidecar.mjs && npm --prefix ../../ui run shell-demo:build"', '"beforeBuildCommand": "node src-tauri/agent/sidecar.mjs && npm run build"'],
      ['"binaries/shell-demo-agent"', `"binaries/${agent}"`],
      ['"title": "Paddock Shell Demo"', `"title": ${JSON.stringify(name)}`],
      [/"shortDescription": "[^"]*"/, `"shortDescription": ${JSON.stringify(name)}`],
      [/"longDescription": "[^"]*"/, `"longDescription": ${JSON.stringify(`${name}, a Paddock app.`)}`],
    ],
    'deny.toml': [[/^# cargo-deny for Paddock Shell Demo \(run in apps\/shell-demo\/src-tauri[^\n]*\n#[^\n]*\n/m, `# cargo-deny for ${name} (run in app/src-tauri: cargo deny check).\n`]],
  };
}

/** The rewrites of the UI, file by file (path relative to the demo's project folder). */
export function uiRewrites({ id, name }) {
  return {
    'src/app/demo-name.ts': [["export const APP_NAME = 'Paddock Shell Demo';", `export const APP_NAME = ${JSON.stringify(name).replaceAll('"', "'")};`]],
    'src/app/demo-about.ts': [
      ["import shellPackage from '../../../paddock-ui/package.json';", "import appPackage from '../../package.json';"],
      ['/** The demo\'s version: the shell\'s own (its single source, rule 6; the e2e reads the same file). */', '/** The browser build\'s stand-in version: the UI package\'s (the desktop app answers its Cargo.toml version). */'],
      ['export const DEMO_VERSION: string = shellPackage.version;', 'export const DEMO_VERSION: string = appPackage.version;'],
      ["'https://dev.decryptology.net/decryp7/paddock/releases'", `'https://dev.decryptology.net/decryp7/${id}/releases'`],
    ],
    'src/app/generated/app-commands.ts': [['cd apps/shell-demo/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test app', 'cd app/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test app']],
    'src/index.html': [['<title>Paddock Shell Demo</title>', `<title>${name}</title>`], ['Starting shell demo&hellip;', `Starting ${name}&hellip;`]],
    'tsconfig.app.json': [
      ['"extends": "../../tsconfig.json"', '"extends": "./tsconfig.json"'],
      ['"outDir": "../../out-tsc/shell-demo"', '"outDir": "./out-tsc/app"'],
    ],
  };
}

/** The rewrites of the workbench demo's Rust side, file by file (path relative to src-tauri). Each `[from, to]` must match. */
export function workbenchRustRewrites({ id, name, identifier }, versions) {
  const { crate, lib } = crateNames(id);
  return {
    'Cargo.toml': [
      [/^# Paddock Workbench Demo:.*\n(#.*\n)*/m, `# ${name}: a Paddock IDE-style app on the \`paddock-shell\` and \`paddock-workbench\` plugins, started from Paddock Workbench Demo.\n`],
      ['name = "paddock-workbench-demo"', `name = "${crate}"`],
      ['default-run = "paddock-workbench-demo"', `default-run = "${crate}"`],
      ['name = "paddock_workbench_demo"', `name = "${lib}"`],
      [/description = "Paddock Workbench Demo:[^"]*"/, `description = "${name}"`],
      [/repository = "[^"]*"\n/, ''],
      ['`workbench-demo-v<version>` and CHANGELOG.md has its notes.', `\`${id}-v<version>\` and CHANGELOG.md has its notes.`],
      [/# The crates from this repository \(path dependencies\)\.[^\n]*\n#[^\n]*\n/, '# The Paddock crates from the `decryp7` registry (crates/README.md in the Paddock repository).\n'],
      { all: (text) => registryDependencies(text, versions) },
    ],
    'src/main.rs': [['paddock_workbench_demo::run();', `${lib}::run();`]],
    'src/lib.rs': [
      [/^\/\/! Paddock Workbench Demo: the workbench demo's UI \(`ui\/projects\/workbench-demo`\)/m, `//! ${name}: the UI (\`app/src\`, started from Paddock Workbench Demo)`],
      [/It is also the starter of new IDE-style apps \(README\.md, "Start a new app from the workbench demo"\)\./, 'It was started from Paddock Workbench Demo (`node scripts/new-app.mjs` in the Paddock repository).'],
      ['pub const APP_ID: &str = "workbench-demo";', `pub const APP_ID: &str = "${id}";`],
      ['pub const APP_NAME: &str = "Paddock Workbench Demo";', `pub const APP_NAME: &str = "${name}";`],
      ['`WORKBENCH_DEMO_HOME`', `\`${id.toUpperCase().replaceAll('-', '_')}_HOME\``],
      ['include_str!("../../../../ui/projects/workbench-demo/dialog-contracts.json")', 'include_str!("../../dialog-contracts.json")'],
    ],
    'tests/app.rs': [
      ['use paddock_workbench_demo::', `use ${lib}::`],
      ['app_dir().join("../../../ui/projects/workbench-demo")', 'app_dir().join("..")'],
      ['snapshot["app"], "Paddock Workbench Demo"', `snapshot["app"], ${JSON.stringify(name)}`],
    ],
    'tauri.conf.json': [
      ['"productName": "Paddock Workbench Demo"', `"productName": ${JSON.stringify(name)}`],
      ['"identifier": "net.decryptology.paddock-workbench-demo"', `"identifier": ${JSON.stringify(identifier)}`],
      ['"frontendDist": "../../../ui/dist/workbench-demo/browser"', '"frontendDist": "../dist/app/browser"'],
      ['"devUrl": "http://localhost:4302"', '"devUrl": "http://localhost:4200"'],
      ['"beforeDevCommand": "npm --prefix ../../ui run workbench-demo"', '"beforeDevCommand": "npm run start"'],
      ['"beforeBuildCommand": "npm --prefix ../../ui run workbench-demo:build"', '"beforeBuildCommand": "npm run build"'],
      ['"title": "Paddock Workbench Demo"', `"title": ${JSON.stringify(name)}`],
      [/"shortDescription": "[^"]*"/, `"shortDescription": ${JSON.stringify(name)}`],
      [/"longDescription": "[^"]*"/, `"longDescription": ${JSON.stringify(`${name}, a Paddock IDE-style app.`)}`],
    ],
    'deny.toml': [[/^# cargo-deny for Paddock Workbench Demo \(run in apps\/workbench-demo\/src-tauri[^\n]*\n/m, `# cargo-deny for ${name} (run in app/src-tauri: cargo deny check). The same policy as the Paddock crates;\n`]],
  };
}

/** The rewrites of the workbench demo's UI, file by file (path relative to the demo's project folder). */
export function workbenchUiRewrites({ name, accent }) {
  return {
    'src/app/params.ts': [
      ["else graphite (the demos' neutral accent)", `else ${accent}`],
      [": 'graphite';", `: '${accent}';`],
    ],
    'src/main.ts': [
      [' * The workbench reference app (ADR 0013 §4; plan W4, W8): every workbench contribution kind, in the browser against mock\n * backends and in the desktop app (apps/workbench-demo) against', ` * ${name}, started from Paddock Workbench Demo: every workbench contribution kind, in the browser against mock\n * backends and in the desktop app (app/src-tauri) against`],
    ],
    'src/index.html': [['<title>Paddock Workbench Demo</title>', `<title>${name}</title>`], ['Starting workbench demo&hellip;', `Starting ${name}&hellip;`]],
    'src/main-window.ts': [["'Workbench demo'", JSON.stringify(name).replaceAll('"', "'")]],
    'src/app/views/orientation-view.ts': [['<h1>Workbench demo</h1>', `<h1>${name}</h1>`]],
    'tsconfig.app.json': [
      ['"extends": "../../tsconfig.json"', '"extends": "./tsconfig.json"'],
      ['"outDir": "../../out-tsc/workbench-demo"', '"outDir": "./out-tsc/app"'],
    ],
    'tsconfig.spec.json': [
      ['"extends": "../../tsconfig.json"', '"extends": "./tsconfig.json"'],
      ['"outDir": "../../out-tsc/workbench-demo-spec"', '"outDir": "./out-tsc/spec"'],
    ],
  };
}

/** Applies `rules` to `text`; every rule must match (a rule that no longer matches is a starter that went stale). */
export function rewrite(file, text, rules) {
  let out = text;
  for (const rule of rules) {
    if (rule.all) {
      out = rule.all(out);
      continue;
    }
    const [from, to] = rule;
    const hit = typeof from === 'string' ? out.includes(from) : new RegExp(from.source, from.flags.replace('g', '')).test(out);
    if (!hit) throw new Error(`${file}: the starter's rewrite of ${String(from).slice(0, 80)} no longer matches the demo; update scripts/new-app.mjs`);
    out = typeof from === 'string' ? out.replaceAll(from, to) : out.replace(from, to);
  }
  return out;
}

/** The new app's Angular workspace (`app/package.json`) with the demo's dependencies and the Paddock packages. */
export function appPackageJson({ id, name, template = 'shell-demo' }, uiPackage, paddockVersion, tauriCli, hasUnitTests = true) {
  const deps = { ...uiPackage.dependencies, '@decryp7/paddock-ui': `^${paddockVersion}` };
  const dev = Object.fromEntries(
    // The unit tests' DOM (jsdom) stays when the app has unit tests.
    Object.entries(uiPackage.devDependencies).filter(([k]) => !['ng-packagr', 'sass', '@vitest/coverage-v8', ...(hasUnitTests ? [] : ['jsdom'])].includes(k)),
  );
  return {
    name: `${id}-app`,
    version: '0.1.0',
    private: true,
    description: `${name}: the UI (Angular, @decryp7/paddock-ui). Started from ${TEMPLATES[template].title}.`,
    scripts: {
      start: 'ng serve --port 4200',
      build: 'ng build',
      ...(hasUnitTests ? { test: 'ng test --watch=false' } : {}),
      e2e: 'playwright test --project=chromium',
      tauri: 'tauri',
    },
    dependencies: Object.fromEntries(Object.entries(deps).sort(([a], [b]) => a.localeCompare(b))),
    devDependencies: Object.fromEntries(
      Object.entries({ ...dev, '@decryp7/paddock-conformance': `^${paddockVersion}`, '@decryp7/paddock-e2e': `^${paddockVersion}`, '@tauri-apps/cli': tauriCli }).sort(([a], [b]) => a.localeCompare(b)),
    ),
  };
}

/**
 * The new app's end-to-end smoke suite (`app/playwright.config.ts`, `app/e2e/smoke.spec.ts`) on `@decryp7/paddock-e2e`:
 * the window starts without page errors, is axe-clean in both themes and has no horizontal scroll at 320 px (rule 18); a
 * workbench app also shows its dock and opens the command palette. The port comes from `PW_PORT` (one per worktree).
 */
export function e2eSmoke({ id, name, template = 'shell-demo' }) {
  const workbench = template === 'workbench-demo';
  const config = `import { paddockConfig } from '@decryp7/paddock-e2e';

// ${name}'s end-to-end specs (plan X39): Chromium in a branch (\`npm run e2e\`), WebKit too on the combined result and in CI.
export default paddockConfig({ app: '${id}', defaultPort: 4200, serve: (port) => \`npx ng serve --port \${port}\` });
`;
  const ready = workbench ? "page.locator('pd-dock [role=tab]').first()" : "page.locator('#pd-main')";
  const extra = workbench
    ? `
  // The command palette opens from the keyboard and lists the commands.
  const workbench = new WorkbenchApp(page);
  await workbench.palette.open();
  await expect(workbench.palette.options.first()).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(workbench.palette.dialog).toBeHidden();
`
    : '';
  const spec = `import { axeInThemes, expect, expectNoHorizontalScroll, test${workbench ? ', WorkbenchApp' : ''} } from '@decryp7/paddock-e2e';

// The smoke suite every app starts with (scripts/new-app.mjs): replace and extend it as the app grows.
test('starts without errors and is accessible in both themes', async ({ page, pageErrors }) => {
  await page.goto('/');
  await expect(${ready}).toBeVisible();
${extra}  expect(await axeInThemes(page)).toEqual([]);
  expect(pageErrors).toEqual([]);
});

test('has no horizontal scroll at 320 px', async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 640 });
  await page.goto('/');
  await expect(${ready}).toBeVisible();
  await expectNoHorizontalScroll(page, '320 px');
});
`;
  return { 'app/playwright.config.ts': config, 'app/e2e/smoke.spec.ts': spec };
}

/** The new app's `angular.json`: one application, built from the demo's settings, with the package's stylesheet. */
export function angularJson(demoProject, template = 'shell-demo') {
  const build = structuredClone(demoProject.architect.build);
  build.options.browser = 'src/main.ts';
  build.options.tsConfig = 'tsconfig.app.json';
  build.options.outputPath = 'dist/app';
  build.options.styles = ['node_modules/@decryp7/paddock-ui/css/paddock.css', 'src/styles.css'];
  if (template === 'workbench-demo') {
    // The dock's stylesheet and the pop-out drag ghost's static page (public/) come with the workbench.
    build.options.styles = ['node_modules/@decryp7/paddock-ui/css/paddock.css', 'node_modules/@decryp7/paddock-ui/css/controls.css', 'node_modules/dockview/dist/styles/dockview.css', 'src/styles.css'];
    build.options.assets = [{ glob: '**/*', input: 'public' }];
  }
  return {
    $schema: './node_modules/@angular/cli/lib/config/schema.json',
    version: 1,
    newProjectRoot: 'projects',
    projects: {
      app: {
        projectType: 'application',
        root: '',
        sourceRoot: 'src',
        prefix: demoProject.prefix,
        architect: {
          build,
          serve: {
            builder: demoProject.architect.serve.builder,
            configurations: { production: { buildTarget: 'app:build:production' }, development: { buildTarget: 'app:build:development' } },
            defaultConfiguration: 'development',
          },
          // The demo's unit tests (Vitest), when it has them: the app keeps them and adds its own.
          ...(demoProject.architect.test
            ? { test: { ...structuredClone(demoProject.architect.test), options: { ...demoProject.architect.test.options, tsConfig: 'tsconfig.spec.json', setupFiles: ['src/test-setup.ts'] } } }
            : {}),
        },
      },
    },
  };
}

/** The new app's `tsconfig.json`: the demo's compiler options without this repository's path mapping. */
export function tsconfigJson(uiTsconfig) {
  const { paths: _paths, ...compilerOptions } = uiTsconfig.compilerOptions;
  return { compileOnSave: false, compilerOptions, angularCompilerOptions: uiTsconfig.angularCompilerOptions };
}

/**
 * The repo root of a new app from Paddock's templates (plan X27, X43): the baseline config files, an AGENTS.md made of the
 * shared doc blocks, docs/suite-lessons.md, docs/releasing.md and the README skeleton. The generator's own tsconfig and the
 * Cargo profiles (the app has no root Cargo.toml yet) are not taken from the template.
 */
export function writeRepoRoot(dest, { id, name }) {
  const config = { app: id, appName: name, appDir: 'app' };
  const put = (path, text) => {
    const file = join(dest, path);
    if (existsSync(file)) return;
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, text);
  };
  for (const e of loadManifest(root)) {
    if (e.path === 'app/tsconfig.json' || e.path === 'Cargo.toml') continue;
    put(e.path, fill(e.text, config));
  }
  const registry = loadBlocks(root);
  for (const [file, names] of Object.entries(registry.defaultTargets)) {
    let text = file === 'AGENTS.md' ? `# ${name}: engineering rules for coding agents and contributors\n` : '';
    for (const n of names) text = inspectBlock(text, registry.blocks.find((b) => b.name === n), config).desired;
    put(file, text);
  }
  put('README.md', fill(readFileSync(join(root, 'docs', 'blocks', 'readme-skeleton.md'), 'utf8'), config));
}

/** The new app's changelog: one release, started from the demo. */
export function changelog(name, date, template = 'shell-demo') {
  const what = template === 'workbench-demo'
    ? 'the Paddock workbench with its sample views, perspectives, Explorer, commands and assistant tool'
    : 'the Paddock app shell with its sample pages, services, jobs, settings and dialogs';
  return `# Changelog\n\n## [0.1.0] - ${date}\n\n### Added\n\n- ${name}, started from ${TEMPLATES[template].title}: ${what}.\n`;
}

/** Text that still names the demo or this repository's layout (a rewrite the starter missed). */
export function leftovers(files, template = 'shell-demo') {
  const t = TEMPLATES[template].id;
  const bad = [
    new RegExp(`["']${TEMPLATES[template].title}["']`),
    new RegExp(`apps/${t}`),
    new RegExp(`ui/projects/${t}`),
    /\.\.\/\.\.\/\.\.\/(crates|ui)\//,
    new RegExp(`paddock_${t.replaceAll('-', '_')}`),
    new RegExp(`"${t}"`),
  ];
  const found = [];
  for (const [file, text] of files) {
    for (const re of bad) if (re.test(text)) found.push(`${file}: ${re}`);
  }
  return found;
}

// -------------------------------------------------------------------------------------------------------------- effects

function walk(dir, base = dir, out = []) {
  for (const entry of readdirSync(dir)) {
    if (SKIP.has(entry)) continue;
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) walk(p, base, out);
    else out.push(relative(base, p).split(sep).join('/'));
  }
  return out;
}

function copyTree(from, to, rules) {
  const written = new Map();
  const unused = new Set(Object.keys(rules));
  for (const file of walk(from)) {
    const target = join(to, file);
    mkdirSync(dirname(target), { recursive: true });
    const r = rules[file];
    if (r) {
      unused.delete(file);
      const text = rewrite(file, readFileSync(join(from, file), 'utf8'), r);
      writeFileSync(target, text);
      written.set(file, text);
    } else {
      cpSync(join(from, file), target);
      if (/\.(rs|ts|json|toml|html|md)$/.test(file)) written.set(file, readFileSync(target, 'utf8'));
    }
  }
  if (unused.size) throw new Error(`the demo has no ${[...unused].join(', ')} any more; update scripts/new-app.mjs`);
  return written;
}

function crateVersions() {
  const toml = readFileSync(join(root, 'crates', 'Cargo.toml'), 'utf8');
  return Object.fromEntries([...toml.matchAll(/^(paddock-[a-z-]+) = \{ path = "[^"]+", version = "([^"]+)"/gm)].map((m) => [m[1], m[2]]));
}

/** Writes the new app; returns its folder. */
export function createApp(input, today = new Date().toISOString().slice(0, 10)) {
  const args = { template: 'shell-demo', accent: 'graphite', ...input };
  const t = TEMPLATES[args.template];
  const workbench = args.template === 'workbench-demo';
  if (!t) throw new Error(`unknown template ${args.template}`);
  const dest = resolve(args.dest ?? join(root, '..', args.id));
  if (existsSync(dest) && readdirSync(dest).length) throw new Error(`${dest} exists and is not empty`);
  const ui = join(root, 'ui');
  const uiPackage = JSON.parse(readFileSync(join(ui, 'package.json'), 'utf8'));
  const paddockVersion = JSON.parse(readFileSync(join(ui, 'projects', 'paddock-ui', 'package.json'), 'utf8')).version;
  const tauriCli = JSON.parse(readFileSync(join(root, 'apps', t.id, 'package.json'), 'utf8')).devDependencies['@tauri-apps/cli'];
  const angular = JSON.parse(readFileSync(join(ui, 'angular.json'), 'utf8'));
  const uiTsconfig = JSON.parse(readFileSync(join(ui, 'tsconfig.json'), 'utf8'));
  const app = join(dest, 'app');
  mkdirSync(app, { recursive: true });
  try {
    const files = new Map();
    const uiRules = workbench ? workbenchUiRewrites(args) : uiRewrites(args);
    const rustRules = (workbench ? workbenchRustRewrites : rustRewrites)(args, crateVersions());
    for (const [f, text] of copyTree(join(ui, 'projects', t.id), app, uiRules)) files.set(`app/${f}`, text);
    for (const [f, text] of copyTree(join(root, 'apps', t.id, 'src-tauri'), join(app, 'src-tauri'), rustRules)) files.set(`app/src-tauri/${f}`, text);
    if (!workbench) {
      // The tray images carry the app's id (src/tray.rs names them).
      const trayDir = join(app, 'src-tauri', 'icons', 'tray');
      for (const f of readdirSync(trayDir)) cpSync(join(trayDir, f), join(trayDir, f.replace('shell-demo-', `${args.id}-`)));
      for (const f of readdirSync(trayDir)) if (f.startsWith('shell-demo-')) rmSync(join(trayDir, f));
    }
    const json = (p, v) => writeFileSync(join(dest, p), `${JSON.stringify(v, null, 2)}\n`);
    json('app/package.json', appPackageJson(args, uiPackage, paddockVersion, tauriCli, !!angular.projects[t.id].architect.test));
    json('app/angular.json', angularJson(angular.projects[t.id], args.template));
    json('app/tsconfig.json', tsconfigJson(uiTsconfig));
    mkdirSync(join(app, 'e2e'), { recursive: true });
    for (const [f, text] of Object.entries(e2eSmoke(args))) writeFileSync(join(dest, f), text);
    writeFileSync(join(dest, 'CHANGELOG.md'), changelog(args.name, today, args.template));
    writeRepoRoot(dest, args);
    writeFileSync(
      join(dest, '.gitignore'),
      '/app/node_modules/\n/app/dist/\n/app/.angular/\n/app/out-tsc/\n/app/src-tauri/target/\n/app/src-tauri/gen/\n/app/playwright-report/\n/app/blob-report/\n.DS_Store\n',
    );
    const found = leftovers(files, args.template);
    if (found.length) throw new Error(`the copy still names the demo:\n${found.join('\n')}`);
    return dest;
  } catch (e) {
    rmSync(dest, { recursive: true, force: true });
    throw e;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const args = parseArgs(process.argv.slice(2));
    const dest = createApp(args);
    console.log(`${args.name} is in ${dest}. Next: ${TEMPLATES[args.template].readme}, "${TEMPLATES[args.template].section}".`);
  } catch (e) {
    console.error(`new-app: ${e.message}`);
    process.exit(1);
  }
}
