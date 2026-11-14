import type { Page, PageRequest } from '../api.types';
import type { DebriefBackend } from '../backend';

/** The demo backend's own page read (`MockBackend.readMockPage`), which never takes a simulated `queryPage` failure. */
interface MockPageReader {
  readMockPage(request: PageRequest): Promise<Page>;
}

/**
 * Reads a page for another mock's answer (an analysis, a share, a search): through the demo backend's own read when it has
 * one, else (a test's small backend) through `queryPage`. `?mockFail=queryPage:N` fails the UI's first N calls of that
 * command; the engine computes these answers itself, so a mock must not take those failures from the panel under test.
 */
export function mockRecordsPage(backend: DebriefBackend, request: PageRequest): Promise<Page> {
  const own = (backend as Partial<MockPageReader>).readMockPage;
  return typeof own === 'function' ? own.call(backend, request) : backend.queryPage(request);
}
