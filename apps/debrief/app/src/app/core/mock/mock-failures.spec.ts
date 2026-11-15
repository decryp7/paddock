import { tinyBackend, query } from '../../../testing/helpers';
import { withMockFailures } from './mock-failures';

describe('withMockFailures', () => {
  it('rejects the first N calls of a named command with a plain message, then answers', async () => {
    const raw = tinyBackend();
    const ui = withMockFailures(raw, { latency: false, fail: { queryFacet: 2 } });
    const req = { query: query(), field: 'level', limit: 5 };
    await expect(ui.queryFacet(req)).rejects.toBe('Simulated queryFacet failure');
    await expect(ui.queryFacet(req)).rejects.toBe('Simulated queryFacet failure');
    expect((await ui.queryFacet(req)).field).toBe('level');
  });

  it('leaves other commands alone and never touches the raw mock', async () => {
    const raw = tinyBackend();
    const ui = withMockFailures(raw, { latency: false, fail: { workspaceSummary: 1 } });
    expect((await ui.queryPage({ query: query(), offset: 0, limit: 3, descending: false })).rows).toHaveLength(3);
    expect((await raw.workspaceSummary()).total).toBeGreaterThan(0);
    await expect(ui.workspaceSummary()).rejects.toBe('Simulated workspaceSummary failure');
    expect((await ui.workspaceSummary()).total).toBeGreaterThan(0);
  });
});
