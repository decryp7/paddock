import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { compile, compileString } from 'sass';
import { describe, expect, it } from 'vitest';
import { contrast, parseColor } from './color';
import { ACCENTS, readRules, resolve, type Scheme } from './css';

const scss = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'projects', 'paddock-ui', 'scss');
// @angular/material is installed under the ui workspace (npm nests it there), or at the repo root.
const nodeModules = [join(scss, '..', '..', '..', 'node_modules'), join(scss, '..', '..', '..', '..', 'node_modules')];
const compiled = (entry: string): string => compile(join(scss, entry), { style: 'expanded', loadPaths: nodeModules }).css;
const controls = compiled('entry-controls.scss');
/** The bridge alone (our rules, every value a --pd-* token) and the whole sheet apps load (Material's own `mat.theme` first). */
const material = compileString("@use 'material';", { style: 'expanded', loadPaths: [scss] }).css;
const materialEntry = compiled('entry-material.scss');
const tokens = readFileSync(join(scss, '_tokens.scss'), 'utf8');
const rules = readRules();

/** Every --pd-* custom property a stylesheet reads. */
const used = (css: string): string[] => [...new Set([...css.matchAll(/var\((--pd-[\w-]+)/g)].map((m) => m[1] as string))];

describe('native checks and radios (X50 review 3 P2-2)', () => {
  it('fill with the control accent everywhere: one rule in the base stylesheet, not the browser blue', () => {
    const base = compiled('entry-paddock.scss');
    const rule = /input\[type=['"]?checkbox['"]?\],\s*input\[type=['"]?radio['"]?\]\s*\{[^}]*accent-color:\s*var\(--pd-accent-control\)/;
    expect(base).toMatch(rule);
  });
});

describe('control styles and the Material bridge (plan X37)', () => {
  it.each([
    ['controls', controls],
    ['material', material],
  ])('%s read only tokens the token sheet defines', (_name, css) => {
    const missing = used(css).filter((t) => !tokens.includes(`${t}:`));
    expect(missing).toEqual([]);
  });

  it.each([
    ['controls', controls],
    ['material', material],
  ])('%s have no colour of their own (hex, rgb, hsl, named)', (_name, css) => {
    const body = css.replace(/\/\*[\s\S]*?\*\//g, '');
    expect(body).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(body).not.toMatch(/\b(rgb|rgba|hsl|hsla|oklch|oklab)\(/);
    expect(body).not.toMatch(/:\s*(white|black|red|blue|green|gray|grey)\b/);
  });

  it('every class starts with pd- (an app global stylesheet cannot collide), apart from Material\'s own', () => {
    const selectors = [...controls.matchAll(/\.([a-z][\w-]*)/g)].map((m) => m[1] as string).filter((c) => !c.startsWith('pd-') && !c.startsWith('mat-'));
    expect(selectors).toEqual([]);
  });

  it('maps the Material system tokens and a component token for each part the apps use', () => {
    const props = [...material.matchAll(/(--mat-[\w-]+)\s*:/g)].map((m) => m[1] as string);
    expect(props.length).toBeGreaterThanOrEqual(150);
    for (const part of ['sys-primary', 'sys-surface', 'sys-outline', 'tooltip-container-color', 'slide-toggle-selected-track-color', 'checkbox-selected-icon-color', 'radio-selected-icon-color', 'form-field-outlined-outline-color', 'select-panel-background-color', 'autocomplete-background-color', 'menu-container-color', 'tab-active-indicator-color', 'datepicker-calendar-container-background-color', 'progress-bar-active-indicator-color', 'dialog-container-color']) {
      expect(props, part).toContain(`--mat-${part}`);
    }
    // Every value is a token reference, a sized token expression or a keyword: nothing hard-coded.
    for (const m of material.matchAll(/(--mat-[\w-]+)\s*:\s*([^;]+);/g)) {
      expect(m[2], m[1]).toMatch(/^(var\(--pd-[\w-]+\)|calc\(var\(--pd-[\w-]+\) \/ [\d.]+\)|calc\(var\(--pd-focus-width\) \+ 1px\)|none|transparent|currentcolor|block|0px|0)$/);
    }
  });

  it('the bridge alone (no mat.theme) sets the hover, focus and pressed state-layer opacities, or Material draws an opaque disc', () => {
    const value = (name: string) => new RegExp(`${name}\\s*:\\s*([^;]+);`).exec(material)?.[1];
    expect(value('--mat-sys-hover-state-layer-opacity')).toBe('var(--pd-hover-alpha)');
    expect(value('--mat-sys-focus-state-layer-opacity')).toBe('var(--pd-hover-alpha)');
    expect(value('--mat-sys-pressed-state-layer-opacity')).toBe('var(--pd-pressed-alpha)');
  });

  describe.each(['dark', 'light'] as Scheme[])('contrast of the pairs the controls add, %s', (scheme) => {
    it.each(ACCENTS)('the control fill (on toggle track, checked box or radio) is 3:1 on the page, surface-1, surface-2 and surface-3, and its ink is legible, accent %s', (accent) => {
      const v = resolve(rules, scheme, accent);
      const c = (n: string) => parseColor(v[n] as string);
      for (const s of ['--pd-bg', '--pd-surface-1', '--pd-surface-2', '--pd-surface-3']) {
        expect(contrast(c('--pd-accent-control'), c(s)), s).toBeGreaterThanOrEqual(3);
      }
      expect(contrast(c('--pd-on-accent-control'), c('--pd-accent-control'))).toBeGreaterThanOrEqual(4.5);
    });

    it.each(ACCENTS)('the focus ring is 3:1 on every surface (SC 2.4.13), accent %s', (accent) => {
      const v = resolve(rules, scheme, accent);
      const c = (n: string) => parseColor(v[n] as string);
      for (const s of ['--pd-bg', '--pd-surface-1', '--pd-surface-2', '--pd-surface-3']) {
        expect(contrast(c('--pd-focus-ring'), c(s)), s).toBeGreaterThanOrEqual(3);
      }
    });

    it.each(ACCENTS)('the selected segment (accent ring on the track) is 3:1 against the track in both components, accent %s', (accent) => {
      const v = resolve(rules, scheme, accent);
      const c = (n: string) => parseColor(v[n] as string);
      // .pd-segmented's track is surface-3, pd-segmented-control's is surface-2.
      for (const track of ['--pd-surface-3', '--pd-surface-2']) expect(contrast(c('--pd-accent'), c(track)), track).toBeGreaterThanOrEqual(3);
    });

    it('the unselected toggle handle and the placeholder text are legible on their track and field', () => {
      const v = resolve(rules, scheme, 'debrief');
      const c = (n: string) => parseColor(v[n] as string);
      expect(contrast(c('--pd-text-muted'), c('--pd-surface-3'))).toBeGreaterThanOrEqual(3);
      expect(contrast(c('--pd-text-subtle'), c('--pd-surface-1'))).toBeGreaterThanOrEqual(4.5);
    });
  });

  it('inputs, selects and the switch show the solid focus ring, not only a translucent halo (review P1-5)', () => {
    expect(controls).toMatch(/\.pd-input:focus-visible,[^{]*\{[^}]*outline: var\(--pd-focus-width\) solid var\(--pd-focus-ring\)/);
    expect(controls).not.toMatch(/box-shadow:\s*0 0 0 2px var\(--pd-accent-ring\)/);
    expect(controls).toMatch(/\.pd-switch:focus-visible\s*\{[^}]*outline: var\(--pd-focus-width\) solid/);
  });

  it('the selected segment is an accent tint with a ring and a heavier label, not a surface that inverts in dark (review P1-6)', () => {
    const block = controls.slice(controls.indexOf('.pd-segmented'));
    expect(block).toMatch(/aria-checked=true\][^{]*\{[^}]*background: var\(--pd-accent-tint\)[^}]*font-weight: var\(--pd-weight-semibold\)[^}]*inset 0 0 0 1px var\(--pd-accent\)/);
  });

  it('toggles, checkboxes and radios use the control tokens in the Material bridge too', () => {
    for (const m of material.matchAll(/(--mat-(?:slide-toggle-selected|checkbox-selected|radio-selected)[\w-]*(?:track|icon)-color):\s*([^;]+);/g)) {
      expect(m[2], m[1]).toBe('var(--pd-accent-control)');
    }
  });
  it('the shared entry carries one Material theme (system tokens, density -3) and our bridge after it, so ours win', () => {
    expect(materialEntry).toContain('--mat-sys-primary');
    // Material's theme is on `html` (specificity 0,0,1), our mapping on `:root` (0,1,0): ours win whatever the order.
    expect(materialEntry).toMatch(/^html \{[^}]*--mat-sys-primary:/m);
    expect(materialEntry).toMatch(/^:root \{[^}]*--mat-sys-primary: var\(--pd-accent-fill\)/m);
    expect(materialEntry).toMatch(/density|--mat-/);
    // No inline style attribute is ever needed: the sheets are plain CSS (CSP forbids style="").
    expect(materialEntry).not.toMatch(/unsafe-inline/);
  });

  it('bridges every Material component of the migration plan', () => {
    const props = new Set([...material.matchAll(/(--mat-[\w-]+)\s*:/g)].map((m) => m[1] as string));
    const parts = ['table-background-color', 'sort-arrow-color', 'paginator-container-background-color', 'tree-container-background-color', 'button-toggle-selected-state-background-color', 'badge-background-color', 'list-list-item-label-text-color', 'expansion-container-background-color', 'timepicker-container-background-color', 'slider-active-track-color', 'chip-selected-label-text-color', 'tab-active-label-text-color', 'progress-bar-active-indicator-color', 'slide-toggle-selected-track-color', 'checkbox-selected-icon-color', 'radio-selected-icon-color', 'select-panel-background-color', 'form-field-outlined-outline-color', 'menu-container-color', 'tooltip-container-color', 'snack-bar-container-color', 'autocomplete-background-color', 'datepicker-calendar-container-background-color', 'button-filled-container-color', 'button-outlined-outline-color', 'button-text-label-text-color', 'button-tonal-container-color', 'button-protected-container-color'];
    for (const part of parts) expect(props, part).toContain(`--mat-${part}`);
  });

  it('matButton: filled is the primary, outlined the secondary, danger a class; no ripple colour anywhere', () => {
    expect(material).toMatch(/--mat-button-filled-container-color: var\(--pd-accent-fill\)/);
    expect(material).toMatch(/--mat-button-outlined-outline-color: var\(--pd-border-strong\)/);
    expect(material).toMatch(/\.mat-mdc-unelevated-button\.pd-button-danger \{[^}]*--pd-danger-fill/);
    for (const m of material.matchAll(/(--mat-button-[a-z]+-ripple-color):\s*([^;]+);/g)) expect(m[2], m[1]).toBe('transparent');
  });

  describe.each(['dark', 'light'] as Scheme[])('contrast of the Material components, %s', (scheme) => {
    /** [ink token, surface token, minimum]: text 4.5, icons, borders and indicators 3 (WCAG 1.4.3, 1.4.11). Read from the bridge. */
    const PAIRS: [string, string, number][] = [
      ['table-row-item-label-text-color', 'table-background-color', 4.5],
      ['table-header-headline-color', 'table-background-color', 4.5],
      ['sort-arrow-color', 'table-background-color', 3],
      ['paginator-container-text-color', 'paginator-container-background-color', 4.5],
      ['paginator-enabled-icon-color', 'paginator-container-background-color', 3],
      ['tree-node-text-color', 'tree-container-background-color', 4.5],
      ['button-toggle-text-color', 'button-toggle-background-color', 4.5],
      ['button-toggle-selected-state-text-color', 'button-toggle-selected-state-background-color', 4.5],
      ['badge-text-color', 'badge-background-color', 4.5],
      ['list-list-item-label-text-color', 'sys-surface', 4.5],
      ['list-list-item-supporting-text-color', 'sys-surface', 4.5],
      ['list-list-item-label-text-color', 'list-list-item-selected-container-color', 4.5],
      ['expansion-header-text-color', 'expansion-container-background-color', 4.5],
      ['expansion-header-description-color', 'expansion-container-background-color', 4.5],
      ['expansion-header-indicator-color', 'expansion-container-background-color', 3],
      ['option-label-text-color', 'timepicker-container-background-color', 4.5],
      ['slider-active-track-color', 'sys-surface', 3],
      ['slider-handle-color', 'sys-surface', 3],
      ['slider-label-label-text-color', 'slider-label-container-color', 4.5],
      ['chip-label-text-color', 'chip-elevated-container-color', 4.5],
      ['chip-selected-label-text-color', 'chip-elevated-selected-container-color', 4.5],
      ['stepper-header-label-text-color', 'sys-surface', 4.5],
      ['stepper-header-selected-state-label-text-color', 'sys-surface', 4.5],
      ['stepper-header-icon-foreground-color', 'stepper-header-icon-background-color', 4.5],
      ['stepper-header-selected-state-icon-foreground-color', 'stepper-header-selected-state-icon-background-color', 4.5],
      ['stepper-header-done-state-icon-foreground-color', 'stepper-header-done-state-icon-background-color', 3],
      ['stepper-header-edit-state-icon-foreground-color', 'stepper-header-edit-state-icon-background-color', 3],
      ['tab-active-label-text-color', 'sys-surface', 4.5],
      ['tab-inactive-label-text-color', 'sys-surface', 4.5],
      ['tab-active-indicator-color', 'sys-surface', 3],
      ['progress-bar-active-indicator-color', 'sys-surface', 3],
      ['slide-toggle-selected-track-color', 'sys-surface', 3],
      ['slide-toggle-selected-handle-color', 'slide-toggle-selected-track-color', 3],
      ['checkbox-selected-icon-color', 'sys-surface', 3],
      ['checkbox-selected-checkmark-color', 'checkbox-selected-icon-color', 4.5],
      ['checkbox-unselected-icon-color', 'sys-surface', 3],
      ['radio-selected-icon-color', 'sys-surface', 3],
      ['radio-unselected-icon-color', 'sys-surface', 3],
      ['checkbox-label-text-color', 'sys-surface', 4.5],
      ['radio-label-text-color', 'sys-surface', 4.5],
      ['select-enabled-trigger-text-color', 'sys-surface', 4.5],
      ['select-placeholder-text-color', 'sys-surface', 4.5],
      ['option-label-text-color', 'select-panel-background-color', 4.5],
      ['form-field-outlined-input-text-color', 'sys-surface', 4.5],
      ['form-field-outlined-label-text-color', 'sys-surface', 4.5],
      ['form-field-outlined-outline-color', 'sys-surface', 3],
      ['form-field-outlined-focus-outline-color', 'sys-surface', 3],
      ['form-field-error-text-color', 'sys-surface', 4.5],
      ['form-field-subscript-text-color', 'sys-surface', 4.5],
      ['menu-item-label-text-color', 'menu-container-color', 4.5],
      ['menu-item-icon-color', 'menu-container-color', 3],
      ['tooltip-supporting-text-color', 'tooltip-container-color', 4.5],
      ['snack-bar-supporting-text-color', 'snack-bar-container-color', 4.5],
      ['snack-bar-button-color', 'snack-bar-container-color', 4.5],
      ['option-label-text-color', 'autocomplete-background-color', 4.5],
      ['datepicker-calendar-date-text-color', 'datepicker-calendar-container-background-color', 4.5],
      ['datepicker-calendar-header-text-color', 'datepicker-calendar-container-background-color', 4.5],
      ['datepicker-calendar-body-label-text-color', 'datepicker-calendar-container-background-color', 4.5],
      ['datepicker-calendar-date-selected-state-text-color', 'datepicker-calendar-date-selected-state-background-color', 4.5],
      ['button-filled-label-text-color', 'button-filled-container-color', 4.5],
      ['button-outlined-label-text-color', 'sys-surface-container', 4.5],
      // The secondary button's border (border-strong) is decoration by design (guide section 8, Buttons); its label identifies it.
      ['button-text-label-text-color', 'sys-surface', 4.5],
      ['button-tonal-label-text-color', 'button-tonal-container-color', 4.5],
      ['button-protected-label-text-color', 'button-protected-container-color', 4.5],
    ];

    /** The --pd token a bridge entry points at (a plain var(--pd-x) value). */
    const target = (name: string): string => {
      const m = new RegExp(`--mat-${name}:\\s*var\\((--pd-[\\w-]+)\\)`).exec(material);
      if (!m) throw new Error(`--mat-${name} is not a plain --pd token in the bridge`);
      return m[1] as string;
    };

    it.each(ACCENTS)('every pair is legible (text 4.5, icons and borders 3), accent %s', (accent) => {
      const v = resolve(rules, scheme, accent);
      // A translucent surface (the accent tint is a colour-mix over transparent) is composited on the page surface it sits on.
      const base = parseColor(v['--pd-surface-1'] as string);
      const c = (n: string) => {
        const mix = /^color-mix\(in srgb, (#[0-9a-f]{6}) ([\d.]+)%, transparent\)$/i.exec(v[n] as string);
        if (!mix) return parseColor(v[n] as string);
        const fg = parseColor(mix[1] as string);
        const a = Number(mix[2]) / 100;
        return { r: fg.r * a + base.r * (1 - a), g: fg.g * a + base.g * (1 - a), b: fg.b * a + base.b * (1 - a), a: 1 };
      };
      const failures: string[] = [];
      for (const [ink, surface, min] of PAIRS) {
        const ratio = contrast(c(target(ink)), c(target(surface)));
        if (ratio < min) failures.push(`${ink} on ${surface}: ${ratio.toFixed(2)} < ${min}`);
      }
      expect(failures).toEqual([]);
    });

    it.each(ACCENTS)('the danger button (fill and ink) is legible, accent %s', (accent) => {
      const v = resolve(rules, scheme, accent);
      const c = (n: string) => parseColor(v[n] as string);
      expect(contrast(c('--pd-on-danger'), c('--pd-danger-fill'))).toBeGreaterThanOrEqual(4.5);
      for (const s of ['--pd-surface-1', '--pd-surface-2']) expect(contrast(c('--pd-danger'), c(s)), s).toBeGreaterThanOrEqual(4.5);
    });
  });
});
