// C54: work a person waits for never runs in macOS QoS BACKGROUND (rule 52). The class keeps a thread on the efficiency
// cores and behind every other thread, so a caller that waits for it (a report on its analysis pool, an import on a lock
// the indexer holds) waits as long as the machine is busy: `debrief report` timed out on the shared CI Mac (Debrief ADR
// 0052, family ADR 0020's classes). Lowered work uses `QOS_CLASS_UTILITY`. Flagged: every `QOS_CLASS_BACKGROUND` in Rust
// code (comments and strings do not count). Work nobody waits on carries
// `// conformance: background-qos — <why nobody waits on it>` on the line or the line above.
import { error } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';

const WAIVER = /conformance:\s*background-qos\s*[—–-]+\s*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*background-qos/;

/** The `QOS_CLASS_BACKGROUND` uses in a Rust source: [{ line, waived: false | 'no-reason' | 'reason' }]. */
export function backgroundQos(text) {
  const clean = maskRust(text);
  const raw = text.split('\n');
  const out = [];
  clean.split('\n').forEach((code, i) => {
    if (!/\bQOS_CLASS_BACKGROUND\b/.test(code)) return;
    const here = `${raw[i] ?? ''}\n${raw[i - 1] ?? ''}`;
    out.push({ line: i + 1, waived: WAIVER.test(here) ? 'reason' : BARE_WAIVER.test(here) ? 'no-reason' : false });
  });
  return out;
}

export default {
  id: 'C54',
  title: 'Work a person waits for never runs in macOS QoS BACKGROUND',
  rule: 52,
  run(ctx) {
    const out = [];
    const fix =
      'Use `QOS_CLASS_UTILITY` for lowered work a caller may wait for (rule 52); work nobody waits on carries `// conformance: background-qos — <why nobody waits on it>`.';
    for (const f of ctx.files()) {
      if (!f.endsWith('.rs')) continue;
      const text = ctx.read(f);
      if (!text || !text.includes('QOS_CLASS_BACKGROUND')) continue;
      for (const u of backgroundQos(text)) {
        if (u.waived === 'reason') continue;
        if (u.waived === 'no-reason') out.push(error('C54', 52, f, u.line, 'the background-qos waiver gives no reason', 'Write `// conformance: background-qos — <why nobody waits on it>`.'));
        else out.push(error('C54', 52, f, u.line, 'a thread in QoS BACKGROUND: a caller waiting for it waits as long as the machine is busy', fix));
      }
    }
    return out;
  },
};
