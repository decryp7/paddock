import { createContext, loadConfig } from './context.mjs';
import { error, unwaivable } from './finding.mjs';
import { checks } from './checks/index.mjs';
import { inScope } from './util/scope.mjs';
import { pathMatches } from './util/fs.mjs';
import { applyWaivers, parseWaivers } from './util/waivers.mjs';

/**
 * Runs the selected checks over `root`. A check that throws, or returns no list of findings, is an unwaivable error finding: a run that did not finish never passes and
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
  for (const check of selected) {
    try {
      const found = await check.run(ctx);
      if (!Array.isArray(found)) {
        findings.push(unwaivable(error(check.id, check.rule, '', 1, `the check did not complete: it returned ${found === undefined ? 'nothing' : typeof found} instead of a list of findings`, 'Fix the check; a run that is incomplete never passes.')));
        continue;
      }
      findings.push(...found);
    } catch (e) {
      findings.push(unwaivable(error(check.id, check.rule, '', 1, `the check crashed: ${e.message}`, 'Fix the check or the input it could not read; a check that cannot run never passes.')));
    }
  }

  const waiverText = ctx.read('paddock.waivers.toml');
  const parsed = waiverText ? parseWaivers(waiverText) : { waivers: [], problems: [] };
  const applied = applyWaivers({ findings, ...parsed, now: ctx.now, pathMatches, checksRun: selected.map((c) => c.id), skipUnused: (w) => w.check === 'C99' && !inScope(options.scope, w.path) });
  findings = applied.findings;
  return {
    app: config.app,
    checks: selected.map((c) => c.id),
    findings,
    waived: applied.waived,
    errors: findings.filter((f) => f.severity === 'error').length,
    warnings: findings.filter((f) => f.severity === 'warn').length,
  };
}
