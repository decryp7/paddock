import { inject } from '@angular/core';
import { WorkspaceStore } from '../core/workspace.store';
import { DebriefCommand } from '../extensions/tokens';
import { anchors, NoteActions } from './note-actions';
import { NOTE_IMAGE_VIEW, NOTES_VIEW } from './notes.ids';

const CATEGORY = () => $localize`:Command palette category|@@notes.category:Notes`;

export function notesCommands(actions: NoteActions, workspace: WorkspaceStore): DebriefCommand[] {
  return [
    { id: 'notes.show', title: $localize`:@@notes.cmd.show:Show notes`, category: CATEGORY(), view: NOTES_VIEW, run: (ctx) => ctx.showView(NOTES_VIEW) },
    {
      id: 'notes.addSelected',
      title: $localize`:@@notes.cmd.addSelected:Add note to the selected event…`,
      category: CATEGORY(),
      keybinding: 'Mod+D',
      when: () => !!workspace.selected(),
      run: () => void actions.writeOnSelected(),
    },
    { id: 'notes.addRange', title: $localize`:@@notes.cmd.addRange:Add note to the time range…`, category: CATEGORY(), when: () => workspace.hasRange(), run: () => void actions.writeOnRange() },
    { id: 'notes.addProject', title: $localize`:@@notes.cmd.addProject:Add note to the project…`, category: CATEGORY(), run: () => void actions.write(anchors.project()) },
    { id: 'notes.image', title: $localize`:@@notes.cmd.image:Mark up an image…`, category: CATEGORY(), view: NOTE_IMAGE_VIEW, run: (ctx) => ctx.showView(NOTE_IMAGE_VIEW) },
  ];
}

/** The commands in the root injector (`lazyCommandFactory` loads this module when the window is idle or a command runs). */
export const notesCommandsFactory = (): DebriefCommand[] => notesCommands(inject(NoteActions), inject(WorkspaceStore));
