import { inject } from '@angular/core';
import { WorkspaceStore } from '../core/workspace.store';
import { DebriefCommand } from '../extensions/tokens';
import { ClockDialogService } from './clock-section';
import { ClockSkewStore } from './clock-skew.store';

/** Palette commands for clock-skew correction. */
export function clockCommands(): DebriefCommand[] {
  const store = inject(WorkspaceStore);
  const skew = inject(ClockSkewStore);
  const dialog = inject(ClockDialogService);
  return [
    {
      id: 'skew.adjust',
      title: $localize`:@@skew.plugin.adjust:Adjust clock of the selected event's source…`,
      category: $localize`:@@skew.plugin.category:Events`,
      when: () => store.selected() !== null,
      run: () => {
        const r = store.selected();
        if (r) dialog.open(r.source, r.sourceName);
      },
    },
    {
      id: 'skew.reference',
      title: $localize`:@@skew.plugin.reference:Use selected event as clock reference`,
      category: $localize`:@@skew.plugin.category:Events`,
      when: () => store.selected()?.ts != null,
      run: () => void skew.setReference(store.selected()),
    },
  ];
}
