import { EnvironmentProviders, inject, makeEnvironmentProviders } from '@angular/core';
import { DEBRIEF_RECORD_MENU_ITEMS, RecordMenuContribution } from '../context/record-actions';
import { DebriefInspectorSection, DebriefView } from '../extensions/tokens';
import { UI_STATE_CONTRIBUTIONS } from '../lifecycle/ui-state';
import { anchors, NoteActions } from './note-actions';
import { NOTE_IMAGE_VIEW, NOTES_VIEW } from './notes.ids';
import { NOTES_COMMANDS_CONTRIBUTIONS } from './notes.contributions';
import { NotesInspectorSection } from './notes-inspector-section';
import { NotesPersistence } from './notes-persistence';
import { contributeFrom, debriefContributionProviders } from '../extensions/contributions';

export function notesViews(): DebriefView[] {
  return [
    { id: NOTES_VIEW, category: 'Analysis', title: $localize`:@@notes.view.notes:Notes`, icon: 'note', defaultPosition: 'right', layout: { area: 'side', min: { width: 320, height: 240 }, pref: { width: 372, height: 400 }, priority: 50 }, component: () => import('./notes-panel').then((m) => m.NotesPanel) },
    { id: NOTE_IMAGE_VIEW, category: 'Analysis', title: $localize`:@@notes.view.image:Image notes`, icon: 'image', defaultPosition: 'center', layout: { area: 'document', min: { width: 420, height: 240 }, pref: { width: 640, height: 400 }, priority: 55 }, component: () => import('./image/note-image-view').then((m) => m.NoteImageView) },
  ];
}

/** "Add note…" in the record context menu (the grid's rows, the inspector's "More actions…"). */
export function notesRecordMenu(actions: NoteActions): RecordMenuContribution {
  return (record) => [{ group: 'mark', shortcut: 'Mod+D', label: $localize`:@@notes.menu.add:Add note…`, run: () => void actions.write(anchors.event(record)) }];
}

const INSPECTOR_SECTION: DebriefInspectorSection = { id: 'notes', title: $localize`:@@notes.inspector.title:Notes`, order: 250, component: NotesInspectorSection };

/** One line in `app.config.ts`: the Notes panel and image view, commands, menus, the inspector section, saving. */
export function provideNotes(): EnvironmentProviders {
  return makeEnvironmentProviders([
    ...contributeFrom('views', notesViews),
    // Stubs only: the commands' code loads when the window is idle or one runs.
    ...NOTES_COMMANDS_CONTRIBUTIONS,
    { provide: DEBRIEF_RECORD_MENU_ITEMS, multi: true, useFactory: () => notesRecordMenu(inject(NoteActions)) },
    ...debriefContributionProviders({ inspectorSections: [INSPECTOR_SECTION] }),
    { provide: UI_STATE_CONTRIBUTIONS, multi: true, useFactory: () => [inject(NotesPersistence).contribution] },
  ]);
}
