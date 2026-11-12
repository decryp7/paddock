import { lazyCommandFactory, type CommandStub } from '../extensions/lazy-registration';

// The registration file of the demo and tour commands: stubs only; `onboarding.commands.ts` and the services it injects
// load when the window is idle or a command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

const CATEGORY = $localize`:@@onboarding.command.category:Help`;

export const ONBOARDING_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'help.demo', title: $localize`:@@onboarding.command.demo:Try the demo`, category: CATEGORY },
  { id: 'help.tour', gated: true, title: $localize`:@@onboarding.command.tour:Take the tour`, category: CATEGORY },
];

export const ONBOARDING_COMMANDS_CONTRIBUTIONS = lazyCommandFactory(() => import('./onboarding.commands').then((m) => m.onboardingCommands), ONBOARDING_COMMAND_STUBS);
