/**
 * The "no UI timeout" check (rule 12, ADR 0008 §4, plan S9, narrowed by plan §12): the UI never gives up on running
 * work, so UI code has no timer that abandons or fails an awaited operation: no `Promise.race` against a timer, no
 * `AbortSignal.timeout`, no call of a `...timeout...` helper (Pitcrew's `callTimeout`, rxjs `timeout`), and no
 * `setTimeout`/`setInterval` whose callback rejects, aborts, cancels or fails something. Plain timers are fine: a
 * debounce, a retry, a tick, a toast that hides itself. Pure, so the e2e runs it over every source file of the shell
 * and its reference apps in Node; comments and strings are ignored.
 */
export function uiTimeoutProblems(path: string, source: string): string[] {
  const code = stripCommentsAndStrings(source);
  const problems: string[] = [];
  const at = (index: number) => `${path}:${code.slice(0, index).split('\n').length}`;
  for (const m of code.matchAll(/\b([A-Za-z_$][\w$]*)\s*(?:<[^>()]*>)?\s*\(/g)) {
    const name = m[1] as string;
    const index = m.index ?? 0;
    if (/\bfunction\s*\*?\s*$/.test(code.slice(Math.max(0, index - 16), index))) continue;
    if (name === 'setTimeout' || name === 'setInterval') {
      const args = callArguments(code, index + m[0].length - 1);
      if (GIVES_UP.test(args)) problems.push(`${at(index)}: a ${name} that gives up on work (the UI never times out work; show "Still working")`);
    } else if (name === 'clearTimeout' || name === 'clearInterval') {
      continue;
    } else if (/timeout/i.test(name)) {
      problems.push(`${at(index)}: ${name}(...) times out work (show "Still working" instead)`);
    }
  }
  for (const m of code.matchAll(/\bPromise\s*\.\s*race\s*\(/g)) {
    const index = m.index ?? 0;
    if (TIMER.test(callArguments(code, index + m[0].length - 1))) problems.push(`${at(index)}: Promise.race against a timer (it abandons work)`);
  }
  return problems;
}

/** What a timer's callback does when it gives up on work: rejects, aborts, cancels, fails or throws. */
const GIVES_UP = /\b(?:reject|abort|cancel|fail|giveUp|throw)\w*\b/i;

/** A timer in a race: a timeout, a delay, a sleep, a deadline, or `setTimeout` itself. */
const TIMER = /\b(?:setTimeout|\w*(?:timeout|timer|delay|sleep|deadline)\w*)\b/i;

/** The text between the parenthesis at `open` and its partner (to the end when unbalanced). */
export function callArguments(code: string, open: number): string {
  let depth = 0;
  for (let i = open; i < code.length; i++) {
    const c = code[i];
    if (c === '(') depth++;
    else if (c === ')' && --depth === 0) return code.slice(open + 1, i);
  }
  return code.slice(open + 1);
}

/** `source` with comments and string contents blanked (line breaks kept, so line numbers stay right). */
export function stripCommentsAndStrings(source: string): string {
  let out = '';
  let i = 0;
  const blank = (s: string) => s.replace(/[^\n]/g, ' ');
  while (i < source.length) {
    const c = source[i] as string;
    const next = source[i + 1];
    if (c === '/' && next === '/') {
      const end = source.indexOf('\n', i);
      const stop = end === -1 ? source.length : end;
      out += blank(source.slice(i, stop));
      i = stop;
    } else if (c === '/' && next === '*') {
      const end = source.indexOf('*/', i + 2);
      const stop = end === -1 ? source.length : end + 2;
      out += blank(source.slice(i, stop));
      i = stop;
    } else if (c === '"' || c === "'" || c === '`') {
      let j = i + 1;
      while (j < source.length && source[j] !== c) j += source[j] === '\\' ? 2 : 1;
      out += c + blank(source.slice(i + 1, j)) + (j < source.length ? c : '');
      i = j + 1;
    } else {
      out += c;
      i++;
    }
  }
  return out;
}
