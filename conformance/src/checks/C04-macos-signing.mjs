// C04: macOS bundle signing. Static part: ad-hoc signing with the hardened runtime off (rule 23), CI verifies the .app and
// the DMG with `codesign --verify --deep --strict`, the verify script never matches `cmd | grep -q` under pipefail.
// Bundle part (--bundle <dir>): the shared macos-verify-bundle.sh runs on the real bundle, sidecars started with their self-test.
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { error } from '../finding.mjs';
import { lineOf } from '../util/fs.mjs';

export const VERIFY_SCRIPT = fileURLToPath(new URL('../../release/macos-verify-bundle.sh', import.meta.url));

/** `.sh` / `.mjs` paths mentioned in a workflow that exist in the tree. */
export function referencedScripts(ctx, text) {
  const paths = new Set(text.match(/[\w./-]+\.(?:sh|mjs)\b/g) ?? []);
  return [...paths].map((p) => p.replace(/^\.\//, '')).filter((p) => ctx.exists(p));
}

function macosConfig(ctx) {
  const dir = ctx.config.tauriDir;
  for (const f of [`${dir}/tauri.macos.conf.json`, `${dir}/tauri.conf.json`]) {
    const text = ctx.read(f);
    if (!text) continue;
    try {
      const m = JSON.parse(text)?.bundle?.macOS ?? JSON.parse(text)?.tauri?.bundle?.macOS;
      if (m) return { file: f, macOS: m, text };
    } catch {
      /* reported by the caller as unreadable */
    }
  }
  return null;
}

/** Runs the shared verify script; returns findings from its ::error:: lines and its exit status. */
export function verifyBundle(ctx, bundle) {
  const args = [VERIFY_SCRIPT, '--bundle-dir', bundle];
  for (const s of ctx.config.sidecars) args.push('--sidecar', `${s.name}:${(s.args ?? ['--self-test']).join(' ')}`);
  const run = (ctx.options.spawn ?? spawnSync)('bash', args, { encoding: 'utf8', env: { ...process.env, ...(ctx.options.bundleEnv ?? {}) }, timeout: 300_000 });
  const text = `${run.stdout ?? ''}\n${run.stderr ?? ''}`;
  if (run.status === 0) return [];
  const errs = [...text.matchAll(/::error::(.*)/g)].map((m) => m[1]);
  return (errs.length ? errs : [`macos-verify-bundle.sh exited with ${run.status ?? run.error?.message}`]).map((m) =>
    error('C04', 23, bundle, 1, m, 'Fix the bundle (ad-hoc seal, one Team ID for every Mach-O, sidecars that start), then run the verify again.'),
  );
}

export default {
  id: 'C04',
  scope: 'apps',
  mode: 'gate',
  needs: 'rust',
  title: 'macOS bundle is sealed ad hoc, verified in CI, sidecars start',
  rule: 23,
  run(ctx) {
    const out = [];
    const conf = macosConfig(ctx);
    if (!conf) out.push(error('C04', 23, ctx.config.tauriDir, 1, 'no bundle.macOS section found in tauri.macos.conf.json or tauri.conf.json', 'Set bundle.macOS.signingIdentity "-" and hardenedRuntime false.'));
    else {
      const { macOS, file, text } = conf;
      if (macOS.signingIdentity !== '-') out.push(error('C04', 23, file, lineOf(text, /signingIdentity/), 'bundle.macOS.signingIdentity is not "-" (ad-hoc seal)', 'Set signingIdentity to "-" until a real identity exists.'));
      else if (macOS.hardenedRuntime !== false) {
        out.push(error('C04', 23, file, lineOf(text, /signingIdentity/), 'hardenedRuntime is not false while signingIdentity is "-" (Tauri turns it on by default; ad-hoc code under it cannot load vendor-signed libraries)', 'Add "hardenedRuntime": false next to signingIdentity.'));
      }
    }

    // CI verifies the .app and the DMG.
    const wf = ctx.workflows();
    const corpus = wf.map((w) => w.text).concat(wf.flatMap((w) => referencedScripts(ctx, w.text).map((p) => ctx.read(p) ?? ''))).join('\n');
    const shared = /conformance\/release\/macos-verify-bundle|paddock-conformance|\.gitea\/actions\/(?:macos-verify|tauri-bundle)|workflows\/app-release\.yml/.test(corpus);
    const noComments = corpus.replace(/^\s*#.*$/gm, '');
    if (!shared && !/codesign\s+--verify\b[^\n]*--deep[^\n]*--strict|codesign\s+--verify\b[^\n]*--strict[^\n]*--deep/.test(noComments)) {
      out.push(error('C04', 23, ctx.config.workflowsDir, 1, 'no CI step runs `codesign --verify --deep --strict` on the built .app', 'Run macos-verify-bundle.sh (conformance/release) after the macOS bundle step.'));
    } else if (!shared && !/hdiutil\s+attach/.test(noComments)) {
      out.push(error('C04', 23, ctx.config.workflowsDir, 1, 'CI verifies the .app but not the app inside the DMG (rule 23)', 'Run macos-verify-bundle.sh: it mounts the DMG and verifies the copy inside.'));
    }

    // A secret that is not set arrives as an empty APPLE_SIGNING_IDENTITY, and Tauri lets the (empty) variable override
    // signingIdentity "-": codesign then fails with `Signing with identity ""`. The job must unset it while it is empty.
    for (const w of wf) {
      const own = w.text.replace(/^\s*#.*$/gm, '');
      if (!/APPLE_SIGNING_IDENTITY\s*:\s*\$\{\{\s*secrets\./.test(own)) continue;
      const jobText = [own, ...referencedScripts(ctx, w.text).map((p) => (ctx.read(p) ?? '').replace(/^\s*#.*$/gm, ''))].join('\n');
      if (!/\bunset\s+APPLE_SIGNING_IDENTITY\b/.test(jobText)) {
        out.push(error('C04', 23, w.file, lineOf(w.text, /APPLE_SIGNING_IDENTITY/), 'APPLE_SIGNING_IDENTITY comes from a secret and reaches tauri build empty while the secret is unset, which overrides signingIdentity "-" with "" (`Signing with identity ""`)', 'Run `[ -n "${APPLE_SIGNING_IDENTITY:-}" ] || unset APPLE_SIGNING_IDENTITY` before tauri build.'));
      }
    }

    // The lesson: `cmd | grep -q` under pipefail reports a signed bundle as unsigned.
    for (const f of ctx.files().filter((p) => p.endsWith('.sh'))) {
      const text = ctx.read(f) ?? '';
      if (!/set\s+-[a-z]*o\s+pipefail|set\s+-[a-z]*e[a-z]*u?o?\s*pipefail|set\s+-euo\s+pipefail/.test(text)) continue;
      const re = /\|\s*grep\s+(?:-[A-Za-z]*q[A-Za-z]*|--quiet)\b/g;
      let m;
      while ((m = re.exec(text))) {
        const line = text.slice(0, m.index).split('\n').length;
        out.push(error('C04', 23, f, line, '`cmd | grep -q` under pipefail: grep exits early, the writer gets SIGPIPE and the pipeline reports failure although the pattern matched (docs/lessons.md)', 'Capture the output first (out=$(cmd 2>&1)), then match it with grep <<<"$out".'));
      }
    }

    if (ctx.options.bundle) out.push(...verifyBundle(ctx, ctx.options.bundle));
    return out;
  },
};
