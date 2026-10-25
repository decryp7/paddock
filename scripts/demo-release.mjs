#!/usr/bin/env node
// The release steps of Paddock's demo apps (plan docs/plans/workbench.md §10, S18, X50 W8), one script for all of them (rule
// 42): used by .gitea/workflows/<app>-release.yml on an `<app>-v*` tag. An app's tags are its own: the packages and crates
// release from `v*` (publish.yml). A new demo app is one row of APPS.
//
//   node scripts/demo-release.mjs <app> version                  the app's version (apps/<app>/src-tauri/Cargo.toml)
//   node scripts/demo-release.mjs <app> check-tag <tag>          the tag names that version (`<app>-v0.1.0`)
//   node scripts/demo-release.mjs <app> icons                    the app's icons are the icon pipeline's (icons/<app>)
//   node scripts/demo-release.mjs <app> notes <out.md>           the version's CHANGELOG section and the First launch notes
//   node scripts/demo-release.mjs <app> publish <tag> <dir> <notes.md>
//                                                                only in CI (refuses elsewhere): the Gitea release with
//                                                                every file of <dir> as an asset (GITEA_TOKEN)
//   node scripts/demo-release.mjs <app> sidecars                 the app's sidecar binaries, one per line (verify-macos.sh)
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isMain } from '../conformance/src/util/is-main.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const GITEA_API = 'https://dev.decryptology.net/api/v1/repos/decryp7/paddock';

/** The demo apps: folder name (also the icon folder and the tag prefix), display name, sidecars that start with `--self-test`. */
export const APPS = {
  'shell-demo': { id: 'shell-demo', title: 'Shell Demo', sidecars: ['shell-demo-agent'] },
  'workbench-demo': { id: 'workbench-demo', title: 'Workbench Demo', sidecars: [] },
};

/** The app of a table row, or an error naming the known ones. */
export function appConfig(id) {
  const app = Object.hasOwn(APPS, id ?? '') ? APPS[id] : undefined;
  if (!app) throw new Error(`unknown app ${JSON.stringify(id ?? '')} (${Object.keys(APPS).join(', ')})`);
  return app;
}

/** The tag prefix of an app's releases. */
export const tagPrefix = (app) => `${app.id}-v`;

const RELEASE = /^\d+\.\d+\.\d+$/;

// ---------------------------------------------------------------------------------------------------------------- pure

/** The `[package]` version of a Cargo.toml (a release number), or an error naming what is wrong. */
export function cargoVersion(text) {
  const version = /^\[package\][^[]*?^version\s*=\s*"([^"]+)"/m.exec(text)?.[1];
  if (!version || !RELEASE.test(version)) throw new Error(`Cargo.toml: no release-number version in [package] (found ${JSON.stringify(version ?? null)})`);
  return version;
}

/** Problems of a release tag against the app's version: empty when the tag is `<app>-v<version>`. */
export function tagProblems(app, tag, version) {
  const prefix = tagPrefix(app);
  if (!tag.startsWith(prefix)) return [`the tag ${tag} is not a ${app.title} tag (${prefix}<version>)`];
  const named = tag.slice(prefix.length);
  if (!RELEASE.test(named)) return [`the tag ${tag} does not name a release number`];
  return named === version ? [] : [`the tag ${tag} names ${named}, but apps/${app.id}/src-tauri/Cargo.toml has ${version}`];
}

/**
 * The notes of `version` from a Keep a Changelog file: the lines under its `## [version]` heading up to the next release
 * heading, trimmed. An error when the version has no section (a release always says what changed).
 */
export function changelogSection(changelog, version) {
  const lines = changelog.split(/\r?\n/);
  const head = (l) => /^##\s/.test(l);
  const start = lines.findIndex((l) => head(l) && new RegExp(`^##\\s+\\[?v?${version.replaceAll('.', '\\.')}\\]?(\\s|$)`).test(l));
  if (start < 0) throw new Error(`CHANGELOG.md has no section for ${version}`);
  const end = lines.findIndex((l, i) => i > start && head(l));
  const body = lines.slice(start + 1, end < 0 ? undefined : end).join('\n').trim();
  if (!body) throw new Error(`CHANGELOG.md's section for ${version} is empty`);
  return body;
}

/** What every release says while the app is not signed (rule 25): how to open it the first time. */
export const firstLaunch = (app) => `### First launch

Paddock ${app.title} is not code-signed yet, so your system may warn the first time you open it.

- **macOS:** if it says the app cannot be opened, choose Done, then System Settings > Privacy & Security > scroll to the message about Paddock ${app.title} > **Open Anyway**.
- **Windows:** if SmartScreen says "Windows protected your PC", choose **More info > Run anyway**.
- **Linux:** install the .deb with your package manager, or make the AppImage executable and run it.

This build cannot update itself: download the next release from this page.`;

/** The release notes: the version's changes, then the first-launch notes. */
export function releaseNotes(app, changelog, version) {
  return `${changelogSection(changelog, version)}\n\n${firstLaunch(app)}\n`;
}

/** Files of `expected` (name to bytes) that `actual` lacks or holds different bytes of. */
export function iconProblems(app, expected, actual) {
  const out = [];
  for (const [name, bytes] of expected) {
    const have = actual.get(name);
    if (!have) out.push(`${name} is missing from apps/${app.id}/src-tauri/icons`);
    else if (!have.equals(bytes)) out.push(`${name} differs from icons/${app.id}/tauri (copy the pipeline's output: icons/README.md)`);
  }
  return out;
}

// -------------------------------------------------------------------------------------------------------------- effects

const appDir = (app) => join(root, 'apps', app.id);

function readFiles(dir, filter = () => true) {
  return new Map(
    readdirSync(dir)
      .filter((f) => statSync(join(dir, f)).isFile() && filter(f))
      .map((f) => [f, readFileSync(join(dir, f))]),
  );
}

/** The app's version, from its single source. */
export function appVersion(app) {
  return cargoVersion(readFileSync(join(appDir(app), 'src-tauri', 'Cargo.toml'), 'utf8'));
}

async function publish(app, tag, dir, notesFile, env) {
  if (env.GITEA_ACTIONS !== 'true' && env.CI !== 'true') throw new Error(`publish runs only in CI from a ${tagPrefix(app)}* tag (rule 9)`);
  const problems = tagProblems(app, tag, appVersion(app));
  if (problems.length) throw new Error(problems.join('\n'));
  const token = (env.GITEA_TOKEN ?? '').replace(/\s+/g, '');
  if (!token) throw new Error('GITEA_TOKEN is not set (a token with write:repository)');
  const headers = { Authorization: `token ${token}` };
  let res = await fetch(`${GITEA_API}/releases/tags/${tag}`, { headers });
  let release;
  if (res.ok) {
    release = await res.json();
    console.log(`release ${tag} exists; adding its missing files`);
  } else {
    res = await fetch(`${GITEA_API}/releases`, {
      method: 'POST',
      headers: { ...headers, 'Content-Type': 'application/json' },
      body: JSON.stringify({ tag_name: tag, name: `Paddock ${app.title} ${tag.slice(tagPrefix(app).length)}`, body: readFileSync(notesFile, 'utf8') }),
    });
    if (!res.ok) throw new Error(`creating the release failed: ${res.status} (the token needs write:repository)`);
    release = await res.json();
  }
  const have = new Set((release.assets ?? []).map((a) => a.name));
  const files = readFiles(dir);
  if (files.size === 0) throw new Error(`no installers in ${dir}`);
  for (const [name, bytes] of files) {
    if (have.has(name)) continue;
    const form = new FormData();
    form.append('attachment', new Blob([bytes]), name);
    const up = await fetch(`${GITEA_API}/releases/${release.id}/assets?name=${encodeURIComponent(name)}`, { method: 'POST', headers, body: form });
    if (!up.ok) throw new Error(`uploading ${name} failed: ${up.status}`);
    console.log(`uploaded ${name} (sha256 ${createHash('sha256').update(bytes).digest('hex')})`);
  }
}

async function main(argv, env) {
  const [appId, command, ...rest] = argv;
  const app = appConfig(appId);
  switch (command) {
    case 'version':
      console.log(appVersion(app));
      return;
    case 'check-tag': {
      const problems = tagProblems(app, rest[0] ?? '', appVersion(app));
      if (problems.length) throw new Error(problems.join('\n'));
      console.log(`${rest[0]} names version ${appVersion(app)}`);
      return;
    }
    case 'icons': {
      const problems = iconProblems(app, readFiles(join(root, 'icons', app.id, 'tauri')), readFiles(join(appDir(app), 'src-tauri', 'icons')));
      if (problems.length) throw new Error(problems.join('\n'));
      console.log(`the app icons are the pipeline's (icons/${app.id}/tauri)`);
      return;
    }
    case 'notes': {
      const out = rest[0];
      if (!out) throw new Error('notes <out.md>');
      writeFileSync(out, releaseNotes(app, readFileSync(join(appDir(app), 'CHANGELOG.md'), 'utf8'), appVersion(app)));
      console.log(`wrote ${basename(out)}`);
      return;
    }
    case 'publish': {
      const [tag, dir, notes] = rest;
      if (!tag || !dir || !notes) throw new Error('publish <tag> <dir> <notes.md>');
      await publish(app, tag, dir, notes, env);
      return;
    }
    case 'sidecars':
      for (const s of app.sidecars) console.log(s);
      return;
    default:
      throw new Error(`unknown command ${JSON.stringify(command ?? '')} (version, check-tag, icons, notes, publish, sidecars)`);
  }
}

if (isMain(import.meta.url)) {
  main(process.argv.slice(2), process.env).catch((e) => {
    console.error(`demo-release: ${e.message}`);
    process.exit(1);
  });
}
