#!/usr/bin/env node
// Debrief's entry to the commit message check. The rules are Paddock's own (conformance/src/commit-lint.mjs: Conventional
// Commits, no co-author or AI attribution, a re-land names what it reverts); this file only forwards to them (rule 55: one
// implementation, no copy).
//   node scripts/commit-lint.mjs --file .git/COMMIT_EDITMSG   (commit-msg hook)
//   node scripts/commit-lint.mjs --range origin/main..HEAD    (CI; merge commits are not checked)
// Exits 1 with one line per bad message.

import { commitLintMain, lint } from '../../../conformance/src/commit-lint.mjs';
import { isMain } from '../../../conformance/src/util/is-main.mjs';

export { lint };

if (isMain(import.meta.url)) process.exit(commitLintMain(process.argv.slice(2)));
