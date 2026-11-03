import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { getInstanceByDom } from 'echarts/core';
import { Chart, animateDraw, prefersReducedMotion } from './chart';

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

describe('animateDraw', () => {
  it('animates only the first draw, and never with reduced motion', () => {
    expect(animateDraw(true, false)).toBe(true);
    expect(animateDraw(false, false)).toBe(false);
    expect(animateDraw(true, true)).toBe(false);
    expect(animateDraw(false, true)).toBe(false);
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

  // A live chart (Counters polls every 2 s) got its options replaced whole with animation on, so every poll replayed the
  // entry animation: about 18 canvas frames per update instead of one (measured in the VM, docs/benchmarks.md 2026-11-03).
  it('animates its first draw only; later options are drawn without animation', async () => {
    const f = TestBed.createComponent(Host);
    const line = (n: number) => ({ xAxis: { type: 'category' }, yAxis: { type: 'value' }, series: [{ type: 'line', data: Array.from({ length: n }, (_, i) => i) }] });
    f.componentInstance.opts.set(line(2));
    const host = () => (f.nativeElement as HTMLElement).querySelector('dy-chart .host') as HTMLDivElement;
    const animation = () => (getInstanceByDom(host())?.getOption() as { animation?: boolean } | undefined)?.animation;
    await vi.waitFor(() => {
      f.detectChanges();
      expect(animation()).toBe(true);
    });
    f.componentInstance.opts.set(line(3));
    await settleAll(f);
    expect(animation()).toBe(false);
    f.componentInstance.opts.set(line(4));
    await settleAll(f);
    expect(animation()).toBe(false);
    f.destroy();
  });

  it('destroys cleanly before ECharts has loaded', () => {
    const f = TestBed.createComponent(Host);
    f.detectChanges();
    expect(() => f.destroy()).not.toThrow();
  });

  // A canvas without a 2D context (jsdom always; WebKit when canvas memory runs out): ECharts would throw in its frame
  // loop and, once it has drawn, on dispose, so the board's teardown failed. The chart says it cannot draw instead.
  it('shows the inline failure, and destroys cleanly, when the canvas has no 2D context', async () => {
    vi.mocked(HTMLCanvasElement.prototype.getContext).mockReturnValue(null);
    const f = TestBed.createComponent(Host);
    await vi.waitFor(() => {
      f.detectChanges();
      expect((f.nativeElement as HTMLElement).querySelector('.failed')?.textContent).toContain('could not be drawn');
    });
    expect((f.nativeElement as HTMLElement).querySelector('dy-chart canvas')).toBeNull();
    expect(() => f.destroy()).not.toThrow();
  });

  it('closes cleanly when ECharts throws on dispose after drawing (the context was lost)', async () => {
    let lost = false;
    const ctx = new Proxy({} as Record<string | symbol, unknown>, {
      get: (_t, k) => (k === 'measureText' ? () => ({ width: 10 }) : k === 'createLinearGradient' ? () => ({ addColorStop: () => undefined }) : k === 'clearRect' && lost ? () => { throw new TypeError('context lost'); } : () => undefined),
      set: () => true,
    });
    vi.mocked(HTMLCanvasElement.prototype.getContext).mockReturnValue(ctx as never);
    const f = TestBed.createComponent(Host);
    f.componentInstance.opts.set({ xAxis: { type: 'category', data: ['a', 'b'] }, yAxis: { type: 'value' }, series: [{ type: 'line', data: [1, 2] }] });
    await vi.waitFor(() => {
      f.detectChanges();
      expect((f.nativeElement as HTMLElement).querySelector('dy-chart canvas')).not.toBeNull();
    });
    lost = true;
    const logged = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    expect(() => f.destroy()).not.toThrow();
    expect(logged).toHaveBeenCalledWith('Chart failed to close', expect.any(TypeError));
  });
});
