// C14: tests run hermetically (see hermetic.mjs, `paddock-conformance run -- <cmd>`). Static part: a workflow step that runs
// tests must go through the wrapper or set HOME and TMPDIR itself. Catches T01, T03, T09 (98k temp folders).
import { warn } from '../finding.mjs';

const TEST = /\b(?:cargo\s+(?:test|nextest)|ng\s+test|npm\s+(?:run\s+)?test|npx\s+(?:ng\s+test|vitest|playwright\s+test)|vitest|playwright\s+test)\b/;

export default {
  id: 'C14',
  title: 'Tests run with HOME and TMPDIR in a scratch folder',
  rule: 7,
  run(ctx) {
    const out = [];
    for (const w of ctx.workflows()) {
      const bare = w.steps.filter((s) => s.run && TEST.test(s.run) && !/paddock-conformance\s+run\b/.test(s.run) && !(('HOME' in s.env) && ('TMPDIR' in s.env)));
      if (bare.length) {
        out.push(warn('C14', 7, w.file, bare[0].line, `${bare.length} step(s) run tests outside the hermetic wrapper (first: "${bare[0].name ?? 'unnamed'}")`, 'Run them as `paddock-conformance run -- <command>` so temp leaks, stray home files and left-behind processes fail the step.'));
      }
    }
    return out;
  },
};
