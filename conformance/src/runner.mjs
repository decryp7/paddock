import { createContext, loadConfig } from './context.mjs';
import { error, unwaivable } from './finding.mjs';
import { checks } from './checks/index.mjs';
import { inScope } from './util/scope.mjs';
import { pathMatches } from './util/fs.mjs';
import { applyWaivers, parseWaivers } from './util/waivers.mjs';

/**
 * Runs the selected checks over `root`. A check that throws (whatever it throws), returns no list, or returns an invalid finding is an unwaivable error finding: a run that did not finish never passes and
 * no waiver (not even `path = "*"`) covers it (ER-33, ADR 0089 D8). Waivers are applied last.
 * Returns { app, checks: [ids run], findings, waived, errors, warnings }.
 */
export async function runChecks({ root, app, only = null, options = {}, registry = checks }) {
  const config = loadConfig(root, app);
  const ctx = createContext({ root, config, options });
  const selected = only ? registry.filter((c) => only.includes(c.id)) : registry;
  const unknown = (only ?? []).filter((id) => !registry.some((c) => c.id === id));
  if (unknown.length) throw new Error(`unknown check: ${unknown.join(', ')}`);

  let findings = [];
  const inlineWaived = [];
  const looked = new Set(); // checks whose result is a real result (a waiver for the others is not "stale", it is unmeasured)
  const validFinding = (f) => f !== null && typeof f === 'object' && ['error', 'warn'].includes(f.severity) && typeof f.message === 'string';
  for (const check of selected) {
    try {
      const found = await check.run(ctx);
      if (!Array.isArray(found)) {
        findings.push(unwaivable(error(check.id, check.rule, '', 1, `the check did not complete: it returned ${found === undefined ? 'nothing' : typeof found} instead of a list of findings`, 'Fix the check; a run that is incomplete never passes.'), 'not-measured'));
        continue;
      }
      const bad = found.findIndex((f) => !validFinding(f));
      if (bad >= 0) {
        const shown = JSON.stringify(found[bad]) ?? String(found[bad]);
        findings.push(unwaivable(error(check.id, check.rule, '', 1, `check broken: it returned an invalid finding (item ${bad}: ${shown.slice(0, 120)}); a finding is an object with severity "error" or "warn" and a message`, 'Fix the check so every finding is made with error() or warn().')));
        continue;
      }
      // A finding the check marked `inlineWaiver: <reason>` is waived at its site (a comment): it counts as waived, never as a finding,
      // and stays in the report (ER-33). `expires` is 'inline': there is no date.
      for (const f of found) {
        if (typeof f.inlineWaiver === 'string') {
          const { inlineWaiver, ...rest } = f;
          inlineWaived.push({ ...rest, waiver: { reason: inlineWaiver, expires: 'inline' } });
        } else findings.push(f);
      }
      looked.add(check.id);
    } catch (e) {
      findings.push(unwaivable(error(check.id, check.rule, '', 1, `the check crashed: ${String(e?.message ?? e)}`, 'Fix the check or the input it could not read; a check that cannot run never passes.')));
    }
  }

  const waiverText = ctx.read('paddock.waivers.toml');
  const parsed = waiverText ? parseWaivers(waiverText) : { waivers: [], problems: [] };
  const applied = applyWaivers({ findings, ...parsed, now: ctx.now, pathMatches, checksRun: selected.map((c) => c.id).filter((id) => looked.has(id)), skipUnused: (w) => w.check === 'C99' && !inScope(options.scope, w.path) });
  findings = applied.findings;
  return {
    app: config.app,
    checks: selected.map((c) => c.id),
    findings,
    waived: [...applied.waived, ...inlineWaived],
    errors: findings.filter((f) => f.severity === 'error').length,
    warnings: findings.filter((f) => f.severity === 'warn').length,
  };
}
