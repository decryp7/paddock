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

const SHELL = 'projects/paddock-ui/shell';

/** Classes that are hooks for tests or other code, not styled on purpose. Keep empty unless a reason is written here. */
const UNSTYLED_ON_PURPOSE = new Set<string>([
  // Library classes styled by the global paddock-ui stylesheet (scss/_controls.scss), not by the component.
  'needs-ai.ts:pd-btn', 'needs-ai.ts:pd-btn--ghost',
  'undo-toast.ts:pd-btn', 'undo-toast.ts:pd-btn--sm', 'undo-toast.ts:pd-btn--icon', 'undo-toast.ts:pd-btn--ghost',
  'activity-page.ts:pd-btn', 'activity-page.ts:pd-btn--sm',
  'setting-field.ts:pd-switch', 'settings-window.ts:pd-check',
  // Structural hooks (selectors in tests, grouping) with no rule of their own.
  'channels-page.ts:pd-ch__namecell', 'report-problem-dialog.ts:pd-diag__what', 'report-problem-dialog.ts:pd-diag__num',
  'sidebar.ts:pd-top', 'sidebar.ts:pd-bottom', 'status-bar.ts:pd-start', 'service-page.ts:pd-service__block',
  'storage-page.ts:pd-storage__block',
]);

function sources(fs: NodeFs, dir: string): string[] {
  return fs.readdirSync(dir).flatMap((name) => {
    const path = `${dir}/${name}`;
    if (fs.statSync(path).isDirectory()) return sources(fs, path);
    return path.endsWith('.ts') && !path.endsWith('.spec.ts') ? [path] : [];
  });
}

/** The static class names a template uses (`class="a b"` without interpolation, and `[class.x]`). */
export function templateClasses(source: string): string[] {
  const out = new Set<string>();
  for (const m of source.matchAll(/\sclass="([^"]*)"/g)) {
    for (const token of (m[1] ?? '').replace(/\{\{.*?\}\}/g, ' ').split(/\s+/)) if (token) out.add(token);
  }
  for (const m of source.matchAll(/\[class\.([A-Za-z0-9_-]+)\]/g)) out.add(m[1] ?? '');
  return [...out];
}

describe('template classes have selectors', () => {
  it('finds static and bound classes', () => {
    expect(templateClasses(`<p class="a b {{ x }}" [class.c]="y"></p>`).sort()).toEqual(['a', 'b', 'c']);
  });

  it('every class used by a shell template is selected by its styles', async () => {
    const fs = await nodeFs();
    const problems: string[] = [];
    for (const file of sources(fs, SHELL)) {
      const text = fs.readFileSync(file, 'utf8');
      if (!/\btemplate:/.test(text) || !/\bstyles:/.test(text)) continue;
      const dir = file.slice(0, file.lastIndexOf('/'));
      let corpus = text;
      for (const m of text.matchAll(/from '(\.\/[^']*styles[^']*)'/g)) {
        try {
          corpus += fs.readFileSync(`${dir}/${(m[1] ?? '').slice(2)}.ts`, 'utf8');
        } catch {
          /* not a local file */
        }
      }
      for (const cls of templateClasses(text)) {
        if (UNSTYLED_ON_PURPOSE.has(`${file.slice(file.lastIndexOf('/') + 1)}:${cls}`)) continue;
        const escaped = cls.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
        if (!new RegExp(`\\.${escaped}(?![A-Za-z0-9_-])`).test(corpus.replace(/class="[^"]*"/g, ''))) problems.push(`${file}: .${cls}`);
      }
    }
    expect(problems).toEqual([]);
  });
});
