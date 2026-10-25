// Generates Pacenotes' draw.io diagrams with the Paddock diagram kit (diagrams/tools at the monorepo root).
// Run: node docs/diagrams/src/generate.mjs, then scripts/export-diagrams.sh. Re-running overwrites the generated .drawio files.
import { Diagram, link, g8 } from '../../../../../diagrams/tools/lib.mjs';
import { writeFileSync } from 'node:fs';

const OUT = new URL('..', import.meta.url).pathname.replace(/\/$/, '');
const DATE = '2026-10-23';

// Pacenotes' accent (not yet in Paddock's themes.mjs): the draft icon colour, stroke on fill as STYLE.md derives them.
const mix = (hex, bg, a) => {
  const p = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
  const [c, b] = [p(hex), p(bg)];
  return '#' + c.map((v, i) => Math.round(b[i] + (v - b[i]) * a).toString(16).padStart(2, '0')).join('').toUpperCase();
};
const accent = { s: ['#831826', '#EE6677'], f: [mix('#831826', '#FFFFFF', 0.1), mix('#EE6677', '#15171B', 0.16)] };
const P = (o) => {
  const d = new Diagram({ app: 'kit', date: DATE, ...o });
  d.t.accent = accent;
  d.t.app = 'Pacenotes';
  return d;
};
const save = (name, d, bottom) => writeFileSync(`${OUT}/${name}.drawio`, d.finish(bottom));
const Y0 = 144;
const bottomOf = (d, ...ids) => Math.max(...ids.map((i) => d.rect[i][1] + d.rect[i][3]));

/** Straight link from a node to a tall hub (or back), pinned at the node's row centre on the hub. */
function toHub(d, hubId, side, nodeId, nside, rev = false) {
  const [, hy, , hh] = d.rect[hubId], [, ny, , nh] = d.rect[nodeId];
  const t = (ny + nh / 2 - hy) / hh;
  const [a, b] = rev ? [nodeId, hubId] : [hubId, nodeId];
  const [sa, sb] = rev ? [nside, side] : [side, nside];
  return link(d, a, sa, b, sb, { t: rev ? [0.5, t] : [t, 0.5] });
}

// ---- 1. Understand a new codebase: tiers and hybrid retrieval ----
{
  const d = P({
    title: 'Understand a new codebase',
    kind: 'Flow · Indexing tiers and hybrid retrieval',
    desc: 'Every language gets tier 1; tree-sitter adds tier 2; a language pack adds tier 3. Vectors are optional and local.',
    w: 1600,
  });
  const W = 240, H = 120;
  const cx = [48, 704, 1008, 1312];
  const r1 = Y0, r2 = Y0 + 168, r3 = Y0 + 336;
  // Tiers group (column B)
  const gx = 352, gy = Y0, nw = 240;
  const nodes = [
    ['t1', 'accent', { name: 'Tier 1 · Text', tech: 'every language', desc: 'Full-text and regex search, file tree, git history and hot spots, docs' }],
    ['t2', 'accent', { name: 'Tier 2 · Syntax', tech: 'tree-sitter grammars', desc: 'Symbols, outline, imports, approximate call graph, where defined and used. C, C++, C#, Java, TypeScript/JavaScript, Rust, Python, Go' }],
    ['t3', 'plugin', { name: 'Tier 3 · Semantic', tech: 'declarative language pack', desc: 'Exact references, types and calls from the server or a SCIP index; cached in the index' }],
  ];
  const hs = nodes.map(([, , o]) => d.fit(nw, o));
  const gh = 40 + hs.reduce((a, b) => a + b, 0) + 24 * 2 + 24;
  d.group('tiers', gx, gy, nw + 48, gh, 'Indexing tiers (per language)');
  let y = gy + 40;
  nodes.forEach(([id, role, o], i) => { d.node(id, gx + 24, y, nw, hs[i], role, o); y += hs[i] + 24; });

  const t1y = d.rect.t1[1], t2b = d.rect.t2[1] + d.rect.t2[3], t3 = d.rect.t3;
  const rh = 128, ry = Math.round((t1y + t2b - rh) / 2);
  d.node('repo', cx[0], ry, W, rh, 'external', {
    name: 'Repository', tech: 'folder on disk', desc: 'Sources, git history, docs and build files (Cargo, CMake, MSBuild, Gradle, npm…). Never executed',
  });
  d.node('lsp', cx[0], t3[1], W, t3[3], 'external', {
    name: 'Language server', tech: 'LSP client · child process', desc: 'rust-analyzer, clangd, pyright, gopls… installed separately; stops when idle',
  });
  d.node('index', cx[1], gy, W + 0, null, 'process', { name: 'Local index', tech: 'SQLite · FTS5 · incremental', desc: 'Files, symbols, edges, full-text, git facts' });
  d.node('chunk', cx[1], gy + 168, W, null, 'core', { name: 'Syntax chunker', desc: 'Functions and classes from tree-sitter; fixed windows for other files' });
  d.node('embed', cx[1], gy + 336, W, null, 'process', { name: 'Embedder', tech: 'optional', desc: 'Pitwall (Qwen3-Embedding) or a small local model' });
  d.node('retr', cx[2], gy, W, null, 'core', { name: 'Hybrid retrieval', desc: 'Text + structural + vector, fused with RRF, then re-ranked; cited results' });
  d.node('vec', cx[2], gy + 168, W, null, 'process', { name: 'Vector index', tech: 'usearch · per repository · opt-in', desc: 'Chunk embeddings, re-embedded on change' });
  d.node('mcp', cx[3], gy, W, null, 'accent', { name: 'MCP server', tech: 'read-only tools', desc: 'The same view for the UI, Pitwall, Claude Code, Debrief and Sighter' });
  d.node('who', cx[3], gy + 168, W, null, 'external', { name: 'Local AI and agents', desc: 'Pitwall, Claude Code, Debrief, Sighter. Answers carry file and line citations' });

  const tt = (id) => (d.rect[id][1] + d.rect[id][3] / 2 - gy) / gh;
  link(d, 'repo', 'R', 'tiers', 'L', { t: [0.5, tt('repo')] });
  link(d, 'lsp', 'R', 'tiers', 'L', { t: [0.5, tt('lsp')] });
  toHub(d, 'tiers', 'R', 'index', 'L');
  toHub(d, 'tiers', 'R', 'chunk', 'L');
  link(d, 'chunk', 'B', 'embed', 'T');
  const e = d.rect.embed, v = d.rect.vec;
  link(d, 'embed', 'R', 'vec', 'B', { t: [0.5, 0.5], via: [[v[0] + v[2] / 2, e[1] + e[3] / 2]] });
  link(d, 'index', 'R', 'retr', 'L');
  link(d, 'vec', 'T', 'retr', 'B');
  link(d, 'retr', 'R', 'mcp', 'L');
  link(d, 'mcp', 'B', 'who', 'T');

  d.legend([
    { kind: 'accent', label: 'Pacenotes' }, { kind: 'core', label: 'Pure core' }, { kind: 'plugin', label: 'Language pack (optional)' },
    { kind: 'process', label: 'Index · data · helper' }, { kind: 'external', label: 'Outside Pacenotes' }, { kind: 'line', label: 'Feeds' },
  ]);
  save('understand-tiers-retrieval', d, bottomOf(d, 'tiers', 'embed', 'who', 'lsp'));
}

// ---- 2. Review pipeline ----
{
  const d = P({
    title: 'Review pipeline',
    kind: 'Flow · From change and rules to findings',
    desc: 'Three rule sources, one list of findings with evidence. Without Pitwall the structural and SARIF rules still run.',
    w: 1400,
  });
  const NW = 264, h = 104;
  const r0 = Y0, r1 = Y0 + h + 88, r2 = r1 + h + 40, r3 = r2 + h + 40;
  d.node('chg', 48 + 24, r0, NW, h, 'process', { name: 'Change to review', tech: 'diff · branch · commit range · whole repo', desc: 'Read-only; the repository is never run' });
  d.group('srcs', 48, r1 - 40, NW + 48, 3 * h + 2 * 40 + 40 + 24, 'Rule sets (repo and user)');
  d.node('s_md', 72, r1, NW, h, 'process', { name: 'Markdown rules', tech: '.pacenotes/rules/*.md', desc: 'Natural language, severity, scope, good and bad examples' });
  d.node('s_st', 72, r2, NW, h, 'process', { name: 'Structural rules', tech: '.pacenotes/rules/*.yml', desc: 'tree-sitter queries, ast-grep-style patterns, tested' });
  d.node('s_sa', 72, r3, NW, h, 'process', { name: 'Linter results', tech: 'SARIF', desc: 'clippy, ESLint, Roslyn analyzers… imported, never run' });
  const bx = 48 + NW + 48 + 72;
  d.node('ground', bx, r0, NW, h, 'core', { name: 'Ground in the index', desc: 'Callers, tests, owners, coverage and history of what changed' });
  d.node('model', bx, r1, NW, h, 'external', { name: 'Local model check', tech: 'Pitwall · optional', desc: 'Judges each rule; must cite file and line' });
  d.node('pat', bx, r2, NW, h, 'core', { name: 'Pattern engine', desc: 'Matches structural rules on changed code' });
  d.node('sarif', bx, r3, NW, h, 'core', { name: 'SARIF import', desc: 'Maps results onto the change' });
  const mx = bx + NW + 72;
  const mh = r3 + h - r0;
  d.node('merge', mx, r0, NW, mh, 'accent', {
    name: 'Findings', tech: 'one list',
    desc: 'Evidence (file and line), explanation and a suggested fix as a diff. Duplicates merged; baseline hides legacy findings so only new ones fail; suppressions need a reason; accepted and dismissed findings tune later runs, locally',
  });
  const ox = mx + NW + 72;
  d.node('o_ui', ox, r0, NW, h, 'ui', { name: 'Review panel', desc: 'Findings by file and severity, inline in the diff' });
  d.node('o_md', ox, r1, NW, h, 'process', { name: 'Markdown and SARIF', desc: 'Export for reports and other viewers' });
  d.node('o_ci', ox, r2, NW, h, 'process', { name: 'CLI for CI', desc: 'Exit codes: 0 clean, 1 findings, 2 error, 3 partial' });
  d.node('o_pr', ox, r3, NW, h, 'external', { name: 'Gitea PR comments', desc: 'Only on your click, after a preview' });

  link(d, 'chg', 'R', 'ground', 'L', { t: [0.5, 0.5], via: [] });
  link(d, 'ground', 'B', 'model', 'T');
  link(d, 's_md', 'R', 'model', 'L');
  link(d, 's_st', 'R', 'pat', 'L');
  link(d, 's_sa', 'R', 'sarif', 'L');
  for (const n of ['model', 'pat', 'sarif']) toHub(d, 'merge', 'L', n, 'R', true);
  for (const n of ['o_ui', 'o_md', 'o_ci', 'o_pr']) toHub(d, 'merge', 'R', n, 'L');

  d.legend([
    { kind: 'accent', label: 'Result' }, { kind: 'core', label: 'Pure core' }, { kind: 'ui', label: 'UI' },
    { kind: 'process', label: 'Input · output' }, { kind: 'external', label: 'Optional / outside' }, { kind: 'line', label: 'Feeds' },
  ]);
  save('review-pipeline', d, r3 + h);
}

// ---- 3. Dependency graph model ----
{
  const d = P({
    title: 'Dependency graph',
    kind: 'Flow · One graph of all links',
    desc: 'Built from the index and manifests; laid out and aggregated in the engine, so thousands of nodes stay usable.',
    w: 1264,
  });
  const NW = 264, h = 104, GW = 408;
  const rows = [0, 1, 2, 3].map((i) => Y0 + i * (h + 40));
  const ins = [
    ['i_idx', 'process', { name: 'Code index', tech: 'tiers 1–3', desc: 'Imports, calls, inheritance, FFI, config use, tests' }],
    ['i_man', 'process', { name: 'Manifests and lock files', desc: 'Packages with versions and licences, services, endpoints, topics, tables' }],
    ['i_adv', 'process', { name: 'Advisory mirror', tech: 'OSV · offline', desc: 'Known vulnerabilities, updated only when you ask' }],
    ['i_spo', 'external', { name: 'Spotter', desc: '“Newer version available”, family integration' }],
  ];
  ins.forEach(([id, role, o], i) => d.node(id, 48, rows[i], NW, h, role, o));
  const gx = 48 + NW + 96;
  const gh = rows[3] + h - rows[0];
  d.node('graph', gx, rows[0], GW, gh, 'accent', {
    name: 'Link graph', tech: 'engine-side layout and aggregation',
    desc: 'Zoom levels: repo and services → packages and modules → files → symbols, disclosed progressively. Nodes: internal module, external package, service, database table, config key, API endpoint, message topic. Edges: imports, calls, inherits and implements, reads and writes table, HTTP, gRPC and IPC calls, FFI, config use, test covers. Cycles highlighted; rules such as “UI must not depend on the database” are checked',
  });
  const ox = gx + GW + 96;
  const outs = [
    ['o_view', 'ui', { name: 'Interactive graph', desc: 'Filters, collapse and expand, focus with N hops, shortest path, diff of a branch' }],
    ['o_exp', 'process', { name: 'draw.io and SVG', desc: 'Editable export of any view' }],
    ['o_mcp', 'accent', { name: 'MCP tools', desc: 'The same queries, read-only' }],
    ['o_rev', 'core', { name: 'Review', desc: 'Layer-rule violations become findings' }],
  ];
  outs.forEach(([id, role, o], i) => d.node(id, ox, rows[i], NW, h, role, o));
  for (const n of ['i_idx', 'i_man', 'i_adv', 'i_spo']) toHub(d, 'graph', 'L', n, 'R', true);
  for (const n of ['o_view', 'o_exp', 'o_mcp', 'o_rev']) toHub(d, 'graph', 'R', n, 'L');
  d.legend([
    { kind: 'accent', label: 'Pacenotes' }, { kind: 'core', label: 'Pure core' }, { kind: 'ui', label: 'UI' },
    { kind: 'process', label: 'Input · output' }, { kind: 'external', label: 'Other app' }, { kind: 'line', label: 'Feeds' },
  ]);
  save('dependency-graph', d, rows[3] + h);
}
console.log('ok');
