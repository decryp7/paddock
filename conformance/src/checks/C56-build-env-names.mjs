// C56: every app that shows the build's commit and date reads the names the build-env action sets (ER-21). Dyno 1.0 called
// `.build_info(option_env!("DYNO_COMMIT"), option_env!("DYNO_BUILT"))` while build-env sets PADDOCK_BUILD_COMMIT and
// PADDOCK_BUILD_DATE, so its About page showed no commit and no date; the shell-demo's `AppInfo::new` commit had the same
// bug. The names come from the code that sets them (`buildEnv` in conformance/ci/build-env.mjs, in order commit, date).
// Checked in the production Rust of an app's `src-tauri/` (so it works from the repo root and from one app's folder):
// `.build_info(` must pass `option_env!` of exactly those two names, and `AppInfo::new(` must read exactly the commit name.
// Not looked at: comments, strings, `tests/`, benches and every `#[cfg(test)]` item (as C45).
import { buildEnv } from '../../ci/build-env.mjs';
import { error } from '../finding.mjs';
import { withoutTestItems } from '../util/rust-test-items.mjs';
import { maskRust } from './C39-freed-port.mjs';

/** The names build-env sets, in order: [commit, date]. */
export const buildEnvNames = () => Object.keys(buildEnv({ env: {}, head: () => null, now: () => new Date(0) }));

const SRC_TAURI = /(^|\/)src-tauri\/.*\.rs$/;
const TEST_FILE = /(^|\/)(?:tests?|benches)(?:\.rs$|\/)/;

/** Problems in the production part of a Rust source: [{ line, message }]. */
export function buildInfoProblems(source, names = buildEnvNames()) {
  const mask = withoutTestItems(maskRust(source));
  const out = [];
  for (const m of mask.matchAll(/\.build_info\s*\(|\bAppInfo::new\s*\(/g)) {
    let depth = 1;
    let i = m.index + m[0].length;
    const from = i;
    for (; i < mask.length && depth > 0; i++) depth += mask[i] === '(' ? 1 : mask[i] === ')' ? -1 : 0;
    // Names are read from the original text; the mask has the same offsets with string insides blanked.
    const read = [...source.slice(from, i).matchAll(/option_env!\s*\(\s*"([^"]*)"\s*\)/g)].map((x) => x[1]);
    const line = mask.slice(0, m.index).split('\n').length + (mask[m.index] === '\n' ? 1 : 0);
    const info = m[0].startsWith('.build_info');
    const ok = info ? read.length === names.length && read.every((n, k) => n === names[k]) : read.length === 1 && read[0] === names[0];
    if (!ok) {
      out.push({
        line,
        message: `${info ? '.build_info' : 'AppInfo::new'} reads ${read.length ? read.join(' and ') : 'no option_env! names'}, but build-env sets ${info ? names.join(' and ') : `exactly ${names[0]}`}: the About page shows no commit${info ? ' or date' : ''}`,
      });
    }
  }
  return out;
}

export default {
  id: 'C56',
  scope: 'both',
  mode: 'gate',
  title: 'Every app reads the build commit and date under the names build-env sets',
  rule: 43,
  run(ctx) {
    const names = buildEnvNames();
    const out = [];
    for (const f of ctx.files()) {
      if (!SRC_TAURI.test(f) || TEST_FILE.test(f)) continue;
      const text = ctx.read(f);
      if (!text || !/\.build_info\s*\(|\bAppInfo::new\s*\(/.test(text)) continue;
      for (const p of buildInfoProblems(text, names)) {
        out.push(error('C56', 43, f, p.line, p.message, `Use option_env!("${names[0]}") for the commit and option_env!("${names[1]}") for the date (build-env sets them).`));
      }
    }
    return out;
  },
};
