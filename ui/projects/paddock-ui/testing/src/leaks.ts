/**
 * The test leak detector: what a test leaves behind is named against that test, not against whichever test runs next.
 *
 * The Angular builder runs Vitest with `isolate: false`, so spec files share one window and one module graph per worker.
 * State one test leaves (a live timer, fake timers, a node on the page, a running job in a page-wide singleton, a load that
 * never settles) then fails a later test, and the wrong test is blamed (debrief.yml run 8170, the leaked mock-jobs
 * singleton, ER-29's fake timer). `installLeakDetector()` runs in every app's and the ui's `test-setup.ts` and checks, per
 * test, after every `afterEach` (Angular's TestBed clean-up included) has run:
 *
 * - tracked in-flight work still pending (`registerInFlight`);
 * - real timers and intervals the test started and left running (not Angular's, Vitest's or jsdom's);
 * - fake timers left installed;
 * - nodes the test left in `document.body` once every clean-up has run (TestBed's own roots and the CDK containers are its job);
 * - registered singletons that hold state (`registerSingleton`), which are then reset.
 *
 * Findings are warnings in `report` mode (the roll-out default, ER-41) and fail the leaking test in `fail` mode. The work
 * of the detector never throws inside the `afterEach` chain (a throw there skips the hooks after it, Angular's clean-up
 * among them, and the next TestBed test fails too): it only collects there, and raises in `onTestFinished`.
 *
 * Design rule it enforces: anything module-level that a hook reads is drained per test (a reader takes what is recorded
 * and clears it), so one stuck item fails only the test that started it.
 */

/** What a check found, for one test. */
export type LeakKind = 'in-flight' | 'timer' | 'fake-timers' | 'dom' | 'singleton';

/** A leak found in one test. */
export interface LeakFinding {
  readonly kind: LeakKind;
  /** One line naming what was left (and where it was started, for a timer). */
  readonly message: string;
}

/** `report` prints a warning per finding; `fail` fails the test that left it; `off` judges nothing (a suite that leaks on purpose). */
export type LeakMode = 'report' | 'fail' | 'off';

/** Work a page-wide record tracks, for example Debrief's lazy start-up loads. */
export interface InFlightSource {
  readonly name: string;
  /** How many items are still pending now. Read-only: it never starts or settles anything. */
  pending(): number;
}

/** A page-wide singleton that a test can leave holding state. */
export interface ResettableSingleton {
  readonly name: string;
  /** A short description of the state left in it, or `null` when it is clean. Read-only. */
  leftover(): string | null;
  /** Back to a fresh instance: called before every test and after a leak was found. */
  reset(): void;
}

/** What `installLeakDetector` can be told. */
export interface LeakDetectorOptions {
  /** Default `report`. */
  readonly mode?: LeakMode;
  /** Checks to skip, by kind. */
  readonly skip?: readonly LeakKind[];
}

/** The hooks and Vitest helpers the detector uses (globals, as with the Angular unit-test builder). */
interface Globals {
  beforeEach?: (fn: (ctx: TestCtx) => void) => void;
  afterEach?: (fn: () => void) => void;
  onTestFinished?: (fn: () => void) => void;
  vi?: { isFakeTimers(): boolean };
}

/** The part of Vitest's test context the detector reads. */
interface TestCtx {
  readonly task?: { readonly name?: string; readonly file?: { readonly name?: string } };
}

type TimerKind = 'setTimeout' | 'setInterval';

interface TrackedTimer {
  readonly kind: TimerKind;
  readonly ms: number;
  readonly where: string;
  reported: boolean;
}

/** All state lives on the shared window, so every module copy in the worker sees the same wrapped timers and registries. */
interface Shared {
  /** The detector of the innermost suite that installed one: it alone judges the current test. */
  current: symbol | null;
  timers: Map<unknown, TrackedTimer>;
  sources: Map<string, InFlightSource>;
  singletons: Map<string, ResettableSingleton>;
  wrapped: boolean;
  real: Record<string, (...a: unknown[]) => unknown>;
}

const SHARED = Symbol.for('@decryp7/paddock-ui/testing:leaks');
/** The kit (`setup.ts`) records the real timer globals under this key; the wrappers replace them there. */
const KIT_REAL_TIMERS = Symbol.for('@decryp7/paddock-ui/testing:real-timers');

function shared(): Shared {
  const win = globalThis as unknown as Record<symbol, Shared | undefined>;
  return (win[SHARED] ??= { current: null, timers: new Map(), sources: new Map(), singletons: new Map(), wrapped: false, real: {} });
}

/** Registers (or replaces, by name) a source of tracked in-flight work, for example a start-up load record. Returns its remover. */
export function registerInFlight(source: InFlightSource): () => void {
  shared().sources.set(source.name, source);
  return () => void shared().sources.delete(source.name);
}

/** Registers (or replaces, by name) a page-wide singleton whose leftover state fails the test that left it. Returns its remover. */
export function registerSingleton(singleton: ResettableSingleton): () => void {
  shared().singletons.set(singleton.name, singleton);
  return () => void shared().singletons.delete(singleton.name);
}

/** Stack frames of code that owns its own timers: the framework, the test runner and the third-party dock library (its blur and drag-end timers are out of a test's reach), not the code under test. */
const FRAMEWORK_FRAME = /node_modules[\\/](?:@angular|zone\.js|vitest|@vitest|jsdom|tinyrainbow|chai|dockview-core)[\\/]|[\\/]@angular[\\/]|[\\/]vitest[\\/]/;
/** Frames the owner search skips (libraries that only forward the call). */
const FORWARDING_FRAME = /node_modules[\\/](?:rxjs|tslib)[\\/]/;

/** Vitest's spy wrapper: a `vi.spyOn(globalThis, 'setTimeout')` forwarding to the real one sits between the caller and us. */
const SPY_FRAME = /[\\/]vitest[\\/]dist[\\/]chunks[\\/]spy\.|@vitest[\\/]spy/;

/**
 * Where a timer was started, from the stack of the call, or `null` when the framework or the runner owns it or no frame names an owner (not
 * the test's to clean up). Frame 0 is "Error" and frame 1 the detector's wrapper. A spy on the timer global (a spec that holds
 * some timers back) forwards the call: the implementation closure and the spy wrapper are skipped, so the timer is
 * blamed on whoever called the spy, which may be Angular Material's form field and not the spec.
 */
export function timerOwner(stack: string | undefined): string | null {
  let frames = (stack ?? '').split('\n').slice(2).map((f) => f.trim()).filter((f) => f.length > 0 && !FORWARDING_FRAME.test(f));
  const spy = frames.findIndex((f) => SPY_FRAME.test(f));
  if (spy >= 0) frames = frames.slice(spy + 1);
  const first = frames[0];
  // No frame to read (every frame was a forwarder, or the call came from native code): not attributable, so not blamed.
  if (!first) return null;
  if (FRAMEWORK_FRAME.test(first)) return null;
  return first.replace(/^at\s+/, '');
}

/** Wraps the timer globals once per worker. Only timers started by non-framework code are tracked. */
function wrapTimers(): void {
  const s = shared();
  if (s.wrapped) return;
  const win = globalThis as unknown as Record<string | symbol, unknown>;
  const kitReal = win[KIT_REAL_TIMERS] as Map<string, unknown> | undefined;
  const track = (kind: TimerKind): void => {
    const real = win[kind] as (...a: unknown[]) => unknown;
    if (typeof real !== 'function') return;
    s.real[kind] = real;
    const wrapper = function (this: unknown, fn: unknown, ms?: number, ...rest: unknown[]): unknown {
      if (typeof fn !== 'function') return real.call(globalThis, fn, ms, ...rest);
      const errorCtor = Error as unknown as { stackTraceLimit: number };
      const limit = errorCtor.stackTraceLimit;
      errorCtor.stackTraceLimit = 12;
      const where = timerOwner(new Error().stack);
      errorCtor.stackTraceLimit = limit;
      if (where === null) return real.call(globalThis, fn, ms, ...rest);
      let id: unknown;
      const run =
        kind === 'setTimeout'
          ? (...a: unknown[]): unknown => {
              s.timers.delete(id);
              return (fn as (...b: unknown[]) => unknown)(...a);
            }
          : fn;
      id = real.call(globalThis, run, ms, ...rest);
      s.timers.set(id, { kind, ms: ms ?? 0, where, reported: false });
      return id;
    };
    win[kind] = wrapper;
    kitReal?.set(kind, wrapper);
  };
  const clear = (name: 'clearTimeout' | 'clearInterval'): void => {
    const real = win[name] as (...a: unknown[]) => unknown;
    if (typeof real !== 'function') return;
    s.real[name] = real;
    const wrapper = function (this: unknown, id?: unknown): unknown {
      s.timers.delete(id);
      return real.call(globalThis, id);
    };
    win[name] = wrapper;
    kitReal?.set(name, wrapper);
  };
  track('setTimeout');
  track('setInterval');
  clear('clearTimeout');
  clear('clearInterval');
  s.wrapped = true;
}

/** The timers that started before this test and were already blamed or are older than it. */
function timerBaseline(): Set<unknown> {
  return new Set(shared().timers.keys());
}

/** A detector for one test: `begin` before it, `snapshot` once its own hooks begin, `finish` after every `afterEach`. */
export class LeakDetector {
  private baseline = new Set<unknown>();
  private fake = false;
  private domBaseline = new Set<Element>();

  constructor(private readonly skip: readonly LeakKind[] = []) {}

  /** Resets the registered singletons and records what already exists, so only this test's leftovers count. */
  begin(): void {
    for (const singleton of shared().singletons.values()) {
      try {
        singleton.reset();
      } catch {
        /* a singleton that cannot reset is found by `finish` */
      }
    }
    this.baseline = timerBaseline();
    this.fake = false;
    this.domBaseline = new Set(typeof document === 'undefined' || !document.body ? [] : Array.from(document.body.children));
  }

  /**
   * The state the clean-up that follows would hide: fake timers (the kit uninstalls them). Never throws.
   */
  snapshot(vi?: { isFakeTimers(): boolean }): void {
    try {
      this.fake = vi?.isFakeTimers() ?? false;
    } catch {
      /* a failing snapshot must not skip Angular's clean-up */
    }
  }

  /** What the test left. Resets the singletons; with `clearTimers` (`fail` mode) the timers and nodes it left are removed so no later test sees them. */
  finish(clearTimers: boolean): LeakFinding[] {
    const found: LeakFinding[] = [];
    const s = shared();
    const add = (kind: LeakKind, message: string): void => {
      if (!this.skip.includes(kind)) found.push({ kind, message });
    };
    try {
      if (this.fake) add('fake-timers', 'left fake timers installed (vi.useFakeTimers without vi.useRealTimers)');
      // After every afterEach: what TestBed and the components' own clean-up removed is not a leak. TestBed's roots (`ng-version`)
      // and the CDK containers go with its module. (The kit empties the body in its own afterEach, so a suite that uses it is
      // clean here by construction.)
      if (typeof document !== 'undefined' && document.body) {
        for (const node of Array.from(document.body.children)) {
          if (this.domBaseline.has(node) || node.hasAttribute('ng-version') || /^cdk-/.test(node.className?.toString() ?? '')) continue;
          add('dom', `left ${describeNode(node)} in document.body`);
          if (clearTimers) node.remove();
        }
      }
      for (const source of s.sources.values()) {
        const pending = source.pending();
        if (pending > 0) add('in-flight', `left ${pending} item(s) of "${source.name}" still in flight`);
      }
      for (const singleton of s.singletons.values()) {
        const left = singleton.leftover();
        if (left !== null) add('singleton', `left "${singleton.name}" holding state: ${left}`);
        singleton.reset();
      }
      for (const [id, timer] of [...s.timers]) {
        if (this.baseline.has(id) || timer.reported) continue;
        timer.reported = true;
        add('timer', `left a live ${timer.kind}(${timer.ms}) started at ${timer.where}`);
        if (clearTimers) {
          const clear = s.real[timer.kind === 'setTimeout' ? 'clearTimeout' : 'clearInterval'];
          clear?.call(globalThis, id);
          s.timers.delete(id);
        }
      }
    } catch (error) {
      found.push({ kind: 'singleton', message: `the detector failed while checking: ${String(error)}` });
    }
    return found;
  }
}

/**
 * One report line. Written straight to stderr when the runner has it: the Angular builder runs Vitest with
 * `silent: 'passed-only'`, which drops `console` output of passing tests, and a report-only finding is on a passing test.
 */
function warn(line: string): void {
  const proc = (globalThis as unknown as { process?: { stderr?: { write(s: string): unknown } } }).process;
  if (proc?.stderr) proc.stderr.write(`${line}\n`);
  else console.warn(line);
}

function describeNode(node: Element): string {
  const id = node.id ? `#${node.id}` : '';
  const cls = typeof node.className === 'string' && node.className ? `.${node.className.trim().split(/\s+/).join('.')}` : '';
  return `<${node.tagName.toLowerCase()}${id}${cls}>`;
}

/**
 * Installs the detector for the tests of the current file (call it from `test-setup.ts`, after any `installTestKit()`
 * so its `afterEach` runs first: it records what the kit's own clean-up would hide). Safe to call twice. A suite that installs its own detector replaces the outer one for its tests.
 */
export function installLeakDetector(options: LeakDetectorOptions = {}): void {
  const mode = options.mode ?? 'report';
  const hooks = globalThis as unknown as Globals;
  wrapTimers();
  const detector = new LeakDetector(options.skip ?? []);
  const id = Symbol('leak-detector');
  let name = 'unknown test';
  hooks.beforeEach?.((ctx) => {
    const task = ctx?.task as { fullName?: string; name?: string; file?: { name?: string } } | undefined;
    // `fullName` already starts with the file; the bare name does not.
    name = task?.fullName ?? `${task?.file?.name ?? 'unknown file'} > ${task?.name ?? 'unknown test'}`;
    try {
      detector.begin();
    } catch (error) {
      warn(`[leak-detector] ${name}: begin failed: ${String(error)}`);
    }
    // `beforeEach` hooks run outer to inner: the innermost install judges the test, so a suite that installs its own mode
    // (this kit's own specs, which leak on purpose) is not judged twice.
    shared().current = id;
    hooks.onTestFinished?.(() => {
      if (shared().current !== id) return;
      const findings = detector.finish(mode === 'fail');
      if (findings.length === 0 || mode === 'off') return;
      if (mode === 'report') {
        for (const f of findings) warn(`[leak-detector] ${name}: ${f.kind}: ${f.message}`);
        return;
      }
      throw new Error(
        `This test leaked, and would have failed a later test instead:\n${findings.map((f) => `  - ${f.kind}: ${f.message}`).join('\n')}\n` +
          'Clean it up in the test (or its afterEach); the detector cleared the timers and reset the singletons.',
      );
    });
  });
  hooks.afterEach?.(() => detector.snapshot(hooks.vi));
}
