// Extra Vitest settings for `ng test` (angular.json "runnerConfig"); the Angular builder supplies the rest.
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    // CI runs with coverage on small runners, which slows the multi-step store and view tests several times over.
    testTimeout: 20_000,
    // A spy (vi.spyOn on a prototype or a service) is put back after every test, so a patch cannot leak into later specs.
    restoreMocks: true,
  },
});
