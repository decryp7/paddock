import { inject, Injectable } from '@angular/core';
import { DebriefCommand } from '../extensions/tokens';
import { STORE_VIEW } from './store-pane-model';
import { StorePaneStore } from './store-pane.store';

export function storePaneCommands(pane: InstanceType<typeof StorePaneStore>): DebriefCommand[] {
  const category = $localize`:Command palette category@@store.category:View`;
  return [
    {
      id: 'store.show',
      title: $localize`:@@store.cmd.show:Open Store`,
      category,
      run: (ctx) => {
        void pane.show(pane.tab() === 'sql' ? 'overview' : pane.tab());
        ctx.showView(STORE_VIEW);
      },
    },
    {
      id: 'store.sql',
      title: $localize`:@@store.cmd.sql:Open SQL console`,
      category,
      run: (ctx) => {
        void pane.show('sql');
        ctx.showView(STORE_VIEW);
      },
    },
  ];
}

/** The Store pane's commands, loaded with the pane's store on first use. */
@Injectable({ providedIn: 'root' })
export class StorePaneCommands {
  readonly commands = storePaneCommands(inject(StorePaneStore));
}
