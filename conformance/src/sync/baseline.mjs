// The repo baseline (plan X27, check C28): config files every app repo carries, kept equal to templates/repo/.
// templates/repo/manifest.json lists them with a mode:
//   exact    the file equals the template (placeholders filled)
//   sections only the named TOML tables equal the template's (an app keeps its own [bans], [workspace.members], ...)
//   extends  a JSON file whose `extends` contains the template's first entry (an app adds its own rules)
// Paths starting with `app/` follow `appDir` of paddock.conformance.json. Pure functions over text.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fill, normalise } from './fill.mjs';

/** The manifest: { files: [{ path, source?, mode, sections?, optional? }] }. */
export function loadManifest(paddock) {
  const dir = join(paddock, 'templates', 'repo');
  const manifest = JSON.parse(readFileSync(join(dir, 'manifest.json'), 'utf8'));
  return manifest.files.map((f) => ({ source: f.path, mode: 'exact', ...f, text: readFileSync(join(dir, f.source ?? f.path), 'utf8') }));
}

/** The path in the app repo for a manifest path (`app/` follows appDir). */
export function targetPath(path, config) {
  return path.startsWith('app/') ? `${config.appDir}/${path.slice(4)}` : path;
}

/** Splits TOML text into { head, tables: [{ header, lines }] }; a table runs to the next line that starts with `[`. */
export function tomlTables(text) {
  const lines = text.split('\n');
  const tables = [];
  let cur = { header: null, lines: [] };
  const head = cur;
  for (const l of lines) {
    if (/^\[[^\]]+\]\s*(#.*)?$/.test(l) || /^\[\[[^\]]+\]\]\s*(#.*)?$/.test(l)) {
      cur = { header: l.replace(/\s*#.*$/, '').trim(), lines: [l] };
      tables.push(cur);
    } else cur.lines.push(l);
  }
  return { head, tables };
}

const headerOf = (name) => `[${name}]`;
const trimTable = (lines) => {
  const out = [...lines];
  while (out.length && out[out.length - 1].trim() === '') out.pop();
  return out.join('\n');
};

/** A table's text (header to the last non-blank line), or null. */
function tableWithComments(text, name) {
  const { tables } = tomlTables(text);
  const t = tables.find((x) => x.header === headerOf(name));
  if (!t) return null;
  return trimTable(t.lines);
}

/** Replaces or appends the named tables of `source` in `current`. */
export function applySections(current, source, sections) {
  let out = current.replace(/\n+$/, '');
  for (const name of sections) {
    const want = tableWithComments(source, name);
    if (want === null) continue;
    const { tables } = tomlTables(out);
    const have = tables.find((x) => x.header === headerOf(name));
    if (have) {
      const old = trimTable(have.lines);
      out = out.replace(old, want);
    } else out = `${out}\n\n${want}`;
  }
  return `${out}\n`;
}

/** Does `current` have every named table equal to the source's? Returns the names that differ. */
export function sectionDiffs(current, source, sections) {
  const bad = [];
  for (const name of sections) {
    const want = tableWithComments(source, name);
    if (want === null) continue;
    const have = tableWithComments(current, name);
    if (have === null || normalise(have) !== normalise(want)) bad.push(name);
  }
  return bad;
}

/**
 * Inspects one manifest entry against the app's file text (null when absent).
 * Returns { state: 'ok' | 'missing' | 'differs', desired, detail }.
 */
export function inspectFile(entry, current, config) {
  const want = fill(entry.text, config);
  if (entry.mode === 'exact') {
    if (current === null) return { state: 'missing', desired: want, detail: 'the file is missing' };
    return normalise(current) === normalise(want)
      ? { state: 'ok', desired: current }
      : { state: 'differs', desired: want, detail: 'it differs from the template' };
  }
  if (entry.mode === 'sections') {
    if (current === null) return { state: 'missing', desired: want, detail: 'the file is missing' };
    const bad = sectionDiffs(current, want, entry.sections);
    return bad.length
      ? { state: 'differs', desired: applySections(current, want, entry.sections), detail: `table(s) ${bad.map(headerOf).join(', ')} differ from the template` }
      : { state: 'ok', desired: current };
  }
  if (entry.mode === 'extends') {
    const preset = JSON.parse(want).extends?.[0];
    if (current === null) return { state: 'missing', desired: want, detail: 'the file is missing' };
    let json;
    try {
      json = JSON.parse(current);
    } catch (e) {
      return { state: 'differs', desired: want, detail: `it is not valid JSON (${e.message})` };
    }
    const list = Array.isArray(json.extends) ? json.extends : [];
    if (list.includes(preset)) return { state: 'ok', desired: current };
    const fixed = { ...json, extends: [preset, ...list] };
    return { state: 'differs', desired: `${JSON.stringify(fixed, null, 2)}\n`, detail: `"extends" lacks ${preset}` };
  }
  throw new Error(`manifest: unknown mode "${entry.mode}" for ${entry.path}`);
}
