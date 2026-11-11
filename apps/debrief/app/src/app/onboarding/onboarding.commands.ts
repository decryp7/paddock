import { inject } from '@angular/core';
import { DebriefCommand } from '../extensions/tokens';
import { DemoService } from './demo.service';
import { TourService } from './tour.service';

/** Help → Try the demo / Take the tour (`lazyCommandFactory` loads this module when the window is idle or a command runs). */
export function onboardingCommands(): DebriefCommand[] {
  const demo = inject(DemoService);
  const tour = inject(TourService);
  return [
    { id: 'help.demo', title: $localize`:@@onboarding.command.demo:Try the demo`, category: $localize`:@@onboarding.command.category:Help`, run: async () => void (await demo.start()) },
    { id: 'help.tour', title: $localize`:@@onboarding.command.tour:Take the tour`, category: $localize`:@@onboarding.command.category:Help`, when: (ctx) => !!ctx.summary?.sources.length, run: () => tour.start() },
  ];
}
