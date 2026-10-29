// C52: Pacenotes never turns on `paddock-repo-graph`'s `cargo-metadata` feature (ADR 0022 §6, plan 2026-10-29 P0.1b).
// The feature runs `cargo metadata`, which executes build scripts' environment probes and proc-macro resolution of the
// analysed repository's tooling; Pacenotes reads other people's repositories and must never run their code (AGENTS.md
// "Never execute repository code"). Flagged in every manifest under apps/pacenotes/: the feature named in the
// `features` of a `paddock-repo-graph` dependency (inline table, `[dependencies.paddock-repo-graph]` table, workspace
// dependency), or forwarded by one of the app's own features as `paddock-repo-graph/cargo-metadata`.
import { error } from '../finding.mjs';

const APP = 'apps/pacenotes/';
const FEATURE = 'cargo-metadata';
const CRATE = 'paddock-repo-graph';

/** Manifest lines (1-based) that switch the feature on. */
export function enablingLines(text) {
  const lines = text.split('\n');
  const hits = [];
  let inCrateTable = false;
  lines.forEach((raw, i) => {
    const line = raw.replace(/#.*$/, '');
    const header = /^\s*\[([^\]]+)\]/.exec(line);
    if (header) inCrateTable = new RegExp(`(^|\\.)${CRATE}$`).test(header[1].trim());
    const quoted = new RegExp(`["']${FEATURE}["']`);
    const forwarded = new RegExp(`["']${CRATE}\\??/${FEATURE}["']`).test(line);
    const inline = new RegExp(`(^|[\\s{,])${CRATE}\\s*=`).test(line) && /features\s*=/.test(line) && quoted.test(line);
    const tableFeatures = inCrateTable && /^\s*features\s*=/.test(line) && quoted.test(line);
    if (forwarded || inline || tableFeatures) hits.push(i + 1);
  });
  return hits;
}

export default {
  id: 'C52',
  title: "Pacenotes does not enable paddock-repo-graph's cargo-metadata feature",
  rule: 40,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!f.startsWith(APP) || !f.endsWith('Cargo.toml')) continue;
      for (const line of enablingLines(ctx.read(f) ?? '')) {
        out.push(error('C52', 40, f, line, `${f} enables ${CRATE}'s ${FEATURE} feature`, 'Remove the feature: it runs `cargo metadata` on repositories Pacenotes only reads. Read Cargo manifests with the crate\'s own parser (ADR 0022 §6).'));
      }
    }
    return out;
  },
};
