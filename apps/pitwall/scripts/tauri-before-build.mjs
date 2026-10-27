// `beforeBuildCommand` of tauri.conf.json: the server sidecar, the Angular production build, and the bundle check
// that index.html has no inline elements (the shared conformance/ci/check-index.mjs; Tauri's CSP nonce would make the webview ignore
// 'unsafe-inline' and block Angular's runtime styles, docs/suite-lessons.md).
import { execFileSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const app = resolve(here, '..', 'app');
const run = (cmd, args, cwd) => execFileSync(cmd, args, { cwd, stdio: 'inherit', shell: process.platform === 'win32' });

run('node', [join(here, 'sidecar.mjs')], app);
if (process.env.PITWALL_SKIP_UI_BUILD !== '1') run('npm', ['run', 'build'], app);
run('node', [join(here, '..', '..', '..', 'conformance', 'ci', 'check-index.mjs')], app);
