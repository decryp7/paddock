import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { compile } from 'sass';
import { describe, expect, it } from 'vitest';
import { contrast, parseColor } from './color';
import { ACCENTS, readRules, resolve, type Scheme } from './css';

const scss = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'projects', 'paddock-ui', 'scss');
const compiled = (entry: string): string => compile(join(scss, entry), { style: 'expanded' }).css;
const controls = compiled('entry-controls.scss');
const material = compiled('entry-material.scss');
const tokens = readFileSync(join(scss, '_tokens.scss'), 'utf8');
const rules = readRules();

/** Every --pd-* custom property a stylesheet reads. */
const used = (css: string): string[] => [...new Set([...css.matchAll(/var\((--pd-[\w-]+)/g)].map((m) => m[1] as string))];

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
      expect(m[2], m[1]).toMatch(/^(var\(--pd-[\w-]+\)|calc\(var\(--pd-[\w-]+\) \/ [\d.]+\)|none|transparent|currentcolor|0px)$/);
    }
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
    expect(controls).toMatch(/\.pd-input:focus-visible,[^{]*\{[^}]*outline: 2px solid var\(--pd-focus-ring\)/);
    expect(controls).not.toMatch(/box-shadow:\s*0 0 0 2px var\(--pd-accent-ring\)/);
    expect(controls).toMatch(/\.pd-switch:focus-visible\s*\{[^}]*outline: 2px solid/);
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
});
