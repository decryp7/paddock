import { inject, Provider } from '@angular/core';
import { MOCK_WORKSPACE } from '../../core/mock-workspace';
import { MockAnalysisBackend, recordsFrom } from '../../analysis/mock/mock-analysis-backend';
import { MockShareBackend } from '../mock/mock-share-backend';
import { ShareBackend } from '../share-backend';

/** The mock share backend over the test's DebriefBackend (unit tests of share features and their hosts). */
export function provideMockShare(): Provider[] {
  return [{ provide: ShareBackend, useFactory: () => new MockShareBackend(inject(MOCK_WORKSPACE), new MockAnalysisBackend(recordsFrom(inject(MOCK_WORKSPACE)))) }];
}
