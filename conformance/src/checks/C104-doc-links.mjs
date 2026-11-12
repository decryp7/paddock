// C104: a relative Markdown link in the rules and docs points at a file that exists. Rules name scripts, hooks and checklists by
// link; a link to a file that was never built or was moved is a rule naming a mechanism that does not exist (ER-21, 2026-11-12).
// Reads docs/rules.md, AGENTS.md and docs/**/*.md under the checked root, and RULES.md of the engineering-rules repo when it is
// given (`options.rulesMd`) or sits beside the repository (`../engineering-rules/RULES.md`); links in RULES.md resolve against that
// repository's root. Skips links with a scheme, `#anchors`, absolute paths, fenced code and code spans. A folder target is fine.
// Dead links that existed when the check was added sit in `conformance/baselines/C104.json` ({file, target}); it may only shrink:
// a listed link that works or is gone is a warning to remove, and an entry the baseline at the merge base lacks is an error.
// Second part (named mechanisms): in RULES.md, docs/rules.md and AGENTS.md a backticked `./dev <cmd>` (the top-level commands of
// tools/paddock-dev/src/cli.rs), `.claude/hooks/*` or `scripts/*` path, and a C-number (a check file under conformance/src/checks,
// or C100 of the gate register) must exist, unless the same sentence says "planned". One error per miss; today's misses sit in the
// baseline's `mentions` list, shrink-only like `links`. `.claude/hooks/*` and `scripts/*` count when they exist in the checked root
// or in the engineering-rules repository (RULES.md's own).
// A root without docs/rules.md (an app folder) is not checked.
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, normalize, resolve } from 'node:path';
import { error, warn } from '../finding.mjs';

export const BASELINE_PATH = 'conformance/baselines/C104.json';
const EXTRA_CHECK_IDS = ['C100']; // lives in the gate register (ADR 0060), not in conformance/src/checks

/** Sentences of `text` as [{ text, start }] (offset of the first character): split at a full stop or semicolon before white space, and at blank lines. */
export function sentences(text) {
  const out = [];
  const re = /[.;:!?](?=\s)|\n\s*\n/g;
  let start = 0;
  for (const m of text.matchAll(re)) {
    out.push({ text: text.slice(start, m.index + 1), start });
    start = m.index + m[0].length;
  }
  out.push({ text: text.slice(start), start });
  return out;
}

/** Top-level `./dev` commands (lower case variant names of `enum Command`), or null when cli.rs cannot be read. */
export function devCommands(cliRs) {
  const m = /enum Command \{([\s\S]*?)\n\}/.exec(cliRs ?? '');
  if (!m) return null;
  return [...m[1].matchAll(/^ {4}([A-Z][A-Za-z_]*)\b/gm)].map((v) => v[1].toLowerCase().replace(/_/g, ''));
}

/** The mechanisms a text names that are not "planned" in the same sentence: [{ token, line, kind }]. */
export function namedMechanisms(text) {
  const out = [];
  const seen = new Set();
  for (const s of sentences(text)) {
    if (/\bplanned\b/i.test(s.text)) continue;
    const found = [];
    for (const m of s.text.matchAll(/`(\.\/dev [a-z][a-z-]*)[^`]*`/g)) found.push([m[1], 'dev', m.index]);
    for (const m of s.text.matchAll(/`((?:\.claude\/hooks|scripts)\/[^`\s*<>]+)`/g)) found.push([m[1], 'path', m.index]);
    for (const m of s.text.matchAll(/\bC(\d{2,3})\b/g)) found.push([`C${m[1]}`, 'check', m.index]);
    for (const [token, kind, at] of found) {
      if (seen.has(token)) continue;
      seen.add(token);
      out.push({ token, kind, line: text.slice(0, s.start + at).split('\n').length });
    }
  }
  return out;
}

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
    return { entries: null, mentions: null }; // the baseline does not exist at the base: its introduction, nothing was added
  }
  try {
    const j = JSON.parse(text);
    return { entries: j.links ?? [], mentions: j.mentions ?? [] };
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
    const cli = ctx.read('tools/paddock-dev/src/cli.rs');
    const commands = devCommands(cli);
    const checkIds = new Set([...EXTRA_CHECK_IDS, ...ctx.files().map((f) => /^conformance\/src\/checks\/(C\d+)-/.exec(f)?.[1]).filter(Boolean)]);
    const haveChecks = checkIds.size > EXTRA_CHECK_IDS.length;
    const rulesRepo = existsSync(rulesMd) ? dirname(rulesMd) : null;
    const present = (rel) => ctx.exists(rel) || (rulesRepo !== null && existsSync(join(rulesRepo, rel)));
    const missing = [];
    const named = [{ file: 'AGENTS.md', text: ctx.read('AGENTS.md') }, { file: 'docs/rules.md', text: ctx.read('docs/rules.md') }];
    if (rulesRepo) named.push({ file: 'RULES.md', text: safeRead(rulesMd) });
    for (const n of named) {
      if (n.text == null) continue;
      for (const m of namedMechanisms(n.text)) {
        const ok =
          m.kind === 'path' ? present(m.token)
          : m.kind === 'check' ? !haveChecks || checkIds.has(m.token)
          : commands === null || commands.some((c) => c.startsWith(m.token.slice(6).replace(/-/g, '')));
        if (!ok) missing.push({ file: n.file, target: m.token, line: m.line });
      }
    }
    let baseline = [];
    let baselineMentions = [];
    const raw = ctx.read(BASELINE_PATH);
    if (raw) {
      const j = JSON.parse(raw);
      baseline = j.links ?? [];
      baselineMentions = j.mentions ?? [];
    }
    const knownM = [...baselineMentions];
    const freshM = [];
    for (const d of missing) {
      const i = knownM.findIndex((b) => b.file === d.file && b.target === d.target);
      if (i >= 0) knownM.splice(i, 1);
      else freshM.push(d);
    }
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
    for (const d of freshM) {
      out.push(error('C104', 40, d.file, d.line, `${d.file}:${d.line}: \`${d.target}\` is named but does not exist on main and the sentence does not say "planned"`, 'Build it, correct the name, or write "planned" in the same sentence (the baseline only shrinks).'));
    }
    for (const b of knownM) {
      out.push(warn('C104', 40, b.file, 1, `${BASELINE_PATH} lists \`${b.target}\` as named but missing, and it now exists or is gone; the baseline may only shrink`, 'Remove the entry from the baseline.'));
    }
    for (const b of known) {
      out.push(warn('C104', 40, b.file, 1, `${BASELINE_PATH} lists a dead link to \`${b.target}\` that now works or is gone; the baseline may only shrink`, 'Remove the entry from the baseline.'));
    }
    if (baseline.length || baselineMentions.length) {
      const g = baselineAtBase(ctx);
      if (g.reason) out.push(warn('C104', 40, BASELINE_PATH, 1, `the baseline's shrink-only rule is not enforced here: ${g.reason}`, 'Fetch the target branch (`git fetch origin main`), or set PADDOCK_BASE_REF.'));
      else if (g.entries) for (const a of [...added(baseline, g.entries), ...added(baselineMentions, g.mentions)]) out.push(error('C104', 40, BASELINE_PATH, 1, `the baseline grew: \`${a.target}\` in ${a.file} is not in the baseline at the merge base`, 'Remove the entry and fix the text; growing the baseline needs the user\'s approval.'));
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
