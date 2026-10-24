// C31: ADR numbers are unique (rule 40; problem B18: parallel branches each took "the next number"). Files in `docs/adr/`
// named `NNNN-title.md` (four digits) must not share a number, and a number is written the same way in every name.
import { error } from '../finding.mjs';

const ADR = /^docs\/adr\/(\d+)-[^/]+\.md$/;

export default {
  id: 'C31',
  title: 'ADR numbers are unique',
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
    return out;
  },
};
