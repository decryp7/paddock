const nodeFs = (): Promise<{ readFileSync(path: string, encoding: 'utf8'): string }> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as never;
import { PD_FAMILY_CONTROLS } from './family-controls';

// The family's control geometry is declared twice: on `:root` by css/material.css and on shared hosts by PD_FAMILY_CONTROLS.
// They must never drift apart.
describe('PD_FAMILY_CONTROLS', () => {
  const declared = [...PD_FAMILY_CONTROLS.matchAll(/(--mat-[\w-]+):\s*([^;]+);/g)].map((m) => [m[1]!, m[2]!.trim()] as const);

  it('carries geometry and type for buttons, form fields, selects and segmented controls, and no colour', () => {
    const names = declared.map(([n]) => n);
    for (const needle of ['--mat-button-filled-container-height', '--mat-button-outlined-container-shape', '--mat-form-field-container-height', '--mat-select-trigger-text-size', '--mat-button-toggle-height']) {
      expect(names).toContain(needle);
    }
    expect(names.some((n) => /color/.test(n))).toBe(false);
    expect(declared.every(([, v]) => v === 'none' || /^var\(--pd-[\w-]+\)$|^0px$/.test(v))).toBe(true);
  });

  it('equals the declarations in scss/_material.scss', async () => {
    const scss = (await nodeFs()).readFileSync('projects/paddock-ui/scss/_material.scss', 'utf8');
    for (const [name, value] of declared) {
      const m = new RegExp(`^\\s*${name}:\\s*([^;]+);`, 'm').exec(scss);
      expect(m?.[1]?.trim(), name).toBe(value);
    }
  });
});
