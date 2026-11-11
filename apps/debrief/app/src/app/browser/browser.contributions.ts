import { lazyCommandFactory, type CommandStub } from '../extensions/lazy-registration';

// The registration file of the Web search command: stubs only; `browser.commands.ts` and the store it reads load when the
// window is idle or the command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

export const BROWSER_COMMAND_STUBS: readonly CommandStub[] = [
  {
    id: 'browser.searchSelected',
    title: $localize`:@@browser.cmd.searchSelected:Web: Search the web for the selected event…`,
    category: $localize`:Command palette category@@browser.category:Web`,
  },
];

export const BROWSER_COMMANDS_CONTRIBUTIONS = lazyCommandFactory(() => import('./browser.commands').then((m) => m.browserCommandsFactory), BROWSER_COMMAND_STUBS);
