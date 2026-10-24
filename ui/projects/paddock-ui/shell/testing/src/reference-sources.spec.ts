import { uiTimeoutProblems } from '@decryp7/paddock-ui/shell/jobs';
import { splashProblems } from '@decryp7/paddock-ui/shell/startup';
import { checkRegistrationSource, isRegistrationFile, lazyExportProblems } from './registration-lint';

// The static checks over the shell's and the reference app's sources, read from disk (vitest runs in ui/): lazy
// registration files (plan §9), lazy chunks that stay lazy (plan S16), no UI timeout (rule 12, plan §12) and the
// splash (ADR 0058), one writer of the look (rule 40, plan S15) and the pseudo-locale's coverage of the shell catalog.
// They ran in the e2e before plan S16; the e2e now uses only @decryp7/paddock-e2e.

/** Node's `fs`, loaded at run time (the spec programme has no Node types). */
interface NodeFs {
  readFileSync(path: string, encoding: 'utf8'): string;
  readdirSync(path: string): string[];
  statSync(path: string): { isDirectory(): boolean };
  existsSync(path: string): boolean;
}
const nodeFs = (): Promise<NodeFs> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as Promise<NodeFs>;

const SHELL = 'projects/paddock-ui/shell';
const DEMO = 'projects/shell-demo/src';

/** Every non-spec TypeScript file under `dir`; `stopAt` skips folders that are entry points of their own. */
function sources(fs: NodeFs, dir: string, stopAt = false): string[] {
  return fs.readdirSync(dir).flatMap((name) => {
    const path = `${dir}/${name}`;
    if (fs.statSync(path).isDirectory()) return stopAt && fs.existsSync(`${path}/ng-package.json`) ? [] : sources(fs, path, stopAt);
    return path.endsWith('.ts') && !path.endsWith('.spec.ts') ? [path] : [];
  });
}

/** Every entry point folder under `dir` (a folder with an ng-package.json), `dir` included. */
function entryPoints(fs: NodeFs, dir: string): string[] {
  const here = fs.existsSync(`${dir}/ng-package.json`) ? [dir] : [];
  const below = fs
    .readdirSync(dir)
    .map((name) => `${dir}/${name}`)
    .filter((path) => fs.statSync(path).isDirectory())
    .flatMap((path) => entryPoints(fs, path));
  return [...here, ...below];
}

describe('shell and shell-demo sources', () => {
  it('registration files import no store, service, command or component (lazy by construction, plan §9)', async () => {
    const fs = await nodeFs();
    const files = [...sources(fs, SHELL), ...sources(fs, DEMO)].filter(isRegistrationFile);
    expect(files).toContain(`${DEMO}/app/demo.contributions.ts`);
    expect(files.length).toBeGreaterThanOrEqual(4);
    // The service part's entry point and its generated wire module are named "service" but hold no service: the entry point
    // exports the page's definition and constants (the page, store and backend are chunks or tree-shaken), the module only types.
    const allow = ['@decryp7/paddock-ui/shell/service', '../generated/service', '@decryp7/paddock-ui/shell/channels', '../generated/channels'];
    expect(files.flatMap((f) => checkRegistrationSource(f, fs.readFileSync(f, 'utf8'), allow))).toEqual([]);
  });

  it('every entry point keeps its lazily loaded components out of its public API (plan S16)', async () => {
    const fs = await nodeFs();
    const entries = entryPoints(fs, SHELL);
    expect(entries).toContain(`${SHELL}/settings/window`);
    const problems = entries.flatMap((entry) => {
      const files = Object.fromEntries(sources(fs, entry, true).map((f) => [f, fs.readFileSync(f, 'utf8')]));
      return lazyExportProblems(files);
    });
    expect(problems).toEqual([]);
  });

  it('has no UI timeout anywhere (rule 12, plan §12)', async () => {
    const fs = await nodeFs();
    const files = [...sources(fs, SHELL), ...sources(fs, DEMO)];
    expect(files.length).toBeGreaterThan(20);
    expect(files.flatMap((f) => uiTimeoutProblems(f, fs.readFileSync(f, 'utf8')))).toEqual([]);
  });

  it('the demo page passes the splash check (ADR 0058)', async () => {
    const fs = await nodeFs();
    expect(splashProblems(fs.readFileSync(`${DEMO}/index.html`, 'utf8'), 'sd-root')).toEqual([]);
  });

  it('has one writer of the look: only the appearance store puts the appearance on <html> (rule 40)', async () => {
    const fs = await nodeFs();
    // The store applies the look (`applyAppearanceTo`); `applyStoredAppearance` paints the stored copy before Angular starts.
    const allowed = ['shell/src/theme/appearance.store.ts', 'shell/src/theme/provide-appearance.ts'];
    const writer = /applyPaddockAppearance\(|setAttribute\(\s*['"]data-(theme|density|contrast|motion|accent|pack)['"]/;
    const found = [...sources(fs, SHELL), ...sources(fs, DEMO)].filter((f) => writer.test(fs.readFileSync(f, 'utf8'))).map((f) => f.replace('projects/paddock-ui/', ''));
    expect(found.sort()).toEqual(allowed);
  });

  it('every message of the shell catalog is in the demo’s pseudo-locale file (paddock-i18n merge)', async () => {
    const fs = await nodeFs();
    const catalog = fs.readFileSync('projects/paddock-ui/i18n/messages.pd.xlf', 'utf8');
    const ids = [...catalog.matchAll(/<trans-unit id="([^"]+)"/g)].map((m) => m[1] as string);
    const loaded = JSON.parse(fs.readFileSync(`${DEMO}/locale/messages.en-XA.json`, 'utf8')) as { translations: Record<string, string> };
    expect(ids.length).toBeGreaterThan(50);
    expect(ids.filter((id) => !(id in loaded.translations))).toEqual([]);
  });
});
