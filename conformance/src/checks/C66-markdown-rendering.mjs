// C66: Markdown files render on Gitea (goldmark, GFM) the way they read. Findings come from util/markdown-lint.mjs: tables
// (a blank line or a non-pipe line inside, a cell count that differs from the header, an unescaped `|` in a cell, a table glued
// to a paragraph), unclosed code fences, broken relative links and same-file anchors, list continuations that lost their
// indent, unclosed HTML tags. The case that prompted it: a blank line split the table in crates/README.md. Generated files
// (THIRD_PARTY_NOTICES.md) and the golden fixtures of tools/wiki (copies of wiki trees, some broken on purpose) are skipped.
import { error } from '../finding.mjs';
import { lintMarkdown } from '../util/markdown-lint.mjs';

/** Files that come from a tool, and test fixtures that hold Markdown on purpose (tools/wiki's golden cases). */
export const SKIPPED = /(^|\/)THIRD_PARTY_NOTICES\.md$|^tools\/wiki\/tests\/golden\/cases\//;

/** Link targets (as `file -> targets`) that may be missing for good, with the reason. Temporary gaps are dated waivers in paddock.waivers.toml. */
export const ALLOWED_MISSING_LINKS = {};

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
