import { Type } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import type { DebriefCommand } from '../extensions/tokens';
import { stubMismatches } from '../../testing/contributions';
import { MemoryLogger } from '../../testing/helpers';
import { AnalysisBackend } from '../core/analysis-backend';
import { DebriefBackend } from '../core/backend';
import { Logger } from '../core/logger';
import { MockBackend } from '../core/mock/mock-backend';
import { MockAnalysisBackend, recordsFrom } from '../analysis/mock/mock-analysis-backend';
import { BundleCommands } from './bundle/bundle.commands';
import { DashboardCommands } from './dashboard/dashboard.commands';
import { RedactedCopyCommands } from './redacted-copy/redacted-copy-commands';
import { RedactionCommands } from './redaction/redaction.commands';
import { ReportCommands } from './report/report.commands';
import { VisualCommands } from './visuals/visual.commands';
import {
  BUNDLE_COMMAND_STUBS,
  DASHBOARD_COMMAND_STUBS,
  REDACTED_COPY_COMMAND_STUBS,
  REDACTION_COMMAND_STUBS,
  REPORT_COMMAND_STUBS,
  VISUAL_COMMAND_STUBS,
} from './share.contributions';
import { provideShare } from './share.plugin';

describe('share.contributions', () => {
  beforeEach(() => {
    const debrief = new MockBackend({ latency: false });
    TestBed.configureTestingModule({
      providers: [
        { provide: Logger, useClass: MemoryLogger },
        { provide: DebriefBackend, useValue: debrief },
        { provide: AnalysisBackend, useValue: new MockAnalysisBackend(recordsFrom(debrief)) },
        provideShare(),
      ],
    });
  });

  it('lists every command of each class, in order, with its title, category, key and view', () => {
    const pairs = [
      [REDACTION_COMMAND_STUBS, RedactionCommands],
      [REPORT_COMMAND_STUBS, ReportCommands],
      [DASHBOARD_COMMAND_STUBS, DashboardCommands],
      [BUNDLE_COMMAND_STUBS, BundleCommands],
      [VISUAL_COMMAND_STUBS, VisualCommands],
      [REDACTED_COPY_COMMAND_STUBS, RedactedCopyCommands],
    ] as const;
    for (const [stubs, cls] of pairs) {
      const commands = TestBed.inject(cls as Type<{ commands: readonly DebriefCommand[] }>).commands;
      expect(stubMismatches(stubs, commands), cls.name).toEqual([]);
    }
  });
});
