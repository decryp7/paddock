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

  it('splits the file name from the start the layout may shorten, with the full value as the tooltip', () => {
    const el = make({ value: '/var/log/app/system.log' });
    // The directory shrinks, the "/" before the name never does (review: the ellipsis must not swallow the separator).
    expect(el.querySelector('.pd-head')?.textContent).toBe('/var/log/app');
    expect(el.querySelector('.pd-sep')?.textContent).toBe('/');
    expect(el.textContent?.replace(/\s+/g, '')).toContain('/var/log/app/system.log');
    expect(el.querySelector('.pd-name')?.textContent).toBe('');
    expect(el.querySelector('.pd-tail')?.textContent).toBe('system.log');
    expect(el.querySelector('.pd-text')?.getAttribute('title')).toBe('/var/log/app/system.log');
    expect(el.querySelector('.pd-copy')).toBeNull();
  });

  it('keeps a Windows separator and has none for a bare name', () => {
    const win = make({ value: 'C:\\logs\\app\\system.log' });
    expect(win.querySelector('.pd-sep')?.textContent).toBe('\\');
    expect(win.querySelector('.pd-head')?.textContent).toBe('C:\\logs\\app');
    expect(make({ value: 'system.log' }).querySelector('.pd-sep')?.textContent).toBe('');
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
    expect(el.querySelector('.pd-text')?.getAttribute('title')).toBe('/var/log/app/system.log');
    const button = el.querySelector<HTMLButtonElement>('.pd-copy')!;
    expect(button.getAttribute('aria-label')).toBe('Copy the full path');
    button.click();
    await f.whenStable();
    expect(written).toEqual(['/var/log/app/system.log']);
    expect(button.getAttribute('aria-label')).toBe('Copied');
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
  it('opens at the pointer with Copy focused, copies the selection and closes; Escape and a click elsewhere close it', async () => {
    const written: string[] = [];
    stubClipboard(async (t) => void written.push(t));
    const f = TestBed.createComponent(PdCopyMenuHost);
    const menu = TestBed.inject(PdCopyMenu);
    const el = f.nativeElement as HTMLElement;
    f.detectChanges();
    expect(el.querySelector('[role=menu]')).toBeNull();
    menu.open(40, 60, 'selected');
    await f.whenStable();
    const box = el.querySelector<HTMLElement>('[role=menu]')!;
    expect(box.style.left).toBe('40px');
    expect(box.style.top).toBe('60px');
    const item = el.querySelector<HTMLButtonElement>('[role=menuitem]')!;
    expect(item.textContent?.trim()).toBe('Copy');
    expect(document.activeElement).toBe(item);
    item.click();
    await f.whenStable();
    expect(written).toEqual(['selected']);
    expect(menu.state()).toBeNull();

    menu.open(1, 2, 'x');
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(menu.state()).toBeNull();
    menu.open(1, 2, 'x');
    document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    expect(menu.state()).toBeNull();
  });

  it('keeps the menu open for a press inside it, and a failing clipboard only closes it', async () => {
    stubClipboard(() => Promise.reject(new Error('denied')));
    const f = TestBed.createComponent(PdCopyMenuHost);
    const menu = TestBed.inject(PdCopyMenu);
    f.detectChanges();
    menu.open(1, 2, 'x');
    await f.whenStable();
    (f.nativeElement as HTMLElement).querySelector('[role=menu]')!.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    expect(menu.state()).not.toBeNull();
    await menu.copy();
    expect(menu.state()).toBeNull();
    await menu.copy();
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
