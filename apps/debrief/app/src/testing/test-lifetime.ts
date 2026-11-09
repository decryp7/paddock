/**
 * Which test is running, for helpers that resume after an await (tests only).
 *
 * Spec files share one page: the Angular builder runs Vitest with `isolate: false`. When a test times out, Vitest
 * reports it and moves on, but nothing stops its body: it resumes later, in the next test or the next file, and acts on
 * whatever TestBed is current then (CI run 7825: a timed-out test's `settle()` instantiated the next file's TestBed, and
 * that file's first `configureTestingModule` failed). `test-setup.ts` marks every test's start and end here; helpers
 * take the generation when called and stop with {@link assertSameTest} once it has moved on.
 */
let generation = 0;

/** Marks the start of a test (the shared `beforeEach` in `test-setup.ts`). */
export function testStarted(): void {
  generation++;
}

/** Marks the end of a test, passed or failed or timed out (the shared `afterEach` in `test-setup.ts`). */
export function testEnded(): void {
  generation++;
}

/** The current test's generation, to pass to {@link assertSameTest} after an await. */
export function currentTest(): number {
  return generation;
}

/** Throws when the test that took `taken` has ended, so its leftover body cannot act on the next test's state. */
export function assertSameTest(taken: number, helper: string): void {
  if (generation !== taken) {
    throw new Error(`${helper} outlived its test (it timed out or was not awaited); stopping before it touches the next test's TestBed`);
  }
}
