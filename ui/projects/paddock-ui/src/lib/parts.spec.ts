import { TestBed } from '@angular/core/testing';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import { copyText } from './clipboard';
import { PD_CLOCK_INTERVAL, PD_CLOCK_SOURCE, PdClock } from './clock';
import { PdCopyMenu, PdCopyMenuHost } from './copy-menu';
import { PdPathText } from './path-text';
import { SuEmptyState } from './states';

/** Replaces the clipboard of jsdom (it has none) for one test. */
function stubClipboard(writeText: (t: string) => Promise<void>): void {
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
}

afterEach(() => {
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: undefined });
  vi.useRealTimers();
});

describe('copyText', () => {
  it('copies, and answers false instead of throwing when the clipboard is not available', async () => {
    const written: string[] = [];
    stubClipboard(async (t) => void written.push(t));
    expect(await copyText('abc')).toBe(true);
    expect(written).toEqual(['abc']);
    stubClipboard(() => Promise.reject(new Error('denied')));
    expect(await copyText('abc')).toBe(false);
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: undefined });
    expect(await copyText('abc')).toBe(false);
  });
});

describe('PdClock', () => {
  let time = 1000;
  beforeEach(() => {
    time = 1000;
    vi.useFakeTimers();
    TestBed.configureTestingModule({
      providers: [
        { provide: PD_CLOCK_SOURCE, useValue: () => time },
        { provide: PD_CLOCK_INTERVAL, useValue: 500 },
      ],
    });
  });

  function setVisibility(state: 'visible' | 'hidden'): void {
    Object.defineProperty(document, 'visibilityState', { configurable: true, value: state });
    document.dispatchEvent(new Event('visibilitychange'));
  }
  afterEach(() => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' });
  });

  it('ticks while the window is visible, stops while it is hidden and jumps to the real time when it shows again', () => {
    const clock = TestBed.inject(PdClock);
    expect(clock.now()).toBe(1000);
    time = 1600;
    vi.advanceTimersByTime(500);
    expect(clock.now()).toBe(1600);
    setVisibility('hidden');
    expect(clock.visible()).toBe(false);
    time = 9000;
    vi.advanceTimersByTime(5000);
    expect(clock.now()).toBe(1600);
    expect(vi.getTimerCount()).toBe(0);
    setVisibility('visible');
    expect(clock.visible()).toBe(true);
    expect(clock.now()).toBe(9000);
    expect(vi.getTimerCount()).toBe(1);
    time = 9100;
    clock.tick();
    expect(clock.now()).toBe(9100);
  });

  it('leaves no timer behind when it is destroyed', () => {
    TestBed.inject(PdClock);
    expect(vi.getTimerCount()).toBe(1);
    TestBed.resetTestingModule();
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe('PdPathText', () => {
  function make(inputs: Record<string, unknown>): HTMLElement {
    const f = TestBed.createComponent(PdPathText);
    for (const [k, v] of Object.entries(inputs)) f.componentRef.setInput(k, v);
    f.detectChanges();
    return f.nativeElement as HTMLElement;
  }

  it('shows the whole value while it fits or is not measured, with the full value as the tooltip', () => {
    const el = make({ value: '/var/log/app/system.log' });
    expect(el.querySelector('.pd-shown')?.textContent).toBe('/var/log/app/system.log');
    expect(el.querySelector('.pd-size')?.getAttribute('data-full')).toBe('/var/log/app/system.log');
    // Whole on screen: nothing to add, so no tooltip (the text is only titled when it is clipped).
    expect(el.querySelector('.pd-text')?.hasAttribute('title')).toBe(false);
    el.querySelector('.pd-text')?.dispatchEvent(new MouseEvent('mouseenter'));
    expect(document.querySelector('.mat-mdc-tooltip')).toBeNull();
    expect(el.querySelector('.pd-copy')).toBeNull();
  });

  it('shows what it is given but copies and titles the full value', async () => {
    const written: string[] = [];
    stubClipboard(async (t) => void written.push(t));
    const f = TestBed.createComponent(PdPathText);
    f.componentRef.setInput('value', 'system.log');
    f.componentRef.setInput('full', '/var/log/app/system.log');
    f.componentRef.setInput('copy', true);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    const text = el.querySelector('.pd-text')!;
    text.dispatchEvent(new MouseEvent('mouseenter'));
    await vi.waitFor(() => expect(document.querySelector('.mat-mdc-tooltip')?.textContent?.trim()).toBe('/var/log/app/system.log'));
    text.dispatchEvent(new MouseEvent('mouseleave'));
    const button = el.querySelector<HTMLButtonElement>('.pd-copy')!;
    expect(button.getAttribute('aria-label')).toBe('Copy the full path');
    button.click();
    await f.whenStable();
    expect(written).toEqual(['/var/log/app/system.log']);
    expect(button.getAttribute('aria-label')).toBe('Copied');
  });

  describe('measured', () => {
    let measure: ((cb: () => void) => void) | undefined;
    class RO {
      constructor(readonly cb: () => void) {
        measure = () => cb();
      }
      observe = vi.fn();
      unobserve = vi.fn();
      disconnect = vi.fn();
    }
    const canvas = (width: number) => vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue({ font: '', measureText: (s: string) => ({ width: s.length * width }) } as unknown as CanvasRenderingContext2D);

    beforeEach(() => {
      vi.stubGlobal('ResizeObserver', RO);
    });
    afterEach(() => {
      vi.unstubAllGlobals();
      vi.restoreAllMocks();
    });

    async function fit(inputs: Record<string, unknown>, boxWidth: number) {
      const f = TestBed.createComponent(PdPathText);
      for (const [k, v] of Object.entries(inputs)) f.componentRef.setInput(k, v);
      f.detectChanges();
      const el = f.nativeElement as HTMLElement;
      const rect = vi.spyOn(el.querySelector('.pd-text')!, 'getBoundingClientRect').mockReturnValue({ width: boxWidth } as DOMRect);
      measure?.(() => undefined);
      await f.whenStable();
      f.detectChanges();
      // A second, different width reuses the canvas context made for the first.
      rect.mockReturnValue({ width: boxWidth + 30 } as DOMRect);
      measure?.(() => undefined);
      await f.whenStable();
      f.detectChanges();
      return el;
    }

    it('shortens in the middle to the measured width and then explains itself with a tooltip', async () => {
      canvas(10);
      const el = await fit({ value: '/var/log/app/system.log', keepTail: 6 }, 150);
      const shown = el.querySelector('.pd-shown')?.textContent ?? '';
      expect(shown).toContain('…');
      expect(shown.length).toBeLessThan('/var/log/app/system.log'.length);
      el.querySelector('.pd-text')?.dispatchEvent(new MouseEvent('mouseenter'));
      await vi.waitFor(() => expect(document.querySelector('.mat-mdc-tooltip')?.textContent?.trim()).toBe('/var/log/app/system.log'));
    });

    it('shortens the end of a title when asked to', async () => {
      canvas(10);
      const el = await fit({ value: 'A very long title of a window', clip: 'end' }, 120);
      expect(el.querySelector('.pd-shown')?.textContent).toContain('…');
    });

    it('shows the whole value where there is no canvas, or it cannot be made', async () => {
      vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
      expect((await fit({ value: '/a/very/long/path' }, 20)).querySelector('.pd-shown')?.textContent).toBe('/a/very/long/path');
      vi.restoreAllMocks();
      vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => {
        throw new Error('no canvas');
      });
      expect((await fit({ value: '/a/very/long/path' }, 20)).querySelector('.pd-shown')?.textContent).toBe('/a/very/long/path');
    });

    it('stops watching its box when it is destroyed', () => {
      const f = TestBed.createComponent(PdPathText);
      f.componentRef.setInput('value', 'x');
      f.detectChanges();
      f.destroy();
    });

    it('puts the Copied label back after a moment', async () => {
      vi.useFakeTimers();
      stubClipboard(async () => undefined);
      const f = TestBed.createComponent(PdPathText);
      f.componentRef.setInput('value', '/a/b');
      f.componentRef.setInput('copy', true);
      f.detectChanges();
      const button = (f.nativeElement as HTMLElement).querySelector<HTMLButtonElement>('.pd-copy')!;
      button.click();
      await vi.advanceTimersByTimeAsync(0);
      f.detectChanges();
      expect(button.getAttribute('aria-label')).toBe('Copied');
      await vi.advanceTimersByTimeAsync(1600);
      f.detectChanges();
      expect(button.getAttribute('aria-label')).toBe('Copy the full path');
    });
  });

  it('does not claim a copy that failed', async () => {
    stubClipboard(() => Promise.reject(new Error('denied')));
    const f = TestBed.createComponent(PdPathText);
    f.componentRef.setInput('value', '/a/b');
    f.componentRef.setInput('copy', true);
    f.detectChanges();
    (f.nativeElement as HTMLElement).querySelector<HTMLButtonElement>('.pd-copy')!.click();
    await f.whenStable();
    expect((f.nativeElement as HTMLElement).querySelector('.pd-copy')?.getAttribute('aria-label')).toBe('Copy the full path');
  });
});

describe('pd-copy-menu', () => {
  const panel = () => document.querySelector<HTMLElement>('.cdk-overlay-container [role=menu]');
  const item = () => document.querySelector<HTMLButtonElement>('.cdk-overlay-container [role=menuitem]');

  it('opens at the pointer with Copy focused, copies the selection and closes; Escape and a click elsewhere close it', async () => {
    const written: string[] = [];
    stubClipboard(async (t) => void written.push(t));
    const f = TestBed.createComponent(PdCopyMenuHost);
    const menu = TestBed.inject(PdCopyMenu);
    f.detectChanges();
    expect(panel()).toBeNull();
    menu.open(40, 60, 'selected');
    await f.whenStable();
    await vi.waitFor(() => expect(panel()).not.toBeNull());
    expect(item()?.textContent?.trim()).toBe('Copy');
    expect(panel()?.getAttribute('aria-label')).toBe('Copy');
    await vi.waitFor(() => expect(document.activeElement).toBe(item()));
    item()!.click();
    await f.whenStable();
    expect(written).toEqual(['selected']);
    expect(menu.state()).toBeNull();
    await vi.waitFor(() => expect(panel()).toBeNull());

    menu.open(1, 2, 'x');
    await vi.waitFor(() => expect(panel()).not.toBeNull());
    document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', keyCode: 27, bubbles: true }));
    await vi.waitFor(() => expect(menu.state()).toBeNull());
    expect(panel()).toBeNull();

    menu.open(1, 2, 'x');
    await vi.waitFor(() => expect(panel()).not.toBeNull());
    document.body.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    await vi.waitFor(() => expect(menu.state()).toBeNull());
  });

  it('stays open through the release of the right click that opened it (WebKit: contextmenu comes on the press)', async () => {
    const f = TestBed.createComponent(PdCopyMenuHost);
    const menu = TestBed.inject(PdCopyMenu);
    f.detectChanges();
    const press = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: 40, clientY: 60 });
    document.body.dispatchEvent(press);
    menu.open(40, 60, 'selected', press);
    await vi.waitFor(() => expect(panel()).not.toBeNull());
    // The button comes up after the menu is drawn: its auxclick lands outside the menu.
    document.body.dispatchEvent(new MouseEvent('auxclick', { bubbles: true, button: 2 }));
    await f.whenStable();
    expect(menu.state()).not.toBeNull();
    expect(panel()).not.toBeNull();
    // A later click elsewhere still closes it.
    document.body.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await vi.waitFor(() => expect(menu.state()).toBeNull());
  });

  it('gives back the selection it opened on when it closes, as a native menu keeps it (WebKit drops it on focus)', async () => {
    const f = TestBed.createComponent(PdCopyMenuHost);
    const menu = TestBed.inject(PdCopyMenu);
    f.detectChanges();
    const p = document.createElement('p');
    p.textContent = 'keep me selected';
    const other = document.createElement('p');
    other.textContent = 'something else';
    document.body.append(p, other);
    const select = (el: Element) => {
      const r = document.createRange();
      r.selectNodeContents(el);
      getSelection()!.removeAllRanges();
      getSelection()!.addRange(r);
    };
    try {
      select(p);
      menu.open(1, 2, 'keep me selected');
      await vi.waitFor(() => expect(panel()).not.toBeNull());
      getSelection()!.removeAllRanges(); // what WebKit does when the menu takes the focus
      document.activeElement?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', keyCode: 27, bubbles: true }));
      await vi.waitFor(() => expect(menu.state()).toBeNull());
      await vi.waitFor(() => expect(getSelection()!.toString()).toBe('keep me selected'));

      // A selection the user made while it was open is theirs: it stays.
      menu.open(1, 2, 'keep me selected');
      await vi.waitFor(() => expect(panel()).not.toBeNull());
      select(other);
      menu.close();
      await vi.waitFor(() => expect(panel()).toBeNull());
      await f.whenStable();
      expect(getSelection()!.toString()).toBe('something else');
    } finally {
      getSelection()!.removeAllRanges();
      p.remove();
      other.remove();
    }
  });

  it('closes when the service closes it, and a failing clipboard only closes the menu', async () => {
    stubClipboard(() => Promise.reject(new Error('denied')));
    const f = TestBed.createComponent(PdCopyMenuHost);
    const menu = TestBed.inject(PdCopyMenu);
    f.detectChanges();
    menu.open(1, 2, 'x');
    await vi.waitFor(() => expect(panel()).not.toBeNull());
    menu.close();
    await vi.waitFor(() => expect(panel()).toBeNull());
    await menu.copy();
    expect(menu.state()).toBeNull();
  });
});

describe('pd-empty-state icon', () => {
  it('draws a glyph of the pd: set when asked, and the default tray otherwise', () => {
    const f = TestBed.createComponent(SuEmptyState);
    f.componentRef.setInput('heading', 'No runners yet');
    f.componentRef.setInput('icon', 'folder');
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('pd-glyph')).not.toBeNull();
    f.componentRef.setInput('icon', '');
    f.detectChanges();
    expect(el.querySelector('pd-glyph')).toBeNull();
    expect(el.querySelector('svg path')).not.toBeNull();
  });
});

describe('pd-glyph', () => {
  it('draws a named glyph, accepts the pd: prefix and custom paths, and is decorative', () => {
    const f = TestBed.createComponent(PdGlyph);
    f.componentRef.setInput('name', 'pd:refresh');
    f.componentRef.setInput('size', 20);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.getAttribute('aria-hidden')).toBe('true');
    expect(el.querySelector('svg')?.getAttribute('width')).toBe('20');
    expect(el.querySelector('path')?.getAttribute('d')).toContain('M20 12a8 8');
    // 1.5 screen px at any size: the paths do not scale their stroke.
    expect(el.querySelector('svg')?.getAttribute('stroke-width')).toBe('1.5');
    expect(el.querySelector('path')?.getAttribute('vector-effect')).toBe('non-scaling-stroke');
    f.componentRef.setInput('size', 24);
    f.detectChanges();
    expect(el.querySelector('svg')?.getAttribute('stroke-width')).toBe('1.5');
    f.componentRef.setInput('stroke', 3);
    f.detectChanges();
    expect(el.querySelector('svg')?.getAttribute('stroke-width')).toBe('3');
    f.componentRef.setInput('paths', 'M1 1h2');
    f.detectChanges();
    expect(el.querySelector('path')?.getAttribute('d')).toBe('M1 1h2');
    f.componentRef.setInput('paths', '');
    f.componentRef.setInput('name', 'nothing');
    f.detectChanges();
    expect(el.querySelector('path')).toBeNull();
  });

  it('draws the dots of a glyph as solid filled circles (no stroke)', () => {
    const f = TestBed.createComponent(PdGlyph);
    f.componentRef.setInput('name', 'more');
    f.detectChanges();
    const dots = (f.nativeElement as HTMLElement).querySelectorAll('circle');
    expect(dots).toHaveLength(3);
    for (const c of Array.from(dots)) {
      expect(c.getAttribute('fill')).toBe('currentColor');
      expect(c.getAttribute('stroke')).toBe('none');
      expect(Number(c.getAttribute('r'))).toBeGreaterThanOrEqual(1);
    }
  });
});
