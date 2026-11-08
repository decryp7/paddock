// C28: the repo baseline. The config files every app repo carries (editorconfig, rustfmt, licences, deny/cliff/lefthook,
// Renovate preset, tsconfig, Cargo profiles, ...) equal Paddock's templates/repo; `paddock-conformance sync --write`
// repairs them. Plan X27.
import { error, warn } from '../finding.mjs';
import { paddockRoot } from '../sync/paddock-root.mjs';
import { planBaseline } from '../sync/plan.mjs';

export default {
  id: 'C28',
  scope: 'apps',
  mode: 'gate',
  needs: 'all',
  title: 'The repo baseline files equal the Paddock templates',
  rule: 42,
  run(ctx) {
    const paddock = paddockRoot(ctx.options);
    if (!paddock) return [warn('C28', 42, 'templates/repo', 1, 'no Paddock checkout with templates/repo (--paddock): the baseline was not compared', 'Run with --paddock <checkout of paddock>.')];
    return planBaseline({ root: ctx.root, paddock, config: ctx.config })
      .filter((i) => i.state !== 'ok')
      .map((i) => error('C28', 42, i.path, 1, `${i.path}: ${i.detail}`, 'Run `paddock-conformance sync --write` (the template is changed in Paddock first), or waive this file with a reason and a date.'));
  },
};
