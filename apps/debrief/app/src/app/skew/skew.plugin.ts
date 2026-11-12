import { EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import { provideSkewBackend } from '../core/skew-backend';
import { DebriefInspectorSection } from '../extensions/tokens';
import { ClockSection } from './clock-section';
import { debriefContributionProviders } from '../extensions/contributions';
import { SKEW_COMMANDS_CONTRIBUTIONS } from './skew.contributions';

/** The inspector section offering "Use as clock reference" and alignment (timed events only). */
export const CLOCK_SECTION: DebriefInspectorSection = { id: 'clock', title: $localize`:@@skew.plugin.section:Clock`, order: 150, component: ClockSection, when: (r) => r.ts !== null };

/**
 * Clock-skew correction: per-source offsets (Sources › Adjust clock…), the inspector's clock section and
 * palette commands. One line in `app.config.ts`.
 */
export function provideClockSkew(): EnvironmentProviders {
  return makeEnvironmentProviders([
    provideSkewBackend(),
    ...debriefContributionProviders({ inspectorSections: [CLOCK_SECTION] }),
    // Stubs only: the commands' code loads when the window is idle or one runs.
    ...SKEW_COMMANDS_CONTRIBUTIONS,
  ]);
}
