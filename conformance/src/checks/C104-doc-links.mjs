// C104: a relative Markdown link in the rules and docs points at a file that exists. Rules name scripts, hooks and checklists by
// link; a link to a file that was never built or was moved is a rule naming a mechanism that does not exist (ER-21, 2026-11-12).
// Reads docs/rules.md, AGENTS.md and docs/**/*.md under the checked root, and RULES.md of the engineering-rules repo when it is
// given (`options.rulesMd`) or sits beside the repository (`../engineering-rules/RULES.md`); links in RULES.md resolve against that
// repository's root. Skips links with a scheme, `#anchors`, absolute paths, fenced code and code spans. A folder target is fine.
// Dead links that existed when the check was added sit in `conformance/baselines/C104.json` ({file, target}); it may only shrink:
// a listed link that works or is gone is a warning to remove, and an entry the baseline at the merge base lacks is an error.
// A root without docs/rules.md (an app folder) is not checked.
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, normalize, resolve } from 'node:path';
import { error, warn } from '../finding.mjs';

export const BASELINE_PATH = 'conformance/baselines/C104.json';

/** `text` with fenced blocks and code spans blanked (same length, newlines kept). */
export function maskCode(text) {
  const blank = (s) => s.replace(/[^\n]/g, ' ');
  return text
    .replace(/^(```|~~~)[^\n]*\n[\s\S]*?^\1[^\n]*$/gm, blank)
    .replace(/`[^`\n]+`/g, blank);
}

/** The relative link targets of a Markdown text: [{ target, line }] (fragment and query removed). */
export function relativeLinks(text) {
  const out = [];
  const masked = maskCode(text);
  const re = /\[[^\]\n]*\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)/g;
  for (const m of masked.matchAll(re)) {
    let t = m[1];
    if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(t) || t.startsWith('#') || t.startsWith('/') || t.startsWith('//')) continue;
    t = t.replace(/[#?].*$/, '');
    if (!t) continue;
    try {
      t = decodeURIComponent(t);
    } catch {
      /* keep as written */
    }
    out.push({ target: t, line: masked.slice(0, m.index).split('\n').length });
  }
  return out;
}

/** Entries of `current` ({file, target}) that `base` lacks (multiset difference). */
export function added(current, base) {
  const left = new Map();
  for (const b of base) left.set(`${b.file}\0${b.target}`, (left.get(`${b.file}\0${b.target}`) ?? 0) + 1);
  const out = [];
  for (const c of current) {
    const k = `${c.file}\0${c.target}`;
    if ((left.get(k) ?? 0) > 0) left.set(k, left.get(k) - 1);
    else out.push(c);
  }
  return out;
}

function baselineAtBase(ctx) {
  const git = ctx.options.git ?? ((args) => execFileSync('git', args, { cwd: ctx.root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }));
  const base = ctx.options.baseRef ?? process.env.PADDOCK_BASE_REF ?? 'origin/main';
  try {
    git(['rev-parse', '--verify', `${base}^{commit}`]);
  } catch {
    return { reason: `${base} is not available` };
  }
  let text;
  try {
    text = git(['show', `${base}:${BASELINE_PATH}`]);
  } catch {
    return { entries: null }; // the baseline does not exist at the base: its introduction, nothing was added
  }
  try {
    return { entries: JSON.parse(text).links ?? [] };
  } catch {
    return { reason: `the baseline at ${base} cannot be read` };
  }
}

export default {
  id: 'C104',
  scope: 'root',
  mode: 'gate',
  title: 'Relative links in rules and docs resolve',
  rule: 40,
  run(ctx) {
    if (!ctx.exists('docs/rules.md')) return [];
    const docs = ctx.files().filter((f) => f === 'AGENTS.md' || f === 'docs/rules.md' || (f.startsWith('docs/') && f.endsWith('.md')));
    const sources = docs.map((file) => ({ file, text: ctx.read(file), exists: (rel) => ctx.exists(rel), base: dirname(file) }));
    const rulesMd = ctx.options.rulesMd ?? resolve(ctx.root, '..', 'engineering-rules', 'RULES.md');
    if (existsSync(rulesMd)) {
      const repo = dirname(rulesMd);
      sources.push({ file: 'RULES.md', abs: rulesMd, repo });
    }
    const dead = [];
    for (const s of sources) {
      let text = s.text;
      if (s.abs) text = safeRead(s.abs);
      if (text == null) continue;
      for (const l of relativeLinks(text)) {
        const ok = s.abs ? existsSync(join(s.repo, normalize(l.target))) : s.exists(normalize(join(s.base, l.target)));
        if (!ok) dead.push({ file: s.file, target: l.target, line: l.line });
      }
    }
    let baseline = [];
    const raw = ctx.read(BASELINE_PATH);
    if (raw) baseline = JSON.parse(raw).links ?? [];
    const known = [...baseline];
    const fresh = [];
    for (const d of dead) {
      const i = known.findIndex((b) => b.file === d.file && b.target === d.target);
      if (i >= 0) known.splice(i, 1);
      else fresh.push(d);
    }
    const out = fresh.map((d) =>
      error('C104', 40, d.file, d.line, `${d.file}:${d.line}: the link target \`${d.target}\` does not exist`, 'Fix the path, build the file the text names, or reword the sentence so it does not name a mechanism that is not there (the baseline only shrinks).'),
    );
    for (const b of known) {
      out.push(warn('C104', 40, b.file, 1, `${BASELINE_PATH} lists a dead link to \`${b.target}\` that now works or is gone; the baseline may only shrink`, 'Remove the entry from the baseline.'));
    }
    if (baseline.length) {
      const g = baselineAtBase(ctx);
      if (g.reason) out.push(warn('C104', 40, BASELINE_PATH, 1, `the baseline's shrink-only rule is not enforced here: ${g.reason}`, 'Fetch the target branch (`git fetch origin main`), or set PADDOCK_BASE_REF.'));
      else if (g.entries) for (const a of added(baseline, g.entries)) out.push(error('C104', 40, BASELINE_PATH, 1, `the baseline grew: \`${a.target}\` in ${a.file} is not in the baseline at the merge base`, 'Remove the entry and fix the link; growing the baseline needs the user\'s approval.'));
    }
    return out;
  },
};

function safeRead(p) {
  try {
    return readFileSync(p, 'utf8');
  } catch {
    return null;
  }
}
