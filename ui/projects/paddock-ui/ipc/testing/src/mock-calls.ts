import type { CommandError } from '@decryp7/paddock-ui/ipc';
import type { MockOptions } from './mock-options';

/**
 * Wraps a mock backend with the URL switches (plan X28): latency, failures and held answers, and a record of the calls.
 * The mock itself stays plain data; this is the one place that plays slowness and errors, so every app's mock behaves the same.
 */

/** One call that reached the backend. */
export interface RecordedCall {
  readonly method: string;
  readonly args: readonly unknown[];
}

/** The ports of the wrapper (injectable for tests). */
export interface MockPorts {
  /** Resolves after `ms` milliseconds. */
  readonly sleep: (ms: number) => Promise<void>;
  /** The error a failing command rejects with. */
  readonly failure: (method: string) => CommandError;
}

const defaultPorts: MockPorts = {
  sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
  failure: (method) => ({ code: 'internal', message: `${method} failed on purpose (the mock's ?fail= switch).`, retryable: true }),
};

/** The wrapped backend, the recorded calls and the way to release held answers. */
export interface MockedBackend<B> {
  readonly backend: B;
  readonly calls: RecordedCall[];
  /** Lets the held answers of `method` (or of every held command) through. */
  release(method?: string): void;
}

/**
 * Wraps `inner`: every method returning a promise records its call, waits the latency, fails while its `fail` count lasts, and
 * waits for a release when held. Other members pass through. `globalThis.__paddockMockRelease` is set for specs and manual use.
 */
export function withMockOptions<B extends object>(inner: B, options: MockOptions, ports: Partial<MockPorts> = {}): MockedBackend<B> {
  const { sleep, failure } = { ...defaultPorts, ...ports };
  const calls: RecordedCall[] = [];
  const failed = new Map<string, number>();
  const held = new Map<string, (() => void)[]>();
  const open = new Set<string>();

  const release = (method?: string): void => {
    const targets = method === undefined ? [...held.keys(), ...options.hold] : [method];
    for (const name of targets) {
      open.add(name);
      for (const resume of held.get(name) ?? []) resume();
      held.delete(name);
    }
  };
  (globalThis as { __paddockMockRelease?: (method?: string) => void }).__paddockMockRelease = release;

  const backend = new Proxy(inner, {
    get(target, prop, receiver) {
      const value = Reflect.get(target, prop, receiver) as unknown;
      if (typeof value !== 'function' || typeof prop !== 'string') return value;
      return (...args: unknown[]) => {
        calls.push({ method: prop, args });
        const result = (value as (...a: unknown[]) => unknown).apply(target, args);
        if (!(result instanceof Promise)) return result;
        return (async () => {
          if (options.latencyMs > 0) await sleep(options.latencyMs);
          if (options.hold.includes(prop) && !open.has(prop)) await new Promise<void>((resume) => held.set(prop, [...(held.get(prop) ?? []), resume]));
          const budget = options.fail[prop];
          if (budget !== undefined) {
            const used = failed.get(prop) ?? 0;
            if (used < budget) {
              failed.set(prop, used + 1);
              // Failing calls still ran the inner method (as a real backend would have started the work); swallow its outcome.
              result.catch(() => undefined);
              throw failure(prop);
            }
          }
          return result;
        })();
      };
    },
  });
  return { backend, calls, release };
}
