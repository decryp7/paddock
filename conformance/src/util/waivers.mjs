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
  return { waivers: waivers.filter((w) => commitWaiverFixed(w, problems)), problems };
}

/**
 * A C23 waiver for a commit is `path = "commit <sha>"`: exactly 10 hex digits (what the conformance run prints) or the full 40,
 * in any case; the full sha is cut to 10 so the conformance run and `commit-lint` match the same way. Globs and other forms
 * waive nothing (a problem is reported). Other C23 waivers (doc block paths) keep their patterns. True to keep the waiver.
 */
function commitWaiverFixed(w, problems) {
  if (w.check !== 'C23' || !/^commit\b/i.test(w.path ?? '')) return true;
  const m = /^commit ([0-9a-f]{10})(?:[0-9a-f]{30})?$/.exec(w.path.toLowerCase());
  if (!m) {
    problems.push(`line ${w.line}: a commit waiver needs path = "commit <10 or 40 hex digits>" (no pattern), not "${w.path}"; it waives nothing`);
    return false;
  }
  w.path = `commit ${m[1]}`;
  return true;
}

/** Checks whose waivers must name one exact path (no `*`): a glob would cover findings nobody has read. */
const EXACT_PATH_CHECKS = new Set(['C99']);

/**
 * Splits findings into kept and waived, and adds findings for bad waivers: a waiver without a reason or a valid date is
 * W002, one past its date is W001 (and waives nothing). With `checksRun`, a live waiver for one of those checks that
 * waived no finding is a W003 warning (the finding was fixed: delete the waiver). `skipUnused(waiver)` drops the W003 for a waiver the run could not have used (a scoped run). A finding with `unwaivable: true` (a check
 * that could not run, "check broken" or "not measured") is never waived.
 */
export function applyWaivers({ findings, waivers, problems = [], now = new Date(), file = 'paddock.waivers.toml', pathMatches, checksRun = null, skipUnused = () => false }) {
  const today = now.toISOString().slice(0, 10);
  const extra = problems.map((p) => ({ check: 'W002', severity: 'error', rule: 5, file, line: 1, message: `waiver file: ${p}`, fix: 'Fix the line.' }));
  const live = [];
  for (const w of waivers) {
    const where = { file, line: w.line, rule: 5 };
    if (!w.check || !w.path || !w.reason?.trim() || !/^\d{4}-\d{2}-\d{2}$/.test(w.expires ?? '')) {
      extra.push({ check: 'W002', severity: 'error', ...where, message: 'a waiver needs check, path, reason and expires (YYYY-MM-DD)', fix: 'Fill in all four keys, or delete the waiver.' });
    } else if (EXACT_PATH_CHECKS.has(w.check) && w.path.includes('*')) {
      extra.push({ check: 'W004', severity: 'error', ...where, message: `waiver for ${w.check} ${w.path} uses a glob; a ${w.check} waiver names one exact path`, fix: 'Name the exact path of each finding (for C99: Cargo.lock#<key> as the finding prints it).' });
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
  const used = new Set();
  for (const f of findings) {
    const w = f.unwaivable ? undefined : live.find((x) => x.check === f.check && pathMatches(x.path, f.file ?? ''));
    if (w) {
      used.add(w);
      waived.push({ ...f, waiver: { reason: w.reason, expires: w.expires } });
    } else kept.push(f);
  }
  if (checksRun) {
    for (const w of live) {
      if (used.has(w) || !checksRun.includes(w.check) || skipUnused(w)) continue;
      extra.push({
        check: 'W003', severity: 'warn', file, line: w.line, rule: 5,
        message: `waiver for ${w.check} ${w.path} waives nothing`,
        fix: 'The finding is gone: delete the waiver.',
      });
    }
  }
  return { findings: [...kept, ...extra], waived };
}
