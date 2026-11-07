// Minimal draw.io (mxGraph XML) builder that applies STYLE.md: tokens, type scale, 8 px grid, pinned orthogonal edges, labels beside lines.
import { theme } from './themes.mjs';

export const FONT = '-apple-system,BlinkMacSystemFont,Segoe UI,system-ui,sans-serif';
export const MONO = 'ui-monospace,SF Mono,Cascadia Mono,Consolas,monospace';
export const ld = ([l, d]) => `light-dark(${l},${d})`;
const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
export const g8 = (n) => Math.ceil(n / 8) * 8;

// ---- text measurement (estimates; draw.io wraps the real text) ----
function cw(ch, mono) {
  if (mono) return 0.6;
  if ('il.,;:\'|!1()[]/ '.includes(ch)) return ch === ' ' ? 0.28 : 0.3;
  if ('mwMW@%'.includes(ch)) return 0.86;
  if (/[A-Z]/.test(ch)) return 0.64;
  return 0.54;
}
/** Segments of a `markdown-lite` string: backticks make monospace runs. */
function segs(text) {
  return String(text).split('`').map((t, i) => ({ t, mono: i % 2 === 1 })).filter((s) => s.t);
}
export function tw(text, size = 12, bold = false) {
  let w = 0;
  for (const s of segs(text)) for (const ch of s.t) w += cw(ch, s.mono) * size;
  return Math.ceil(w * (bold ? 1.07 : 1) * 1.08);
}
export function wrap(text, size, maxw, bold = false) {
  const out = [];
  let line = '';
  for (const word of String(text).split(/ +/)) {
    const cand = line ? `${line} ${word}` : word;
    if (line && tw(cand, size, bold) > maxw) { out.push(line); line = word; } else line = cand;
  }
  if (line) out.push(line);
  return out;
}
const html = (text, mutedStyle = '') =>
  segs(text).map((s) => (s.mono ? `<span style="font-family:${MONO};white-space:nowrap">${esc(s.t)}</span>` : esc(s.t))).join('') + mutedStyle;

// ---- diagram ----
export class Diagram {
  constructor({ title = '', kind = '', desc = '', w = 800, app = 'kit', date = new Date().toISOString().slice(0, 10), bare = false }) {
    this.t = theme(app);
    this.w = w;
    this.title = title;
    this.date = date;
    this.cells = [];
    this.rect = {};
    this.n = 10;
    this.legendItems = [];
    if (bare) return;
    this.text('title', [48, 40, w - 96, 32], title, { size: 24, weight: 600 });
    this.text('kind', [48, 72, w - 96, 24], kind, { size: 14, color: 'muted' });
    this.text('desc', [48, 96, w - 96, 20], desc, { size: 12, color: 'muted' });
  }
  id() { return `c${this.n++}`; }
  raw(xml) { this.cells.push(xml); }
  base(extra = '') { return `html=1;whiteSpace=wrap;fontFamily=${FONT};fontSize=12;fontColor=${ld(this.t.text)};${extra}`; }

  text(id, [x, y, w, h], text, { size = 12, weight = 400, color = 'text', align = 'left', valign = 'top' } = {}) {
    const col = color === 'text' ? this.t.text : this.t.muted;
    const v = `<span style="font-size:${size}px;font-weight:${weight}">${html(text)}</span>`;
    this.raw(`<mxCell id="${id}" value="${esc(v)}" style="${this.base(`text;strokeColor=none;fillColor=none;align=${align};verticalAlign=${valign};spacing=0;fontColor=${ld(col)};`)}" vertex="1" parent="1"><mxGeometry x="${x}" y="${y}" width="${w}" height="${h}" as="geometry"/></mxCell>`);
  }

  /** A text label placed beside a line. anchor: where the label sits relative to point (x, y): r, l, a (above), b (below). */
  lbl(text, x, y, anchor = 'r', { maxw = 200, color = 'muted' } = {}) {
    const lines = wrap(text, 12, maxw);
    const w = Math.max(...lines.map((l) => tw(l, 12))) + 8;
    const h = lines.length * 16 + 2;
    const pos = { r: [x + 8, y - h / 2], l: [x - 8 - w, y - h / 2], a: [x - w / 2, y - h - 6], b: [x - w / 2, y + 6], ar: [x + 6, y - h - 4], al: [x - w - 6, y - h - 4], br: [x + 6, y + 4], bl: [x - w - 6, y + 4] }[anchor];
    this.text(this.id(), [Math.round(pos[0]), Math.round(pos[1]), w, h], lines.join(' '), { color, align: anchor.endsWith('l') || anchor === 'l' ? 'right' : anchor === 'r' || anchor.endsWith('r') ? 'left' : 'center' });
    return { x: pos[0], y: pos[1], w, h };
  }

  styleFor(role, { planned = false, sw = 1.5, arc = 6 } = {}) {
    const r = this.t[role];
    const dash = role === 'external' || planned ? 'dashed=1;dashPattern=6 4;' : '';
    const fill = planned ? ld(this.t.white) : ld(r.f);
    return `rounded=1;absoluteArcSize=1;arcSize=${arc};strokeWidth=${sw};strokeColor=${ld(r.s)};fillColor=${fill};${dash}`;
  }

  /** Height a node needs for its text at width w. */
  fit(w, { name, tech, desc, minH = 56 }) {
    const inner = w - 28;
    let h = 12 + Math.ceil(wrap(name, 14, inner, true).length * 17.5);
    if (tech) h += Math.ceil(wrap(`[${tech}]`, 12, inner).length * 15);
    if (desc) h += 4 + Math.ceil(wrap(desc, 12, inner).length * 15);
    return Math.max(minH, g8(h + 12));
  }

  /** A labelled box. h omitted: sized to content. role: accent | core | ui | plugin | process | external | person. */
  node(id, x, y, w, h, role, { name, tech, desc, planned = false, person = false } = {}) {
    const r = person ? 'process' : role;
    if (h == null) h = this.fit(w, { name, tech, desc });
    let v = `<span style="font-size:14px;font-weight:600">${html(name)}</span>`;
    if (tech) v += `<br><span style="font-size:12px;color:${ld(this.t.muted)}">${html(`[${tech}]`)}</span>`;
    if (desc) v += `<br><div style="margin-top:4px;font-size:12px">${html(desc)}</div>`;
    this.rect[id] = [x, y, w, h];
    this.raw(`<mxCell id="${id}" value="${esc(v)}" style="${this.base(`${this.styleFor(r, { planned })}align=left;verticalAlign=top;spacingLeft=14;spacingRight=14;spacingTop=10;spacingBottom=8;`)}" vertex="1" parent="1"><mxGeometry x="${x}" y="${y}" width="${w}" height="${h}" as="geometry"/></mxCell>`);
    if (person) this.glyph(x + w - 32, y + 10);
    if (planned) this.tag(x + w - 8, y + 8, 'planned', role);
    return [x, y, w, h];
  }
  /** Person glyph (stick figure, 14 x 20), top-left at (x, y). */
  glyph(x, y) {
    this.raw(`<mxCell id="${this.id()}" value="" style="shape=umlActor;verticalLabelPosition=bottom;verticalAlign=top;strokeWidth=1.5;strokeColor=${ld(this.t.process.s)};fillColor=none;" vertex="1" parent="1"><mxGeometry x="${x}" y="${y}" width="14" height="20" as="geometry"/></mxCell>`);
  }
  /** "planned" pill, right edge at x. */
  tag(xr, y, text = 'planned', role = 'accent') {
    const w = tw(text, 11, true) + 14;
    const r = this.t[role] || this.t.accent;
    this.raw(`<mxCell id="${this.id()}" value="${esc(`<span style="font-size:11px;font-weight:600">${esc(text)}</span>`)}" style="${this.base(`rounded=1;absoluteArcSize=1;arcSize=10;strokeWidth=1;strokeColor=${ld(r.s)};fillColor=${ld(this.t.white)};fontColor=${ld(r.s)};align=center;verticalAlign=middle;spacing=0;`)}" vertex="1" parent="1"><mxGeometry x="${xr - w}" y="${y}" width="${w}" height="18" as="geometry"/></mxCell>`);
  }

  /** Boundary group. os: a real OS-process boundary, drawn dashed. Registered as a rect so edges can attach. */
  group(id, x, y, w, h, label, { os = false, arc = 10 } = {}) {
    const g = this.t.group;
    this.rect[id] = [x, y, w, h];
    const v = `<span style="font-size:12px;font-weight:600">${html(label)}</span>`;
    this.raw(`<mxCell id="${id}" value="${esc(v)}" style="${this.base(`rounded=1;absoluteArcSize=1;arcSize=${arc};strokeWidth=1;strokeColor=${ld(g.s)};fillColor=${ld(g.f)};${os ? 'dashed=1;dashPattern=6 4;' : ''}align=left;verticalAlign=top;spacingLeft=14;spacingTop=8;fontColor=${ld(this.t.muted)};`)}" vertex="1" parent="1"><mxGeometry x="${x}" y="${y}" width="${w}" height="${h}" as="geometry"/></mxCell>`);
  }

  /** A note: text in a quiet box, sized to the text. */
  note(id, x, y, w, text, h) {
    const lines = wrap(text, 12, w - 28);
    h = h ?? g8(lines.length * 15 + 24);
    this.rect[id] = [x, y, w, h];
    this.raw(`<mxCell id="${id}" value="${esc(`<span style="font-size:12px">${html(text)}</span>`)}" style="${this.base(`rounded=1;absoluteArcSize=1;arcSize=6;strokeWidth=1;strokeColor=${ld(this.t.group.s)};fillColor=${ld(this.t.process.f)};align=left;verticalAlign=middle;spacingLeft=14;spacingRight=14;`)}" vertex="1" parent="1"><mxGeometry x="${x}" y="${y}" width="${w}" height="${h}" as="geometry"/></mxCell>`);
    return h;
  }

  /** Absolute point on a registered rect: side T/B/L/R, t = 0..1 along that side. */
  port(id, side, t = 0.5) {
    const [x, y, w, h] = this.rect[id];
    return { T: [x + w * t, y], B: [x + w * t, y + h], L: [x, y + h * t], R: [x + w, y + h * t] }[side].map(Math.round);
  }

  /**
   * Orthogonal rounded edge between two registered rects, pinned at absolute points a and b (on their borders).
   * via: waypoints, already orthogonal (each shares x or y with its neighbour).
   */
  edge(from, to, a, b, { via = [], dash = false, arrow = 'block', start = 'none', id = this.id() } = {}) {
    const pin = (rid, [px, py], p) => {
      const [x, y, w, h] = this.rect[rid];
      const fx = ((px - x) / w).toFixed(4), fy = ((py - y) / h).toFixed(4);
      return `${p}X=${fx};${p}Y=${fy};${p}Dx=0;${p}Dy=0;${p}Perimeter=0;`;
    };
    const col = ld(this.t.edge);
    const st = `edgeStyle=orthogonalEdgeStyle;rounded=1;arcSize=8;jettySize=auto;orthogonalLoop=1;html=1;strokeWidth=1.5;strokeColor=${col};endArrow=${arrow};endFill=${arrow === 'block' || arrow.startsWith('diamond') ? 1 : 0};endSize=6;startArrow=${start};startFill=${start.startsWith('diamond') ? 1 : 0};startSize=8;${dash ? 'dashed=1;dashPattern=6 4;' : ''}${pin(from, a, 'exit')}${pin(to, b, 'entry')}`;
    const pts = via.map(([x, y]) => `<mxPoint x="${x}" y="${y}"/>`).join('');
    this.raw(`<mxCell id="${id}" value="" style="${st}" edge="1" parent="1" source="${from}" target="${to}"><mxGeometry relative="1" as="geometry">${pts ? `<Array as="points">${pts}</Array>` : ''}</mxGeometry></mxCell>`);
    return id;
  }
  /** A free line (no endpoints on shapes): sequence messages, lifelines. */
  line(a, b, { dash = false, arrow = 'block', via = [], width = 1.5, color = this.t.edge, rounded = true } = {}) {
    const col = ld(color);
    const st = `edgeStyle=none;rounded=${rounded ? 1 : 0};html=1;strokeWidth=${width};strokeColor=${col};endArrow=${arrow};endFill=${arrow === 'block' ? 1 : 0};endSize=6;${dash ? 'dashed=1;dashPattern=6 4;' : ''}`;
    const pts = via.map(([x, y]) => `<mxPoint x="${x}" y="${y}"/>`).join('');
    this.raw(`<mxCell id="${this.id()}" value="" style="${st}" edge="1" parent="1"><mxGeometry relative="1" as="geometry"><mxPoint x="${a[0]}" y="${a[1]}" as="sourcePoint"/><mxPoint x="${b[0]}" y="${b[1]}" as="targetPoint"/>${pts ? `<Array as="points">${pts}</Array>` : ''}</mxGeometry></mxCell>`);
  }

  legend(items) { this.legendItems = items; }

  /** Adds legend and footer below contentBottom and returns the XML document. */
  finish(contentBottom) {
    const y = g8(contentBottom + 40);
    if (this.legendItems.length) {
      let x = 64;
      const parts = [];
      for (const it of this.legendItems) {
        const lw = tw(it.label, 12) + 4, sw = it.kind === 'tag' ? tw('planned', 11, true) + 14 : 28; // swatch width
        parts.push({ it, x, lw, sw });
        x += sw + 8 + lw + 24;
      }
      const total = x - 24 - 64 + 32;
      this.raw(`<mxCell id="legend" value="" style="rounded=1;absoluteArcSize=1;arcSize=14;strokeWidth=1;strokeColor=${ld(this.t.group.s)};fillColor=none;" vertex="1" parent="1"><mxGeometry x="48" y="${y}" width="${total}" height="40" as="geometry"/></mxCell>`);
      for (const { it, x: lx, lw, sw } of parts) {
        const cy = y + 12;
        if (it.kind === 'line' || it.kind === 'dashline') this.line([lx, cy + 8], [lx + 28, cy + 8], { dash: it.kind === 'dashline', arrow: it.arrow ?? 'block' });
        else if (it.kind === 'tag') this.tag(lx + sw, cy - 1, 'planned', it.role); // role: the stroke of the tagged nodes
        else {
          const planned = it.kind === 'planned';
          this.raw(`<mxCell id="${this.id()}" value="" style="${it.ellipse ? 'ellipse;' : ''}${this.styleFor(planned ? 'accent' : it.kind, { planned, sw: 1.5, arc: 4 })}" vertex="1" parent="1"><mxGeometry x="${lx}" y="${cy}" width="28" height="16" as="geometry"/></mxCell>`);
        }
        this.text(this.id(), [lx + sw + 8, cy - 1, lw, 18], it.label, { color: 'muted' });
      }
    }
    const right = Math.max(0, ...Object.values(this.rect).map(([x, , w]) => x + w));
    this.text('footer', [Math.max(right, 400) - 360, y + 11, 360, 18], `${this.t.app} · updated ${this.date}`, { size: 11, color: 'muted', align: 'right' });
    this.h = y + 40 + 48;
    return this.xml();
  }

  /** Just the mxGraphModel, for a draw.io shape library entry. */
  fragment() {
    return `<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/>${this.cells.join('')}</root></mxGraphModel>`;
  }

  xml() {
    return `<mxfile host="drawio" version="32.5.5" compressed="false">
  <diagram id="${esc(this.title)}" name="${esc(this.title)}">
    <mxGraphModel dx="0" dy="0" grid="1" gridSize="8" guides="1" tooltips="1" connect="1" arrows="1" fold="1" page="0" math="0" shadow="0" background="none">
      <root>
        <mxCell id="0"/>
        <mxCell id="1" parent="0"/>
${this.cells.map((c) => '        ' + c).join('\n')}
      </root>
    </mxGraphModel>
  </diagram>
</mxfile>
`;
  }
}

/**
 * Edge between two registered rects from side sa to side sb. Opposite sides facing each other are joined with a straight line
 * through the middle of the overlap; otherwise the centres of the sides are used (t = [ta, tb] overrides). label: placed beside
 * the line (above a horizontal one, right of a vertical one).
 */
export function link(d, a, sa, b, sb, opts = {}) {
  const { t, label, via, lw, at = 0.5, ...rest } = opts;
  let p = d.port(a, sa, t?.[0] ?? 0.5), q = d.port(b, sb, t?.[1] ?? 0.5);
  const ra = d.rect[a], rb = d.rect[b];
  if (!t && !via) {
    const ov = (i, n) => [Math.max(ra[i], rb[i]), Math.min(ra[i] + ra[n], rb[i] + rb[n])];
    if ((sa === 'R' && sb === 'L') || (sa === 'L' && sb === 'R')) {
      const [lo, hi] = ov(1, 3);
      if (hi > lo) { const y = Math.round((lo + hi) / 2); p = [p[0], y]; q = [q[0], y]; }
    } else if ((sa === 'B' && sb === 'T') || (sa === 'T' && sb === 'B')) {
      const [lo, hi] = ov(0, 2);
      if (hi > lo) { const x = Math.round((lo + hi) / 2); p = [x, p[1]]; q = [x, q[1]]; }
    }
  }
  const id = d.edge(a, b, p, q, { via: via ?? [], ...rest });
  if (label) {
    if (p[1] === q[1] && !via) {
      const room = Math.abs(q[0] - p[0]);
      if (tw(label, 12) + 16 > room) console.warn(`label "${label}" is wider than its ${room}px gap (${a} -> ${b})`);
      d.lbl(label, (p[0] + q[0]) / 2, p[1], 'a', { maxw: Math.max(room - 16, 60) });
    } else if (p[0] === q[0] && !via) d.lbl(label, p[0], p[1] + (q[1] - p[1]) * at, 'r');
    else if (lw) d.lbl(label, lw[0], lw[1], lw[2] ?? 'r');
  }
  return id;
}
