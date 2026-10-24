// paddock.waivers.toml: `[[waiver]]` tables with string values (check, path, reason, expires = YYYY-MM-DD). This is the
// whole TOML subset the file uses; anything else in it is reported rather than guessed at.

/** Parses the waiver file. Returns { waivers, problems } where problems are strings naming the line. */
export function parseWaivers(text) {
  const waivers = [];
  const problems = [];
  let cur = null;
  text.split('\n').forEach((raw, i) => {
    const line = raw.replace(/\s+#.*$/, '').trim();
    if (!line || line.startsWith('#')) return;
    if (line === '[[waiver]]') {
      cur = { line: i + 1 };
      waivers.push(cur);
      return;
    }
    const m = /^(\w+)\s*=\s*"((?:[^"\\]|\\.)*)"$/.exec(line);
    if (!m || !cur) {
      problems.push(`line ${i + 1}: cannot read "${raw.trim()}" (expected [[waiver]] and key = "value")`);
      return;
    }
    cur[m[1]] = m[2].replace(/\\(["\\])/g, '$1');
  });
  return { waivers, problems };
}

/**
 * Splits findings into kept and waived, and adds findings for bad waivers: a waiver without a reason or a valid date is
 * W002, one past its date is W001 (and waives nothing).
 */
export function applyWaivers({ findings, waivers, problems = [], now = new Date(), file = 'paddock.waivers.toml', pathMatches }) {
  const today = now.toISOString().slice(0, 10);
  const extra = problems.map((p) => ({ check: 'W002', severity: 'error', rule: 5, file, line: 1, message: `waiver file: ${p}`, fix: 'Fix the line.' }));
  const live = [];
  for (const w of waivers) {
    const where = { file, line: w.line, rule: 5 };
    if (!w.check || !w.path || !w.reason?.trim() || !/^\d{4}-\d{2}-\d{2}$/.test(w.expires ?? '')) {
      extra.push({ check: 'W002', severity: 'error', ...where, message: 'a waiver needs check, path, reason and expires (YYYY-MM-DD)', fix: 'Fill in all four keys, or delete the waiver.' });
    } else if (w.expires < today) {
      extra.push({
        check: 'W001', severity: 'error', ...where,
        message: `waiver for ${w.check} ${w.path} expired on ${w.expires}`,
        fix: 'Fix the finding, or renew the waiver with a new date and a reason.',
      });
    } else live.push(w);
  }
  const kept = [];
  const waived = [];
  for (const f of findings) {
    const w = live.find((x) => x.check === f.check && pathMatches(x.path, f.file ?? ''));
    if (w) waived.push({ ...f, waiver: { reason: w.reason, expires: w.expires } });
    else kept.push(f);
  }
  return { findings: [...kept, ...extra], waived };
}
