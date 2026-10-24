import { readdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

// Review 2 (N-P2-3) and design guide §6: no icon is drawn below 14 px (a 1.5 px stroke at 12 px merges "!" and "i" with their
// frames). This reads every template and stylesheet of the library, the shell, the gallery and the shell demo.
const projects = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'projects');

function* sources(dir: string): Generator<string> {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) {
      if (!['node_modules', 'css'].includes(name)) yield* sources(p);
    } else if (/\.(ts|scss|css)$/.test(name) && !/\.spec\.ts$/.test(name)) yield p;
  }
}

describe('icon size', () => {
  const files = [...sources(projects)].map((p) => [p, readFileSync(p, 'utf8')] as const);

  it('no pd-glyph is given a size under 14 px', () => {
    const small: string[] = [];
    for (const [p, text] of files) for (const m of text.matchAll(/<pd-glyph\b[^>]*\[?size\]?="(\d+)"/g)) if (Number(m[1]) < 14) small.push(`${p}: ${m[0]}`);
    expect(small).toEqual([]);
  });

  it('no stylesheet sizes a pd-glyph or mat-icon under 14 px', () => {
    const small: string[] = [];
    for (const [p, text] of files) for (const m of text.matchAll(/(?:pd-glyph|\.mat-icon)[^{}]*\{[^}]*\b(?:width|height):\s*(\d+)px/g)) if (Number(m[1]) < 14) small.push(`${p}: ${m[0].slice(0, 80)}`);
    expect(small).toEqual([]);
  });
});
