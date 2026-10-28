import { describe, expect, it } from 'vitest';

// ER-21: the pd- rename (b88a387a1) left ABOUT_STYLES selecting .btn, .facts ...: three windows rendered unstyled and no
// test noticed. Every class a shell component's template uses must have a selector in the styles that component
// declares (its own `styles:` text or a relative styles module it imports). Read from disk (vitest runs in ui/).

interface NodeFs {
  readFileSync(path: string, encoding: 'utf8'): string;
  readdirSync(path: string): string[];
  statSync(path: string): { isDirectory(): boolean };
}
const nodeFs = (): Promise<NodeFs> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as Promise<NodeFs>;

const ROOTS = ['projects/paddock-ui/shell', 'projects/paddock-ui/src/lib'];

/** Classes that are hooks for tests or other code, not styled on purpose. Keep empty unless a reason is written here. */
const UNSTYLED_ON_PURPOSE = new Set<string>([
  // Library classes styled by the global paddock-ui stylesheet (scss/_controls.scss), not by the component.
  'needs-ai.ts:pd-btn', 'needs-ai.ts:pd-btn--ghost',
  'activity-page.ts:pd-btn', 'activity-page.ts:pd-btn--sm',
  'setting-field.ts:pd-switch', 'settings-window.ts:pd-check',
  // Structural hooks (selectors in tests, grouping) with no rule of their own.
  'channels-page.ts:pd-ch__namecell', 'report-problem-dialog.ts:pd-diag__what', 'report-problem-dialog.ts:pd-diag__num',
  'sidebar.ts:pd-top', 'sidebar.ts:pd-bottom', 'status-bar.ts:pd-start', 'service-page.ts:pd-service__block',
  'storage-page.ts:pd-storage__block',
  // State classes on the host that a parent, the e2e kit or a theme reads; the component itself has no rule for them.
  'frame.ts:pd-drawer-open', 'setting-field.ts:pd-edited', 'status-bar.ts:pd-compact', 'service-status.ts:pd-open', 'jobs-status.ts:pd-open',
  'job.ts:pd-job',
  // Block, slot and marker classes (the component's own name, a slot wrapper, an icon marker) styled by element selectors or none.
  'dialog-host.ts:pd-in-page-dialog', 'setting-field.ts:pd-badge', 'dialog-frame.ts:pd-dialog-frame', 'settings-window.ts:pd-settings-window', 'setting-field.ts:pd-setting', 'card.ts:pd-hero',
  'job.ts:pd-state--ok', 'job.ts:pd-state--bad', 'states.ts:pd-secondary', 'status-pill.ts:pd-label',
  // The "[class]" expressions build "pd-<name> pd-<name>--<variant>": the block class is a hook, the variants are selected.
  'metric.ts:pd-metric', 'skeleton.ts:pd-skeleton', 'status-pill.ts:pd-pill', 'tray-popover.ts:pd-tray',
  // Primary modifier of a library button, styled by the global scss/_controls.scss.
  'needs-ai.ts:pd-btn--primary',
]);

function sources(fs: NodeFs, dir: string): string[] {
  return fs.readdirSync(dir).flatMap((name) => {
    const path = `${dir}/${name}`;
    if (fs.statSync(path).isDirectory()) return sources(fs, path);
    return path.endsWith('.ts') && !path.endsWith('.spec.ts') ? [path] : [];
  });
}

/** The class names a component's own markup applies: template `class="a b"` (no interpolation), `[class.x]`, host `class` and `'[class.x]'`, and literals in a host `'[class]'` expression. */
export function templateClasses(source: string): string[] {
  const out = new Set<string>();
  for (const m of source.matchAll(/\sclass="([^"]*)"/g)) {
    for (const token of (m[1] ?? '').replace(/\{\{.*?\}\}/g, ' ').split(/\s+/)) if (token) out.add(token);
  }
  for (const m of source.matchAll(/\[class\.([A-Za-z0-9_-]+)\]/g)) out.add(m[1] ?? '');
  for (const m of source.matchAll(/\bclass:\s*'([^']*)'/g)) for (const token of (m[1] ?? '').split(/\s+/)) if (token) out.add(token);
  for (const m of source.matchAll(/'\[class\]':\s*('(?:[^'\\]|\\.)*'|`[^`]*`)/g)) for (const t of (m[1] ?? '').matchAll(/pd-[A-Za-z0-9_-]+/g)) if (!t[0].endsWith('-')) out.add(t[0]);
  return [...out];
}

/** The text a selector search runs over: the source without the markup that applies classes, so a binding cannot satisfy itself. */
export function stylesCorpus(source: string): string {
  return source
    .replace(/\sclass="[^"]*"/g, ' ')
    .replace(/\[class\.[A-Za-z0-9_-]+\]/g, ' ')
    .replace(/'\[class\.[A-Za-z0-9_-]+\]'/g, ' ')
    .replace(/\bclass:\s*'[^']*'/g, ' ')
    .replace(/'\[class\]':\s*('(?:[^'\\]|\\.)*'|`[^`]*`)/g, ' ');
}

describe('template classes have selectors', () => {
  it('finds static, bound and host classes', () => {
    expect(templateClasses(`<p class="a b {{ x }}" [class.c]="y"></p>`).sort()).toEqual(['a', 'b', 'c']);
    expect(templateClasses(`host: { class: 'h1 h2', '[class.h3]': 'x', '[class]': '"pd-job pd-job--" + t + (f ? " pd-job--flat" : "")' }`).sort()).toEqual(['h1', 'h2', 'h3', 'pd-job', 'pd-job--flat']);
  });

  it('a binding does not count as a selector', () => {
    expect(stylesCorpus(`'[class.pd-open]': 'open()', <i [class.pd-x]="y" class="pd-z"> :host(.pd-q) {}`)).not.toMatch(/pd-open|pd-x|pd-z/);
    expect(stylesCorpus(`:host(.pd-q) {}`)).toContain('.pd-q');
  });

  it('every class used by a shell or library component is selected by its styles', async () => {
    const fs = await nodeFs();
    const problems: string[] = [];
    for (const file of ROOTS.flatMap((root) => sources(fs, root))) {
      const text = fs.readFileSync(file, 'utf8');
      if (!/\btemplate:/.test(text) || !/\bstyles:/.test(text)) continue;
      const dir = file.slice(0, file.lastIndexOf('/'));
      let corpus = stylesCorpus(text);
      for (const m of text.matchAll(/from '(\.{1,2}\/[^']*styles[^']*)'/g)) {
        try {
          corpus += stylesCorpus(fs.readFileSync(`${dir}/${m[1] ?? ''}.ts`, 'utf8'));
        } catch {
          /* not a local file */
        }
      }
      for (const cls of templateClasses(text)) {
        if (UNSTYLED_ON_PURPOSE.has(`${file.slice(file.lastIndexOf('/') + 1)}:${cls}`)) continue;
        const escaped = cls.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
        if (!new RegExp(`\\.${escaped}(?![A-Za-z0-9_-])`).test(corpus)) problems.push(`${file}: .${cls}`);
      }
    }
    expect(problems).toEqual([]);
  });
});
