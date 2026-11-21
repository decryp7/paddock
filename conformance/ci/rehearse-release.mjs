#!/usr/bin/env node
// The release job's own work, without the publishing: what a tag's `release` job (app-release.yml) does with the installers
// (.gitea/actions/publish-release), run for the tag rehearsal (.gitea/workflows/tag-rehearsal.yml). Release notes and latest.json are
// made from the built installers and their signatures exactly as a release makes them, then READ BACK and checked: every entry has a signature
// and a URL (the signatures themselves are verified against the build's public key by tauri-bundle.sh, updater-key-check.mjs). Nothing is published:
// this file imports no publisher and uses no network or token, and it refuses to run unless PADDOCK_TAG_REHEARSAL=1 on a run that is not a real tag.
//
//   node rehearse-release.mjs --app-name Dyno --changelog <file> --dir release --version 1.2.0 [--extra <text>]
import { spawnSync } from 'node:child_process';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isMain } from '../src/util/is-main.mjs';
import { isRehearsal, isRealTagRef } from '../src/util/is-tag-run.mjs';

const here = dirname(fileURLToPath(import.meta.url));
/** The base URL of a rehearsal's latest.json: never a real download address, so an updater that read it could not fetch anything. */
export const REHEARSAL_BASE = 'https://rehearsal.invalid/releases/download';

/** Throws unless this is a tag rehearsal on a run that is not a real tag. */
export function requireRehearsal(env = process.env) {
  if (!isRehearsal(env)) throw new Error('PADDOCK_TAG_REHEARSAL is not 1: this script rehearses a release and must never run in a real one');
  if (isRealTagRef(env)) throw new Error('this is a real tag run: a rehearsal never runs on a tag');
}

/** Throws unless `latest` (the parsed latest.json) has at least one platform entry, each with a signature and a URL under the rehearsal's own base. */
export function checkLatest(latest, version) {
  if (latest.version !== version) throw new Error(`latest.json says version ${latest.version}, the build is ${version}`);
  const entries = Object.values(latest.platforms ?? {});
  if (!entries.length) throw new Error('latest.json has no platform entry: the installers carried no updater signature');
  for (const [key, e] of Object.entries(latest.platforms)) {
    if (!e.signature || !e.url) throw new Error(`latest.json platform ${key} has no url or signature`);
    if (!e.url.startsWith(REHEARSAL_BASE)) throw new Error(`latest.json platform ${key} points at ${e.url}, not at the rehearsal's own base`);
  }
}

export function rehearse({ appName, changelog, dir, version, extra = '', env = process.env, run = spawnSync }) {
  requireRehearsal(env);
  const node = (script, args) => {
    const r = run(process.execPath, [join(here, script), ...args], { encoding: 'utf8', env });
    if (r.status !== 0) throw new Error(`${script} failed (exit ${r.status}): ${String(r.stderr || r.stdout || r.error?.message || '').trim().split('\n').slice(-3).join(' ')}`);
  };
  node('release-notes.mjs', ['--app', appName, '--changelog', changelog, '--out', join(dir, 'RELEASE_NOTES.md'), '--extra', extra]);
  const files = readdirSync(dir);
  if (!files.some((f) => f.endsWith('.sig'))) throw new Error('no .sig next to any installer: a real release would publish no latest.json (a signed build was meant)');
  const latest = join(dir, 'latest.json');
  node('../release/latest-json.mjs', ['--dir', dir, '--version', version, '--notes', join(dir, 'RELEASE_NOTES.md'), '--base-url', `${REHEARSAL_BASE}/rehearsal-v${version}`, '--out', latest]);
  checkLatest(JSON.parse(readFileSync(latest, 'utf8')), version);
  return { platforms: Object.keys(JSON.parse(readFileSync(latest, 'utf8')).platforms) };
}

if (isMain(import.meta.url)) {
  const o = {};
  const argv = process.argv.slice(2);
  for (let i = 0; i < argv.length; i += 2) o[argv[i].replace(/^--/, '')] = argv[i + 1];
  const fail = (m) => {
    console.error(`::error::tag rehearsal (release path): ${m}`);
    process.exit(1);
  };
  if (!o['app-name'] || !o.changelog || !o.dir || !o.version) fail('usage: rehearse-release.mjs --app-name <Name> --changelog <file> --dir <dir> --version <x.y.z>');
  try {
    const r = rehearse({ appName: o['app-name'], changelog: o.changelog, dir: o.dir, version: o.version, extra: o.extra ?? '' });
    console.log(`rehearsal: release notes and latest.json made and read back (${r.platforms.join(', ')}); nothing published`);
  } catch (e) {
    fail(`${e.message}. Next step: the release job runs the same notes and latest.json steps; fix the cause above.`);
  }
}
