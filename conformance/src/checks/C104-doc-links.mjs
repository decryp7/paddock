// C104: a relative Markdown link in the rules and docs points at a file that exists. Rules name scripts, hooks and checklists by
// link; a link to a file that was never built or was moved is a rule naming a mechanism that does not exist (ER-21, 2026-11-12).
// Reads docs/rules.md, AGENTS.md and docs/**/*.md under the checked root, and RULES.md of the engineering-rules repo when it is
// given (`options.rulesMd`) or sits beside the repository (`../engineering-rules/RULES.md`); links in RULES.md resolve against that
// repository's root. Skips links with a scheme, `#anchors`, absolute paths, fenced code and code spans. A folder target is fine.
// Dead links that existed when the check was added sit in `conformance/baselines/C104.json` ({file, target}); it may only shrink:
// a listed link that works or is gone is a warning to remove, and an entry the baseline at the merge base lacks is an error.
// Second part (named mechanisms): in RULES.md, docs/rules.md and AGENTS.md a backticked `./dev <cmd>` (the top-level commands of
// tools/paddock-dev/src/cli.rs), `.claude/hooks/*` or `scripts/*` path, and a C-number (a check file under conformance/src/checks,
// or C100 of the gate register) must exist, unless its own clause says "planned". One error per miss; today's misses sit in the
// baseline's `mentions` list, shrink-only like `links`. `.claude/hooks/*` and `scripts/*` count when they exist in the checked root
// or in the engineering-rules repository (RULES.md's own).
// A root without docs/rules.md (an app folder) is not checked.
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, normalize, resolve } from 'node:path';
import { error, warn } from '../finding.mjs';
import { baselineGrowth } from '../util/baseline.mjs';

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

const kebab = (n) => n.replace(/([a-z0-9])([A-Z])/g, '$1-$2').toLowerCase();

/** Top-level `./dev` command names of `enum Command` (kebab-case variant names, or `#[command(name = "...")]`), or null when cli.rs cannot be read. */
export function devCommands(cliRs) {
  const m = /enum Command \{([\s\S]*?)\n\}/.exec(cliRs ?? '');
  if (!m) return null;
  const out = [];
  let name = null;
  for (const line of m[1].split('\n')) {
    const n = /^ {4}#\[command\([^\]]*\bname\s*=\s*"([^"]+)"/.exec(line);
    if (n) name = n[1];
    const v = /^ {4}([A-Z][A-Za-z0-9_]*)\b/.exec(line);
    if (v) {
      out.push(name ?? kebab(v[1]));
      name = null;
    }
  }
  return out;
}

/** True when "planned" stands in the clause of the mention at `at` (`len` long): since the last , ; ( ) before it, or the 40 characters after it up to a , ; or ). */
function plannedNear(sentence, at, len) {
  const before = sentence.slice(0, at);
  const cut = Math.max(before.lastIndexOf(','), before.lastIndexOf(';'), before.lastIndexOf('('), before.lastIndexOf(')'));
  if (/\bplanned\b/i.test(before.slice(cut + 1))) return true;
  const after = sentence.slice(at + len, at + len + 40).split(/[;),]/)[0];
  return /\bplanned\b/i.test(after);
}

/** The mechanisms a text names that are not "planned" in their own clause: [{ token, line, kind }]. */
export function namedMechanisms(text) {
  const out = [];
  const seen = new Set();
  for (const s of sentences(text)) {
    const found = [];
    for (const m of s.text.matchAll(/`(\.\/dev [a-z][a-z-]*)[^`]*`/g)) found.push([m[1], 'dev', m.index]);
    for (const m of s.text.matchAll(/`((?:\.claude\/hooks|scripts)\/[^`\s*<>]+)`/g)) found.push([m[1], 'path', m.index]);
    for (const m of s.text.matchAll(/\bC(\d{2,3})\b/g)) found.push([`C${m[1]}`, 'check', m.index]);
    for (const [token, kind, at] of found) {
      if (plannedNear(s.text, at, token.length)) continue;
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
    const rulesRead = existsSync(rulesMd) && safeRead(rulesMd) !== null;
    const rulesRepo = rulesRead ? dirname(rulesMd) : null;
    const present = (rel) => ctx.exists(rel) || (rulesRepo !== null && existsSync(join(rulesRepo, rel)));
    const missing = [];
    const named = [{ file: 'AGENTS.md', text: ctx.read('AGENTS.md') }, { file: 'docs/rules.md', text: ctx.read('docs/rules.md') }];
    if (rulesRead) named.push({ file: 'RULES.md', text: safeRead(rulesMd) });
    for (const n of named) {
      if (n.text == null) continue;
      for (const m of namedMechanisms(n.text)) {
        const ok =
          m.kind === 'path' ? present(m.token)
          : m.kind === 'check' ? !haveChecks || checkIds.has(m.token)
          : commands === null || commands.includes(m.token.slice(6));
        if (!ok) missing.push({ file: n.file, target: m.token, line: m.line });
      }
    }
    const out = [];
    // ER-33: RULES.md is a second repository; when it was not read, nothing was measured for it (CI has no copy beside the repo).
    const skipFile = rulesRead ? null : 'RULES.md';
    if (!rulesRead) {
      out.push(warn('C104', 40, 'RULES.md', 1, 'RULES.md: not measured (the file was not found or not readable, so its links and named mechanisms were not checked)', 'Check out engineering-rules beside this repository, or pass --rules-md <file>.'));
    }
    let baseline = { links: [], mentions: [] };
    const raw = ctx.read(BASELINE_PATH);
    if (raw) {
      const j = JSON.parse(raw);
      baseline = { links: j.links ?? [], mentions: j.mentions ?? [] };
    }
    const measured = (list) => list.filter((e) => e.file !== skipFile);
    // Matches today's findings against the baseline: returns { fresh, stale }.
    const split = (found, listed) => {
      const left = [...measured(listed)];
      const fresh = [];
      for (const d of found) {
        const i = left.findIndex((b) => b.file === d.file && b.target === d.target);
        if (i >= 0) left.splice(i, 1);
        else fresh.push(d);
      }
      return { fresh, stale: left };
    };
    const links = split(dead, baseline.links);
    const mentions = split(missing, baseline.mentions);
    for (const d of links.fresh) {
      out.push(error('C104', 40, d.file, d.line, `${d.file}:${d.line}: the link target \`${d.target}\` does not exist in this tree`, 'Fix the path, build the file the text names, or reword the sentence so it does not name a file that is not there (the baseline only shrinks).'));
    }
    for (const d of mentions.fresh) {
      out.push(error('C104', 40, d.file, d.line, `${d.file}:${d.line}: \`${d.target}\` is named but does not exist in this tree and its clause does not say "planned"`, 'Build it, correct the name, or write "planned" in the same clause (the baseline only shrinks).'));
    }
    for (const b of mentions.stale) {
      out.push(warn('C104', 40, b.file, 1, `${BASELINE_PATH} lists \`${b.target}\` as named but missing, and it now exists or is gone; the baseline may only shrink`, 'Remove the entry from the baseline.'));
    }
    for (const b of links.stale) {
      out.push(warn('C104', 40, b.file, 1, `${BASELINE_PATH} lists a dead link to \`${b.target}\` that now works or is gone; the baseline may only shrink`, 'Remove the entry from the baseline.'));
    }
    // Shrink-only against the merge base with the target branch (the shared rule, util/baseline.mjs), one list at a time.
    for (const [key, list] of [['links', measured(baseline.links)], ['mentions', measured(baseline.mentions)]]) {
      if (!list.length) continue;
      const g = baselineGrowth({
        root: ctx.root,
        path: BASELINE_PATH,
        current: list,
        keyOf: (e) => `${e.file}\0${e.target}`,
        entriesOf: (j) => j[key] ?? [],
        ...(ctx.options.git ? { git: ctx.options.git } : {}),
        ...(ctx.options.baseRef ? { baseRef: ctx.options.baseRef } : {}),
      });
      if (g.reason) out.push(warn('C104', 40, BASELINE_PATH, 1, `the baseline's shrink-only rule is not enforced here: ${g.reason}`, 'Fetch the target branch (`git fetch origin main`), or set PADDOCK_BASE_REF.'));
      else for (const a of g.added) out.push(error('C104', 40, BASELINE_PATH, 1, `the baseline grew: \`${a.target}\` in ${a.file} is not in the baseline at the merge base ${g.base.slice(0, 9)}`, 'Remove the entry and fix the text; growing the baseline needs the user\'s approval.'));
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
