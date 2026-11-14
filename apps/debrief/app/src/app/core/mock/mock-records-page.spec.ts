import { MemoryLogger, query, tinyBackend, tinyDataset } from '../../../testing/helpers';
import type { DebriefBackend } from '../backend';
import { withFailureLogging } from '../logger';
import { MockBackend } from './mock-backend';
import { mockRecordsPage } from './mock-records-page';

describe('mockRecordsPage', () => {
  const request = { query: query(), offset: 0, limit: 3, descending: false };

  it("reads through the app's logging proxy without taking a simulated queryPage failure", async () => {
    // As the browser demo wires it: the MockBackend behind the failure-logging proxy (provideBackend).
    const backend: DebriefBackend = withFailureLogging(new MockBackend({ latency: false, dataset: tinyDataset, fail: { queryPage: 1 } }), new MemoryLogger());
    expect((await mockRecordsPage(backend, request)).rows).toHaveLength(3);
    // The UI's first call still gets the failure it was given.
    await expect(backend.queryPage(request)).rejects.toThrow('Simulated queryPage failure');
    expect((await backend.queryPage(request)).rows).toHaveLength(3);
  });

  it('falls back to queryPage for a backend without the mock read', async () => {
    const backend = tinyBackend();
    const own = { queryPage: (r: typeof request) => backend.queryPage(r) } as unknown as DebriefBackend;
    expect((await mockRecordsPage(own, request)).rows).toHaveLength(3);
  });
});
