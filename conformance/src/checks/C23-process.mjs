// C23, first part: commit messages follow the shared commit-lint rules (Conventional Commits; no Co-Authored-By; no AI
// attribution). Second part (plan X43): the shared doc blocks (AGENTS.md rules and common sections, suite lessons,
// releasing, first launch) between `<!-- paddock:block -->` markers equal Paddock's docs/blocks. The second part needs
// `--paddock`; without it C28 already warns, so this part stays quiet.
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { error, warn } from '../finding.mjs';
import { lint, messagesInRange } from '../commit-lint.mjs';
import { paddockRoot } from '../sync/paddock-root.mjs';
import { planBlocks } from '../sync/plan.mjs';

const WHAT = {
  missing: (b) => `the shared block "${b}" is missing`,
  unmarked: (b) => `the section "${b}" is not between paddock:block markers (it may have drifted)`,
  outdated: (b) => `the shared block "${b}" is outdated: Paddock's source changed`,
  edited: (b) => `the shared block "${b}" was edited by hand inside its markers`,
  broken: (b) => `the markers of the shared block "${b}" are unmatched`,
};

/** Findings for the doc blocks of an app (empty when ok). */
export function blockFindings(ctx) {
  const paddock = paddockRoot(ctx.options);
  if (!paddock) return [];
  const { items } = planBlocks({ root: ctx.root, paddock, config: ctx.config });
  return items
    .filter((i) => i.state !== 'ok')
    .map((i) => error('C23', 8, i.path, 1, `${i.path}: ${WHAT[i.state](i.block)}`, i.state === 'broken' ? 'Repair the markers by hand, then run `paddock-conformance sync --write`.' : 'Run `paddock-conformance sync --write`; change the text in Paddock (docs/blocks) first, and keep app-specific text outside the markers.'));
}

const check = {
  id: 'C23',
  title: 'Commit messages: Conventional Commits, no co-author or AI attribution',
  rule: 8,
  run(ctx) {
    return [...commitFindings(ctx), ...blockFindings(ctx)];
  },
};

function commitFindings(ctx) {
  if (!existsSync(join(ctx.root, '.git'))) return [warn('C23', 8, '.git', 1, 'not a git checkout (an export?): commit messages were not checked', 'Run the conformance job on a checkout with history.')];
  const out = [];
  const range = ctx.config.commitRange ?? '-200';
  let items;
  try {
    items = messagesInRange(range, ctx.root, ctx.options.git);
  } catch (e) {
    return [error('C23', 8, '.git', 1, `cannot read the commit history (${range}): ${e.message}`, 'Fetch enough history (fetch-depth) or set commitRange in paddock.conformance.json.')];
  }
  for (const { id, message } of items) {
    for (const p of lint(message)) out.push(error('C23', 8, `commit ${id}`, 1, p, 'Reword the commit (git commit --amend / rebase) before it is pushed.'));
  }
  return out;
}

export default check;
