// C31: ADR and rule numbers are unique (rule 40; problem B18: parallel branches each took "the next number"). Files in `docs/adr/`
// named `NNNN-title.md` (four digits) must not share a number, and a number is written the same way in every name.
import { error, warn } from '../finding.mjs';

const ADR = /^docs\/adr\/(\d+)-[^/]+\.md$/;

export default {
  id: 'C31',
  scope: 'root',
  mode: 'gate',
  title: 'ADR and rule numbers are unique',
  rule: 40,
  run(ctx) {
    const byNumber = new Map();
    const out = [];
    for (const f of ctx.files()) {
      const m = ADR.exec(f);
      if (!m) continue;
      if (m[1].length !== 4) out.push(error('C31', 40, f, 1, `the number ${m[1]} is not four digits`, 'Name ADRs NNNN-title.md with a four-digit number.'));
      const n = Number(m[1]);
      byNumber.set(n, [...(byNumber.get(n) ?? []), f]);
    }
    for (const [n, files] of [...byNumber].sort((a, b) => a[0] - b[0])) {
      if (files.length > 1) {
        for (const f of files) out.push(error('C31', 40, f, 1, `ADR number ${String(n).padStart(4, '0')} is used by ${files.length} files: ${files.join(', ')}`, 'Renumber the later one to the next free number (merge the branches first, then take the number).'));
      }
    }
    out.push(...ruleNumbers(ctx));
    return out;
  },
};

// Rules in docs/rules.md: `N. **Title.**` lines. A number is used once. A branch writes a new rule as `NEW-<slug>. **...` and the
// train integrator numbers it at landing (user, 2026-11-25), so a placeholder is a warning here, never an error.
const RULE = /^(\d+)\. \*\*/;
const PLACEHOLDER = /^NEW-[a-z0-9-]+\. \*\*/;

function ruleNumbers(ctx) {
  if (!ctx.exists('docs/rules.md')) return [];
  const out = [];
  const at = new Map();
  ctx.read('docs/rules.md').split('\n').forEach((l, i) => {
    const m = RULE.exec(l);
    if (m) at.set(Number(m[1]), [...(at.get(Number(m[1])) ?? []), i + 1]);
    if (PLACEHOLDER.test(l)) out.push(warn('C31', 40, 'docs/rules.md', i + 1, 'a NEW- rule placeholder is on the tree', 'The train integrator assigns the next free number at landing and replaces every `rule NEW-<slug>` citation; it must not reach main.'));
  });
  for (const [n, ls] of [...at].sort((a, b) => a[0] - b[0])) {
    if (ls.length > 1) out.push(error('C31', 40, 'docs/rules.md', ls[1], `rule number ${n} is used ${ls.length} times (lines ${ls.join(', ')})`, 'Write a new rule as `NEW-<slug>.`; the integrator numbers it at landing.'));
  }
  return out;
}
