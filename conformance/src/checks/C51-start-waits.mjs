// C51 (report-only, the narrow first step of ADR 0031 section 8: start and spawn waits only, not every `Duration` wait): a start or spawn is judged on run state, not on a fixed wait (ADR 0031). Warns, never fails, about non-test
// Rust that gives a starting process or service a fixed time to become ready:
// - a `Duration` constant, static or struct field whose name says START, SPAWN, LAUNCH or READY and also TIMEOUT, LIMIT, DEADLINE,
//   WAIT, GRACE, BUDGET or WINDOW (`START_LIMIT`, `start_timeout`, `ready_deadline`);
// - inside a `fn` whose name starts with start, spawn, launch or boot: `Deadline::after(`, `recv_timeout(`,
//   `wait_timeout(` / `wait_timeout_while(`, or `Instant::now() +`.
// A site passes with `// cap: <why>`, `// external: <why>`, `// shutdown: <why>`, `// ui: <why>` or `// pace: <why>` (a
// throttle or poll interval that bounds nothing) on the line or the line above, or when the line uses `Patience` (the shared
// start judge, `paddock_proc::StartJudge`). Scope: `crates/`, `apps/*/crates`, `apps/*/app/src-tauri` and `tools/`; `tests/`,
// `benches/`, `examples/`, `fuzz/` and in-file `#[cfg(test)]` modules are skipped. The waiver list may only shrink; the check
// turns to errors when the apps have adopted the judge (ADR 0031 section 8).
import { warn } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';

const WAIVER = /\/\/\s*(?:cap|external|shutdown|ui|pace):\s*\S.{2,}/;
const SCOPE = /^(?:crates\/|tools\/|apps\/[^/]+\/crates\/|apps\/[^/]+\/app\/src-tauri\/)/;
const SKIP = /(?:^|\/)(?:tests|benches|examples|fuzz)\//;
const KIND = '(?:START|SPAWN|LAUNCH|READY)';
const BOUND = '(?:TIMEOUT|LIMIT|DEADLINE|WAIT|GRACE|BUDGET|WINDOW)';
const NAMED = new RegExp(`\\b((?:const|static)\\s+)?([A-Za-z0-9_]*${KIND}[A-Za-z0-9_]*${BOUND}[A-Za-z0-9_]*)\\s*:\\s*(?:Option<\\s*)?(?:std::time::)?Duration\\b`, 'i');
const IN_START_FN = /\b(?:Deadline::after|Instant::now\(\)\s*\+|\.recv_timeout|\.wait_timeout(?:_while)?)\s*\(?/;
const START_FN = /\bfn\s+((?:start|spawn|launch|boot)(?:_\w*)?)\b/;

const lineAt = (text, index) => text.slice(0, index).split('\n').length;

/** `text` with every in-file `#[cfg(test)] mod name { ... }` blanked (newlines kept), up to its closing brace, so code after it still counts. */
function withoutTests(text) {
  const masked = maskRust(text);
  let out = text;
  for (const m of masked.matchAll(/#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*mod\s+\w+\s*\{/g)) {
    let depth = 0;
    let end = masked.length;
    for (let i = m.index + m[0].length - 1; i < masked.length; i++) {
      if (masked[i] === '{') depth++;
      else if (masked[i] === '}' && --depth === 0) {
        end = i + 1;
        break;
      }
    }
    out = out.slice(0, m.index) + out.slice(m.index, end).replace(/[^\n]/g, ' ') + out.slice(end);
  }
  return out;
}

/** The fixed start waits in one Rust source: [{ line, what }] for the sites that carry no waiver. */
export function startWaits(source) {
  const text = withoutTests(source);
  const masked = maskRust(text);
  const lines = text.split('\n');
  const waived = (line) => {
    const near = `${lines[line - 1] ?? ''}\n${lines[line - 2] ?? ''}`;
    return WAIVER.test(near) || /\bPatience\b/.test(lines[line - 1] ?? '');
  };
  const out = [];
  masked.split('\n').forEach((l, i) => {
    const m = NAMED.exec(l);
    if (m && !waived(i + 1)) out.push({ line: i + 1, what: `\`${m[2]}\` is a fixed time for a start` });
  });
  // Calls inside start-like functions: the function runs from its `fn` line to the next `fn` at the same or lower indent.
  const ml = masked.split('\n');
  let current = null;
  ml.forEach((l, i) => {
    const f = START_FN.exec(l);
    if (f) current = { name: f[1], indent: l.search(/\S/) };
    else if (current && /\bfn\s+\w+/.test(l) && l.search(/\S/) <= current.indent) current = null;
    if (current && !f && IN_START_FN.test(l) && !waived(i + 1)) out.push({ line: i + 1, what: `a fixed wait inside \`${current.name}\`` });
  });
  return out.sort((a, b) => a.line - b.line);
}

export default {
  id: 'C51',
  scope: 'both',
  mode: 'report-only',
  title: 'Starts are judged on run state, not on a fixed wait (report-only)',
  rule: 40,
  run(ctx) {
    const out = [];
    for (const f of ctx.files()) {
      if (!f.endsWith('.rs') || !SCOPE.test(f) || SKIP.test(f)) continue;
      const text = ctx.read(f);
      if (!text) continue;
      for (const s of startWaits(text)) {
        out.push(warn('C51', 40, f, s.line, `${s.what}: a working start must not fail only because time passed (ADR 0031, ER-19)`, 'Judge it with `paddock_proc::StartJudge` / `StartWatch`, or mark a real cap with `// cap: <why>` (also `external`, `shutdown`, `ui`, `pace`).'));
      }
    }
    return out;
  },
};
