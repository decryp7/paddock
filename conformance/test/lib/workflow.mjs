// Workflow and action YAML as data, for tests: jobs, steps, `if`, `needs`, `env`, `with`, outputs. A test asserts on these
// fields, never on the file's text, so an intended edit (a reordered list, a longer `if`, a moved comment) does not break it
// and a real change (a dropped need, a lost condition) still does. New workflow tests use this helper (ER-21: three tests
// pinned YAML text and broke on intended edits; lib-usage.test.mjs keeps new ones honest).
//
// The reader is a small YAML subset (block maps and lists, flow lists and maps, quoted and plain scalars, `|` and `>` block
// scalars, comments); the repo has no YAML dependency, and conformance/src/util/workflows.mjs is line based and has no `if`,
// `with` or outputs. Anchors, aliases and merge keys throw, so a file that uses them is never read wrongly.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { PADDOCK } from '../helpers.mjs';

const isBlank = (t) => t === '' || t.startsWith('#');
const KEY = /^(?:"[^"]*"|'[^']*'|[^\s"'#[{\-][^:]*?|-[^\s:][^:]*?):(?:\s|$)/;

/** Cuts a trailing ` # comment` that is outside quotes. */
function uncomment(s) {
  let q = null;
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (q) { if (c === q) q = null; } else if (c === '"' || c === "'") q = c;
    else if (c === '#' && (i === 0 || /\s/.test(s[i - 1]))) return s.slice(0, i).trimEnd();
  }
  return s;
}

/** Splits at top-level commas of a flow collection body. */
function splitFlow(body) {
  const out = []; let depth = 0, q = null, cur = '';
  for (const c of body) {
    if (q) { cur += c; if (c === q) q = null; continue; }
    if (c === '"' || c === "'") { q = c; cur += c; continue; }
    if ('[{'.includes(c)) depth++;
    if (']}'.includes(c)) depth--;
    if (c === ',' && depth === 0) { out.push(cur); cur = ''; } else cur += c;
  }
  if (cur.trim()) out.push(cur);
  return out;
}

function scalar(raw) {
  const s = uncomment(raw.trim());
  if (/^[&*]\w/.test(s) || s.startsWith('<<')) throw new Error(`YAML anchors, aliases and merge keys are not read: ${s}`);
  if (s.startsWith('"')) return JSON.parse(s);
  if (s.startsWith("'")) return s.slice(1, -1).replace(/''/g, "'");
  if (s.startsWith('[')) return splitFlow(s.slice(1, -1)).map(scalar);
  if (s.startsWith('{')) {
    return Object.fromEntries(splitFlow(s.slice(1, -1)).map((kv) => {
      const at = kv.indexOf(':');
      return [scalar(kv.slice(0, at)), scalar(kv.slice(at + 1))];
    }));
  }
  if (s === 'true') return true;
  if (s === 'false') return false;
  if (s === 'null' || s === '~' || s === '') return null;
  if (/^-?\d+(\.\d+)?$/.test(s)) return Number(s);
  return s;
}

/** The YAML subset above as plain objects, arrays and scalars. */
export function parseYaml(text) {
  const lines = text.replace(/\r\n/g, '\n').split('\n').map((s) => ({ s, indent: s.length - s.trimStart().length, t: s.trim() }));
  let p = 0;
  const skip = () => { while (p < lines.length && isBlank(lines[p].t)) p++; };
  const isItem = (l) => l && (l.t === '-' || l.t.startsWith('- '));

  function node(min) {
    skip();
    const L = lines[p];
    if (!L || L.indent < min) return null;
    if (isItem(L)) return seq(L.indent);
    if (KEY.test(L.t)) return map(L.indent);
    p++;
    return scalar(L.t);
  }
  function seq(ind) {
    const out = [];
    for (;;) {
      skip();
      const L = lines[p];
      if (!isItem(L) || L.indent !== ind) break;
      const rest = L.t.slice(1).trimStart();
      if (!rest) { p++; out.push(node(ind + 1)); continue; }
      // The item's own text is a node at the column after "- ".
      const col = ind + (L.t.length - rest.length);
      lines[p] = { s: ' '.repeat(col) + rest, indent: col, t: rest };
      out.push(node(col));
    }
    return out;
  }
  function map(ind) {
    const out = {};
    for (;;) {
      skip();
      const L = lines[p];
      if (!L || L.indent !== ind || isItem(L) || !KEY.test(L.t)) break;
      const m = /^("[^"]*"|'[^']*'|.*?):(?:\s+(.*))?$/.exec(L.t);
      const key = scalar(m[1]);
      const rest = (m[2] ?? '').trim();
      p++;
      if (/^[|>][+-]?\d*(\s+#.*)?$/.test(rest)) { out[key] = blockScalar(ind, rest[0] === '>'); continue; }
      if (rest && !rest.startsWith('#')) { out[key] = scalar(rest); continue; }
      skip();
      const next = lines[p];
      out[key] = next && (next.indent > ind || (next.indent === ind && isItem(next))) ? node(next.indent) : null;
    }
    return out;
  }
  function blockScalar(ind, fold) {
    const body = [];
    while (p < lines.length && (lines[p].t === '' || lines[p].indent > ind)) body.push(lines[p++]);
    while (body.length && body[body.length - 1].t === '') body.pop();
    const base = Math.min(...body.filter((l) => l.t).map((l) => l.indent), Infinity);
    const rows = body.map((l) => l.s.slice(Math.min(base, l.indent)));
    return (fold ? rows.join(' ') : rows.join('\n')) + '\n';
  }
  const root = node(0);
  skip();
  if (p < lines.length) throw new Error(`YAML not read from line ${p + 1}: ${lines[p].t}`);
  return root;
}

/** A workflow or action file of the repository, parsed. */
export const loadYaml = (...rel) => parseYaml(readFileSync(join(PADDOCK, ...rel), 'utf8'));
export const loadWorkflow = (name) => loadYaml('.gitea', 'workflows', name);
export const loadAction = (name) => loadYaml('.gitea', 'actions', name, 'action.yml');

/** An expression without its `${{ }}` wrapper and with single spaces: `${{ a  &&  b }}` and `a && b` compare equal. */
export const expr = (e) => String(e ?? '').trim().replace(/^\$\{\{\s*([\s\S]*?)\s*\}\}$/, '$1').replace(/\s+/g, ' ');

/** The top-level `&&` parts of an expression (not those inside parentheses or quotes), normalised. */
export function conjuncts(e) {
  const s = expr(e);
  const out = []; let depth = 0, q = null, cur = '';
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (q) { cur += c; if (c === q) q = null; continue; }
    if (c === '"' || c === "'") { q = c; cur += c; continue; }
    if (c === '(') depth++;
    if (c === ')') depth--;
    if (c === '&' && s[i + 1] === '&' && depth === 0) { out.push(cur.trim()); cur = ''; i++; } else cur += c;
  }
  if (cur.trim()) out.push(cur.trim());
  return out.filter(Boolean);
}

/** A job's `needs` as a list, whatever the YAML form (absent: none). */
export const needsOf = (job) => [].concat(job?.needs ?? []);
/** A job by id, failing with the ids that exist. */
export function job(wf, id) {
  const j = wf.jobs?.[id];
  if (!j) throw new Error(`no job ${id}; jobs: ${Object.keys(wf.jobs ?? {}).join(', ')}`);
  return j;
}
/** The steps of a job (or of a composite action's `runs`). */
export const stepsOf = (j) => j?.steps ?? j?.runs?.steps ?? [];
/** A step by `id` or `name`. */
export const stepBy = (j, sel) => stepsOf(j).find((s) => s.id === sel || s.name === sel);
/** The steps of a job that `uses` the action `ref` (as written, for example `./.gitea/actions/release-verdict`). */
export const stepsUsing = (j, ref) => stepsOf(j).filter((s) => s.uses === ref);
/** Every `with:` map of a workflow: job-level (reusable workflows) and step-level, as { where, with }. */
export function withs(wf) {
  return Object.entries(wf.jobs ?? {}).flatMap(([id, j]) => [
    ...(j.with ? [{ where: id, with: j.with }] : []),
    ...stepsOf(j).filter((s) => s.with).map((s) => ({ where: `${id}/${s.name ?? s.uses ?? s.id}`, with: s.with })),
  ]);
}

/** The shell of the composite action's `run` step selected by id, name or index, with `${{ key }}` filled from `values` as the runner would. */
export function actionScript(wf, sel, values = {}) {
  const steps = stepsOf(wf);
  const st = typeof sel === 'number' ? steps[sel < 0 ? steps.length + sel : sel] : steps.find((s) => s.id === sel || s.name === sel);
  if (!st?.run) throw new Error(`no run step ${sel}`);
  return st.run.replace(/\$\{\{\s*([\w.]+)\s*\}\}/g, (_, k) => values[k] ?? (() => { throw new Error(`unexpected expression ${k}`); })());
}

/** A $GITHUB_OUTPUT file as a map: `k=v` lines and `k<<DELIM` blocks; a later write wins, as on the runner. */
export function parseOutputs(text) {
  const out = {};
  const lines = text.split('\n');
  for (let i = 0; i < lines.length; i++) {
    const heredoc = /^([^=<]+)<<(.+)$/.exec(lines[i]);
    if (heredoc) {
      const body = [];
      for (i++; i < lines.length && lines[i] !== heredoc[2]; i++) body.push(lines[i]);
      out[heredoc[1]] = body.join('\n');
    } else if (lines[i].includes('=')) {
      const at = lines[i].indexOf('=');
      out[lines[i].slice(0, at)] = lines[i].slice(at + 1);
    }
  }
  return out;
}
