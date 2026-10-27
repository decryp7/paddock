#!/usr/bin/env node
// Release notes: the git-cliff changelog of the release plus the "First launch" block (plan X10 gap: it was a heredoc in
// five workflows). The block is Paddock's docs/blocks/first-launch.md with the product name filled in; it stays until the
// apps are signed (decision 2026-10-23).
//
//   node release-notes.mjs --app Pitcrew --changelog artifacts/CHANGELOG-latest.md --out artifacts/RELEASE_NOTES.md [--no-first-launch] [--extra <text>]
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isMain } from '../src/util/is-main.mjs';

const here = dirname(fileURLToPath(import.meta.url));

/** The First launch block for an app (no trailing newline). */
export function firstLaunch(appName, source = join(here, '..', '..', 'docs', 'blocks', 'first-launch.md')) {
  if (!/^[\p{L}\p{N}][\p{L}\p{N} .-]{0,38}$/u.test(appName)) throw new Error(`the app name ${JSON.stringify(appName)} is not a product name`);
  return readFileSync(source, 'utf8').replaceAll('{{AppName}}', appName).replace(/\n+$/, '');
}

/** The notes: changelog text, a blank line, the First launch block, an app's own paragraph (`extra`), one final newline. */
export function releaseNotes({ appName, changelog, withFirstLaunch = true, source, extra = '' }) {
  const body = changelog.replace(/\s+$/, '');
  const notes = withFirstLaunch ? `${body}\n\n${firstLaunch(appName, source)}` : body;
  return `${extra.trim() ? `${notes}\n\n${extra.trim()}` : notes}\n`;
}

function main(argv) {
  const o = { first: true };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--no-first-launch') o.first = false;
    else if (['--app', '--changelog', '--out', '--extra'].includes(argv[i])) o[argv[i].slice(2)] = argv[++i];
    else throw new Error(`unknown option ${argv[i]}`);
  }
  if (!o.app || !o.changelog || !o.out) throw new Error('usage: release-notes.mjs --app <Name> --changelog <file> --out <file> [--no-first-launch]');
  writeFileSync(o.out, releaseNotes({ appName: o.app, changelog: readFileSync(o.changelog, 'utf8'), withFirstLaunch: o.first, extra: o.extra ?? '' }));
}

if (isMain(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (e) {
    console.error(`release-notes: ${e.message}`);
    process.exit(2);
  }
}
