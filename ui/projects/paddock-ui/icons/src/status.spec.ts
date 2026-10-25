import { TestBed } from '@angular/core/testing';
import { PD_ICON_PATHS } from './paths';
import { PD_STATUS_GLYPHS, PdStatusGlyph, statusGlyph } from './status';
import { statusGlyphProblems, statusMapProblems } from './status-lint';

/** Node's `fs`, loaded at run time (the spec programme has no Node types). */
interface NodeFs {
  readFileSync(path: string, encoding: 'utf8'): string;
  readdirSync(path: string): string[];
  statSync(path: string): { isDirectory(): boolean };
}
const nodeFs = (): Promise<NodeFs> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as Promise<NodeFs>;

function sources(fs: NodeFs, dir: string): string[] {
  return fs.readdirSync(dir).flatMap((name) => {
    const path = `${dir}/${name}`;
    if (name === 'node_modules' || name === 'generated') return [];
    if (fs.statSync(path).isDirectory()) return sources(fs, path);
    return /\.(ts|html)$/.test(path) && !path.endsWith('.spec.ts') ? [path] : [];
  });
}

describe('status glyphs (review 3 P2-4)', () => {
  it('map every status to its own shape: error circle-×, warning triangle, info circle-i, success check', () => {
    expect(PD_STATUS_GLYPHS).toEqual({ error: 'error', warning: 'alert', info: 'info', success: 'check' });
    expect(new Set(Object.values(PD_STATUS_GLYPHS)).size).toBe(4);
    expect(statusGlyph('error')).toBe('error');
  });

  it('draws the glyph of its status', async () => {
    const fixture = TestBed.createComponent(PdStatusGlyph);
    fixture.componentRef.setInput('status', 'error');
    fixture.componentRef.setInput('size', 14);
    await fixture.whenStable();
    const el = fixture.nativeElement as HTMLElement;
    expect(el.getAttribute('data-status')).toBe('error');
    expect(el.getAttribute('aria-hidden')).toBe('true');
    expect(el.querySelector('svg')?.getAttribute('width')).toBe('14');
    expect(el.querySelector('path')?.getAttribute('d')).toBe(PD_ICON_PATHS.error);
  });

  it('finds a status glyph written by name, and nothing else', () => {
    const text = 'a\n<p><pd-glyph name="alert" [size]="14" /></p>\n<pd-glyph class="x" name="pd:error" />\n<pd-glyph name="refresh" />\n<pd-status-glyph status="error" />';
    expect(statusGlyphProblems('f.ts', text)).toEqual([
      'f.ts:2: <pd-glyph name="alert"> (use <pd-status-glyph status="…"> or statusGlyph)',
      'f.ts:3: <pd-glyph name="error"> (use <pd-status-glyph status="…"> or statusGlyph)',
    ]);
  });

  it('finds a status mapped to a glyph in TypeScript, and leaves other maps alone (review 4 P2-3)', () => {
    const text = "const A = { neutral: '', warn: 'alert', bad: 'close' };\nconst B = {\n  error: 'pd:alert',\n  file: 'file',\n};\nconst C = { error: statusGlyph('error') };";
    expect(statusMapProblems('m.ts', text)).toEqual([
      "m.ts:1: warn: 'alert' (derive it from PD_STATUS_GLYPHS / statusGlyph)",
      "m.ts:1: bad: 'close' (derive it from PD_STATUS_GLYPHS / statusGlyph)",
      "m.ts:3: error: 'alert' (derive it from PD_STATUS_GLYPHS / statusGlyph)",
    ]);
    expect(statusMapProblems('x/icons/src/status.ts', "{ error: 'error', warning: 'alert' }")).toEqual([]);
    // A map to statuses, not glyphs, is fine: the shared map draws them.
    expect(statusMapProblems('v.ts', "export const TONE_STATUS: Readonly<Record<Tone, PdStatus | null>> = { neutral: null, bad: 'error' };")).toEqual([]);
  });

  it('has no second status → glyph map in TypeScript (review 4 P2-3)', async () => {
    const fs = await nodeFs();
    const files = ['projects/paddock-ui', 'projects/shell-demo/src', 'projects/workbench-demo/src', 'projects/gallery/src']
      .flatMap((d) => sources(fs, d))
      .filter((f) => f.endsWith('.ts') && !f.endsWith('/status-lint.ts'));
    expect(files.length).toBeGreaterThan(100);
    expect(files.flatMap((f) => statusMapProblems(f, fs.readFileSync(f, 'utf8')))).toEqual([]);
  });

  it('is used by every template of the library, the demos and the gallery: no status glyph is written by name', async () => {
    const fs = await nodeFs();
    const files = ['projects/paddock-ui', 'projects/shell-demo/src', 'projects/workbench-demo/src', 'projects/gallery/src'].flatMap((d) => sources(fs, d)).filter((f) => !f.endsWith('/status-lint.ts'));
    expect(files.length).toBeGreaterThan(100);
    expect(files.flatMap((f) => statusGlyphProblems(f, fs.readFileSync(f, 'utf8')))).toEqual([]);
  });
});
