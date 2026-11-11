import { inject, Injectable } from '@angular/core';
import { RecordActions } from '../context/record-actions';
import { WorkspaceStore } from '../core/workspace.store';
import { DebriefCommand } from '../extensions/tokens';

/** Palette commands of the markers feature. */
@Injectable({ providedIn: 'root' })
export class MarkersCommands {
  private readonly store = inject(WorkspaceStore);
  private readonly actions = inject(RecordActions);

  readonly commands: DebriefCommand[] = [
    { id: 'markers.manage', title: $localize`:@@markers.cmd.manage.title:Manage markers`, category: $localize`:Command palette category|@@markers.cmd.category:Markers`, view: 'markers', run: (ctx) => ctx.showView('markers') },
    {
      id: 'markers.highlight',
      title: $localize`:@@markers.cmd.highlight.title:Highlight lines…`,
      category: $localize`:Command palette category|@@markers.cmd.category:Markers`,
      run: (ctx) => {
        const record = this.store.selected();
        if (!record) return ctx.notifyError($localize`:@@markers.cmd.highlight.selectFirst:Select an event in the grid first, then highlight lines like it.`);
        this.actions.highlight(record, document.getSelection()?.toString());
      },
    },
    {
      id: 'markers.infer',
      title: $localize`:@@markers.cmd.infer.title:Infer pattern from selected lines…`,
      category: $localize`:Command palette category|@@markers.cmd.category:Markers`,
      // Every line selected in the grid (shift/ctrl+click), else the event selected elsewhere.
      run: (ctx) => {
        if (!this.actions.inferSelected()) ctx.notifyError($localize`:@@markers.cmd.infer.selectFirst:Select one or more events in the grid first.`);
      },
    },
  ];
}
