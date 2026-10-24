#!/usr/bin/env node
// Paddock Shell Demo's release steps (plan docs/plans/workbench.md §10, S18), used by .gitea/workflows/shell-demo-release.yml
// on a `shell-demo-v*` tag. The app's tags are its own: the packages and crates release from `v*` (publish.yml).
//
//   node scripts/shell-demo-release.mjs version                  the app's version (apps/shell-demo/src-tauri/Cargo.toml)
//   node scripts/shell-demo-release.mjs check-tag <tag>          the tag names that version (`shell-demo-v0.1.0`)
//   node scripts/shell-demo-release.mjs icons                    the app's icons are the icon pipeline's (icons/shell-demo)
//   node scripts/shell-demo-release.mjs notes <out.md>           the version's CHANGELOG section and the First launch notes
//   node scripts/shell-demo-release.mjs publish <tag> <dir> <notes.md>
//                                                                only in CI (refuses elsewhere): the Gitea release with
//                                                                every file of <dir> as an asset (GITEA_TOKEN)
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const app = join(root, 'apps', 'shell-demo');
const GITEA_API = 'https://dev.decryptology.net/api/v1/repos/decryp7/paddock';

/** The tag prefix of the app's releases. */
export const TAG_PREFIX = 'shell-demo-v';

const RELEASE = /^\d+\.\d+\.\d+$/;

// ---------------------------------------------------------------------------------------------------------------- pure

/** The `[package]` version of a Cargo.toml (a release number), or an error naming what is wrong. */
export function cargoVersion(text) {
  const version = /^\[package\][^[]*?^version\s*=\s*"([^"]+)"/m.exec(text)?.[1];
  if (!version || !RELEASE.test(version)) throw new Error(`Cargo.toml: no release-number version in [package] (found ${JSON.stringify(version ?? null)})`);
  return version;
}

/** Problems of a release tag against the app's version: empty when the tag is `shell-demo-v<version>`. */
export function tagProblems(tag, version) {
  if (!tag.startsWith(TAG_PREFIX)) return [`the tag ${tag} is not a Shell Demo tag (${TAG_PREFIX}<version>)`];
  const named = tag.slice(TAG_PREFIX.length);
  if (!RELEASE.test(named)) return [`the tag ${tag} does not name a release number`];
  return named === version ? [] : [`the tag ${tag} names ${named}, but apps/shell-demo/src-tauri/Cargo.toml has ${version}`];
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
export const FIRST_LAUNCH = `### First launch

Paddock Shell Demo is not code-signed yet, so your system may warn the first time you open it.

- **macOS:** if it says the app cannot be opened, choose Done, then System Settings > Privacy & Security > scroll to the message about Paddock Shell Demo > **Open Anyway**.
- **Windows:** if SmartScreen says "Windows protected your PC", choose **More info > Run anyway**.
- **Linux:** install the .deb with your package manager, or make the AppImage executable and run it.

This build cannot update itself: download the next release from this page.`;

/** The release notes: the version's changes, then the first-launch notes. */
export function releaseNotes(changelog, version) {
  return `${changelogSection(changelog, version)}\n\n${FIRST_LAUNCH}\n`;
}

/** Files of `expected` (name to bytes) that `actual` lacks or holds different bytes of. */
export function iconProblems(expected, actual) {
  const out = [];
  for (const [name, bytes] of expected) {
    const have = actual.get(name);
    if (!have) out.push(`${name} is missing from apps/shell-demo/src-tauri/icons`);
    else if (!have.equals(bytes)) out.push(`${name} differs from icons/shell-demo/tauri (copy the pipeline's output: icons/README.md)`);
  }
  return out;
}

// -------------------------------------------------------------------------------------------------------------- effects

function readFiles(dir, filter = () => true) {
  return new Map(
    readdirSync(dir)
      .filter((f) => statSync(join(dir, f)).isFile() && filter(f))
      .map((f) => [f, readFileSync(join(dir, f))]),
  );
}

/** The app's version, from its single source. */
export function appVersion() {
  return cargoVersion(readFileSync(join(app, 'src-tauri', 'Cargo.toml'), 'utf8'));
}

async function publish(tag, dir, notesFile, env) {
  if (env.GITEA_ACTIONS !== 'true' && env.CI !== 'true') throw new Error('publish runs only in CI from a shell-demo-v* tag (rule 9)');
  const problems = tagProblems(tag, appVersion());
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
      body: JSON.stringify({ tag_name: tag, name: `Paddock Shell Demo ${tag.slice(TAG_PREFIX.length)}`, body: readFileSync(notesFile, 'utf8') }),
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
  const [command, ...rest] = argv;
  switch (command) {
    case 'version':
      console.log(appVersion());
      return;
    case 'check-tag': {
      const problems = tagProblems(rest[0] ?? '', appVersion());
      if (problems.length) throw new Error(problems.join('\n'));
      console.log(`${rest[0]} names version ${appVersion()}`);
      return;
    }
    case 'icons': {
      const problems = iconProblems(readFiles(join(root, 'icons', 'shell-demo', 'tauri')), readFiles(join(app, 'src-tauri', 'icons')));
      if (problems.length) throw new Error(problems.join('\n'));
      console.log('the app icons are the pipeline\'s (icons/shell-demo/tauri)');
      return;
    }
    case 'notes': {
      const out = rest[0];
      if (!out) throw new Error('notes <out.md>');
      writeFileSync(out, releaseNotes(readFileSync(join(app, 'CHANGELOG.md'), 'utf8'), appVersion()));
      console.log(`wrote ${basename(out)}`);
      return;
    }
    case 'publish': {
      const [tag, dir, notes] = rest;
      if (!tag || !dir || !notes) throw new Error('publish <tag> <dir> <notes.md>');
      await publish(tag, dir, notes, env);
      return;
    }
    default:
      throw new Error(`unknown command ${JSON.stringify(command ?? '')} (version, check-tag, icons, notes, publish)`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2), process.env).catch((e) => {
    console.error(`shell-demo-release: ${e.message}`);
    process.exit(1);
  });
}
