import { lazyCommands, type CommandStub } from '../extensions/lazy-registration';

// The registration file of the Go back / Go forward commands: stubs only; `navigation-commands.ts` and the history it
// reads load when the window is idle or a command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

const CATEGORY = $localize`:Command category@@navigation.category:Go`;

export const NAVIGATION_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'nav.back', gated: true, title: $localize`:@@navigation.cmd.back:Go back`, category: CATEGORY, keybinding: 'Alt+Left' },
  { id: 'nav.forward', gated: true, title: $localize`:@@navigation.cmd.forward:Go forward`, category: CATEGORY, keybinding: 'Alt+Right' },
];

export const NAVIGATION_COMMANDS_CONTRIBUTIONS = lazyCommands(() => import('./navigation-commands').then((m) => m.NavigationCommands), NAVIGATION_COMMAND_STUBS);
