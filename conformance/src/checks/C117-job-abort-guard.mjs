// C117 (rule 3: fix everything locally first; ER-9, ER-33): a step that still runs after an earlier step failed (`if: ... !cancelled()`,
// so one run reports every failed check) in a job that sets up with build-jobs or job-isolation must also test
// `env.PADDOCK_JOB_ABORTED != '1'`. When that setup fails (./dev could not be built, no slot) conformance/ci/kit.sh writes the variable
// to GITHUB_ENV; without the test the job goes on and runs its checks without the slot, temp folders and core share (run 8027: crate on
// Windows ran its tests after ./dev failed). The second sighting of "a job that went on after its setup failed" (runs 7773 to 7775, 8027).
//
// Reads `.gitea/workflows/*.yml|yaml` and `.gitea/actions/**/action.yml|yaml` (the `yaml` package, as C111). A job is isolated when one
// of its steps uses ./.gitea/actions/build-jobs or ./.gitea/actions/job-isolation; the composite actions such a job uses are held to the
// same rule in their own steps. A job-level `if:` is not a step and is not read (the `env` context does not exist there, and a job
// starts with its own setup). Report-only (a warning) until its first green run (ER-41); then `mode: 'gate'`.
// Positive controls (ER-33): no workflow found, or no `yaml` package, is "check broken", never a pass; a repository with workflows
// but no isolated job at all also means the check saw nothing it was built for, so it is "check broken" too.
import { error, unwaivable, warn } from '../finding.mjs';

const WORKFLOW = /^\.gitea\/workflows\/[^/]+\.ya?ml$/;
const ACTION = /^\.gitea\/actions\/((?:.+\/)?[^/]+)\/action\.ya?ml$/;
const SETUP_USES = /^\.\/\.gitea\/actions\/(?:build-jobs|job-isolation)\/?$/;
const LOCAL_USES = /^\.\/\.gitea\/actions\/(.+?)\/?$/;
const GUARD = 'PADDOCK_JOB_ABORTED';

const NEXT = "Add `&& env.PADDOCK_JOB_ABORTED != '1'` after `!cancelled()` in this step's `if:` (conformance/ci/kit.sh job_setup sets it when the job's setup failed).";

const plain = (node) => (node && typeof node.toJSON === 'function' ? node.toJSON() : node);

/** The steps of a job or composite action as [{ uses, condition, line }]; `line` is the `if:` value's line, else the step's. */
function stepsOf(seq, lineOf) {
  const out = [];
  for (const step of seq?.items ?? []) {
    if (!step || typeof step.get !== 'function') continue;
    const cond = step.get('if', true);
    out.push({
      uses: String(plain(step.get('uses')) ?? ''),
      condition: cond === undefined ? '' : String(plain(cond) ?? ''),
      line: lineOf((cond ?? step).range?.[0] ?? 0),
    });
  }
  return out;
}

/**
 * Findings-to-be of one text: { jobs: [{ name, isolated, used, steps }], steps } for a workflow, { steps } for an action.
 * Exported for the tests.
 */
export function readFile(text, { parseDocument, LineCounter }, kind) {
  const lc = new LineCounter();
  const doc = parseDocument(text, { lineCounter: lc });
  const lineOf = (offset) => lc.linePos(offset).line;
  if (kind === 'action') return { steps: stepsOf(doc.getIn(['runs', 'steps'], true), lineOf) };
  const jobs = [];
  const map = doc.get('jobs', true);
  for (const pair of map?.items ?? []) {
    const steps = stepsOf(pair.value?.get?.('steps', true), lineOf);
    jobs.push({
      name: String(plain(pair.key)),
      isolated: steps.some((s) => SETUP_USES.test(s.uses)),
      used: steps.map((s) => LOCAL_USES.exec(s.uses)?.[1]).filter(Boolean),
      steps,
    });
  }
  return { jobs };
}

export default {
  id: 'C117',
  title: 'steps that run after a failure also stop when the job setup failed',
  rule: 3,
  scope: 'root',
  mode: 'report-only',
  async run(ctx) {
    if (ctx.app !== 'paddock') return [];
    let yaml;
    try {
      yaml = await import('yaml');
    } catch (e) {
      return [unwaivable(error('C117', 3, 'conformance/package.json', 1, `check broken: the yaml package cannot be loaded (${e.code ?? e.message})`, 'Run `npm ci --ignore-scripts` in conformance/.'))];
    }
    const files = ctx.files();
    const workflows = files.filter((f) => WORKFLOW.test(f));
    if (workflows.length === 0) {
      return [unwaivable(error('C117', 3, '.gitea', 1, 'check broken: no workflow file was found to read', 'Run it with --root at the repository root; a tree without .gitea/ should not select C117.'))];
    }
    const out = [];
    const isolatedActions = new Set();
    let isolatedJobs = 0;
    const check = (file, label, steps) => {
      for (const s of steps) {
        if (s.condition.includes('!cancelled()') && !s.condition.includes(GUARD)) {
          out.push(warn('C117', 3, file, s.line, `${label}: this step runs after a failed step (!cancelled()) but not after the job's setup failed (no ${GUARD} test)`, NEXT));
        }
      }
    };
    for (const file of workflows) {
      const text = ctx.read(file);
      if (text == null) continue;
      const { jobs } = readFile(text, yaml, 'workflow');
      for (const job of jobs.filter((j) => j.isolated)) {
        isolatedJobs++;
        job.used.forEach((a) => isolatedActions.add(a));
        check(file, `job ${job.name}`, job.steps);
      }
    }
    if (isolatedJobs === 0) {
      return [unwaivable(error('C117', 3, '.gitea/workflows', 1, 'check broken: no job that uses build-jobs or job-isolation was found, so the check saw nothing it was built for', 'Fix the check (a renamed action?) or retire it.'))];
    }
    for (const file of files) {
      const name = ACTION.exec(file)?.[1];
      if (!name || !isolatedActions.has(name)) continue;
      const text = ctx.read(file);
      if (text == null) continue;
      check(file, `action ${name} (used by an isolated job)`, readFile(text, yaml, 'action').steps);
    }
    return out;
  },
};
