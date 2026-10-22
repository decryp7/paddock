// Compiles the SCSS entry points to plain CSS in projects/paddock-ui/css (shipped next to the SCSS sources).
import { compile } from 'sass';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', 'projects', 'paddock-ui');
const out = join(root, 'css');
mkdirSync(out, { recursive: true });
for (const name of ['tokens', 'paddock']) {
  const { css } = compile(join(root, 'scss', `entry-${name}.scss`), { style: 'expanded' });
  writeFileSync(join(out, `${name}.css`), css);
}
console.log('css built');
