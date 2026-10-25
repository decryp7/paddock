import { P, save, link } from './common.mjs';
import { ld } from '../../../../../diagrams/tools/lib.mjs';

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
function ellipse(d, id, x, y, w, h, text) {
  d.rect[id] = [x, y, w, h];
  const a = d.t.accent;
  d.raw(`<mxCell id="${id}" value="${esc(`<span style="font-size:12px">${esc(text)}</span>`)}" style="${d.base(`ellipse;strokeWidth=1.5;strokeColor=${ld(a.s)};fillColor=${ld(a.f)};align=center;verticalAlign=middle;spacingLeft=24;spacingRight=24;`)}" vertex="1" parent="1"><mxGeometry x="${x}" y="${y}" width="${w}" height="${h}" as="geometry"/></mxCell>`);
}
function actor(d, id, x, y, name) {
  d.rect[id] = [x, y, 40, 64];
  d.raw(`<mxCell id="${id}" value="${esc(`<span style="font-size:12px;font-weight:600">${esc(name)}</span>`)}" style="${d.base(`shape=umlActor;verticalLabelPosition=bottom;verticalAlign=top;strokeWidth=1.5;strokeColor=${ld(d.t.process.s)};fillColor=none;whiteSpace=nowrap;`)}" vertex="1" parent="1"><mxGeometry x="${x}" y="${y}" width="40" height="64" as="geometry"/></mxCell>`);
}
/** rows: texts for column 1; sat: [{ text, row, kind }] satellites in column 2 fanning out of `hubRow`. */
export function useCases(file, { title, desc, bound, owner, rows, hubRow, sats, ext }) {
  const UW = 320, UH = 56, P_ = 80, y0 = 144, bx = 232;
  const sw = sats.length ? 104 + 280 : 0;
  const bw = 24 + UW + sw + 24, bh = rows.length * P_ + 40;
  const d = P({ title, kind: 'UML · Use case diagram', desc, w: bx + bw + (ext ? 120 + 80 : 0) + 48 });
  d.group('b', bx, y0, bw, bh, bound);
  const cy = (i) => y0 + 40 + i * P_ + UH / 2;
  rows.forEach((t, i) => ellipse(d, `u${i}`, bx + 24, y0 + 40 + i * P_, UW, UH, t));
  const mid = (cy(0) + cy(rows.length - 1)) / 2;
  actor(d, 'a', 88, Math.round(mid - 32), owner);
  const trunk = bx - 56;
  rows.forEach((_, i) => d.edge('a', `u${i}`, [128, Math.round(mid)], [bx + 24, cy(i)], { arrow: 'none', via: Math.round(mid) === cy(i) ? [] : [[trunk, Math.round(mid)], [trunk, cy(i)]] }));
  const sx = bx + 24 + UW + 104;
  const gapX = bx + 24 + UW + 52;
  sats.forEach((s, i) => {
    ellipse(d, `s${i}`, sx, y0 + 40 + s.row * P_, 280, UH, s.text);
    d.edge(`u${hubRow}`, `s${i}`, [bx + 24 + UW, cy(hubRow)], [sx, cy(s.row)], { dash: true, arrow: 'open', via: cy(hubRow) === cy(s.row) ? [] : [[gapX, cy(hubRow)], [gapX, cy(s.row)]] });
    d.lbl(s.kind, gapX + 4, (cy(hubRow) + cy(s.row)) / 2, 'r');
  });
  if (ext) {
    actor(d, 'x', bx + bw + 120, Math.round(cy(ext.row) - 32), ext.name);
    d.edge('x', `u${ext.row}`, [bx + bw + 120, Math.round(cy(ext.row))], [bx + bw - 24, cy(ext.row)], { arrow: 'none' });
  }
  d.legend([{ kind: 'accent', label: 'Use case' }, { kind: 'line', label: 'Association' }, ...(sats.length ? [{ kind: 'dashline', label: '«include» / «extend»' }] : [])]);
  save(file, d, y0 + bh);
}
