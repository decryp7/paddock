import { Component } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { injectNow } from './now';

@Component({ selector: 'dy-now-host', template: '' })
class Host {
  readonly now = injectNow(1000);
}

describe('injectNow', () => {
  afterEach(() => vi.useRealTimers());

  it('ticks until the component is destroyed', () => {
    vi.useFakeTimers({ now: 1_000_000 });
    const f = TestBed.createComponent(Host);
    expect(f.componentInstance.now()).toBe(1_000_000);
    vi.advanceTimersByTime(3000);
    expect(f.componentInstance.now()).toBe(1_003_000);
    f.destroy();
    vi.advanceTimersByTime(5000);
    expect(f.componentInstance.now()).toBe(1_003_000);
    expect(vi.getTimerCount()).toBe(0);
  });
});
