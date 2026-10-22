import { Diagram, link, g8 } from '../../../../paddock/diagrams/tools/lib.mjs';
import { sequence, dot, cls } from '../../../../paddock/diagrams/tools/uml.mjs';
import { writeFileSync } from 'node:fs';
export { link, g8, sequence, dot, cls };
/** Where the .drawio sources and exports live: dyno/diagrams (the docs link there). */
export const OUT = new URL('../../../diagrams', import.meta.url).pathname.replace(/\/$/, '');

const mix = (hex, bg, a) => {
  const q = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
  const [c, b] = [q(hex), q(bg)];
  return '#' + c.map((v, i) => Math.round(b[i] + (v - b[i]) * a).toString(16).padStart(2, '0')).join('').toUpperCase();
};

class DynoDiagram extends Diagram {
  constructor(o) {
    super(o);
    // The kit's plugin colour (#B0508A) is within 30 degrees of Dyno's magenta accent: use the next free chart colour (sky).
    this.t.plugin = { s: ['#0072B2', '#56B4E9'], f: [mix('#0072B2', '#FFFFFF', 0.1), mix('#56B4E9', '#15171B', 0.16)] };
  }
  /** Like the kit's node, but warns when the text does not fit the given height. */
  node(id, x, y, w, h, role, o = {}) {
    if (h != null) {
      const need = this.fit(w, { name: o.name, tech: o.tech, desc: o.desc });
      if (h < need) console.warn(`node ${id} (${o.name}) is ${h}px tall but its text needs ${need}px`);
    }
    return super.node(id, x, y, w, h, role, o);
  }
  finish(bottom) {
    const xml = super.finish(bottom);
    const right = Math.max(0, ...Object.values(this.rect).map(([x, , w]) => x + w)) + 48;
    if (this.h > 1200 && !this.tall) console.warn(`${this.title}: ${this.h}px tall (limit 1200)`);
    if (right > 1600) console.warn(`${this.title}: ${right}px wide (limit 1600)`);
    return xml;
  }
}
export const P = (o) => new DynoDiagram({ app: 'dyno', date: '2026-10-23', ...o });
export const save = (name, d, bottom) => writeFileSync(`${OUT}/${name}.drawio`, d.finish(bottom));
/** Port fraction on rect `hub` for the vertical centre of rect `other`. */
export function tOf(d, hub, other, axis = 1) {
  const [x, y, w, h] = d.rect[hub], [ox, oy, ow, oh] = d.rect[other];
  return axis === 1 ? (oy + oh / 2 - y) / h : (ox + ow / 2 - x) / w;
}
/** Straight link from a tall hub to a node beside it: hub side sh, node side sn, hub-end fraction from the node's centre. */
export function hub(d, h, sh, n, sn, opts = {}) {
  const t = tOf(d, h, n);
  const [a, b] = opts.rev ? [n, h] : [h, n];
  const [sa, sb] = opts.rev ? [sn, sh] : [sh, sn];
  const ta = opts.rev ? 0.5 : t, tb = opts.rev ? t : 0.5;
  const rest = { ...opts }; delete rest.rev;
  return link(d, a, sa, b, sb, { ...rest, t: [ta, tb] });
}
/** Two parallel straight horizontal edges between adjacent boxes: a -> b (upper) with label f, b -> a (lower) with label r. */
export function pair(d, a, b, f, r, o = {}) {
  const [x, y, w, h] = d.rect[a], [bx] = d.rect[b];
  const t1 = o.t1 ?? 0.3, t2 = o.t2 ?? 0.7;
  const y1 = Math.round(y + h * t1), y2 = Math.round(y + h * t2);
  const xa = x + w, xb = bx;
  d.edge(a, b, [xa, y1], [xb, y1], {});
  d.edge(b, a, [xb, y2], [xa, y2], {});
  if (f) d.lbl(f, (xa + xb) / 2, y1, 'a', { maxw: xb - xa - 16 });
  if (r) d.lbl(r, (xa + xb) / 2, y2, 'b', { maxw: xb - xa - 16 });
}
