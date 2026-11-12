// C110: every Markdown table in a tracked .md file renders as a table (GFM, which Gitea uses). The case that prompted it:
// crates/README.md had a blank line inside its big table, so the 28 rows after it showed as plain text; other files had
// unescaped `|` inside code spans (an extra cell), short rows and rows glued to a paragraph. The rules are in util/md-tables.mjs.
// Reads `git ls-files -- '*.md'` in the checked root; not a git checkout: nothing to check. Fenced code is skipped.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { error } from '../finding.mjs';
import { tableFindings } from '../util/md-tables.mjs';

export default {
  id: 'C110',
  scope: 'root',
  mode: 'gate',
  title: 'Markdown tables render as tables',
  rule: 40,
  run(ctx) {
    const git = ctx.options.git ?? ((args) => execFileSync('git', args, { cwd: ctx.root, encoding: 'utf8', maxBuffer: 64 << 20 }));
    let listing;
    try {
      git(['rev-parse', '--show-toplevel']);
      listing = git(['ls-files', '-z', '--', '*.md']);
    } catch {
      return []; // not a git checkout (an export): nothing to check
    }
    const findings = [];
    for (const p of listing.split('\0').filter(Boolean)) {
      let text;
      try { text = readFileSync(join(ctx.root, p), 'utf8'); } catch { continue; } // deleted in the working tree, or not readable
      for (const f of tableFindings(text)) findings.push(error('C110', 40, p, f.line, `${p}:${f.line}: ${f.problem}`, f.fix));
    }
    return findings;
  },
};
