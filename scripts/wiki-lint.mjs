#!/usr/bin/env node
// The mechanical lint of the project wiki (docs/wiki, the LLM Wiki pilot; see docs/wiki/README.md). Cheap and exact, so it runs on
// every wiki change; the contradiction check against rules and ADRs is a bounded reading pass by a reviewer, not this script.
//
// Errors (exit 1):
//   - a relative link that does not resolve (file, or the heading anchor in a Markdown target)
//   - a page that index.md does not list; a page with no inbound link from another page (orphan)
//   - a page without `verified: YYYY-MM-DD` (a real date, not in the future) or without a non-empty `sources:`
//   - a bullet that names no source (a Markdown link or `src:`)
//   - a log.md line that does not follow `## [YYYY-MM-DD] ingest|lint|fix | subject`
//   - rule text copied: a run of COPY_WORDS or more words that docs/rules.md also contains
// Warnings (exit 0, or 1 with --strict): a page not verified within --max-age days (default 30).
//
// Usage: node scripts/wiki-lint.mjs [--wiki docs/wiki] [--rules docs/rules.md] [--max-age 30] [--today YYYY-MM-DD] [--strict]
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** A copied run of this many words is rule text (the pilot's rule: no sentence of 8 or more words shared with rules.md). */
export const COPY_WORDS = 8;
/** Pages not verified within this many days are flagged. */
export const MAX_AGE_DAYS = 30;
/** Files of the wiki folder that are not topic pages. */
const SPECIAL = new Set(['README.md', 'index.md', 'log.md']);
const LOG_LINE = /^## \[\d{4}-\d{2}-\d{2}\] (ingest|lint|fix) \| .+/;

const DAY_MS = 86_400_000;

/** `{ meta, body, bodyStart }`: the front matter of a page (`verified`, `sources`) and the text after it. */
export function parsePage(text) {
  const m = /^---\n([\s\S]*?)\n---\n?/.exec(text);
  if (!m) return { meta: {}, body: text, bodyStart: 0 };
  const meta = {};
  const lines = m[1].split('\n');
  for (let i = 0; i < lines.length; i++) {
    const kv = /^([a-z_]+):\s*(.*)$/.exec(lines[i]);
    if (!kv) continue;
    const [, key, rest] = kv;
    if (rest.startsWith('[')) {
      meta[key] = rest.replace(/^\[|\]$/g, '').split(',').map((s) => s.trim()).filter(Boolean);
    } else if (rest === '') {
      const items = [];
      while (i + 1 < lines.length && /^\s+-\s+/.test(lines[i + 1])) items.push(lines[++i].replace(/^\s+-\s+/, '').trim());
      meta[key] = items;
    } else {
      meta[key] = rest.trim();
    }
  }
  return { meta, body: text.slice(m[0].length), bodyStart: m[0].length };
}

/** The anchor Gitea/GitHub give a heading. */
export const slug = (heading) =>
  heading
    .trim()
    .toLowerCase()
    .replace(/[`*_~]/g, '')
    .replace(/[^\p{L}\p{N}\s-]/gu, '')
    .replace(/\s/g, '-');

const headingsOf = (text) => [...text.matchAll(/^#{1,6}\s+(.+?)\s*#*$/gm)].map((m) => slug(m[1]));

/** Relative link targets of a Markdown text: `[{ target, anchor, line }]`, skipping code and absolute URLs. */
export function linksOf(text) {
  const out = [];
  const lines = text.split('\n');
  let fenced = false;
  lines.forEach((raw, i) => {
    if (/^\s*```/.test(raw)) fenced = !fenced;
    if (fenced) return;
    const line = raw.replace(/`[^`]*`/g, '');
    for (const m of line.matchAll(/\[[^\]]*\]\(([^)\s]+)\)/g)) {
      const href = m[1];
      if (/^([a-z][a-z0-9+.-]*:|#)/i.test(href)) continue;
      const [target, anchor] = href.split('#');
      out.push({ target, anchor: anchor ?? '', line: i + 1 });
    }
  });
  return out;
}

const words = (text) =>
  text
    .toLowerCase()
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/[`*_>#|]/g, ' ')
    .split(/[^\p{L}\p{N}'.-]+/u)
    .map((w) => w.replace(/^[.'-]+|[.'-]+$/g, ''))
    .filter(Boolean);

/** The set of every COPY_WORDS-word window of `text` (joined by a space). */
export function windows(text, n = COPY_WORDS) {
  const w = words(text);
  const set = new Set();
  for (let i = 0; i + n <= w.length; i++) set.add(w.slice(i, i + n).join(' '));
  return set;
}

/** Days from `a` to `b` (both `YYYY-MM-DD`). */
const daysBetween = (a, b) => Math.round((Date.parse(b) - Date.parse(a)) / DAY_MS);
const isDate = (s) => /^\d{4}-\d{2}-\d{2}$/.test(s ?? '') && !Number.isNaN(Date.parse(s)) && new Date(s).toISOString().startsWith(s);

/**
 * Lints a wiki folder. `rulesFile` is the text source of the copied-rule check (may be absent). Returns `{ errors, warnings }`,
 * each a list of `path:line message` strings.
 */
export function lintWiki({ wikiDir, rulesFile, today, maxAge = MAX_AGE_DAYS }) {
  const errors = [];
  const warnings = [];
  const root = resolve(wikiDir);
  const names = readdirSync(root).filter((f) => f.endsWith('.md'));
  const pages = names.filter((f) => !SPECIAL.has(f));
  const text = Object.fromEntries(names.map((f) => [f, readFileSync(join(root, f), 'utf8')]));
  const at = (f, line, msg) => `${relative(process.cwd(), join(root, f)) || f}${line ? `:${line}` : ''}: ${msg}`;

  // Links resolve; inbound links per page (from other wiki files, the index and the log not counted for the orphan check).
  const inbound = Object.fromEntries(pages.map((p) => [p, new Set()]));
  const indexed = new Set();
  for (const f of names) {
    const { body, bodyStart } = parsePage(text[f]);
    const shift = text[f].slice(0, bodyStart).split('\n').length - 1;
    for (const { target, anchor, line } of linksOf(body)) {
      const path = resolve(root, dirname(f), target);
      if (!target) continue;
      if (!existsSync(path)) {
        errors.push(at(f, line + shift, `broken link: ${target} does not exist`));
        continue;
      }
      if (anchor && path.endsWith('.md') && !headingsOf(readFileSync(path, 'utf8')).includes(anchor)) {
        errors.push(at(f, line + shift, `broken link: ${target} has no heading #${anchor}`));
      }
      const hit = relative(root, path);
      if (f === 'index.md' && inbound[hit]) indexed.add(hit);
      if (inbound[hit] && hit !== f && f !== 'index.md' && f !== 'log.md') inbound[hit].add(f);
    }
  }
  if (!text['index.md']) errors.push(at('index.md', 0, 'missing index.md'));
  if (!text['log.md']) errors.push(at('log.md', 0, 'missing log.md'));

  for (const p of pages) {
    if (!indexed.has(p)) errors.push(at(p, 0, 'not listed in index.md'));
    if (inbound[p].size === 0) errors.push(at(p, 0, 'orphan: no other page links to it'));
    const { meta, body, bodyStart } = parsePage(text[p]);
    if (!isDate(meta.verified)) {
      errors.push(at(p, 1, 'front matter needs `verified: YYYY-MM-DD` (a real date)'));
    } else {
      const age = daysBetween(meta.verified, today);
      if (age < 0) errors.push(at(p, 1, `verified ${meta.verified} is in the future`));
      else if (age > maxAge) warnings.push(at(p, 1, `stale: verified ${meta.verified}, ${age} days ago (limit ${maxAge})`));
    }
    if (!Array.isArray(meta.sources) || meta.sources.length === 0) errors.push(at(p, 1, 'front matter needs a non-empty `sources:` list'));
    // A claim without a source is not written: every bullet carries a link or an explicit `src:`.
    const shift = text[p].slice(0, bodyStart).split('\n').length - 1;
    // A bullet is its first line plus the indented lines that continue it.
    let fenced = false;
    let bullet = null;
    const close = () => {
      if (bullet && !/\]\([^)]+\)|\bsrc:/.test(bullet.text)) errors.push(at(p, bullet.line, 'bullet names no source (add a link or `src: ...`)'));
      bullet = null;
    };
    body.split('\n').forEach((raw, i) => {
      if (/^\s*```/.test(raw)) fenced = !fenced;
      if (fenced) return;
      if (/^\s*[-*]\s+\S/.test(raw)) {
        close();
        bullet = { line: i + 1 + shift, text: raw };
      } else if (bullet && /^\s+\S/.test(raw)) bullet.text += ` ${raw}`;
      else close();
    });
    close();
  }

  // The log: one dated line per change, in the fixed shape.
  (text['log.md'] ?? '').split('\n').forEach((raw, i) => {
    if (raw.startsWith('## ') && !LOG_LINE.test(raw)) errors.push(at('log.md', i + 1, 'log line must be `## [YYYY-MM-DD] ingest|lint|fix | subject`'));
  });

  // No rule text: the wiki links to rules and never restates them.
  if (rulesFile && existsSync(rulesFile)) {
    const rules = windows(readFileSync(rulesFile, 'utf8'));
    for (const f of names) {
      // Over the whole body, so a sentence wrapped across lines is still found.
      const hit = [...windows(parsePage(text[f]).body)].find((w) => rules.has(w));
      if (hit) errors.push(at(f, 0, `copies rule text (${COPY_WORDS}+ words shared with ${relative(process.cwd(), rulesFile)}): "${hit}..."; link the rule instead`));
    }
  }
  return { errors, warnings };
}

/** The CLI. Returns the exit code. */
export function main(argv, { out = console.log, err = console.error } = {}) {
  const opt = (name, fallback) => (argv.includes(name) ? argv[argv.indexOf(name) + 1] : fallback);
  const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..');
  const wikiDir = resolve(opt('--wiki', join(repo, 'docs/wiki')));
  const rulesFile = resolve(opt('--rules', join(repo, 'docs/rules.md')));
  const today = opt('--today', new Date().toISOString().slice(0, 10));
  const { errors, warnings } = lintWiki({ wikiDir, rulesFile, today, maxAge: Number(opt('--max-age', MAX_AGE_DAYS)) });
  for (const w of warnings) out(`warning ${w}`);
  for (const e of errors) err(`error ${e}`);
  out(`wiki-lint: ${errors.length} error(s), ${warnings.length} warning(s)`);
  return errors.length > 0 || (argv.includes('--strict') && warnings.length > 0) ? 1 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = main(process.argv.slice(2));
