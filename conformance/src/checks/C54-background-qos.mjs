// C54: work a person waits for never runs at an OS's starvable background level (rule 52). At those levels a thread
// gets the CPU or the disk only after all other work, for as long as there is any, so a caller that waits for it (a report
// on its analysis pool, an import on a lock the indexer holds) waits as long as the machine is busy: `debrief report`
// timed out on the shared CI Mac (Debrief ADR 0052; family ADR 0020's classes, amended 2026-10-30). Lowered work uses the
// gentle level: `QOS_CLASS_UTILITY` on macOS, nice 10 on Linux, `THREAD_PRIORITY_BELOW_NORMAL` on Windows.
//
// Flagged, outside comments:
// - in Rust code, the identifiers of `STARVABLE` and the raw numbers of the Windows levels in `creation_flags(..)`,
//   `PROCESS_CREATION_FLAGS(..)` and `THREAD_PRIORITY(..)`;
// - commands in string literals, launchd `ProgramArguments` and systemd `Exec*=` lines: `taskpolicy -b` or
//   `-c background`, `ionice -c3` (or `idle`) and `chrt --idle`/`-i`, by the program's basename;
// - service settings in plists, systemd units and the Rust strings that render them (not in tests: `#[cfg(test)]`
//   modules and files under `tests/` are skipped): launchd `ProcessType` `Background`, `Adaptive` in a plist without XPC
//   `MachServices`, `LowPriorityIO` or `LowPriorityBackgroundIO` `true`, systemd `IOSchedulingClass=idle` and
//   `CPUSchedulingPolicy=idle`, and `paddock_service::ProcessType::Background`.
//
// Work nobody waits on carries `conformance: background-qos — <why nobody waits on it>` in a comment (`//`, `<!-- -->` or
// a `#` line) on the same line, or on a comment-only line right above. A waiver in a string is no waiver; a waiver that
// waives nothing is a warning.
//
// Every scan is one pass or a bounded look-ahead: no regular expression here can backtrack beyond a fixed width.
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

const OTHER = {
  'taskpolicy -b': 'macOS taskpolicy -b (the process in background: efficiency cores only, throttled I/O and GPU)',
  'taskpolicy -c background': 'macOS taskpolicy -c background (QoS BACKGROUND for the process)',
  'ionice -c3': 'Linux ionice idle class (disk access only when nothing else uses it)',
  'chrt --idle': 'Linux chrt SCHED_IDLE (runs only when nothing else does)',
  'launchd ProcessType Background': 'launchd throttles the whole service like QoS BACKGROUND',
  'launchd ProcessType Adaptive without XPC': 'with no XPC request to raise it, an Adaptive service stays throttled like Background',
  'launchd LowPriorityIO': "launchd throttles all of the service's disk I/O",
  'launchd LowPriorityBackgroundIO': "launchd throttles the service's disk I/O while it is in the background",
  'systemd IOSchedulingClass=idle': 'the service reads and writes the disk only when nothing else does',
  'systemd CPUSchedulingPolicy=idle': 'the service runs only when nothing else does (SCHED_IDLE)',
  'paddock-service ProcessType::Background': 'renders launchd ProcessType Background (and LowPriorityIO, idle I/O with low_priority)',
};

/**
 * Raw numbers of starvable Windows levels, by the call they are passed to. (0x10000 is not flagged as a creation flag:
 * there it is `INHERIT_PARENT_AFFINITY`.)
 */
const RAW_NUMBERS = {
  creation_flags: { 64: 'IDLE_PRIORITY_CLASS (0x40)', 1048576: 'PROCESS_MODE_BACKGROUND_BEGIN (0x100000)' },
  PROCESS_CREATION_FLAGS: { 64: 'IDLE_PRIORITY_CLASS (0x40)', 1048576: 'PROCESS_MODE_BACKGROUND_BEGIN (0x100000)' },
  THREAD_PRIORITY: { 65536: 'THREAD_MODE_BACKGROUND_BEGIN (0x10000)', '-15': 'THREAD_PRIORITY_IDLE (-15)' },
};

/** What a finding's name means. */
export function describe(name) {
  return STARVABLE[name] ?? OTHER[name] ?? STARVABLE[name.split(' ')[0]] ?? name;
}

// ---- reading a file into code, strings and comments, line by line -----------------------------------------------------

/**
 * A Rust source as { code: line[] (strings and comments blanked), comments: line[] (comment text only),
 * literals: [{ line, text }] (string literal contents in order, escapes as written) }, all 0-based lines. One pass;
 * knows nested block comments, raw and byte strings, char literals and lifetimes.
 */
export function lexRust(text) {
  const n = text.split('\n').length;
  const code = Array.from({ length: n }, () => '');
  const comments = Array.from({ length: n }, () => '');
  const literals = [];
  let line = 0;
  let i = 0;
  /** Adds `ch` to `into` (the comment) and a blank to the code, or ends the line. */
  const commentChar = (ch) => {
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
        s += ch;
        if (ch === '\n') line++;
        else code[line] += ' ';
      }
      i += step;
    }
    literals.push({ line: start, text: s });
    code[line] += close;
    i += close.length;
  };
  while (i < text.length) {
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
      i++;
    }
  }
  return { code, comments, literals };
}

/** A plist as { code, comments } by line: `<!-- -->` is the comment. */
function lexPlist(text) {
  const code = [];
  const comments = [];
  let inComment = false;
  for (const raw of text.split('\n')) {
    let c = '';
    let k = '';
    for (let i = 0; i < raw.length; ) {
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
function lexUnit(text) {
  const code = [];
  const comments = [];
  for (const raw of text.split('\n')) {
    const comment = /^\s*[#;]/.test(raw);
    code.push(comment ? '' : raw);
    comments.push(comment ? raw : '');
  }
  return { code, comments, literals: [] };
}

// ---- tests are not where a service is configured ----------------------------------------------------------------------

const TEST_FILE = /(?:^|\/)tests\/|(?:^|\/)(?:tests|[\w-]+_tests?)\.rs$/;

/** 0-based lines inside `#[cfg(test)] mod name { .. }`. */
function cfgTestLines(code) {
  const joined = code.join('\n');
  const out = new Set();
  let from = 0;
  for (;;) {
    const at = joined.indexOf('#[cfg(test)]', from);
    if (at < 0) break;
    from = at + 12;
    const m = /^\s*(?:#\[[^\]\n]{0,80}\]\s*)*(?:pub(?:\([^)\n]{0,20}\))?\s+)?mod\s+\w+\s*\{/.exec(joined.slice(from, from + 200));
    if (!m) continue;
    let i = from + m[0].length;
    let depth = 1;
    while (i < joined.length && depth > 0) {
      if (joined[i] === '{') depth++;
      else if (joined[i] === '}') depth--;
      i++;
    }
    const first = joined.slice(0, at).split('\n').length - 1;
    const last = joined.slice(0, i).split('\n').length - 1;
    for (let l = first; l <= last; l++) out.add(l);
    from = i;
  }
  return out;
}

// ---- the scans --------------------------------------------------------------------------------------------------------

function scanIdentifiers(code, push) {
  code.forEach((l, i) => {
    for (const m of l.matchAll(/[A-Za-z_][A-Za-z0-9_]*/g)) if (Object.hasOwn(STARVABLE, m[0])) push(i, m[0]);
  });
}

function scanRawNumbers(code, push) {
  code.forEach((l, i) => {
    for (const [call, values] of Object.entries(RAW_NUMBERS)) {
      let from = 0;
      for (;;) {
        const at = l.indexOf(`${call}(`, from);
        if (at < 0) break;
        from = at + call.length + 1;
        if (/\w/.test(l[at - 1] ?? ' ')) continue;
        let depth = 1;
        let j = from;
        while (j < l.length && depth > 0) {
          if (l[j] === '(') depth++;
          else if (l[j] === ')') depth--;
          j++;
        }
        for (const m of l.slice(from, j - 1).matchAll(/(?<![\w.])(-?)(0x[0-9a-fA-F_]+|[0-9][0-9_]*)(?![\w.])/g)) {
          const v = Number(m[2].replace(/_/g, '')) * (m[1] ? -1 : 1);
          const name = values[String(v)];
          if (name) push(i, name);
        }
      }
    }
  });
}

/** Command tokens from string literals (Rust), `<string>` values (plist) or `Exec*=` lines (unit), with their lines. */
function commandTokens(kind, lexed) {
  const tokens = [];
  if (kind === 'rust') {
    for (const lit of lexed.literals) {
      let line = lit.line;
      for (const part of lit.text.split(/(\n)/)) {
        if (part === '\n') line++;
        else for (const t of part.split(/\s+/)) if (t) tokens.push({ line, t });
      }
    }
  } else if (kind === 'plist') {
    lexed.code.forEach((l, i) => {
      for (const m of l.matchAll(/<string>([^<]{0,400})<\/string>/g)) for (const t of m[1].split(/\s+/)) if (t) tokens.push({ line: i, t });
    });
  } else {
    lexed.code.forEach((l, i) => {
      const m = /^\s*Exec\w*\s*=\s*(.*)$/.exec(l);
      if (m) for (const t of m[1].split(/\s+/)) if (t) tokens.push({ line: i, t: t.replace(/^[-@:+!]+/, '') });
    });
  }
  return tokens;
}

const LOOK_AHEAD = 8;
const PROGRAMS = new Set(['taskpolicy', 'ionice', 'chrt']);

function scanCommands(tokens, push) {
  tokens.forEach((tok, i) => {
    const prog = tok.t.split('/').pop();
    if (!PROGRAMS.has(prog)) return;
    // The arguments: the next few tokens, up to the next of these programs and within three lines.
    const next = [];
    for (const x of tokens.slice(i + 1, i + 1 + LOOK_AHEAD)) {
      if (x.line > tok.line + 3 || PROGRAMS.has(x.t.split('/').pop())) break;
      next.push(x.t);
    }
    const pair = (a, b) => next.some((t, k) => t === a && next[k + 1] === b);
    let name = null;
    if (prog === 'taskpolicy') {
      if (next.includes('-b')) name = 'taskpolicy -b';
      else if (pair('-c', 'background') || next.includes('-cbackground')) name = 'taskpolicy -c background';
    } else if (prog === 'ionice') {
      const idle = next.some((t) => /^(?:-c3|-cidle|--class=(?:3|idle))$/.test(t));
      if (idle || pair('-c', '3') || pair('-c', 'idle') || pair('--class', '3') || pair('--class', 'idle')) name = 'ionice -c3';
    } else if (next.includes('--idle') || next.includes('-i')) name = 'chrt --idle';
    if (name) push(tok.line, name);
  });
}

const KEY_NAMES = new Set(['ProcessType', 'LowPriorityIO', 'LowPriorityBackgroundIO', 'MachServices']);

/** Plist text as a stream of tags, each with its line: from a plist's code, or from Rust string literals. */
function plistTags(kind, lexed) {
  const tags = [];
  const fromText = (text, firstLine) => {
    let line = firstLine;
    let last = 0;
    for (const m of text.matchAll(/<(\/?)(plist|key|string|true|false|integer|dict|array|real|date|data)\b[^<>]{0,60}>/g)) {
      for (let k = last; k < m.index; k++) if (text[k] === '\n') line++;
      last = m.index;
      const end = m.index + m[0].length;
      const lt = text.indexOf('<', end);
      const content = text.slice(end, Math.min(lt < 0 ? text.length : lt, end + 200)).trim();
      tags.push({ line, close: m[1] === '/', name: m[2], content });
    }
  };
  if (kind === 'plist') fromText(lexed.code.join('\n'), 0);
  else
    for (const lit of lexed.literals) {
      // A renderer that writes a key with a helper (`key(o, "LowPriorityIO")`) passes the bare name.
      if (KEY_NAMES.has(lit.text)) tags.push({ line: lit.line, close: false, name: 'key', content: lit.text });
      else fromText(lit.text, lit.line);
    }
  return tags;
}

/** Each key with the value tag right after it; `Adaptive` counts only in a plist (from `<plist`) without `MachServices`. */
function scanPlist(tags, push) {
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
      if (key.content === 'ProcessType' && t.name === 'string') {
        if (t.content === 'Background') push(key.line, 'launchd ProcessType Background');
        else if (t.content === 'Adaptive') block.adaptive.push(key.line);
      } else if ((key.content === 'LowPriorityIO' || key.content === 'LowPriorityBackgroundIO') && t.name === 'true') {
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

function scanUnit(lines, push) {
  for (const { line, text } of lines) {
    const m = /^(?:\s|\\t){0,8}(IOSchedulingClass|CPUSchedulingPolicy)[ \t]{0,8}=[ \t]{0,8}([A-Za-z0-9-]{1,20})/.exec(text);
    if (!m) continue;
    if (m[1] === 'IOSchedulingClass' && (m[2] === 'idle' || m[2] === '3')) push(line, 'systemd IOSchedulingClass=idle');
    if (m[1] === 'CPUSchedulingPolicy' && m[2] === 'idle') push(line, 'systemd CPUSchedulingPolicy=idle');
  }
}

function scanServiceType(code, push) {
  code.forEach((l, i) => {
    if (/\bProcessType\s{0,4}::\s{0,4}Background\b/.test(l)) push(i, 'paddock-service ProcessType::Background');
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
 * 1-based lines of inline waivers that waived nothing as the array's `unusedWaivers`.
 */
export function starvableUses(text, file) {
  const kind = kindOf(file);
  const result = [];
  Object.defineProperty(result, 'unusedWaivers', { value: [], enumerable: false });
  if (!kind) return result;
  const lexed = kind === 'rust' ? lexRust(text) : kind === 'plist' ? lexPlist(text) : lexUnit(text);
  const found = [];
  const seen = new Set();
  const push = (line, name) => {
    if (seen.has(`${line}:${name}`)) return;
    seen.add(`${line}:${name}`);
    found.push({ line, name });
  };
  const skip = TEST_FILE.test(file) ? null : kind === 'rust' ? cfgTestLines(lexed.code) : new Set();
  const service = (line, name) => {
    if (skip && !skip.has(line)) push(line, name);
  };
  if (kind === 'rust') {
    scanIdentifiers(lexed.code, push);
    scanRawNumbers(lexed.code, push);
    scanServiceType(lexed.code, service);
  }
  scanCommands(commandTokens(kind, lexed), push);
  if (kind !== 'unit') scanPlist(plistTags(kind, lexed), service);
  scanUnit(unitLines(kind, lexed), service);

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
    if (BARE_WAIVER.test(c) && !used.has(l)) result.unusedWaivers.push(l + 1);
  });
  return result;
}

const HINT = new RegExp(
  [
    ...Object.keys(STARVABLE),
    'taskpolicy',
    'ionice',
    'chrt',
    'ProcessType',
    'LowPriority',
    'SchedulingClass',
    'SchedulingPolicy',
    'creation_flags',
    'THREAD_PRIORITY',
    'PROCESS_CREATION_FLAGS',
    'background-qos',
  ].join('|'),
);

export default {
  id: 'C54',
  title: 'Work a person waits for never runs at an OS starvable background level',
  rule: 52,
  run(ctx) {
    const out = [];
    const fix =
      'Lower work a caller may wait for gently (rule 52): `QOS_CLASS_UTILITY` on macOS, nice 10 on Linux, `THREAD_PRIORITY_BELOW_NORMAL` on Windows; work nobody waits on carries a `conformance: background-qos — <why nobody waits on it>` comment.';
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
