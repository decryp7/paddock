// C98: a store's temporary opt-out of the read retry ends on time (ADR 0058, rule 30). In the shared UI
// (`ui/projects/paddock-ui`, specs skipped), a `withRequestStatus({ retry: false, kind: 'temporary', ... })` names `until`, the
// date (YYYY-MM-DD) by which the store adopts the retry; a missing date, or one before today, is an error. The unit spec
// `request-status-stores.spec.ts` checks the declaration itself; the calendar lives here, so the unit suite never turns red
// by date. Adopt the retry (`retry: true`), or move the date with a reason in the commit.
import { error } from '../finding.mjs';

const SOURCE = /^ui\/projects\/paddock-ui\/.*\.ts$/;
const SKIP = /\.spec\.ts$|(^|\/)(e2e|node_modules|dist)\//;

/** The temporary opt-outs of a file: [{ line, until }] (`until` null when missing or not YYYY-MM-DD). */
export function temporaryOptOuts(text) {
  const out = [];
  let at = text.indexOf('withRequestStatus(');
  while (at >= 0) {
    const end = text.indexOf('})', at);
    const call = text.slice(at, end < 0 ? undefined : end + 2);
    if (/kind:\s*'temporary'/.test(call)) {
      out.push({ line: text.slice(0, at).split('\n').length, until: /until:\s*'(\d{4}-\d{2}-\d{2})'/.exec(call)?.[1] ?? null });
    }
    at = text.indexOf('withRequestStatus(', at + 1);
  }
  return out;
}

export default {
  id: 'C98',
  title: 'temporary opt-outs of the read retry end on time',
  rule: 30,
  run(ctx) {
    const today = ctx.now.toISOString().slice(0, 10);
    const out = [];
    for (const file of ctx.files()) {
      if (!SOURCE.test(file) || SKIP.test(file)) continue;
      const text = ctx.read(file) ?? '';
      if (!text.includes('withRequestStatus(')) continue;
      for (const { line, until } of temporaryOptOuts(text)) {
        if (until === null) {
          out.push(error('C98', 30, file, line, 'a temporary opt-out of the read retry has no `until` date (YYYY-MM-DD)', "Add `until: 'YYYY-MM-DD'`, the date by which the store adopts `retry: true` (ADR 0058)."));
        } else if (until < today) {
          out.push(error('C98', 30, file, line, `the temporary opt-out of the read retry ran out: ${until} has passed`, 'Adopt the retry (`retry: true` and the declared read handle, ADR 0058), or move the date with the reason in the commit.'));
        }
      }
    }
    return out;
  },
};
