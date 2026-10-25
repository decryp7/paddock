/** The test globals the kit uses when a test runner defines them (Vitest through the Angular unit-test builder does). */
interface HookGlobals {
  beforeEach?: (fn: () => void) => void;
  afterEach?: (fn: () => void) => void;
  /** Vitest's `vi` (a global when `globals` is on, as the Angular builder sets it). */
  vi?: { useRealTimers(): unknown; restoreAllMocks(): unknown; unstubAllGlobals(): unknown };
}

/** What `installTestKit` can be told. */
export interface TestKitOptions {
  /** Window properties deleted before and after every test (default: the Tauri stub a spec may leave behind). */
  readonly dropWindowKeys?: readonly string[];
}

/** Clears the browser storages; a storage that cannot be reached (an opaque origin) has nothing to clear. */
function clearStorage(name: 'localStorage' | 'sessionStorage'): void {
  try {
    globalThis[name]?.clear();
  } catch {
    /* no storage here */
  }
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
 * `matchMedia`, `ResizeObserver`, pointer capture and `PointerEvent`, `scrollIntoView`, and a clean window between tests. The
 * Angular builder runs Vitest with `isolate: false`, so spec files share one window per worker and whatever a test leaves
 * behind (often because it failed before its own clean-up) reaches the next file, which then fails only in full runs. Before
 * and after every test the kit therefore puts back real timers, restores every `vi.spyOn`, undoes every `vi.stubGlobal`,
 * empties the page and both storages, and deletes the window keys (a spec that stands in for Tauri sets
 * `window.__TAURI_INTERNALS__`, which would make the next spec believe it runs inside Tauri). Only what is missing is added.
 * Safe to call twice.
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
    if (!('ResizeObserver' in globalThis)) win['ResizeObserver'] = ResizeObserverStub;
    if (!('PointerEvent' in globalThis)) win['PointerEvent'] = PointerEventPolyfill;
    if (!Element.prototype.setPointerCapture) {
      Element.prototype.setPointerCapture = () => undefined;
      Element.prototype.releasePointerCapture = () => undefined;
      Element.prototype.hasPointerCapture = () => false;
    }
    if (!Element.prototype.scrollIntoView) Element.prototype.scrollIntoView = () => undefined;
  }
  const keys = options.dropWindowKeys ?? ['__TAURI_INTERNALS__'];
  const hooks = globalThis as unknown as HookGlobals;
  const drop = () => {
    for (const key of keys) delete win[key];
    hooks.vi?.useRealTimers();
    hooks.vi?.restoreAllMocks();
    hooks.vi?.unstubAllGlobals();
    if (typeof document !== 'undefined') document.body?.replaceChildren();
    clearStorage('localStorage');
    clearStorage('sessionStorage');
  };
  hooks.beforeEach?.(drop);
  hooks.afterEach?.(drop);
}
