#!/usr/bin/env node
// Publishes a Gitea release for a tag and uploads its files, through the Gitea API only (no Go action needed on the
// runner). The release is created as a draft and published after the last upload. Re-running is safe: an existing release (or draft) for the tag is reused and files already attached are skipped.
//
//   GITEA_TOKEN=... node conformance/release/publish.mjs --api https://host/api/v1 --repo owner/name --tag v1.2.0 \
//     --notes artifacts/RELEASE_NOTES.md --dir artifacts [--replace]
//
// Tags: `vX.Y.Z`, an app's `<app>-vX.Y.Z` in the monorepo, or its moving `<app>-latest` (ADR 0014 §4: installed apps read
// `<app>-latest/latest.json`, their only updater endpoint). `--replace` replaces files the release already has (the moving
// release); without it they are skipped. Nothing is ever written to an app's old repository: there is no update bridge
// (ADR 0014, amendment 2026-10-25: installs from before the monorepo are reinstalled once by hand).
//
// The token needs write:repository (create a release, upload assets). It is read from the environment and never printed:
// RELEASE_TOKEN (a personal access token) when it is set and not empty, else GITEA_TOKEN (the job's github.token, which
// publish.yml released v0.3.0 with). The message names which one was used, never its value.

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { basename, join } from 'node:path';
import { isMain } from '../src/util/is-main.mjs';

/** Release notes beyond this are cut (Gitea stores the body as text). */
export const MAX_NOTES = 60000;

/** Every regular file under `dir`, recursively, sorted for a stable upload order. */
export function filesUnder(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    const s = statSync(p);
    if (s.isDirectory()) out.push(...filesUnder(p));
    else if (s.isFile()) out.push(p);
  }
  return out.sort();
}

/** A release tag: `vX.Y.Z[-pre]`, `<app>-vX.Y.Z[-pre]` or `<app>-latest`. */
const TAG = /^(?:[a-z][a-z0-9-]*-)?v\d+\.\d+\.\d+(-[\w.]+)?$|^[a-z][a-z0-9-]*-latest$/;

/** Whether a tag names a pre-release: a semver suffix (`v1.2.0-rc.1`), never the app prefix (`debrief-v1.2.0`). */
export const isPrerelease = (tag) => /v\d+\.\d+\.\d+-/.test(tag);

/** Parses `--key value` pairs and the `--replace` flag; every option is required. */
export function parseArgs(argv) {
  const want = ['api', 'repo', 'tag', 'notes', 'dir'];
  const flags = ['replace'];
  const o = {};
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i]?.replace(/^--/, '');
    if (flags.includes(k)) {
      o[k] = true;
      continue;
    }
    if (!want.includes(k) || argv[i + 1] === undefined) throw new Error(`unknown or incomplete option: ${argv[i]}`);
    o[k] = argv[++i];
  }
  for (const k of want) if (!o[k]) throw new Error(`--${k} is required`);
  if (!/^[\w.-]+\/[\w.-]+$/.test(o.repo)) throw new Error(`--repo must be owner/name, got ${o.repo}`);
  if (!TAG.test(o.tag)) throw new Error(`--tag must be vX.Y.Z, <app>-vX.Y.Z or <app>-latest, got ${o.tag}`);
  return { ...o, api: o.api.replace(/\/+$/, '') };
}

async function call(fetchImpl, token, method, url, body) {
  const res = await fetchImpl(url, {
    method,
    headers: { Authorization: `token ${token}`, ...(body && !(body instanceof FormData) ? { 'Content-Type': 'application/json' } : {}) },
    body: body instanceof FormData ? body : body ? JSON.stringify(body) : undefined,
  });
  if (res.status === 404 && method === 'GET') return null;
  if (!res.ok) throw new Error(`${method} ${url.replace(/\?.*/, '')}: HTTP ${res.status} ${(await res.text()).slice(0, 300)}`);
  return res.status === 204 ? null : res.json();
}

/**
 * Creates (or reuses) the release and uploads the files it does not have yet; with `replace`, a file it has is deleted
 * and uploaded again. Returns the release's page URL.
 */
export async function publish({ api, repo, tag, notes, dir, replace = false }, token, fetchImpl = fetch, log = console.log) {
  const base = `${api}/repos/${repo}`;
  const body = readFileSync(notes, 'utf8').slice(0, MAX_NOTES);
  let release = await call(fetchImpl, token, 'GET', `${base}/releases/tags/${encodeURIComponent(tag)}`);
  // A draft left by an interrupted run is not found by tag: look for it among the drafts.
  if (!release) {
    const drafts = await call(fetchImpl, token, 'GET', `${base}/releases?draft=true&limit=50`);
    release = (drafts ?? []).find((r) => r.tag_name === tag && r.draft) ?? null;
  }
  if (release) log(`release ${tag} exists; reusing it`);
  else {
    release = await call(fetchImpl, token, 'POST', `${base}/releases`, { tag_name: tag, name: tag, body, draft: true, prerelease: isPrerelease(tag) });
    log(`created release ${tag}`);
  }
  const have = new Map((release.assets ?? []).map((a) => [a.name, a.id]));
  for (const file of filesUnder(dir)) {
    const name = basename(file);
    if (have.has(name) && replace) {
      await call(fetchImpl, token, 'DELETE', `${base}/releases/${release.id}/assets/${have.get(name)}`);
      have.delete(name);
      log(`  replacing ${name}`);
    }
    if (have.has(name)) {
      log(`  skip ${name} (already attached)`);
      continue;
    }
    const form = new FormData();
    form.append('attachment', new Blob([readFileSync(file)]), name);
    await call(fetchImpl, token, 'POST', `${base}/releases/${release.id}/assets?name=${encodeURIComponent(name)}`, form);
    have.set(name, null);
    log(`  uploaded ${name}`);
  }
  // Published only now, after the last upload: nobody (and no updater) sees a release with half its files.
  if (release.draft) {
    release = await call(fetchImpl, token, 'PATCH', `${base}/releases/${release.id}`, { draft: false, prerelease: isPrerelease(tag) });
    log(`published release ${tag}`);
  }
  return release.html_url;
}

/**
 * The token to publish with and the name of where it came from: `RELEASE_TOKEN` when set and not blank, else the fallback
 * `GITEA_TOKEN`; null when neither is. The caller prints the name, never the token.
 */
export function pickToken(env) {
  for (const name of ['RELEASE_TOKEN', 'GITEA_TOKEN']) {
    const value = env[name]?.trim();
    if (value) return { name, token: value };
  }
  return null;
}

if (isMain(import.meta.url)) {
  const picked = pickToken(process.env);
  if (!picked) {
    console.error('publish: neither RELEASE_TOKEN nor the fallback GITEA_TOKEN (the job token) is set');
    process.exit(2);
  }
  console.log(`publish: using ${picked.name}${picked.name === 'GITEA_TOKEN' ? ' (the job token: RELEASE_TOKEN is not set)' : ''}`);
  try {
    console.log(await publish(parseArgs(process.argv.slice(2)), picked.token));
  } catch (e) {
    console.error(`publish: ${e.message}`);
    process.exit(1);
  }
}
