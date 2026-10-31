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

/** The timer globals Vitest's fake timers replace and a spec may spy on. */
const TIMER_GLOBALS = [
  'setTimeout',
  'clearTimeout',
  'setInterval',
  'clearInterval',
  'setImmediate',
  'clearImmediate',
  'requestAnimationFrame',
  'cancelAnimationFrame',
  'requestIdleCallback',
  'cancelIdleCallback',
] as const;

/** Where the real timer globals are kept: on the shared window, so the first install in a worker records them once. */
const REAL_TIMERS = Symbol.for('@decryp7/paddock-ui/testing:real-timers');

/** The real timer globals, recorded by the first install in this worker (before any test could fake or spy on them). */
function realTimers(win: Record<string | symbol, unknown>): ReadonlyMap<string, unknown> {
  const known = win[REAL_TIMERS] as Map<string, unknown> | undefined;
  if (known) return known;
  const real = new Map<string, unknown>();
  for (const name of TIMER_GLOBALS) if (typeof win[name] === 'function') real.set(name, win[name]);
  win[REAL_TIMERS] = real;
  return real;
}

/**
 * Puts back every timer global that is not the real one and returns their names. A `vi.spyOn(globalThis, 'setTimeout')`
 * made under fake timers keeps the fake as its original, and Vitest (5.0) restores it again at every `vi.restoreAllMocks()`,
 * even after the spec's own `mockRestore()`: once the fake clock is uninstalled, that writes back a fake bound to a clock
 * that is gone. Its timers never fire, and every later spec in the worker that waits on a real timer hangs until the test
 * timeout (CI runs 7257, 7274 and 7276).
 */
function putBackTimers(win: Record<string | symbol, unknown>, real: ReadonlyMap<string, unknown>): string[] {
  const replaced: string[] = [];
  for (const [name, fn] of real) {
    if (win[name] !== fn) {
      win[name] = fn;
      replaced.push(name);
    }
  }
  return replaced;
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
 * and after every test the kit therefore restores every `vi.spyOn`, puts back real timers, undoes every `vi.stubGlobal`,
 * empties the page and both storages, checks that every timer global is the real one recorded by the worker's first install
 * (putting it back, and failing the test that left it replaced), and deletes the window keys (a spec that stands in for Tauri sets
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
  const hooks = globalThis as unknown as HookGlobals;
  const real = realTimers(win);
  const drop = (): string[] => {
    for (const key of keys) delete win[key];
    // Spies first, then the clock: a spy on a faked timer restored after the clock is gone would write the dead fake back.
    hooks.vi?.restoreAllMocks();
    hooks.vi?.useRealTimers();
    hooks.vi?.unstubAllGlobals();
    if (typeof document !== 'undefined') document.body?.replaceChildren();
    clearStorage('localStorage');
    clearStorage('sessionStorage');
    return putBackTimers(win, real);
  };
  hooks.beforeEach?.(() => void drop());
  hooks.afterEach?.(() => {
    const replaced = drop();
    if (replaced.length) {
      throw new Error(
        `This test left ${replaced.join(', ')} replaced after its clean-up (the kit put the real one back). Do not ` +
          `vi.spyOn a timer while fake timers are on: Vitest restores the spied-on fake at every restoreAllMocks, even after ` +
          `mockRestore, and once the clock is gone its timers never fire, so later specs in this worker hang. Count timers ` +
          `with vi.getTimerCount() instead.`,
      );
    }
  });
}
