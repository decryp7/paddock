#!/usr/bin/env node
// What's new shows the running version's section of CHANGELOG.md to the user (crates/paddock-updater/src/notes.rs). Users read
// plain sentences, never ADR or work-package numbers or request identifiers (ER-21: the class is checked, not each entry).
// The engineering detail lives in docs/. Checks the newest release section, the one that ships.
//
//   node scripts/check-release-notes.mjs [path/to/CHANGELOG.md]
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

/** What a user must not read: an ADR number, a work package, a SNAKE_CASE identifier, or a backtick (the dialog shows text as it is written, so code marks appear raw). */
export const FORBIDDEN = [/ADR \d/, /WP-/, /[A-Z]{2,}_[A-Z]/, /`/];

/** The newest release's section: its heading and body lines. */
export function newestSection(changelog) {
  const lines = changelog.split(/\r?\n/);
  const start = lines.findIndex((l) => /^## /.test(l));
  if (start < 0) return { heading: '', lines: [], offset: 0 };
  let end = lines.findIndex((l, i) => i > start && /^## /.test(l));
  if (end < 0) end = lines.length;
  return { heading: lines[start], lines: lines.slice(start + 1, end), offset: start + 2 };
}

/** The offending lines of the newest section, each as `line N: text`. */
export function problems(changelog) {
  const { lines, offset } = newestSection(changelog);
  const out = [];
  lines.forEach((l, i) => {
    const hit = FORBIDDEN.find((re) => re.test(l));
    if (hit) out.push(`CHANGELOG.md:${offset + i}: matches ${hit}: ${l.slice(0, 100)}`);
  });
  return out;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const file = process.argv[2] ?? join(dirname(fileURLToPath(import.meta.url)), '..', 'CHANGELOG.md');
  const found = problems(readFileSync(file, 'utf8'));
  for (const f of found) console.error(`::error::${f}`);
  if (found.length) {
    console.error('::error::The release notes users read have ADR, work-package or identifier references. Write plain sentences; keep the detail in apps/dyno/docs/.');
    process.exit(1);
  }
  console.log('release notes: plain sentences');
}
