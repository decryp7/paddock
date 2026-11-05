// The verdict of a VM run, derived from what was measured (ER-27, audit "a result can't pass while measuring nothing"). A run is PASS only when
// every step its manifest requires (manifest.mjs) ran and passed. FAIL: a required or other step failed in the product. INCOMPLETE: something
// required is missing, not measured, skipped, or its check was broken; it has its own exit code, so a script can tell it from FAIL and from an
// error of the tool. Pure: no I/O, no clock.

/** Exit codes of every runner: 0 PASS, 1 a product FAIL, 2 the tool could not run, 3 INCOMPLETE (nothing failed, but not everything was measured). */
export const EXIT = { PASS: 0, FAIL: 1, ERROR: 2, INCOMPLETE: 3 };

/**
 * Builds a step from checks. A check may carry `kind: 'check'` (the measurement or its control failed: not a statement about the product) or
 * `info: true` (a fact for reading, never a pass or a fail). Failing product checks make the step `fail`; when every failing check is of kind
 * `check` the step is `check-broken`; a step with no judged check at all is `not-measured`, so a literal `ok: true` row cannot make a pass.
 */
export function makeResult(title, checks, evidence = [], pass = '') {
  const judged = checks.filter((c) => !c.info);
  const bad = judged.filter((c) => !c.ok);
  const product = bad.filter((c) => c.kind !== 'check');
  const status = product.length ? 'fail' : bad.length ? 'check-broken' : judged.length ? 'pass' : 'not-measured';
  const summary = bad.length ? (product.length ? product : bad).map((c) => c.detail).join('; ').slice(0, 240) : judged.length ? pass : 'nothing was judged';
  return { title, status, cls: status === 'check-broken' ? 'check' : 'product', summary, checks: checks.map((c) => ({ ...c, ok: c.info ? true : c.ok })), evidence };
}

/** 'check' when the step's failure is the measurement's, 'product' otherwise. */
export function stepClass(step) {
  if (step.status === 'check-broken') return 'check';
  if (step.cls) return step.cls;
  const bad = (step.checks ?? []).filter((c) => !c.ok);
  return step.status === 'fail' && bad.length > 0 && bad.every((c) => c.kind === 'check') ? 'check' : 'product';
}

/**
 * @param {{steps: object[], required: {id:string, match:RegExp}[], partial?: boolean}} run
 * @returns {{verdict:'PASS'|'FAIL'|'INCOMPLETE', missing:{id:string, why:string}[], failed:object[], broken:object[]}}
 */
export function deriveVerdict({ steps, required }) {
  const failed = steps.filter((s) => s.status === 'fail' && stepClass(s) === 'product');
  const broken = steps.filter((s) => s.status === 'check-broken' || (s.status === 'fail' && stepClass(s) === 'check'));
  const missing = [];
  for (const r of required) {
    const found = steps.filter((s) => r.match.test(s.title));
    if (!found.length) missing.push({ id: r.id, why: 'the step did not run' });
    else if (!found.some((s) => s.status === 'pass') && !found.some((s) => s.status === 'fail' && stepClass(s) === 'product')) {
      missing.push({ id: r.id, why: `the step ended ${found.map((s) => s.status).join('/')}${found[0].summary ? `: ${String(found[0].summary).slice(0, 120)}` : ''}` });
    }
  }
  if (!required.length) missing.push({ id: 'manifest', why: 'no required steps are declared for this run, so nothing can be proven' });
  if (!steps.length) missing.push({ id: 'steps', why: 'the run produced no steps' });
  const verdict = failed.length ? 'FAIL' : missing.length || broken.length ? 'INCOMPLETE' : 'PASS';
  return { verdict, missing, failed, broken };
}

export const exitCodeFor = (v) => EXIT[v.verdict];

const SHA12 = /^[0-9a-f]{12}$/;

/**
 * The build binding: the installed binary carries the commit CI built it from. `expected` is the first 12 hex digits of the CI run's head
 * commit, `found` whether the installed binary contains it (null: could not be read). Dyno 1.1.0 prints its version only, so the binary
 * is searched for the 12-digit string the build-env action embeds (`option_env!("PADDOCK_BUILD_COMMIT")`).
 */
export function buildStep({ expected, found, where = 'the installed dyno-app' }) {
  const title = 'Build binding: the installed binary is the commit CI built';
  if (!expected || !SHA12.test(expected)) return { title, status: 'not-measured', cls: 'check', summary: 'no expected build commit (give --expect-commit <12 hex> or --run <ci run id>)', checks: [{ ok: false, kind: 'check', detail: `expected commit ${expected ?? 'absent'} is not 12 hex digits` }], evidence: [] };
  if (found === null || found === undefined) return { title, status: 'not-measured', cls: 'check', summary: `could not read ${where}`, checks: [{ ok: false, kind: 'check', detail: `${where} could not be searched for ${expected}` }], evidence: [] };
  return makeResult(title, [{ ok: found === true, detail: found ? `${where} contains the build commit ${expected}` : `${where} does NOT contain the expected build commit ${expected}: it is another build` }], [], `built from ${expected}`);
}

/** The expected 12-digit commit from a CI head sha or an explicit value; null when it is not a hex sha. */
export function expectedCommit(sha) {
  const s = String(sha ?? '').trim().toLowerCase();
  return /^[0-9a-f]{12,40}$/.test(s) ? s.slice(0, 12) : null;
}
