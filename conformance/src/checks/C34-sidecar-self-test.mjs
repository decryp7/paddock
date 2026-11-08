// C34: every sidecar answers `--self-test` and CI starts it from the built bundle (rule 23, plan X34). Each `bundle.externalBin`
// of the Tauri config is (1) listed in `paddock.conformance.json` "sidecars", which C04 starts from the bundle with its self-test
// arguments, and (2) implemented: the crate that builds it handles the flag (`paddock_proc::self_test`, or its own check of
// `--self-test`).
import { error, warn } from '../finding.mjs';

const SELF_TEST = /--self-test|self_test\b|SELF_TEST\b/;

/** Names (`binaries/x` -> `x`) of the sidecars in a Tauri config text. */
export function externalBins(text) {
  try {
    const conf = JSON.parse(text);
    return (conf?.bundle?.externalBin ?? []).filter((b) => typeof b === 'string').map((b) => b.split('/').pop());
  } catch {
    return [];
  }
}

/** Directory (relative POSIX path) of the crate that builds binary `name`, or null. */
export function crateDirOf(ctx, name) {
  for (const f of ctx.files()) {
    if (!f.endsWith('/Cargo.toml') && f !== 'Cargo.toml') continue;
    const text = ctx.read(f) ?? '';
    const bin = new RegExp(`\\[\\[bin\\]\\][^\\[]*?name\\s*=\\s*"${name}"`, 's').test(text);
    const pkgDefault = new RegExp(`\\[package\\][^\\[]*?name\\s*=\\s*"${name}"`, 's').test(text);
    if (bin || pkgDefault) return f.includes('/') ? f.slice(0, f.lastIndexOf('/')) : '';
  }
  return null;
}

export default {
  id: 'C34',
  scope: 'apps',
  mode: 'gate',
  needs: 'rust',
  title: 'Sidecars implement --self-test and CI starts them from the bundle',
  rule: 23,
  run(ctx) {
    const out = [];
    const confs = ctx.files().filter((f) => f.startsWith(`${ctx.config.tauriDir}/`) && /(?:^|\/)tauri[^/]*\.conf\.json$/.test(f) && f.split('/').length === ctx.config.tauriDir.split('/').length + 1);
    const declared = new Set((ctx.config.sidecars ?? []).map((s) => s.name));
    const seen = new Set();
    for (const conf of confs) {
      for (const name of externalBins(ctx.read(conf) ?? '')) {
        if (seen.has(name)) continue;
        seen.add(name);
        if (!declared.has(name)) out.push(error('C34', 23, conf, 1, `the sidecar ${name} is not in "sidecars" of paddock.conformance.json, so CI never starts it from the bundle`, `Add { "name": "${name}", "args": ["--self-test"] } to "sidecars".`));
        const dir = crateDirOf(ctx, name);
        if (dir === null) {
          out.push(warn('C34', 23, conf, 1, `cannot find the crate that builds ${name}, so its --self-test is unchecked`, 'Build sidecars from crates of the workspace (a [[bin]] or a package named like the sidecar).'));
          continue;
        }
        const handles = ctx.files().some((f) => f.endsWith('.rs') && (dir === '' || f.startsWith(`${dir}/`)) && SELF_TEST.test(ctx.read(f) ?? ''));
        if (!handles) out.push(error('C34', 23, `${dir ? `${dir}/` : ''}Cargo.toml`, 1, `${name} does not implement --self-test`, 'Call paddock_proc::self_test::handle(...) first in main (it starts, runs its checks, touches nothing and exits 0).'));
      }
    }
    return out;
  },
};
