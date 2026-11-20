// The sidecar build and the macOS helper's signature files, shared by sidecar.mjs (Tauri's beforeBuildCommand, which never sees the
// updater key) and sign-helper.mjs (the release's own signing step, which does; ADR 0085 amendment 0085-2026-11-12, WP-4 item 11).
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isTagRun } from '../../../../../conformance/src/util/is-tag-run.mjs';
import { cargoTargetDir } from '../../../../../conformance/src/sidecar/sidecar.mjs';

// [binary name, package]: the CLI's binary is `dyno`, its package `dyno-cli`. The helper (ADR 0027) ships next to the agent on every
// OS: the program the one OS prompt runs to install it (macOS, Windows), `/usr/bin/dyno-helper` in the .deb; the AppImage never uses it.
export const SIDECARS = [['dyno-agent', 'dyno-agent'], ['dyno', 'dyno-cli'], ['dyno-bench', 'dyno-bench'], ['dyno-bench-gpu', 'dyno-bench-gpu'], ['dyno-helper', 'dyno-helper']];

const here = dirname(fileURLToPath(import.meta.url));

/** The app's folder, target triple and profile, as Tauri's hook and the signing step both see them. */
export function context({ dev = false, env = process.env, root = resolve(here, '..', '..', '..'), exec = execFileSync } = {}) {
  const host = /host: (\S+)/.exec(exec('rustc', ['-vV'], { encoding: 'utf8' }))?.[1];
  const triple = env.TAURI_ENV_TARGET_TRIPLE || host;
  if (!triple) throw new Error('cannot tell the target triple (rustc -vV)');
  const binaries = join(root, 'app', 'src-tauri', 'binaries');
  return {
    root, dev, host, triple, env, exec, binaries,
    profile: dev ? 'dev' : 'dist',
    exe: triple.includes('windows') ? '.exe' : '',
    apple: triple.includes('apple-darwin'),
    helper: join(binaries, `dyno-helper-${triple}`),
    sig: join(binaries, 'dyno-helper.minisig'),
    hashFile: join(binaries, 'dyno-helper.sha256'),
  };
}

/**
 * Builds every sidecar in one cargo call (the hook and the signing step both do, so feature unification, and so the helper's bytes, are
 * the same) and copies each to binaries/<name>-<triple>[.exe], except those in `keep`.
 */
export function buildSidecars(ctx, { keep = [] } = {}) {
  const chosen = SIDECARS;
  const args = ['build', ...chosen.flatMap(([, pkg]) => ['-p', pkg]), '--profile', ctx.profile];
  if (ctx.triple !== ctx.host) args.push('--target', ctx.triple);
  ctx.exec('cargo', args, { cwd: ctx.root, stdio: 'inherit' });
  const targetDir = cargoTargetDir(ctx.root);
  for (const [name] of chosen) {
    if (keep.includes(name)) continue;
    const built = join(targetDir, ...(ctx.triple !== ctx.host ? [ctx.triple] : []), ctx.dev ? 'debug' : 'dist', `${name}${ctx.exe}`);
    const out = join(ctx.binaries, `${name}-${ctx.triple}${ctx.exe}`);
    mkdirSync(dirname(out), { recursive: true });
    copyFileSync(built, out);
    console.log(`sidecar: ${out}`);
  }
}

/** A release is a tag run (defined once, in conformance/src/util/is-tag-run.mjs; check-bundle.sh asks the same module). */
export const isRelease = isTagRun;

export const sha256 = (file) => createHash('sha256').update(readFileSync(file)).digest('hex');

/** True when the helper, its non-empty signature and the hash of those exact bytes are all there (what the signing step leaves). */
export function helperSigned(ctx) {
  try {
    if (!existsSync(ctx.helper)) return false;
    const hash = readFileSync(ctx.hashFile, 'utf8').trim();
    return hash !== '' && readFileSync(ctx.sig).length > 0 && hash === sha256(ctx.helper);
  } catch {
    return false;
  }
}

/** The files an unsigned build leaves: the elevated install then refuses with "no signature file", it never installs unchecked. */
export function writeUnsigned(ctx) {
  mkdirSync(ctx.binaries, { recursive: true });
  writeFileSync(ctx.sig, '');
  writeFileSync(ctx.hashFile, '');
}
