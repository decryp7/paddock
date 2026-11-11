import { lazyCommandFactory, type CommandStub } from '../extensions/lazy-registration';
import { NOTE_IMAGE_VIEW, NOTES_VIEW } from './notes.ids';

// The registration file of the Notes commands: stubs only; `notes.commands.ts` and the stores it reads load when the
// window is idle or a command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

const CATEGORY = $localize`:Command palette category|@@notes.category:Notes`;

export const NOTES_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'notes.show', title: $localize`:@@notes.cmd.show:Show notes`, category: CATEGORY, view: NOTES_VIEW },
  { id: 'notes.addSelected', title: $localize`:@@notes.cmd.addSelected:Add note to the selected event…`, category: CATEGORY, keybinding: 'Mod+D' },
  { id: 'notes.addRange', title: $localize`:@@notes.cmd.addRange:Add note to the time range…`, category: CATEGORY },
  { id: 'notes.addProject', title: $localize`:@@notes.cmd.addProject:Add note to the project…`, category: CATEGORY },
  { id: 'notes.image', title: $localize`:@@notes.cmd.image:Mark up an image…`, category: CATEGORY, view: NOTE_IMAGE_VIEW },
];

export const NOTES_COMMANDS_CONTRIBUTIONS = lazyCommandFactory(() => import('./notes.commands').then((m) => m.notesCommandsFactory), NOTES_COMMAND_STUBS);
