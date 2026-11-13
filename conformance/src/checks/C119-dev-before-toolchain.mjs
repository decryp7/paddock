// C119: no workflow job runs `./dev` before its toolchain is set up (ER-9, ER-26). `./dev` hashes its own sources with `rustc -vV` and
// builds itself with that compiler, so a step that runs it before `.gitea/actions/setup-rust` (or any action that goes through
// `conformance/ci/kit.sh`, whose first act is `./dev --ensure-toolchain`) can hash and build with another compiler than the job's later
// steps, or find none (run 8034: build-jobs ran before the pinned toolchain existed; CI-in-Rust WP-3 review: five root jobs called
// `./dev ci run` with no setup-rust). Judged per job of `.gitea/workflows/*.yml`: a step whose `run:` script has a line that runs
// `./dev` is a finding unless an earlier step of the same job `uses:` a local action that sets the toolchain up (an action whose
// text names `kit.sh`, `--ensure-toolchain` or `actions/setup-rust`) or runs `kit.sh` / `--ensure-toolchain` itself. A justified case
// carries `conformance: dev-before-toolchain — <why>` (any comment form) on the line or the line above. ER-41: report-only (warnings)
// until its first green run on main; flip SEVERITY to `error` and `mode` to 'gate' then (docs/gates.toml, C119).
// The Rust host has the same check (tools/paddock-dev/src/check/c119.rs); tools/paddock-dev/tests/it/check_c119.rs holds them equal.
// Runs for the Paddock root (`--app paddock`) only. No workflow file found is "check broken" (ER-33), never a pass.
import { error, unwaivable, warn } from '../finding.mjs';

/** The severity of a finding: report-only for now (ER-41). */
export const SEVERITY = warn;
const WAIVER = /conformance:\s*dev-before-toolchain[ \t]*[—–-]+[ \t]*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*dev-before-toolchain/;
const WORKFLOW = /^\.gitea\/workflows\/[^/]+\.ya?ml$/;
const ACTION = /^\.gitea\/actions\/([^/]+)\/action\.ya?ml$/;
const SETS_UP = /kit\.sh|--ensure-toolchain|actions\/setup-rust/;
const RUNS_DEV = /(?:^|[\s;&|(])\.\/dev(?:\s|$)/;
const FIX = 'Add `- uses: ./.gitea/actions/setup-rust` before the first step that runs `./dev` (the job then hashes and builds the tool with the pinned compiler). A justified case carries `# conformance: dev-before-toolchain — <why>`.';

/** The local actions that set the toolchain up: those whose text goes through kit.sh or names setup-rust. */
export function toolchainActions(actionTexts) {
  const names = new Set(['setup-rust']);
  for (const [name, text] of actionTexts) if (SETS_UP.test(text)) names.add(name);
  return names;
}

const indentOf = (l) => l.length - l.trimStart().length;

/** The `./dev` runs before any toolchain setup in one workflow text: [{ line, job, waived }] (lines are 1-based). */
export function devBeforeToolchain(text, setupActions) {
  const lines = text.split('\n');
  const out = [];
  let inJobs = false;
  let job = null;
  let inSteps = false;
  let ready = false;
  let scriptIndent = -1;
  lines.forEach((raw, i) => {
    const l = raw.replace(/\r$/, '');
    const blank = l.trim() === '';
    if (!blank && indentOf(l) === 0 && !l.trimStart().startsWith('#')) {
      inJobs = /^jobs:\s*(?:#.*)?$/.test(l);
      job = null; inSteps = false; scriptIndent = -1;
      return;
    }
    if (!inJobs || blank) return;
    const ind = indentOf(l);
    if (scriptIndent >= 0 && ind > scriptIndent) {
      if (!l.trimStart().startsWith('#')) check(l, i);
      return;
    }
    scriptIndent = -1;
    if (ind === 2 && /^ {2}[A-Za-z0-9_-]+:\s*(?:#.*)?$/.test(l)) { job = l.trim().replace(/:.*$/, ''); inSteps = false; ready = false; return; }
    if (job === null) return;
    if (ind === 4 && /^ {4}steps:\s*(?:#.*)?$/.test(l)) { inSteps = true; return; }
    if (ind === 4 && !l.trimStart().startsWith('#')) { inSteps = false; return; }
    if (!inSteps) return;
    const body = l.replace(/^\s*-\s+/, '').trimStart();
    const uses = /^uses:\s*\.\/\.gitea\/actions\/([\w-]+)/.exec(body);
    if (uses && setupActions.has(uses[1])) { ready = true; return; }
    const run = /^run:\s*(.*)$/.exec(body);
    if (run) {
      const v = run[1].replace(/\s+#.*$/, '');
      if (/^[|>][+-]?\d*\s*$/.test(v)) { scriptIndent = ind; return; }
      check(v, i);
    }
  });
  return out;

  function check(scriptLine, i) {
    if (scriptLine.trimStart().startsWith('#')) return;
    if (/kit\.sh|--ensure-toolchain/.test(scriptLine)) { ready = true; return; }
    if (ready || !RUNS_DEV.test(scriptLine)) return;
    const near = `${lines[i - 1] ?? ''}\n${lines[i]}`;
    const w = WAIVER.exec(near);
    out.push({ line: i + 1, job, waived: w ? w[1] : BARE_WAIVER.test(near) ? 'no-reason' : undefined });
    ready = true; // one finding per job: the first run is the one to fix
  }
}

export default {
  id: 'C119',
  title: 'no workflow job runs ./dev before its toolchain is set up',
  rule: 3,
  scope: 'root',
  mode: 'report-only',
  run(ctx) {
    if (ctx.app !== 'paddock') return [];
    const files = ctx.files();
    const workflows = files.filter((f) => WORKFLOW.test(f)).sort();
    if (workflows.length === 0) {
      return [unwaivable(error('C119', 3, '.gitea/workflows', 1, 'check broken: no workflow file was found to read', 'Run it with --root at the repository root.'))];
    }
    const actions = [];
    for (const f of files) {
      const m = ACTION.exec(f);
      if (m) actions.push([m[1], ctx.read(f) ?? '']);
    }
    const setup = toolchainActions(actions);
    const out = [];
    for (const file of workflows) {
      const text = ctx.read(file);
      if (text === null || text === undefined) {
        out.push(unwaivable(error('C119', 3, file, 1, 'check broken: the file cannot be read', 'Fix the file permissions or remove the file.')));
        continue;
      }
      for (const f of devBeforeToolchain(text, setup)) {
        if (f.waived && f.waived !== 'no-reason') continue;
        const bare = f.waived === 'no-reason' ? ' (the waiver comment has no reason)' : '';
        out.push(SEVERITY('C119', 3, file, f.line, `job \`${f.job}\` runs ./dev before its toolchain is set up${bare} (ER-9: ./dev hashes and builds itself with the job's compiler)`, FIX));
      }
    }
    return out;
  },
};
