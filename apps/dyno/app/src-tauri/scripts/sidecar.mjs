// Builds dyno-agent, the dyno command (crate dyno-cli) and the benchmark runner dyno-bench (ADR 0030) and puts them where Tauri's `externalBin` expects them:
// app/src-tauri/binaries/<name>-<target triple>[.exe]. Run by `beforeBuildCommand` / `beforeDevCommand`
// (tauri.conf.json); `--dev` builds the debug profile. Honours CARGO_TARGET_DIR, a build.target-dir in the cargo configuration (CI's .cargo/ci.toml) and TAURI_ENV_TARGET_TRIPLE.
//
// This hook never signs and never needs the updater key (ADR 0085 amendment 0085-2026-11-12, item 11: the key is stripped before npm and
// this hook). On macOS the release's own step (sign-helper.mjs, run by tauri-bundle.sh as --pre-sign) built, sealed and signed the helper
// first: this script keeps those bytes. Without them it leaves empty signature files, and on a tag it fails naming that step.
import { buildSidecars, context, helperSigned, isRelease, writeUnsigned } from './sidecar-lib.mjs';

const ctx = context({ dev: process.argv.includes('--dev') });
const prebuilt = ctx.apple && helperSigned(ctx);
buildSidecars(ctx, { keep: prebuilt ? ['dyno-helper'] : [] });
if (ctx.apple) {
  if (prebuilt) {
    console.log(`sidecar: kept the helper signed by sign-helper.mjs (${ctx.helper})`);
  } else if (isRelease(ctx.env)) {
    console.log('::error::the macOS dyno-helper has no signature on a tag run: the signing step (sign-helper.mjs, run by tauri-bundle.sh as --pre-sign) did not leave a signed helper. Next step: check that the release job passes `pre-sign` and that TAURI_SIGNING_PRIVATE_KEY reached it.');
    process.exit(1);
  } else {
    writeUnsigned(ctx);
    console.log('::warning::no signed helper from sign-helper.mjs: the bundled helper carries no signature, its install will be refused');
  }
}
