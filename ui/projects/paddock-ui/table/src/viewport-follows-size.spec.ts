import { CdkVirtualScrollViewport, CdkFixedSizeVirtualScroll } from '@angular/cdk/scrolling';
import { ChangeDetectionStrategy, Component, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { By } from '@angular/platform-browser';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { PdViewportFollowsSize } from './viewport-follows-size';

@Component({
  selector: 'pd-vfs-host',
  imports: [CdkVirtualScrollViewport, CdkFixedSizeVirtualScroll, PdViewportFollowsSize],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<cdk-virtual-scroll-viewport pdViewportFollowsSize [itemSize]="20"></cdk-virtual-scroll-viewport>`,
})
class Host {}

afterEach(() => vi.unstubAllGlobals());

describe('pdViewportFollowsSize', () => {
  it('measures the CDK viewport again whenever its element changes size, and stops when destroyed', () => {
    const watched: { target: Element; cb: ResizeObserverCallback; disconnected: boolean }[] = [];
    vi.stubGlobal(
      'ResizeObserver',
      class {
        private readonly entry: { target: Element; cb: ResizeObserverCallback; disconnected: boolean };
        constructor(cb: ResizeObserverCallback) {
          this.entry = { target: null as unknown as Element, cb, disconnected: false };
          watched.push(this.entry);
        }
        observe(target: Element): void {
          this.entry.target = target;
        }
        unobserve(): void {}
        disconnect(): void {
          this.entry.disconnected = true;
        }
      },
    );
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const f = TestBed.createComponent(Host);
    f.detectChanges();
    const viewport = f.debugElement.query(By.directive(CdkVirtualScrollViewport));
    const cdk = viewport.componentInstance as CdkVirtualScrollViewport;
    const check = vi.spyOn(cdk, 'checkViewportSize');
    const mine = watched.find((w) => w.target === viewport.nativeElement)!;
    expect(mine).toBeDefined();
    mine.cb([], {} as ResizeObserver);
    expect(check).toHaveBeenCalledTimes(1);
    f.destroy();
    expect(mine.disconnected).toBe(true);
  });

  it('does nothing where there is no ResizeObserver', () => {
    vi.stubGlobal('ResizeObserver', undefined);
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const f = TestBed.createComponent(Host);
    expect(() => f.detectChanges()).not.toThrow();
  });
});
