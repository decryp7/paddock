// C167 (ER-21 class "a test that needs the git history runs in a one-commit checkout": scripts/npm-audit.test.mjs's ER-46 growth test measures
// its allow list against the merge base and fails as `not measured: no merge base` when the job checked out one commit. Run 8327 (ci.yml's ui and
// tools jobs, fixed by 138f8b879) and run 8807 (shell-demo-release.yml's gates job; workbench-demo-release.yml and publish.yml have the same step)).
// A workflow job that runs those tests, by `npm run test:scripts` or `npm-audit.test` in a step's `run`, or by `./dev ci run <project> <phase>`
// whose phase in the project's `paddock.ci.json` runs them, needs (1) a `actions/checkout` step with `fetch-depth: 0` and (2) PADDOCK_BASE_REF,
// in the job's `env` or the running step's `env` (the dispatch's base, else origin/main, as 138f8b879 set it).
// ER-42: workflows are read through the `yaml` parser (as C111 does) and the contracts through their JSON, never by grepping text. Comment lines
// of a `run` do not count. A workflow that does not parse, or no workflow at all, is "check broken" (ER-33), never a pass.
// ER-41: report-only (warnings) until its first green run on the real tree, recorded by run id here. Recorded: not yet.
import { error, unwaivable, warn } from '../finding.mjs';
import { BASE_REF_VAR } from '../util/baseline.mjs';

export const SEVERITY = warn;
const FILE = /^\.gitea\/workflows\/[^/]+\.ya?ml$/;
const GROWTH_TESTS = /\bnpm run test:scripts\b|\bnpm-audit\.test\b/;
const DEV_CI_RUN = /(?:^|[\s;&|])\.\/dev ci run (\S+) (\S+)/;
const FIX = `Give the job \`${BASE_REF_VAR}: \${{ inputs.base || 'origin/main' }}\` in its \`env\` and \`fetch-depth: 0\` on its actions/checkout step, as ci.yml's ui job does (138f8b879). Re-run: node conformance/bin/paddock-conformance.mjs --app paddock --root . --only C167`;

/** The command lines of a `run` value that are not comments. */
const codeLines = (run) => String(run ?? '').split('\n').map((l) => l.trim()).filter((l) => l && !l.startsWith('#'));

/** Whether the project's contract phase runs the growth tests: true or false; null when the contract cannot be read. */
function phaseRunsTests(ctx, project, phase) {
  for (const path of [`${project}/paddock.ci.json`, `apps/${project}/paddock.ci.json`]) {
    const text = ctx.read(path);
    if (text === null || text === undefined) continue;
    try {
      const commands = JSON.parse(text)[phase];
      return Array.isArray(commands) && commands.some((c) => GROWTH_TESTS.test(String(c)));
    } catch {
      return null;
    }
  }
  return null;
}

/** Whether a step runs the growth tests (true), does not (false), or runs a phase whose contract cannot be read (null). */
function stepRunsTests(ctx, step) {
  let unknown = false;
  for (const line of codeLines(step.run)) {
    if (GROWTH_TESTS.test(line)) return true;
    const m = DEV_CI_RUN.exec(line);
    if (!m) continue;
    const r = phaseRunsTests(ctx, m[1], m[2]);
    if (r === true) return true;
    if (r === null) unknown = true;
  }
  return unknown ? null : false;
}

const isCheckout = (s) => /^actions\/checkout(@|$)/.test(String(s?.uses ?? ''));
const hasBase = (env) => typeof env?.[BASE_REF_VAR] === 'string' && env[BASE_REF_VAR].trim() !== '';

/** The problems of one job: [] when it does not run the tests or has what they need. */
export function jobProblems(ctx, job) {
  const steps = Array.isArray(job?.steps) ? job.steps : [];
  const runners = steps.filter((s) => stepRunsTests(ctx, s) === true);
  if (!runners.length) return [];
  const problems = [];
  const checkouts = steps.filter(isCheckout);
  if (!checkouts.some((s) => Number(s.with?.['fetch-depth']) === 0 && s.with?.['fetch-depth'] !== '')) {
    problems.push(checkouts.length ? '`fetch-depth: 0` on its actions/checkout step (it fetches one commit, so there is no merge base)' : 'a full-history actions/checkout step (`fetch-depth: 0`)');
  }
  if (!hasBase(job.env) && !runners.every((s) => hasBase(s.env))) problems.push(`${BASE_REF_VAR} in its \`env\` (the base the growth test measures against)`);
  return problems;
}

export default {
  id: 'C167',
  scope: 'root',
  mode: 'report-only',
  title: 'A workflow job that runs the merge-base growth tests checks out the history and names the base ref',
  rule: 21,
  async run(ctx) {
    if (ctx.app && ctx.app !== 'paddock') return [];
    let parse;
    try {
      ({ parse } = await import('yaml'));
    } catch (e) {
      return [unwaivable(error('C167', 21, 'conformance/package.json', 1, `check broken: the yaml package cannot be loaded (${e.code ?? e.message})`, 'Run `npm ci --ignore-scripts -w conformance` at the repository root.'))];
    }
    const files = ctx.files().filter((f) => FILE.test(f));
    if (!files.length) return [unwaivable(error('C167', 21, '.gitea/workflows', 1, 'check broken: no workflow file was found to read', 'Run it with --root at the repository root.'))];
    const out = [];
    for (const file of files) {
      let doc;
      try {
        doc = parse(ctx.read(file) ?? '');
      } catch (e) {
        out.push(unwaivable(error('C167', 21, file, 1, `check broken: the workflow does not parse (${String(e.message).split('\n')[0]})`, 'Fix the YAML (C111 names the line).')));
        continue;
      }
      for (const [id, job] of Object.entries(doc?.jobs ?? {})) {
        const problems = jobProblems(ctx, job);
        if (!problems.length) continue;
        out.push(SEVERITY('C167', 21, file, 1, `job \`${id}\` runs scripts/npm-audit.test.mjs's growth test, which needs the git history, but lacks ${problems.join(' and ')}: it fails as "not measured: no merge base" (runs 8327, 8807)`, FIX));
      }
    }
    return out;
  },
};
