#!/usr/bin/env node
// A release tag names the app's version: `pitcrew-v1.2.3` with --prefix pitcrew- needs "version": "1.2.3" in the app's
// tauri.conf.json (the installers carry that version; a mismatch publishes a release whose name lies). Like the demos'
// `check-tag`, shared.
//
//   node check-tag.mjs --tag pitcrew-v1.2.3 --prefix pitcrew- --conf apps/pitcrew/app/src-tauri/tauri.conf.json
import { readFileSync } from 'node:fs';
import { isMain } from '../src/util/is-main.mjs';

/** The version a tag names, or throws: `<prefix>v<semver>`. */
export function tagVersion(tag, prefix = '') {
  const m = new RegExp(`^${prefix.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}v(\\d+\\.\\d+\\.\\d+(?:-[0-9A-Za-z.-]+)?)$`).exec(tag);
  if (!m) throw new Error(`the tag ${tag} is not ${prefix}vX.Y.Z`);
  return m[1];
}

/** Throws unless the tag's version is the config's. */
export function checkTag(tag, prefix, conf) {
  const want = tagVersion(tag, prefix);
  const have = conf.version;
  if (have !== want) throw new Error(`the tag ${tag} says ${want}, tauri.conf.json says ${have}: bump the version first, then tag`);
  return want;
}

if (isMain(import.meta.url)) {
  const o = {};
  const argv = process.argv.slice(2);
  for (let i = 0; i < argv.length; i += 2) o[argv[i].replace(/^--/, '')] = argv[i + 1];
  try {
    if (!o.tag || !o.conf) throw new Error('usage: check-tag.mjs --tag <tag> [--prefix <app->] --conf <tauri.conf.json>');
    console.log(`tag ${o.tag} matches version ${checkTag(o.tag, o.prefix ?? '', JSON.parse(readFileSync(o.conf, 'utf8')))}`);
  } catch (e) {
    console.error(`::error::check-tag: ${e.message}`);
    process.exit(1);
  }
}
