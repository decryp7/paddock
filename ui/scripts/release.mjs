// Builds the library and packs it as dist/paddock-ui-<version>.tgz (npm pack of the built package).
//   node scripts/release.mjs            build + pack
//   node scripts/release.mjs --publish  also create the Gitea release v<version> and attach the tarball
//                                       (needs GITEA_TOKEN; the tag must already be pushed)
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, readFileSync, renameSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ui = join(dirname(fileURLToPath(import.meta.url)), '..');
const kit = join(ui, '..');
const run = (cmd, args, cwd = ui) => execFileSync(cmd, args, { cwd, stdio: 'inherit' });
const version = JSON.parse(readFileSync(join(ui, 'projects/paddock-ui/package.json'), 'utf8')).version;
const dist = join(ui, 'dist/paddock-ui');

run('npm', ['run', 'build']);
for (const f of ['LICENSE-MIT', 'LICENSE-APACHE']) copyFileSync(join(kit, f), join(dist, f));
copyFileSync(join(ui, 'README.md'), join(dist, 'README.md'));
const out = join(ui, 'dist');
mkdirSync(out, { recursive: true });
run('npm', ['pack', dist, '--pack-destination', out]);
const packed = join(out, `decryp7-paddock-ui-${version}.tgz`);
const tarball = join(out, `paddock-ui-${version}.tgz`);
if (existsSync(packed)) renameSync(packed, tarball);
console.log(`\ntarball: ${tarball}`);

if (process.argv.includes('--publish')) {
  const token = process.env.GITEA_TOKEN;
  if (!token) throw new Error('GITEA_TOKEN is not set');
  const api = 'https://dev.decryptology.net/api/v1/repos/decryp7/paddock-kit';
  const headers = { Authorization: `token ${token}` };
  const tag = `v${version}`;
  let res = await fetch(`${api}/releases/tags/${tag}`, { headers });
  let release;
  if (res.ok) release = await res.json();
  else {
    res = await fetch(`${api}/releases`, {
      method: 'POST',
      headers: { ...headers, 'Content-Type': 'application/json' },
      body: JSON.stringify({ tag_name: tag, name: tag, body: `paddock-ui ${version}. See CHANGELOG.md.` }),
    });
    if (!res.ok) throw new Error(`create release failed: ${res.status}`);
    release = await res.json();
  }
  const form = new FormData();
  form.append('attachment', new Blob([readFileSync(tarball)]), `paddock-ui-${version}.tgz`);
  res = await fetch(`${api}/releases/${release.id}/assets?name=paddock-ui-${version}.tgz`, { method: 'POST', headers, body: form });
  if (!res.ok) throw new Error(`upload failed: ${res.status}`);
  const asset = await res.json();
  console.log(`attached: ${asset.browser_download_url}`);
}
