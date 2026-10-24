// C23, first part: commit messages follow the shared commit-lint rules (Conventional Commits; no Co-Authored-By; no AI
// attribution). Second part (AGENTS.md rules block equals the pinned Paddock revision) is not built yet.
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { error, warn } from '../finding.mjs';
import { lint, messagesInRange } from '../commit-lint.mjs';

export default {
  id: 'C23',
  title: 'Commit messages: Conventional Commits, no co-author or AI attribution',
  rule: 8,
  run(ctx) {
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
  },
};
