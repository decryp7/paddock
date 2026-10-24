// A small, dependency-free reader for the parts of Gitea/GitHub workflow files the checks need: jobs (with `needs`) and
// steps (with `env`, `run`, `uses`). It is line based on purpose; it reads the indentation, not the full YAML grammar.

const indentOf = (l) => l.length - l.trimStart().length;
const isBlank = (l) => l.trim() === '' || l.trim().startsWith('#');

/** Lines of the block that starts after line index `i` and is indented deeper than `base` (blank and comment lines kept). */
function blockAfter(lines, i, base) {
  const out = [];
  for (let j = i + 1; j < lines.length; j++) {
    if (!isBlank(lines[j]) && indentOf(lines[j]) <= base) break;
    out.push(j);
  }
  return out;
}

/** Jobs of a workflow: [{ id, line, needs: string[], text, startLine }] */
export function parseJobs(text) {
  const lines = text.split('\n');
  const at = lines.findIndex((l) => /^jobs:\s*$/.test(l));
  if (at < 0) return [];
  const body = blockAfter(lines, at, 0);
  const first = body.find((j) => !isBlank(lines[j]));
  if (first === undefined) return [];
  const base = indentOf(lines[first]);
  const heads = body.filter((j) => !isBlank(lines[j]) && indentOf(lines[j]) === base && /^\s*[\w.-]+:\s*$/.test(lines[j]));
  return heads.map((h, n) => {
    const end = n + 1 < heads.length ? heads[n + 1] : body[body.length - 1] + 1;
    const jobLines = lines.slice(h, end);
    const id = lines[h].trim().replace(/:$/, '');
    return { id, line: h + 1, needs: parseNeeds(jobLines, base), text: jobLines.join('\n') };
  });
}

function parseNeeds(jobLines, base) {
  for (let i = 0; i < jobLines.length; i++) {
    const m = /^(\s*)needs:\s*(.*)$/.exec(jobLines[i]);
    if (!m || m[1].length !== base + 2) continue;
    const rest = m[2].replace(/#.*$/, '').trim();
    if (rest.startsWith('[')) return rest.replace(/^\[|\]$/g, '').split(',').map((s) => s.trim()).filter(Boolean);
    if (rest) return [rest];
    const out = [];
    for (let j = i + 1; j < jobLines.length && /^\s*-\s/.test(jobLines[j]); j++) out.push(jobLines[j].replace(/^\s*-\s*/, '').replace(/#.*$/, '').trim());
    return out;
  }
  return [];
}

/** Steps of a workflow: [{ job, name, line, text, run, uses, env: Record<string, string> }] (env of the step only). */
export function parseSteps(text) {
  const lines = text.split('\n');
  const out = [];
  for (let i = 0; i < lines.length; i++) {
    if (!/^\s*steps:\s*$/.test(lines[i])) continue;
    const body = blockAfter(lines, i, indentOf(lines[i]));
    const first = body.find((j) => !isBlank(lines[j]));
    if (first === undefined) continue;
    const base = indentOf(lines[first]);
    const heads = body.filter((j) => indentOf(lines[j]) === base && /^\s*-\s/.test(lines[j]));
    heads.forEach((h, n) => {
      const end = n + 1 < heads.length ? heads[n + 1] : body[body.length - 1] + 1;
      const stepLines = lines.slice(h, end);
      out.push(describeStep(stepLines, h + 1));
    });
  }
  return out;
}

function describeStep(stepLines, line) {
  // Treat the "- " marker as indentation so every key of the step sits at the same column.
  const norm = stepLines.map((l, i) => (i === 0 ? l.replace(/^(\s*)-\s/, '$1  ') : l));
  const base = indentOf(norm[0]);
  const text = norm.join('\n');
  const key = (k) => {
    const re = new RegExp(`^\\s{${base}}${k}:\\s*(.*)$`);
    for (let i = 0; i < norm.length; i++) {
      const m = re.exec(norm[i]);
      if (!m) continue;
      const v = m[1].trim();
      if (/^[|>][+-]?$/.test(v)) {
        const block = [];
        for (let j = i + 1; j < norm.length && (isBlank(norm[j]) || indentOf(norm[j]) > base); j++) block.push(norm[j].trim());
        return block.join('\n');
      }
      return v;
    }
    return null;
  };
  const env = {};
  const e = norm.findIndex((l) => new RegExp(`^\\s{${base}}env:\\s*$`).test(l));
  if (e >= 0) {
    for (let j = e + 1; j < norm.length && (isBlank(norm[j]) || indentOf(norm[j]) > base); j++) {
      const m = /^\s*([A-Za-z_][\w]*):\s*(.*)$/.exec(norm[j]);
      if (m) env[m[1]] = m[2];
    }
  }
  return { name: key('name'), line, text, run: key('run'), uses: key('uses'), env };
}
