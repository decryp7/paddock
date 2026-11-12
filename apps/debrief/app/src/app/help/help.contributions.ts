import { lazyCommands, type CommandStub } from '../extensions/lazy-registration';
import { MANAGE_PARSERS_COMMAND } from '../settings/parsers/parsers.types';
import { EDIT_SHORTCUTS_COMMAND } from '../settings/keyboard/keyboard.types';
import { SHORTCUTS_KEY } from './help.ids';

// The registration file of the Help commands: stubs only; `help-commands.ts` and the services it injects load when the
// window is idle or a command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

const CATEGORY = $localize`:@@help.command.category:Help`;

export const HELP_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'help.shortcuts', title: $localize`:@@help.command.shortcuts:Keyboard shortcuts`, category: CATEGORY, keybinding: SHORTCUTS_KEY },
  { id: 'help.whatsNew', title: $localize`:@@help.command.whatsNew:What's new`, category: CATEGORY },
  { id: EDIT_SHORTCUTS_COMMAND, title: $localize`:@@help.command.editShortcuts:Edit keyboard shortcuts…`, category: CATEGORY },
  { id: MANAGE_PARSERS_COMMAND, title: $localize`:@@help.command.manageParsers:Manage parsers…`, category: $localize`:@@help.command.profileCategory:Profile` },
];

export const HELP_COMMANDS_CONTRIBUTIONS = lazyCommands(() => import('./help-commands').then((m) => m.HelpCommands), HELP_COMMAND_STUBS);
