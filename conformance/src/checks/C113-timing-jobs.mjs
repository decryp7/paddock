// C113: a job that gates on time never shares a runner with a heavy job (ER-12, rule 5, ER-9; debrief.yml 8007 measured the macOS
// start-up benchmark at 1.8x its budgets beside the LTO bundle job). Timing jobs run real-app start-up budgets, benchmarks and the
// sidecar start checks; heavy jobs are the bundle, installer, gates and Rust test builds. SCOPE: one workflow file at a time; jobs of different files (debrief-bench.yml beside debrief.yml) are not compared. In one file, every timing job
// must be ordered with every heavy job that can run on a runner class it also uses: the timing job `needs:` the heavy one
// (through any chain), or the heavy one needs it. Otherwise the two start together and the budget measures the load.
//
// The declaration (a comment, so it works on a `uses:` job too, which has no `env:`), inside the job, before the job's `runs-on:`
// or `uses:` line or anywhere in its body:
//     # paddock-timing-job: <classes> — <why>        classes: any, or a list of linux, windows, macos (e.g. `macos`)
//     # paddock-timing-overlap: <reason>              the overlap is accepted for now (a platform limit), with its reason
//     # paddock-heavy-job                             marks a heavy job whose id is not one of the heavy names
// Heavy by id: bundle, installer(s)[-x], gates, build[-x], rust-lint, rust-test[-x], ci. A job that calls a reusable workflow
// has no `runs-on`: its classes are linux, windows and macos. Two jobs that cannot run in the same run are not paired: one whose
// `if` is only for a tag (no `||`) and one whose `if` is only for a non-tag.
// Reads the job blocks of the line-based workflow reader (context.workflows()); it reads only comments, `runs-on`, `needs`,
// `if` and `os: [..]`, so it works the same on the shared YAML reader when that lands.
import { error } from '../finding.mjs';

const CLASSES = ['linux', 'windows', 'macos'];
const HEAVY_ID = /^(?:bundle|installers?(?:-.+)?|gates|build(?:-.+)?|rust-lint|rust-test(?:-.+)?|ci)$/;
const TIMING = /^\s*#\s*paddock-timing-job:\s*([a-z, ]+?)\s*[—–-]+\s*(\S.{2,})$/m;
const OVERLAP = /^\s*#\s*paddock-timing-overlap:\s*(\S.{2,})$/m;
const HEAVY_MARK = /^\s*#\s*paddock-heavy-job\b/m;

const classOf = (runner) => {
  const r = runner.toLowerCase();
  return r.includes('mac') ? 'macos' : r.includes('win') ? 'windows' : r.includes('ubuntu') || r.includes('linux') ? 'linux' : null;
};

/** The body of a job without the comment and blank lines that trail it (they belong to the next job's header). */
function body(text) {
  const lines = text.split('\n');
  while (lines.length && /^\s*(#.*)?$/.test(lines[lines.length - 1])) lines.pop();
  return lines.join('\n');
}

/** The runner classes a job uses: from `runs-on`, expanded through a `matrix.os` list; a reusable-workflow call uses all. */
export function runnerClasses(text) {
  const on = /^\s{4}runs-on:\s*(.+?)\s*$/m.exec(text)?.[1];
  if (!on) return new Set(CLASSES);
  if (/matrix\.os/.test(on)) {
    const list = /^\s+os:\s*\[([^\]]*)\]/m.exec(text)?.[1];
    if (!list) return new Set(CLASSES);
    return new Set(list.split(',').map((s) => classOf(s)).filter(Boolean));
  }
  return new Set([classOf(on)].filter(Boolean));
}

const ifOf = (text) => /^\s{4}if:\s*(.+?)\s*$/m.exec(text)?.[1] ?? '';
const TAG = /startsWith\(\s*github\.ref\s*,\s*'refs\/tags\//;
/** 'tag' when the job runs only on a tag, 'branch' when only off one, '' when either. */
function side(ifText) {
  if (!TAG.test(ifText) || ifText.includes('||')) return '';
  return /!\s*startsWith\(\s*github\.ref\s*,\s*'refs\/tags\//.test(ifText) ? 'branch' : 'tag';
}

/** Findings of one workflow file: [{ line, job, heavy, classes }] for each unordered timing/heavy pair. */
export function unorderedPairs(jobs) {
  const byId = new Map(jobs.map((j) => [j.id, { ...j, text: body(j.text) }]));
  const ancestors = (id, seen = new Set()) => {
    for (const n of byId.get(id)?.needs ?? []) if (!seen.has(n)) { seen.add(n); ancestors(n, seen); }
    return seen;
  };
  const out = [];
  for (const t of byId.values()) {
    const decl = TIMING.exec(t.text);
    if (!decl || OVERLAP.test(t.text)) continue;
    const want = decl[1].trim() === 'any' ? new Set(CLASSES) : new Set(decl[1].split(/[,\s]+/).filter((c) => CLASSES.includes(c)));
    const mine = new Set([...runnerClasses(t.text)].filter((c) => want.has(c)));
    for (const h of byId.values()) {
      if (h.id === t.id || !(HEAVY_ID.test(h.id) || HEAVY_MARK.test(h.text))) continue;
      const shared = [...runnerClasses(h.text)].filter((c) => mine.has(c));
      if (!shared.length) continue;
      if (ancestors(t.id).has(h.id) || ancestors(h.id).has(t.id)) continue;
      const [a, b] = [side(ifOf(t.text)), side(ifOf(h.text))];
      if (a && b && a !== b) continue;
      out.push({ line: t.line, job: t.id, heavy: h.id, classes: shared });
    }
  }
  return out;
}

export default {
  id: 'C113',
  scope: 'both',
  mode: 'gate',
  title: 'Timing jobs never overlap heavy jobs on a runner class',
  rule: 12,
  run(ctx) {
    const out = [];
    for (const w of ctx.workflows()) {
      for (const j of w.jobs) {
        if (/^\s*#\s*paddock-timing-job:/m.test(body(j.text)) && !TIMING.test(body(j.text))) {
          out.push(error('C113', 12, w.file, j.line, `job \`${j.id}\`: the paddock-timing-job declaration needs classes and a reason`, 'Write `# paddock-timing-job: macos — <why this job gates on time>` (classes: any, or linux, windows, macos).'));
        }
      }
      for (const p of unorderedPairs(w.jobs)) {
        out.push(
          error(
            'C113',
            12,
            w.file,
            p.line,
            `timing job \`${p.job}\` is not ordered with heavy job \`${p.heavy}\` (both run on ${p.classes.join(', ')}): its budget would measure the other job's load`,
            `Add \`${p.heavy}\` to the needs of \`${p.job}\` (with \`if: \${{ !cancelled() && ... }}\` so a failed heavy job does not skip it), or the reverse. If a platform limit forbids it, say so with \`# paddock-timing-overlap: <reason>\` in the job.`,
          ),
        );
      }
    }
    return out;
  },
};
