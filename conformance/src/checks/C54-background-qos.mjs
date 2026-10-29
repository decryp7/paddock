// C54: work a person waits for never runs at an OS's starvable background level (rule 52). Those levels put a thread
// behind every other one (and most also throttle its I/O and memory), so a caller that waits for it (a report on its
// analysis pool, an import on a lock the indexer holds) waits as long as the machine is busy: `debrief report` timed out on
// the shared CI Mac (Debrief ADR 0052, family ADR 0020's classes). Lowered work uses the gentle level: `QOS_CLASS_UTILITY`
// on macOS, nice 10 on Linux, `THREAD_PRIORITY_BELOW_NORMAL` on Windows. Flagged in Rust code (comments and strings do not
// count): the levels of `STARVABLE`. Work nobody waits on carries
// `// conformance: background-qos — <why nobody waits on it>` on the line or the line above.
import { error } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';

const WAIVER = /conformance:\s*background-qos\s*[—–-]+\s*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*background-qos/;

/** The starvable levels, by identifier, with the OS and what the level does. */
export const STARVABLE = {
  QOS_CLASS_BACKGROUND: 'macOS QoS BACKGROUND (efficiency cores only, behind every other thread, throttled I/O)',
  THREAD_MODE_BACKGROUND_BEGIN: 'Windows thread background mode (lowest CPU, I/O and memory priority)',
  PROCESS_MODE_BACKGROUND_BEGIN: 'Windows process background mode (lowest CPU, I/O and memory priority)',
  IDLE_PRIORITY_CLASS: 'Windows idle priority class (runs only when nothing else does)',
  THREAD_PRIORITY_IDLE: 'Windows idle thread priority (runs only when nothing else does)',
  THREAD_PRIORITY_LOWEST: 'Windows lowest thread priority',
  SCHED_IDLE: 'Linux SCHED_IDLE (runs only when nothing else does)',
  IOPRIO_CLASS_IDLE: 'Linux idle I/O class (disk access only when nothing else uses it)',
};
const PATTERN = new RegExp(`\\b(${Object.keys(STARVABLE).join('|')})\\b`, 'g');

/** The starvable-level uses in a Rust source: [{ line, name, waived: false | 'no-reason' | 'reason' }]. */
export function backgroundQos(text) {
  const clean = maskRust(text);
  const raw = text.split('\n');
  const out = [];
  clean.split('\n').forEach((code, i) => {
    const here = `${raw[i] ?? ''}\n${raw[i - 1] ?? ''}`;
    const waived = WAIVER.test(here) ? 'reason' : BARE_WAIVER.test(here) ? 'no-reason' : false;
    for (const m of code.matchAll(PATTERN)) out.push({ line: i + 1, name: m[1], waived });
  });
  return out;
}

export default {
  id: 'C54',
  title: 'Work a person waits for never runs at an OS starvable background level',
  rule: 52,
  run(ctx) {
    const out = [];
    const fix =
      'Lower work a caller may wait for gently (rule 52): `QOS_CLASS_UTILITY` on macOS, nice 10 on Linux, `THREAD_PRIORITY_BELOW_NORMAL` on Windows; work nobody waits on carries `// conformance: background-qos — <why nobody waits on it>`.';
    for (const f of ctx.files()) {
      if (!f.endsWith('.rs')) continue;
      const text = ctx.read(f);
      if (!text || !Object.keys(STARVABLE).some((k) => text.includes(k))) continue;
      for (const u of backgroundQos(text)) {
        if (u.waived === 'reason') continue;
        if (u.waived === 'no-reason') out.push(error('C54', 52, f, u.line, 'the background-qos waiver gives no reason', 'Write `// conformance: background-qos — <why nobody waits on it>`.'));
        else out.push(error('C54', 52, f, u.line, `${u.name}: ${STARVABLE[u.name]}; a caller waiting for this work waits as long as the machine is busy`, fix));
      }
    }
    return out;
  },
};
