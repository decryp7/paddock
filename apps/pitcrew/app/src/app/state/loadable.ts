import type { CommandError } from '../core/api.types';
import { toCommandError } from '../core/command-error';

/** One request's result as panels show it: the last good value, whether a request runs, and its error. */
export interface Loadable<T> {
  value: T | null;
  loading: boolean;
  error: CommandError | null;
}

export function idle<T>(): Loadable<T> {
  return { value: null, loading: false, error: null };
}

/** Marks a request as running, keeping the last good value (so a refresh does not blank the panel). */
export function pending<T>(l: Loadable<T> | undefined): Loadable<T> {
  return { value: l?.value ?? null, loading: true, error: null };
}

/** Runs `fn` and returns the settled state; a failure keeps the previous value next to the error. */
export async function settle<T>(previous: Loadable<T> | undefined, fn: () => Promise<T>): Promise<Loadable<T>> {
  try {
    return { value: await fn(), loading: false, error: null };
  } catch (e) {
    return { value: previous?.value ?? null, loading: false, error: toCommandError(e) };
  }
}
