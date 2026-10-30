import { observeHeight } from './observe-height';

describe('observeHeight', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('reports the height rounded up, again only when it changes, and stops', () => {
    let fire: () => void = () => undefined;
    const disconnect = vi.fn();
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: () => void) {
          fire = cb;
        }
        observe(): void {
          fire();
        }
        disconnect = disconnect;
      },
    );
    let height = 430.2;
    const el = { getBoundingClientRect: () => ({ height }) } as unknown as Element;
    const report = vi.fn();
    const stop = observeHeight(el, report);
    expect(report).toHaveBeenCalledExactlyOnceWith(431);
    fire();
    expect(report).toHaveBeenCalledTimes(1);
    height = 0;
    fire();
    expect(report).toHaveBeenCalledTimes(1);
    height = 500;
    fire();
    expect(report).toHaveBeenLastCalledWith(500);
    stop();
    expect(disconnect).toHaveBeenCalled();
  });

  it('does nothing where ResizeObserver is missing', () => {
    vi.stubGlobal('ResizeObserver', undefined);
    const report = vi.fn();
    const stop = observeHeight(document.createElement('div'), report);
    expect(report).not.toHaveBeenCalled();
    expect(() => stop()).not.toThrow();
  });
});
