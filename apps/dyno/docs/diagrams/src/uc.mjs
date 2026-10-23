import { P, save } from './common.mjs';
import { ld } from '../../../../paddock/diagrams/tools/lib.mjs';

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
function ellipse(d, id, x, y, w, h, text, planned) {
  d.rect[id] = [x, y, w, h];
  const a = d.t.accent;
  d.raw(`<mxCell id="${id}" value="${esc(`<span style="font-size:12px">${esc(text)}</span>`)}" style="${d.base(`ellipse;strokeWidth=1.5;strokeColor=${ld(a.s)};fillColor=${ld(planned ? d.t.white : a.f)};${planned ? 'dashed=1;dashPattern=6 4;' : ''}align=center;verticalAlign=middle;spacingLeft=28;spacingRight=28;`)}" vertex="1" parent="1"><mxGeometry x="${x}" y="${y}" width="${w}" height="${h}" as="geometry"/></mxCell>`);
  if (planned) d.tag(x + w + 84, y + 19, 'planned', 'accent');
}
function actor(d, id, x, y, name) {
  d.rect[id] = [x, y, 40, 64];
  d.raw(`<mxCell id="${id}" value="${esc(`<span style="font-size:12px;font-weight:600">${esc(name)}</span>`)}" style="${d.base(`shape=umlActor;verticalLabelPosition=bottom;verticalAlign=top;spacingTop=8;strokeWidth=1.5;strokeColor=${ld(d.t.process.s)};fillColor=none;whiteSpace=nowrap;`)}" vertex="1" parent="1"><mxGeometry x="${x}" y="${y}" width="40" height="64" as="geometry"/></mxCell>`);
}
/** rows: [text, planned?] stacked in one column inside the system boundary; the owner joins them through one vertical bus with taps. */
function useCases(file, { title, desc, owner, rows, ext }) {
  const UW = 360, UH = 56, PITCH = 72, y0 = 144, bx = 248;
  const bw = 24 + UW + 96 + 24, bh = 40 + rows.length * PITCH - (PITCH - UH) + 24;
  const d = P({ title, kind: 'UML · Use case diagram', desc, w: bx + bw + (ext ? 232 : 48) });
  d.group('b', bx, y0, bw, bh, 'Dyno');
  const cy = (i) => y0 + 40 + i * PITCH + UH / 2;
  rows.forEach(([t, p], i) => ellipse(d, `u${i}`, bx + 24, y0 + 40 + i * PITCH, UW, UH, t, p));
  const mid = Math.round((cy(0) + cy(rows.length - 1)) / 2);
  actor(d, 'a', 96, mid - 32, owner);
  const trunk = bx - 56;
  rows.forEach((_, i) => d.edge('a', `u${i}`, [136, mid], [bx + 24, cy(i)], { arrow: 'none', via: mid === cy(i) ? [] : [[trunk, mid], [trunk, cy(i)]] }));
  if (ext) {
    const i = ext.row, ex = bx + bw + 120;
    actor(d, 'x', ex, Math.round(cy(i) - 32), ext.name);
    d.edge('x', `u${i}`, [ex, Math.round(cy(i))], [bx + 24 + UW, cy(i)], { arrow: 'none', via: [] });
  }
  d.legend([{ kind: 'accent', label: 'Use case' }, { kind: 'planned', label: 'Planned' }, { kind: 'line', label: 'Association' }]);
  save(file, d, y0 + bh);
}
useCases('uml-use-cases-observe', {
  title: 'Use cases — see and understand', desc: 'What the PC owner learns about the machine. Nothing here changes it. Dashed ellipses are planned.', owner: 'PC owner',
  rows: [['Watch live performance'], ['Browse and sort processes'], ['Ask "why is my PC slow?"'], ['Review history and spikes'], ['Check health and read the reasons'], ['Receive alerts and anomalies'], ['Browse Issues and unusual processes', 1], ['Watched folders with keep-for times', 1], ['Read the weekly report', 1], ['Browse performance counters'], ['Check throttling, disk and battery life'], ['Ask "is this normal?"']],
});
useCases('uml-use-cases-act', {
  title: 'Use cases — change and tune', desc: 'Everything that changes the machine needs an enabled rule or an explicit click, and is undoable where the OS allows. Dashed ellipses are planned.', owner: 'PC owner',
  rows: [['End, suspend or re-prioritise an app'], ['Tame an app with a rule or profile'], ['Manage startup items'], ['Undo an action from Activity'], ['Set tray metric, theme, sampling'], ['Install the helper, approve an admin change'], ['Start the agent at login'], ['Clean up disk with quarantine', 1], ['Apply a tuning suggestion', 1], ['Run a benchmark on click', 1]],
  ext: { name: 'OS (login, power)', row: 6 },
});
