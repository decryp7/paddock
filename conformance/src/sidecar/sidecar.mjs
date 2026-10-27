// The sidecar kit (plan X34): one mechanism for what Pitcrew's, Pitwall's, Dyno's, Debrief's and the shell demo's sidecar scripts
// each did: build the helper binaries of an app and put them where Tauri's `bundle.externalBin` expects them
// (`<out>/<name>-<target triple>[.exe]`), and, for rule 23, start each from the built bundle with `--self-test`.
// Pure planning (`planSidecars`, `parseSidecarSpec`) is separate from the I/O (`buildSidecars`, `selfTestSidecars`), which takes
// its process and file operations as parameters so every path is tested without cargo.
import { execFileSync, spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, mkdirSync, readdirSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';

/** `name`, `name=package` or `name=package:bin` as { name, package, bin } (bin defaults to the name, the package too). */
export function parseSidecarSpec(spec) {
  const m = /^([A-Za-z0-9_-]{1,64})(?:=([A-Za-z0-9_-]{1,64})(?::([A-Za-z0-9_-]{1,64}))?)?$/.exec(spec);
  if (!m) throw new Error(`not a sidecar spec: ${JSON.stringify(spec)} (use name, name=package or name=package:bin)`);
  return { name: m[1], package: m[2] ?? m[1], bin: m[3] ?? m[1] };
}

/** `rustc -vV`'s host triple. */
export function hostTriple(exec = (cmd, args) => execFileSync(cmd, args, { encoding: 'utf8' })) {
  return /^host: (.+)$/m.exec(exec('rustc', ['-vV']))?.[1]?.trim();
}

const TRIPLES = { 'universal-apple-darwin': ['aarch64-apple-darwin', 'x86_64-apple-darwin'] };

/**
 * Cargo's build folder for the workspace at `root`: CARGO_TARGET_DIR when set, else what `cargo metadata` reports, which honours a
 * `build.target-dir` in the cargo configuration (CI's host runners set it in `.cargo/ci.toml`: sccache puts every CARGO_* variable into
 * its cache key, so CI never sets CARGO_TARGET_DIR), else `<root>/target`. Every sidecar script resolves the folder through this.
 */
export function cargoTargetDir(root, env = process.env, metadata = () => execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1'], { cwd: root, env, encoding: 'utf8', maxBuffer: 64 << 20, timeout: 120000 })) {
  if (env.CARGO_TARGET_DIR) return resolve(env.CARGO_TARGET_DIR);
  try {
    const dir = JSON.parse(metadata()).target_directory;
    if (typeof dir === 'string' && dir) return dir;
  } catch {
    // No cargo or an unreadable answer: the default folder (the build itself reports what is wrong).
  }
  return join(root, 'target');
}

/** The folder cargo writes a profile into. */
export function profileDir(profile) {
  return profile === 'dev' || profile === 'debug' ? 'debug' : profile;
}

/**
 * What to build and where cargo puts it. A build for the host runs without `--target`, into `<targetDir>/<profile>`, where `tauri
 * build` builds the app too (with `--target` cargo would compile every dependency a second time). Other targets, and both halves of
 * `universal-apple-darwin`, keep `--target` and their folder. `profile` is `debug`/`dev`, `release` or a custom profile name.
 */
export function planSidecars({ sidecars, target, host, profile, targetDir, manifestPath }) {
  if (!sidecars?.length) throw new Error('no sidecars to build');
  if (!target) throw new Error('cannot tell the target: pass --target <triple>');
  const native = target === host;
  const triples = TRIPLES[target] ?? [target];
  const select = sidecars.flatMap((s) => ['-p', s.package, '--bin', s.bin]);
  const release = profile === 'release' ? ['--release'] : profileDir(profile) === 'debug' ? [] : ['--profile', profile];
  const manifest = manifestPath ? ['--manifest-path', manifestPath] : [];
  const builds = native ? [['build', ...manifest, ...select, ...release]] : triples.map((t) => ['build', ...manifest, ...select, '--target', t, ...release]);
  const ext = target.includes('-windows-') ? '.exe' : '';
  const dir = profileDir(profile);
  const binaries = sidecars.map((s) => ({
    name: s.name,
    file: `${s.name}-${target}${ext}`,
    built: native ? [join(targetDir, dir, `${s.bin}${ext}`)] : triples.map((t) => join(targetDir, t, dir, `${s.bin}${ext}`)),
  }));
  return { builds, binaries, ext };
}

const realOps = {
  run: (cmd, args, cwd) => execFileSync(cmd, args, { cwd, stdio: 'inherit' }),
  copy: copyFileSync,
  mkdir: (p) => mkdirSync(p, { recursive: true }),
  chmod: chmodSync,
  size: (p) => statSync(p).size,
  log: (m) => console.log(m),
};

/**
 * Builds the sidecars and copies them to `out`. A built file that is empty is an error (a placeholder must never be shipped);
 * a universal macOS build is joined with `lipo`. Returns the files written.
 */
export function buildSidecars({ root, out, sidecars, target, host, profile = 'release', targetDir, manifestPath, ops = {} }) {
  const o = { ...realOps, ...ops };
  const plan = planSidecars({ sidecars, target, host, profile, targetDir: targetDir ?? join(root, 'target'), manifestPath });
  for (const argv of plan.builds) {
    o.log(`cargo ${argv.join(' ')}`);
    o.run('cargo', argv, root);
  }
  o.mkdir(out);
  const written = [];
  for (const { file, built } of plan.binaries) {
    for (const b of built) if (o.size(b) === 0) throw new Error(`${b} is empty: the sidecar did not build`);
    const dest = join(out, file);
    if (built.length > 1) o.run('lipo', ['-create', '-output', dest, ...built], root);
    else o.copy(built[0], dest);
    if (!plan.ext) o.chmod(dest, 0o755);
    o.log(`sidecar: ${dest}`);
    written.push(dest);
  }
  return written;
}

/** The binaries in `dir` for the sidecar names: `<name>` or `<name>-<triple>[.exe]` (what a bundle holds next to the app). */
export function findSidecars(dir, names, list = (d) => readdirSync(d)) {
  const files = list(dir);
  return names.map((name) => {
    const hit = files.find((f) => f === name || f === `${name}.exe` || (f.startsWith(`${name}-`) && /^[a-z0-9_]+-[a-z0-9_]+-[a-z0-9_-]+(\.exe)?$/.test(f.slice(name.length + 1))));
    if (!hit) throw new Error(`no sidecar ${name} in ${dir}`);
    return join(dir, hit);
  });
}

/**
 * Rule 23: starts each sidecar of a built bundle with `--self-test` (as the user's machine would), bounded by `timeoutMs`.
 * Returns { name, file, ok, output } for each; a missing file is a thrown error, a sidecar that fails or hangs is `ok: false`.
 */
export function selfTestSidecars({ dir, names, args = ['--self-test'], timeoutMs = 30_000, spawn = (file, a, t) => spawnSync(file, a, { encoding: 'utf8', timeout: t }), list }) {
  const files = findSidecars(dir, names, list);
  return files.map((file, i) => {
    const r = spawn(file, args, timeoutMs);
    const output = `${r.stdout ?? ''}${r.stderr ?? ''}`.trim();
    const ok = r.status === 0 && !r.error && !r.signal;
    return { name: names[i], file, ok, output: ok ? output : `${output}${r.error ? ` (${r.error.message})` : ''}${r.signal ? ` (ended by ${r.signal})` : ''}`.trim() };
  });
}

/** The command line: `build` and `self-test`. Returns the exit code; `io` carries the environment for tests. */
export function main(argv, io = {}) {
  const log = io.log ?? ((m) => console.log(m));
  const err = io.error ?? ((m) => console.error(m));
  const env = io.env ?? process.env;
  const [command, ...rest] = argv;
  const flags = { sidecar: [] };
  const bools = new Set();
  const valued = new Set(['--root', '--out', '--sidecar', '--profile', '--target', '--manifest-path', '--dir', '--timeout']);
  try {
    for (let i = 0; i < rest.length; i++) {
      const a = rest[i];
      if (valued.has(a)) {
        const v = rest[++i];
        if (v === undefined) throw new Error(`${a} needs a value`);
        if (a === '--sidecar') flags.sidecar.push(v);
        else flags[a.slice(2)] = v;
      } else if (a === '--dev') bools.add('dev');
      else throw new Error(`unexpected argument: ${a}`);
    }
    if (!flags.sidecar.length) throw new Error('name at least one --sidecar');
    if (command === 'build') {
      const root = resolve(flags.root ?? '.');
      const host = (io.host ?? hostTriple)();
      const sidecars = flags.sidecar.map(parseSidecarSpec);
      const profile = bools.has('dev') ? 'debug' : (flags.profile ?? env['PADDOCK_CARGO_PROFILE'] ?? 'release');
      const target = flags.target ?? env['TAURI_ENV_TARGET_TRIPLE'] ?? host;
      const targetDir = cargoTargetDir(root, env);
      const written = buildSidecars({ root, out: resolve(flags.out ?? join(root, 'binaries')), sidecars, target, host, profile, targetDir, manifestPath: flags['manifest-path'], ops: { ...io.ops, log } });
      return written.length ? 0 : 1;
    }
    if (command === 'self-test') {
      if (!flags.dir) throw new Error('--dir <folder with the sidecars> is needed');
      const results = selfTestSidecars({ dir: resolve(flags.dir), names: flags.sidecar, timeoutMs: flags.timeout ? Number(flags.timeout) : undefined, spawn: io.spawn, list: io.list });
      for (const r of results) (r.ok ? log : err)(`${r.ok ? 'ok  ' : 'FAIL'} ${r.name}${r.output ? `: ${r.output.split('\n').join(' | ')}` : ''}`);
      return results.every((r) => r.ok) ? 0 : 1;
    }
    throw new Error('usage: paddock-sidecar build|self-test --sidecar name[=package[:bin]] ...');
  } catch (e) {
    err(`paddock-sidecar: ${e.message}`);
    return 2;
  }
}

