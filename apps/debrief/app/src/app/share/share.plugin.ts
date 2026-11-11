import { EnvironmentProviders, inject, makeEnvironmentProviders } from '@angular/core';
import { AnalysisBackend } from '../core/analysis-backend';
import { DebriefBackend } from '../core/backend';
import { reportBackendFailures } from '../core/failure-reporter';
import { Logger, withFailureLogging } from '../core/logger';
import { isTauri } from '../core/platform';
import { DEBRIEF_RECORD_MENU_ITEMS } from '../context/record-actions';
import { SNAPSHOT_TEXT_FILTER } from '../diagnostics/error-snapshot.service';
import { reportMenuItems } from './report/report-menu';
import { ShareBackend, TauriShareBackend } from './share-backend';
import { SHARE_CONTRIBUTIONS } from './share.contributions';
import { snapshotRedaction } from './snapshot-redaction';
import { browserMocks } from '../core/mock-registry';

/** The engine-backed share backend inside Tauri, the mock (over the mock workspace) in a browser. */
export function createShareBackend(): ShareBackend {
  const backend: ShareBackend = isTauri() ? new TauriShareBackend() : new (browserMocks().MockShareBackend)(inject(DebriefBackend), inject(AnalysisBackend));
  return withFailureLogging<ShareBackend>(backend, inject(Logger), { onFailure: reportBackendFailures() });
}

/** One line in `app.config.ts`: redaction, dashboards, reports, images and bundles. */
export function provideShare(): EnvironmentProviders {
  return makeEnvironmentProviders([
    { provide: ShareBackend, useFactory: createShareBackend },
    { provide: SNAPSHOT_TEXT_FILTER, useFactory: snapshotRedaction },
    { provide: DEBRIEF_RECORD_MENU_ITEMS, multi: true, useFactory: reportMenuItems },
    // Views and commands: static data; the command classes load when the window is idle or a command runs.
    ...SHARE_CONTRIBUTIONS,
  ]);
}
