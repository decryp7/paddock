// C137 (ER-21 class "a spec imports what its CI job never builds": the Debrief dock spec imported the kit by package name, which
// resolves to `ui/dist`; Debrief's e2e job has no `build:e2e` step, so every shard failed with "Cannot find module" and ran no test; review of
// 7bcba6e54, 2026-11-17).
// An app's e2e spec (`apps/<app>/app/e2e/*.ts`) that value-imports `@decryp7/paddock-e2e` gets the built package (`ui/dist/paddock-e2e`). It needs
// one of: the app's workflow (`.gitea/workflows/<app>.yml`, else `ci.yml`) has a `build:e2e` step, or the spec imports the kit's source
// by a relative path (as `e2e/test.ts` does with `pinned-clock.ts`). `import type` is erased and is not read.
// ER-41: report-only (warnings) until its first green run on the real tree, recorded by run id here. Recorded: not yet.
import { warn } from '../finding.mjs';

export const SEVERITY = warn;
const SPEC = /^apps\/([^/]+)\/app\/e2e\/.+\.ts$/;
const KIT_IMPORT = /^\s*(?:import|export)\s+(?!type\b)[^;]*?\bfrom\s*['"]@decryp7\/paddock-e2e(?:\/[^'"]*)?['"]/m;
const BUILD_STEP = /\bbuild:e2e\b/;

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
      if (!builds.has(app)) {
        const text = ctx.read(`.gitea/workflows/${app}.yml`) ?? ctx.read('.gitea/workflows/ci.yml') ?? '';
        builds.set(app, BUILD_STEP.test(text));
      }
      return builds.get(app);
    };
    for (const f of ctx.files()) {
      const m = SPEC.exec(f);
      if (!m || !f.endsWith('.ts') || !/paddock-e2e/.test(ctx.read(f) ?? '')) continue;
      const line = kitImportLine(ctx.read(f));
      if (!line || builtFor(m[1])) continue;
      out.push(
        SEVERITY('C137', 21, f, line, `imports @decryp7/paddock-e2e by package name, which resolves to ui/dist, but ${m[1]}'s workflow has no \`build:e2e\` step: every shard fails with "Cannot find module"`,
          'Import the source by a relative path (`../../../../ui/projects/paddock-e2e/src/<file>.ts`, as e2e/test.ts does), or add the `npm run build:e2e -w ui` step to the app\'s workflow.'),
      );
    }
    return out;
  },
};
