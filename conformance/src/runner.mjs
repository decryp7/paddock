import { createContext, loadConfig } from './context.mjs';
import { error } from './finding.mjs';
import { checks } from './checks/index.mjs';
import { pathMatches } from './util/fs.mjs';
import { applyWaivers, parseWaivers } from './util/waivers.mjs';

/**
 * Runs the selected checks over `root`. A check that throws is an error finding (it fails closed). Waivers are applied last.
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
      findings.push(...(await check.run(ctx)));
    } catch (e) {
      findings.push(error(check.id, check.rule, '', 1, `the check crashed: ${e.message}`, 'Fix the check or the input it could not read; a check that cannot run never passes.'));
    }
  }

  const waiverText = ctx.read('paddock.waivers.toml');
  const parsed = waiverText ? parseWaivers(waiverText) : { waivers: [], problems: [] };
  const applied = applyWaivers({ findings, ...parsed, now: ctx.now, pathMatches });
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
