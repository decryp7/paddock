import { P, save } from './common.mjs';
import { ld } from '../../../../../diagrams/tools/lib.mjs';

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
    const rs = ext.rows ?? [ext.row], ex = bx + bw + 120, ay = Math.round(rs.reduce((a, i) => a + cy(i), 0) / rs.length), bus = ex - 48;
    actor(d, 'x', ex, ay - 32, ext.name);
    rs.forEach((i) => d.edge('x', `u${i}`, [ex, ay], [bx + 24 + UW, cy(i)], { arrow: 'none', via: ay === cy(i) ? [] : [[bus, ay], [bus, cy(i)]] }));
  }
  d.legend([{ kind: 'accent', label: 'Use case' }, ...(rows.some(([, p]) => p) ? [{ kind: 'planned', label: 'Planned' }] : []), { kind: 'line', label: 'Association', arrow: 'none' }]);
  save(file, d, y0 + bh);
}
useCases('uml-use-cases-observe', {
  title: 'Use cases — see and understand', desc: 'What the PC owner learns about the machine.', owner: 'PC owner',
  rows: [['Watch live performance and history'], ['Browse and sort processes'], ['Ask "why is my PC slow?" or "is this normal?"'], ['Check health and read the reasons'], ['See what starts at login'], ['Receive alarms and anomalies'], ['Browse Issues and unusual processes'], ['Read the weekly report'], ['Browse performance counters'], ['Check throttling, disk and battery life'], ['Browse hardware and software inventory'], ['Use Extras; see network changes']],
});
useCases('uml-use-cases-setup', {
  title: 'Use cases — run and set up', desc: 'What the PC owner runs or sets up. Benchmarks start only on a click; Explain this (Pitwall) is still planned. Power and access are in their own diagram.', owner: 'PC owner',
  rows: [['Acknowledge, mute or note an alarm'], ['Run a benchmark and compare'], ['Explain this in Pitwall (redacted summary)', true], ['Save the flight recorder'], ['Map the disk and drill into folders'], ['Export a diagnostics bundle or the inventory'], ['Search the web or a hash in the browser'], ['Use the dyno command line'], ['Set tray metric, theme, sampling'], ['Start the agent at login']],
  ext: { name: 'OS (login)', row: 9 },
});

useCases('uml-use-cases-power', {
  title: 'Use cases — power and access', desc: 'What the PC owner turns on or asks Dyno to do. An AI client on this PC may keep it awake or wake a saved computer only if the owner allows each action.', owner: 'PC owner',
  rows: [['Sleep, hibernate (Windows, Linux) or shut down on idle or a timer'], ['Schedule a wake of this computer'], ['Turn on the administrator helper'], ['Turn on AI access for a local AI app'], ['Keep the computer awake'], ['Wake another computer (Wake-on-LAN)']],
  ext: { name: 'AI client (if allowed)', rows: [4, 5] },
});
