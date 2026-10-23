import { observeHeight } from './observe-height';

describe('observeHeight', () => {
  const original = globalThis.ResizeObserver;
  afterEach(() => {
    globalThis.ResizeObserver = original;
  });

  function fakeObserver() {
    const state = { cb: () => undefined as void, disconnected: false };
    globalThis.ResizeObserver = class {
      constructor(cb: () => void) {
        state.cb = cb;
      }
      observe = vi.fn();
      unobserve = vi.fn();
      disconnect = () => (state.disconnected = true);
    } as unknown as typeof ResizeObserver;
    return state;
  }

  const element = (h: () => number) => ({ getBoundingClientRect: () => ({ height: h() }) }) as unknown as Element;

  it('reports a new height rounded up, and only when it changed', () => {
    const state = fakeObserver();
    let height = 120.2;
    const seen: number[] = [];
    observeHeight(element(() => height), (h) => seen.push(h));
    state.cb();
    state.cb();
    height = 200;
    state.cb();
    expect(seen).toEqual([121, 200]);
  });

  it('ignores an element that has no height yet', () => {
    const state = fakeObserver();
    const seen: number[] = [];
    observeHeight(element(() => 0), (h) => seen.push(h));
    state.cb();
    expect(seen).toEqual([]);
  });

  it('stops observing', () => {
    const state = fakeObserver();
    observeHeight(element(() => 10), () => undefined)();
    expect(state.disconnected).toBe(true);
  });

  it('does nothing where ResizeObserver does not exist', () => {
    (globalThis as { ResizeObserver?: unknown }).ResizeObserver = undefined;
    expect(() => observeHeight(element(() => 10), () => undefined)()).not.toThrow();
  });
});
