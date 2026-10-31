// C59: an app that wires the shared updater says how it installs (ER-21; ADR 0034 §1 spirit: one mechanism, one check).
// Pitcrew 1.1 built its `UpdatesBuilder` with a public key and no `.installer(...)`: the update downloaded, then "Restart to
// update" failed with "this build has no installer" and "Install when the app quits" recorded a failed install the user met
// at the next start. A second app had the same wiring trap. The builder now refuses (at the app's start, with or without a
// key) one that says neither `.installer(..)` (the `PluginInstaller` of feature `install`) nor `.no_install()`; this check
// is the static half: it does not depend on the CI's key or on a start test running. In the production Rust of an app's
// `src-tauri/` (as C56), a file that calls `UpdatesBuilder::new(` must also call `.installer(` or `.no_install(`.
// Not looked at: comments, strings, `tests/`, benches and every `#[cfg(test)]` item.
import { error } from '../finding.mjs';
import { withoutTestItems } from '../util/rust-test-items.mjs';
import { maskRust } from './C39-freed-port.mjs';

const SRC_TAURI = /(^|\/)src-tauri\/.*\.rs$/;
const TEST_FILE = /(^|\/)(?:tests?|benches)(?:\.rs$|\/)/;

/** Problems in the production part of a Rust source: [{ line, message }]. */
export function updaterInstallerProblems(source) {
  const mask = withoutTestItems(maskRust(source));
  const first = /\bUpdatesBuilder::new\s*\(/.exec(mask);
  if (!first) return [];
  if (/\.\s*(?:installer|no_install)\s*\(/.test(mask)) return [];
  return [
    {
      line: mask.slice(0, first.index).split('\n').length,
      message:
        'UpdatesBuilder::new(..) without .installer(..) or .no_install(): an update is downloaded but "Restart to update" fails with "this build has no installer"',
    },
  ];
}

export default {
  id: 'C59',
  title: 'Every app that wires the shared updater says how it installs',
  rule: 43,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!SRC_TAURI.test(f) || TEST_FILE.test(f)) continue;
      const text = ctx.read(f);
      if (!text || !text.includes('UpdatesBuilder')) continue;
      for (const p of updaterInstallerProblems(text)) {
        out.push(
          error('C59', 43, f, p.line, p.message, 'Add `.installer(Arc::new(paddock_updater::tauri::PluginInstaller::new(app.clone(), key)))` (feature `install`), or `.no_install()` for a build that only notifies.'),
        );
      }
    }
    return out;
  },
};
