// C54: work a person waits for never runs at an OS's starvable background level (rule 52). Those levels put a thread
// behind every other one (and most also throttle its I/O and memory), so a caller that waits for it (a report on its
// analysis pool, an import on a lock the indexer holds) waits as long as the machine is busy: `debrief report` timed out on
// the shared CI Mac (Debrief ADR 0052, family ADR 0020's classes). Lowered work uses the gentle level: `QOS_CLASS_UTILITY`
// on macOS, nice 10 on Linux, `THREAD_PRIORITY_BELOW_NORMAL` on Windows. Flagged in Rust code (comments and strings do not
// count): the levels of `STARVABLE` and a `taskpolicy -b` command line in a string. The same levels set for a whole
// service are flagged too, in launchd plists, systemd units and the Rust strings that render them: `ProcessType`
// `Background` (and `Adaptive` without XPC `MachServices`, which then stays throttled), `LowPriorityIO` true, and
// `IOSchedulingClass=idle`, and a `paddock_service::ProcessType::Background` request. Work nobody waits on carries `conformance: background-qos — <why nobody waits on it>` in a
// comment on the line or the line above (`//`, `<!-- -->` or `#`).
import { error } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';

const WAIVER = /conformance:\s*background-qos\s*[—–-]+\s*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*background-qos/;

/** The starvable levels, by identifier, with the OS and what the level does. */
export const STARVABLE = {
  QOS_CLASS_BACKGROUND: 'macOS QoS BACKGROUND (efficiency cores only, behind every other thread, throttled I/O)',
  PRIO_DARWIN_BG: 'macOS process background (taskpolicy -b: efficiency cores only, throttled I/O and GPU)',
  THREAD_MODE_BACKGROUND_BEGIN: 'Windows thread background mode (lowest CPU, I/O and memory priority)',
  PROCESS_MODE_BACKGROUND_BEGIN: 'Windows process background mode (lowest CPU, I/O and memory priority)',
  IDLE_PRIORITY_CLASS: 'Windows idle priority class (runs only when nothing else does)',
  THREAD_PRIORITY_IDLE: 'Windows idle thread priority (runs only when nothing else does)',
  THREAD_PRIORITY_LOWEST: 'Windows lowest thread priority',
  SCHED_IDLE: 'Linux SCHED_IDLE (runs only when nothing else does)',
  IOPRIO_CLASS_IDLE: 'Linux idle I/O class (disk access only when nothing else uses it)',
};
const PATTERN = new RegExp(`\\b(${Object.keys(STARVABLE).join('|')})\\b`, 'g');
const TASKPOLICY = 'taskpolicy -b';
const TASKPOLICY_TEXT = 'macOS taskpolicy -b (the process in background: efficiency cores only, throttled I/O and GPU)';
/** A string literal in code that starts `taskpolicy -b` as one command line, or names `taskpolicy` with a `"-b"` argument
 * on the same or one of the next three lines. */
function taskpolicyAt(raw, clean, i) {
  for (const m of raw[i].matchAll(/"([^"\\]*)/g)) {
    if (clean[i][m.index] !== '"') continue; // inside a comment, or the closing quote of another literal
    if (/(?:^|[\s;&|])taskpolicy\s+-b\b/.test(m[1])) return true;
    if (m[1] === 'taskpolicy' && raw.slice(i, i + 4).some((l) => /"-b"/.test(l))) return true;
  }
  return false;
}

/** The starvable-level uses in a Rust source: [{ line, name, waived: false | 'no-reason' | 'reason' }]. */
export function backgroundQos(text) {
  const clean = maskRust(text);
  const raw = text.split('\n');
  const out = [];
  const cleanLines = clean.split('\n');
  cleanLines.forEach((code, i) => {
    const here = `${raw[i] ?? ''}\n${raw[i - 1] ?? ''}`;
    const waived = WAIVER.test(here) ? 'reason' : BARE_WAIVER.test(here) ? 'no-reason' : false;
    for (const m of code.matchAll(PATTERN)) out.push({ line: i + 1, name: m[1], waived });
    if (raw[i]?.includes('taskpolicy') && taskpolicyAt(raw, cleanLines, i)) out.push({ line: i + 1, name: TASKPOLICY, waived });
  });
  return out;
}

/** `text` with comments blanked but string contents kept (Rust), so a renderer's output text can be matched. */
function rustWithoutComments(text) {
  const clean = maskRust(text);
  let inside = false;
  let out = '';
  for (let i = 0; i < text.length; i++) {
    if (clean[i] === '"') inside = !inside;
    out += inside ? text[i] : clean[i];
  }
  return out;
}

/** `text` with comments blanked: `<!-- -->` in a plist, `#`/`;` lines in a unit, Rust comments (strings kept). */
function withoutComments(text, file) {
  if (file.endsWith('.rs')) return rustWithoutComments(text);
  const blank = (s) => s.replace(/[^\n]/g, ' ');
  if (file.endsWith('.plist')) return text.replace(/<!--[\s\S]*?-->/g, blank);
  return text.replace(/^[ \t]*[#;].*$/gm, blank).replace(/[ \t]+#.*$/gm, blank);
}

// Whitespace in a file, or written as an escape in a Rust string, or the end of one string literal and the start of the
// next (a renderer that writes the key and the value with two calls).
const GAP = String.raw`(?:\s|\\[nt]|"[^"\n]{0,40}"?)*?`;
const SERVICE = [
  { name: 'launchd ProcessType Background', re: new RegExp(String.raw`<key>ProcessType</key>${GAP}<string>Background</string>`, 'g') },
  { name: 'launchd ProcessType Adaptive without XPC', re: new RegExp(String.raw`<key>ProcessType</key>${GAP}<string>Adaptive</string>`, 'g'), unlessXpc: true },
  { name: 'launchd LowPriorityIO', re: new RegExp(String.raw`<key>LowPriorityIO</key>${GAP}<true\s*/>`, 'g') },
  { name: 'launchd LowPriorityIO', re: /"LowPriorityIO"/g, rustOnly: true },
  { name: 'paddock-service ProcessType::Background', re: /\bProcessType::Background\b/g, rustOnly: true },
  { name: 'systemd IOSchedulingClass=idle', re: /(?:(?<!\w)|(?<=\\[nt]))IOSchedulingClass\s*=\s*(?:idle|3)\b/g },
];
const SERVICE_TEXT = {
  'launchd ProcessType Background': 'launchd throttles the whole service like QoS BACKGROUND',
  'launchd ProcessType Adaptive without XPC': 'with no XPC request to raise it, an Adaptive service stays throttled like Background',
  'launchd LowPriorityIO': 'launchd throttles all of the service\'s disk I/O',
  'paddock-service ProcessType::Background': 'renders launchd ProcessType Background (and LowPriorityIO, idle I/O with low_priority)',
  'systemd IOSchedulingClass=idle': 'the service reads and writes the disk only when nothing else does',
};

/** The starvable service settings in a plist, a systemd unit or a Rust renderer: [{ line, name, waived }]. */
export function backgroundServiceSettings(text, file) {
  const code = withoutComments(text, file);
  const raw = text.split('\n');
  const xpc = /<key>MachServices<\/key>/.test(code);
  const out = [];
  const seen = new Set();
  for (const s of SERVICE) {
    if (s.rustOnly && !file.endsWith('.rs')) continue;
    if (s.unlessXpc && xpc) continue;
    for (const m of code.matchAll(s.re)) {
      const line = code.slice(0, m.index).split('\n').length;
      if (seen.has(`${line}:${s.name}`)) continue;
      seen.add(`${line}:${s.name}`);
      const here = `${raw[line - 1] ?? ''}\n${raw[line - 2] ?? ''}`;
      out.push({ line, name: s.name, waived: WAIVER.test(here) ? 'reason' : BARE_WAIVER.test(here) ? 'no-reason' : false });
    }
  }
  return out.sort((a, b) => a.line - b.line);
}

const SERVICE_FILE = /\.(?:plist|service|socket|timer|conf)$/;
const SERVICE_HINT = /ProcessType|LowPriorityIO|IOSchedulingClass/;

export default {
  id: 'C54',
  title: 'Work a person waits for never runs at an OS starvable background level',
  rule: 52,
  run(ctx) {
    const out = [];
    const fix =
      'Lower work a caller may wait for gently (rule 52): `QOS_CLASS_UTILITY` on macOS, nice 10 on Linux, `THREAD_PRIORITY_BELOW_NORMAL` on Windows; work nobody waits on carries `// conformance: background-qos — <why nobody waits on it>`.';
    for (const f of ctx.files()) {
      const rust = f.endsWith('.rs');
      if (!rust && !SERVICE_FILE.test(f)) continue;
      const text = ctx.read(f);
      if (!text) continue;
      const uses = [];
      if (rust && [...Object.keys(STARVABLE), 'taskpolicy'].some((k) => text.includes(k))) uses.push(...backgroundQos(text));
      if (SERVICE_HINT.test(text)) uses.push(...backgroundServiceSettings(text, f));
      for (const u of uses) {
        if (u.waived === 'reason') continue;
        if (u.waived === 'no-reason') out.push(error('C54', 52, f, u.line, 'the background-qos waiver gives no reason', 'Write `// conformance: background-qos — <why nobody waits on it>`.'));
        else out.push(error('C54', 52, f, u.line, `${u.name}: ${STARVABLE[u.name] ?? SERVICE_TEXT[u.name] ?? TASKPOLICY_TEXT}; a caller waiting for this work waits as long as the machine is busy`, fix));
      }
    }
    return out;
  },
};
