import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Chart, prefersReducedMotion } from './chart';

@Component({
  imports: [Chart],
  template: `<dy-chart [options]="opts()" label="Demo series" />`,
})
class Host {
  readonly opts = signal<Record<string, unknown> | null>({ series: [] });
}


describe('prefersReducedMotion', () => {
  afterEach(() => vi.unstubAllGlobals());
  it('follows the media query', () => {
    vi.stubGlobal('matchMedia', () => ({ matches: true }));
    expect(prefersReducedMotion()).toBe(true);
    vi.stubGlobal('matchMedia', () => ({ matches: false }));
    expect(prefersReducedMotion()).toBe(false);
  });
  it('is false when matchMedia is missing', () => {
    vi.stubGlobal('matchMedia', undefined);
    expect(prefersReducedMotion()).toBe(false);
  });
});

/** jsdom has no canvas; a permissive 2D context lets ECharts initialise and draw. */
function stubCanvas() {
  const ctx = new Proxy(
    {},
    {
      get: (t, k) => (k in t ? (t as Record<string | symbol, unknown>)[k] : k === 'measureText' ? () => ({ width: 10 }) : k === 'createLinearGradient' ? () => ({ addColorStop: () => undefined }) : () => undefined),
      set: (t, k, v) => ((t as Record<string | symbol, unknown>)[k] = v) || true,
    },
  );
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(ctx as never);
}

async function settleAll(f: { detectChanges(): void; whenStable(): Promise<unknown> }) {
  f.detectChanges();
  await f.whenStable();
  await new Promise((r) => setTimeout(r, 50));
  f.detectChanges();
  await f.whenStable();
}

describe('Chart', () => {
  beforeEach(() => {
    stubCanvas();
    vi.stubGlobal('matchMedia', () => ({ matches: false }));
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('exposes its label as an accessible image and renders a canvas host', async () => {
    const f = TestBed.createComponent(Host);
    await settleAll(f);
    const dy = (f.nativeElement as HTMLElement).querySelector('dy-chart')!;
    expect(dy.getAttribute('role')).toBe('img');
    expect(dy.getAttribute('aria-label')).toBe('Demo series');
    expect(dy.querySelector('.host')?.getAttribute('aria-hidden')).toBe('true');
    expect(dy.querySelector('.failed')).toBeNull();
    f.destroy();
  });

  it('draws valid options without an error and survives new and null options', async () => {
    const f = TestBed.createComponent(Host);
    await settleAll(f);
    f.componentInstance.opts.set({ xAxis: { type: 'category', data: ['a', 'b'] }, yAxis: { type: 'value' }, series: [{ type: 'line', data: [1, 2] }] });
    await settleAll(f);
    f.componentInstance.opts.set(null);
    await settleAll(f);
    expect((f.nativeElement as HTMLElement).querySelector('.failed')).toBeNull();
    f.destroy();
  });

  it('destroys cleanly before ECharts has loaded', () => {
    const f = TestBed.createComponent(Host);
    f.detectChanges();
    expect(() => f.destroy()).not.toThrow();
  });
});
