// Compiles the SCSS entry points to plain CSS in projects/paddock-ui/css (shipped next to the SCSS sources).
import { compile } from 'sass';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { stampBuild } from '../projects/paddock-e2e/src/build-guard.ts';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', 'projects', 'paddock-ui');
const out = join(root, 'css');
mkdirSync(out, { recursive: true });
for (const name of ['tokens', 'paddock', 'controls', 'material']) {
  const { css } = compile(join(root, 'scss', `entry-${name}.scss`), { style: 'expanded', loadPaths: [join(root, '..', '..', 'node_modules'), join(root, '..', '..', '..', 'node_modules')] });
  // Unchanged css is left alone (its mtime stays), and the stamp tells the e2e guard which sources it was built from.
  const file = join(out, `${name}.css`);
  if (!existsSync(file) || readFileSync(file, 'utf8') !== css) writeFileSync(file, css);
}
stampBuild(join(root, '..', '..'), 'css');
console.log('css built');
