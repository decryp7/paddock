// Records tests/golden-prune/expected.json: what tools/prune-build-folders.sh and tools/prune-ci-build-folder.sh do to the
// fixture build folder (fixture.json) in every case of cases.json: exit code, standard output (the scratch root and the
// measured sizes masked: a size depends on the file system) and the tree left behind. Run once from the repository root
// (`node tools/paddock-dev/tests/golden-prune/generate.mjs`) and commit the result; tests/prune.rs builds the same fixture and
// compares `dev host prune` with it, so the test outlives the scripts (ADR 0022 §3.6, R5). The scripts are run with
// PADDOCK_PRUNE_UNIT=kb, as their own tests do.
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, lstatSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '../../../..');
const fixture = JSON.parse(readFileSync(join(here, 'fixture.json'), 'utf8'));
const cases = JSON.parse(readFileSync(join(here, 'cases.json'), 'utf8'));

function build(folder) {
  mkdirSync(folder, { recursive: true });
  for (const f of fixture.files) {
    if (f.path.endsWith('/')) mkdirSync(join(folder, f.path), { recursive: true });
    else {
      mkdirSync(dirname(join(folder, f.path)), { recursive: true });
      writeFileSync(join(folder, f.path), Buffer.alloc(f.size, 7));
    }
  }
  for (const l of fixture.links) symlinkSync(l.target, join(folder, l.path));
}

function tree(dir, prefix = '') {
  const out = [];
  for (const name of readdirSync(dir).sort()) {
    const st = lstatSync(join(dir, name));
    const rel = prefix + name;
    if (st.isSymbolicLink()) out.push(`${rel} -> link`);
    else if (st.isDirectory()) out.push(`${rel}/`, ...tree(join(dir, name), `${rel}/`));
    else out.push(`${rel} ${st.size}`);
  }
  return out;
}

const mask = (text, root) => text.split(root).join('<ROOT>').replace(/ is \d+ \(cap/g, ' is <N> (cap').replace(/still \d+;/g, 'still <N>;');

const result = {};
for (const c of cases) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'paddock-prune-golden-')));
  try {
    for (const d of ['home/target-app-s1', 'home/target-app-s2', 'home/notes', 'other/build']) build(join(root, d));
    symlinkSync('target-app-s1', join(root, 'home/linkroot'));
    mkdirSync(join(root, 'outside'));
    writeFileSync(join(root, 'outside/canary'), 'x');
    const folder = c.folder === '' ? '' : join(root, c.folder);
    let r;
    if (c.mode === 'plain') {
      r = spawnSync('bash', [join(repo, 'tools/prune-build-folders.sh'), folder, c.cap], { env: { PATH: '/usr/bin:/bin', PADDOCK_PRUNE_UNIT: 'kb' }, encoding: 'utf8' });
    } else {
      r = spawnSync('bash', [join(repo, 'tools/prune-ci-build-folder.sh'), folder, c.cap, ...(c.dropBundles ? ['--drop-bundles'] : [])], {
        env: { PATH: '/usr/bin:/bin', HOME: root, RUNNER_OS: 'macOS', PADDOCK_CI_HOME: join(root, 'home'), PADDOCK_CI_SLOT: c.slot, PADDOCK_PRUNE_UNIT: 'kb' },
        encoding: 'utf8',
      });
    }
    result[c.name] = { exit: r.status, stdout: mask(r.stdout + (c.mode === 'ci' ? r.stderr : ''), root), tree: tree(root) };
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}
writeFileSync(join(here, 'expected.json'), `${JSON.stringify(result, null, 2)}\n`);
console.log(`recorded ${Object.keys(result).length} cases`);
