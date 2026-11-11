import { isMac } from '../extensions/keybinding';
import { lazyCommands, type CommandStub } from '../extensions/lazy-registration';
import { NEW_WINDOW_KEYBINDING, QUIT_COMMAND, QUIT_KEYBINDING } from './windowing.ids';

// The registration file of the window commands: stubs only; `window-commands.ts` and the windowing service load when the
// window is idle or a command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

const CATEGORY = $localize`:Command category@@windowing.commands.category:Window`;

export const WINDOWING_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'window.new', title: $localize`:@@windowing.commands.new:New window`, category: CATEGORY, keybinding: NEW_WINDOW_KEYBINDING },
  { id: 'window.close', title: $localize`:@@windowing.commands.close:Close window`, category: CATEGORY },
  { id: 'window.next', gated: true, title: $localize`:@@windowing.commands.next:Switch to next window`, category: CATEGORY },
  {
    id: QUIT_COMMAND,
    title: isMac() ? $localize`:@@windowing.commands.quitMac:Quit Debrief` : $localize`:@@windowing.commands.exit:Exit`,
    category: CATEGORY,
    ...(isMac() ? { keybinding: QUIT_KEYBINDING } : {}),
  },
];

export const WINDOWING_COMMANDS_CONTRIBUTIONS = lazyCommands(() => import('./window-commands').then((m) => m.WindowCommands), WINDOWING_COMMAND_STUBS);
