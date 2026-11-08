// After `ng build paddock-ui`: gives each secondary entry's folder package.json a `main`, next to the `module` that
// ng-packagr writes. Angular's builders read `module`; Node's resolver (Playwright's loader, which follows the apps'
// tsconfig `paths` to `ui/dist/paddock-ui/<entry>`) needs `main` or it reports "Cannot find module" for a folder that
// has no index file. Node >= 22.12 requires the .mjs target directly.
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

/** Adds `main` (= `module`) to every folder package.json under `dist` that lacks it; returns the files changed. */
export function addEntryMains(dist) {
  const changed = [];
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (!statSync(path).isDirectory() || name === 'fesm2022' || name === 'types') continue;
      const file = join(path, 'package.json');
      try {
        const pkg = JSON.parse(readFileSync(file, 'utf8'));
        if (typeof pkg.module === 'string' && pkg.main === undefined) {
          writeFileSync(file, `${JSON.stringify({ main: pkg.module, ...pkg }, null, 2)}\n`);
          changed.push(file);
        }
      } catch (e) {
        if (e.code !== 'ENOENT') throw e;
      }
      walk(path);
    }
  };
  walk(dist);
  return changed;
}
