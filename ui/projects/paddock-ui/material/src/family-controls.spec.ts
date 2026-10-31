const nodeFs = (): Promise<{ readFileSync(path: string, encoding: 'utf8'): string }> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as never;

// ER-21: the family's control geometry and type are ONE section of scss/_material.scss, set on `body` so a host app's Material theme
// (on `html` or `:root`) cannot beat it and the cdk overlays inherit it. These checks keep every such token in that section.
const GEOMETRY = /^--mat-(?:button-(?:filled|outlined|text|tonal|protected)-(?:container-height|container-shape|label-text-(?:font|size|weight|transform))|form-field-(?:container-(?:height|vertical-padding|text-font|text-size|text-line-height)|outlined-(?:container-shape|label-text-populated-size|label-text-size|focus-outline-width)|subscript-text-(?:size|font))|select-trigger-text-(?:font|size)|button-toggle-(?:height|shape|label-text-(?:font|size|line-height))|icon-button-(?:state-layer-size|icon-size)|checkbox-state-layer-size|radio-state-layer-size|chip-(?:container-height|container-shape-radius|label-text-(?:font|size))|slide-toggle-label-text-(?:font|size))$/;

function declarations(text: string): [string, string][] {
  return [...text.matchAll(/^\s*(--mat-[\w-]+):\s*([^;]+);/gm)].map((m) => [m[1]!, m[2]!.trim()]);
}

describe('the family controls section of _material.scss', async () => {
  const scss = (await nodeFs()).readFileSync('projects/paddock-ui/scss/_material.scss', 'utf8');
  const begin = scss.indexOf('// family-controls:begin');
  const end = scss.indexOf('// family-controls:end');
  const section = scss.slice(begin, end);
  const outside = scss.slice(0, begin) + scss.slice(end);

  it('exists and sits on body, not :root', () => {
    expect(begin).toBeGreaterThan(0);
    expect(end).toBeGreaterThan(begin);
    expect(/^body\s*\{/m.test(section)).toBe(true);
    expect(/:root|html\s*\{/.test(section.replace(/\/\/.*$/gm, ''))).toBe(false);
  });

  it('carries geometry and type for buttons, form fields, selects, toggles, state layers and chips, and no colour', () => {
    const names = declarations(section).map(([n]) => n);
    for (const needle of [
      '--mat-button-filled-container-height',
      '--mat-button-text-container-shape',
      '--mat-form-field-container-height',
      '--mat-form-field-outlined-label-text-populated-size',
      '--mat-form-field-subscript-text-size',
      '--mat-form-field-outlined-focus-outline-width',
      '--mat-select-trigger-text-size',
      '--mat-button-toggle-height',
      '--mat-icon-button-state-layer-size',
      '--mat-chip-container-height',
    ]) {
      expect(names, needle).toContain(needle);
    }
    expect(names.filter((n) => /color|opacity/.test(n))).toEqual([]);
    expect(names.every((n) => GEOMETRY.test(n)), names.filter((n) => !GEOMETRY.test(n)).join(', ')).toBe(true);
  });

  it('has every such geometry or type token of the file, and no copy of one outside it', () => {
    const stray = declarations(outside).filter(([n]) => GEOMETRY.test(n) && n !== '--mat-slide-toggle-label-text-size');
    expect(stray.map(([n]) => n)).toEqual([]);
  });
});
