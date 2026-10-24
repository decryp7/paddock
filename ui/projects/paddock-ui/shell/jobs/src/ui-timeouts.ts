/**
 * The "no UI timeout" check (rule 12, ADR 0008 §4, plan S9): the UI never gives up on a running job, so UI code has no
 * delayed `setTimeout`, no `AbortSignal.timeout`, no `Promise.race` and no call of a `...timeout...` helper (Pitcrew's
 * `callTimeout`, rxjs `timeout`). A zero-delay `setTimeout` (a macrotask) and `clearTimeout` are fine. Pure, so the e2e
 * runs it over every source file of the shell and its reference apps in Node; comments and strings are ignored.
 */
export function uiTimeoutProblems(path: string, source: string): string[] {
  const code = stripCommentsAndStrings(source);
  const problems: string[] = [];
  const at = (index: number) => `${path}:${code.slice(0, index).split('\n').length}`;
  for (const m of code.matchAll(/\b([A-Za-z_$][\w$]*)\s*(?:<[^>()]*>)?\s*\(/g)) {
    const name = m[1] as string;
    const index = m.index ?? 0;
    if (/\bfunction\s*\*?\s*$/.test(code.slice(Math.max(0, index - 16), index))) continue;
    if (name === 'setTimeout') {
      if (!/^setTimeout\s*\([^;]*?,\s*0\s*\)/.test(code.slice(index))) problems.push(`${at(index)}: a delayed setTimeout (the UI never times out work)`);
    } else if (name === 'clearTimeout') {
      continue;
    } else if (/timeout/i.test(name)) {
      problems.push(`${at(index)}: ${name}(...) times out work (show "Still working" instead)`);
    }
  }
  for (const m of code.matchAll(/\bPromise\s*\.\s*race\s*\(/g)) problems.push(`${at(m.index ?? 0)}: Promise.race (a race against a timer abandons work)`);
  return problems;
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
