import { lazyCommands, type CommandStub } from '../extensions/lazy-registration';

// The registration file of the built-in commands whose list is static: focus regions, Edit ▸ Copy / Find and Paste as
// log. Stubs only; the classes and the stores they read load when the window is idle or a command runs. The
// "Open <view>" commands (`BuiltinCommands`) stay eager: one per registered view. `extensions/lazy-commands.spec.ts` keeps
// the stubs equal to the code.

const VIEW = $localize`:@@shell.regions.category:View`;
const EDIT = $localize`:@@shell.edit.category:Edit`;

export const REGION_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'view.focusNextRegion', title: $localize`:@@shell.regions.next:Focus next region`, category: VIEW, keybinding: 'F6' },
  { id: 'view.focusPreviousRegion', title: $localize`:@@shell.regions.previous:Focus previous region`, category: VIEW, keybinding: 'Shift+F6' },
  { id: 'view.focusSidebar', title: $localize`:@@shell.regions.sidebar:Focus side bar`, category: VIEW, keybinding: 'Mod+0' },
  { id: 'view.focusEvents', title: $localize`:@@shell.regions.events:Focus events grid`, category: VIEW, keybinding: 'Mod+Shift+G' },
];

export const EDIT_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'edit.copy', title: $localize`:@@shell.edit.copy:Copy`, category: EDIT },
  { id: 'edit.copyJson', title: $localize`:@@shell.edit.copyJson:Copy as JSON`, category: EDIT },
  { id: 'edit.find', title: $localize`:@@shell.edit.find:Find`, category: EDIT, keybinding: 'Mod+F' },
];

export const PASTE_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'edit.pasteLog', title: $localize`:@@sources.paste.command:Paste as log…`, category: EDIT, keybinding: 'Mod+Shift+V' },
];

export const BUILTIN_COMMANDS_CONTRIBUTIONS = [
  ...lazyCommands(() => import('../shell/regions').then((m) => m.RegionNavigator), REGION_COMMAND_STUBS),
  ...lazyCommands(() => import('../shell/edit-commands').then((m) => m.EditCommands), EDIT_COMMAND_STUBS),
  ...lazyCommands(() => import('../sources/paste/paste-commands').then((m) => m.PasteCommands), PASTE_COMMAND_STUBS),
];
