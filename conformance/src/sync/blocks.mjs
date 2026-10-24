// Shared doc blocks (plan X43, check C23 second part). A block is a section of Markdown that Paddock owns and an app
// copies between two markers:
//
//   <!-- paddock:block design-principles rev=1a2b3c4d -->
//   ## Design principles ...
//   <!-- /paddock:block design-principles -->
//
// `rev` is the first 8 hex digits of the SHA-256 of the source, so an outdated copy and a hand edit are told apart. The
// app keeps its own sections outside the markers. Pure functions over text; the check and `sync` do the file work.
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fill, normalise } from './fill.mjs';

const DOCS_URL = 'https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/';
const RULES_INTRO = `The family's binding rules, copied from [paddock/docs/rules.md](${DOCS_URL}rules.md) (the source; update there first). Where an older rule in this file disagrees, these win.`;

/** The `rules` block source from docs/rules.md: every section demoted one level, relative links made absolute. */
export function renderRules(rulesMd) {
  const lines = normalise(rulesMd).split('\n');
  const first = lines.findIndex((l) => l.startsWith('## '));
  if (first < 0) throw new Error('docs/rules.md has no "## " section');
  let fence = false;
  const body = lines.slice(first).map((l) => {
    if (l.startsWith('```')) fence = !fence;
    const line = fence ? l : l.replace(/^(#{2,}) /, '#$1 ');
    return line.replace(/\]\((?!https?:|#|mailto:)([^)\s]+)\)/g, (_, p) => `](${DOCS_URL}${p})`);
  });
  return `## Paddock rules (binding)\n\n${RULES_INTRO}\n\n${body.join('\n').replace(/\n+$/, '')}\n`;
}

/** The registry (docs/blocks/blocks.json) with every block's text loaded: [{ name, heading, whole, text }]. */
export function loadBlocks(paddock) {
  const dir = join(paddock, 'docs', 'blocks');
  const registry = JSON.parse(readFileSync(join(dir, 'blocks.json'), 'utf8'));
  const blocks = registry.blocks.map((b) => {
    const text = b.source === 'rules' ? renderRules(readFileSync(join(paddock, 'docs', 'rules.md'), 'utf8')) : normalise(readFileSync(join(dir, b.file), 'utf8'));
    return { name: b.name, heading: b.heading ?? null, whole: !!b.whole, text };
  });
  return { ...registry, blocks };
}

/** First 8 hex digits of the SHA-256 of a block's source text. */
export const revOf = (text) => createHash('sha256').update(text).digest('hex').slice(0, 8);

const begin = (name) => new RegExp(`^<!-- paddock:block ${name} rev=([0-9a-f]{8}) -->$`);
const endLine = (name) => `<!-- /paddock:block ${name} -->`;

/** The marked block `name` in `text`: { start, end (line indexes, markers included), rev, body } or null; `broken` when a marker is unmatched. */
export function findBlock(text, name) {
  const lines = text.split('\n');
  const re = begin(name);
  const start = lines.findIndex((l) => re.test(l));
  if (start < 0) return lines.some((l) => l.includes(`<!-- /paddock:block ${name} -->`)) ? { broken: true } : null;
  const end = lines.findIndex((l, i) => i > start && l === endLine(name));
  if (end < 0) return { broken: true };
  return { start, end, rev: re.exec(lines[start])[1], body: lines.slice(start + 1, end).join('\n') };
}

/** Index range [from, to) of the section that starts at the `heading` line (to the next heading of the same or a higher level; fences respected), or null. */
export function findSection(text, heading) {
  const lines = text.split('\n');
  const level = /^#+/.exec(heading)[0].length;
  const from = lines.findIndex((l) => l.trimEnd() === heading);
  if (from < 0) return null;
  let fence = false;
  let to = lines.length;
  for (let i = from + 1; i < lines.length; i++) {
    if (lines[i].startsWith('```')) fence = !fence;
    const m = !fence && /^(#+) /.exec(lines[i]);
    if (m && m[1].length <= level) {
      to = i;
      break;
    }
    if (!fence && lines[i].startsWith('<!-- paddock:block ')) {
      to = i;
      break;
    }
  }
  return { from, to };
}

const wrap = (block, config) => {
  const body = fill(block.text, config).replace(/\n+$/, '');
  return `<!-- paddock:block ${block.name} rev=${revOf(block.text)} -->\n${body}\n${endLine(block.name)}`;
};

/**
 * How `text` stands against `block`: { state: 'ok' | 'missing' | 'unmarked' | 'outdated' | 'edited' | 'broken', desired }.
 * `desired` is the whole file text with the block in place (what `sync --write` writes).
 */
export function inspectBlock(text, block, config) {
  const wanted = wrap(block, config);
  const body = fill(block.text, config).replace(/\n+$/, '');
  const found = findBlock(text, block.name);
  const lines = text.split('\n');
  if (found?.broken) return { state: 'broken', desired: text };
  if (found) {
    const state = found.body === body ? (found.rev === revOf(block.text) ? 'ok' : 'outdated') : found.rev === revOf(block.text) ? 'edited' : 'outdated';
    const out = [...lines.slice(0, found.start), ...wanted.split('\n'), ...lines.slice(found.end + 1)];
    return { state, desired: out.join('\n') };
  }
  if (block.whole) return { state: text.trim() ? 'unmarked' : 'missing', desired: `${wanted}\n` };
  const section = block.heading ? findSection(text, block.heading) : null;
  if (section) {
    let { from, to } = section;
    while (to > from && lines[to - 1].trim() === '') to--;
    const out = [...lines.slice(0, from), ...wanted.split('\n'), ...lines.slice(to)];
    return { state: 'unmarked', desired: out.join('\n') };
  }
  const base = text.replace(/\n+$/, '');
  return { state: 'missing', desired: `${base ? `${base}\n\n` : ''}${wanted}\n` };
}
