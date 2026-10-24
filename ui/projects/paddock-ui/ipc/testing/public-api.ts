/**
 * `@decryp7/paddock-ui/ipc/testing`: the mock kit for browser development and tests (plan X28). `parseMockOptions` reads the
 * URL switches (`?mock=&latency=&fail=&hold=`), `formatMockOptions` writes them for Playwright specs, and `withMockOptions`
 * plays them on a mock backend (latency, failures, held answers, recorded calls). The mock backend itself stays in the app
 * and is imported lazily by `provideBackend`, so none of this reaches a release bundle.
 */
export { parseMockOptions, formatMockOptions, MAX_MOCK_LATENCY_MS, type MockOptions } from './src/mock-options';
export { withMockOptions, type MockedBackend, type MockPorts, type RecordedCall } from './src/mock-calls';
