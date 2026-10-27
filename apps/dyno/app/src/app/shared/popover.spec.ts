import { Component } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../core/backend';
import { Popover } from './popover';

@Component({ imports: [Popover], template: `<dy-popover [maxHeight]="300"><p>content</p></dy-popover>` })
class Host {}

describe('Popover', () => {
  let callbacks: (() => void)[];
  beforeEach(() => {
    callbacks = [];
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: () => void) {
          callbacks.push(cb);
        }
        observe(): void {}
        disconnect(): void {}
      },
    );
  });
  afterEach(() => vi.unstubAllGlobals());

  it('reports its measured height to the shell on layout changes, once per distinct height, with the max height set', async () => {
    const popoverResize = vi.fn().mockResolvedValue(undefined);
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: { popoverResize } }] });
    const f = TestBed.createComponent(Host);
    f.detectChanges();
    await f.whenStable();
    const sheet = f.nativeElement.querySelector('.sheet') as HTMLElement;
    expect(sheet.style.getPropertyValue('--popover-max')).toBe('300px');
    let height = 120.2;
    vi.spyOn(sheet, 'getBoundingClientRect').mockImplementation(() => ({ height }) as DOMRect);
    callbacks.forEach((c) => c());
    callbacks.forEach((c) => c());
    expect(popoverResize).toHaveBeenCalledTimes(1);
    expect(popoverResize).toHaveBeenCalledWith(121);
    height = 200;
    callbacks.forEach((c) => c());
    expect(popoverResize).toHaveBeenLastCalledWith(200);
  });

  it('ignores a failing resize', async () => {
    const popoverResize = vi.fn().mockRejectedValue(new Error('no window'));
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: { popoverResize } }] });
    const f = TestBed.createComponent(Host);
    f.detectChanges();
    await f.whenStable();
    const sheet = f.nativeElement.querySelector('.sheet') as HTMLElement;
    vi.spyOn(sheet, 'getBoundingClientRect').mockImplementation(() => ({ height: 50 }) as DOMRect);
    expect(() => callbacks.forEach((c) => c())).not.toThrow();
  });
});
