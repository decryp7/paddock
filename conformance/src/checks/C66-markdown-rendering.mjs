// C66: Markdown files render on Gitea (goldmark, GFM) the way they read. Findings come from util/markdown-lint.mjs: tables
// (a blank line or a non-pipe line inside, a cell count that differs from the header, an unescaped `|` in a cell, a table glued
// to a paragraph), unclosed code fences, broken relative links and same-file anchors, list continuations that lost their
// indent, unclosed HTML tags. The case that prompted it: a blank line split the table in crates/README.md. Generated files
// (THIRD_PARTY_NOTICES.md) are skipped.
import { error } from '../finding.mjs';
import { lintMarkdown } from '../util/markdown-lint.mjs';

/** Files that come from a tool. */
export const SKIPPED = /(^|\/)THIRD_PARTY_NOTICES\.md$/;

/** Link targets (as `file -> target`) that may be missing for now, with the reason. Empty is the goal. */
export const ALLOWED_MISSING_LINKS = {
  // the note lands with branch docs/embeddings-for-logs; delete this entry when it is on main
  'docs/plans/shared-next.md': ['docs/research/2026-10-embeddings-for-logs.md'],
};

export default {
  id: 'C66',
  title: 'Markdown renders as written',
  rule: 40,
  run(ctx) {
    const findings = [];
    for (const file of ctx.files().filter((f) => f.endsWith('.md') && !SKIPPED.test(f))) {
      const allowed = ALLOWED_MISSING_LINKS[file] ?? [];
      const exists = (p) => ctx.exists(p) || ctx.tree.isDir(p) || allowed.includes(p);
      for (const p of lintMarkdown(ctx.read(file) ?? '', { file, exists }))
        findings.push(error('C66', 40, file, p.line, `${p.kind}: ${p.message}`, 'Fix the Markdown so Gitea renders it as written (tables: no blank line, escape | as \\|, one blank line before; links: point at a file that exists).'));
    }
    return findings;
  },
};
