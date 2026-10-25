/** The test globals the kit uses when a test runner defines them (Vitest through the Angular unit-test builder does). */
interface HookGlobals {
  beforeEach?: (fn: () => void) => void;
  afterEach?: (fn: () => void) => void;
}

/** What `installTestKit` can be told. */
export interface TestKitOptions {
  /** Window properties deleted before and after every test (default: the Tauri stub a spec may leave behind). */
  readonly dropWindowKeys?: readonly string[];
}

/** Reports a zero-sized entry for the target so libraries that read `entry.target` keep working in jsdom. */
class ResizeObserverStub {
  constructor(private readonly callback: ResizeObserverCallback) {}
  observe(target: Element): void {
    this.callback([{ target, contentRect: target.getBoundingClientRect() } as ResizeObserverEntry], this as unknown as ResizeObserver);
  }
  unobserve(): void {}
  disconnect(): void {}
}

/** `PointerEvent` for jsdom, which has none: a mouse event with a `pointerId`. */
class PointerEventPolyfill extends MouseEvent {
  readonly pointerId: number;
  constructor(type: string, init: PointerEventInit = {}) {
    super(type, init);
    this.pointerId = init.pointerId ?? 1;
  }
}

/**
 * The jsdom gaps every Paddock app's specs need (plan X38), in one call from the app's `test-setup.ts`:
 * `matchMedia`, `ResizeObserver`, pointer capture and `PointerEvent`, `scrollIntoView`, and a clean window between tests (a spec
 * that stands in for Tauri sets `window.__TAURI_INTERNALS__`; spec files can share a window, so a leftover stub would make
 * the next spec believe it runs inside Tauri). Only what is missing is added. Safe to call twice.
 */
export function installTestKit(options: TestKitOptions = {}): void {
  const win = globalThis as unknown as Record<string, unknown>;
  if (typeof window !== 'undefined') {
    if (!window.matchMedia) {
      window.matchMedia = (query: string) =>
        ({
          matches: false,
          media: query,
          onchange: null,
          addEventListener: () => undefined,
          removeEventListener: () => undefined,
          addListener: () => undefined,
          removeListener: () => undefined,
          dispatchEvent: () => false,
        }) as MediaQueryList;
    }
    // `typeof`, not `in`: a spec that stubbed the global to `undefined` and shares the worker with the next file leaves the key behind.
    if (typeof win['ResizeObserver'] !== 'function') win['ResizeObserver'] = ResizeObserverStub;
    if (typeof win['PointerEvent'] !== 'function') win['PointerEvent'] = PointerEventPolyfill;
    if (!Element.prototype.setPointerCapture) {
      Element.prototype.setPointerCapture = () => undefined;
      Element.prototype.releasePointerCapture = () => undefined;
      Element.prototype.hasPointerCapture = () => false;
    }
    if (!Element.prototype.scrollIntoView) Element.prototype.scrollIntoView = () => undefined;
  }
  const keys = options.dropWindowKeys ?? ['__TAURI_INTERNALS__'];
  const drop = () => {
    for (const key of keys) delete win[key];
  };
  const hooks = globalThis as unknown as HookGlobals;
  hooks.beforeEach?.(drop);
  hooks.afterEach?.(drop);
}
