import { ChangeDetectionStrategy, Component, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { SuTrayPopover } from './tray-popover';

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SuTrayPopover],
  template: `<pd-tray-popover label="Dyno" (dismissed)="dismissed = dismissed + 1">@if (more()) {<p class="more">more</p>}<p>content</p></pd-tray-popover>`,
})
class Host {
  readonly more = signal(false);
  dismissed = 0;
}

describe('pd-tray-popover scroll region', () => {
  let scrollHeight = 100;
  let clientHeight = 100;
  const observers: { callback: () => void; disconnected: boolean }[] = [];

  function stub(): void {
    Object.defineProperty(Element.prototype, 'scrollHeight', { configurable: true, get: () => scrollHeight });
    Object.defineProperty(Element.prototype, 'clientHeight', { configurable: true, get: () => clientHeight });
    vi.stubGlobal(
      'ResizeObserver',
      class {
        private readonly o = { callback: () => undefined as void, disconnected: false };
        constructor(callback: () => void) {
          this.o.callback = callback;
          observers.push(this.o);
        }
        observe(): void {}
        disconnect(): void {
          this.o.disconnected = true;
        }
      },
    );
  }

  afterEach(() => {
    delete (Element.prototype as { scrollHeight?: number }).scrollHeight;
    delete (Element.prototype as { clientHeight?: number }).clientHeight;
    observers.length = 0;
    scrollHeight = clientHeight = 100;
  });

  async function setup() {
    stub();
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const f = TestBed.createComponent(Host);
    document.body.appendChild(f.nativeElement);
    await f.whenStable();
    TestBed.tick();
    await f.whenStable();
    return f;
  }
  const content = (f: { nativeElement: HTMLElement }) => f.nativeElement.querySelector<HTMLElement>('.pd-content')!;

  it('is not a focus stop while its content fits', async () => {
    const f = await setup();
    expect(content(f).hasAttribute('tabindex')).toBe(false);
    expect(content(f).hasAttribute('role')).toBe(false);
  });

  it('becomes a named, focusable region when the content scrolls, and goes back when it fits again', async () => {
    const f = await setup();
    scrollHeight = 700;
    clientHeight = 520;
    observers[0]!.callback();
    await f.whenStable();
    TestBed.tick();
    expect(content(f).getAttribute('tabindex')).toBe('0');
    expect(content(f).getAttribute('role')).toBe('region');
    expect(content(f).getAttribute('aria-label')).toBe('Dyno content');
    scrollHeight = 520;
    f.componentInstance.more.set(true);
    await f.whenStable();
    await new Promise((r) => setTimeout(r, 0));
    TestBed.tick();
    expect(content(f).hasAttribute('tabindex')).toBe(false);
  });

  it('stops observing when destroyed and still dismisses with Escape', async () => {
    const f = await setup();
    f.nativeElement.querySelector('pd-tray-popover').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    expect(f.componentInstance.dismissed).toBe(1);
    f.destroy();
    expect(observers[0]!.disconnected).toBe(true);
  });
});
