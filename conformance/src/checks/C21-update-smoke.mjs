// C21: the service-after-update smoke is a local tool, run before a release (user decision 2026-10-28: no VMs in CI;
// problem R03). Where the repository has the smoke (`scripts/shell-demo/smoke-service-update.sh`): the one-command runner
// `tools/vm-smoke.sh` exists, the shared releasing block (docs/blocks/releasing.md) asks for it, and no workflow runs it (macos-vm-smoke.sh and tart too; a CI runner never registers real
// services, rule 7, and CI has no VMs).
import { error } from '../finding.mjs';

const SMOKE = 'scripts/shell-demo/smoke-service-update.sh';
const RUNNER = 'tools/vm-smoke.sh';
const BLOCK = 'docs/blocks/releasing.md';

export default {
  id: 'C21',
  title: 'The update smoke is a local tool and a release checklist step',
  rule: 7,
  run(ctx) {
    const files = ctx.files();
    if (!files.includes(SMOKE)) return [];
    const out = [];
    if (!files.includes(RUNNER)) out.push(error('C21', 7, SMOKE, 1, `${RUNNER} is missing`, 'Keep the one-command local runner (vm-smoke.sh macos|linux).'));
    const list = ctx.read(BLOCK);
    if (list == null) out.push(error('C21', 7, BLOCK, 1, `${BLOCK} is missing`, 'Keep the shared releasing block with the pre-tag step "run the local VM update smoke".'));
    else if (!/vm-smoke\.sh/.test(list)) out.push(error('C21', 7, BLOCK, 1, 'the releasing block does not ask for the local VM update smoke', 'Add the step to "Before tagging": tools/vm-smoke.sh macos and linux.'));
    for (const w of ctx.workflows()) {
      if (/vm-smoke\.sh|smoke-service-update|build-macos-n-n1|build-linux-arm64|\btart\s+(run|clone|exec|pull)\b/.test(w.text.split('\n').filter((l) => !/^\s*#/.test(l)).join('\n'))) {
        out.push(error('C21', 7, w.file, 1, 'a workflow runs the VM update smoke; CI has no VMs', 'Run it locally (tools/vm-smoke.sh) and keep it out of .gitea/workflows.'));
      }
    }
    return out;
  },
};
