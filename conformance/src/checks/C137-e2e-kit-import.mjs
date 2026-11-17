// C137 (ER-21 class "a spec imports what its CI job never builds": the Debrief dock spec imported the kit by package name, which
// resolves to `ui/dist`; Debrief's e2e job has no `build:e2e` step, so every shard failed with "Cannot find module" and ran no test; review of
// 7bcba6e54, 2026-11-17).
// An app's e2e spec (`apps/<app>/app/e2e/*.ts`) that value-imports `@decryp7/paddock-e2e` gets the built package (`ui/dist/paddock-e2e`). It needs
// one of: the app builds it in its own CI contract, or the spec imports the kit's source by a relative path (as `e2e/test.ts` does with
// `pinned-clock.ts`). `import type` is erased and is not read. The contract is `apps/<app>/paddock.ci.json` (ER-42: read through the declared
// data, never by grepping a workflow that may belong to another job): its `workflow` names the app's own workflow file, whose steps' `run` text
// must run `build:e2e` in each of its jobs that run Playwright (a build in another job, or a comment, does not count; a workflow with no Playwright job cannot be judged and is reported); an app without one is a leg of ci.yml, and each of its `e2e*` command lists (`e2e`, `e2eWebkit`) must run it.
// An app with no readable contract is reported (the check cannot tell).
// ER-41: report-only (warnings) until its first green run on the real tree, recorded by run id here. Recorded: not yet.
import { warn } from '../finding.mjs';
import { parseJobs, parseSteps } from '../util/workflows.mjs';

export const SEVERITY = warn;
const SPEC = /^apps\/([^/]+)\/app\/e2e\/.+\.ts$/;
const KIT_IMPORT = /^\s*(?:import|export)\s+(?!type\b)[^;]*?\bfrom\s*['"]@decryp7\/paddock-e2e(?:\/[^'"]*)?['"]/m;
const BUILD_STEP = /\bbuild:e2e\b/;

/** Whether command text runs `build:e2e` on a line that is not a comment. */
const runsBuild = (text) => (text ?? '').split('\n').some((l) => !l.trim().startsWith('#') && BUILD_STEP.test(l.replace(/\s#.*$/, '')));

/** Whether the app's declared CI builds the kit: true, false, or null when its contract cannot be read. */
export function appBuildsKit(ctx, app) {
  let contract;
  try {
    contract = JSON.parse(ctx.read(`apps/${app}/paddock.ci.json`) ?? '');
  } catch {
    return null;
  }
  if (typeof contract.workflow === 'string') {
    const text = ctx.read(`.gitea/workflows/${contract.workflow}`);
    if (text === null || text === undefined) return null;
    // Only the jobs that run Playwright count, each by its own steps (a build in another job is on another runner and does not carry over).
    const e2eJobs = parseJobs(text).filter((j) => parseSteps(j.text).some((st) => /\bplaywright\b/i.test(st.run ?? '')));
    return e2eJobs.length ? e2eJobs.every((j) => parseSteps(j.text).some((st) => runsBuild(st.run))) : null;
  }
  // A leg of ci.yml: the contract's e2e command lists (`e2e`, `e2eWebkit`, ...) are what the e2e legs run, one command per line.
  const lists = Object.entries(contract).filter(([k, v]) => /^e2e/i.test(k) && Array.isArray(v));
  return lists.length ? lists.every(([, v]) => v.some((c) => runsBuild(String(c)))) : null;
}

/** The 1-based line of the first value import of the kit by package name in `text`, or 0 when there is none. */
export function kitImportLine(text) {
  const m = KIT_IMPORT.exec(text);
  if (!m) return 0;
  return text.slice(0, m.index + m[0].search(/\S/)).split('\n').length;
}

export default {
  id: 'C137',
  scope: 'root',
  mode: 'report-only',
  title: 'An app e2e spec imports the kit by package name only when its CI job builds it',
  rule: 21,
  run(ctx) {
    const out = [];
    const builds = new Map();
    const builtFor = (app) => {
      if (!builds.has(app)) builds.set(app, appBuildsKit(ctx, app));
      return builds.get(app);
    };
    for (const f of ctx.files()) {
      const m = SPEC.exec(f);
      if (!m || !f.endsWith('.ts') || !/paddock-e2e/.test(ctx.read(f) ?? '')) continue;
      const line = kitImportLine(ctx.read(f));
      const built = line ? builtFor(m[1]) : true;
      if (built === true) continue;
      out.push(
        SEVERITY('C137', 21, f, line, built === null
            ? `imports @decryp7/paddock-e2e by package name (ui/dist) and apps/${m[1]}/paddock.ci.json or its workflow cannot be read, so the build cannot be checked`
            : `imports @decryp7/paddock-e2e by package name, which resolves to ui/dist, but ${m[1]}'s declared CI never runs \`build:e2e\`: every shard fails with "Cannot find module"`,
          'Import the source by a relative path (`../../../../ui/projects/paddock-e2e/src/<file>.ts`, as e2e/test.ts does), or add the `npm run build:e2e -w ui` step to the app\'s workflow.'),
      );
    }
    return out;
  },
};
