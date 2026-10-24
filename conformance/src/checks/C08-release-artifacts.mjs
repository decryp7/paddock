// C08: the shared release scripts are used (not drifted copies), and the release files are right: installer versions equal the
// version, no stale installers, release notes carry "First launch", latest.json is https and signed. Catches B13 to B15.
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { error } from '../finding.mjs';
import { appVersion } from '../util/version.mjs';

const RELEASE_DIR = fileURLToPath(new URL('../../release/', import.meta.url));

/** Source with the leading comment block (and the shebang) removed: the explanation differs per app, the code must not. */
export function normalized(text) {
  const lines = text.replace(/\r\n/g, '\n').split('\n');
  let i = 0;
  while (i < lines.length && (lines[i].startsWith('#!') || lines[i].startsWith('//'))) i++;
  return lines.slice(i).join('\n');
}
const sha = (s) => createHash('sha256').update(s).digest('hex');

function walkDir(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walkDir(join(dir, e.name)) : [join(dir, e.name)]));
}

export function staticFindings(ctx) {
  const out = [];
  for (const name of ['publish.mjs', 'latest-json.mjs']) {
    const shared = normalized(readFileSync(join(RELEASE_DIR, name), 'utf8'));
    const copies = ctx.files().filter((f) => f.endsWith(`release/${name}`) && !f.startsWith('conformance/'));
    for (const f of copies) {
      if (sha(normalized(ctx.read(f) ?? '')) !== sha(shared)) out.push(error('C08', 25, f, 1, `${f} differs from the shared ${name}`, `Delete the copy and use conformance/release/${name} unmodified.`));
    }
  }
  return out;
}

export function artifactFindings(ctx, dir) {
  const out = [];
  let files;
  try {
    files = walkDir(dir);
  } catch (e) {
    return [error('C08', 25, dir, 1, `cannot read the artifacts folder: ${e.message}`, 'Pass --artifacts <folder with the release files>.')];
  }
  if (!files.length) return [error('C08', 25, dir, 1, 'the artifacts folder is empty', 'Collect the installers, signatures, notes and latest.json there.')];
  const version = ctx.options.version?.replace(/^v/, '') ?? appVersion(ctx);
  if (!version) return [error('C08', 25, dir, 1, 'the app version is unknown (--version vX.Y.Z)', 'Pass the release version.')];
  const rel = (f) => f.slice(dir.length + 1);

  for (const f of files) {
    const base = f.split('/').pop();
    const m = /[_-](\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?)[_.-]/.exec(base);
    if (m && /\.(exe|msi|dmg|deb|rpm|AppImage|sig)$/.test(base) && m[1] !== version) {
      out.push(error('C08', 25, rel(f), 1, `${base} is version ${m[1]}, the release is ${version} (a stale installer from an earlier build)`, 'Clear the bundle folder before building so an old installer is never collected.'));
    }
  }
  const notes = files.find((f) => /RELEASE_NOTES\.md$/.test(f));
  if (!notes) out.push(error('C08', 25, dir, 1, 'no RELEASE_NOTES.md in the artifacts', 'Generate the release notes into the artifacts folder.'));
  else if (!/first launch/i.test(readFileSync(notes, 'utf8'))) out.push(error('C08', 25, rel(notes), 1, 'the release notes have no "First launch" section (until the apps are signed)', 'Add the First launch section to the notes template.'));

  const latest = files.find((f) => f.endsWith('latest.json'));
  if (!latest) out.push(error('C08', 25, dir, 1, 'no latest.json in the artifacts', 'Run latest-json.mjs after the bundles are signed.'));
  else {
    try {
      const j = JSON.parse(readFileSync(latest, 'utf8'));
      if (j.version !== version) out.push(error('C08', 25, rel(latest), 1, `latest.json says ${j.version}, the release is ${version}`, 'Regenerate latest.json for this version.'));
      const entries = Object.entries(j.platforms ?? {});
      if (!entries.length) out.push(error('C08', 25, rel(latest), 1, 'latest.json lists no platform', 'Sign the updater artifacts (TAURI_SIGNING_PRIVATE_KEY, createUpdaterArtifacts).'));
      for (const [k, v] of entries) {
        if (!String(v.url).startsWith('https://')) out.push(error('C08', 25, rel(latest), 1, `${k}: the url is not https (${v.url})`, 'Use the public https base URL of the release.'));
        if (!v.signature) out.push(error('C08', 25, rel(latest), 1, `${k}: no signature`, 'Only signed updater artifacts belong in latest.json.'));
      }
    } catch (e) {
      out.push(error('C08', 25, rel(latest), 1, `latest.json is not valid JSON: ${e.message}`, 'Regenerate it with latest-json.mjs.'));
    }
  }
  return out;
}

export default {
  id: 'C08',
  title: 'Shared release scripts used; release artifacts consistent',
  rule: 25,
  run(ctx) {
    const out = staticFindings(ctx);
    if (ctx.options.artifacts) out.push(...artifactFindings(ctx, ctx.options.artifacts));
    return out;
  },
};
