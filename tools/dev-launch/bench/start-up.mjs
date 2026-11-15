// Start-up cost of the ./dev launcher against the shell wrapper it replaced (docs/benchmarks.md, rule 48).
//   node tools/dev-launch/bench/start-up.mjs <repo with the new dev> <old dev script> <work dir> [N]
// A throw-away repository in <work dir> (emptied first: name a folder of its own) with the real inputs list (placeholder files), the
// new stage-0 `dev` and tools/dev-launch, and a cached stand-in paddock-dev for its hash. It prints the cold build of the launcher and
// the wall time (median, minimum, p90) of `./dev --print-binary` and `./dev hello` for the old shell and the new launcher,
// alternating, two rounds.
import { execFileSync, spawnSync } from 'node:child_process';
import { chmodSync, cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';

const [src, oldDev, workArg, nArg] = process.argv.slice(2);
if (!src || !oldDev || !workArg) {
  console.error('usage: node start-up.mjs <repo with the new dev> <old dev script> <work dir> [N]');
  process.exit(2);
}
const work = resolve(workArg);
const n = Number(nArg ?? 30);
if (work.split('/').filter(Boolean).length < 3) throw new Error(`the work folder ${work} is too close to the root to empty`);
rmSync(work, { recursive: true, force: true });
const repo = join(work, 'repo');
const home = join(work, 'home');
mkdirSync(join(repo, 'tools/paddock-dev/src'), { recursive: true });
mkdirSync(home, { recursive: true });
const git = (...a) => execFileSync('git', ['-C', repo, ...a], { stdio: 'pipe' }).toString();
git('init', '-q', '-b', 'main');
git('config', 'user.email', 't@example.invalid');
git('config', 'user.name', 't');
git('config', 'commit.gpgsign', 'false');
const inputsText = readFileSync(join(src, 'tools/paddock-dev/inputs.txt'), 'utf8');
writeFileSync(join(repo, 'tools/paddock-dev/inputs.txt'), inputsText);
for (const p of inputsText.split('\n').map((l) => l.trim()).filter((l) => l && !l.startsWith('#'))) {
  if (p.endsWith('.toml')) {
    mkdirSync(dirname(join(repo, p)), { recursive: true });
    writeFileSync(join(repo, p), '# x\n');
  } else {
    mkdirSync(join(repo, p), { recursive: true });
    writeFileSync(join(repo, p, 'keep'), 'x\n');
  }
}
writeFileSync(join(repo, 'tools/paddock-dev/src/main.rs'), 'fn main() {}\n');
cpSync(join(src, 'tools/dev-launch'), join(repo, 'tools/dev-launch'), { recursive: true });
rmSync(join(repo, 'tools/dev-launch/target'), { recursive: true, force: true });
cpSync(join(src, 'dev'), join(repo, 'dev'));
chmodSync(join(repo, 'dev'), 0o755);
writeFileSync(join(repo, '.gitignore'), '/tools/dev-launch/target/\n');
git('add', '-A');
git('commit', '-q', '-m', 'fixture');

const env = { ...process.env, PADDOCK_DEV_HOME: home };
delete env.CARGO_TARGET_DIR;
const run = (cmd, args) => spawnSync(cmd, args, { cwd: repo, env, stdio: ['ignore', 'pipe', 'pipe'], encoding: 'utf8' });
let t = performance.now();
run('./dev', ['--wrapper-hash']);
console.log(`cold: the first ./dev builds the launcher (release): ${((performance.now() - t) / 1000).toFixed(1)} s`);
const hash = run('./dev', ['--wrapper-hash']).stdout.split('\n')[0];
const host = /^host: (.*)$/m.exec(execFileSync('rustc', ['-vV']).toString())[1];
const entry = join(home, 'bin', hash);
mkdirSync(entry, { recursive: true });
const tool = join(entry, `paddock-dev-${host}${host.includes('windows') ? '.exe' : ''}`);
writeFileSync(tool, '#!/bin/sh\ncase "$1" in --version) echo 0;; esac\n');
chmodSync(tool, 0o755);
const oldBin = run('sh', [oldDev, '--print-binary']).stdout.trim();
const newBin = run('./dev', ['--print-binary']).stdout.trim();
if (!oldBin || oldBin !== newBin) throw new Error(`the old shell and the launcher disagree on the binary: ${oldBin} / ${newBin}`);

const bench = (label, cmd, args) => {
  for (let i = 0; i < 3; i++) run(cmd, args);
  const w = [];
  for (let i = 0; i < n; i++) {
    const s = performance.now();
    run(cmd, args);
    w.push(performance.now() - s);
  }
  w.sort((a, b) => a - b);
  console.log(`${label}: n=${n} median ${Math.round(w[Math.floor(n / 2)])} ms, min ${Math.round(w[0])} ms, p90 ${Math.round(w[Math.ceil(n * 0.9) - 1])} ms`);
};
const load = existsSync('/proc/loadavg') ? readFileSync('/proc/loadavg', 'utf8').split(' ').slice(0, 3).join(' ') : execFileSync('uptime').toString().replace(/.*averages?: /, '').trim();
console.log(`${process.platform} ${process.arch}; load: ${load}`);
for (let round = 0; round < 2; round++) {
  bench('old shell    ./dev --print-binary', 'sh', [oldDev, '--print-binary']);
  bench('new launcher ./dev --print-binary', './dev', ['--print-binary']);
  bench('old shell    ./dev hello', 'sh', [oldDev, 'hello']);
  bench('new launcher ./dev hello', './dev', ['hello']);
}
