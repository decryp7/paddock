// Builds the library and publishes it to the Gitea npm registry (https://dev.decryptology.net/api/packages/decryp7/npm/).
//   node scripts/release.mjs            build and `npm pack --dry-run` of dist/paddock-ui (shows what would be published)
//   node scripts/release.mjs --publish  also npm publish and create the Gitea release v<version> with notes (no assets)
//                                       Token: PACKAGE_TOKEN (write:package), else GITEA_TOKEN. The tag must already be pushed.
// Publishing a version that already exists is reported as "already published" and is not an error.
import { execFileSync, spawnSync } from 'node:child_process';
import { copyFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ui = join(dirname(fileURLToPath(import.meta.url)), '..');
const kit = join(ui, '..');
const run = (cmd, args, cwd = ui) => execFileSync(cmd, args, { cwd, stdio: 'inherit' });
const version = JSON.parse(readFileSync(join(ui, 'projects/paddock-ui/package.json'), 'utf8')).version;
const dist = join(ui, 'dist/paddock-ui');
const registry = 'https://dev.decryptology.net/api/packages/decryp7/npm/';

run('npm', ['run', 'build']);
for (const f of ['LICENSE-MIT', 'LICENSE-APACHE']) copyFileSync(join(kit, f), join(dist, f));
copyFileSync(join(ui, 'README.md'), join(dist, 'README.md'));

if (!process.argv.includes('--publish')) {
  run('npm', ['pack', dist, '--dry-run']);
} else {
  const token = process.env.PACKAGE_TOKEN || process.env.GITEA_TOKEN;
  if (!token) throw new Error('PACKAGE_TOKEN (or GITEA_TOKEN) is not set: it needs the write:package scope');

  // The token goes into a throw-away npmrc that is deleted afterwards; it is never printed.
  const dir = mkdtempSync(join(tmpdir(), 'paddock-npm-'));
  const npmrc = join(dir, 'npmrc');
  writeFileSync(npmrc, `@decryp7:registry=${registry}\n//${registry.replace(/^https:\/\//, '')}:_authToken=${token}\n`, { mode: 0o600 });
  try {
    const res = spawnSync('npm', ['publish', dist, '--userconfig', npmrc, '--registry', registry], { cwd: ui, encoding: 'utf8' });
    const output = `${res.stdout}${res.stderr}`.split(token).join('***');
    process.stdout.write(output);
    if (res.status === 0) console.log(`published @decryp7/paddock-ui@${version}`);
    else if (/\b409\b|already exists|cannot publish over|previously published/i.test(output)) {
      console.log(`@decryp7/paddock-ui@${version} is already published; nothing to do`);
    } else throw new Error(`npm publish failed (exit ${res.status})`);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }

  const api = 'https://dev.decryptology.net/api/v1/repos/decryp7/paddock';
  const headers = { Authorization: `token ${token}` };
  const tag = `v${version}`;
  let res = await fetch(`${api}/releases/tags/${tag}`, { headers });
  if (res.ok) console.log(`release ${tag} already exists`);
  else {
    res = await fetch(`${api}/releases`, {
      method: 'POST',
      headers: { ...headers, 'Content-Type': 'application/json' },
      body: JSON.stringify({
        tag_name: tag,
        name: tag,
        body: `@decryp7/paddock-ui ${version}, published to the Gitea npm registry (${registry}). See CHANGELOG.md.`,
      }),
    });
    if (!res.ok) throw new Error(`create release failed: ${res.status} (the token needs write:repository to create releases)`);
    console.log(`created release ${tag}`);
  }
}
