import { inject } from '@angular/core';
import { DebriefCommand } from '../../extensions/tokens';
import { PLAYBOOKS_VIEW } from './playbooks.ids';
import { PlaybookStore } from './playbook.store';

export function playbooksCommands(store: InstanceType<typeof PlaybookStore>): DebriefCommand[] {
  const category = $localize`:Command palette category@@playbooks.category:View`;
  return [
    { id: 'playbooks.show', title: $localize`:@@playbooks.cmd.show:View: Playbooks`, category, view: PLAYBOOKS_VIEW, run: (ctx) => ctx.showView(PLAYBOOKS_VIEW) },
    {
      id: 'playbooks.new',
      title: $localize`:@@playbooks.cmd.new:Playbooks: New playbook`,
      category,
      run: (ctx) => {
        ctx.showView(PLAYBOOKS_VIEW);
        store.startNew();
      },
    },
  ];
}

/** The commands in the root injector (`lazyCommandFactory` loads this module when the window is idle or a command runs). */
export const playbooksCommandsFactory = (): DebriefCommand[] => playbooksCommands(inject(PlaybookStore));
