import { Diagram, link, g8 } from '../../../../paddock/diagrams/tools/lib.mjs';
import { sequence, dot, cls } from '../../../../paddock/diagrams/tools/uml.mjs';
import { writeFileSync } from 'node:fs';
export { link, g8, sequence, dot, cls };
export const OUT = new URL('..', import.meta.url).pathname.replace(/\/$/, '');
export const P = (o) => new Diagram({ app: 'spotter', date: '2026-10-23', ...o });
export const save = (name, d, bottom) => writeFileSync(`${OUT}/${name}.drawio`, d.finish(bottom));
/** Port fraction on rect `id` for the vertical centre of rect `other` (for straight links from a tall hub). */
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
