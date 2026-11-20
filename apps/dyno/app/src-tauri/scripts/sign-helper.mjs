// The release's own step that signs the macOS dyno-helper (Security review F2, ADR 0027 amendment): on macOS the helper that the one
// password prompt copies into /Library as root sits in a user-owned bundle, so the release signs its final bytes with the family update key.
// tauri-bundle.sh runs this as `--pre-sign`, in two parts, because the key must reach the signing and nothing that runs branch code:
//   node sign-helper.mjs build   builds the helper (the same build sidecar.mjs does) and seals it ad hoc; no key in the environment
//   node sign-helper.mjs sign    signs those bytes with the key (TAURI_SIGNING_PRIVATE_KEY in this one process's environment)
// sidecar.mjs, in Tauri's beforeBuildCommand, never signs and never needs the key (ADR 0085 amendment 0085-2026-11-12, item 11): it uses
// the helper this step left, or on a tag fails naming this step. Tauri re-signs every sidecar ad hoc (`codesign --force -s <identity>`; with the
// hardened runtime only for a real Apple identity), so the helper is sealed here the same way first. Measured 2026-11-04 and again in
// conformance/test/dyno-helper-sign.test.mjs: an ad-hoc `codesign --force -s -` of an ad-hoc signed dyno-helper under another file name leaves the
// bytes identical (the identifier comes from the embedded Info.plist), so Tauri ships the bytes signed here. With a Developer ID the bytes
// would change: check-bundle.sh then fails loudly, and that release signs after the bundle instead.
// Outputs: binaries/dyno-helper.minisig (bundled into Contents/Resources by tauri.macos.conf.json) and binaries/dyno-helper.sha256, the
// hash it covers, which scripts/check-bundle.sh compares with the helper inside the built .app. Without a key off a tag (forks, pull
// requests, dev) both files are empty; on a tag a missing key is an error.
import { copyFileSync, mkdtempSync, realpathSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildSidecars, context, isRelease, sha256, writeUnsigned } from './sidecar-lib.mjs';

/** Builds the helper and seals it ad hoc. Off macOS there is nothing to do. */
export function buildHelper(ctx) {
  if (!ctx.apple) return console.log('sign-helper: not a macOS target, nothing to build');
  writeUnsigned(ctx); // a signature left by an earlier run never describes these bytes
  buildSidecars(ctx, { names: ['dyno-helper'] });
  ctx.exec('codesign', ['--force', '-s', '-', ctx.helper], { stdio: 'inherit' });
}

/** Signs the sealed helper under `dyno-helper-<version>-<triple>` and writes the signature and hash files. Returns the exit code. */
export function signHelper(ctx) {
  if (!ctx.apple) return 0;
  if (!ctx.env.TAURI_SIGNING_PRIVATE_KEY) {
    if (isRelease(ctx.env)) {
      console.log('::error::sign-helper: TAURI_SIGNING_PRIVATE_KEY did not reach the helper signing step on a tag run. Next step: check that tauri-bundle.sh passed the key (updater_key_exec) and that the release job has the secret.');
      return 1;
    }
    writeUnsigned(ctx);
    console.log('::warning::no TAURI_SIGNING_PRIVATE_KEY: the bundled helper carries no signature, its install will be refused');
    return 0;
  }
  // The signature's trusted comment carries the signed file's name, so the helper is signed under `dyno-helper-<version>-<triple>`:
  // the elevated install requires exactly its own version and architecture, and no older family-signed helper passes.
  const version = JSON.parse(readFileSync(join(ctx.root, 'app', 'src-tauri', 'tauri.conf.json'), 'utf8')).version;
  const dir = mkdtempSync(join(tmpdir(), 'dyno-helper-sign-'));
  try {
    const named = join(dir, `dyno-helper-${version}-${ctx.triple}`);
    copyFileSync(ctx.helper, named);
    ctx.exec('npx', ['tauri', 'signer', 'sign', named], { cwd: join(ctx.root, 'app'), stdio: 'inherit', env: ctx.env });
    copyFileSync(`${named}.sig`, ctx.sig);
    writeFileSync(ctx.hashFile, sha256(ctx.helper) + '\n');
    console.log(`helper signature: ${ctx.sig} covers sha256 ${readFileSync(ctx.hashFile, 'utf8').trim()}`);
    return 0;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

if (process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) {
  const mode = process.argv[2];
  if (mode !== 'build' && mode !== 'sign') {
    console.log('::error::usage: node sign-helper.mjs build|sign');
    process.exit(2);
  }
  try {
    const ctx = context({});
    if (mode === 'build') buildHelper(ctx);
    else process.exitCode = signHelper(ctx);
  } catch (e) {
    console.log(`::error::sign-helper ${mode} failed: ${e.message}. Next step: the lines above name the failing command; run it in apps/dyno.`);
    process.exitCode = 1;
  }
}
