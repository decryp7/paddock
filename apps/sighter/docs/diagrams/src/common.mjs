import { Diagram, link, g8 } from '../../../../paddock/diagrams/tools/lib.mjs';
import { sequence, dot, cls } from '../../../../paddock/diagrams/tools/uml.mjs';
import { writeFileSync } from 'node:fs';
export { link, g8, sequence, dot, cls };
export const OUT = new URL('..', import.meta.url).pathname.replace(/\/$/, '');
export const P = (o) => new Diagram({ app: 'sighter', date: '2026-10-23', ...o });
export const save = (name, d, bottom) => writeFileSync(`${OUT}/${name}.drawio`, d.finish(bottom));
