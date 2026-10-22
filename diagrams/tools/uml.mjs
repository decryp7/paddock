// UML helpers on top of lib.mjs: sequence diagrams, state machines, class boxes.
import { ld, tw, wrap, g8, MONO } from './lib.mjs';

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

/** Initial (kind 'start'), final ('end') pseudo-states and a choice diamond ('choice'); (x, y) is the top-left. */
export function dot(d, id, x, y, kind = 'start') {
  const c = ld(d.t.edge);
  if (kind === 'start') {
    d.rect[id] = [x, y, 16, 16];
    d.raw(`<mxCell id="${id}" value="" style="ellipse;strokeWidth=1.5;strokeColor=${c};fillColor=${c};" vertex="1" parent="1"><mxGeometry x="${x}" y="${y}" width="16" height="16" as="geometry"/></mxCell>`);
  } else if (kind === 'end') {
    d.rect[id] = [x, y, 24, 24];
    d.raw(`<mxCell id="${id}" value="" style="ellipse;strokeWidth=1.5;strokeColor=${c};fillColor=none;" vertex="1" parent="1"><mxGeometry x="${x}" y="${y}" width="24" height="24" as="geometry"/></mxCell>`);
    d.raw(`<mxCell id="${d.id()}" value="" style="ellipse;strokeWidth=0;fillColor=${c};" vertex="1" parent="1"><mxGeometry x="${x + 6}" y="${y + 6}" width="12" height="12" as="geometry"/></mxCell>`);
  } else {
    d.rect[id] = [x, y, 32, 32];
    d.raw(`<mxCell id="${id}" value="" style="rhombus;strokeWidth=1.5;strokeColor=${c};fillColor=${ld(d.t.white)};" vertex="1" parent="1"><mxGeometry x="${x}" y="${y}" width="32" height="32" as="geometry"/></mxCell>`);
  }
}

/** UML class box: header (with optional stereotype), attribute and operation compartments in monospace. Returns its height. */
export function cls(d, id, x, y, w, { name, stereotype, attrs = [], ops = [], role = 'accent' }) {
  const lh = 16;
  const head = stereotype ? 48 : 32;
  const ha = attrs.length ? attrs.length * lh + 16 : 0;
  const ho = ops.length ? ops.length * lh + 16 : 0;
  const h = g8(head + ha + ho);
  d.rect[id] = [x, y, w, h];
  const r = d.t[role];
  const hv = (stereotype ? `<span style="font-size:12px;color:${ld(d.t.muted)}">«${esc(stereotype)}»</span><br>` : '') + `<span style="font-size:14px;font-weight:600;font-family:${MONO}">${esc(name)}</span>`;
  d.raw(`<mxCell id="${id}" value="${esc(hv)}" style="${d.base(`${d.styleFor(role)}align=center;verticalAlign=top;spacingTop=${stereotype ? 6 : 6};`)}" vertex="1" parent="1"><mxGeometry x="${x}" y="${y}" width="${w}" height="${h}" as="geometry"/></mxCell>`);
  const comp = (items, cy, n) => {
    d.raw(`<mxCell id="${d.id()}" value="" style="endArrow=none;html=1;strokeWidth=1;strokeColor=${ld(r.s)};" edge="1" parent="1"><mxGeometry relative="1" as="geometry"><mxPoint x="${x}" y="${cy}" as="sourcePoint"/><mxPoint x="${x + w}" y="${cy}" as="targetPoint"/></mxGeometry></mxCell>`);
    const v = items.map((t) => esc(t).replace(/ /g, '&nbsp;')).join('<br>');
    d.raw(`<mxCell id="${d.id()}" value="${esc(`<span style="font-family:${MONO};font-size:12px">${v}</span>`)}" style="${d.base('text;strokeColor=none;fillColor=none;align=left;verticalAlign=top;spacingLeft=12;spacingTop=8;')}" vertex="1" parent="1"><mxGeometry x="${x}" y="${cy}" width="${w}" height="${n * lh + 16}" as="geometry"/></mxCell>`);
  };
  if (attrs.length) comp(attrs, y + head, attrs.length);
  if (ops.length) comp(ops, y + head + ha, ops.length);
  return h;
}

/**
 * Sequence diagram. parts: [{ id, name, tech, role }]. Returns an object whose methods advance a cursor downwards;
 * call done() last: it draws the lifelines behind everything and returns the bottom y.
 */
export function sequence(d, parts, { top = 144, pw = 160, gap = 40, x0 = 48 } = {}) {
  const px = (i) => x0 + i * (pw + gap) + pw / 2;
  const idx = Object.fromEntries(parts.map((p, i) => [p.id, i]));
  const headH = Math.max(...parts.map((p) => d.fit(pw, { name: p.name, tech: p.tech })));
  const behind = d.cells.length;
  parts.forEach((p, i) => d.node(p.id, x0 + i * (pw + gap), top, pw, headH, p.role || 'accent', { name: p.name, tech: p.tech }));
  let y = top + headH + 32;
  const xl = x0 - 16, xr = x0 + parts.length * (pw + gap) - gap + 16;
  const open = [];
  const gaps = [];
  const S = {
    get y() { return y; },
    msg(a, b, label, { reply = false } = {}) {
      const xa = px(idx[a]), xb = px(idx[b]);
      const room = Math.max(Math.abs(xb - xa) - 16, 144);
      const lines = wrap(label, 12, room);
      const lw = Math.max(...lines.map((l) => tw(l, 12))) + 8, lh = lines.length * 16;
      const ly = y + lh + 8;
      const lx = Math.min(xa, xb) + 8;
      d.text(d.id(), [lx, y, lw, lh], lines.join(' '), {});
      gaps.push([lx - 4, lx + lw + 4, y, y + lh]);
      d.line([xa, ly], [xb, ly], { dash: reply, arrow: reply ? 'open' : 'block' });
      y = ly + 24;
    },
    self(p, label) {
      const x = px(idx[p]);
      const room = pw + gap - 44;
      const lines = wrap(label, 12, room);
      const lh = lines.length * 16;
      const h = Math.max(32, lh + 4);
      d.line([x, y], [x, y + h], { via: [[x + 24, y], [x + 24, y + h]] });
      const sw = Math.max(...lines.map((l) => tw(l, 12))) + 8;
      d.text(d.id(), [x + 32, y + (h - lh) / 2, sw, lh], lines.join(' '), {});
      gaps.push([x + 28, x + 36 + sw, y + (h - lh) / 2, y + (h + lh) / 2]);
      y += h + 24;
    },
    note(p, text, w = 280) {
      const h = d.note(d.id(), Math.round(px(idx[p]) - w / 2), y, w, text);
      y += h + 24;
    },
    open(kind, cond) { y += 8; open.push({ kind, cond, y0: y }); y += 32; },
    else(cond) {
      const o = open[open.length - 1];
      d.line([xl, y - 4], [xr, y - 4], { dash: true, arrow: 'none', width: 1, color: d.t.group.s, rounded: false });
      d.text(d.id(), [xl + 72, y, xr - xl - 80, 18], cond, { color: 'muted' });
      gaps.push([xl + 68, xl + 76 + tw(cond, 12) + 8, y, y + 18]);
      y += 32;
      o.has = true;
    },
    close() {
      const o = open.pop();
      const h = y - o.y0;
      d.raw(`<mxCell id="${d.id()}" value="" style="rounded=1;absoluteArcSize=1;arcSize=6;strokeWidth=1;strokeColor=${ld(d.t.edge)};fillColor=none;" vertex="1" parent="1"><mxGeometry x="${xl}" y="${o.y0}" width="${xr - xl}" height="${h}" as="geometry"/></mxCell>`);
      d.raw(`<mxCell id="${d.id()}" value="${esc(`<span style="font-size:11px;font-weight:600">${o.kind}</span>`)}" style="${d.base(`rounded=1;absoluteArcSize=1;arcSize=6;strokeWidth=1;strokeColor=${ld(d.t.edge)};fillColor=${ld(d.t.process.f)};align=center;verticalAlign=middle;spacing=0;`)}" vertex="1" parent="1"><mxGeometry x="${xl + 8}" y="${o.y0 + 6}" width="40" height="18" as="geometry"/></mxCell>`);
      d.text(d.id(), [xl + 56, o.y0 + 6, xr - xl - 64, 18], o.cond, { color: 'muted' });
      gaps.push([xl + 4, xl + 60 + tw(o.cond, 12) + 8, o.y0 + 4, o.y0 + 26]);
      y += 16;
    },
    done() {
      const n = d.cells.length;
      parts.forEach((p, i) => {
        const x = px(i);
        const cuts = gaps.filter((g) => x > g[0] && x < g[1]).sort((m, k) => m[2] - k[2]);
        let y0 = top + headH;
        for (const g of cuts) { if (g[2] > y0) d.line([x, y0], [x, g[2]], { dash: true, arrow: 'none', width: 1, color: d.t.edge }); y0 = Math.max(y0, g[3]); }
        if (y > y0) d.line([x, y0], [x, y], { dash: true, arrow: 'none', width: 1, color: d.t.edge });
      });
      const life = d.cells.splice(n);
      d.cells.splice(behind + parts.length, 0, ...life);
      return y;
    },
  };
  return S;
}
