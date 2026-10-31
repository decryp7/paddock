// Generates the DG1 round-trip INPUT files (inputs/*.drawio) in the canonical form of ADR 0011: uncompressed, one page,
// one cell per line, fixed attribute order, generated cells wrapped in <object> with gen_* records. The hashes are
// placeholders of the right shape (16 hex digits): the real writer arrives with DG1; here they only have to survive a save.
// Run: node gen-inputs.mjs   (writes next to this file)
import { createHash } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const h16 = (s) => createHash('sha256').update(s).digest('hex').slice(0, 16);
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/\n/g, '&#xa;');
const GRID = 10;

const NODE_STYLE = 'rounded=1;whiteSpace=wrap;html=1;fillColor=#dae8fc;strokeColor=#6c8ebf;fontSize=12;';
const EDGE_STYLE = 'edgeStyle=orthogonalEdgeStyle;rounded=0;html=1;endArrow=block;endFill=1;';

function node({ id, kind = 'module', label, x, y, w = 120, h = 50, style = NODE_STYLE, parent = '1', extra = {} }) {
  const key = `${kind}:${id}`;
  const attrs = { label, gen_key: `pacenotes|${key}`, gen_label: h16(label), gen_style: h16(style), gen_geo: `${x},${y},${w},${h}`, gen_parent: parent, ...extra };
  const a = Object.entries(attrs).map(([k, v]) => `${k}="${esc(String(v))}"`).join(' ');
  return `        <object id="pn:${kind}:${h16(key)}" ${a}>\n          <mxCell style="${esc(style)}" vertex="1" parent="${parent}"><mxGeometry x="${x}" y="${y}" width="${w}" height="${h}" as="geometry"/></mxCell>\n        </object>`;
}
const nid = (kind, id) => `pn:${kind}:${h16(`${kind}:${id}`)}`;
function edge({ from, to, label = '', style = EDGE_STYLE, kind = 'module', ekind = 'depends' }) {
  const s = nid(kind, from), t = nid(kind, to), key = `${ekind}:${from}->${to}`;
  const attrs = { label, gen_key: `pacenotes|${key}`, gen_label: h16(label), gen_style: h16(style), gen_ends: `${s},${t}`, gen_parent: '1' };
  const a = Object.entries(attrs).map(([k, v]) => `${k}="${esc(String(v))}"`).join(' ');
  return `        <object id="pn:edge:${h16(key)}" ${a}>\n          <mxCell style="${esc(style)}" edge="1" parent="1" source="${s}" target="${t}"><mxGeometry relative="1" as="geometry"/></mxCell>\n        </object>`;
}
function file(name, ids, cells) {
  const page = `        <object id="0" gen_page="v1;${ids.join(',')};tombstones=">\n          <mxCell/>\n        </object>`;
  return `<mxfile host="pacenotes" version="1" compressed="false">
  <diagram id="${esc(name)}" name="${esc(name)}">
    <mxGraphModel dx="0" dy="0" grid="1" gridSize="${GRID}" guides="1" tooltips="1" connect="1" arrows="1" fold="1" page="0" pageScale="1" pageWidth="1169" pageHeight="827" math="0" shadow="0">
      <root>
${page}
        <mxCell id="1" parent="0"/>
${cells.join('\n')}
      </root>
    </mxGraphModel>
  </diagram>
</mxfile>
`;
}
const write = (f, text) => { mkdirSync(join(here, 'inputs'), { recursive: true }); writeFileSync(join(here, 'inputs', f), text); };

// 01: the smallest canonical file (4 modules, 3 edges).
{
  const n = [['app', 'app', 40, 40], ['core', 'core', 240, 40], ['store', 'store', 240, 140], ['util', 'util', 440, 90]];
  const cells = [...n.map(([id, label, x, y]) => node({ id, label, x, y })),
    edge({ from: 'app', to: 'core' }), edge({ from: 'core', to: 'store' }), edge({ from: 'core', to: 'util' })];
  const ids = [...n.map(([id]) => nid('module', id))];
  write('01-canonical-basic.drawio', file('01 canonical basic', ids, cells));
}
// 02: module graph, 10 nodes, 12 edges.
{
  const n = [['cli', 40, 40], ['app', 220, 40], ['api', 400, 40], ['core', 220, 140], ['parse', 40, 240], ['index', 220, 240], ['store', 400, 240],
    ['git', 580, 240], ['rules', 220, 340], ['diagram', 400, 340]];
  const e = [['cli', 'app'], ['app', 'api'], ['app', 'core'], ['api', 'core'], ['core', 'parse'], ['core', 'index'], ['index', 'store'], ['index', 'git'],
    ['app', 'rules'], ['rules', 'index'], ['diagram', 'index'], ['parse', 'index']];
  const cells = [...n.map(([id, x, y]) => node({ id: `pacenotes-${id}`, label: `pacenotes-${id}`, x, y, w: 140 })), ...e.map(([a, b]) => edge({ from: `pacenotes-${a}`, to: `pacenotes-${b}` }))];
  write('02-module-graph.drawio', file('02 module graph', n.map(([id]) => nid('module', `pacenotes-${id}`)), cells));
}
// 03: C4-ish containers: one system boundary containing three containers (children use the boundary as parent), a person
// and an external system outside; edges carry labels.
{
  const bstyle = 'rounded=0;whiteSpace=wrap;html=1;fillColor=none;strokeColor=#666666;dashed=1;container=1;collapsible=0;recursiveResize=0;verticalAlign=top;align=left;spacingLeft=8;fontStyle=1;';
  const cstyle = 'rounded=1;whiteSpace=wrap;html=1;fillColor=#1168bd;strokeColor=#0b4884;fontColor=#ffffff;';
  const pstyle = 'rounded=1;whiteSpace=wrap;html=1;fillColor=#08427b;strokeColor=#073b6f;fontColor=#ffffff;';
  const xstyle = 'rounded=1;whiteSpace=wrap;html=1;fillColor=#999999;strokeColor=#8a8a8a;fontColor=#ffffff;';
  const L = (t, s) => `<b>${t}</b><br><font style="font-size:10px">[${s}]</font>`;
  const bid = nid('system', 'pacenotes');
  const cells = [
    node({ id: 'user', kind: 'person', label: L('Reviewer', 'Person'), x: 40, y: 200, w: 160, h: 80, style: pstyle }),
    node({ id: 'pacenotes', kind: 'system', label: 'Pacenotes', x: 280, y: 40, w: 560, h: 420, style: bstyle }),
    node({ id: 'ui', kind: 'container', label: L('Workbench', 'Angular'), x: 40, y: 60, w: 200, h: 90, style: cstyle, parent: bid }),
    node({ id: 'engine', kind: 'container', label: L('Engine', 'Rust'), x: 300, y: 60, w: 200, h: 90, style: cstyle, parent: bid }),
    node({ id: 'db', kind: 'container', label: L('Index store', 'DuckDB'), x: 300, y: 260, w: 200, h: 90, style: cstyle, parent: bid }),
    node({ id: 'git', kind: 'external', label: L('Git repository', 'External'), x: 960, y: 200, w: 160, h: 80, style: xstyle }),
    edge({ from: 'user', to: 'ui', label: 'Reads findings', kind: 'person', ekind: 'uses' }).replace('target="' + nid('person', 'ui') + '"', 'target="' + nid('container', 'ui') + '"').replace(nid('person', 'ui'), nid('container', 'ui')),
  ];
  // simpler and explicit edges with correct end kinds:
  cells.pop();
  const e2 = (key, s, t, label) => {
    const attrs = { label, gen_key: `pacenotes|${key}`, gen_label: h16(label), gen_style: h16(EDGE_STYLE), gen_ends: `${s},${t}`, gen_parent: '1' };
    const a = Object.entries(attrs).map(([k, v]) => `${k}="${esc(String(v))}"`).join(' ');
    return `        <object id="pn:edge:${h16(key)}" ${a}>\n          <mxCell style="${esc(EDGE_STYLE)}" edge="1" parent="1" source="${s}" target="${t}"><mxGeometry relative="1" as="geometry"/></mxCell>\n        </object>`;
  };
  cells.push(e2('uses:user->ui', nid('person', 'user'), nid('container', 'ui'), 'Reads findings'),
    e2('uses:ui->engine', nid('container', 'ui'), nid('container', 'engine'), 'Commands (IPC)'),
    e2('uses:engine->db', nid('container', 'engine'), nid('container', 'db'), 'Reads and writes'),
    e2('uses:engine->git', nid('container', 'engine'), nid('external', 'git'), 'Reads history'));
  const ids = [nid('person', 'user'), bid, nid('container', 'ui'), nid('container', 'engine'), nid('container', 'db'), nid('external', 'git')];
  write('03-c4-containers.drawio', file('03 c4 containers', ids, cells));
}
// 04: labels and styles: HTML labels, entities, unicode, multi-line, tooltip, link, tags (H16), dashed edge, a manual cell.
{
  const s1 = 'rounded=1;whiteSpace=wrap;html=1;fillColor=#d5e8d4;strokeColor=#82b366;fontStyle=1;fontSize=14;';
  const s2 = 'shape=cylinder3;whiteSpace=wrap;html=1;boundedLbl=1;backgroundOutline=1;size=15;fillColor=#fff2cc;strokeColor=#d6b656;';
  const dashed = 'edgeStyle=orthogonalEdgeStyle;rounded=0;html=1;dashed=1;dashPattern=4 4;endArrow=open;endFill=0;strokeColor=#b85450;';
  const cells = [
    node({ id: 'a', kind: 'class', label: '<b>Reader</b><br>fn read(&lt;T&gt;) &amp; more', x: 40, y: 40, w: 200, h: 70, style: s1, extra: { tooltip: 'Reads files & checks <limits>', link: 'https://example.invalid/reader', tags: 'core hot' } }),
    node({ id: 'b', kind: 'class', label: 'Naïve ünïcode — 日本語 ✓', x: 340, y: 40, w: 220, h: 70, style: s1 }),
    node({ id: 'c', kind: 'class', label: 'Line one\nLine two', x: 40, y: 190, w: 200, h: 70, style: s2 }),
    node({ id: 'd', kind: 'class', label: 'plain', x: 340, y: 190, w: 220, h: 70, style: NODE_STYLE + 'dashed=1;opacity=80;rotation=0;' }),
    edge({ from: 'a', to: 'b', label: 'calls', style: dashed, kind: 'class', ekind: 'calls' }),
    edge({ from: 'c', to: 'd', label: 'uses <T>', kind: 'class', ekind: 'uses' }),
    // a manual cell (no gen_key): must survive untouched
    `        <mxCell id="manual-note" value="hand-made note" style="shape=note;whiteSpace=wrap;html=1;fillColor=#ffe6cc;strokeColor=#d79b00;" vertex="1" parent="1"><mxGeometry x="640" y="40" width="140" height="80" as="geometry"/></mxCell>`,
  ];
  write('04-labels-styles.drawio', file('04 labels and styles', ['a', 'b', 'c', 'd'].map((i) => nid('class', i)), cells));
}
console.log('inputs written');
