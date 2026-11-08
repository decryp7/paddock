// C54: work a person waits for never runs at an OS's starvable background level (rule 52). At those levels a thread
// gets the CPU or the disk only after all other work, for as long as there is any, so a caller that waits for it (a report
// on its analysis pool, an import on a lock the indexer holds) waits as long as the machine is busy: `debrief report`
// timed out on the shared CI Mac (Debrief ADR 0052; family ADR 0020's classes, amended 2026-10-30). Lowered work uses the
// gentle level: `QOS_CLASS_UTILITY` on macOS, nice 10 on Linux, `THREAD_PRIORITY_BELOW_NORMAL` on Windows.
//
// Flagged, outside comments and tests (`#[cfg(test)]` modules, files under `tests/`):
// - in Rust code, the identifiers of `STARVABLE`, the thread-priority crate's idle levels, and the raw numbers of the
//   starvable levels (in `creation_flags(..)`, `PROCESS_CREATION_FLAGS(..)`, `THREAD_PRIORITY(..)` and `ioprio_set(..)`,
//   directly or through a `const`, and `3 << 13`), except in a match arm that only reads a level (`X =>`);
// - commands, by the program's basename, with flags from their own argument list: `taskpolicy -b`, `-c background`,
//   `-c maintenance`, `-d throttle`; `ionice -c3` (or `idle`); `chrt --idle`/`-i`; in Rust strings, launchd
//   `ProgramArguments` and systemd `Exec*=` lines;
// - service settings in plists, systemd units and the Rust that renders them: launchd `ProcessType` `Background`,
//   `Adaptive` in a plist without XPC `MachServices`, `LowPriorityIO` or `LowPriorityBackgroundIO` `true` (paired with
//   its own key), systemd `IOSchedulingClass=idle` and `CPUSchedulingPolicy=idle`, and
//   `paddock_service::ProcessType::Background`.
//
// Work nobody waits on carries `conformance: background-qos — <why nobody waits on it>` in a comment (`//`, `<!-- -->` or
// a `#` line) on the same line, or on a comment-only line right above. A waiver in a string is no waiver; a waiver that
// waives nothing is a warning.
//
// Cost: linear in the file's size. Each character is lexed once; tags, tokens and lines are each visited a bounded number
// of times (look-ahead is capped); every regular expression has bounded repetition or runs on one line. `stats.steps`
// counts that work so a test can check it grows linearly.
import { error, warn } from '../finding.mjs';

const WAIVER = /conformance:\s*background-qos\s*[—–-]+\s*\S.{2,}/;
const BARE_WAIVER = /conformance:\s*background-qos/;

/** The starvable levels, by identifier, with the OS and what the level does. */
export const STARVABLE = {
  QOS_CLASS_BACKGROUND: 'macOS QoS BACKGROUND (efficiency cores only, after all other work, throttled I/O)',
  PRIO_DARWIN_BG: 'macOS process background (taskpolicy -b: efficiency cores only, throttled I/O and GPU)',
  IOPOL_THROTTLE: 'macOS throttled I/O policy (disk access after all other I/O)',
  THREAD_MODE_BACKGROUND_BEGIN: 'Windows thread background mode (lowest CPU, I/O and memory priority)',
  PROCESS_MODE_BACKGROUND_BEGIN: 'Windows process background mode (lowest CPU, I/O and memory priority)',
  IDLE_PRIORITY_CLASS: 'Windows idle priority class (runs only when nothing else does)',
  THREAD_PRIORITY_IDLE: 'Windows idle thread priority (runs only when nothing else does)',
  ProcessPowerThrottling: 'Windows EcoQoS for a process (efficiency cores, lowest clock)',
  ThreadPowerThrottling: 'Windows EcoQoS for a thread (efficiency cores, lowest clock)',
  PROCESS_POWER_THROTTLING_EXECUTION_SPEED: 'Windows EcoQoS (efficiency cores, lowest clock)',
  THREAD_POWER_THROTTLING_EXECUTION_SPEED: 'Windows EcoQoS (efficiency cores, lowest clock)',
  SCHED_IDLE: 'Linux SCHED_IDLE (runs only when nothing else does)',
  IOPRIO_CLASS_IDLE: 'Linux idle I/O class (disk access only when nothing else uses it)',
};

/**
 * Paths to a starvable level in the thread-priority crate. (`ThreadPriority::Min` is not one: it is the lowest normal level,
 * about `THREAD_PRIORITY_LOWEST` on Windows and nice 19 on Linux, which is slowed, not starved.)
 */
const PATHS = [
  { re: /\bNormalThreadSchedulePolicy\s{0,4}::\s{0,4}Idle\b/g, name: 'thread-priority NormalThreadSchedulePolicy::Idle' },
  { re: /\bWinAPIThreadPriority\s{0,4}::\s{0,4}Idle\b/g, name: 'thread-priority WinAPIThreadPriority::Idle' },
  { re: /\bWinAPIThreadPriority\s{0,4}::\s{0,4}BackgroundModeBegin\b/g, name: 'thread-priority WinAPIThreadPriority::BackgroundModeBegin' },
];

const OTHER = {
  'taskpolicy -b': 'macOS taskpolicy -b (the process in background: efficiency cores only, throttled I/O and GPU)',
  'taskpolicy -c background': 'macOS taskpolicy -c background (QoS BACKGROUND for the process)',
  'taskpolicy -c maintenance': 'macOS taskpolicy -c maintenance (QoS MAINTENANCE: at the background level)',
  'taskpolicy -d throttle': 'macOS taskpolicy -d throttle (disk I/O after all other I/O)',
  'ionice -c3': 'Linux ionice idle class (disk access only when nothing else uses it)',
  'chrt --idle': 'Linux chrt SCHED_IDLE (runs only when nothing else does)',
  'thread-priority NormalThreadSchedulePolicy::Idle': 'Linux SCHED_IDLE through the thread-priority crate',
  'thread-priority WinAPIThreadPriority::Idle': 'Windows idle thread priority through the thread-priority crate',
  'thread-priority WinAPIThreadPriority::BackgroundModeBegin': 'Windows thread background mode through the thread-priority crate',
  'launchd ProcessType Background': 'launchd throttles the whole service like QoS BACKGROUND',
  'launchd ProcessType Adaptive without XPC': 'with no XPC request to raise it, an Adaptive service stays throttled like Background',
  'launchd LowPriorityIO': "launchd throttles all of the service's disk I/O",
  'launchd LowPriorityBackgroundIO': "launchd throttles the service's disk I/O while it is in the background",
  'systemd IOSchedulingClass=idle': 'the service reads and writes the disk only when nothing else does',
  'systemd CPUSchedulingPolicy=idle': 'the service runs only when nothing else does (SCHED_IDLE)',
  'paddock-service ProcessType::Background': 'renders launchd ProcessType Background (and LowPriorityIO, idle I/O with low_priority)',
};

const FLAG_NAMES = { 64: 'IDLE_PRIORITY_CLASS (0x40)', 1048576: 'PROCESS_MODE_BACKGROUND_BEGIN (0x100000)' };
/**
 * Raw numbers of starvable levels, by the call they are passed to: a value's name, or none. (0x10000 is not flagged as a
 * creation flag: there it is `INHERIT_PARENT_AFFINITY`.)
 */
const RAW_NUMBERS = {
  creation_flags: (v) => FLAG_NAMES[v],
  PROCESS_CREATION_FLAGS: (v) => FLAG_NAMES[v],
  THREAD_PRIORITY: (v) => (v === 65536 ? 'THREAD_MODE_BACKGROUND_BEGIN (0x10000)' : v === -15 ? 'THREAD_PRIORITY_IDLE (-15)' : undefined),
  ioprio_set: (v) => (v >= 0 && v >> 13 === 3 ? 'IOPRIO_CLASS_IDLE (3 << 13)' : undefined),
};

/** What a finding's name means. */
export function describe(name) {
  return STARVABLE[name] ?? OTHER[name] ?? STARVABLE[name.split(' ')[0]] ?? name;
}

// ---- reading a file into code, strings and comments, line by line -----------------------------------------------------

/**
 * A Rust source as { code: line[] (strings and comments blanked), comments: line[] (comment text only),
 * literals: [{ line, stmt, text }] (string literal contents in order, escapes as written; `stmt` counts the `;`, `{` and
 * `}` before it) }, all 0-based lines. Knows nested block comments, raw and byte strings, char literals and lifetimes.
 */
export function lexRust(text, stats = { steps: 0 }) {
  const n = text.split('\n').length;
  const code = Array.from({ length: n }, () => '');
  const comments = Array.from({ length: n }, () => '');
  const literals = [];
  let line = 0;
  let stmt = 0;
  let i = 0;
  const commentChar = (ch) => {
    stats.steps++;
    if (ch === '\n') line++;
    else {
      comments[line] += ch;
      code[line] += ' ';
    }
  };
  /** A string literal's body from `i` up to `close`; escapes are skipped only in a normal string. */
  const literal = (close, escapes) => {
    const start = line;
    let s = '';
    while (i < text.length && !text.startsWith(close, i)) {
      const step = escapes && text[i] === '\\' ? 2 : 1;
      for (const ch of text.slice(i, i + step)) {
        stats.steps++;
        s += ch;
        if (ch === '\n') line++;
        else code[line] += ' ';
      }
      i += step;
    }
    literals.push({ line: start, stmt, text: s });
    code[line] += close;
    i += close.length;
  };
  while (i < text.length) {
    stats.steps++;
    const c = text[i];
    const prev = text[i - 1] ?? ' ';
    if (c === '/' && text[i + 1] === '/') {
      while (i < text.length && text[i] !== '\n') commentChar(text[i++]);
    } else if (c === '/' && text[i + 1] === '*') {
      let depth = 0;
      while (i < text.length) {
        if (text.startsWith('/*', i)) {
          depth++;
          commentChar('/');
          commentChar('*');
          i += 2;
        } else if (text.startsWith('*/', i)) {
          depth--;
          commentChar('*');
          commentChar('/');
          i += 2;
          if (depth === 0) break;
        } else commentChar(text[i++]);
      }
    } else if (!/\w/.test(prev) && /^b?r#*"/.test(text.slice(i, i + 8))) {
      const open = /^b?r(#*)"/.exec(text.slice(i, i + 8));
      code[line] += open[0];
      i += open[0].length;
      literal(`"${open[1]}`, false);
    } else if (c === '"' || (c === 'b' && text[i + 1] === '"' && !/\w/.test(prev))) {
      const open = c === 'b' ? 'b"' : '"';
      code[line] += open;
      i += open.length;
      literal('"', true);
    } else if (c === "'") {
      const ch = /^'(?:\\u\{[0-9a-fA-F]+\}|\\.|[^\\'\n])'/.exec(text.slice(i, i + 12));
      const len = ch ? ch[0].length : 1;
      code[line] += ch ? `'${' '.repeat(len - 2)}'` : "'";
      i += len;
    } else {
      if (c === '\n') line++;
      else code[line] += c;
      if (c === ';' || c === '{' || c === '}') stmt++;
      i++;
    }
  }
  return { code, comments, literals };
}

/** A plist as { code, comments } by line: `<!-- -->` is the comment. */
function lexPlist(text, stats) {
  const code = [];
  const comments = [];
  let inComment = false;
  for (const raw of text.split('\n')) {
    let c = '';
    let k = '';
    for (let i = 0; i < raw.length; ) {
      stats.steps++;
      if (!inComment && raw.startsWith('<!--', i)) {
        inComment = true;
        i += 4;
      } else if (inComment && raw.startsWith('-->', i)) {
        inComment = false;
        i += 3;
      } else {
        if (inComment) k += raw[i];
        else c += raw[i];
        i++;
      }
    }
    code.push(c);
    comments.push(k);
  }
  return { code, comments, literals: [] };
}

/** A systemd unit as { code, comments } by line: a line starting with `#` or `;` is a comment (systemd has no trailing ones). */
function lexUnit(text, stats) {
  const code = [];
  const comments = [];
  for (const raw of text.split('\n')) {
    stats.steps++;
    const comment = /^\s*[#;]/.test(raw);
    code.push(comment ? '' : raw);
    comments.push(comment ? raw : '');
  }
  return { code, comments, literals: [] };
}

// ---- tests are not where levels are chosen ----------------------------------------------------------------------------

const TEST_FILE = /(?:^|\/)tests\/|(?:^|\/)(?:tests|[\w-]+_tests?)\.rs$/;

/** 0-based lines inside `#[cfg(test)] mod name { .. }`; each character is looked at once. */
function cfgTestLines(code, stats) {
  const joined = code.join('\n');
  const out = new Set();
  let from = 0;
  let line = 0;
  let counted = 0;
  const lineAt = (offset) => {
    for (; counted < offset; counted++) if (joined[counted] === '\n') line++;
    return line;
  };
  for (;;) {
    const at = joined.indexOf('#[cfg(test)]', from);
    if (at < 0) break;
    from = at + 12;
    const m = /^\s*(?:#\[[^\]\n]{0,80}\]\s*)*(?:pub(?:\([^)\n]{0,20}\))?\s+)?mod\s+\w+\s*\{/.exec(joined.slice(from, from + 200));
    if (!m) continue;
    let i = from + m[0].length;
    let depth = 1;
    while (i < joined.length && depth > 0) {
      stats.steps++;
      if (joined[i] === '{') depth++;
      else if (joined[i] === '}') depth--;
      i++;
    }
    const first = lineAt(at);
    const last = lineAt(i);
    for (let l = first; l <= last; l++) out.add(l);
    from = i;
  }
  return out;
}

// ---- the scans --------------------------------------------------------------------------------------------------------

/** Whether the code after `end` on the line only reads a level in a match arm (`X =>`, `Some(X) =>`). */
const isArm = (l, end) => /^[\s)]{0,8}=>/.test(l.slice(end, end + 12));

function scanIdentifiers(code, stats, push) {
  code.forEach((l, i) => {
    for (const m of l.matchAll(/[A-Za-z_][A-Za-z0-9_]*/g)) {
      stats.steps++;
      if (Object.hasOwn(STARVABLE, m[0]) && !isArm(l, m.index + m[0].length)) push(i, m[0]);
    }
    for (const p of PATHS) for (const m of l.matchAll(p.re)) if (!isArm(l, m.index + m[0].length)) push(i, p.name);
    if (/(?<![\w.])3\s{0,4}<<\s{0,4}(?:13|IOPRIO_CLASS_SHIFT)\b/.test(l)) push(i, 'IOPRIO_CLASS_IDLE (3 << 13)');
  });
}

const NUMBER = /(?<![\w.])(-?)(0x[0-9a-fA-F_]+|[0-9][0-9_]*)(?![\w.])/g;
const parseNumber = (m) => Number(m[2].replace(/_/g, '')) * (m[1] ? -1 : 1);

/** `const NAME: T = expr;` (one line) as NAME -> the numbers in expr. */
function constants(code, stats) {
  const out = new Map();
  for (const l of code) {
    stats.steps++;
    const m = /\b(?:const|static)\s+([A-Z][A-Z0-9_]{0,60})\s*:[^=;]{0,80}=([^;]{1,200});/.exec(l);
    if (m) out.set(m[1], [...m[2].matchAll(NUMBER)].map(parseNumber));
  }
  return out;
}

function scanRawNumbers(code, consts, stats, push) {
  code.forEach((l, i) => {
    for (const [call, name] of Object.entries(RAW_NUMBERS)) {
      let from = 0;
      for (;;) {
        const at = l.indexOf(`${call}(`, from);
        if (at < 0) break;
        let j = at + call.length + 1;
        if (/\w/.test(l[at - 1] ?? ' ')) {
          from = j;
          continue;
        }
        const argsFrom = j;
        let depth = 1;
        for (; j < l.length && depth > 0; j++) {
          stats.steps++;
          if (l[j] === '(') depth++;
          else if (l[j] === ')') depth--;
        }
        const args = l.slice(argsFrom, j - 1);
        for (const m of args.matchAll(NUMBER)) {
          const n = name(parseNumber(m));
          if (n) push(i, n);
        }
        for (const m of args.matchAll(/\b[A-Z][A-Z0-9_]{0,60}\b/g)) {
          for (const v of consts.get(m[0]) ?? []) {
            const n = name(v);
            if (n) push(i, `${n.replace(/\)$/, '')}, through ${m[0]})`);
          }
        }
        from = j; // resume after the closing parenthesis: nothing inside is scanned twice
      }
    }
  });
}

/**
 * Command tokens with the argument list each belongs to: { line, t, list, shell }. In Rust a program named by a literal of
 * its own (`Command::new("taskpolicy")`) takes the literals of the same statement; a program inside a literal with spaces
 * (a shell line) takes the rest of that literal. In a plist the list is the `<array>`; in a unit, the `Exec*=` line.
 */
function commandTokens(kind, lexed, stats) {
  const tokens = [];
  const add = (line, t, list, shell) => {
    stats.steps++;
    tokens.push({ line, t, list, shell });
  };
  if (kind === 'rust') {
    lexed.literals.forEach((lit, k) => {
      const shell = /\s/.test(lit.text.trim());
      let line = lit.line;
      for (const part of lit.text.split(/(\n)/)) {
        if (part === '\n') line++;
        else for (const t of part.split(/\s+/)) if (t) add(line, t, shell ? `lit${k}` : `stmt${lit.stmt}`, shell);
      }
    });
  } else if (kind === 'plist') {
    let array = 0;
    lexed.code.forEach((l, i) => {
      for (const m of l.matchAll(/<(\/?)(array|string)\b[^<>]{0,40}>(?:([^<]{0,400})<\/string>)?/g)) {
        if (m[2] === 'array') array++;
        else if (m[3] !== undefined) {
          const shell = /\s/.test(m[3].trim());
          for (const t of m[3].split(/\s+/)) if (t) add(i, t, shell ? `str${i}:${m.index}` : `array${array}`, shell);
        }
      }
    });
  } else {
    lexed.code.forEach((l, i) => {
      const m = /^\s*Exec\w*\s*=\s*(.*)$/.exec(l);
      if (!m) return;
      const words = m[1].split(/\s+/).filter(Boolean);
      if (!words.length) return;
      // systemd's prefixes belong to the first word only; with `@` the second word is argv[0], not an argument.
      const prefix = /^[-@:+!]+/.exec(words[0])?.[0] ?? '';
      words[0] = words[0].slice(prefix.length);
      if (prefix.includes('@')) words.splice(1, 1);
      for (const t of words) add(i, t, `line${i}`, true);
    });
  }
  return tokens;
}

const LOOK_AHEAD = 8;
const PROGRAMS = new Set(['taskpolicy', 'ionice', 'chrt']);
const SEPARATOR = /^(?:;|&&|\|\||\||&)$/;

function scanCommands(tokens, push) {
  tokens.forEach((tok, i) => {
    const prog = tok.t.split('/').pop();
    if (!PROGRAMS.has(prog)) return;
    // Its own argument list: the next tokens of the same list, up to a shell separator or another of these programs.
    const next = [];
    for (const x of tokens.slice(i + 1, i + 1 + LOOK_AHEAD)) {
      if (x.list !== tok.list || SEPARATOR.test(x.t) || PROGRAMS.has(x.t.split('/').pop())) break;
      next.push(x.t.replace(/[;&|]+$/, ''));
      if (/[;&|]$/.test(x.t)) break;
    }
    const pair = (a, b) => next.some((t, k) => t === a && next[k + 1] === b);
    let name = null;
    if (prog === 'taskpolicy') {
      if (next.includes('-b')) name = 'taskpolicy -b';
      else if (pair('-c', 'background') || next.includes('-cbackground')) name = 'taskpolicy -c background';
      else if (pair('-c', 'maintenance') || next.includes('-cmaintenance')) name = 'taskpolicy -c maintenance';
      else if (pair('-d', 'throttle') || next.includes('-dthrottle')) name = 'taskpolicy -d throttle';
    } else if (prog === 'ionice') {
      const idle = next.some((t) => /^(?:-c3|-cidle|--class=(?:3|idle))$/.test(t));
      if (idle || pair('-c', '3') || pair('-c', 'idle') || pair('--class', '3') || pair('--class', 'idle')) name = 'ionice -c3';
    } else if (next.includes('--idle') || next.includes('-i')) name = 'chrt --idle';
    if (name) push(tok.line, name);
  });
}

const KEY_NAMES = new Set(['ProcessType', 'LowPriorityIO', 'LowPriorityBackgroundIO', 'MachServices']);
const VALUE_WORDS = new Set(['Background', 'Adaptive', 'Standard', 'Interactive']);

/**
 * Plist text as a stream of tags, each with its line: from a plist's code, or from Rust string literals. In Rust a bare
 * literal that looks like a plist key (`key(o, "RunAtLoad")`) is a key, and a bare `ProcessType` value is a string.
 */
function plistTags(kind, lexed, stats) {
  const tags = [];
  const fromText = (text, firstLine) => {
    let line = firstLine;
    let last = 0;
    for (const m of text.matchAll(/<(\/?)(plist|key|string|true|false|integer|dict|array|real|date|data)\b[^<>]{0,60}>/g)) {
      for (let k = last; k < m.index; k++) if (text[k] === '\n') line++;
      last = m.index;
      stats.steps++;
      const end = m.index + m[0].length;
      const lt = text.indexOf('<', end);
      const content = text.slice(end, Math.min(lt < 0 ? text.length : lt, end + 200)).trim();
      tags.push({ line, close: m[1] === '/', name: m[2], content });
    }
  };
  if (kind === 'plist') fromText(lexed.code.join('\n'), 0);
  else
    for (const lit of lexed.literals) {
      stats.steps++;
      if (VALUE_WORDS.has(lit.text)) tags.push({ line: lit.line, close: false, name: 'string', content: lit.text });
      else if (KEY_NAMES.has(lit.text) || /^[A-Z][A-Za-z0-9]{2,40}$/.test(lit.text)) tags.push({ line: lit.line, close: false, name: 'key', content: lit.text });
      else fromText(lit.text, lit.line);
    }
  return tags;
}

/**
 * Each key with the value tag right after it (in Rust, within two lines; a value farther away belongs to code this
 * cannot follow); `Adaptive` counts only in a plist (from `<plist`) without `MachServices`.
 */
function scanPlist(kind, tags, push) {
  let block = { xpc: false, adaptive: [] };
  const flush = () => {
    if (!block.xpc) for (const l of block.adaptive) push(l, 'launchd ProcessType Adaptive without XPC');
  };
  let key = null;
  for (const t of tags) {
    if (t.name === 'plist' && !t.close) {
      flush();
      block = { xpc: false, adaptive: [] };
      key = null;
    } else if (t.name === 'key') {
      if (!t.close) {
        key = t;
        if (t.content === 'MachServices') block.xpc = true;
      }
    } else if (key && !t.close) {
      const near = kind !== 'rust' || t.line - key.line <= 2;
      if (near && key.content === 'ProcessType' && t.name === 'string') {
        if (t.content === 'Background') push(key.line, 'launchd ProcessType Background');
        else if (t.content === 'Adaptive') block.adaptive.push(key.line);
      } else if (near && (key.content === 'LowPriorityIO' || key.content === 'LowPriorityBackgroundIO') && t.name === 'true') {
        push(key.line, `launchd ${key.content}`);
      }
      key = null;
    }
  }
  flush();
}

/** systemd `Key=value` lines: a unit's code lines, or the lines of Rust string literals (split at `\n` escapes too). */
function unitLines(kind, lexed) {
  if (kind === 'unit') return lexed.code.map((text, line) => ({ line, text }));
  const out = [];
  for (const lit of lexed.literals) {
    let line = lit.line;
    for (const part of lit.text.split(/(\n|\\n)/)) {
      if (part === '\n') line++;
      else if (part !== '\\n') out.push({ line, text: part });
    }
  }
  return out;
}

function scanUnit(lines, stats, push) {
  for (const { line, text } of lines) {
    stats.steps++;
    const m = /^(?:\s|\\t){0,8}(IOSchedulingClass|CPUSchedulingPolicy)[ \t]{0,8}=[ \t]{0,8}([A-Za-z0-9-]{1,20})/.exec(text);
    if (!m) continue;
    if (m[1] === 'IOSchedulingClass' && (m[2] === 'idle' || m[2] === '3')) push(line, 'systemd IOSchedulingClass=idle');
    if (m[1] === 'CPUSchedulingPolicy' && m[2] === 'idle') push(line, 'systemd CPUSchedulingPolicy=idle');
  }
}

function scanServiceType(code, push) {
  code.forEach((l, i) => {
    for (const m of l.matchAll(/\bProcessType\s{0,4}::\s{0,4}Background\b/g)) {
      if (!isArm(l, m.index + m[0].length)) push(i, 'paddock-service ProcessType::Background');
    }
  });
}

// ---- putting it together ----------------------------------------------------------------------------------------------

function kindOf(file) {
  if (file.endsWith('.rs')) return 'rust';
  if (file.endsWith('.plist')) return 'plist';
  if (/\.(?:service|socket|timer)$/.test(file) || /(?:^|\/)systemd\/.+\.conf$/.test(file)) return 'unit';
  return null;
}

/**
 * The starvable uses in a file: [{ line (1-based), name, waived: false | 'no-reason' | 'reason' }] by line, with the
 * 1-based lines of inline waivers that waived nothing as the array's (non-enumerable) `unusedWaivers`. `stats.steps`
 * grows with the work done.
 */
export function starvableUses(text, file, stats = { steps: 0 }) {
  const kind = kindOf(file);
  const result = [];
  Object.defineProperty(result, 'unusedWaivers', { value: [], enumerable: false });
  if (!kind || TEST_FILE.test(file)) return result;
  const lexed = kind === 'rust' ? lexRust(text, stats) : kind === 'plist' ? lexPlist(text, stats) : lexUnit(text, stats);
  const tests = kind === 'rust' ? cfgTestLines(lexed.code, stats) : new Set();
  const found = [];
  const seen = new Set();
  const push = (line, name) => {
    if (tests.has(line) || seen.has(`${line}:${name}`)) return;
    seen.add(`${line}:${name}`);
    found.push({ line, name });
  };
  if (kind === 'rust') {
    scanIdentifiers(lexed.code, stats, push);
    scanRawNumbers(lexed.code, constants(lexed.code, stats), stats, push);
    scanServiceType(lexed.code, push);
  }
  scanCommands(commandTokens(kind, lexed, stats), push);
  if (kind !== 'unit') scanPlist(kind, plistTags(kind, lexed, stats), push);
  scanUnit(unitLines(kind, lexed), stats, push);

  const used = new Set();
  const waiverOnlyLine = (l) => l >= 0 && (lexed.code[l] ?? '').trim() === '' && BARE_WAIVER.test(lexed.comments[l] ?? '');
  found.sort((a, b) => a.line - b.line || (a.name < b.name ? -1 : 1));
  for (const f of found) {
    const at = BARE_WAIVER.test(lexed.comments[f.line] ?? '') ? f.line : waiverOnlyLine(f.line - 1) ? f.line - 1 : -1;
    if (at >= 0) used.add(at);
    const waived = at < 0 ? false : WAIVER.test(lexed.comments[at]) ? 'reason' : 'no-reason';
    result.push({ line: f.line + 1, name: f.name, waived });
  }
  lexed.comments.forEach((c, l) => {
    if (BARE_WAIVER.test(c) && !used.has(l) && !tests.has(l)) result.unusedWaivers.push(l + 1);
  });
  return result;
}

const HINT = new RegExp(
  [
    ...Object.keys(STARVABLE),
    'taskpolicy',
    'ionice',
    'chrt',
    'ThreadSchedulePolicy',
    'WinAPIThreadPriority',
    'ProcessType',
    'LowPriority',
    'SchedulingClass',
    'SchedulingPolicy',
    'creation_flags',
    'PROCESS_CREATION_FLAGS',
    'ioprio',
    '<<',
    'background-qos',
  ].join('|'),
);

export default {
  id: 'C54',
  scope: 'both',
  mode: 'gate',
  title: 'Work a person waits for never runs at an OS starvable background level',
  rule: 52,
  run(ctx) {
    const out = [];
    const fix =
      'Give the thread its rule 52 role: bulk work (long, with progress and Cancel) runs at `QOS_CLASS_UTILITY` on macOS, nice 10 on Linux, `THREAD_PRIORITY_BELOW_NORMAL` on Windows; live work (a person waits on it within a deadline or for a screen to fill) may use the first two but stays at the normal level on Windows; work nobody waits on carries a `conformance: background-qos — <why nobody waits on it>` comment.';
    for (const f of ctx.files()) {
      if (!kindOf(f)) continue;
      const text = ctx.read(f);
      if (!text || !HINT.test(text)) continue;
      const uses = starvableUses(text, f);
      for (const u of uses) {
        if (u.waived === 'reason') continue;
        if (u.waived === 'no-reason') out.push(error('C54', 52, f, u.line, 'the background-qos waiver gives no reason', 'Write `conformance: background-qos — <why nobody waits on it>`.'));
        else out.push(error('C54', 52, f, u.line, `${u.name}: ${describe(u.name)}; a caller waiting for this work waits as long as the machine is busy`, fix));
      }
      for (const line of uses.unusedWaivers) {
        out.push(warn('C54', 52, f, line, 'the background-qos waiver waives nothing (no starvable level on its line or the next)', 'Delete the waiver, or move it next to the use it explains.'));
      }
    }
    return out;
  },
};
